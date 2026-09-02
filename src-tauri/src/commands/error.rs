use pctoolkit_core::{CoreError, CoreErrorCode};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CommandErrorCode {
    InvalidInput,
    OperationBusy,
    OperationCancelled,
    OperationFailed,
    PermissionDenied,
    TaskJoinFailed,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandError {
    pub code: CommandErrorCode,
    pub details: BTreeMap<&'static str, String>,
    pub retryable: bool,
}

pub type CommandResult<T> = Result<T, CommandError>;

impl CommandError {
    pub fn from_core(operation: &'static str, error: CoreError) -> Self {
        let code = match error.code() {
            CoreErrorCode::InvalidInput => CommandErrorCode::InvalidInput,
            CoreErrorCode::OperationBusy => CommandErrorCode::OperationBusy,
            CoreErrorCode::OperationCancelled => CommandErrorCode::OperationCancelled,
            CoreErrorCode::PermissionDenied => CommandErrorCode::PermissionDenied,
            CoreErrorCode::OperationFailed => CommandErrorCode::OperationFailed,
        };
        let retryable = matches!(
            code,
            CommandErrorCode::OperationBusy | CommandErrorCode::OperationFailed
        );
        let mut details = BTreeMap::new();
        details.insert("operation", operation.to_string());
        Self {
            code,
            details,
            retryable,
        }
    }

    pub fn task_join(operation: &'static str, error: impl ToString) -> Self {
        let mut details = BTreeMap::new();
        details.insert("operation", operation.to_string());
        details.insert("error", error.to_string());
        Self {
            code: CommandErrorCode::TaskJoinFailed,
            details,
            retryable: true,
        }
    }
}

pub async fn run_blocking<T, E, F>(operation: &'static str, task: F) -> CommandResult<T>
where
    T: Send + 'static,
    E: Into<CoreError> + Send + 'static,
    F: FnOnce() -> Result<T, E> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(task)
        .await
        .map_err(|e| CommandError::task_join(operation, e))?
        .map_err(|e| CommandError::from_core(operation, e.into()))
}
