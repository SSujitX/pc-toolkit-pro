use pctoolkit_core::{
    cancel_cleanup as core_cancel, execute_cleanup as core_execute, scan_cleanup,
    CleanupExecuteRequest, CleanupResult, CleanupScan,
};
use tauri::AppHandle;

use crate::commands::error::{run_blocking, CommandResult};
use crate::events;

#[tauri::command]
pub async fn scan_cleanup_candidates() -> CommandResult<CleanupScan> {
    run_blocking("scan_cleanup_candidates", scan_cleanup).await
}

#[tauri::command]
pub async fn execute_cleanup(
    app: AppHandle,
    request: CleanupExecuteRequest,
) -> CommandResult<CleanupResult> {
    run_blocking("execute_cleanup", move || {
        core_execute(request, |progress| {
            events::emit(&app, events::CLEANUP_PROGRESS, progress);
        })
    })
    .await
}

#[tauri::command]
pub fn cancel_cleanup() {
    core_cancel();
}
