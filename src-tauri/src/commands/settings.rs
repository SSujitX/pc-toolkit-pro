use std::fs;
use std::path::PathBuf;

use tauri_plugin_opener::OpenerExt;

/// Open `%LOCALAPPDATA%\PC Toolkit Pro` in Explorer (create if missing).
/// Uses the Rust opener API so we are not blocked by `opener:default`
/// (which does not include `allow-open-path`).
#[tauri::command]
pub fn open_app_data_folder(app: tauri::AppHandle) -> Result<(), String> {
    let base = std::env::var_os("LOCALAPPDATA")
        .or_else(|| std::env::var_os("APPDATA"))
        .ok_or_else(|| "app data directory unavailable".to_string())?;
    let mut path = PathBuf::from(base);
    path.push("PC Toolkit Pro");
    fs::create_dir_all(&path).map_err(|e| format!("could not create app data folder: {e}"))?;
    app.opener()
        .open_path(path.to_string_lossy(), None::<&str>)
        .map_err(|e| e.to_string())
}

use pctoolkit_core::app_settings::{
    load_app_settings, plan_registration, save_app_settings, AppStartupSettings, StartupRegistrationState,
};
use pctoolkit_platform::autostart;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppStartupStatus {
    pub settings: AppStartupSettings,
    pub registration: StartupRegistrationState,
    pub elevated: bool,
}

/// Make the Windows start registration match the settings (Run value / elevated sign-in task).
pub fn apply_startup_registration(settings: AppStartupSettings) -> StartupRegistrationState {
    let elevated = pctoolkit_platform::is_user_admin();
    let Ok(exe) = std::env::current_exe() else {
        return StartupRegistrationState::Failed;
    };
    let exe = exe.to_string_lossy().into_owned();
    let plan = plan_registration(settings, elevated, autostart::elevated_task_exists());
    let mut ok = true;
    match plan.task {
        Some(true) => ok &= autostart::register_elevated_task(&exe).is_ok(),
        Some(false) => ok &= autostart::remove_elevated_task().is_ok(),
        None => {}
    }
    if plan.run_value {
        ok &= autostart::set_run_value(&exe).is_ok();
    } else {
        ok &= autostart::remove_run_value().is_ok();
    }
    if !ok {
        log::warn!("startup_registration_failed state={:?}", plan.state);
        return StartupRegistrationState::Failed;
    }
    plan.state
}

#[tauri::command]
pub async fn get_app_startup_settings() -> Result<AppStartupStatus, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let settings = load_app_settings();
        let elevated = pctoolkit_platform::is_user_admin();
        let plan = plan_registration(settings, elevated, autostart::elevated_task_exists());
        AppStartupStatus { settings, registration: plan.state, elevated }
    })
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_app_startup_settings(settings: AppStartupSettings) -> Result<AppStartupStatus, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let settings = save_app_settings(settings).map_err(|e| e.to_string())?;
        let registration = apply_startup_registration(settings);
        Ok(AppStartupStatus { settings, registration, elevated: pctoolkit_platform::is_user_admin() })
    })
    .await
    .map_err(|e| e.to_string())?
}
