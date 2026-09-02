use pctoolkit_core::{get_system_information_with_progress, SystemInfoProgress};
use tauri::AppHandle;

use crate::commands::error::{run_blocking, CommandResult};
use crate::events;

pub const SYSTEM_INFO_PROGRESS: &str = "system-info-progress";

#[tauri::command]
pub async fn get_system_information(
    app: AppHandle,
) -> CommandResult<pctoolkit_core::system_info::SystemInformationDto> {
    run_blocking("get_system_information", move || {
        get_system_information_with_progress(|progress: SystemInfoProgress| {
            events::emit(&app, SYSTEM_INFO_PROGRESS, progress);
        })
    })
    .await
}
