//! Windows Package Manager (`winget`) facts: list available upgrades and upgrade one package.
//!
//! winget is Microsoft's official package manager. We only drive its documented CLI: the listing
//! is read-only, and an upgrade runs one exact package id at a time with the vendor's own silent
//! switches (`--silent`). Installers that need elevation raise their own UAC prompt; we never
//! elevate silently. Every child process runs without a console window, with a timeout and capped
//! output. Errors carry typed reasons and exit codes only — never raw output, which can contain
//! private paths.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::hidden_child::{CappedReader, ChildJob, READER_GRACE};
use crate::process::hide_console;

// ------------------------------------------------------------------------------------------------
// Hidden process runner (shared with `disk_health`)

const OUTPUT_CAP: usize = 4 * 1024 * 1024;
const POLL: Duration = Duration::from_millis(50);

/// Why a hidden child process did not produce a usable exit status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RunFailure {
    NotFound,
    Timeout,
    Cancelled,
    SpawnFailed,
}

pub(crate) struct HiddenOutput {
    pub exit_code: Option<i32>,
    pub stdout: String,
}

/// Run `program` with no console window, stdin closed, stdout captured (capped) and stderr
/// discarded. `on_output` receives raw stdout chunks as they arrive (progress parsing).
/// On timeout / cancel the child's whole process tree (kill-on-close Job Object) is killed, and
/// the output reader is joined with a deadline so a leftover grandchild cannot hang the caller.
pub(crate) fn run_hidden(
    program: &Path,
    args: &[&str],
    timeout: Duration,
    should_cancel: &dyn Fn() -> bool,
    on_output: Option<std::sync::Arc<dyn Fn(&[u8]) + Send + Sync>>,
) -> Result<HiddenOutput, RunFailure> {
    let mut cmd = Command::new(program);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .env("NO_COLOR", "1");
    hide_console(&mut cmd);
    let mut child = cmd.spawn().map_err(|error| match error.kind() {
        std::io::ErrorKind::NotFound => RunFailure::NotFound,
        _ => RunFailure::SpawnFailed,
    })?;
    let job = ChildJob::attach(&child);
    let reader = child.stdout.take().map(|s| {
        let sink = on_output.clone();
        CappedReader::spawn(s, OUTPUT_CAP, move |chunk| {
            if let Some(sink) = &sink {
                sink(chunk);
            }
        })
    });

    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) => {}
            Err(_) => break Err(RunFailure::SpawnFailed),
        }
        if should_cancel() {
            break Err(RunFailure::Cancelled);
        }
        if started.elapsed() >= timeout {
            break Err(RunFailure::Timeout);
        }
        thread::sleep(POLL);
    };
    if status.is_err() {
        if let Some(job) = &job {
            job.terminate();
        }
        let _ = child.kill();
        let _ = child.wait();
    } else if let Some(job) = job {
        job.release();
    }
    let stdout = reader
        .map(|r| decode_output(&r.finish(READER_GRACE)))
        .unwrap_or_default();
    let status = status?;
    Ok(HiddenOutput { exit_code: status.code(), stdout })
}

/// Decode child output: UTF-16 LE when it starts with a BOM (some System32 tools), else UTF-8.
pub(crate) fn decode_output(bytes: &[u8]) -> String {
    if bytes.len() >= 2 && bytes[0] == 0xFF && bytes[1] == 0xFE {
        let wide: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        return String::from_utf16_lossy(&wide);
    }
    String::from_utf8_lossy(bytes).into_owned()
}

