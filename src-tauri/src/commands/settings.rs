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
