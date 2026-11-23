use serde::{Deserialize, Serialize};
use std::ffi::OsStr;
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
    EventViewer,
    ResourceMonitor,
    ReliabilityMonitor,
    ComputerManagement,
    TaskScheduler,
    SystemProperties,
    EnvironmentVariables,
    DirectXDiagnostic,
    DiskCleanup,
    DefragmentOptimize,
    SystemRestore,
    MemoryDiagnostic,
    WindowsUpdate,
    WindowsSecurity,
    StartupApps,
    InstalledApps,
    StorageSettings,
    PowerOptions,
    SoundSettings,
    DisplaySettings,
    Firewall,
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
        QuickActionId::CommandPromptAdmin => elevate(&cmd_exe()),
        QuickActionId::PowerShellAdmin => elevate(&crate::registry::powershell_exe()),
        QuickActionId::EventViewer => spawn_shell("eventvwr.msc"),
        QuickActionId::ResourceMonitor => spawn_system32("resmon.exe"),
        QuickActionId::ReliabilityMonitor => spawn_system32_args("perfmon.exe", &["/rel"]),
        QuickActionId::ComputerManagement => spawn_shell("compmgmt.msc"),
        QuickActionId::TaskScheduler => spawn_shell("taskschd.msc"),
        QuickActionId::SystemProperties => spawn_system32("SystemPropertiesAdvanced.exe"),
        QuickActionId::EnvironmentVariables => spawn_system32_args("rundll32.exe", &["sysdm.cpl,EditEnvironmentVariables"]),
        QuickActionId::DirectXDiagnostic => spawn_system32("dxdiag.exe"),
        QuickActionId::DiskCleanup => spawn_system32("cleanmgr.exe"),
        QuickActionId::DefragmentOptimize => spawn_system32("dfrgui.exe"),
        QuickActionId::SystemRestore => spawn_system32("rstrui.exe"),
        QuickActionId::MemoryDiagnostic => spawn_system32("MdSched.exe"),
        QuickActionId::WindowsUpdate => spawn_shell("ms-settings:windowsupdate"),
        QuickActionId::WindowsSecurity => spawn_shell("windowsdefender:"),
        QuickActionId::StartupApps => spawn_shell("ms-settings:startupapps"),
        QuickActionId::InstalledApps => spawn_shell("ms-settings:appsfeatures"),
        QuickActionId::StorageSettings => spawn_shell("ms-settings:storagesense"),
        QuickActionId::PowerOptions => spawn_shell("powercfg.cpl"),
        QuickActionId::SoundSettings => spawn_shell("mmsys.cpl"),
        QuickActionId::DisplaySettings => spawn_shell("ms-settings:display"),
        QuickActionId::Firewall => spawn_shell("wf.msc"),
    }
}

fn system32_path(exe: &str) -> PathBuf {
    crate::registry::system32_exe(exe)
}

fn cmd_exe() -> PathBuf {
    crate::registry::system32_exe("cmd.exe")
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
    let mut cmd = Command::new(cmd_exe());
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

/// System32 tool with fixed arguments (constants only — never user input).
fn spawn_system32_args(exe: &str, args: &[&str]) -> PlatformResult<()> {
    let path = system32_path(exe);
    let mut cmd = Command::new(cmd_exe());
    cmd.args(["/C", "start", ""]);
    cmd.arg(&path);
    cmd.args(args);
    crate::process::hide_console(&mut cmd);
    cmd.spawn()
        .map_err(|e| PlatformError::OperationFailed(e.to_string()))?;
    Ok(())
}

fn spawn_shell(target: &str) -> PlatformResult<()> {
    let mut cmd = Command::new(cmd_exe());
    cmd.args(["/C", "start", "", target]);
    crate::process::hide_console(&mut cmd);
    cmd.spawn()
        .map_err(|e| PlatformError::OperationFailed(e.to_string()))?;
    Ok(())
}

fn launch_settings() -> PlatformResult<()> {
    let mut cmd = Command::new(cmd_exe());
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
    let mut cmd = Command::new(cmd_exe());
    cmd.args(["/C", "start", "", "ms-screenclip:"]);
    crate::process::hide_console(&mut cmd);
    cmd.spawn()
        .map_err(|e| PlatformError::OperationFailed(e.to_string()))?;
    Ok(())
}

fn elevate(program: &std::path::Path) -> PlatformResult<()> {
    // Absolute System32 path so `Start-Process` cannot resolve a planted exe via PATH.
    let program = program.to_string_lossy();
    let script = format!(
        "Start-Process -FilePath '{}' -Verb RunAs -WorkingDirectory $env:SystemRoot\\System32",
        program.replace('\'', "''")
    );
    let mut cmd = Command::new(crate::registry::powershell_exe());
    cmd.args(["-NoProfile", "-WindowStyle", "Hidden", "-Command", &script]);
    crate::process::hide_console(&mut cmd);
    cmd.spawn()
        .map_err(|e| PlatformError::OperationFailed(e.to_string()))?;
    Ok(())
}

/// Prompt UAC and start a new elevated copy of this executable.
/// On success the caller must exit quickly so the single-instance lock is released
/// before the elevated process finishes startup.
pub fn relaunch_self_elevated() -> PlatformResult<()> {
    relaunch_self_elevated_with_args("")
}

/// Start an elevated copy of this executable with `args` (normal UAC prompt; never silent).
/// Returns `elevationCancelled` when the user declines.
pub fn relaunch_self_elevated_with_args(args: &str) -> PlatformResult<()> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;

        use windows::core::PCWSTR;
        use windows::Win32::Foundation::{CloseHandle, ERROR_CANCELLED, GetLastError};
        use windows::Win32::UI::Shell::{
            ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW,
        };
        use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

        let exe = std::env::current_exe()
            .map_err(|e| PlatformError::OperationFailed(e.to_string()))?;
        if !exe.is_file() {
            return Err(PlatformError::OperationFailed(
                "application executable is missing".into(),
            ));
        }

        let mut file: Vec<u16> = exe.as_os_str().encode_wide().chain(Some(0)).collect();
        let mut verb: Vec<u16> = OsStr::new("runas").encode_wide().chain(Some(0)).collect();
        let mut params: Vec<u16> = OsStr::new(args).encode_wide().chain(Some(0)).collect();

        let mut info = SHELLEXECUTEINFOW {
            cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_NOCLOSEPROCESS,
            lpVerb: PCWSTR(verb.as_mut_ptr()),
            lpFile: PCWSTR(file.as_mut_ptr()),
            lpParameters: if args.is_empty() { PCWSTR::null() } else { PCWSTR(params.as_mut_ptr()) },
            nShow: SW_SHOWNORMAL.0,
            ..Default::default()
        };

        if unsafe { ShellExecuteExW(&mut info) }.is_err() {
            let code = unsafe { GetLastError() };
            if code == ERROR_CANCELLED {
                return Err(PlatformError::OperationFailed("elevationCancelled".into()));
            }
            return Err(PlatformError::OperationFailed(format!(
                "elevationFailed:{code:?}"
            )));
        }

        if !info.hProcess.is_invalid() {
            let _ = unsafe { CloseHandle(info.hProcess) };
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = args;
        Err(PlatformError::Unsupported)
    }
}