// ------------------------------------------------------------------------------------------------
// Public contract

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum WingetReason {
    /// winget (App Installer) is not installed / not on this machine.
    ToolUnavailable,
    Timeout,
    Cancelled,
    /// winget ran but its output had no recognizable upgrade table.
    InvalidOutput,
    /// winget exited with an error (for example a source could not be reached).
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WingetError {
    pub reason: WingetReason,
    pub exit_code: Option<i32>,
}

impl WingetError {
    fn new(reason: WingetReason) -> Self {
        Self { reason, exit_code: None }
    }
}

/// One row of `winget upgrade`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WingetUpgrade {
    pub name: String,
    pub id: String,
    /// winget shortened the id with `…`; it cannot be passed to `--id --exact`.
    pub id_truncated: bool,
    pub current_version: String,
    pub available_version: String,
    pub source: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum WingetUpgradeStatus {
    Upgraded,
    /// Installed, but Windows needs a restart to finish.
    RestartRequired,
    /// winget reports no applicable upgrade (already current or not upgradable this way).
    NotApplicable,
    /// The installer refused because the application is running.
    PackageInUse,
    /// The package id no longer matches an installed package.
    NotFound,
    Timeout,
    Cancelled,
    ToolUnavailable,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WingetUpgradeOutcome {
    pub status: WingetUpgradeStatus,
    pub exit_code: Option<i32>,
}

// Documented winget HRESULT exit codes (AppInstallerErrors.h).
const ERR_NO_APPLICATIONS_FOUND: u32 = 0x8A15_0014;
const ERR_UPDATE_NOT_APPLICABLE: u32 = 0x8A15_002B;
const ERR_INSTALL_PACKAGE_IN_USE: u32 = 0x8A15_0101;
const ERR_INSTALL_REBOOT_REQUIRED_TO_FINISH: u32 = 0x8A15_0109;
const ERR_INSTALL_REBOOT_REQUIRED_FOR_INSTALL: u32 = 0x8A15_010A;

fn exit_hresult(code: Option<i32>) -> Option<u32> {
    code.map(|c| c as u32)
}

/// Locate `winget.exe` without a shell: PATH first, then the per-user App Execution Alias.
pub fn winget_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            let candidate = dir.join("winget.exe");
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    let local = std::env::var_os("LOCALAPPDATA")?;
    let alias = PathBuf::from(local).join("Microsoft").join("WindowsApps").join("winget.exe");
    // App Execution Aliases are zero-byte reparse points; `exists` follows them poorly, so
    // check the link metadata itself.
    std::fs::symlink_metadata(&alias).ok().map(|_| alias)
}

/// Read-only `winget upgrade` listing (no `--include-unknown`).
pub fn winget_list_upgrades(
    timeout: Duration,
    should_cancel: &dyn Fn() -> bool,
) -> Result<Vec<WingetUpgrade>, WingetError> {
    let exe = winget_path().ok_or_else(|| WingetError::new(WingetReason::ToolUnavailable))?;
    let output = run_hidden(
        &exe,
        &["upgrade", "--accept-source-agreements", "--disable-interactivity"],
        timeout,
        should_cancel,
        None,
    )
    .map_err(|failure| WingetError::new(run_failure_reason(failure)))?;
    interpret_listing(&output.stdout, output.exit_code)
}

fn run_failure_reason(failure: RunFailure) -> WingetReason {
    match failure {
        RunFailure::NotFound => WingetReason::ToolUnavailable,
        RunFailure::Timeout => WingetReason::Timeout,
        RunFailure::Cancelled => WingetReason::Cancelled,
        RunFailure::SpawnFailed => WingetReason::Failed,
    }
}

fn interpret_listing(stdout: &str, exit_code: Option<i32>) -> Result<Vec<WingetUpgrade>, WingetError> {
    if let Some(rows) = parse_upgrade_table(stdout) {
        // A parsed table wins even when one source failed (partial results are still real rows).
        return Ok(rows);
    }
    match exit_hresult(exit_code) {
        Some(0) | Some(ERR_UPDATE_NOT_APPLICABLE) | Some(ERR_NO_APPLICATIONS_FOUND) => Ok(Vec::new()),
        _ => Err(WingetError { reason: WingetReason::Failed, exit_code }),
    }
}

/// True when `id` looks like a winget package id we may pass as one argument.
pub fn is_valid_winget_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 256
        && !id.starts_with('-')
        && id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '+'))
}

