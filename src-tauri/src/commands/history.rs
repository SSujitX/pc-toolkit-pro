use pctoolkit_core::{
    clear_history as core_clear, list_history as core_list, CoreError, HistoryRecord,
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
