//! Narrow registry facts and mutations used by the Uninstaller domain.
//!
//! Paths are always passed as a typed root + subkey (never a free-form `HKEY_*` string) so callers
//! cannot accidentally target an unrelated hive. 64-bit and 32-bit views are explicit because
//! uninstall entries for x86 programs live under `WOW6432Node`.

use serde::{Deserialize, Serialize};

use crate::{PlatformError, PlatformResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RegistryRoot {
    LocalMachine,
    CurrentUser,
}

impl RegistryRoot {
    /// Short label used in exported `.reg` backups and UI paths.
    pub fn label(self) -> &'static str {
        match self {
            Self::LocalMachine => "HKEY_LOCAL_MACHINE",
            Self::CurrentUser => "HKEY_CURRENT_USER",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RegistryView {
    Native64,
    Wow32,
}

/// A fully-qualified registry key location.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegistryKeyPath {
    pub root: RegistryRoot,
    pub view: RegistryView,
    pub subkey: String,
}

impl RegistryKeyPath {
    pub fn new(root: RegistryRoot, view: RegistryView, subkey: impl Into<String>) -> Self {
        Self {
            root,
            view,
            subkey: subkey.into(),
        }
    }

    pub fn child(&self, name: &str) -> Self {
        Self::new(self.root, self.view, format!("{}\\{}", self.subkey, name))
    }

    /// Human-readable path (`HKEY_LOCAL_MACHINE\SOFTWARE\...`). The 32-bit view is shown with its
    /// real `WOW6432Node` location so users can find it in regedit.
    pub fn display(&self) -> String {
        let subkey = match self.view {
            RegistryView::Wow32 => wow_display_subkey(&self.subkey),
            RegistryView::Native64 => self.subkey.clone(),
        };
        format!("{}\\{}", self.root.label(), subkey)
    }
}

fn wow_display_subkey(subkey: &str) -> String {
    let lower = subkey.to_ascii_lowercase();
    if lower.starts_with("software\\") && !lower.starts_with("software\\wow6432node") {
        format!("SOFTWARE\\WOW6432Node\\{}", &subkey["software\\".len()..])
    } else {
        subkey.to_string()
    }
}

/// A registry value as text. Non-string types are rendered for display/matching only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryValueText {
    pub name: String,
    pub text: String,
}

#[cfg(windows)]
mod imp {
    use super::*;
    use windows::core::{HSTRING, PCWSTR, PWSTR};
    use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_NO_MORE_ITEMS, ERROR_SUCCESS, WIN32_ERROR};
    use windows::Win32::System::Registry::{
        RegCloseKey, RegDeleteTreeW, RegDeleteValueW, RegEnumKeyExW, RegEnumValueW, RegOpenKeyExW,
        RegQueryValueExW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_QUERY_VALUE, KEY_READ,
        KEY_WOW64_32KEY, KEY_WOW64_64KEY, KEY_WRITE, REG_DWORD, REG_EXPAND_SZ, REG_QWORD, REG_SAM_FLAGS,
        REG_SZ, REG_VALUE_TYPE,
    };

    /// Access mask for deleting a whole subtree (enumerate + delete children + write).
    const DELETE_ACCESS: u32 = 0x0001_0000;

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
        // ERROR_ACCESS_DENIED = 5
        if code.0 == 5 {
            PlatformError::PermissionDenied
        } else {
            PlatformError::OperationFailed(format!("registryError:{}", code.0))
        }
    }

    fn open(path: &RegistryKeyPath, access: REG_SAM_FLAGS) -> PlatformResult<Option<OwnedKey>> {
        let mut key = HKEY::default();
        let subkey = HSTRING::from(path.subkey.as_str());
        let status = unsafe {
            RegOpenKeyExW(
                root_handle(path.root),
                &subkey,
                Some(0),
                access | view_flag(path.view),
                &mut key,
            )
        };
        if status == ERROR_SUCCESS {
            Ok(Some(OwnedKey(key)))
        } else if status == ERROR_FILE_NOT_FOUND {
            Ok(None)
        } else {
            Err(map_error(status))
        }
    }

    /// Last write time of a key as Unix milliseconds (Explorer's fallback "installed on" date).
    pub fn key_last_write_ms(path: &RegistryKeyPath) -> Option<i64> {
        use windows::Win32::Foundation::FILETIME;
        use windows::Win32::System::Registry::RegQueryInfoKeyW;
        let key = open(path, KEY_QUERY_VALUE).ok().flatten()?;
        let mut time = FILETIME::default();
        let status = unsafe {
            RegQueryInfoKeyW(key.0, None, None, None, None, None, None, None, None, None, None, Some(&mut time))
        };
        if status != ERROR_SUCCESS {
            return None;
        }
        let ticks = (u64::from(time.dwHighDateTime) << 32) | u64::from(time.dwLowDateTime);
        // FILETIME: 100 ns ticks since 1601-01-01.
        Some((ticks as i64 - 116_444_736_000_000_000) / 10_000)
    }

    pub fn key_exists(path: &RegistryKeyPath) -> bool {
        matches!(open(path, KEY_QUERY_VALUE), Ok(Some(_)))
    }

    pub fn subkey_names(path: &RegistryKeyPath) -> PlatformResult<Vec<String>> {
        let Some(key) = open(path, KEY_READ)? else {
            return Ok(Vec::new());
        };
        let mut names = Vec::new();
        let mut index = 0u32;
        loop {
            let mut buf = [0u16; 512];
            let mut len = buf.len() as u32;
            let status = unsafe {
                RegEnumKeyExW(
                    key.0,
                    index,
                    Some(PWSTR(buf.as_mut_ptr())),
                    &mut len,
                    None,
                    None,
                    None,
                    None,
                )
            };
            if status == ERROR_NO_MORE_ITEMS {
                break;
            }
            if status == ERROR_SUCCESS {
                names.push(String::from_utf16_lossy(&buf[..len as usize]));
            }
            index += 1;
            if index > 100_000 {
                break;
            }
        }
        Ok(names)
    }

    fn render_value(kind: REG_VALUE_TYPE, data: &[u8]) -> Option<String> {
        if kind == REG_SZ || kind == REG_EXPAND_SZ {
            let wide: Vec<u16> = data
                .chunks_exact(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .collect();
            let end = wide.iter().position(|c| *c == 0).unwrap_or(wide.len());
            Some(String::from_utf16_lossy(&wide[..end]).trim().to_string())
        } else if kind == REG_DWORD && data.len() >= 4 {
            Some(u32::from_le_bytes([data[0], data[1], data[2], data[3]]).to_string())
        } else if kind == REG_QWORD && data.len() >= 8 {
            let mut bytes = [0u8; 8];
            bytes.copy_from_slice(&data[..8]);
            Some(u64::from_le_bytes(bytes).to_string())
        } else {
            None
        }
    }

    pub fn read_value(path: &RegistryKeyPath, name: &str) -> Option<String> {
        let key = open(path, KEY_QUERY_VALUE).ok().flatten()?;
        let value_name = HSTRING::from(name);
        let mut kind = REG_VALUE_TYPE::default();
        let mut size = 0u32;
        let status = unsafe {
            RegQueryValueExW(key.0, &value_name, None, Some(&mut kind), None, Some(&mut size))
        };
        if status != ERROR_SUCCESS || size == 0 || size > 64 * 1024 {
            return None;
        }
        let mut data = vec![0u8; size as usize];
        let status = unsafe {
            RegQueryValueExW(
                key.0,
                &value_name,
                None,
                Some(&mut kind),
                Some(data.as_mut_ptr()),
                Some(&mut size),
            )
        };
        if status != ERROR_SUCCESS {
            return None;
        }
        data.truncate(size as usize);
        render_value(kind, &data)
    }

    /// Names of every value under `path`, whatever their type (binary values included).
    pub fn value_names(path: &RegistryKeyPath) -> PlatformResult<Vec<String>> {
        let Some(key) = open(path, KEY_READ)? else {
            return Ok(Vec::new());
        };
        let mut out = Vec::new();
        let mut index = 0u32;
        loop {
            let mut name = [0u16; 16_384];
            let mut name_len = name.len() as u32;
            let status = unsafe {
                RegEnumValueW(key.0, index, Some(PWSTR(name.as_mut_ptr())), &mut name_len, None, None, None, None)
            };
            if status == ERROR_NO_MORE_ITEMS {
                break;
            }
            if status == ERROR_SUCCESS {
                out.push(String::from_utf16_lossy(&name[..name_len as usize]));
            }
            index += 1;
            if index > 100_000 {
                break;
            }
        }
        Ok(out)
    }

    pub fn values(path: &RegistryKeyPath) -> PlatformResult<Vec<RegistryValueText>> {
        let Some(key) = open(path, KEY_READ)? else {
            return Ok(Vec::new());
        };
        let mut out = Vec::new();
        let mut index = 0u32;
        loop {
            let mut name = [0u16; 16_384];
            let mut name_len = name.len() as u32;
            let mut kind = 0u32;
            let mut data = vec![0u8; 8192];
            let mut data_len = data.len() as u32;
            let status = unsafe {
                RegEnumValueW(
                    key.0,
                    index,
                    Some(PWSTR(name.as_mut_ptr())),
                    &mut name_len,
                    None,
                    Some(&mut kind),
                    Some(data.as_mut_ptr()),
                    Some(&mut data_len),
                )
            };
            if status == ERROR_NO_MORE_ITEMS {
                break;
            }
            if status == ERROR_SUCCESS {
                data.truncate(data_len as usize);
                if let Some(text) = render_value(REG_VALUE_TYPE(kind), &data) {
                    out.push(RegistryValueText {
                        name: String::from_utf16_lossy(&name[..name_len as usize]),
                        text,
                    });
                }
            }
            index += 1;
            if index > 100_000 {
                break;
            }
        }
        Ok(out)
    }

    pub fn delete_tree(path: &RegistryKeyPath) -> PlatformResult<()> {
        let (parent, leaf) = split_parent(&path.subkey)?;
        let parent_path = RegistryKeyPath::new(path.root, path.view, parent);
        let Some(parent_key) =
            open(&parent_path, KEY_READ | KEY_WRITE | REG_SAM_FLAGS(DELETE_ACCESS))?
        else {
            return Ok(());
        };
        let leaf = HSTRING::from(leaf);
        let status = unsafe { RegDeleteTreeW(parent_key.0, &leaf) };
        if status == ERROR_SUCCESS || status == ERROR_FILE_NOT_FOUND {
            // RegDeleteTreeW removes children only when the leaf is passed relative to its parent;
            // the leaf key itself is removed as part of the same call.
            Ok(())
        } else {
            Err(map_error(status))
        }
    }

    pub fn delete_value(path: &RegistryKeyPath, name: &str) -> PlatformResult<()> {
        let Some(key) = open(path, KEY_WRITE)? else {
            return Ok(());
        };
        let name = HSTRING::from(name);
        let status = unsafe { RegDeleteValueW(key.0, PCWSTR(name.as_ptr())) };
        if status == ERROR_SUCCESS || status == ERROR_FILE_NOT_FOUND {
            Ok(())
        } else {
            Err(map_error(status))
        }
    }

    fn split_parent(subkey: &str) -> PlatformResult<(&str, &str)> {
        let trimmed = subkey.trim_end_matches('\\');
        let idx = trimmed
            .rfind('\\')
            .ok_or_else(|| PlatformError::OperationFailed("registryRootDeleteRefused".into()))?;
        Ok((&trimmed[..idx], &trimmed[idx + 1..]))
    }
}

