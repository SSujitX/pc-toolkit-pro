//! Windows Disk Cleanup handlers (`IEmptyVolumeCache` / `IEmptyVolumeCache2`).
//!
//! Disk Cleanup (`cleanmgr`) delegates every category to an in-process COM handler registered under
//! `HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\VolumeCaches\<name>`. Driving those
//! handlers directly lets us reuse the OS-owned cleanup logic (Windows Update component store,
//! Delivery Optimization, previous installations) instead of guessing at protected folders or
//! shelling out to DISM / `cleanmgr`.
//!
//! Threading: handlers are registered as apartment-threaded shell extensions, so each call
//! initializes COM as STA on the calling thread and releases it before returning. Callers must run
//! these functions on a blocking worker thread (`spawn_blocking`), never the async adapter thread,
//! because `GetSpaceUsed` / `Purge` for Update Cleanup can take minutes.
//!
//! Elevation: kinds that touch machine-wide servicing state report `needsAdmin` up front when the
//! process token is not elevated. Nothing here elevates on its own.

use serde::{Deserialize, Serialize};

use crate::PlatformResult;

/// Disk Cleanup categories the product may drive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum VolumeCacheKind {
    InternetCache,
    DeliveryOptimization,
    DefenderCache,
    UpdateCleanup,
    PreviousInstallations,
    ErrorReports,
    TemporarySetupFiles,
}

impl VolumeCacheKind {
    pub const ALL: [Self; 7] = [
        Self::InternetCache,
        Self::DeliveryOptimization,
        Self::DefenderCache,
        Self::UpdateCleanup,
        Self::PreviousInstallations,
        Self::ErrorReports,
        Self::TemporarySetupFiles,
    ];

    /// Registry key name under `Explorer\VolumeCaches`.
    pub fn handler_name(self) -> &'static str {
        match self {
            Self::InternetCache => "Internet Cache Files",
            Self::DeliveryOptimization => "Delivery Optimization Files",
            Self::DefenderCache => "Windows Defender",
            Self::UpdateCleanup => "Update Cleanup",
            Self::PreviousInstallations => "Previous Installations",
            Self::ErrorReports => "Windows Error Reporting Files",
            Self::TemporarySetupFiles => "Temporary Setup Files",
        }
    }

    /// Kinds whose handler only works from an elevated token (servicing store, `Windows.old`,
    /// setup leftovers under the system drive root). Others are attempted and reported honestly.
    pub fn requires_admin(self) -> bool {
        matches!(
            self,
            Self::UpdateCleanup | Self::PreviousInstallations | Self::TemporarySetupFiles
        )
    }
}

