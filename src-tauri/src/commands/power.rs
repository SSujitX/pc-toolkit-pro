use pctoolkit_core::{
    cancel_scheduled_shutdown as core_cancel, execute_power, schedule_shutdown as core_schedule,
    PowerAction, ScheduleRequest,
};

use crate::commands::error::{run_blocking, CommandResult};

#[tauri::command]
pub async fn execute_power_action(action: PowerAction) -> CommandResult<()> {
    run_blocking("execute_power_action", move || execute_power(action)).await
}

#[tauri::command]
pub async fn schedule_shutdown(request: ScheduleRequest) -> CommandResult<()> {
    run_blocking("schedule_shutdown", move || core_schedule(request)).await
}

#[tauri::command]
pub async fn cancel_scheduled_shutdown() -> CommandResult<()> {
    run_blocking("cancel_scheduled_shutdown", core_cancel).await
}
