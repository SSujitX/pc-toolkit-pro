use serde::{Deserialize, Serialize};
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::Command;

use crate::{PlatformError, PlatformResult};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum QuickActionId {
    TaskManager,
    DeviceManager,
    ControlPanel,
    DiskManagement,
    CommandPromptAdmin,
    PowerShellAdmin,
    SystemInfo,
    RegistryEditor,
    Settings,
    Services,
    FileExplorer,
    NetworkConnections,
    SnippingTool,
    Notepad,
    VolumeMixer,
}

pub fn launch_program(id: QuickActionId) -> PlatformResult<()> {
    match id {
        // GUI apps: prefer System32 + detached start so PATH / console flags
        // inherited from the Tauri host do not block Task Manager and peers.
        QuickActionId::TaskManager => spawn_system32("taskmgr.exe"),
        QuickActionId::ControlPanel => spawn_system32("control.exe"),
        QuickActionId::SystemInfo => spawn_system32("msinfo32.exe"),
        QuickActionId::RegistryEditor => spawn_system32("regedit.exe"),
        QuickActionId::FileExplorer => spawn_system32("explorer.exe"),
        QuickActionId::Notepad => spawn_system32("notepad.exe"),
        QuickActionId::VolumeMixer => spawn_system32("sndvol.exe"),
        QuickActionId::SnippingTool => launch_snipping_tool(),
        QuickActionId::DeviceManager => spawn_shell("devmgmt.msc"),
        QuickActionId::DiskManagement => spawn_shell("diskmgmt.msc"),
        QuickActionId::Services => spawn_shell("services.msc"),
        QuickActionId::NetworkConnections => spawn_shell("ncpa.cpl"),
        QuickActionId::Settings => launch_settings(),
        QuickActionId::CommandPromptAdmin => elevate("cmd.exe"),
        QuickActionId::PowerShellAdmin => elevate("powershell.exe"),
    }
}

fn system32_path(exe: &str) -> PathBuf {
    let root = std::env::var_os("SystemRoot").unwrap_or_else(|| OsString::from(r"C:\Windows"));
    PathBuf::from(root).join("System32").join(exe)
}

fn spawn_system32(exe: &str) -> PlatformResult<()> {
    let path = system32_path(exe);
    if path.is_file() {
        return spawn_detached(&path);
    }
    // Fallback: `start` resolves via PATH / App Paths.
    spawn_shell(exe)
}

fn spawn_detached(path: &std::path::Path) -> PlatformResult<()> {
    // `cmd /C start "" <path>` detaches a visible GUI process reliably.
    let mut cmd = Command::new("cmd");
    cmd.args([
        "/C",
        "start",
        "",
        &path.to_string_lossy(),
    ]);
    crate::process::hide_console(&mut cmd);
    cmd.spawn()
        .map_err(|e| PlatformError::OperationFailed(e.to_string()))?;
    Ok(())
}

fn spawn_shell(target: &str) -> PlatformResult<()> {
    let mut cmd = Command::new("cmd");
    cmd.args(["/C", "start", "", target]);
    crate::process::hide_console(&mut cmd);
    cmd.spawn()
        .map_err(|e| PlatformError::OperationFailed(e.to_string()))?;
    Ok(())
}

fn launch_settings() -> PlatformResult<()> {
    let mut cmd = Command::new("cmd");
    cmd.args(["/C", "start", "", "ms-settings:"]);
    crate::process::hide_console(&mut cmd);
    if cmd.spawn().is_err() {
        return spawn_system32("control.exe");
    }
    Ok(())
}

fn launch_snipping_tool() -> PlatformResult<()> {
    // Win10 classic + Win11 Screen clip URI fallback.
    if spawn_system32("SnippingTool.exe").is_ok() {
        return Ok(());
    }
    let mut cmd = Command::new("cmd");
    cmd.args(["/C", "start", "", "ms-screenclip:"]);
    crate::process::hide_console(&mut cmd);
    cmd.spawn()
        .map_err(|e| PlatformError::OperationFailed(e.to_string()))?;
    Ok(())
}

fn elevate(program: &str) -> PlatformResult<()> {
    let script = format!(
        "Start-Process -FilePath '{}' -Verb RunAs -WorkingDirectory $env:SystemRoot\\System32",
        program.replace('\'', "''")
    );
    let mut cmd = Command::new("powershell");
    cmd.args(["-NoProfile", "-WindowStyle", "Hidden", "-Command", &script]);
    crate::process::hide_console(&mut cmd);
    cmd.spawn()
        .map_err(|e| PlatformError::OperationFailed(e.to_string()))?;
    Ok(())
}
