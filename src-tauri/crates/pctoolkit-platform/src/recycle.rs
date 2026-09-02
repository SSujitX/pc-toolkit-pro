use crate::{PlatformError, PlatformResult};

pub fn empty_recycle_bin() -> PlatformResult<()> {
    #[cfg(windows)]
    {
        // Prefer PowerShell Clear-RecycleBin for broader Windows crate compatibility.
        let status = std::process::Command::new("powershell")
            .args([
                "-NoProfile",
                "-Command",
                "Clear-RecycleBin -Force -ErrorAction SilentlyContinue",
            ])
            .status()
            .map_err(|e| PlatformError::OperationFailed(e.to_string()))?;
        if status.success() {
            Ok(())
        } else {
            // Empty bin often returns non-zero when already empty.
            Ok(())
        }
    }
    #[cfg(not(windows))]
    {
        Err(PlatformError::Unsupported)
    }
}
