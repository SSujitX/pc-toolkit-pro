//! Care category: Software updates, through Windows Package Manager (`winget`).
//!
//! Scan is the read-only `winget upgrade` listing (sources agreed, no interactivity, ~45 s
//! timeout). Each upgradable package is one `Review` item — never auto-fixed, never preselected.
//! Fix upgrades the chosen packages one at a time with `winget upgrade --id <id> --exact --silent`;
//! installers that need elevation show their own UAC prompt (we never elevate silently).
//! A running upgrade is not killed when the user stops Care (that could leave a half-installed
//! program); the stop takes effect before the next package.
//!
//! winget missing → NotApplicable (`toolUnavailable`). Listing timeout → Attention (`timeout`);
//! other listing failures (for example an unreachable source) → Attention (`failed`).

use std::collections::{BTreeMap, HashMap};
use std::sync::Mutex;
use std::time::Duration;

use pctoolkit_platform::winget::{
    is_valid_winget_id, winget_list_upgrades, winget_upgrade_package, WingetReason, WingetUpgrade,
    WingetUpgradeStatus,
};

use crate::care::{
    fix_all, skipped, CareCategory, CareCategoryId, CareCategoryScan, CareCategoryState, CareContext, CareFixResult,
    CareFixStatus, CareItem, CareRisk,
};
use crate::shared::CoreResult;

const ID: CareCategoryId = CareCategoryId::Software;
const LIST_TIMEOUT: Duration = Duration::from_secs(45);
const UPGRADE_TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// Item id → (winget package id, display name) from the latest scan.
static TARGETS: Mutex<Option<HashMap<String, (String, String)>>> = Mutex::new(None);

pub struct SoftwareCategory;

pub(crate) fn item_id(package_id: &str) -> String {
    format!("winget:{package_id}")
}

pub(crate) fn to_item(row: &WingetUpgrade) -> CareItem {
    let mut params = BTreeMap::new();
    params.insert("name".to_string(), row.name.clone());
    params.insert("id".to_string(), row.id.clone());
    params.insert("current".to_string(), row.current_version.clone());
    params.insert("available".to_string(), row.available_version.clone());
    if let Some(source) = &row.source {
        params.insert("source".to_string(), source.clone());
    }
    let fixable = !row.id_truncated && is_valid_winget_id(&row.id);
    CareItem {
        id: item_id(&row.id),
        category: ID,
        title_key: None,
        title: Some(row.name.clone()),
        params,
        detail_key: Some(
            if fixable { "care.software.updateAvailable" } else { "care.software.updateIdTruncated" }.to_string(),
        ),
        detail: None,
        location: None,
        bytes: None,
        count: 1,
        risk: if fixable { CareRisk::Review } else { CareRisk::Info },
        selected: false,
        fixable,
        needs_admin: false,
        action: fixable.then(|| "upgrade".to_string()),
    }
}

pub(crate) fn listing_failure_state(reason: WingetReason) -> (CareCategoryState, &'static str) {
    match reason {
        WingetReason::ToolUnavailable => (CareCategoryState::NotApplicable, "toolUnavailable"),
        WingetReason::Timeout => (CareCategoryState::Attention, "timeout"),
        WingetReason::Cancelled => (CareCategoryState::Skipped, "skipped"),
        WingetReason::InvalidOutput | WingetReason::Failed => (CareCategoryState::Attention, "failed"),
    }
}

pub(crate) fn upgrade_result(status: WingetUpgradeStatus) -> (CareFixStatus, Option<&'static str>) {
    match status {
        WingetUpgradeStatus::Upgraded => (CareFixStatus::Fixed, None),
        WingetUpgradeStatus::RestartRequired => (CareFixStatus::Fixed, Some("restartRequired")),
        WingetUpgradeStatus::NotApplicable => (CareFixStatus::Skipped, Some("notApplicable")),
        WingetUpgradeStatus::PackageInUse => (CareFixStatus::Skipped, Some("runningProcesses")),
        WingetUpgradeStatus::NotFound => (CareFixStatus::Skipped, Some("itemChanged")),
        WingetUpgradeStatus::Cancelled => (CareFixStatus::Skipped, Some("cancelled")),
        WingetUpgradeStatus::Timeout => (CareFixStatus::Failed, Some("timeout")),
        WingetUpgradeStatus::ToolUnavailable => (CareFixStatus::Failed, Some("toolUnavailable")),
        WingetUpgradeStatus::Failed => (CareFixStatus::Failed, Some("failed")),
    }
}