/// Upgrade one package by exact id, silently, with agreements accepted. Blocks until winget
/// exits or `timeout` elapses (then winget is stopped and `Timeout` is reported).
pub fn winget_upgrade_package(
    id: &str,
    timeout: Duration,
    should_cancel: &dyn Fn() -> bool,
) -> WingetUpgradeOutcome {
    if !is_valid_winget_id(id) {
        return WingetUpgradeOutcome { status: WingetUpgradeStatus::NotFound, exit_code: None };
    }
    let Some(exe) = winget_path() else {
        return WingetUpgradeOutcome { status: WingetUpgradeStatus::ToolUnavailable, exit_code: None };
    };
    let result = run_hidden(
        &exe,
        &[
            "upgrade",
            "--id",
            id,
            "--exact",
            "--silent",
            "--accept-package-agreements",
            "--accept-source-agreements",
            "--disable-interactivity",
        ],
        timeout,
        should_cancel,
        None,
    );
    match result {
        Ok(output) => WingetUpgradeOutcome {
            status: upgrade_status(output.exit_code),
            exit_code: output.exit_code,
        },
        Err(failure) => WingetUpgradeOutcome {
            status: match failure {
                RunFailure::NotFound => WingetUpgradeStatus::ToolUnavailable,
                RunFailure::Timeout => WingetUpgradeStatus::Timeout,
                RunFailure::Cancelled => WingetUpgradeStatus::Cancelled,
                RunFailure::SpawnFailed => WingetUpgradeStatus::Failed,
            },
            exit_code: None,
        },
    }
}

fn upgrade_status(exit_code: Option<i32>) -> WingetUpgradeStatus {
    match exit_hresult(exit_code) {
        Some(0) => WingetUpgradeStatus::Upgraded,
        Some(ERR_INSTALL_REBOOT_REQUIRED_TO_FINISH) | Some(ERR_INSTALL_REBOOT_REQUIRED_FOR_INSTALL) => {
            WingetUpgradeStatus::RestartRequired
        }
        Some(ERR_UPDATE_NOT_APPLICABLE) => WingetUpgradeStatus::NotApplicable,
        Some(ERR_INSTALL_PACKAGE_IN_USE) => WingetUpgradeStatus::PackageInUse,
        Some(ERR_NO_APPLICATIONS_FOUND) => WingetUpgradeStatus::NotFound,
        _ => WingetUpgradeStatus::Failed,
    }
}

// ------------------------------------------------------------------------------------------------
// Table parser

/// Visible lines of console output: progress spinners / bars redraw with `\r`, so keep only the
/// text after the last carriage return of each line.
fn visible_lines(stdout: &str) -> Vec<String> {
    stdout
        .split('\n')
        .map(|raw| {
            let line = raw.trim_end_matches('\r');
            let visible = line.rsplit('\r').next().unwrap_or(line);
            visible.trim_end().to_string()
        })
        .collect()
}

fn is_rule_line(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.len() >= 10 && trimmed.chars().all(|c| c == '-')
}

/// Whitespace-separated tokens with their byte offsets.
fn tokens(line: &str) -> Vec<(usize, &str)> {
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    for (i, c) in line.char_indices() {
        if c.is_whitespace() {
            if let Some(s) = start.take() {
                out.push((s, &line[s..i]));
            }
        } else if start.is_none() {
            start = Some(i);
        }
    }
    if let Some(s) = start {
        out.push((s, &line[s..]));
    }
    out
}

/// Header column count. Column names are localized; most locales use one word per column. If a
/// locale uses multi-word names, columns are still separated by at least two spaces.
fn header_columns(header: &str) -> Option<(usize, usize)> {
    let single = tokens(header);
    if matches!(single.len(), 4 | 5) {
        let last = single.last().map(|(o, _)| header[..*o].chars().count())?;
        return Some((single.len(), last));
    }
    let wide: Vec<&str> = header.split("  ").map(str::trim).filter(|s| !s.is_empty()).collect();
    if matches!(wide.len(), 4 | 5) {
        let last_name = wide.last()?;
        let offset = header.rfind(last_name)?;
        return Some((wide.len(), header[..offset].chars().count()));
    }
    None
}

