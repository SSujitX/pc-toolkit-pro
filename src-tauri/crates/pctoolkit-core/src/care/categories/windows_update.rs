//! Care category: Windows Update status (read-only).
//!
//! Reports a disabled update service, a pending restart, stale search / installation dates, and
//! not-installed updates from the locally cached catalog (offline WUA search, ~20 s cap). Nothing
//! is installed or changed here; every item is Attention with the `openWindowsUpdate` action.

use std::time::Duration;

use pctoolkit_platform::security_center::{
    search_pending_updates, windows_update_dates, PendingUpdatesOutcome, WindowsUpdateDates,
};
use pctoolkit_platform::{read_registry_value, registry_key_exists, RegistryKeyPath, RegistryRoot, RegistryView};

use crate::care::{
    fix_all, skipped, CareCategory, CareCategoryId, CareCategoryScan, CareContext, CareFixResult, CareFixStatus, CareItem,
    CareRisk,
};
use crate::shared::CoreResult;

const ID: CareCategoryId = CareCategoryId::WindowsUpdate;
const SEARCH_TIMEOUT: Duration = Duration::from_secs(20);
/// Windows checks daily; a week without a successful check deserves attention.
pub const STALE_CHECK_DAYS: i64 = 7;
/// Monthly cumulative updates: 45 days without an installation means at least one was missed.
pub const STALE_INSTALL_DAYS: i64 = 45;

pub struct WindowsUpdateCategory;

/// Facts gathered by the scan (injectable for tests).
#[derive(Debug, Clone, Default)]
pub struct UpdateFacts {
    pub service_disabled: bool,
    pub reboot_pending: bool,
    pub dates: Option<WindowsUpdateDates>,
    pub pending: Option<PendingUpdatesOutcome>,
}

/// One finding: locale key suffix + params.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateFinding {
    pub key: &'static str,
    pub params: Vec<(&'static str, String)>,
}

fn days_since(ms: i64, now_ms: i64) -> i64 {
    ((now_ms - ms).max(0)) / 86_400_000
}

/// Pure decision over gathered facts.
pub fn update_findings(facts: &UpdateFacts, now_ms: i64) -> Vec<UpdateFinding> {
    let mut out = Vec::new();
    let f = |key, params| UpdateFinding { key, params };
    if facts.service_disabled {
        out.push(f("serviceDisabled", vec![]));
    }
    if facts.reboot_pending {
        out.push(f("rebootPending", vec![]));
    }
    if let Some(dates) = facts.dates {
        match dates.last_search_success_ms {
            Some(ms) if days_since(ms, now_ms) >= STALE_CHECK_DAYS => {
                out.push(f("checkStale", vec![("days", days_since(ms, now_ms).to_string())]))
            }
            Some(_) => {}
            None => out.push(f("neverChecked", vec![])),
        }
        if let Some(ms) = dates.last_installation_success_ms {
            if days_since(ms, now_ms) >= STALE_INSTALL_DAYS {
                out.push(f("installStale", vec![("days", days_since(ms, now_ms).to_string())]));
            }
        }
    }
    match &facts.pending {
        Some(PendingUpdatesOutcome::Found(p)) if p.total > 0 => out.push(f(
            "updatesPending",
            vec![("count", p.total.to_string()), ("security", p.security.to_string())],
        )),
        Some(PendingUpdatesOutcome::TimedOut) => out.push(f("checkSlow", vec![])),
        Some(PendingUpdatesOutcome::Failed(_)) => out.push(f("checkFailed", vec![])),
        _ => {}
    }
    out
}

fn item(finding: UpdateFinding) -> CareItem {
    CareItem {
        id: finding.key.to_string(),
        category: ID,
        title_key: Some(format!("care.windowsUpdate.{}.title", finding.key)),
        title: None,
        params: finding.params.into_iter().map(|(k, v)| (k.to_string(), v)).collect(),
        detail_key: Some(format!("care.windowsUpdate.{}.detail", finding.key)),
        detail: None,
        location: None,
        bytes: None,
        count: 1,
        risk: CareRisk::Info,
        selected: false,
        fixable: false,
        needs_admin: false,
        action: Some("openWindowsUpdate".to_string()),
    }
}

fn lm(subkey: &str) -> RegistryKeyPath {
    RegistryKeyPath::new(RegistryRoot::LocalMachine, RegistryView::Native64, subkey)
}

impl CareCategory for WindowsUpdateCategory {
    fn id(&self) -> CareCategoryId {
        ID
    }

