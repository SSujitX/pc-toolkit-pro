use pctoolkit_core::{
    cancel_memory_optimize as core_cancel, get_memory_stats as core_stats, load_settings,
    optimize_memory as core_optimize, save_settings, CoreError, MemoryCleanerSettings,
    MemoryOptimizeResult, MemoryProgress, MemoryStats, OptimizeMemoryRequest,
};
use pctoolkit_platform::{is_user_admin, relaunch_self_elevated};
use serde::Serialize;
use tauri::AppHandle;

use crate::commands::error::{run_blocking, CommandError, CommandErrorCode, CommandResult};
use crate::events;
use std::collections::BTreeMap;

pub const MEMORY_PROGRESS: &str = "memory-progress";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ElevationStatus {
    pub elevated: bool,
}

#[tauri::command]
pub fn get_elevation_status() -> ElevationStatus {
    ElevationStatus {
        elevated: is_user_admin(),
    }
}

/// Show UAC, start an elevated copy, then quit so single-instance can hand off.
#[tauri::command]
pub fn restart_as_administrator(app: AppHandle) -> CommandResult<()> {
    if is_user_admin() {
        return Ok(());
    }
    relaunch_self_elevated().map_err(|e| {
        let msg = e.to_string();
        let mut details = BTreeMap::new();
        details.insert("operation", "restart_as_administrator".to_string());
        details.insert("error", msg.clone());
        let code = if msg.contains("elevationCancelled") {
            CommandErrorCode::OperationCancelled
        } else {
            CommandErrorCode::OperationFailed
        };
        CommandError {
            code,
            details,
            retryable: matches!(code, CommandErrorCode::OperationFailed),
        }
    })?;
    // Release the single-instance lock so the elevated process can own the tray.
    app.exit(0);
    Ok(())
}

#[tauri::command]
pub async fn get_memory_stats() -> CommandResult<MemoryStats> {
    run_blocking("get_memory_stats", || core_stats()).await
}

#[tauri::command]
pub async fn get_memory_cleaner_settings() -> CommandResult<MemoryCleanerSettings> {
    run_blocking("get_memory_cleaner_settings", || {
        Ok::<_, CoreError>(load_settings())
    })
    .await
}

#[tauri::command]
pub async fn set_memory_cleaner_settings(
    settings: MemoryCleanerSettings,
) -> CommandResult<MemoryCleanerSettings> {
    run_blocking("set_memory_cleaner_settings", move || save_settings(settings)).await
}

#[tauri::command]
pub async fn optimize_memory(
    app: AppHandle,
    request: OptimizeMemoryRequest,
) -> CommandResult<MemoryOptimizeResult> {
    run_blocking("optimize_memory", move || {
        core_optimize(request, |progress: MemoryProgress| {
            events::emit(&app, MEMORY_PROGRESS, progress);
        })
    })
    .await
}

#[tauri::command]
pub fn cancel_memory_optimize() {
    core_cancel();
}
