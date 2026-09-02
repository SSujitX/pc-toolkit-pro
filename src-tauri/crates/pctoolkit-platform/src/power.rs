use serde::{Deserialize, Serialize};
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
        PowerAction::Shutdown => run_shutdown(&["/s", "/f", "/t", "0"]),
        PowerAction::Restart => run_shutdown(&["/r", "/f", "/t", "0"]),
        PowerAction::Hibernate => run_shutdown(&["/h"]),
        PowerAction::SignOut => run_shutdown(&["/l"]),
        PowerAction::Sleep => {
            Command::new("rundll32.exe")
                .args(["powrprof.dll,SetSuspendState", "0,1,0"])
                .spawn()
                .map_err(|e| PlatformError::OperationFailed(e.to_string()))?;
            Ok(())
        }
        PowerAction::Lock => {
            Command::new("rundll32.exe")
                .args(["user32.dll,LockWorkStation"])
                .spawn()
                .map_err(|e| PlatformError::OperationFailed(e.to_string()))?;
            Ok(())
        }
    }
}

pub fn schedule_shutdown(seconds: u64) -> PlatformResult<()> {
    let secs = seconds.to_string();
    run_shutdown(&["/s", "/f", "/t", &secs])
}

pub fn cancel_scheduled_shutdown() -> PlatformResult<()> {
    let _ = Command::new("shutdown").args(["/a"]).status();
    Ok(())
}

fn run_shutdown(args: &[&str]) -> PlatformResult<()> {
    Command::new("shutdown")
        .args(args)
        .spawn()
        .map_err(|e| PlatformError::OperationFailed(e.to_string()))?;
    Ok(())
}
