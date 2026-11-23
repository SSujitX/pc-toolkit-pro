//! File-system mutations and Shell facts used by leftover cleanup.

use std::path::{Path, PathBuf};

use crate::{PlatformError, PlatformResult};

/// Error code returned by [`recycle_path`] when the shell would bypass the Recycle Bin.
pub const WOULD_DELETE_PERMANENTLY: &str = "wouldDeletePermanently";

/// Move a file or folder to the Recycle Bin so the user can restore it.
///
/// Never deletes permanently. `SHFileOperation` with `FOF_ALLOWUNDO | FOF_NOCONFIRMATION`
/// silently destroys items that do not fit the bin (larger than its quota, a volume without a
/// Recycle Bin), and `FOF_WANTNUKEWARNING` would block a background clean on a dialog. Instead
/// `IFileOperation` runs without UI and a progress sink inspects `PreDeleteItem`: when the copy
/// engine does not offer `TSF_DELETE_RECYCLE_IF_POSSIBLE`, the delete is aborted before any
/// data is removed and `OperationFailed(WOULD_DELETE_PERMANENTLY)` is returned so callers
/// report the item honestly.
pub fn recycle_path(path: &Path) -> PlatformResult<()> {
    #[cfg(windows)]
    {
        recycle_imp::recycle(path)
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        Err(PlatformError::Unsupported)
    }
}

/// Whether `PreDeleteItem` flags say the item goes to the Recycle Bin (pure, testable).
#[cfg_attr(not(windows), allow(dead_code))]
fn delete_flags_recycle(flags: u32) -> bool {
    /// `TSF_DELETE_RECYCLE_IF_POSSIBLE` (shobjidl.h).
    const TSF_DELETE_RECYCLE_IF_POSSIBLE: u32 = 0x0080;
    flags & TSF_DELETE_RECYCLE_IF_POSSIBLE != 0
}

#[cfg(windows)]
mod recycle_imp {
    use std::cell::Cell;
    use std::path::Path;
    use std::rc::Rc;