/// Typed outcome of a handler probe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum VolumeCacheStatus {
    /// Handler initialized and reported a size (possibly zero).
    Ready,
    /// Handler reported there is nothing to clean (`S_FALSE` from Initialize).
    Empty,
    /// Handler key or COM class is not registered on this Windows build.
    HandlerMissing,
    /// Not elevated, or the handler returned access denied.
    NeedsAdmin,
    /// Handler could not report a size (`S_FALSE` / sentinel from `GetSpaceUsed`).
    SizeUnknown,
    Cancelled,
    Failed,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VolumeCacheEstimate {
    pub kind: VolumeCacheKind,
    pub status: VolumeCacheStatus,
    /// True only when the handler is usable and reported a size.
    pub available: bool,
    pub bytes: u64,
    pub needs_admin: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum VolumeCachePurgeStatus {
    /// Verification found nothing left.
    Completed,
    /// Purge ran but the re-estimate still reports bytes.
    Partial,
    NothingToPurge,
    Cancelled,
    NeedsAdmin,
    Unavailable,
    Failed,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VolumeCachePurge {
    pub kind: VolumeCacheKind,
    pub status: VolumeCachePurgeStatus,
    /// Estimated bytes before minus verified bytes after.
    pub released_bytes: u64,
    pub remaining_bytes: u64,
}

impl VolumeCacheEstimate {
    fn with_status(kind: VolumeCacheKind, status: VolumeCacheStatus, bytes: u64) -> Self {
        Self {
            kind,
            status,
            available: status == VolumeCacheStatus::Ready,
            bytes: if status == VolumeCacheStatus::Ready { bytes } else { 0 },
            needs_admin: status == VolumeCacheStatus::NeedsAdmin,
        }
    }
}

/// Read-only size estimate for one Disk Cleanup category on the system drive.
pub fn volume_cache_estimate(kind: VolumeCacheKind) -> PlatformResult<VolumeCacheEstimate> {
    volume_cache_estimate_cancellable(kind, &|| false)
}

/// Same as [`volume_cache_estimate`], polling `should_cancel` from the handler's progress callback.
pub fn volume_cache_estimate_cancellable(
    kind: VolumeCacheKind,
    should_cancel: &dyn Fn() -> bool,
) -> PlatformResult<VolumeCacheEstimate> {
    imp::estimate(kind, should_cancel)
}

/// Purge one category, then verify with a fresh handler instance.
///
/// Destructive: callers must have explicit user confirmation. Returns `NeedsAdmin` without touching
/// anything when the kind requires elevation and the token is not elevated.
pub fn volume_cache_purge(
    kind: VolumeCacheKind,
    should_cancel: &dyn Fn() -> bool,
) -> PlatformResult<VolumeCachePurge> {
    imp::purge(kind, should_cancel)
}

/// Fast check for `%SystemDrive%\Windows.old` as a real directory (not a junction / symlink).
pub fn windows_old_present() -> bool {
    let drive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".to_string());
    let path = std::path::PathBuf::from(format!("{drive}\\Windows.old"));
    match std::fs::symlink_metadata(&path) {
        Ok(meta) => meta.is_dir() && !is_reparse_point(&meta),
        Err(_) => false,
    }
}

fn is_reparse_point(meta: &std::fs::Metadata) -> bool {
    if meta.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    {
        false
    }
}

/// Map a purge's before/after sizes to a status. Pure so it can be unit-tested.
fn purge_status(before: u64, after: Option<u64>) -> (VolumeCachePurgeStatus, u64, u64) {
    match after {
        Some(0) => (VolumeCachePurgeStatus::Completed, before, 0),
        Some(remaining) => (
            if remaining >= before {
                VolumeCachePurgeStatus::Failed
            } else {
                VolumeCachePurgeStatus::Partial
            },
            before.saturating_sub(remaining),
            remaining,
        ),
        // Verification unavailable: do not claim released bytes we cannot confirm.
        None => (VolumeCachePurgeStatus::Partial, 0, before),
    }
}

#[cfg(windows)]
mod imp {
    use super::*;
    use std::cell::Cell;
    use std::rc::Rc;
    use std::thread::ThreadId;

    use windows::core::{implement, Interface, Result as WinResult, GUID, HRESULT, HSTRING, PCWSTR, PWSTR};
    use windows::Win32::Foundation::{
        E_ABORT, E_ACCESSDENIED, ERROR_ACCESS_DENIED, ERROR_FILE_NOT_FOUND,
        REGDB_E_CLASSNOTREG, S_FALSE,
    };
    use windows::Win32::System::Com::{
        CLSIDFromString, CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize,
        CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
    };
    use windows::Win32::System::Registry::{RegCloseKey, RegOpenKeyExW, HKEY, HKEY_LOCAL_MACHINE, KEY_READ};
    use windows::Win32::UI::LegacyWindowsEnvironmentFeatures::{
        IEmptyVolumeCache, IEmptyVolumeCache2, IEmptyVolumeCacheCallBack, IEmptyVolumeCacheCallBack_Impl,
        EMPTY_VOLUME_CACHE_FLAGS,
    };

    use crate::registry::{read_value, RegistryKeyPath, RegistryRoot, RegistryView};

    const VOLUME_CACHES: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\VolumeCaches";

    // ---------------------------------------------------------------- cancellation callback

    /// Shared between our stack frame and the COM callback object.
    ///
    /// The callback only dereferences `should_cancel` while `active` is set, which our guard clears
    /// before the borrowed closure goes out of scope, and only on the thread that created it (STA
    /// calls are synchronous). A handler that keeps the callback alive after `Deactivate` therefore
    /// sees an inert object instead of a dangling closure.
    struct CancelGate {
        active: Cell<bool>,
        thread: ThreadId,
        should_cancel: *const (dyn Fn() -> bool + 'static),
    }

    impl CancelGate {
        fn cancelled(&self) -> bool {
            if !self.active.get() || std::thread::current().id() != self.thread {
                return false;
            }
            // SAFETY: see the type-level comment; the pointer is live while `active` is true.
            unsafe { (*self.should_cancel)() }
        }
    }

    struct GateGuard(Rc<CancelGate>);

    impl Drop for GateGuard {
        fn drop(&mut self) {
            self.0.active.set(false);
        }
    }

    #[implement(IEmptyVolumeCacheCallBack)]
    struct CancelCallback {
        gate: Rc<CancelGate>,
    }

    impl IEmptyVolumeCacheCallBack_Impl for CancelCallback_Impl {
        fn ScanProgress(&self, _space_used: u64, _flags: u32, _status: &PCWSTR) -> WinResult<()> {
            if self.gate.cancelled() {
                Err(E_ABORT.into())
            } else {
                Ok(())
            }
        }

        fn PurgeProgress(&self, _freed: u64, _to_free: u64, _flags: u32, _status: &PCWSTR) -> WinResult<()> {
            if self.gate.cancelled() {
                Err(E_ABORT.into())
            } else {
                Ok(())
            }
        }
    }

    fn make_callback(should_cancel: &dyn Fn() -> bool) -> (GateGuard, IEmptyVolumeCacheCallBack) {
        let short: *const (dyn Fn() -> bool + '_) = should_cancel;
        // SAFETY: lifetime erasure only; `GateGuard` disables the gate before the borrow ends.
        let erased: *const (dyn Fn() -> bool + 'static) = unsafe { std::mem::transmute(short) };
        let gate = Rc::new(CancelGate {
            active: Cell::new(true),
            thread: std::thread::current().id(),
            should_cancel: erased,
        });
        let callback: IEmptyVolumeCacheCallBack = CancelCallback { gate: gate.clone() }.into();
        (GateGuard(gate), callback)
    }

    // ---------------------------------------------------------------- COM / registry plumbing

    struct ComApartment {
        owned: bool,
    }

    impl ComApartment {
        fn enter() -> Self {
            // S_OK / S_FALSE must be balanced; RPC_E_CHANGED_MODE means the thread is already MTA,
            // in which case COM hosts the apartment-threaded handler in its own STA for us.
            let owned = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.is_ok();
            Self { owned }
        }
    }

    impl Drop for ComApartment {
        fn drop(&mut self) {
            if self.owned {
                unsafe { CoUninitialize() };
            }
        }
    }

    struct OwnedKey(HKEY);

    impl Drop for OwnedKey {
        fn drop(&mut self) {
            unsafe {
                let _ = RegCloseKey(self.0);
            }
        }
    }

    fn is_access_denied(hr: HRESULT) -> bool {
        hr == E_ACCESSDENIED || hr == HRESULT::from_win32(ERROR_ACCESS_DENIED.0)
    }

    fn process_is_elevated() -> bool {
        crate::monitor::is_user_admin()
    }

    fn system_volume() -> String {
        let drive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".to_string());
        format!("{}\\", drive.trim_end_matches('\\'))
    }

    fn handler_clsid(kind: VolumeCacheKind) -> Option<GUID> {
        let path = RegistryKeyPath::new(
            RegistryRoot::LocalMachine,
            RegistryView::Native64,
            format!("{VOLUME_CACHES}\\{}", kind.handler_name()),
        );
        let text = read_value(&path, "")
            .filter(|v| !v.trim().is_empty())
            .or_else(|| read_value(&path, "CLSID"))?;
        let wide = HSTRING::from(text.trim());
        unsafe { CLSIDFromString(&wide) }.ok()
    }

    unsafe fn free_co_string(value: PWSTR) {
        if !value.is_null() {
            CoTaskMemFree(Some(value.0 as *const core::ffi::c_void));
        }
    }

    /// An initialized handler. `Deactivate` runs on drop so every exit path releases it.
    struct Handler {
        cache: IEmptyVolumeCache,
        _key: OwnedKey,
    }

    impl Drop for Handler {
        fn drop(&mut self) {
            let _ = unsafe { self.cache.Deactivate() };
        }
    }

    fn open_handler(kind: VolumeCacheKind) -> Result<Handler, VolumeCacheStatus> {
        let clsid = handler_clsid(kind).ok_or(VolumeCacheStatus::HandlerMissing)?;

        let mut raw_key = HKEY::default();
        let subkey = HSTRING::from(format!("{VOLUME_CACHES}\\{}", kind.handler_name()));
        let status = unsafe { RegOpenKeyExW(HKEY_LOCAL_MACHINE, &subkey, Some(0), KEY_READ, &mut raw_key) };
        if status == ERROR_FILE_NOT_FOUND {
            return Err(VolumeCacheStatus::HandlerMissing);
        }
        if status == ERROR_ACCESS_DENIED {
            return Err(VolumeCacheStatus::NeedsAdmin);
        }
        if status.is_err() {
            return Err(VolumeCacheStatus::Failed);
        }
        let key = OwnedKey(raw_key);

        let cache: IEmptyVolumeCache = match unsafe { CoCreateInstance(&clsid, None, CLSCTX_INPROC_SERVER) } {
            Ok(cache) => cache,
            Err(error) if error.code() == REGDB_E_CLASSNOTREG => return Err(VolumeCacheStatus::HandlerMissing),
            Err(error) if is_access_denied(error.code()) => return Err(VolumeCacheStatus::NeedsAdmin),
            Err(_) => return Err(VolumeCacheStatus::Failed),
        };

        let volume = HSTRING::from(system_volume());
        let key_name = HSTRING::from(kind.handler_name());
        let mut display = PWSTR::null();
        let mut description = PWSTR::null();
        let mut button = PWSTR::null();
        let mut flags = EMPTY_VOLUME_CACHE_FLAGS(0);

        // Raw vtable calls: the generated wrappers fold S_FALSE into Ok, but S_FALSE from
        // Initialize means "nothing to clean" and must not be reported as a usable handler.
        let hr = unsafe {
            if let Ok(cache2) = cache.cast::<IEmptyVolumeCache2>() {
                (Interface::vtable(&cache2).InitializeEx)(
                    Interface::as_raw(&cache2),
                    key.0,
                    PCWSTR(volume.as_ptr()),
                    PCWSTR(key_name.as_ptr()),
                    &mut display,
                    &mut description,
                    &mut button,
                    &mut flags,
                )
            } else {
                (Interface::vtable(&cache).Initialize)(
                    Interface::as_raw(&cache),
                    key.0,
                    PCWSTR(volume.as_ptr()),
                    &mut display,
                    &mut description,
                    &mut flags,
                )
            }
        };
        unsafe {
            free_co_string(display);
            free_co_string(description);
            free_co_string(button);
        }

        if hr == S_FALSE {
            // The handler declined to activate, so it is released without Deactivate.
            return Err(VolumeCacheStatus::Empty);
        }
        if is_access_denied(hr) {
            return Err(VolumeCacheStatus::NeedsAdmin);
        }
        if hr.is_err() {
            return Err(VolumeCacheStatus::Failed);
        }
        Ok(Handler { cache, _key: key })
    }

    fn space_used(handler: &Handler, callback: &IEmptyVolumeCacheCallBack) -> Result<u64, VolumeCacheStatus> {
        let mut bytes = 0u64;
        let hr = unsafe {
            (Interface::vtable(&handler.cache).GetSpaceUsed)(
                Interface::as_raw(&handler.cache),
                &mut bytes,
                Interface::as_raw(callback),
            )
        };
        if hr == E_ABORT {
            return Err(VolumeCacheStatus::Cancelled);
        }
        if is_access_denied(hr) {
            return Err(VolumeCacheStatus::NeedsAdmin);
        }
        if hr.is_err() {
            return Err(VolumeCacheStatus::Failed);
        }
        if hr == S_FALSE || bytes == u64::MAX {
            return Err(VolumeCacheStatus::SizeUnknown);
        }
        Ok(bytes)
    }

    fn measure(kind: VolumeCacheKind, should_cancel: &dyn Fn() -> bool) -> Result<u64, VolumeCacheStatus> {
        let handler = open_handler(kind)?;
        let (_guard, callback) = make_callback(should_cancel);
        let result = space_used(&handler, &callback);
        drop(handler);
        result
    }

    pub(super) fn estimate(
        kind: VolumeCacheKind,
        should_cancel: &dyn Fn() -> bool,
    ) -> PlatformResult<VolumeCacheEstimate> {
        if kind.requires_admin() && !process_is_elevated() {
            return Ok(VolumeCacheEstimate::with_status(kind, VolumeCacheStatus::NeedsAdmin, 0));
        }
        let _apartment = ComApartment::enter();
        Ok(match measure(kind, should_cancel) {
            Ok(bytes) => VolumeCacheEstimate::with_status(kind, VolumeCacheStatus::Ready, bytes),
            Err(status) => VolumeCacheEstimate::with_status(kind, status, 0),
        })
    }

    pub(super) fn purge(kind: VolumeCacheKind, should_cancel: &dyn Fn() -> bool) -> PlatformResult<VolumeCachePurge> {
        let result = |status, released, remaining| VolumeCachePurge {
            kind,
            status,
            released_bytes: released,
            remaining_bytes: remaining,
        };
        if kind.requires_admin() && !process_is_elevated() {
            return Ok(result(VolumeCachePurgeStatus::NeedsAdmin, 0, 0));
        }
        let _apartment = ComApartment::enter();

        let handler = match open_handler(kind) {
            Ok(handler) => handler,
            Err(VolumeCacheStatus::Empty) => return Ok(result(VolumeCachePurgeStatus::NothingToPurge, 0, 0)),
            Err(VolumeCacheStatus::NeedsAdmin) => return Ok(result(VolumeCachePurgeStatus::NeedsAdmin, 0, 0)),
            Err(VolumeCacheStatus::Failed) => return Ok(result(VolumeCachePurgeStatus::Failed, 0, 0)),
            Err(_) => return Ok(result(VolumeCachePurgeStatus::Unavailable, 0, 0)),
        };
        let (guard, callback) = make_callback(should_cancel);
        let before = match space_used(&handler, &callback) {
            Ok(0) => return Ok(result(VolumeCachePurgeStatus::NothingToPurge, 0, 0)),
            Ok(bytes) => bytes,
            Err(VolumeCacheStatus::Cancelled) => return Ok(result(VolumeCachePurgeStatus::Cancelled, 0, 0)),
            Err(VolumeCacheStatus::NeedsAdmin) => return Ok(result(VolumeCachePurgeStatus::NeedsAdmin, 0, 0)),
            Err(VolumeCacheStatus::Failed) => return Ok(result(VolumeCachePurgeStatus::Failed, 0, 0)),
            Err(_) => return Ok(result(VolumeCachePurgeStatus::Unavailable, 0, 0)),
        };
        if should_cancel() {
            return Ok(result(VolumeCachePurgeStatus::Cancelled, 0, before));
        }

        let hr = unsafe {
            (Interface::vtable(&handler.cache).Purge)(
                Interface::as_raw(&handler.cache),
                before,
                Interface::as_raw(&callback),
            )
        };
        drop(handler);
        drop(guard);
        let cancelled = hr == E_ABORT;
        if is_access_denied(hr) {
            return Ok(result(VolumeCachePurgeStatus::NeedsAdmin, 0, before));
        }

        // Verify with a fresh instance; reusing a purged handler for GetSpaceUsed is not a
        // documented sequence and some handlers cache their first scan.
        let after = match measure(kind, &|| false) {
            Ok(bytes) => Some(bytes),
            Err(VolumeCacheStatus::Empty) => Some(0),
            Err(_) => None,
        };
        let (mut status, released, remaining) = purge_status(before, after);
        if cancelled {
            status = VolumeCachePurgeStatus::Cancelled;
        } else if hr.is_err() && status != VolumeCachePurgeStatus::Completed {
            status = VolumeCachePurgeStatus::Failed;
        }
        Ok(result(status, released, remaining))
    }
}

#[cfg(not(windows))]
mod imp {
    use super::*;
    use crate::PlatformError;

    pub(super) fn estimate(
        _kind: VolumeCacheKind,
        _should_cancel: &dyn Fn() -> bool,
    ) -> PlatformResult<VolumeCacheEstimate> {
        Err(PlatformError::Unsupported)
    }

    pub(super) fn purge(_kind: VolumeCacheKind, _should_cancel: &dyn Fn() -> bool) -> PlatformResult<VolumeCachePurge> {
        Err(PlatformError::Unsupported)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handler_names_are_unique() {
        let mut names: Vec<_> = VolumeCacheKind::ALL.iter().map(|k| k.handler_name()).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), VolumeCacheKind::ALL.len());
    }

    #[test]
    fn servicing_kinds_require_admin() {
        assert!(VolumeCacheKind::UpdateCleanup.requires_admin());
        assert!(VolumeCacheKind::PreviousInstallations.requires_admin());
        assert!(!VolumeCacheKind::InternetCache.requires_admin());
    }

    #[test]
    fn estimate_only_reports_bytes_when_ready() {
        let est = VolumeCacheEstimate::with_status(VolumeCacheKind::InternetCache, VolumeCacheStatus::Failed, 42);
        assert!(!est.available);
        assert_eq!(est.bytes, 0);
        let est = VolumeCacheEstimate::with_status(VolumeCacheKind::UpdateCleanup, VolumeCacheStatus::NeedsAdmin, 0);
        assert!(est.needs_admin);
        assert!(!est.available);
    }

    #[test]
    fn purge_status_reports_verified_bytes_only() {
        assert_eq!(purge_status(100, Some(0)), (VolumeCachePurgeStatus::Completed, 100, 0));
        assert_eq!(purge_status(100, Some(40)), (VolumeCachePurgeStatus::Partial, 60, 40));
        assert_eq!(purge_status(100, Some(100)), (VolumeCachePurgeStatus::Failed, 0, 100));
        assert_eq!(purge_status(100, Some(150)), (VolumeCachePurgeStatus::Failed, 0, 150));
        assert_eq!(purge_status(100, None), (VolumeCachePurgeStatus::Partial, 0, 100));
    }

    /// Read-only smoke run against the real machine. Never purges.
    #[test]
    #[ignore]
    fn smoke_estimate_all_kinds() {
        for kind in VolumeCacheKind::ALL {
            let started = std::time::Instant::now();
            let est = volume_cache_estimate(kind).expect("estimate");
            println!(
                "{:<22} status={:?} available={} bytes={} needsAdmin={} ms={}",
                kind.handler_name(),
                est.status,
                est.available,
                est.bytes,
                est.needs_admin,
                started.elapsed().as_millis()
            );
        }
        println!("windows_old_present={}", windows_old_present());
    }
}
