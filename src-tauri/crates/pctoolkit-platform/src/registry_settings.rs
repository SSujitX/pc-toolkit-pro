//! Typed registry *setting* values: read with their type, write, and re-import `.reg` backups.
//!
//! `registry.rs` renders values as text for matching; Care fixes need the exact type and data so a
//! previous value can be stored and restored byte-for-byte. Only `REG_DWORD`, `REG_SZ`, and
//! `REG_EXPAND_SZ` are writable here — other types are reported as [`RegistryData::Other`] so
//! callers refuse to change (and therefore never fail to restore) a value they cannot round-trip.

use serde::{Deserialize, Serialize};

use crate::registry::{RegistryKeyPath, RegistryView};
use crate::{PlatformError, PlatformResult};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "type", content = "data")]
pub enum RegistryData {
    Dword(u32),
    String(String),
    ExpandString(String),
    /// Present but of a type this module does not write (binary, multi-string, QWORD, …).
    Other(u32),
}

impl RegistryData {
    pub fn as_dword(&self) -> Option<u32> {
        match self {
            Self::Dword(v) => Some(*v),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(s) | Self::ExpandString(s) => Some(s.as_str()),
            _ => None,
        }
    }

    pub fn is_writable(&self) -> bool {
        !matches!(self, Self::Other(_))
    }
}

#[cfg(windows)]
mod imp {
    use super::*;
    use crate::registry::RegistryRoot;
    use windows::core::HSTRING;
    use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS, WIN32_ERROR};
    use windows::Win32::System::Registry::{
        RegCloseKey, RegCreateKeyExW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY,
        HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_QUERY_VALUE, KEY_SET_VALUE, KEY_WOW64_32KEY,
        KEY_WOW64_64KEY, REG_DWORD, REG_EXPAND_SZ, REG_OPTION_NON_VOLATILE, REG_SAM_FLAGS, REG_SZ,
        REG_VALUE_TYPE,
    };

    struct OwnedKey(HKEY);

    impl Drop for OwnedKey {
        fn drop(&mut self) {
            unsafe {
                let _ = RegCloseKey(self.0);
            }
        }
    }

    fn root_handle(root: RegistryRoot) -> HKEY {
        match root {
            RegistryRoot::LocalMachine => HKEY_LOCAL_MACHINE,
            RegistryRoot::CurrentUser => HKEY_CURRENT_USER,
        }
    }

    fn view_flag(view: RegistryView) -> REG_SAM_FLAGS {
        match view {
            RegistryView::Native64 => KEY_WOW64_64KEY,
            RegistryView::Wow32 => KEY_WOW64_32KEY,
        }
    }

    fn map_error(code: WIN32_ERROR) -> PlatformError {
        if code.0 == 5 {
            PlatformError::PermissionDenied
        } else {
            PlatformError::OperationFailed(format!("registryError:{}", code.0))
        }
    }

    fn decode_wide(data: &[u8]) -> String {
        let wide: Vec<u16> = data.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        let end = wide.iter().position(|c| *c == 0).unwrap_or(wide.len());
        String::from_utf16_lossy(&wide[..end])
    }

    pub fn read(path: &RegistryKeyPath, name: &str) -> PlatformResult<Option<RegistryData>> {
        let mut key = HKEY::default();
        let subkey = HSTRING::from(path.subkey.as_str());
        let status = unsafe {
            RegOpenKeyExW(root_handle(path.root), &subkey, Some(0), KEY_QUERY_VALUE | view_flag(path.view), &mut key)
        };
        if status == ERROR_FILE_NOT_FOUND {
            return Ok(None);
        }
        if status != ERROR_SUCCESS {
            return Err(map_error(status));
        }
        let key = OwnedKey(key);
        let value_name = HSTRING::from(name);
        let mut kind = REG_VALUE_TYPE::default();
        let mut size = 0u32;
        let status = unsafe { RegQueryValueExW(key.0, &value_name, None, Some(&mut kind), None, Some(&mut size)) };
        if status == ERROR_FILE_NOT_FOUND {
            return Ok(None);
        }
        if status != ERROR_SUCCESS {
            return Err(map_error(status));
        }
        if size > 64 * 1024 {
            return Ok(Some(RegistryData::Other(kind.0)));
        }
        let mut data = vec![0u8; size as usize];
        let status = unsafe {
            RegQueryValueExW(key.0, &value_name, None, Some(&mut kind), Some(data.as_mut_ptr()), Some(&mut size))
        };
        if status == ERROR_FILE_NOT_FOUND {
            return Ok(None);
        }
        if status != ERROR_SUCCESS {
            return Err(map_error(status));
        }
        data.truncate(size as usize);
        Ok(Some(if kind == REG_DWORD && data.len() >= 4 {
            RegistryData::Dword(u32::from_le_bytes([data[0], data[1], data[2], data[3]]))
        } else if kind == REG_SZ {
            RegistryData::String(decode_wide(&data))
        } else if kind == REG_EXPAND_SZ {
            RegistryData::ExpandString(decode_wide(&data))
        } else {
            RegistryData::Other(kind.0)
        }))
    }

    pub fn write(path: &RegistryKeyPath, name: &str, value: &RegistryData) -> PlatformResult<()> {
        let (kind, bytes): (REG_VALUE_TYPE, Vec<u8>) = match value {
            RegistryData::Dword(v) => (REG_DWORD, v.to_le_bytes().to_vec()),
            RegistryData::String(s) | RegistryData::ExpandString(s) => {
                let bytes = s.encode_utf16().chain(std::iter::once(0)).flat_map(u16::to_le_bytes).collect();
                let kind = if matches!(value, RegistryData::String(_)) { REG_SZ } else { REG_EXPAND_SZ };
                (kind, bytes)
            }
            RegistryData::Other(_) => {
                return Err(PlatformError::OperationFailed("registryTypeUnsupported".into()))
            }
        };
        let mut key = HKEY::default();
        let subkey = HSTRING::from(path.subkey.as_str());
        let status = unsafe {
            RegCreateKeyExW(
                root_handle(path.root),
                &subkey,
                None,
                None,
                REG_OPTION_NON_VOLATILE,
                KEY_SET_VALUE | view_flag(path.view),
                None,
                &mut key,
                None,
            )
        };
        if status != ERROR_SUCCESS {
            return Err(map_error(status));
        }
        let key = OwnedKey(key);
        let value_name = HSTRING::from(name);
        let status = unsafe { RegSetValueExW(key.0, &value_name, None, kind, Some(&bytes)) };
        if status == ERROR_SUCCESS {
            Ok(())
        } else {
            Err(map_error(status))
        }
    }
}