/// Parse one data row from the right: `[Source] Available Version Id`, the rest is the name.
/// Ids never contain spaces; versions may be prefixed with `<` / `>` (e.g. `< 3.14.7`).
fn parse_row(line: &str, has_source: bool) -> Option<WingetUpgrade> {
    let mut toks = tokens(line);
    let source = if has_source { Some(toks.pop()?.1.to_string()) } else { None };
    let available = toks.pop()?.1.to_string();
    let mut current = toks.pop()?.1.to_string();
    if let Some((_, cmp)) = toks.last() {
        if matches!(*cmp, "<" | ">") {
            current = format!("{cmp} {current}");
            toks.pop();
        }
    }
    let (id_offset, id) = toks.pop()?;
    let name = line[..id_offset].trim().to_string();
    if name.is_empty() || id.is_empty() || available.is_empty() || current.is_empty() {
        return None;
    }
    let id_truncated = id.ends_with('…');
    if !id_truncated && !is_valid_winget_id(id) {
        return None;
    }
    Some(WingetUpgrade {
        name,
        id: id.to_string(),
        id_truncated,
        current_version: current,
        available_version: available,
        source,
    })
}

fn last_column_aligned(line: &str, last_offset: usize) -> bool {
    let Some((byte_start, _)) = tokens(line).last().copied() else {
        return false;
    };
    let prefix = &line[..byte_start];
    let start = prefix.chars().count();
    let wide_slack = prefix.chars().filter(|c| !c.is_ascii()).count();
    start <= last_offset && start + wide_slack >= last_offset
}

