//! Start PC Toolkit Pro with Windows: a per-user `Run` value (normal start) or, when the user
//! explicitly enables "Always run as administrator", a per-user logon task with highest
//! privileges (created only from an elevated process, so Windows asked for consent once).

use crate::registry::{self, RegistryKeyPath, RegistryRoot, RegistryView};
use crate::registry_settings::{read_registry_data, write_registry_data, RegistryData};
use crate::startup_items::{current_user_id, delayed_logon_task_xml, delete_task, register_user_logon_task, task_xml};
use crate::PlatformResult;

const RUN_SUBKEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
/// `Run` value name (shown in Task Manager's Startup tab).
pub const AUTOSTART_RUN_VALUE: &str = "PC Toolkit Pro";
/// Task Scheduler folder / task name for the elevated sign-in start.
pub const AUTOSTART_TASK_FOLDER: &str = "PC Toolkit Pro";
pub const AUTOSTART_TASK_NAME: &str = "Start at sign-in (administrator)";
/// Command-line flag marking a start by Windows (used for "start minimized").
pub const AUTOSTART_FLAG: &str = "--autostart";

fn run_key() -> RegistryKeyPath {
    RegistryKeyPath::new(RegistryRoot::CurrentUser, RegistryView::Native64, RUN_SUBKEY)
}

fn task_path() -> String {
    format!("\\{AUTOSTART_TASK_FOLDER}\\{AUTOSTART_TASK_NAME}")
}

/// Quoted exe + autostart flag, as stored in the `Run` value / task.
pub fn autostart_command(exe: &str) -> String {
    format!("\"{exe}\" {AUTOSTART_FLAG}")
}

pub fn run_value() -> Option<String> {
    match read_registry_data(&run_key(), AUTOSTART_RUN_VALUE) {
        Ok(Some(RegistryData::String(s))) | Ok(Some(RegistryData::ExpandString(s))) => Some(s),
        _ => None,
    }
}

pub fn set_run_value(exe: &str) -> PlatformResult<()> {
    write_registry_data(&run_key(), AUTOSTART_RUN_VALUE, &RegistryData::String(autostart_command(exe)))
}

pub fn remove_run_value() -> PlatformResult<()> {
    if run_value().is_none() {
        return Ok(());
    }
    registry::delete_value(&run_key(), AUTOSTART_RUN_VALUE)
}

/// Task XML: logon trigger for the current user, highest available privileges, short delay so the
/// desktop settles first.
pub fn elevated_task_xml(user_id: &str, exe: &str) -> String {
    delayed_logon_task_xml(user_id, exe, AUTOSTART_FLAG, 5, "Starts PC Toolkit Pro as administrator when you sign in.")
        .replace("<RunLevel>LeastPrivilege</RunLevel>", "<RunLevel>HighestAvailable</RunLevel>")
}

pub fn elevated_task_exists() -> bool {
    task_xml(&task_path()).is_some()
}

/// Register (or update) the elevated sign-in task. Needs an elevated process.
pub fn register_elevated_task(exe: &str) -> PlatformResult<()> {
    let user = current_user_id().ok_or_else(|| crate::PlatformError::OperationFailed("userUnknown".into()))?;
    register_user_logon_task(AUTOSTART_TASK_FOLDER, AUTOSTART_TASK_NAME, &elevated_task_xml(&user, exe)).map(|_| ())
}

/// Remove the elevated sign-in task (needs an elevated process when it exists).
pub fn remove_elevated_task() -> PlatformResult<()> {
    if !elevated_task_exists() {
        return Ok(());
    }
    delete_task(&task_path())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn elevated_task_requests_highest_privileges_and_autostart_flag() {
        let xml = elevated_task_xml("PC\\user", r"C:\Apps\PCToolkitPro.exe");
        assert!(xml.contains("<RunLevel>HighestAvailable</RunLevel>"));
        assert!(!xml.contains("LeastPrivilege"));
        assert!(xml.contains("<Arguments>--autostart</Arguments>"));
        assert!(xml.contains("<UserId>PC\\user</UserId>"));
    }

    #[test]
    fn autostart_command_quotes_the_path() {
        assert_eq!(autostart_command(r"C:\A B\x.exe"), "\"C:\\A B\\x.exe\" --autostart");
    }
}
