use crate::{PlatformError, PlatformResult};

pub fn empty_recycle_bin() -> PlatformResult<()> {
    #[cfg(windows)]
    {
        // Prefer PowerShell Clear-RecycleBin for broader Windows crate compatibility.
        let mut cmd = std::process::Command::new("powershell");
        cmd.args([
            "-NoProfile",
            "-WindowStyle",
            "Hidden",
            "-Command",
            "Clear-RecycleBin -Force -ErrorAction SilentlyContinue",
        ]);
        crate::process::hide_console(&mut cmd);
        let status = cmd
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
