//! Windows Security Center, Windows Firewall, and Windows Update Agent facts.
//!
//! * Antivirus / firewall products come from the documented WSC product list (`IWSCProductList`),
//!   which is what Windows Security itself shows. It is not available on Server SKUs.
//! * Firewall profile state uses `INetFwPolicy2` (never the deprecated `netsh firewall`).
//! * Windows Update facts use the Windows Update Agent COM API read-only: last search /
//!   installation dates, and an **offline** search of the locally cached catalog (no network,
//!   no downloads, no installs) bounded by a timeout.
//!
//! This module reports facts only; deciding what is an issue belongs to Core.

use serde::{Deserialize, Serialize};
use std::time::Duration;

use crate::{PlatformError, PlatformResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SecurityProductKind {
    Antivirus,
    Firewall,
}

/// WSC product state (`WSC_SECURITY_PRODUCT_STATE`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SecurityProductState {
    On,
    Off,
    Snoozed,
    Expired,
    Unknown,
}

impl SecurityProductState {
    /// Decode the raw `WSC_SECURITY_PRODUCT_STATE` value.
    pub fn from_wsc(raw: i32) -> Self {
        match raw {
            0 => Self::On,
            1 => Self::Off,
            2 => Self::Snoozed,
            3 => Self::Expired,
            _ => Self::Unknown,
        }
    }
}

/// Decode `WSC_SECURITY_SIGNATURE_STATUS` (0 = out of date, 1 = up to date).
pub fn signatures_up_to_date_from_wsc(raw: i32) -> Option<bool> {
    match raw {
        0 => Some(false),
        1 => Some(true),
        _ => None,
    }
}

/// WSC registration GUID used by Microsoft Defender Antivirus.
const DEFENDER_WSC_GUID: &str = "{D68DDC3A-831F-4FAE-9E44-DA132C1ACF46}";

