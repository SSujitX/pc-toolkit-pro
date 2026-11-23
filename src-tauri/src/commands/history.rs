use pctoolkit_core::{
    clear_history as core_clear, history_settings, list_history as core_list, set_history_settings,
    CoreError, HistoryRecord, HistorySettings,
};
use tauri::command;

use crate::commands::error::{run_blocking, CommandResult};

#[command]
pub async fn list_history() -> CommandResult<Vec<HistoryRecord>> {
    run_blocking("list_history", || Ok::<_, CoreError>(core_list())).await
}

#[command]
pub async fn clear_history() -> CommandResult<()> {
    run_blocking("clear_history", || {
        core_clear();
        Ok::<_, CoreError>(())
    })
    .await
}

#[command]
pub async fn get_history_settings() -> CommandResult<HistorySettings> {
    run_blocking("get_history_settings", || Ok::<_, CoreError>(history_settings())).await
}

/// Saves the retention limit (clamped to 1..=1000) and trims older records immediately.
#[command]
pub async fn update_history_settings(settings: HistorySettings) -> CommandResult<HistorySettings> {
    run_blocking("update_history_settings", move || {
        Ok::<_, CoreError>(set_history_settings(settings))
    })
    .await
}