    use windows::core::{implement, Ref, Result as WinResult, HRESULT, HSTRING, PCWSTR};
    use windows::Win32::Foundation::E_ABORT;
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL, COINIT_APARTMENTTHREADED,
    };
    use windows::Win32::UI::Shell::{
        FileOperation, IFileOperation, IFileOperationProgressSink, IFileOperationProgressSink_Impl,
        IShellItem, SHCreateItemFromParsingName, FOFX_EARLYFAILURE, FOFX_RECYCLEONDELETE, FOF_ALLOWUNDO,
        FOF_NO_UI,
    };

    use super::{delete_flags_recycle, WOULD_DELETE_PERMANENTLY};
    use crate::{PlatformError, PlatformResult};

    /// Aborts any delete the copy engine would perform without the Recycle Bin.
    #[implement(IFileOperationProgressSink)]
    struct RecycleOnlySink {
        would_delete_permanently: Rc<Cell<bool>>,
    }

    #[allow(non_snake_case)]
    impl IFileOperationProgressSink_Impl for RecycleOnlySink_Impl {
        fn StartOperations(&self) -> WinResult<()> {
            Ok(())
        }
        fn FinishOperations(&self, _hr: HRESULT) -> WinResult<()> {
            Ok(())
        }
        fn PreRenameItem(&self, _f: u32, _i: Ref<'_, IShellItem>, _n: &PCWSTR) -> WinResult<()> {
            Ok(())
        }
        fn PostRenameItem(
            &self,
            _f: u32,
            _i: Ref<'_, IShellItem>,
            _n: &PCWSTR,
            _hr: HRESULT,
            _c: Ref<'_, IShellItem>,
        ) -> WinResult<()> {
            Ok(())
        }
        fn PreMoveItem(&self, _f: u32, _i: Ref<'_, IShellItem>, _d: Ref<'_, IShellItem>, _n: &PCWSTR) -> WinResult<()> {
            Ok(())
        }
        fn PostMoveItem(
            &self,
            _f: u32,
            _i: Ref<'_, IShellItem>,
            _d: Ref<'_, IShellItem>,
            _n: &PCWSTR,
            _hr: HRESULT,
            _c: Ref<'_, IShellItem>,
        ) -> WinResult<()> {
            Ok(())
        }
        fn PreCopyItem(&self, _f: u32, _i: Ref<'_, IShellItem>, _d: Ref<'_, IShellItem>, _n: &PCWSTR) -> WinResult<()> {
            Ok(())
        }
        fn PostCopyItem(
            &self,
            _f: u32,
            _i: Ref<'_, IShellItem>,
            _d: Ref<'_, IShellItem>,
            _n: &PCWSTR,
            _hr: HRESULT,
            _c: Ref<'_, IShellItem>,
        ) -> WinResult<()> {
            Ok(())
        }
        fn PreDeleteItem(&self, flags: u32, _i: Ref<'_, IShellItem>) -> WinResult<()> {
            if delete_flags_recycle(flags) {
                Ok(())
            } else {
                self.would_delete_permanently.set(true);
                Err(E_ABORT.into())
            }
        }
        fn PostDeleteItem(&self, _f: u32, _i: Ref<'_, IShellItem>, _hr: HRESULT, _c: Ref<'_, IShellItem>) -> WinResult<()> {
            Ok(())
        }
        fn PreNewItem(&self, _f: u32, _d: Ref<'_, IShellItem>, _n: &PCWSTR) -> WinResult<()> {
            Ok(())
        }
        fn PostNewItem(
            &self,
            _f: u32,
            _d: Ref<'_, IShellItem>,
            _n: &PCWSTR,
            _t: &PCWSTR,
            _a: u32,
            _hr: HRESULT,
            _c: Ref<'_, IShellItem>,
        ) -> WinResult<()> {
            Ok(())
        }
        fn UpdateProgress(&self, _total: u32, _done: u32) -> WinResult<()> {
            Ok(())
        }
        fn ResetTimer(&self) -> WinResult<()> {
            Ok(())
        }
        fn PauseTimer(&self) -> WinResult<()> {
            Ok(())
        }
        fn ResumeTimer(&self) -> WinResult<()> {
            Ok(())
        }
    }

    pub(super) fn recycle(path: &Path) -> PlatformResult<()> {
        // IFileOperation needs a single-threaded apartment. A caller thread that already joined
        // the MTA cannot switch, so the operation then runs on a short-lived STA thread.
        if unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.is_ok() {
            let result = recycle_on_sta(path);
            unsafe { CoUninitialize() };
            return result;
        }
        std::thread::scope(|scope| scope.spawn(|| recycle(path)).join())
            .unwrap_or_else(|_| Err(PlatformError::OperationFailed("recycleFailed".into())))
    }

    fn is_access_denied(code: u32) -> bool {
        // E_ACCESSDENIED / COPYENGINE_E_ACCESS_DENIED_SRC / COPYENGINE_E_SHARING_VIOLATION_SRC
        matches!(code, 0x8007_0005 | 0x8027_0021 | 0x8027_0023)
    }

    fn recycle_on_sta(path: &Path) -> PlatformResult<()> {
        let failed = |e: windows::core::Error| {
            let code = e.code().0 as u32;
            if is_access_denied(code) {
                PlatformError::PermissionDenied
            } else {
                PlatformError::OperationFailed(format!("recycleFailed:{code:#x}"))
            }
        };
        let op: IFileOperation = unsafe { CoCreateInstance(&FileOperation, None, CLSCTX_ALL) }.map_err(failed)?;
        unsafe { op.SetOperationFlags(FOF_NO_UI | FOF_ALLOWUNDO | FOFX_RECYCLEONDELETE | FOFX_EARLYFAILURE) }
            .map_err(failed)?;
        let item: IShellItem =
            unsafe { SHCreateItemFromParsingName(&HSTRING::from(path.as_os_str()), None) }.map_err(failed)?;
        let flag = Rc::new(Cell::new(false));
        let sink: IFileOperationProgressSink =
            RecycleOnlySink { would_delete_permanently: Rc::clone(&flag) }.into();
        unsafe { op.DeleteItem(&item, &sink) }.map_err(failed)?;
        let performed = unsafe { op.PerformOperations() };
        if flag.get() {
            return Err(PlatformError::OperationFailed(WOULD_DELETE_PERMANENTLY.into()));
        }
        performed.map_err(failed)?;
        if unsafe { op.GetAnyOperationsAborted() }.map(|b| b.as_bool()).unwrap_or(true) {
            return Err(PlatformError::OperationFailed("recycleAborted".into()));
        }
        Ok(())
    }
}

/// Expand 8.3 short components (`C:\PROGRA~1`) to their long names with `GetLongPathNameW`.
/// Only works for paths that exist; `None` when the path is missing or the call fails.
pub fn long_path_name(path: &Path) -> Option<PathBuf> {
    #[cfg(windows)]
    {
        use windows::core::HSTRING;
        use windows::Win32::Storage::FileSystem::GetLongPathNameW;
        let source = HSTRING::from(path.as_os_str());
        let mut buf = vec![0u16; 512];
        loop {
            let len = unsafe { GetLongPathNameW(&source, Some(&mut buf)) } as usize;
            if len == 0 {
                return None;
            }
            if len < buf.len() {
                return Some(PathBuf::from(String::from_utf16_lossy(&buf[..len])));
            }
            // `len` is the required size including the terminator.
            if len > 32_768 {
                return None;
            }
            buf.resize(len, 0);
        }
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        None
    }
}

