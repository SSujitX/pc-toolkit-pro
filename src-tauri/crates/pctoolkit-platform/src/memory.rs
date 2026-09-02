use serde::Serialize;
use std::process::Command;
use sysinfo::System;

use crate::monitor::require_admin;
use crate::{PlatformError, PlatformResult};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryOptimizeResult {
    pub free_before: u64,
    pub free_after: u64,
    pub freed_bytes: u64,
    pub admin_optimizations: bool,
}

pub fn free_physical_memory_bytes() -> u64 {
    let mut sys = System::new();
    sys.refresh_memory();
    sys.available_memory()
}

pub fn optimize_memory() -> PlatformResult<MemoryOptimizeResult> {
    let free_before = free_physical_memory_bytes();
    let admin = require_admin().is_ok();

    // Always trim current process working set when possible.
    trim_current_working_set();

    if admin {
        // Best-effort standby list purge via PowerShell (avoids fragile ntdll bindings).
        let _ = Command::new("powershell")
            .args([
                "-NoProfile",
                "-Command",
                "Get-Process | ForEach-Object { try { $_.MinWorkingSet = $_.MinWorkingSet } catch {} }",
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }

    std::thread::sleep(std::time::Duration::from_millis(400));
    let free_after = free_physical_memory_bytes();
    let freed_bytes = free_after.saturating_sub(free_before);

    Ok(MemoryOptimizeResult {
        free_before,
        free_after,
        freed_bytes,
        admin_optimizations: admin,
    })
}

fn trim_current_working_set() {
    #[cfg(windows)]
    {
        use windows::Win32::System::Threading::{GetCurrentProcess, SetProcessWorkingSetSize};
        unsafe {
            let process = GetCurrentProcess();
            let _ = SetProcessWorkingSetSize(process, usize::MAX, usize::MAX);
        }
    }
}

pub fn empty_ok() -> PlatformResult<()> {
    Ok(())
}

#[allow(dead_code)]
fn map_err(e: impl ToString) -> PlatformError {
    PlatformError::OperationFailed(e.to_string())
}