/// True when a WSC product GUID / name identifies Microsoft Defender Antivirus.
pub fn is_microsoft_defender(guid: &str, name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    guid.trim().eq_ignore_ascii_case(DEFENDER_WSC_GUID)
        || lower == "windows defender"
        || lower.starts_with("microsoft defender")
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SecurityProductFact {
    pub kind: SecurityProductKind,
    pub name: String,
    pub guid: String,
    pub state: SecurityProductState,
    /// Only meaningful for antivirus products.
    pub signatures_up_to_date: Option<bool>,
    pub is_microsoft_defender: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FirewallProfileKind {
    Domain,
    Private,
    Public,
}

impl FirewallProfileKind {
    pub const ALL: [FirewallProfileKind; 3] = [Self::Domain, Self::Private, Self::Public];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Domain => "domain",
            Self::Private => "private",
            Self::Public => "public",
        }
    }

    /// `NET_FW_PROFILE_TYPE2` bit.
    pub fn bit(self) -> i32 {
        match self {
            Self::Domain => 1,
            Self::Private => 2,
            Self::Public => 4,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FirewallProfileFact {
    pub profile: FirewallProfileKind,
    pub enabled: bool,
    /// The machine is currently connected to a network of this profile type.
    pub active: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowsUpdateDates {
    /// Last successful update search (Unix ms, UTC).
    pub last_search_success_ms: Option<i64>,
    /// Last successful update installation (Unix ms, UTC).
    pub last_installation_success_ms: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingUpdatesFact {
    pub total: u32,
    /// Updates that carry an MSRC severity (security updates).
    pub security: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PendingUpdatesOutcome {
    Found(PendingUpdatesFact),
    TimedOut,
    Stopped,
    Failed(String),
}

/// Convert an OLE Automation date (days since 1899-12-30, UTC for WUA) to Unix milliseconds.
pub fn ole_date_to_unix_ms(date: f64) -> Option<i64> {
    // A zero / negative date is how WUA reports "never".
    if !date.is_finite() || date <= 1.0 {
        return None;
    }
    const OLE_TO_UNIX_DAYS: f64 = 25_569.0;
    Some(((date - OLE_TO_UNIX_DAYS) * 86_400_000.0).round() as i64)
}

#[cfg(windows)]
mod imp {
    use super::*;
    use windows::core::{Interface, BSTR};
    use windows::Win32::Foundation::{VARIANT_FALSE, VARIANT_TRUE};
    use windows::Win32::NetworkManagement::WindowsFirewall::{INetFwPolicy2, NetFwPolicy2, NET_FW_PROFILE_TYPE2};
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, CLSCTX_LOCAL_SERVER,
        COINIT_MULTITHREADED,
    };
    use windows::Win32::System::SecurityCenter::{
        IWSCProductList, WSCProductList, WSC_SECURITY_PROVIDER_ANTIVIRUS, WSC_SECURITY_PROVIDER_FIREWALL,
    };
    use windows::Win32::System::UpdateAgent::{
        AutomaticUpdates, IAutomaticUpdates2, IUpdateSession, UpdateSession,
    };
    use windows::Win32::System::Variant::{VARIANT, VT_DATE};

    /// Balanced COM initialisation for the calling thread (MTA). `RPC_E_CHANGED_MODE` means the
    /// thread already has an apartment we may use but must not uninitialise.
    struct ComScope(bool);

    impl ComScope {
        fn enter() -> Self {
            Self(unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok())
        }
    }

    impl Drop for ComScope {
        fn drop(&mut self) {
            if self.0 {
                unsafe { CoUninitialize() };
            }
        }
    }

    fn com_error(prefix: &str, e: windows::core::Error) -> PlatformError {
        if e.code().0 as u32 == 0x8007_0005 {
            PlatformError::PermissionDenied
        } else {
            PlatformError::OperationFailed(format!("{prefix}:{:#010x}", e.code().0 as u32))
        }
    }

    pub fn list_products(kind: SecurityProductKind) -> PlatformResult<Vec<SecurityProductFact>> {
        let _com = ComScope::enter();
        unsafe {
            let list: IWSCProductList = CoCreateInstance(&WSCProductList, None, CLSCTX_INPROC_SERVER)
                .map_err(|e| com_error("wscCreate", e))?;
            let provider = match kind {
                SecurityProductKind::Antivirus => WSC_SECURITY_PROVIDER_ANTIVIRUS,
                SecurityProductKind::Firewall => WSC_SECURITY_PROVIDER_FIREWALL,
            };
            list.Initialize(provider).map_err(|e| com_error("wscInitialize", e))?;
            let count = list.Count().map_err(|e| com_error("wscCount", e))?.max(0) as u32;
            let mut out = Vec::new();
            for index in 0..count.min(64) {
                let Ok(product) = list.get_Item(index) else { continue };
                let name = product.ProductName().map(|b| b.to_string()).unwrap_or_default();
                let guid = product.ProductGuid().map(|b| b.to_string()).unwrap_or_default();
                let state = product
                    .ProductState()
                    .map(|s| SecurityProductState::from_wsc(s.0))
                    .unwrap_or(SecurityProductState::Unknown);
                let signatures_up_to_date = match kind {
                    SecurityProductKind::Antivirus => product
                        .SignatureStatus()
                        .ok()
                        .and_then(|s| signatures_up_to_date_from_wsc(s.0)),
                    SecurityProductKind::Firewall => None,
                };
                out.push(SecurityProductFact {
                    kind,
                    is_microsoft_defender: is_microsoft_defender(&guid, &name),
                    name,
                    guid,
                    state,
                    signatures_up_to_date,
                });
            }
            Ok(out)
        }
    }

    fn firewall_policy() -> PlatformResult<INetFwPolicy2> {
        unsafe { CoCreateInstance(&NetFwPolicy2, None, CLSCTX_INPROC_SERVER) }
            .map_err(|e| com_error("firewallCreate", e))
    }

    pub fn firewall_profiles() -> PlatformResult<Vec<FirewallProfileFact>> {
        let _com = ComScope::enter();
        let policy = firewall_policy()?;
        unsafe {
            let current = policy.CurrentProfileTypes().unwrap_or(0);
            let mut out = Vec::new();
            for profile in FirewallProfileKind::ALL {
                let enabled = policy
                    .get_FirewallEnabled(NET_FW_PROFILE_TYPE2(profile.bit()))
                    .map_err(|e| com_error("firewallRead", e))?;
                out.push(FirewallProfileFact {
                    profile,
                    enabled: enabled.as_bool(),
                    active: current & profile.bit() != 0,
                });
            }
            Ok(out)
        }
    }

    pub fn set_firewall_enabled(profile: FirewallProfileKind, enabled: bool) -> PlatformResult<()> {
        let _com = ComScope::enter();
        let policy = firewall_policy()?;
        let value = if enabled { VARIANT_TRUE } else { VARIANT_FALSE };
        unsafe { policy.put_FirewallEnabled(NET_FW_PROFILE_TYPE2(profile.bit()), value) }
            .map_err(|e| com_error("firewallWrite", e))
    }

    fn variant_date_ms(value: &VARIANT) -> Option<i64> {
        unsafe {
            let inner = &value.Anonymous.Anonymous;
            if inner.vt == VT_DATE {
                ole_date_to_unix_ms(inner.Anonymous.date)
            } else {
                None
            }
        }
    }

    pub fn update_dates() -> PlatformResult<WindowsUpdateDates> {
        let _com = ComScope::enter();
        unsafe {
            let au: windows::core::IUnknown =
                CoCreateInstance(&AutomaticUpdates, None, CLSCTX_INPROC_SERVER | CLSCTX_LOCAL_SERVER)
                    .map_err(|e| com_error("wuaCreate", e))?;
            let au2: IAutomaticUpdates2 = au.cast().map_err(|e| com_error("wuaCast", e))?;
            let results = au2.Results().map_err(|e| com_error("wuaResults", e))?;
            Ok(WindowsUpdateDates {
                last_search_success_ms: results.LastSearchSuccessDate().ok().as_ref().and_then(variant_date_ms),
                last_installation_success_ms: results
                    .LastInstallationSuccessDate()
                    .ok()
                    .as_ref()
                    .and_then(variant_date_ms),
            })
        }
    }

    /// Offline search of the locally cached update catalog. Runs on the caller's thread.
    pub fn search_pending_offline() -> PlatformResult<PendingUpdatesFact> {
        let _com = ComScope::enter();
        unsafe {
            let session: IUpdateSession = CoCreateInstance(&UpdateSession, None, CLSCTX_INPROC_SERVER | CLSCTX_LOCAL_SERVER)
                .map_err(|e| com_error("wuaSession", e))?;
            let _ = session.SetClientApplicationID(&BSTR::from("PC Toolkit Pro"));
            let searcher = session.CreateUpdateSearcher().map_err(|e| com_error("wuaSearcher", e))?;
            // Offline: query only what the last Windows Update scan cached — no network traffic.
            searcher.SetOnline(VARIANT_FALSE).map_err(|e| com_error("wuaOnline", e))?;
            let result = searcher
                .Search(&BSTR::from("IsInstalled=0 and IsHidden=0"))
                .map_err(|e| com_error("wuaSearch", e))?;
            let updates = result.Updates().map_err(|e| com_error("wuaUpdates", e))?;
            let count = updates.Count().unwrap_or(0).max(0);
            let mut security = 0u32;
            for index in 0..count {
                if let Ok(update) = updates.get_Item(index) {
                    if update.MsrcSeverity().is_ok_and(|s| !s.is_empty()) {
                        security += 1;
                    }
                }
            }
            Ok(PendingUpdatesFact { total: count as u32, security })
        }
    }
}

/// Antivirus or firewall products registered with Windows Security Center.
pub fn list_security_products(kind: SecurityProductKind) -> PlatformResult<Vec<SecurityProductFact>> {
    #[cfg(windows)]
    {
        imp::list_products(kind)
    }
    #[cfg(not(windows))]
    {
        let _ = kind;
        Err(PlatformError::Unsupported)
    }
}

/// Windows Defender Firewall state per profile. Fails when the firewall service is not running.
pub fn firewall_profiles() -> PlatformResult<Vec<FirewallProfileFact>> {
    #[cfg(windows)]
    {
        imp::firewall_profiles()
    }
    #[cfg(not(windows))]
    {
        Err(PlatformError::Unsupported)
    }
}

/// Turn one Windows Defender Firewall profile on/off (`INetFwPolicy2::put_FirewallEnabled`).
/// Requires elevation; returns `PermissionDenied` otherwise.
pub fn set_firewall_profile_enabled(profile: FirewallProfileKind, enabled: bool) -> PlatformResult<()> {
    #[cfg(windows)]
    {
        imp::set_firewall_enabled(profile, enabled)
    }
    #[cfg(not(windows))]
    {
        let _ = (profile, enabled);
        Err(PlatformError::Unsupported)
    }
}

/// Last successful Windows Update search / installation dates (`IAutomaticUpdates2::Results`).
pub fn windows_update_dates() -> PlatformResult<WindowsUpdateDates> {
    #[cfg(windows)]
    {
        imp::update_dates()
    }
    #[cfg(not(windows))]
    {
        Err(PlatformError::Unsupported)
    }
}

static SEARCH_IN_FLIGHT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Count not-installed, not-hidden updates from the locally cached catalog, waiting at most
/// `timeout` and returning early when `should_stop()` turns true. A timed-out search keeps running
/// on its worker thread (WUA searches cannot be aborted synchronously); a second call while it is
/// still running reports `TimedOut` instead of piling up workers.
pub fn search_pending_updates(timeout: Duration, should_stop: &dyn Fn() -> bool) -> PendingUpdatesOutcome {
    #[cfg(windows)]
    {
        use std::sync::atomic::Ordering;
        use std::sync::mpsc;
        use std::time::Instant;

        if SEARCH_IN_FLIGHT.swap(true, Ordering::AcqRel) {
            return PendingUpdatesOutcome::TimedOut;
        }
        let (tx, rx) = mpsc::channel();
        let spawned = std::thread::Builder::new().name("wua-search".into()).spawn(move || {
            let result = imp::search_pending_offline();
            SEARCH_IN_FLIGHT.store(false, Ordering::Release);
            let _ = tx.send(result);
        });
        if spawned.is_err() {
            SEARCH_IN_FLIGHT.store(false, Ordering::Release);
            return PendingUpdatesOutcome::Failed("threadSpawn".into());
        }
        let started = Instant::now();
        loop {
            match rx.recv_timeout(Duration::from_millis(100)) {
                Ok(Ok(fact)) => return PendingUpdatesOutcome::Found(fact),
                Ok(Err(PlatformError::OperationFailed(code))) => return PendingUpdatesOutcome::Failed(code),
                Ok(Err(PlatformError::PermissionDenied)) => {
                    return PendingUpdatesOutcome::Failed("permissionDenied".into())
                }
                Ok(Err(PlatformError::Unsupported)) => return PendingUpdatesOutcome::Failed("unsupported".into()),
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return PendingUpdatesOutcome::Failed("workerExited".into())
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
            if should_stop() {
                return PendingUpdatesOutcome::Stopped;
            }
            if started.elapsed() >= timeout {
                return PendingUpdatesOutcome::TimedOut;
            }
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (timeout, should_stop, &SEARCH_IN_FLIGHT);
        PendingUpdatesOutcome::Failed("unsupported".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wsc_product_state_decodes_documented_values() {
        assert_eq!(SecurityProductState::from_wsc(0), SecurityProductState::On);
        assert_eq!(SecurityProductState::from_wsc(1), SecurityProductState::Off);
        assert_eq!(SecurityProductState::from_wsc(2), SecurityProductState::Snoozed);
        assert_eq!(SecurityProductState::from_wsc(3), SecurityProductState::Expired);
        assert_eq!(SecurityProductState::from_wsc(42), SecurityProductState::Unknown);
        assert_eq!(signatures_up_to_date_from_wsc(0), Some(false));
        assert_eq!(signatures_up_to_date_from_wsc(1), Some(true));
        assert_eq!(signatures_up_to_date_from_wsc(7), None);
    }

    #[test]
    fn defender_is_recognised_by_guid_or_name() {
        assert!(is_microsoft_defender("{d68ddc3a-831f-4fae-9e44-da132c1acf46}", "x"));
        assert!(is_microsoft_defender("", "Windows Defender"));
        assert!(is_microsoft_defender("", "Microsoft Defender Antivirus"));
        assert!(!is_microsoft_defender("{00000000-0000-0000-0000-000000000000}", "Vendor AV"));
    }

    #[test]
    fn ole_dates_convert_to_unix_ms() {
        // 1970-01-01T00:00:00Z
        assert_eq!(ole_date_to_unix_ms(25_569.0), Some(0));
        // 2000-01-01T12:00:00Z = 36526.5
        assert_eq!(ole_date_to_unix_ms(36_526.5), Some(946_728_000_000));
        assert_eq!(ole_date_to_unix_ms(0.0), None);
        assert_eq!(ole_date_to_unix_ms(f64::NAN), None);
    }

    #[test]
    #[ignore = "reads live Security Center / firewall / Windows Update state"]
    fn smoke_security_center_facts() {
        println!("av = {:?}", list_security_products(SecurityProductKind::Antivirus));
        println!("fw products = {:?}", list_security_products(SecurityProductKind::Firewall));
        println!("fw profiles = {:?}", firewall_profiles());
        println!("wu dates = {:?}", windows_update_dates());
        let started = std::time::Instant::now();
        println!("pending = {:?} in {:?}", search_pending_updates(Duration::from_secs(20), &|| false), started.elapsed());
    }
}
