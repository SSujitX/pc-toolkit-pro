mod commands;
mod events;

use tauri::{Manager, WindowEvent};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            commands::monitor::get_monitor_snapshot,
            commands::quick_actions::open_quick_action,
            commands::cleaner::scan_cleanup_candidates,
            commands::cleaner::execute_cleanup,
            commands::cleaner::scan_deep_cleanup,
            commands::cleaner::execute_deep_cleanup_command,
            commands::cleaner::cancel_cleanup,
            commands::power::execute_power_action,
            commands::power::schedule_shutdown,
            commands::power::cancel_scheduled_shutdown,
            commands::system_info::get_system_information,
            commands::history::list_history,
            commands::history::clear_history,
            commands::memory::get_memory_stats,
            commands::memory::get_memory_cleaner_settings,
            commands::memory::set_memory_cleaner_settings,
            commands::memory::optimize_memory,
            commands::memory::cancel_memory_optimize,
            commands::memory::get_elevation_status,
            commands::memory::restart_as_administrator,
            commands::settings::open_app_data_folder,
            commands::window::show_main_window,
            commands::window::minimize_main_window,
            commands::window::toggle_maximize_main_window,
            commands::window::close_main_window,
            commands::window::hide_main_window,
        ])
        .setup(|app| {
            // Ensure a tray icon exists as soon as the process starts (Windows needs a
            // real icon; FE attaches the full menu after Vue mounts).
            setup_tray_icon(app.handle())?;
            Ok(())
        })
        .on_window_event(|window, event| {
            // Match Python tray behavior: close / Alt+F4 hides to tray; Exit quits.
            if window.label() != "main" {
                return;
            }
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .run(tauri::generate_context!())
        .expect("PC Toolkit Pro failed to start");
}

/// Stable id so the Vue tray service can find this icon and attach the menu.
pub const TRAY_ID: &str = "pctoolkit-main-tray";

fn setup_tray_icon(app: &tauri::AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    use tauri::image::Image;
    use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

    let icon = app
        .default_window_icon()
        .cloned()
        .or_else(|| Image::from_bytes(include_bytes!("../icons/32x32.png")).ok())
        .ok_or("tray icon missing")?;

    let _tray = TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .tooltip("PC Toolkit Pro")
        .show_menu_on_left_click(false)
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let app = tray.app_handle();
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.unminimize();
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
        })
        .build(app)?;

    Ok(())
}