/// Read one value with its type. `Ok(None)` = key or value absent; access denied is an error so
/// callers can tell "absent" from "unknown".
pub fn read_registry_data(path: &RegistryKeyPath, name: &str) -> PlatformResult<Option<RegistryData>> {
    #[cfg(windows)]
    {
        imp::read(path, name)
    }
    #[cfg(not(windows))]
    {
        let _ = (path, name);
        Err(PlatformError::Unsupported)
    }
}

/// Write one value, creating the key when missing. HKLM writes require elevation and fail with
/// `PermissionDenied` otherwise (never elevated silently).
pub fn write_registry_data(path: &RegistryKeyPath, name: &str, value: &RegistryData) -> PlatformResult<()> {
    #[cfg(windows)]
    {
        imp::write(path, name, value)
    }
    #[cfg(not(windows))]
    {
        let _ = (path, name, value);
        Err(PlatformError::Unsupported)
    }
}

/// Re-import a `.reg` backup written by `export_registry_key` (`reg.exe import`, same view).
pub fn import_registry_file(file: &std::path::Path, view: RegistryView) -> PlatformResult<()> {
    #[cfg(windows)]
    {
        use std::process::Command;
        let mut cmd = Command::new(crate::registry::system32_exe("reg.exe"));
        cmd.arg("import").arg(file).arg(match view {
            RegistryView::Native64 => "/reg:64",
            RegistryView::Wow32 => "/reg:32",
        });
        crate::hide_console(&mut cmd);
        let status = cmd
            .output()
            .map_err(|e| PlatformError::OperationFailed(format!("regImportSpawn:{}", e.kind())))?
            .status;
        if status.success() {
            Ok(())
        } else {
            Err(PlatformError::OperationFailed(format!("regImportExit:{}", status.code().unwrap_or(-1))))
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (file, view);
        Err(PlatformError::Unsupported)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_data_serde_round_trip() {
        for value in [
            RegistryData::Dword(5),
            RegistryData::String("Warn".into()),
            RegistryData::ExpandString("%SystemRoot%".into()),
            RegistryData::Other(3),
        ] {
            let json = serde_json::to_string(&value).unwrap();
            assert_eq!(serde_json::from_str::<RegistryData>(&json).unwrap(), value);
        }
        assert!(!RegistryData::Other(3).is_writable());
    }
}
