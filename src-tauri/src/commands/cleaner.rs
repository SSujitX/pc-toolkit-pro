use pctoolkit_core::{
    cancel_cleanup as core_cancel, execute_cleanup as core_execute, execute_deep_cleanup,
    scan_cleanup_with_progress, scan_deep_cleanup_with_progress, CleanupExecuteRequest,
    CleanupResult, CleanupScan, DeepCleanupExecuteRequest, DeepCleanupResult, DeepCleanupScan,
};
use tauri::AppHandle;

use crate::commands::error::{run_blocking, CommandResult};
use crate::events;

pub const DEEP_CLEANUP_PROGRESS: &str = "deep-cleanup-progress";

#[tauri::command]
pub async fn scan_cleanup_candidates(app: AppHandle) -> CommandResult<CleanupScan> {
    run_blocking("scan_cleanup_candidates", move || {
        scan_cleanup_with_progress(|progress| {
            events::emit(&app, events::CLEANUP_PROGRESS, progress);
        })
    })
    .await
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
pub async fn scan_deep_cleanup(app: AppHandle) -> CommandResult<DeepCleanupScan> {
    run_blocking("scan_deep_cleanup", move || {
        scan_deep_cleanup_with_progress(|progress| {
            events::emit(&app, DEEP_CLEANUP_PROGRESS, progress);
        })
    })
    .await
}

#[tauri::command]
pub async fn execute_deep_cleanup_command(
    app: AppHandle,
    request: DeepCleanupExecuteRequest,
) -> CommandResult<DeepCleanupResult> {
    run_blocking("execute_deep_cleanup", move || {
        execute_deep_cleanup(request, |progress| {
            events::emit(&app, DEEP_CLEANUP_PROGRESS, progress);
        })
    })
    .await
}

#[tauri::command]
pub fn cancel_cleanup() {
    core_cancel();
}
