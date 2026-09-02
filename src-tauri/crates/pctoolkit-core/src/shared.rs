use pctoolkit_platform::PlatformError;
use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum CoreErrorCode {
    InvalidInput,
    OperationBusy,
    OperationCancelled,
    OperationFailed,
    PermissionDenied,
}

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("permission denied")]
    PermissionDenied,
    #[error("operation busy")]
    OperationBusy,
    #[error("operation cancelled")]
    OperationCancelled,
    #[error("operation failed: {0}")]
    OperationFailed(String),
    #[error("invalid input: {0}")]
    InvalidInput(String),
}

impl CoreError {
    pub fn code(&self) -> CoreErrorCode {
        match self {
            Self::PermissionDenied => CoreErrorCode::PermissionDenied,
            Self::OperationBusy => CoreErrorCode::OperationBusy,
            Self::OperationCancelled => CoreErrorCode::OperationCancelled,
            Self::OperationFailed(_) => CoreErrorCode::OperationFailed,
            Self::InvalidInput(_) => CoreErrorCode::InvalidInput,
        }
    }
}

impl From<PlatformError> for CoreError {
    fn from(value: PlatformError) -> Self {
        match value {
            PlatformError::PermissionDenied => Self::PermissionDenied,
            PlatformError::Unsupported => Self::OperationFailed("unsupported".into()),
            PlatformError::OperationFailed(msg) => Self::OperationFailed(msg),
        }
    }
}

pub type CoreResult<T> = Result<T, CoreError>;
