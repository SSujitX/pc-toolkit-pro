use crate::commands::error::{run_blocking, CommandResult};

#[tauri::command]
pub async fn get_monitor_snapshot() -> CommandResult<pctoolkit_core::monitor::MonitorSnapshot> {
    run_blocking("get_monitor_snapshot", pctoolkit_core::get_monitor_snapshot).await
}
