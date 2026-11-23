use pctoolkit_core::{
    cancel_uninstaller as core_cancel, clean_leftovers, dismiss_removed_program, focus_uninstaller, get_program_icons,
    list_programs, ProgramIcon, scan_program_leftovers,
    uninstall_program, LeftoverCleanRequest, LeftoverCleanResult, LeftoverScan, ProgramInventory,
    UninstallReport, UninstallRequest,
};
use serde::Deserialize;
use tauri::AppHandle;

use crate::commands::error::{run_blocking, CommandResult};
use crate::events;

pub const UNINSTALLER_PROGRESS: &str = "uninstaller-progress";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProgramIdRequest {
    pub program_id: String,
}

#[tauri::command]
pub async fn list_installed_programs(app: AppHandle) -> CommandResult<ProgramInventory> {
    run_blocking("list_installed_programs", move || {
        list_programs(|progress| events::emit(&app, UNINSTALLER_PROGRESS, progress))
    })
    .await
}

#[tauri::command]
pub async fn uninstall_program_command(
    app: AppHandle,
    request: UninstallRequest,
) -> CommandResult<UninstallReport> {
    run_blocking("uninstall_program", move || {
        uninstall_program(request, |progress| {
            events::emit(&app, UNINSTALLER_PROGRESS, progress);
        })
    })
    .await
}

#[tauri::command]
pub async fn scan_program_leftovers_command(
    app: AppHandle,
    request: ProgramIdRequest,
) -> CommandResult<LeftoverScan> {
    run_blocking("scan_program_leftovers", move || {
        scan_program_leftovers(&request.program_id, |progress| {
            events::emit(&app, UNINSTALLER_PROGRESS, progress);
        })
    })
    .await
}

#[tauri::command]
pub async fn clean_uninstall_leftovers(
    app: AppHandle,
    request: LeftoverCleanRequest,
) -> CommandResult<LeftoverCleanResult> {
    run_blocking("clean_uninstall_leftovers", move || {
        clean_leftovers(request, |progress| {
            events::emit(&app, UNINSTALLER_PROGRESS, progress);
        })
    })
    .await
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProgramIconsRequest {
    pub program_ids: Vec<String>,
}

/// Icons are resolved lazily in batches so the list renders immediately.
#[tauri::command]
pub async fn get_program_icons_command(request: ProgramIconsRequest) -> CommandResult<Vec<ProgramIcon>> {
    run_blocking("get_program_icons", move || {
        Ok::<_, pctoolkit_core::CoreError>(get_program_icons(&request.program_ids))
    })
    .await
}

/// Bring the vendor uninstaller window to the front (it may be waiting for the user).
#[tauri::command]
pub async fn focus_uninstaller_window() -> CommandResult<bool> {
    run_blocking("focus_uninstaller_window", || {
        Ok::<_, pctoolkit_core::CoreError>(focus_uninstaller())
    })
    .await
}

/// Hide a program from the "removed by another uninstaller" list.
#[tauri::command]
pub async fn dismiss_removed_program_command(request: ProgramIdRequest) -> CommandResult<()> {
    run_blocking("dismiss_removed_program", move || {
        dismiss_removed_program(&request.program_id);
        Ok::<_, pctoolkit_core::CoreError>(())
    })
    .await
}

#[tauri::command]
pub fn cancel_uninstaller() {
    core_cancel();
}