#[cfg(windows)]
pub use imp::{delete_tree, delete_value, key_exists, key_last_write_ms, read_value, subkey_names, value_names, values};

#[cfg(not(windows))]
pub fn key_exists(_path: &RegistryKeyPath) -> bool {
    false
}
#[cfg(not(windows))]
pub fn key_last_write_ms(_path: &RegistryKeyPath) -> Option<i64> {
    None
}
#[cfg(not(windows))]
pub fn subkey_names(_path: &RegistryKeyPath) -> PlatformResult<Vec<String>> {
    Err(PlatformError::Unsupported)
}
#[cfg(not(windows))]
pub fn read_value(_path: &RegistryKeyPath, _name: &str) -> Option<String> {
    None
}
#[cfg(not(windows))]
pub fn value_names(_path: &RegistryKeyPath) -> PlatformResult<Vec<String>> {
    Err(PlatformError::Unsupported)
}
#[cfg(not(windows))]
pub fn values(_path: &RegistryKeyPath) -> PlatformResult<Vec<RegistryValueText>> {
    Err(PlatformError::Unsupported)
}
#[cfg(not(windows))]
pub fn delete_tree(_path: &RegistryKeyPath) -> PlatformResult<()> {
    Err(PlatformError::Unsupported)
}
#[cfg(not(windows))]
pub fn delete_value(_path: &RegistryKeyPath, _name: &str) -> PlatformResult<()> {
    Err(PlatformError::Unsupported)
}

