//! Privacy trace facts: how many entries an Explorer MRU key holds, and where the per-user
//! Recent items folder is. Read-only; Core decides what is a trace and how it is removed
//! (`reg export` backup, then `delete_registry_tree`).

use std::path::PathBuf;

use crate::{PlatformError, PlatformResult, RegistryKeyPath};

/// Bookkeeping values of MRU keys; they order entries but are not entries themselves.
const MRU_BOOKKEEPING: [&str; 2] = ["MRUList", "MRUListEx"];

/// Maximum subkey depth walked when counting (OpenSavePidlMRU / RecentDocs nest one level).
const MAX_DEPTH: u32 = 3;

/// True when a value name counts as a trace entry (not the default value or MRU bookkeeping).
pub fn is_trace_value_name(name: &str) -> bool {
    !name.is_empty() && !MRU_BOOKKEEPING.iter().any(|b| b.eq_ignore_ascii_case(name))
}

/// Number of entry values under `path` and its subkeys (any value type, binary PIDLs included).
/// A missing key counts as zero.
pub fn registry_trace_count(path: &RegistryKeyPath) -> PlatformResult<u64> {
    #[cfg(windows)]
    {
        imp::count(path, 0)
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        Err(PlatformError::Unsupported)
    }
}

/// The current user's Recent items folder (`FOLDERID_Recent`, honours redirection).
pub fn recent_items_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        use windows::Win32::System::Com::CoTaskMemFree;
        use windows::Win32::UI::Shell::{FOLDERID_Recent, SHGetKnownFolderPath, KF_FLAG_DEFAULT};
        let raw = unsafe { SHGetKnownFolderPath(&FOLDERID_Recent, KF_FLAG_DEFAULT, None) }.ok()?;
        let path = unsafe { raw.to_string() }.ok();
        unsafe { CoTaskMemFree(Some(raw.0 as *const core::ffi::c_void)) };
        path.map(PathBuf::from)
    }
    #[cfg(not(windows))]
    {
        None
    }
}

#[cfg(windows)]
mod imp {
    use super::*;
    use crate::{RegistryRoot, RegistryView};
    use windows::core::{HSTRING, PWSTR};
    use windows::Win32::Foundation::{ERROR_ACCESS_DENIED, ERROR_FILE_NOT_FOUND, ERROR_NO_MORE_ITEMS, ERROR_SUCCESS};
    use windows::Win32::System::Registry::{
        RegCloseKey, RegEnumKeyExW, RegEnumValueW, RegOpenKeyExW, HKEY, HKEY_CURRENT_USER,
        HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_32KEY, KEY_WOW64_64KEY,
    };

    struct OwnedKey(HKEY);
    impl Drop for OwnedKey {
        fn drop(&mut self) {
            unsafe {
                let _ = RegCloseKey(self.0);
            }
        }
    }

    fn open(path: &RegistryKeyPath) -> PlatformResult<Option<OwnedKey>> {
        let root = match path.root {
            RegistryRoot::LocalMachine => HKEY_LOCAL_MACHINE,
            RegistryRoot::CurrentUser => HKEY_CURRENT_USER,
        };
        let view = match path.view {
            RegistryView::Native64 => KEY_WOW64_64KEY,
            RegistryView::Wow32 => KEY_WOW64_32KEY,
        };
        let mut key = HKEY::default();
        let subkey = HSTRING::from(path.subkey.as_str());
        let status = unsafe { RegOpenKeyExW(root, &subkey, Some(0), KEY_READ | view, &mut key) };
        if status == ERROR_SUCCESS {
            Ok(Some(OwnedKey(key)))
        } else if status == ERROR_FILE_NOT_FOUND {
            Ok(None)
        } else if status == ERROR_ACCESS_DENIED {
            Err(PlatformError::PermissionDenied)
        } else {
            Err(PlatformError::OperationFailed(format!("registryOpen:{}", status.0)))
        }
    }

    pub(super) fn count(path: &RegistryKeyPath, depth: u32) -> PlatformResult<u64> {
        let Some(key) = open(path)? else {
            return Ok(0);
        };
        let mut total = 0u64;
        let mut index = 0u32;
        loop {
            let mut name = [0u16; 16_384];
            let mut len = name.len() as u32;
            let status = unsafe {
                RegEnumValueW(key.0, index, Some(PWSTR(name.as_mut_ptr())), &mut len, None, None, None, None)
            };
            if status == ERROR_NO_MORE_ITEMS || index > 100_000 {
                break;
            }
            if status == ERROR_SUCCESS && is_trace_value_name(&String::from_utf16_lossy(&name[..len as usize])) {
                total += 1;
            }
            index += 1;
        }
        if depth >= MAX_DEPTH {
            return Ok(total);
        }
        let mut subkeys = Vec::new();
        let mut index = 0u32;
        loop {
            let mut name = [0u16; 256];
            let mut len = name.len() as u32;
            let status = unsafe {
                RegEnumKeyExW(key.0, index, Some(PWSTR(name.as_mut_ptr())), &mut len, None, None, None, None)
            };
            if status == ERROR_NO_MORE_ITEMS || index > 10_000 {
                break;
            }
            if status == ERROR_SUCCESS {
                subkeys.push(String::from_utf16_lossy(&name[..len as usize]));
            }
            index += 1;
        }
        drop(key);
        for sub in subkeys {
            // A denied child is skipped; its parent's count stays honest about what was readable.
            if let Ok(n) = count(&path.child(&sub), depth + 1) {
                total += n;
            }
        }
        Ok(total)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bookkeeping_values_are_not_entries() {
        assert!(!is_trace_value_name("MRUListEx"));
        assert!(!is_trace_value_name("mrulist"));
        assert!(!is_trace_value_name(""));
        assert!(is_trace_value_name("a"));
        assert!(is_trace_value_name("0"));
        assert!(is_trace_value_name("url1"));
    }

    #[cfg(windows)]
    #[test]
    fn missing_key_counts_zero() {
        use crate::{RegistryRoot, RegistryView};
        let path = RegistryKeyPath::new(
            RegistryRoot::CurrentUser,
            RegistryView::Native64,
            "Software\\PC Toolkit Pro\\DefinitelyMissingTestKey",
        );
        assert_eq!(registry_trace_count(&path).unwrap(), 0);
    }
}