    fn default_weight_ms(&self) -> u64 {
        4_000
    }

    fn scan(&self, ctx: &CareContext) -> CoreResult<CareCategoryScan> {
        ctx.current(ID, "Windows Update");
        let service_disabled = read_registry_value(&lm("SYSTEM\\CurrentControlSet\\Services\\wuauserv"), "Start")
            .is_some_and(|v| v.trim() == "4");
        let reboot_pending = registry_key_exists(&lm(
            "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\WindowsUpdate\\Auto Update\\RebootRequired",
        )) || registry_key_exists(&lm(
            "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Component Based Servicing\\RebootPending",
        ));
        let dates = windows_update_dates().ok();
        if ctx.should_stop() {
            return Ok(skipped(ID));
        }
        // A disabled service cannot search; do not wait for a guaranteed failure.
        let pending = if service_disabled {
            None
        } else {
            let outcome = search_pending_updates(SEARCH_TIMEOUT, &|| ctx.should_stop());
            if outcome == PendingUpdatesOutcome::Stopped {
                return Ok(skipped(ID));
            }
            Some(outcome)
        };
        let facts = UpdateFacts { service_disabled, reboot_pending, dates, pending };
        let items: Vec<CareItem> = update_findings(&facts, chrono::Utc::now().timestamp_millis())
            .into_iter()
            .map(item)
            .collect();
        ctx.found(ID, items.len() as u64);
        Ok(CareCategoryScan::from_items(ID, items))
    }

    fn fix(&self, item_ids: &[String], _ctx: &CareContext) -> CoreResult<Vec<CareFixResult>> {
        Ok(fix_all(ID, item_ids, CareFixStatus::Skipped, Some("notFixable")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pctoolkit_platform::security_center::PendingUpdatesFact;

    const DAY: i64 = 86_400_000;

    fn keys(facts: &UpdateFacts, now: i64) -> Vec<&'static str> {
        update_findings(facts, now).into_iter().map(|f| f.key).collect()
    }

    #[test]
    fn healthy_machine_has_no_findings() {
        let now = 100 * DAY;
        let facts = UpdateFacts {
            dates: Some(WindowsUpdateDates {
                last_search_success_ms: Some(now - DAY),
                last_installation_success_ms: Some(now - 10 * DAY),
            }),
            pending: Some(PendingUpdatesOutcome::Found(PendingUpdatesFact { total: 0, security: 0 })),
            ..Default::default()
        };
        assert!(keys(&facts, now).is_empty());
    }

    #[test]
    fn findings_cover_each_fact() {
        let now = 100 * DAY;
        let facts = UpdateFacts {
            service_disabled: true,
            reboot_pending: true,
            dates: Some(WindowsUpdateDates {
                last_search_success_ms: Some(now - 9 * DAY),
                last_installation_success_ms: Some(now - 60 * DAY),
            }),
            pending: Some(PendingUpdatesOutcome::Found(PendingUpdatesFact { total: 3, security: 1 })),
        };
        assert_eq!(
            keys(&facts, now),
            vec!["serviceDisabled", "rebootPending", "checkStale", "installStale", "updatesPending"]
        );
        let found = update_findings(&facts, now);
        assert_eq!(found[2].params, vec![("days", "9".to_string())]);
        assert_eq!(found[4].params, vec![("count", "3".to_string()), ("security", "1".to_string())]);
    }

    #[test]
    fn slow_and_failed_searches_are_reported_honestly() {
        let slow = UpdateFacts { pending: Some(PendingUpdatesOutcome::TimedOut), ..Default::default() };
        assert_eq!(keys(&slow, 0), vec!["checkSlow"]);
        let failed = UpdateFacts { pending: Some(PendingUpdatesOutcome::Failed("x".into())), ..Default::default() };
        assert_eq!(keys(&failed, 0), vec!["checkFailed"]);
        let never = UpdateFacts { dates: Some(WindowsUpdateDates::default()), ..Default::default() };
        assert_eq!(keys(&never, 0), vec!["neverChecked"]);
    }

    #[test]
    #[ignore = "reads live Windows Update state (read-only, offline search)"]
    fn smoke_windows_update_scan() {
        let ctx = CareContext::detached(pctoolkit_platform::is_user_admin());
        let started = std::time::Instant::now();
        let scan = WindowsUpdateCategory.scan(&ctx).unwrap();
        println!("windowsUpdate: state={:?} elapsed={:?}", scan.state, started.elapsed());
        for item in &scan.items {
            println!("  {} {:?}", item.id, item.params);
        }
    }
}
