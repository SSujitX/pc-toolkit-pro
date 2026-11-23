//! Care category: Junk. A fast, conservative slice of Deep Cleanup.
//!
//! Scans only the catalog rules of the system / application / browser groups (no project
//! discovery, no AI models, no development tools) and offers rules that are `recommended` and
//! `found`. The Recycle Bin and anything rooted in Downloads are never offered here. One Care item
//! per rule; fixing executes those rule ids through Deep Cleanup, which re-resolves and
//! re-validates every root (the item id is the only thing trusted).

use std::collections::HashSet;
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

use crate::care::{
    fix_all, skipped, CareCategory, CareCategoryId, CareCategoryScan, CareCategoryState, CareContext, CareFixResult,
    CareFixStatus, CareItem, CareRisk,
};
use crate::cleaner::cancel_cleanup;
use crate::cleaner_deep::{
    execute_deep_cleanup, scan_deep_cleanup_groups, DeepCleanupExecReason, DeepCleanupExecStatus,
    DeepCleanupExecuteRequest, DeepCleanupGroup, DeepCleanupRisk, DeepCleanupRuleResult, DeepCleanupRuleStatus,
};
use crate::shared::{CoreError, CoreResult};

const ID: CareCategoryId = CareCategoryId::Junk;
const GROUPS: [DeepCleanupGroup; 3] = [DeepCleanupGroup::System, DeepCleanupGroup::Application, DeepCleanupGroup::Browser];
/// Rules that must never appear in Care Junk even if a catalog change marks them recommended.
const NEVER_OFFERED: [&str; 2] = ["system.recycleBin", "system.stalePartialDownloads"];
/// Deep Cleanup shares one busy flag with Quick Cleaner / Deep Cleanup pages and possibly another
/// Care category; wait this long for it before reporting `busy`.
const BUSY_WAIT: Duration = Duration::from_secs(30);
const BUSY_POLL: Duration = Duration::from_millis(150);

/// Rule ids offered by the latest Care Junk scan; fixes are limited to these.
static OFFERED: Mutex<Option<HashSet<String>>> = Mutex::new(None);

pub struct JunkCategory;

/// Pure filter: which Deep Cleanup results become Care Junk items.
pub(crate) fn is_offered(rule: &DeepCleanupRuleResult) -> bool {
    GROUPS.contains(&rule.group)
        && rule.recommended
        && rule.status == DeepCleanupRuleStatus::Found
        && rule.bytes > 0
        && !NEVER_OFFERED.contains(&rule.id.as_str())
        && !rule.id.to_ascii_lowercase().contains("download")
}

pub(crate) fn to_item(rule: &DeepCleanupRuleResult) -> CareItem {
    let risk = if rule.risk == DeepCleanupRisk::Safe { CareRisk::Safe } else { CareRisk::Review };
    CareItem {
        id: rule.id.clone(),
        category: ID,
        title_key: Some(rule.name_key.clone()),
        title: None,
        params: Default::default(),
        detail_key: Some(rule.detail_key.clone()),
        detail: None,
        location: None,
        bytes: Some(rule.bytes),
        count: rule.item_count.max(1),
        risk,
        selected: risk == CareRisk::Safe && !rule.needs_admin,
        fixable: true,
        needs_admin: rule.requires_elevation,
        action: None,
    }
}

fn exec_reason(reason: DeepCleanupExecReason) -> &'static str {
    match reason {
        DeepCleanupExecReason::RunningProcesses => "runningProcesses",
        DeepCleanupExecReason::NeedsAdmin => "needsAdmin",
        DeepCleanupExecReason::ItemsBusy => "itemsBusy",
        DeepCleanupExecReason::ItemsFailed => "itemsFailed",
        DeepCleanupExecReason::Cancelled => "cancelled",
        DeepCleanupExecReason::UnknownRule => "unknownItem",
        DeepCleanupExecReason::NotApplicable => "notApplicable",
        DeepCleanupExecReason::ChangedSinceScan => "itemChanged",
        DeepCleanupExecReason::ToolFailed => "failed",
    }
}

