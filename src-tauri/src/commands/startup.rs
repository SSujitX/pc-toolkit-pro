use pctoolkit_core::startup::{
    get_startup_icons, list_startup_items, run_startup_action, startup_item_reveal_path, StartupActionRequest,
    StartupActionResult, StartupIcon, StartupInventory, StartupListRequest,
};
use pctoolkit_core::CoreError;
use serde::Deserialize;
use tauri_plugin_opener::OpenerExt;

use crate::commands::error::{run_blocking, CommandError, CommandResult};

#[tauri::command]
pub async fn list_startup_items_command(request: StartupListRequest) -> CommandResult<StartupInventory> {
    run_blocking("list_startup_items", move || list_startup_items(request)).await
}

/// Enable / disable / delay / undelay / delete one item by id from the latest listing.
#[tauri::command]
pub async fn startup_item_action_command(request: StartupActionRequest) -> CommandResult<StartupActionResult> {
    run_blocking("startup_item_action", move || run_startup_action(request)).await
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StartupIconsRequest {
    pub ids: Vec<String>,
}

#[tauri::command]
pub async fn get_startup_icons_command(request: StartupIconsRequest) -> CommandResult<Vec<StartupIcon>> {
    run_blocking("get_startup_icons", move || Ok::<_, CoreError>(get_startup_icons(&request.ids))).await
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StartupItemIdRequest {
    pub id: String,
}

/// "Open file location": the path is resolved in Core from the listing (the frontend only sends
/// the id) and revealed with the Rust opener API, which `opener:default` permissions do not gate.
#[tauri::command]
pub async fn reveal_startup_item_command(app: tauri::AppHandle, request: StartupItemIdRequest) -> CommandResult<()> {
    let path = run_blocking("reveal_startup_item", move || startup_item_reveal_path(&request.id)).await?;
    app.opener()
        .reveal_item_in_dir(path)
        .map_err(|_| CommandError::from_core("reveal_startup_item", CoreError::OperationFailed("revealFailed".into())))
}
