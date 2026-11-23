use tauri::Manager;

#[tauri::command]
pub fn show_main_window(app: tauri::AppHandle) -> Result<(), String> {
    // Started by Windows with "start minimized": stay in the tray once; later calls show normally.
    if let Some(hidden) = app.try_state::<crate::StartHidden>() {
        if hidden.0.swap(false, std::sync::atomic::Ordering::SeqCst) {
            return Ok(());
        }
    }
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "main window missing".to_string())?;
    crate::webview_memory::set_low_memory(&window, false);
    window.show().map_err(|e| e.to_string())?;
    window.set_focus().map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn hide_main_window(app: tauri::AppHandle) -> Result<(), String> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "main window missing".to_string())?;
    window.hide().map_err(|e| e.to_string())?;
    crate::webview_memory::set_low_memory(&window, true);
    Ok(())
}

#[tauri::command]
pub fn minimize_main_window(app: tauri::AppHandle) -> Result<(), String> {
    app.get_webview_window("main")
        .ok_or_else(|| "main window missing".to_string())?
        .minimize()
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn toggle_maximize_main_window(app: tauri::AppHandle) -> Result<(), String> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "main window missing".to_string())?;
    if window.is_maximized().map_err(|e| e.to_string())? {
        window.unmaximize().map_err(|e| e.to_string())
    } else {
        window.maximize().map_err(|e| e.to_string())
    }
}

#[tauri::command]
pub fn close_main_window(app: tauri::AppHandle) -> Result<(), String> {
    // Hide instead of destroy so tray can restore.
    hide_main_window(app)
}
