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
            commands::window::show_main_window,
            commands::window::minimize_main_window,
            commands::window::toggle_maximize_main_window,
            commands::window::close_main_window,
            commands::window::hide_main_window,
        ])
        .setup(|app| {
            // Tray is configured from the frontend after mount for icon/menu flexibility.
            let _ = app;
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
