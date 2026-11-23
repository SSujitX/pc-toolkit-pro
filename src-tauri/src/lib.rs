mod commands;
mod events;
mod webview_memory;

use tauri::{Manager, WindowEvent};

/// True when another copy of this exe is already running (the single-instance plugin will hand
/// over to it, so elevating first would only cost the user a pointless UAC prompt).
fn another_instance_running() -> bool {
    let Some(image) = std::env::current_exe()
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
    else {
        return false;
    };
    pctoolkit_platform::count_running_images(&[image.as_str()]) > 1
}

/// Set when Windows started the app (`--autostart`) and "start minimized" is on.
pub struct StartHidden(pub std::sync::atomic::AtomicBool);

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let autostart = std::env::args().any(|a| a == pctoolkit_platform::autostart::AUTOSTART_FLAG);
    let startup = pctoolkit_core::app_settings::load_app_settings();
    // "Always run as administrator": relaunch through the normal UAC prompt before anything else
    // (no single-instance lock is held yet). Declining continues unelevated with honest skips.
    if startup.run_as_admin && !pctoolkit_platform::is_user_admin() && !another_instance_running() {
        let args = if autostart { pctoolkit_platform::autostart::AUTOSTART_FLAG } else { "" };
        if pctoolkit_platform::relaunch_self_elevated_with_args(args).is_ok() {
            std::process::exit(0);
        }
    }
    let start_hidden = autostart && startup.start_with_windows && startup.start_minimized;

    tauri::Builder::default()
        .manage(StartHidden(std::sync::atomic::AtomicBool::new(start_hidden)))
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
            commands::process::probe_running_processes_command,
            commands::process::close_running_processes_command,
            commands::power::execute_power_action,
            commands::power::schedule_shutdown,
            commands::power::cancel_scheduled_shutdown,
            commands::system_info::get_system_information,
            commands::history::list_history,
            commands::history::clear_history,
            commands::history::get_history_settings,
            commands::history::update_history_settings,
            commands::memory::get_memory_stats,
            commands::memory::get_memory_cleaner_settings,
            commands::memory::set_memory_cleaner_settings,
            commands::memory::optimize_memory,
            commands::memory::cancel_memory_optimize,
            commands::memory::get_elevation_status,
            commands::memory::restart_as_administrator,
            commands::care::scan_care,
            commands::care::fix_care,
            commands::care::stop_care,
            commands::care::skip_care_category,
            commands::care::open_care_action,
            commands::settings::get_app_startup_settings,
            commands::settings::set_app_startup_settings,
            commands::care::cancel_care_after_action,
            commands::care::list_care_undo_records,
            commands::care::undo_care_fixes,
            commands::uninstaller::list_installed_programs,
            commands::uninstaller::uninstall_program_command,
            commands::uninstaller::scan_program_leftovers_command,
            commands::uninstaller::clean_uninstall_leftovers,
            commands::uninstaller::cancel_uninstaller,
            commands::uninstaller::get_program_icons_command,
            commands::uninstaller::focus_uninstaller_window,
            commands::uninstaller::dismiss_removed_program_command,
            commands::settings::open_app_data_folder,
            commands::startup::list_startup_items_command,
            commands::startup::startup_item_action_command,
            commands::startup::get_startup_icons_command,
            commands::startup::reveal_startup_item_command,
            commands::window::show_main_window,
            commands::window::minimize_main_window,
            commands::window::toggle_maximize_main_window,
            commands::window::close_main_window,
            commands::window::hide_main_window,
        ])
        .setup(move |app| {
            // Ensure a tray icon exists as soon as the process starts (Windows needs a
            // real icon; FE attaches the full menu after Vue mounts).
            setup_tray_icon(app.handle())?;
            commands::memory::start_auto_clean_scheduler(app.handle().clone());
            // Started straight into the tray: the window is never shown, so trim WebView2 once
            // the page has loaded.
            if start_hidden {
                let handle = app.handle().clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_secs(8));
                    if let Some(webview) = handle.get_webview_window("main") {
                        let visible = webview.is_visible().unwrap_or(true);
                        if !visible {
                            webview_memory::set_low_memory(&webview, true);
                        }
                    }
                });
            }
            // Keep the Windows start registration in sync (e.g. create the elevated sign-in
            // task on the first elevated launch, or fix a moved exe path).
            std::thread::spawn(move || {
                let settings = pctoolkit_core::app_settings::load_app_settings();
                if settings.start_with_windows
                    || pctoolkit_platform::autostart::run_value().is_some()
                    || pctoolkit_platform::autostart::elevated_task_exists()
                {
                    commands::settings::apply_startup_registration(settings);
                }
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            // Match Python tray behavior: close / Alt+F4 hides to tray; Exit quits.
            if window.label() != "main" {
                return;
            }
            match event {
                WindowEvent::CloseRequested { api, .. } => {
                    api.prevent_close();
                    let _ = window.hide();
                    if let Some(webview) = window.app_handle().get_webview_window("main") {
                        webview_memory::set_low_memory(&webview, true);
                    }
                }
                // Any path that brings the window back (tray, second launch, JS) ends in focus.
                WindowEvent::Focused(true) => {
                    if let Some(webview) = window.app_handle().get_webview_window("main") {
                        webview_memory::set_low_memory(&webview, false);
                    }
                }
                _ => {}
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
