use pctoolkit_platform::{
    close_processes, probe_running_processes, ProcessCloseBatchResult, RunningProcessGroup,
};
use serde::Deserialize;

use crate::commands::error::{run_blocking, CommandResult, CoreError};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProcessNamesRequest {
    pub names: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CloseProcessesRequest {
    pub names: Vec<String>,
    pub force: bool,
}

#[tauri::command]
pub async fn probe_running_processes_command(
    request: ProcessNamesRequest,
) -> CommandResult<Vec<RunningProcessGroup>> {
    run_blocking("probe_running_processes", move || {
        probe_running_processes(&request.names).map_err(CoreError::from)
    })
    .await
}

#[tauri::command]
pub async fn close_running_processes_command(
    request: CloseProcessesRequest,
) -> CommandResult<ProcessCloseBatchResult> {
    run_blocking("close_running_processes", move || {
        close_processes(&request.names, request.force).map_err(CoreError::from)
    })
    .await
}
