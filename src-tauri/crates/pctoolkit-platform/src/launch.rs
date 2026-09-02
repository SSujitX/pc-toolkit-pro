use serde::{Deserialize, Serialize};
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
        QuickActionId::TaskManager => spawn("taskmgr", &[]),
        QuickActionId::DeviceManager => spawn_shell("devmgmt.msc"),
        QuickActionId::ControlPanel => spawn("control", &[]),
        QuickActionId::DiskManagement => spawn_shell("diskmgmt.msc"),
        QuickActionId::SystemInfo => spawn("msinfo32", &[]),
        QuickActionId::RegistryEditor => spawn("regedit", &[]),
        QuickActionId::Services => spawn_shell("services.msc"),
        QuickActionId::FileExplorer => spawn("explorer", &[]),
        QuickActionId::NetworkConnections => spawn_shell("ncpa.cpl"),
        QuickActionId::SnippingTool => spawn("snippingtool", &[]),
        QuickActionId::Notepad => spawn("notepad", &[]),
        QuickActionId::VolumeMixer => spawn("sndvol", &[]),
        QuickActionId::Settings => launch_settings(),
        QuickActionId::CommandPromptAdmin => elevate("cmd"),
        QuickActionId::PowerShellAdmin => elevate("powershell"),
    }
}

fn spawn(program: &str, args: &[&str]) -> PlatformResult<()> {
    Command::new(program)
        .args(args)
        .spawn()
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
        return spawn("control", &[]);
    }
    Ok(())
}

fn elevate(program: &str) -> PlatformResult<()> {
    let script = format!(
        "Start-Process {} -Verb RunAs -WorkingDirectory 'C:\\Windows\\System32'",
        program
    );
    let mut cmd = Command::new("powershell");
    cmd.args(["-NoProfile", "-WindowStyle", "Hidden", "-Command", &script]);
    crate::process::hide_console(&mut cmd);
    cmd.spawn()
        .map_err(|e| PlatformError::OperationFailed(e.to_string()))?;
    Ok(())
}