/// Export a key (and its subtree) to a `.reg` file with the built-in `reg.exe` so a deleted entry
/// can be restored by double-clicking the backup. The 32-bit view is exported with `/reg:32`.
pub fn export_key(path: &RegistryKeyPath, target: &std::path::Path) -> PlatformResult<()> {
    #[cfg(windows)]
    {
        use std::process::Command;
        let mut cmd = Command::new(system32_exe("reg.exe"));
        cmd.arg("export")
            .arg(format!("{}\\{}", path.root.label(), path.subkey))
            .arg(target)
            .arg("/y");
        cmd.arg(match path.view {
            RegistryView::Native64 => "/reg:64",
            RegistryView::Wow32 => "/reg:32",
        });
        crate::hide_console(&mut cmd);
        // output() (not status()) so reg.exe's console text is captured instead of inherited.
        let status = cmd
            .output()
            .map_err(|e| PlatformError::OperationFailed(format!("regExportSpawn:{}", e.kind())))?
            .status;
        if status.success() {
            Ok(())
        } else {
            Err(PlatformError::OperationFailed(format!(
                "regExportExit:{}",
                status.code().unwrap_or(-1)
            )))
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (path, target);
        Err(PlatformError::Unsupported)
    }
}

/// Absolute `%SystemRoot%\System32\<name>` path for an OS tool.
///
/// Spawning a bare name (`"cmd"`, `"taskkill"`) lets `CreateProcess` pick up an exe planted
/// next to our binary or in the current directory before System32, which is dangerous when
/// the app runs elevated. Every OS tool is therefore launched by absolute path.
pub fn system32_exe(name: &str) -> std::path::PathBuf {
    let root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
    std::path::PathBuf::from(root).join("System32").join(name)
}

/// Absolute path of Windows PowerShell 5.1 (`System32\WindowsPowerShell\v1.0`).
pub(crate) fn powershell_exe() -> std::path::PathBuf {
    system32_exe("WindowsPowerShell").join("v1.0").join("powershell.exe")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wow_display_inserts_wow6432node_once() {
        let path = RegistryKeyPath::new(
            RegistryRoot::LocalMachine,
            RegistryView::Wow32,
            "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\App",
        );
        assert_eq!(
            path.display(),
            "HKEY_LOCAL_MACHINE\\SOFTWARE\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\App"
        );
        let native = RegistryKeyPath::new(RegistryRoot::CurrentUser, RegistryView::Native64, "Software\\Vendor");
        assert_eq!(native.display(), "HKEY_CURRENT_USER\\Software\\Vendor");
    }
}
