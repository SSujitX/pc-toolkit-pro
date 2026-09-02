use pctoolkit_core::{
    cancel_memory_optimize as core_cancel, get_memory_stats as core_stats, load_settings,
    optimize_memory as core_optimize, save_settings, CoreError, MemoryCleanerSettings,
    MemoryOptimizeResult, MemoryProgress, MemoryStats, OptimizeMemoryRequest,
};
use tauri::AppHandle;

use crate::commands::error::{run_blocking, CommandResult};
use crate::events;

pub const MEMORY_PROGRESS: &str = "memory-progress";

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
