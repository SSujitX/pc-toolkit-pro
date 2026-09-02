use pctoolkit_platform::{launch_program, QuickActionId};
use serde::Deserialize;

use crate::commands::error::{run_blocking, CommandResult};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OpenQuickActionRequest {
    pub action: QuickActionId,
}

#[tauri::command]
pub async fn open_quick_action(request: OpenQuickActionRequest) -> CommandResult<()> {
    run_blocking("open_quick_action", move || {
        launch_program(request.action).map_err(pctoolkit_core::CoreError::from)
    })
    .await
}