impl CareCategory for SoftwareCategory {
    fn id(&self) -> CareCategoryId {
        ID
    }

    fn default_weight_ms(&self) -> u64 {
        15_000
    }

    fn scan(&self, ctx: &CareContext) -> CoreResult<CareCategoryScan> {
        if ctx.should_stop() {
            return Ok(skipped(ID));
        }
        ctx.current(ID, "winget");
        let rows = match winget_list_upgrades(LIST_TIMEOUT, &|| ctx.should_stop()) {
            Ok(rows) => rows,
            Err(error) => {
                let (state, reason) = listing_failure_state(error.reason);
                return Ok(CareCategoryScan::with_state(ID, state, Some(reason)));
            }
        };
        let items: Vec<CareItem> = rows.iter().map(to_item).collect();
        let targets = rows
            .iter()
            .filter(|r| !r.id_truncated && is_valid_winget_id(&r.id))
            .map(|r| (item_id(&r.id), (r.id.clone(), r.name.clone())))
            .collect();
        *TARGETS.lock().unwrap_or_else(|e| e.into_inner()) = Some(targets);
        ctx.found(ID, items.len() as u64);
        Ok(CareCategoryScan::from_items(ID, items))
    }

    fn fix(&self, item_ids: &[String], ctx: &CareContext) -> CoreResult<Vec<CareFixResult>> {
        let targets = TARGETS.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let Some(targets) = targets else {
            return Ok(fix_all(ID, item_ids, CareFixStatus::Skipped, Some("unknownItem")));
        };
        let mut out = Vec::new();
        for id in item_ids {
            let (status, reason) = if ctx.should_stop() {
                (CareFixStatus::Skipped, Some("cancelled"))
            } else if let Some((package, name)) = targets.get(id) {
                ctx.current(ID, name.clone());
                // Never kill a running installer on stop; it would leave a broken install.
                upgrade_result(winget_upgrade_package(package, UPGRADE_TIMEOUT, &|| false).status)
            } else {
                (CareFixStatus::Skipped, Some("unknownItem"))
            };
            out.push(CareFixResult {
                item_id: id.clone(),
                category: ID,
                status,
                reason: reason.map(str::to_string),
                freed_bytes: 0,
                undoable: false,
            });
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    fn row(id: &str, truncated: bool) -> WingetUpgrade {
        WingetUpgrade {
            name: "Tool".into(),
            id: id.into(),
            id_truncated: truncated,
            current_version: "1.0".into(),
            available_version: "2.0".into(),
            source: Some("winget".into()),
        }
    }

    #[test]
    fn upgrades_are_review_and_never_preselected() {
        let item = to_item(&row("Acme.Tool", false));
        assert_eq!(item.id, "winget:Acme.Tool");
        assert_eq!(item.risk, CareRisk::Review);
        assert!(item.fixable && !item.selected && !item.needs_admin);
        assert_eq!(item.params.get("available").map(String::as_str), Some("2.0"));
    }

    #[test]
    fn truncated_ids_are_informational() {
        let item = to_item(&row("Microsoft.VisualS…", true));
        assert!(!item.fixable);
        assert_eq!(item.risk, CareRisk::Info);
        assert_eq!(item.action, None);
    }

    #[test]
    fn failures_map_to_honest_states() {
        assert_eq!(listing_failure_state(WingetReason::ToolUnavailable).0, CareCategoryState::NotApplicable);
        assert_eq!(listing_failure_state(WingetReason::Timeout), (CareCategoryState::Attention, "timeout"));
        assert_eq!(listing_failure_state(WingetReason::Failed).0, CareCategoryState::Attention);
        assert_eq!(upgrade_result(WingetUpgradeStatus::PackageInUse).1, Some("runningProcesses"));
        assert_eq!(upgrade_result(WingetUpgradeStatus::Upgraded).0, CareFixStatus::Fixed);
    }

    #[test]
    #[ignore = "runs the read-only winget listing on this machine"]
    fn smoke_scan() {
        let started = Instant::now();
        let scan = SoftwareCategory.scan(&CareContext::detached(false)).unwrap();
        eprintln!(
            "software state={:?} reason={:?} items={} fixable={} elapsed={:?}",
            scan.state,
            scan.reason,
            scan.items.len(),
            scan.items.iter().filter(|i| i.fixable).count(),
            started.elapsed()
        );
    }
}
