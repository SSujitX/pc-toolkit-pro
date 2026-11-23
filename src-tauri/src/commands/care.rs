//! Care adapter: scan / fix / stop / skip over `pctoolkit_core::care::engine`.

use pctoolkit_core::care::engine::{
    fix_care as core_fix, scan_care as core_scan, skip_care_category as core_skip, stop_care as core_stop,
    CareFixReport, CareFixRequest, CareScanReport,
};
use pctoolkit_core::care::{care_action_uri, CareCategoryId};
use pctoolkit_core::CoreError;
use tauri::AppHandle;
use tauri_plugin_opener::OpenerExt;

use crate::commands::error::{run_blocking, CommandError, CommandResult};
use crate::events;

pub const CARE_PROGRESS: &str = "care-progress";

#[tauri::command]
pub async fn scan_care(app: AppHandle, categories: Vec<CareCategoryId>) -> CommandResult<CareScanReport> {
    run_blocking("scan_care", move || {
        core_scan(&categories, |progress| {
            events::emit(&app, CARE_PROGRESS, progress);
        })
    })
    .await
}

#[tauri::command]
pub async fn fix_care(app: AppHandle, request: CareFixRequest) -> CommandResult<CareFixReport> {
    run_blocking("fix_care", move || {
        core_fix(request, |progress| {
            events::emit(&app, CARE_PROGRESS, progress);
        })
    })
    .await
}

#[tauri::command]
pub fn stop_care() {
    core_stop();
}

#[tauri::command]
pub fn skip_care_category(category: CareCategoryId) {
    core_skip(category);
}

/// Open the Windows page behind an informational Care item (allowlisted ids only).
#[tauri::command]
pub fn open_care_action(app: AppHandle, action: String) -> CommandResult<()> {
    let uri = care_action_uri(&action)
        .ok_or_else(|| CommandError::from_core("open_care_action", CoreError::InvalidInput("unknown care action".into())))?;
    app.opener().open_url(uri, None::<&str>).map_err(|_| {
        CommandError::from_core("open_care_action", CoreError::OperationFailed("open care action failed".into()))
    })
}

/// Cancel a pending Auto fix shutdown / restart (`shutdown /a`).
#[tauri::command]
pub async fn cancel_care_after_action() -> CommandResult<()> {
    run_blocking("cancel_care_after_action", pctoolkit_core::care::engine::cancel_care_after_action).await
}

/// Undo journal of setting / registry changes made by Care fixes (newest first).
#[tauri::command]
pub async fn list_care_undo_records() -> CommandResult<Vec<pctoolkit_core::care::undo::CareUndoRecord>> {
    run_blocking("list_care_undo_records", pctoolkit_core::care::undo::list_care_undo_records).await
}

/// Restore the given undo records (changed-since-fix items are skipped, never overwritten).
#[tauri::command]
pub async fn undo_care_fixes(
    record_ids: Vec<String>,
) -> CommandResult<Vec<pctoolkit_core::care::undo::CareUndoOutcome>> {
    run_blocking("undo_care_fixes", move || {
        pctoolkit_core::care::undo::undo_care_fixes(&record_ids, pctoolkit_platform::is_user_admin())
    })
    .await
}