/// Parse every upgrade table in `winget upgrade` output (the main table and the
/// "require explicit targeting" table). `None` when no table header was recognized.
pub(crate) fn parse_upgrade_table(stdout: &str) -> Option<Vec<WingetUpgrade>> {
    let lines = visible_lines(stdout);
    let mut rows: Vec<WingetUpgrade> = Vec::new();
    let mut saw_table = false;
    let mut i = 1;
    while i < lines.len() {
        if !is_rule_line(&lines[i]) || lines[i - 1].trim().is_empty() {
            i += 1;
            continue;
        }
        let Some((columns, last_offset)) = header_columns(&lines[i - 1]) else {
            i += 1;
            continue;
        };
        saw_table = true;
        let has_source = columns == 5;
        i += 1;
        while i < lines.len() {
            let line = &lines[i];
            if line.trim().is_empty() || is_rule_line(line) {
                break;
            }
            // Rows are aligned: the last column starts at the header's last column offset.
            // Footers ("36 upgrades available.") are not. Non-ASCII characters before it may be
            // double-width (CJK), which shifts the char index left by up to one per character.
            if !last_column_aligned(line, last_offset) {
                break;
            }
            let Some(row) = parse_row(line, has_source) else {
                break;
            };
            if !rows.iter().any(|r| r.id == row.id) {
                rows.push(row);
            }
            i += 1;
        }
    }
    saw_table.then_some(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    const REAL: &str = "Name                                                         Id                                    Version              Available                 Source\r\n\
-----------------------------------------------------------------------------------------------------------------------------------------------------------------\r\n\
7-Zip 26.02 (x64)                                            7zip.7zip                             26.02                26.06                     winget\r\n\
Python Launcher                                              Python.Launcher                       < 3.14.7             3.15.2                    winget\r\n\
Spotify                                                      Spotify.Spotify                       1.2.93.667.g7b5cc0ce 1.3.10.2586.g81a118a22460 winget\r\n\
WinLibs (MCF threads, UCRT runtime)                          BrechtSanders.WinLibs.MCF.UCRT        16.1.0-14.0.0-r1     16.2.0-14.0.0-r3          winget\r\n\
36 upgrades available.\r\n\
\r\n\
The following packages have an upgrade available, but require explicit targeting for upgrade:\r\n\
Name    Id              Version  Available Source\r\n\
-------------------------------------------------\r\n\
Discord Discord.Discord 1.0.9257 1.0.9269  winget\r\n";

    #[test]
    fn parses_real_table_with_comparators_and_second_table() {
        let rows = parse_upgrade_table(REAL).expect("table");
        assert_eq!(rows.len(), 5);
        assert_eq!(rows[0].name, "7-Zip 26.02 (x64)");
        assert_eq!(rows[0].id, "7zip.7zip");
        assert_eq!(rows[0].current_version, "26.02");
        assert_eq!(rows[0].available_version, "26.06");
        assert_eq!(rows[0].source.as_deref(), Some("winget"));
        assert_eq!(rows[1].current_version, "< 3.14.7");
        assert_eq!(rows[2].available_version, "1.3.10.2586.g81a118a22460");
        assert_eq!(rows[3].name, "WinLibs (MCF threads, UCRT runtime)");
        assert_eq!(rows[4].id, "Discord.Discord");
        assert!(rows.iter().all(|r| !r.id_truncated));
    }

    #[test]
    fn strips_progress_spinner_and_bars() {
        let row = |c: [&str; 5]| format!("{:<19}{:<20}{:<8}{:<10}{}", c[0], c[1], c[2], c[3], c[4]);
        let out = format!(
            "\r   - \r   \\ \r   |\r   / \r          \r  ██████████▒▒▒▒  1.00 MB / 2.00 MB\r          \r{}\n{}\n{}\n{}\n2 upgrades available.\n",
            row(["Name", "Id", "Version", "Available", "Source"]),
            "-".repeat(61),
            row(["Visual Studio Code", "Microsoft.VisualS…", "1.90.0", "1.95.3", "winget"]),
            row(["Café Tööl ünicode", "Vendor.CafeTool", "1.0", "2.0", "winget"]),
        );
        let out = out.as_str();
        let rows = parse_upgrade_table(out).expect("table");
        assert_eq!(rows.len(), 2);
        assert!(rows[0].id_truncated);
        assert_eq!(rows[0].id, "Microsoft.VisualS…");
        assert_eq!(rows[1].name, "Café Tööl ünicode");
        assert_eq!(rows[1].id, "Vendor.CafeTool");
    }

    #[test]
    fn four_column_table_has_no_source() {
        let out = "Name  Id          Version Available\n-----------------------------------\nFoo   Acme.Foo    1.0     1.1\n";
        let rows = parse_upgrade_table(out).expect("table");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].source, None);
        assert_eq!(rows[0].available_version, "1.1");
    }

    #[test]
    fn localized_header_is_detected_by_rule_line() {
        let out = "Nom              ID              Version Disponible Source\n----------------------------------------------------------\nOutil            Acme.Outil      1.0     1.2        winget\n1 mises à niveau disponibles.\n";
        let rows = parse_upgrade_table(out).expect("table");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, "Acme.Outil");
    }

    #[test]
    fn no_table_maps_by_exit_code() {
        let none = "No installed package found matching input criteria.\r\n";
        assert_eq!(parse_upgrade_table(none), None);
        assert_eq!(interpret_listing(none, Some(0)), Ok(Vec::new()));
        assert_eq!(interpret_listing(none, Some(ERR_UPDATE_NOT_APPLICABLE as i32)), Ok(Vec::new()));
        let failed = interpret_listing("Failed when searching source: winget\r\n", Some(0x8A15_0006u32 as i32));
        assert_eq!(failed.unwrap_err().reason, WingetReason::Failed);
    }

    #[test]
    fn rejects_unsafe_ids() {
        assert!(is_valid_winget_id("Microsoft.VisualStudio.2022.Community"));
        assert!(is_valid_winget_id("9NBLGGH4NNS1"));
        assert!(!is_valid_winget_id("--override"));
        assert!(!is_valid_winget_id("a b"));
        assert!(!is_valid_winget_id("x\"y"));
        assert!(!is_valid_winget_id(""));
    }

    #[test]
    fn upgrade_exit_codes_are_typed() {
        assert_eq!(upgrade_status(Some(0)), WingetUpgradeStatus::Upgraded);
        assert_eq!(upgrade_status(Some(ERR_UPDATE_NOT_APPLICABLE as i32)), WingetUpgradeStatus::NotApplicable);
        assert_eq!(upgrade_status(Some(ERR_INSTALL_PACKAGE_IN_USE as i32)), WingetUpgradeStatus::PackageInUse);
        assert_eq!(upgrade_status(Some(ERR_INSTALL_REBOOT_REQUIRED_TO_FINISH as i32)), WingetUpgradeStatus::RestartRequired);
        assert_eq!(upgrade_status(Some(1)), WingetUpgradeStatus::Failed);
        assert_eq!(upgrade_status(None), WingetUpgradeStatus::Failed);
    }
}
