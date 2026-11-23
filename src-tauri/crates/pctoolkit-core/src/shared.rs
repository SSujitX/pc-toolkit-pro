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

/// Human-readable size for history summaries, e.g. `2.1 GB`, `512 KB`, `0 B`.
pub fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod size_tests {
    use super::format_size;

    #[test]
    fn formats_sizes_for_history() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(2_271_350_784), "2.1 GB");
        assert_eq!(format_size(6 * 1024 * 1024), "6.0 MB");
    }
}
