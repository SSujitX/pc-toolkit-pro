//! Physical/virtual memory stats and area-based optimization (Windows).
//!
//! Reimplements known Windows memory-list / cache operations from public docs and
//! observed behavior of similar tools. Does **not** copy GPL source.
//! Areas that lack privilege or OS support are skipped with an honest outcome.

use serde::{Deserialize, Serialize};

use crate::{PlatformError, PlatformResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MemoryArea {
    CombinedPageList,
    ModifiedFileCache,
    ModifiedPageList,
    RegistryCache,
    StandbyList,
    StandbyListLowPriority,
    SystemFileCache,
    WorkingSet,
}

impl MemoryArea {
    pub const ALL: [MemoryArea; 8] = [
        MemoryArea::WorkingSet,
        MemoryArea::SystemFileCache,
        MemoryArea::ModifiedPageList,
        MemoryArea::StandbyList,
        MemoryArea::StandbyListLowPriority,
        MemoryArea::CombinedPageList,
        MemoryArea::RegistryCache,
        MemoryArea::ModifiedFileCache,
    ];

    /// Deterministic optimize order (spec).
    pub const OPTIMIZE_ORDER: [MemoryArea; 8] = [
        MemoryArea::WorkingSet,
        MemoryArea::SystemFileCache,
        MemoryArea::ModifiedPageList,
        MemoryArea::StandbyList,
        MemoryArea::StandbyListLowPriority,
        MemoryArea::CombinedPageList,
        MemoryArea::RegistryCache,
        MemoryArea::ModifiedFileCache,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            MemoryArea::CombinedPageList => "combinedPageList",
            MemoryArea::ModifiedFileCache => "modifiedFileCache",
            MemoryArea::ModifiedPageList => "modifiedPageList",
            MemoryArea::RegistryCache => "registryCache",
            MemoryArea::StandbyList => "standbyList",
            MemoryArea::StandbyListLowPriority => "standbyListLowPriority",
            MemoryArea::SystemFileCache => "systemFileCache",
            MemoryArea::WorkingSet => "workingSet",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryStats {
    pub physical_total: u64,
    pub physical_avail: u64,
    pub physical_used: u64,
    pub physical_load_percent: f32,
    pub virtual_total: u64,
    pub virtual_avail: u64,
    pub virtual_used: u64,
    pub virtual_load_percent: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AreaStatus {
    Ok,
    Skipped,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AreaOutcome {
    pub id: MemoryArea,
    pub status: AreaStatus,
    /// Stable codes: skippedNeedAdmin | skippedUnsupportedOs | failed | cancelled
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryOptimizeResult {
    pub free_before: u64,
    pub free_after: u64,
    pub freed_bytes: u64,
    /// True when the process token could enable profile/quota privileges (typically admin).
    pub admin_optimizations: bool,
    pub areas: Vec<AreaOutcome>,
}

/// Sort selected areas into optimize order; standby mutex already resolved by caller.
pub fn ordered_areas(selected: &[MemoryArea]) -> Vec<MemoryArea> {
    MemoryArea::OPTIMIZE_ORDER
        .iter()
        .copied()
        .filter(|a| selected.contains(a))
        .collect()
}

pub fn free_physical_memory_bytes() -> u64 {
    memory_stats()
        .map(|s| s.physical_avail)
        .unwrap_or(0)
}

pub fn memory_stats() -> PlatformResult<MemoryStats> {
    #[cfg(windows)]
    {
        return windows_impl::memory_stats();
    }
    #[cfg(not(windows))]
    {
        Err(PlatformError::Unsupported)
    }
}

/// Optimize selected areas. Cooperative cancel between areas.
pub fn optimize_memory_areas(
    areas: &[MemoryArea],
    mut on_progress: impl FnMut(usize, usize, MemoryArea),
    mut should_cancel: impl FnMut() -> bool,
) -> PlatformResult<MemoryOptimizeResult> {
    #[cfg(windows)]
    {
        return windows_impl::optimize_memory_areas(areas, &mut on_progress, &mut should_cancel);
    }
    #[cfg(not(windows))]
    {
        let _ = (areas, &mut on_progress, &mut should_cancel);
        Err(PlatformError::Unsupported)
    }
}

/// Backward-compatible entry: default areas (all except low-priority standby).
pub fn optimize_memory() -> PlatformResult<MemoryOptimizeResult> {
    let defaults: Vec<MemoryArea> = MemoryArea::ALL
        .iter()
        .copied()
        .filter(|a| *a != MemoryArea::StandbyListLowPriority)
        .collect();
    optimize_memory_areas(&defaults, |_, _, _| {}, || false)
}

#[cfg(windows)]
mod windows_impl {
    use super::*;
    use std::mem::{size_of, zeroed};
    use std::ptr;

    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{
        CloseHandle, GetLastError, SetLastError, ERROR_NOT_ALL_ASSIGNED, GENERIC_READ, HANDLE,
        WIN32_ERROR,
    };
    use windows::Win32::Security::{
        AdjustTokenPrivileges, LookupPrivilegeValueW, LUID_AND_ATTRIBUTES, SE_PRIVILEGE_ENABLED,
        TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES, TOKEN_QUERY,
    };
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, FlushFileBuffers, FILE_FLAG_NO_BUFFERING, FILE_SHARE_READ, FILE_SHARE_WRITE,
        OPEN_EXISTING,
    };
    use windows::Win32::System::IO::DeviceIoControl;
    use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    const SYSTEM_FILE_CACHE_INFORMATION: u32 = 21;
    const SYSTEM_MEMORY_LIST_INFORMATION: u32 = 80;
    const SYSTEM_COMBINE_PHYSICAL_MEMORY_INFORMATION: u32 = 130;
    const SYSTEM_REGISTRY_RECONCILIATION_INFORMATION: u32 = 155;

    const MEMORY_EMPTY_WORKING_SETS: u32 = 2;
    const MEMORY_FLUSH_MODIFIED_LIST: u32 = 3;
    const MEMORY_PURGE_STANDBY_LIST: u32 = 4;
    const MEMORY_PURGE_LOW_PRIORITY_STANDBY_LIST: u32 = 5;

    /// NTSTATUS STATUS_PRIVILEGE_NOT_HELD
    const STATUS_PRIVILEGE_NOT_HELD: i32 = -1073741727; // 0xC0000061

    const FSCTL_DISCARD_VOLUME_CACHE: u32 = 0x0009_0054;
    const FSCTL_RESET_WRITE_ORDER: u32 = 0x0009_00F8;

    #[link(name = "ntdll")]
    extern "system" {
        fn NtSetSystemInformation(
            system_information_class: u32,
            system_information: *mut core::ffi::c_void,
            system_information_length: u32,
        ) -> i32;
    }

    #[repr(C)]
    struct MemoryCombineInformationEx {
        handle: isize,
        pages_combined: usize,
        flags: i64,
    }

    #[repr(C)]
    struct SystemFileCacheInformation64 {
        current_size: i64,
        peak_size: i64,
        page_fault_count: i64,
        minimum_working_set: i64,
        maximum_working_set: i64,
        current_size_including_transition_in_pages: i64,
        peak_size_including_transition_in_pages: i64,
        transition_repurpose_count: i64,
        flags: i64,
    }

    pub fn memory_stats() -> PlatformResult<MemoryStats> {
        unsafe {
            let mut status: MEMORYSTATUSEX = zeroed();
            status.dwLength = size_of::<MEMORYSTATUSEX>() as u32;
            GlobalMemoryStatusEx(&mut status)
                .map_err(|e| PlatformError::OperationFailed(e.to_string()))?;

            let physical_total = status.ullTotalPhys;
            let physical_avail = status.ullAvailPhys;
            let physical_used = physical_total.saturating_sub(physical_avail);
            let physical_load_percent = status.dwMemoryLoad as f32;

            let virtual_total = status.ullTotalPageFile;
            let virtual_avail = status.ullAvailPageFile;
            let virtual_used = virtual_total.saturating_sub(virtual_avail);
            let virtual_load_percent = if virtual_total == 0 {
                0.0
            } else {
                (virtual_used as f64 / virtual_total as f64 * 100.0) as f32
            };

            Ok(MemoryStats {
                physical_total,
                physical_avail,
                physical_used,
                physical_load_percent,
                virtual_total,
                virtual_avail,
                virtual_used,
                virtual_load_percent,
            })
        }
    }

    pub fn optimize_memory_areas(
        areas: &[MemoryArea],
        on_progress: &mut dyn FnMut(usize, usize, MemoryArea),
        should_cancel: &mut dyn FnMut() -> bool,
    ) -> PlatformResult<MemoryOptimizeResult> {
        let free_before = memory_stats().map(|s| s.physical_avail).unwrap_or(0);
        let profile_ok = enable_privilege("SeProfileSingleProcessPrivilege");
        let quota_ok = enable_privilege("SeIncreaseQuotaPrivilege");
        let elevated = profile_ok || quota_ok || crate::monitor::is_user_admin();

        // Always trim our own working set (safe, no special privilege).
        trim_current_working_set();

        let ordered = ordered_areas(areas);
        let total = ordered.len();
        let mut outcomes = Vec::with_capacity(total);

        for (index, area) in ordered.iter().copied().enumerate() {
            if should_cancel() {
                outcomes.push(AreaOutcome {
                    id: area,
                    status: AreaStatus::Skipped,
                    detail: Some("cancelled".into()),
                });
                for rest in ordered.iter().skip(index + 1).copied() {
                    outcomes.push(AreaOutcome {
                        id: rest,
                        status: AreaStatus::Skipped,
                        detail: Some("cancelled".into()),
                    });
                }
                break;
            }

            on_progress(index + 1, total, area);
            let outcome = run_area(area, profile_ok, quota_ok);
            outcomes.push(outcome);
        }

        // Brief settle so free-RAM delta is measurable.
        std::thread::sleep(std::time::Duration::from_millis(400));
        let free_after = memory_stats().map(|s| s.physical_avail).unwrap_or(free_before);
        let freed_bytes = free_after.saturating_sub(free_before);

        Ok(MemoryOptimizeResult {
            free_before,
            free_after,
            freed_bytes,
            admin_optimizations: elevated,
            areas: outcomes,
        })
    }

    fn run_area(area: MemoryArea, profile_ok: bool, quota_ok: bool) -> AreaOutcome {
        let result = match area {
            MemoryArea::WorkingSet => {
                if profile_ok {
                    nt_memory_list_command(MEMORY_EMPTY_WORKING_SETS)
                } else {
                    // Own-process trim already ran; system-wide EmptyWorkingSets needs admin.
                    Err("skippedNeedAdmin")
                }
            }
            MemoryArea::SystemFileCache => {
                if !quota_ok {
                    Err("skippedNeedAdmin")
                } else {
                    optimize_system_file_cache()
                }
            }
            MemoryArea::ModifiedPageList => {
                if !profile_ok {
                    Err("skippedNeedAdmin")
                } else {
                    nt_memory_list_command(MEMORY_FLUSH_MODIFIED_LIST)
                }
            }
            MemoryArea::StandbyList => {
                if !profile_ok {
                    Err("skippedNeedAdmin")
                } else {
                    nt_memory_list_command(MEMORY_PURGE_STANDBY_LIST)
                }
            }
            MemoryArea::StandbyListLowPriority => {
                if !profile_ok {
                    Err("skippedNeedAdmin")
                } else {
                    nt_memory_list_command(MEMORY_PURGE_LOW_PRIORITY_STANDBY_LIST)
                }
            }
            MemoryArea::CombinedPageList => {
                if !profile_ok {
                    Err("skippedNeedAdmin")
                } else {
                    optimize_combined_page_list()
                }
            }
            MemoryArea::RegistryCache => optimize_registry_cache(),
            MemoryArea::ModifiedFileCache => optimize_modified_file_cache(),
        };

        match result {
            Ok(()) => AreaOutcome {
                id: area,
                status: AreaStatus::Ok,
                detail: None,
            },
            Err(code) if code == "skippedNeedAdmin" || code == "skippedUnsupportedOs" => {
                AreaOutcome {
                    id: area,
                    status: AreaStatus::Skipped,
                    detail: Some(code.into()),
                }
            }
            Err(code) => AreaOutcome {
                id: area,
                status: AreaStatus::Failed,
                detail: Some(code.into()),
            },
        }
    }

    fn map_nt_status(status: i32) -> Result<(), &'static str> {
        if status == 0 {
            Ok(())
        } else if status == STATUS_PRIVILEGE_NOT_HELD {
            Err("skippedNeedAdmin")
        } else {
            Err("failed")
        }
    }

    fn nt_memory_list_command(command: u32) -> Result<(), &'static str> {
        unsafe {
            let mut cmd = command;
            let status = NtSetSystemInformation(
                SYSTEM_MEMORY_LIST_INFORMATION,
                &mut cmd as *mut u32 as *mut _,
                size_of::<u32>() as u32,
            );
            map_nt_status(status)
        }
    }

    fn optimize_combined_page_list() -> Result<(), &'static str> {
        unsafe {
            let mut info = MemoryCombineInformationEx {
                handle: 0,
                pages_combined: 0,
                flags: 0,
            };
            let status = NtSetSystemInformation(
                SYSTEM_COMBINE_PHYSICAL_MEMORY_INFORMATION,
                &mut info as *mut _ as *mut _,
                size_of::<MemoryCombineInformationEx>() as u32,
            );
            map_nt_status(status)
        }
    }

    fn optimize_registry_cache() -> Result<(), &'static str> {
        unsafe {
            let status = NtSetSystemInformation(
                SYSTEM_REGISTRY_RECONCILIATION_INFORMATION,
                ptr::null_mut(),
                0,
            );
            map_nt_status(status)
        }
    }

    fn optimize_system_file_cache() -> Result<(), &'static str> {
        unsafe {
            let mut info: SystemFileCacheInformation64 = zeroed();
            info.minimum_working_set = -1;
            info.maximum_working_set = -1;
            let status = NtSetSystemInformation(
                SYSTEM_FILE_CACHE_INFORMATION,
                &mut info as *mut _ as *mut _,
                size_of::<SystemFileCacheInformation64>() as u32,
            );
            map_nt_status(status)?;
        }

        // SetSystemFileCacheSize(-1, -1, 0) flush — link from kernel32.
        #[link(name = "kernel32")]
        extern "system" {
            fn SetSystemFileCacheSize(
                minimum_file_cache_size: usize,
                maximum_file_cache_size: usize,
                flags: u32,
            ) -> i32;
        }
        unsafe {
            let flush = usize::MAX; // (SIZE_T)-1
            if SetSystemFileCacheSize(flush, flush, 0) == 0 {
                return Err("failed");
            }
        }
        Ok(())
    }

    fn optimize_modified_file_cache() -> Result<(), &'static str> {
        use std::ffi::OsStr;
        use std::os::windows::ffi::OsStrExt;

        let mut any_ok = false;
        for letter in b'A'..=b'Z' {
            let root = format!("{}:\\", letter as char);
            let meta = match std::fs::metadata(&root) {
                Ok(m) => m,
                Err(_) => continue,
            };
            if !meta.is_dir() {
                continue;
            }
            // Skip non-fixed roughly: only try if CreateFile volume succeeds.
            let volume = format!("\\\\.\\{}:", letter as char);
            let wide: Vec<u16> = OsStr::new(&volume)
                .encode_wide()
                .chain(std::iter::once(0))
                .collect();
            unsafe {
                let handle = CreateFileW(
                    PCWSTR(wide.as_ptr()),
                    GENERIC_READ.0,
                    FILE_SHARE_READ | FILE_SHARE_WRITE,
                    None,
                    OPEN_EXISTING,
                    FILE_FLAG_NO_BUFFERING,
                    None,
                );
                let Ok(handle) = handle else {
                    continue;
                };
                if handle.is_invalid() {
                    continue;
                }

                let mut returned = 0u32;
                let mut one = [0u8; 1];
                let _ = DeviceIoControl(
                    handle,
                    FSCTL_RESET_WRITE_ORDER,
                    Some(one.as_mut_ptr() as *const _ as *const _),
                    1,
                    None,
                    0,
                    Some(&mut returned),
                    None,
                );
                let _ = DeviceIoControl(
                    handle,
                    FSCTL_DISCARD_VOLUME_CACHE,
                    None,
                    0,
                    None,
                    0,
                    Some(&mut returned),
                    None,
                );
                if FlushFileBuffers(handle).is_ok() {
                    any_ok = true;
                }
                let _ = CloseHandle(handle);
            }
        }

        if any_ok {
            Ok(())
        } else {
            Err("failed")
        }
    }

    fn trim_current_working_set() {
        use windows::Win32::System::Threading::SetProcessWorkingSetSize;
        unsafe {
            let process = GetCurrentProcess();
            let _ = SetProcessWorkingSetSize(process, usize::MAX, usize::MAX);
        }
    }

    fn enable_privilege(name: &str) -> bool {
        unsafe {
            let mut token = HANDLE::default();
            if OpenProcessToken(
                GetCurrentProcess(),
                TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY,
                &mut token,
            )
            .is_err()
            {
                return false;
            }

            let mut luid = windows::Win32::Foundation::LUID::default();
            let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
            if LookupPrivilegeValueW(PCWSTR::null(), PCWSTR(wide.as_ptr()), &mut luid).is_err() {
                let _ = CloseHandle(token);
                return false;
            }

            let mut tp = TOKEN_PRIVILEGES {
                PrivilegeCount: 1,
                Privileges: [LUID_AND_ATTRIBUTES {
                    Luid: luid,
                    Attributes: SE_PRIVILEGE_ENABLED,
                }],
            };

            // AdjustTokenPrivileges can return success even when privileges were not assigned.
            SetLastError(WIN32_ERROR(0));
            if AdjustTokenPrivileges(token, false, Some(&mut tp), 0, None, None).is_err() {
                let _ = CloseHandle(token);
                return false;
            }
            let assigned = GetLastError() != ERROR_NOT_ALL_ASSIGNED;
            let _ = CloseHandle(token);
            assigned
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordered_areas_follows_spec_order() {
        let selected = [
            MemoryArea::RegistryCache,
            MemoryArea::WorkingSet,
            MemoryArea::StandbyList,
        ];
        assert_eq!(
            ordered_areas(&selected),
            vec![
                MemoryArea::WorkingSet,
                MemoryArea::StandbyList,
                MemoryArea::RegistryCache,
            ]
        );
    }
}
