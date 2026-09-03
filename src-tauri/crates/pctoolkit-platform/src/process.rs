use std::collections::{HashMap, HashSet};
use std::process::Command;
use std::thread;
use std::time::Duration;

use serde::Serialize;
use sysinfo::{ProcessesToUpdate, System};

use crate::{PlatformError, PlatformResult};

/// Windows `CREATE_NO_WINDOW` — hide console for background tooling (powershell, net, etc.).
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub fn hide_console(cmd: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunningProcessGroup {
    pub image_name: String,
    pub process_count: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessCloseTargetResult {
    pub image_name: String,
    pub status: String,
    pub remaining_count: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessCloseBatchResult {
    pub targets: Vec<ProcessCloseTargetResult>,
}

/// Image names that Deep Cleanup is allowed to close (case-insensitive).
const CLOSE_ALLOWLIST: &[&str] = &[
    "chrome.exe",
    "msedge.exe",
    "firefox.exe",
    "brave.exe",
    "opera.exe",
    "opera_gx.exe",
    "vivaldi.exe",
    "chromium.exe",
    "discord.exe",
    "telegram.exe",
    "whatsapp.exe",
    "whatsapp.desktop.exe",
    "whatsapp.root.exe",
    "steam.exe",
    "steamwebhelper.exe",
    "ms-teams.exe",
    "teams.exe",
    "spotify.exe",
    "slack.exe",
    "notion.exe",
    "docker desktop.exe",
    "code.exe",
    "cursor.exe",
    "vscodium.exe",
    "windsurf.exe",
];

fn normalize_image(name: &str) -> String {
    let trimmed = name.trim();
    let with_exe = if trimmed.to_ascii_lowercase().ends_with(".exe") {
        trimmed.to_string()
    } else {
        format!("{trimmed}.exe")
    };
    with_exe.to_ascii_lowercase()
}

fn is_allowed(image: &str) -> bool {
    CLOSE_ALLOWLIST
        .iter()
        .any(|allowed| allowed.eq_ignore_ascii_case(image))
}

fn refresh_system() -> System {
    let mut sys = System::new();
    sys.refresh_processes(ProcessesToUpdate::All, true);
    sys
}

fn count_by_image(sys: &System, images: &HashSet<String>) -> HashMap<String, u32> {
    let mut counts: HashMap<String, u32> = HashMap::new();
    for process in sys.processes().values() {
        let name = normalize_image(&process.name().to_string_lossy());
        if images.contains(&name) {
            *counts.entry(name).or_default() += 1;
        }
    }
    counts
}

/// Probe which of the requested image names are currently running.
pub fn probe_running_processes(names: &[String]) -> PlatformResult<Vec<RunningProcessGroup>> {
    let images: HashSet<String> = names
        .iter()
        .map(|n| normalize_image(n))
        .filter(|n| is_allowed(n))
        .collect();
    if images.is_empty() {
        return Ok(Vec::new());
    }
    let sys = refresh_system();
    let counts = count_by_image(&sys, &images);
    let mut groups: Vec<_> = counts
        .into_iter()
        .filter(|(_, count)| *count > 0)
        .map(|(image_name, process_count)| RunningProcessGroup {
            image_name,
            process_count,
        })
        .collect();
    groups.sort_by(|a, b| a.image_name.cmp(&b.image_name));
    Ok(groups)
}

fn taskkill(image: &str, force: bool) -> PlatformResult<()> {
    let mut cmd = Command::new("taskkill");
    cmd.args(["/IM", image]);
    if force {
        cmd.arg("/F");
    }
    hide_console(&mut cmd);
    let status = cmd
        .status()
        .map_err(|e| PlatformError::OperationFailed(e.to_string()))?;
    // taskkill returns non-zero when the process is already gone — treat as ok.
    if status.success() || !force {
        return Ok(());
    }
    Ok(())
}

fn kill_via_sysinfo(sys: &System, image: &str) {
    let target = normalize_image(image);
    for process in sys.processes().values() {
        let name = normalize_image(&process.name().to_string_lossy());
        if name == target {
            let _ = process.kill();
        }
    }
}

/// Close matching processes. `force=false` tries a soft taskkill first; remaining
/// processes are reported so the UI can offer a force retry.
pub fn close_processes(names: &[String], force: bool) -> PlatformResult<ProcessCloseBatchResult> {
    let images: Vec<String> = names
        .iter()
        .map(|n| normalize_image(n))
        .filter(|n| is_allowed(n))
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    if images.is_empty() {
        return Ok(ProcessCloseBatchResult { targets: vec![] });
    }

    for image in &images {
        let _ = taskkill(image, force);
        if force {
            let sys = refresh_system();
            kill_via_sysinfo(&sys, image);
        }
    }

    // Give Windows a moment to tear down handles before re-probing.
    thread::sleep(Duration::from_millis(if force { 400 } else { 700 }));
    let sys = refresh_system();
    let image_set: HashSet<String> = images.iter().cloned().collect();
    let remaining = count_by_image(&sys, &image_set);

    let mut targets = Vec::new();
    for image in images {
        let left = *remaining.get(&image).unwrap_or(&0);
        let status = if left == 0 {
            "closed"
        } else if force {
            "failed"
        } else {
            "remaining"
        };
        targets.push(ProcessCloseTargetResult {
            image_name: image,
            status: status.into(),
            remaining_count: left,
        });
    }
    targets.sort_by(|a, b| a.image_name.cmp(&b.image_name));
    Ok(ProcessCloseBatchResult { targets })
}
