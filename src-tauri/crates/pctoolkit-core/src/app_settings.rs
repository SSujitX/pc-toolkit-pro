//! App startup settings: start with Windows, start minimized to tray, always run as administrator.
//!
//! Persisted in `%LOCALAPPDATA%\PC Toolkit Pro\app-settings.json` (unreadable → defaults).
//! Registration policy is a pure function ([`plan_registration`]) so it can be tested; the
//! adapter applies it with platform `autostart` helpers on every start and on every change.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

use crate::shared::{CoreError, CoreResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AppStartupSettings {
    /// Start PC Toolkit Pro when the user signs in.
    pub start_with_windows: bool,
    /// When started by Windows, stay in the tray instead of showing the window.
    pub start_minimized: bool,
    /// Always run elevated: normal launches show the UAC prompt; with `start_with_windows` an
    /// elevated sign-in task starts it without a prompt (explicit opt-in).
    pub run_as_admin: bool,
}

impl Default for AppStartupSettings {
    fn default() -> Self {
        Self { start_with_windows: false, start_minimized: true, run_as_admin: false }
    }
}

fn settings_path() -> Option<PathBuf> {
    let base = std::env::var_os("LOCALAPPDATA").or_else(|| std::env::var_os("APPDATA"))?;
    Some(PathBuf::from(base).join("PC Toolkit Pro").join("app-settings.json"))
}

pub fn load_app_settings() -> AppStartupSettings {
    settings_path()
        .and_then(|p| fs::read_to_string(p).ok())
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

pub fn save_app_settings(settings: AppStartupSettings) -> CoreResult<AppStartupSettings> {
    let path = settings_path().ok_or_else(|| CoreError::OperationFailed("settings path".into()))?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| CoreError::OperationFailed(e.to_string()))?;
    }
    let raw = serde_json::to_string_pretty(&settings).map_err(|e| CoreError::OperationFailed(e.to_string()))?;
    fs::write(path, raw).map_err(|e| CoreError::OperationFailed(e.to_string()))?;
    Ok(settings)
}

/// What should exist for "start with Windows".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegistrationPlan {
    /// Per-user `Run` value present?
    pub run_value: bool,
    /// Elevated sign-in task: Some(true) register, Some(false) remove, None leave as is.
    pub task: Option<bool>,
    pub state: StartupRegistrationState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StartupRegistrationState {
    /// Not starting with Windows.
    Off,
    /// Starts via the per-user Run value (no admin).
    RunValue,
    /// Starts elevated via the sign-in task (no prompt).
    ElevatedTask,
    /// Run-as-admin start needs one elevated launch to create the task; until then the Run
    /// value starts it and Windows asks for permission.
    NeedsAdminToCreateTask,
    /// An elevated task from earlier must be removed, which needs an elevated launch.
    NeedsAdminToRemoveTask,
    /// Registry / Task Scheduler call failed.
    Failed,
}

/// Pure policy: desired registration from settings, elevation, and whether the task exists.
pub fn plan_registration(settings: AppStartupSettings, is_admin: bool, task_exists: bool) -> RegistrationPlan {
    use StartupRegistrationState as S;
    if !settings.start_with_windows {
        return match (task_exists, is_admin) {
            (true, true) => RegistrationPlan { run_value: false, task: Some(false), state: S::Off },
            (true, false) => RegistrationPlan { run_value: false, task: None, state: S::NeedsAdminToRemoveTask },
            _ => RegistrationPlan { run_value: false, task: None, state: S::Off },
        };
    }
    if settings.run_as_admin {
        return match (is_admin, task_exists) {
            (true, _) => RegistrationPlan { run_value: false, task: Some(true), state: S::ElevatedTask },
            (false, true) => RegistrationPlan { run_value: false, task: None, state: S::ElevatedTask },
            (false, false) => RegistrationPlan { run_value: true, task: None, state: S::NeedsAdminToCreateTask },
        };
    }
    match (task_exists, is_admin) {
        (true, true) => RegistrationPlan { run_value: true, task: Some(false), state: S::RunValue },
        // An old elevated task would still start the app elevated; keep the Run value too
        // (single instance prevents doubles) and ask for one elevated launch to clean it up.
        (true, false) => RegistrationPlan { run_value: true, task: None, state: S::NeedsAdminToRemoveTask },
        _ => RegistrationPlan { run_value: true, task: None, state: S::RunValue },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use StartupRegistrationState as S;

    fn s(start: bool, admin: bool) -> AppStartupSettings {
        AppStartupSettings { start_with_windows: start, start_minimized: true, run_as_admin: admin }
    }

    #[test]
    fn off_removes_everything_it_can() {
        assert_eq!(plan_registration(s(false, false), false, false).state, S::Off);
        let p = plan_registration(s(false, true), true, true);
        assert_eq!((p.run_value, p.task, p.state), (false, Some(false), S::Off));
        assert_eq!(plan_registration(s(false, false), false, true).state, S::NeedsAdminToRemoveTask);
    }

    #[test]
    fn normal_start_uses_run_value() {
        let p = plan_registration(s(true, false), false, false);
        assert_eq!((p.run_value, p.task, p.state), (true, None, S::RunValue));
    }

    #[test]
    fn admin_start_uses_task_only_when_elevated_or_existing() {
        let p = plan_registration(s(true, true), true, false);
        assert_eq!((p.run_value, p.task, p.state), (false, Some(true), S::ElevatedTask));
        let p = plan_registration(s(true, true), false, true);
        assert_eq!((p.run_value, p.task, p.state), (false, None, S::ElevatedTask));
        let p = plan_registration(s(true, true), false, false);
        assert_eq!((p.run_value, p.task, p.state), (true, None, S::NeedsAdminToCreateTask));
    }

    #[test]
    fn defaults_and_partial_json() {
        assert_eq!(AppStartupSettings::default(), s(false, false));
        let parsed: AppStartupSettings = serde_json::from_str(r#"{"startWithWindows":true}"#).unwrap();
        assert!(parsed.start_with_windows && parsed.start_minimized && !parsed.run_as_admin);
    }
}
