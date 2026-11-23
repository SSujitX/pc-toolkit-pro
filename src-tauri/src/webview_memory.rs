//! Shrink WebView2's memory while the main window is hidden in the tray.
//!
//! Uses the documented `ICoreWebView2_19::MemoryUsageTargetLevel`: `Low` lets WebView2 trim its
//! caches and working set while the app sits in the tray; `Normal` is restored as soon as the
//! window is shown or focused. Older WebView2 runtimes without the interface are left untouched.

pub fn set_low_memory(window: &tauri::WebviewWindow, low: bool) {
    #[cfg(windows)]
    {
        let _ = window.with_webview(move |webview| unsafe {
            use webview2_com::Microsoft::Web::WebView2::Win32::{
                ICoreWebView2_19, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW,
                COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL,
            };
            use windows_core::Interface;
            let Ok(core) = webview.controller().CoreWebView2() else {
                return;
            };
            if let Ok(core19) = core.cast::<ICoreWebView2_19>() {
                let level = if low {
                    COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW
                } else {
                    COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL
                };
                let _ = core19.SetMemoryUsageTargetLevel(level);
            }
        });
    }
    #[cfg(not(windows))]
    {
        let _ = (window, low);
    }
}
