use thiserror::Error;

#[derive(Debug, Error)]
pub enum PlatformError {
    #[error("permission denied")]
    PermissionDenied,
    #[error("operation failed: {0}")]
    OperationFailed(String),
    #[error("unsupported on this platform")]
    Unsupported,
}

pub type PlatformResult<T> = Result<T, PlatformError>;
