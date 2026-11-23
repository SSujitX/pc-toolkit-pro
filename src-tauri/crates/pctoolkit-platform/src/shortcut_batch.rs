//! Batch `.lnk` resolution for scans that examine hundreds of shortcuts.
//!
//! [`crate::resolve_shortcut`] initializes and uninitializes COM around every call; on a thread
//! without COM that tears the apartment down each time (~25 ms per link). Holding one apartment
//! for the whole batch makes the nested initializations cheap reference-count bumps.

use std::path::PathBuf;

/// Resolve each shortcut's target (same semantics as [`crate::resolve_shortcut`]). Stops early
/// when `should_cancel` returns true; unresolved / skipped entries are `None`.
pub fn resolve_shortcuts(links: &[PathBuf], should_cancel: &dyn Fn() -> bool) -> Vec<Option<PathBuf>> {
    let _apartment = Apartment::enter();
    let mut out = Vec::with_capacity(links.len());
    for link in links {
        if should_cancel() {
            out.push(None);
            continue;
        }
        out.push(crate::resolve_shortcut(link));
    }
    out
}

struct Apartment {
    #[cfg_attr(not(windows), allow(dead_code))]
    initialized: bool,
}

impl Apartment {
    fn enter() -> Self {
        #[cfg(windows)]
        {
            use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
            // RPC_E_CHANGED_MODE (thread already MTA) is fine: shell links work there too.
            let initialized = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.is_ok();
            Self { initialized }
        }
        #[cfg(not(windows))]
        {
            Self { initialized: false }
        }
    }
}

impl Drop for Apartment {
    fn drop(&mut self) {
        #[cfg(windows)]
        if self.initialized {
            unsafe { windows::Win32::System::Com::CoUninitialize() };
        }
    }
}