/// Schedule a locked file or empty folder for deletion at the next restart. Requires
/// administrator rights (writes `PendingFileRenameOperations`).
pub fn delete_on_reboot(path: &Path) -> PlatformResult<()> {
    #[cfg(windows)]
    {
        use windows::core::HSTRING;
        use windows::Win32::Storage::FileSystem::{MoveFileExW, MOVEFILE_DELAY_UNTIL_REBOOT};
        let source = HSTRING::from(path.as_os_str());
        unsafe { MoveFileExW(&source, None, MOVEFILE_DELAY_UNTIL_REBOOT) }.map_err(|e| {
            if e.code().0 as u32 == 0x8007_0005 {
                PlatformError::PermissionDenied
            } else {
                PlatformError::OperationFailed(format!("deleteOnReboot:{:?}", e.code()))
            }
        })
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        Err(PlatformError::Unsupported)
    }
}

/// Resolve a `.lnk` shortcut to its target path (no UI, no network search).
pub fn resolve_shortcut(link: &Path) -> Option<PathBuf> {
    #[cfg(windows)]
    {
        use windows::core::{Interface, HSTRING};
        use windows::Win32::System::Com::{
            CoCreateInstance, CoInitializeEx, CoUninitialize, IPersistFile, CLSCTX_INPROC_SERVER,
            COINIT_APARTMENTTHREADED, STGM_READ,
        };
        use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};

        let initialized = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.is_ok();
        let result = (|| {
            let shell_link: IShellLinkW =
                unsafe { CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER) }.ok()?;
            let persist: IPersistFile = shell_link.cast().ok()?;
            let path = HSTRING::from(link.as_os_str());
            unsafe { persist.Load(&path, STGM_READ) }.ok()?;
            let mut buf = [0u16; 1024];
            unsafe { shell_link.GetPath(&mut buf, std::ptr::null_mut(), 0) }.ok()?;
            let end = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
            let target = String::from_utf16_lossy(&buf[..end]);
            (!target.trim().is_empty()).then(|| PathBuf::from(target))
        })();
        if initialized {
            unsafe { CoUninitialize() };
        }
        result
    }
    #[cfg(not(windows))]
    {
        let _ = link;
        None
    }
}

/// The current user's Desktop folder (honours OneDrive / folder redirection).
pub fn user_desktop_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        use windows::Win32::System::Com::CoTaskMemFree;
        use windows::Win32::UI::Shell::{FOLDERID_Desktop, SHGetKnownFolderPath, KF_FLAG_DEFAULT};
        let raw = unsafe { SHGetKnownFolderPath(&FOLDERID_Desktop, KF_FLAG_DEFAULT, None) }.ok()?;
        let path = unsafe { raw.to_string() }.ok();
        unsafe { CoTaskMemFree(Some(raw.0 as *const core::ffi::c_void)) };
        path.map(PathBuf::from)
    }
    #[cfg(not(windows))]
    {
        None
    }
}

/// Roots (`C:\`, `D:\`, ...) of local fixed volumes. Removable, network, optical, and RAM
/// drives are left out so callers never block on a disconnected share or spin up a USB disk.
pub fn fixed_drive_roots() -> Vec<PathBuf> {
    #[cfg(windows)]
    {
        use windows::core::PCWSTR;
        use windows::Win32::Storage::FileSystem::{GetDriveTypeW, GetLogicalDrives};
        const DRIVE_FIXED: u32 = 3;
        let mask = unsafe { GetLogicalDrives() };
        let mut roots = Vec::new();
        for (bit, letter) in (b'A'..=b'Z').enumerate() {
            if mask & (1 << bit) == 0 {
                continue;
            }
            let root = format!("{}:\\", letter as char);
            let wide: Vec<u16> = root.encode_utf16().chain(std::iter::once(0)).collect();
            if unsafe { GetDriveTypeW(PCWSTR(wide.as_ptr())) } == DRIVE_FIXED {
                roots.push(PathBuf::from(root));
            }
        }
        roots
    }
    #[cfg(not(windows))]
    {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_recycle_flagged_deletes_are_allowed() {
        assert!(delete_flags_recycle(0x0080));
        assert!(delete_flags_recycle(0x0080 | 0x0001));
        // Without TSF_DELETE_RECYCLE_IF_POSSIBLE the copy engine would destroy the item.
        assert!(!delete_flags_recycle(0));
        assert!(!delete_flags_recycle(0x0001 | 0x0040));
    }

    /// Moves a tiny temp file to the real Recycle Bin, so it only runs on demand.
    #[cfg(windows)]
    #[test]
    #[ignore]
    fn smoke_recycle_temp_file() {
        let dir = std::env::temp_dir().join("pctk-recycle-smoke");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("recycle-me.txt");
        std::fs::write(&file, b"x").unwrap();
        recycle_path(&file).unwrap();
        assert!(!file.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