pub(crate) fn exec_status(status: DeepCleanupExecStatus) -> CareFixStatus {
    match status {
        DeepCleanupExecStatus::Completed => CareFixStatus::Fixed,
        DeepCleanupExecStatus::Partial => CareFixStatus::Partial,
        DeepCleanupExecStatus::Blocked | DeepCleanupExecStatus::Skipped => CareFixStatus::Skipped,
        DeepCleanupExecStatus::Failed => CareFixStatus::Failed,
    }
}

/// Retry `op` while Deep Cleanup reports busy (bounded, stop-aware).
fn with_busy_retry<T>(ctx: &CareContext, mut op: impl FnMut() -> CoreResult<T>) -> CoreResult<T> {
    let started = Instant::now();
    loop {
        match op() {
            Err(CoreError::OperationBusy) if started.elapsed() < BUSY_WAIT && !ctx.should_stop() => {
                thread::sleep(BUSY_POLL);
            }
            other => return other,
        }
    }
}

impl CareCategory for JunkCategory {
    fn id(&self) -> CareCategoryId {
        ID
    }

    fn default_weight_ms(&self) -> u64 {
        6_000
    }

    fn scan(&self, ctx: &CareContext) -> CoreResult<CareCategoryScan> {
        if ctx.should_stop() {
            return Ok(skipped(ID));
        }
        let scan = with_busy_retry(ctx, || {
            scan_deep_cleanup_groups(&GROUPS, |p| {
                // Deep Cleanup polls its own cancel flag; mirror Care stop / skip into it.
                // The callback only runs while this scan holds the Deep Cleanup busy flag.
                if ctx.should_stop() {
                    cancel_cleanup();
                }
                if !p.current_path.is_empty() {
                    ctx.current(ID, p.current_path);
                }
                ctx.found(ID, p.items_scanned);
            })
        });
        let scan = match scan {
            Ok(scan) => scan,
            Err(CoreError::OperationCancelled) => return Ok(skipped(ID)),
            Err(CoreError::OperationBusy) => {
                return Ok(CareCategoryScan::with_state(ID, CareCategoryState::Skipped, Some("busy")))
            }
            Err(e) => return Err(e),
        };
        if ctx.should_stop() {
            return Ok(skipped(ID));
        }
        let items: Vec<CareItem> = scan.rules.iter().filter(|r| is_offered(r)).map(to_item).collect();
        *OFFERED.lock().unwrap_or_else(|e| e.into_inner()) = Some(items.iter().map(|i| i.id.clone()).collect());
        let total: u64 = items.iter().map(|i| i.count).sum();
        ctx.found(ID, total);
        Ok(CareCategoryScan::from_items(ID, items))
    }

