use serde::{Deserialize, Serialize};
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::Command;

use crate::{PlatformError, PlatformResult};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PowerAction {
    Shutdown,
    Restart,
    Sleep,
    Hibernate,
    Lock,
    SignOut,
}

pub fn execute_power_action(action: PowerAction) -> PlatformResult<()> {
    match action {
        // Match Python inspiration: shutdown /s|/r|/h|/l with /f where applicable.
        PowerAction::Shutdown => run_shutdown(&["/s", "/f", "/t", "0"])?,
        PowerAction::Restart => run_shutdown(&["/r", "/f", "/t", "0"])?,
        PowerAction::Hibernate => run_shutdown(&["/h"])?,
        PowerAction::SignOut => run_shutdown(&["/l"])?,
        PowerAction::Sleep => sleep_now()?,
        PowerAction::Lock => lock_workstation()?,
    }
    Ok(())
}

pub fn schedule_shutdown(seconds: u64) -> PlatformResult<()> {
    // Replace any pending schedule so /t does not fail with "already scheduled".
    let _ = cancel_scheduled_shutdown();
    let secs = seconds.to_string();
    run_shutdown(&["/s", "/f", "/t", &secs])
}

pub fn cancel_scheduled_shutdown() -> PlatformResult<()> {
    // `/a` returns non-zero when nothing is pending — treat as success for UX.
    match run_shutdown_status(&["/a"]) {
        Ok(()) => Ok(()),
        Err(_) => Ok(()),
    }
}

fn sleep_now() -> PlatformResult<()> {
    #[cfg(windows)]
    {
        // Inspiration: rundll32 powrprof.dll,SetSuspendState 0,1,0
        // Win32: hibernate=false, force=true, disable_wake_events=false
        use windows::Win32::System::Power::SetSuspendState;
        let ok = unsafe { SetSuspendState(false, true, false) };
        if !ok {
            return Err(PlatformError::OperationFailed(
                "SetSuspendState failed".into(),
            ));
        }
        return Ok(());
    }
    #[cfg(not(windows))]
    {
        Err(PlatformError::Unsupported)
    }
}

fn lock_workstation() -> PlatformResult<()> {
    #[cfg(windows)]
    {
        use windows::Win32::System::Shutdown::LockWorkStation;
        unsafe { LockWorkStation() }.map_err(|e| {
            PlatformError::OperationFailed(format!("LockWorkStation failed: {e}"))
        })?;
        return Ok(());
    }
    #[cfg(not(windows))]
    {
        Err(PlatformError::Unsupported)
    }
}

fn shutdown_exe() -> PathBuf {
    let root = std::env::var_os("SystemRoot").unwrap_or_else(|| OsString::from(r"C:\Windows"));
    PathBuf::from(root).join("System32").join("shutdown.exe")
}

fn run_shutdown(args: &[&str]) -> PlatformResult<()> {
    run_shutdown_status(args)
}

fn run_shutdown_status(args: &[&str]) -> PlatformResult<()> {
    let exe = shutdown_exe();
    let mut cmd = if exe.is_file() {
        Command::new(&exe)
    } else {
        Command::new("shutdown")
    };
    cmd.args(args);
    crate::process::hide_console(&mut cmd);
    // Wait + check exit code (Python used check=True). spawn-only hid failures.
    let status = cmd
        .status()
        .map_err(|e| PlatformError::OperationFailed(e.to_string()))?;
    if status.success() {
        Ok(())
    } else {
        Err(PlatformError::OperationFailed(format!(
            "shutdown {:?} exited with {}",
            args,
            status.code().unwrap_or(-1)
        )))
    }
}
