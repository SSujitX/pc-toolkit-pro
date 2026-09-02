use pctoolkit_core::HistoryRecord;

use crate::commands::error::CommandResult;

#[tauri::command]
pub fn list_history() -> CommandResult<Vec<HistoryRecord>> {
    Ok(pctoolkit_core::list_history())
}