    fn fix(&self, item_ids: &[String], ctx: &CareContext) -> CoreResult<Vec<CareFixResult>> {
        let offered = OFFERED.lock().unwrap_or_else(|e| e.into_inner()).clone().unwrap_or_default();
        let (known, unknown): (Vec<String>, Vec<String>) = item_ids.iter().cloned().partition(|id| offered.contains(id));
        let mut results = fix_all(ID, &unknown, CareFixStatus::Skipped, Some("unknownItem"));
        if known.is_empty() {
            return Ok(results);
        }
        if ctx.should_stop() {
            results.extend(fix_all(ID, &known, CareFixStatus::Skipped, Some("cancelled")));
            return Ok(results);
        }
        let outcome = with_busy_retry(ctx, || {
            execute_deep_cleanup(DeepCleanupExecuteRequest { rule_ids: known.clone() }, |p| {
                if ctx.should_stop() {
                    cancel_cleanup();
                }
                if !p.current_path.is_empty() {
                    ctx.current(ID, p.current_path);
                }
            })
        });
        match outcome {
            Ok(done) => {
                for id in &known {
                    let rule = done.rules.iter().find(|r| &r.id == id);
                    results.push(match rule {
                        Some(r) => CareFixResult {
                            item_id: id.clone(),
                            category: ID,
                            status: exec_status(r.status),
                            reason: r.reason.map(exec_reason).map(str::to_string),
                            freed_bytes: r.freed_bytes,
                            undoable: false,
                        },
                        None => CareFixResult {
                            item_id: id.clone(),
                            category: ID,
                            status: CareFixStatus::Skipped,
                            reason: Some(if done.cancelled { "cancelled" } else { "noResult" }.into()),
                            freed_bytes: 0,
                            undoable: false,
                        },
                    });
                }
            }
            Err(CoreError::OperationBusy) => results.extend(fix_all(ID, &known, CareFixStatus::Skipped, Some("busy"))),
            Err(CoreError::OperationCancelled) => {
                results.extend(fix_all(ID, &known, CareFixStatus::Skipped, Some("cancelled")))
            }
            Err(e) => return Err(e),
        }
        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cleaner_deep::DeepCleanupRuleExtras;

    fn rule(id: &str, group: DeepCleanupGroup, recommended: bool, status: DeepCleanupRuleStatus) -> DeepCleanupRuleResult {
        DeepCleanupRuleResult {
            id: id.into(),
            group,
            name_key: format!("deepCleaner.rules.{id}"),
            detail_key: format!("deepCleaner.rules.{id}Detail"),
            risk: DeepCleanupRisk::Safe,
            bytes: 1024,
            item_count: 3,
            recommended,
            selected: recommended,
            status,
            related_processes: vec![],
            running_processes: vec![],
            requires_elevation: false,
            needs_admin: false,
            limited: false,
            refused_roots: 0,
            extras: DeepCleanupRuleExtras::default(),
        }
    }

    #[test]
    fn offers_only_recommended_found_caches_of_plain_groups() {
        use DeepCleanupGroup::*;
        use DeepCleanupRuleStatus::*;
        assert!(is_offered(&rule("system.userTemp", System, true, Found)));
        assert!(is_offered(&rule("browser.chromeCache", Browser, true, Found)));
        assert!(is_offered(&rule("app.steamCache", Application, true, Found)));
        assert!(!is_offered(&rule("dev.npmCache", Development, true, Found)));
        assert!(!is_offered(&rule("ai.x", Ai, true, Found)));
        assert!(!is_offered(&rule("projects.x", Projects, true, Found)));
        assert!(!is_offered(&rule("system.windowsLogs", System, false, Found)));
        assert!(!is_offered(&rule("system.userTemp", System, true, RequiresClose)));
        assert!(!is_offered(&rule("system.userTemp", System, true, Limited)));
        assert!(!is_offered(&rule("system.recycleBin", System, true, Found)));
        assert!(!is_offered(&rule("system.stalePartialDownloads", System, true, Found)));
        assert!(!is_offered(&rule("system.someDownloadsRule", System, true, Found)));
    }

    #[test]
    fn item_maps_risk_and_admin() {
        let mut r = rule("system.deliveryOptimization", DeepCleanupGroup::System, true, DeepCleanupRuleStatus::Found);
        r.requires_elevation = true;
        r.needs_admin = true;
        let item = to_item(&r);
        assert_eq!(item.risk, CareRisk::Safe);
        assert!(item.needs_admin);
        assert!(!item.selected, "admin-only items are never preselected when not elevated");
        assert_eq!(item.count, 3);
        assert_eq!(item.bytes, Some(1024));
        assert!(item.location.is_none());

        r.risk = DeepCleanupRisk::Recoverable;
        r.needs_admin = false;
        let item = to_item(&r);
        assert_eq!(item.risk, CareRisk::Review);
        assert!(!item.selected);
    }

    #[test]
    fn unknown_ids_are_skipped_without_touching_deep_cleanup() {
        *OFFERED.lock().unwrap() = Some(HashSet::new());
        let out = JunkCategory.fix(&["system.userTemp".into()], &CareContext::detached(false)).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].status, CareFixStatus::Skipped);
        assert_eq!(out[0].reason.as_deref(), Some("unknownItem"));
    }
}
