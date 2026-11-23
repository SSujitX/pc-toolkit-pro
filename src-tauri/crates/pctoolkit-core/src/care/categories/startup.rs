//! Care category: Startup. Reports enabled third-party startup entries (Run values, Startup
//! folder files, logon / boot tasks) with their measured boot impact when Windows recorded one,
//! and entries whose program no longer exists.
//!
//! Honesty rules: entries are never auto-fixed (risk Review, unselected) because whether a
//! program should start with Windows is the user's choice; "disable" uses the same reversible
//! `StartupApproved` / task switch as the Startup Manager. Services are listed only in the
//! Startup Manager, not here — suggesting service changes in a one-click scan is too risky.
//! Broken entries (target missing) are Safe and removed with a backup.

use std::collections::BTreeMap;

use pctoolkit_platform::startup_items::StartupSource;

use crate::care::{
    fix_all, skipped, CareCategory, CareCategoryId, CareCategoryScan, CareContext, CareFixResult, CareFixStatus,
    CareItem, CareRisk,
};
use crate::shared::CoreResult;
use crate::startup::{
    self, StartupActionKind, StartupActionRequest, StartupActionStatus, StartupImpact, StartupItem, StartupState,
};

const ID: CareCategoryId = CareCategoryId::Startup;
const DISABLE_PREFIX: &str = "disable:";
const REMOVE_PREFIX: &str = "remove:";

pub struct StartupCategory;

fn impact_key(impact: StartupImpact) -> &'static str {
    match impact {
        StartupImpact::High => "high",
        StartupImpact::Medium => "medium",
        StartupImpact::Low => "low",
        StartupImpact::NotFlagged => "notFlagged",
        StartupImpact::Unknown => "unknown",
    }
}

fn source_key(source: StartupSource) -> &'static str {
    match source {
        StartupSource::Run => "run",
        StartupSource::RunOnce => "runOnce",
        StartupSource::StartupFolder => "startupFolder",
        StartupSource::ScheduledTask => "scheduledTask",
        StartupSource::Service => "service",
    }
}

/// Which startup items become Care findings (pure; unit-tested).
pub(crate) fn care_items(items: &[StartupItem], is_admin: bool) -> Vec<CareItem> {
    let mut out = Vec::new();
    for item in items {
        let mut params = BTreeMap::new();
        params.insert("source".to_string(), source_key(item.source).to_string());
        if let Some(publisher) = &item.publisher {
            params.insert("publisher".to_string(), publisher.clone());
        }
        let needs_admin = item.requires_admin && !is_admin;
        if item.can_delete {
            out.push(CareItem {
                id: format!("{REMOVE_PREFIX}{}", item.id),
                category: ID,
                title_key: None,
                title: Some(item.name.clone()),
                params,
                detail_key: Some("care.startup.items.broken".into()),
                detail: None,
                location: Some(item.command.clone()),
                bytes: None,
                count: 1,
                risk: CareRisk::Safe,
                selected: !needs_admin,
                fixable: true,
                needs_admin,
                action: None,
            });
            continue;
        }
        let third_party_entry = matches!(
            item.source,
            StartupSource::Run | StartupSource::StartupFolder | StartupSource::ScheduledTask
        );
        if !third_party_entry || item.is_microsoft || item.broken || item.state != StartupState::Enabled || !item.can_toggle {
            continue;
        }
        if let Some(ms) = item.impact_ms {
            params.insert("seconds".to_string(), format!("{:.1}", ms as f64 / 1000.0));
        }
        out.push(CareItem {
            id: format!("{DISABLE_PREFIX}{}", item.id),
            category: ID,
            title_key: None,
            title: Some(item.name.clone()),
            params,
            detail_key: Some(format!("care.startup.items.enabled.{}", impact_key(item.impact))),
            detail: None,
            location: Some(item.command.clone()),
            bytes: None,
            count: 1,
            risk: CareRisk::Review,
            selected: false,
            fixable: true,
            needs_admin,
            action: None,
        });
    }
    out
}

impl CareCategory for StartupCategory {
    fn id(&self) -> CareCategoryId {
        ID
    }

    fn default_weight_ms(&self) -> u64 {
        1_500
    }

    fn scan(&self, ctx: &CareContext) -> CoreResult<CareCategoryScan> {
        if ctx.should_stop() {
            return Ok(skipped(ID));
        }
        ctx.current(ID, "startup");
        let items = startup::current_items();
        if ctx.should_stop() {
            return Ok(skipped(ID));
        }
        let found = care_items(&items, ctx.is_admin);
        ctx.found(ID, found.len() as u64);
        Ok(CareCategoryScan::from_items(ID, found))
    }

    fn fix(&self, item_ids: &[String], ctx: &CareContext) -> CoreResult<Vec<CareFixResult>> {
        // Refresh the Startup Manager's id cache so every fix re-validates against the system.
        let _ = startup::current_items();
        let mut results = Vec::with_capacity(item_ids.len());
        for care_id in item_ids {
            if ctx.should_stop() {
                results.extend(fix_all(ID, std::slice::from_ref(care_id), CareFixStatus::Skipped, Some("cancelled")));
                continue;
            }
            let (action, id) = if let Some(id) = care_id.strip_prefix(DISABLE_PREFIX) {
                (StartupActionKind::Disable, id)
            } else if let Some(id) = care_id.strip_prefix(REMOVE_PREFIX) {
                (StartupActionKind::Delete, id)
            } else {
                results.extend(fix_all(ID, std::slice::from_ref(care_id), CareFixStatus::Skipped, Some("itemChanged")));
                continue;
            };
            let outcome = startup::run_startup_action(StartupActionRequest { id: id.to_string(), action })?;
            let (status, reason) = match outcome.status {
                StartupActionStatus::Done => (CareFixStatus::Fixed, None),
                StartupActionStatus::SkippedNeedAdmin => (CareFixStatus::Skipped, Some("needsAdmin".to_string())),
                StartupActionStatus::NotFound | StartupActionStatus::Unsupported => {
                    (CareFixStatus::Skipped, Some("itemChanged".to_string()))
                }
                StartupActionStatus::Failed => (CareFixStatus::Failed, outcome.reason.or(Some("failed".into()))),
            };
            results.push(CareFixResult {
                item_id: care_id.clone(),
                category: ID,
                status,
                reason,
                freed_bytes: 0,
                // Disabling writes a reversible StartupApproved / task switch; deletes keep a backup.
                undoable: status == CareFixStatus::Fixed,
            });
        }
        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pctoolkit_platform::startup_items::StartupScope;

    fn item(id: &str, source: StartupSource) -> StartupItem {
        StartupItem {
            id: id.into(),
            name: id.into(),
            entry_name: id.into(),
            publisher: Some("Vendor".into()),
            description: None,
            command: "C:\\Vendor\\app.exe".into(),
            target_path: None,
            source,
            scope: StartupScope::User,
            state: StartupState::Enabled,
            service_start: None,
            is_microsoft: false,
            requires_admin: false,
            can_toggle: true,
            can_delay: true,
            can_delete: false,
            broken: false,
            is_new: false,
            impact: StartupImpact::High,
            impact_ms: Some(1_500),
            disabled_at_ms: None,
            delay_seconds: None,
        }
    }

    #[test]
    fn enabled_third_party_entries_are_review_and_unselected() {
        let found = care_items(&[item("a", StartupSource::Run)], false);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].id, "disable:a");
        assert_eq!(found[0].risk, CareRisk::Review);
        assert!(!found[0].selected);
        assert_eq!(found[0].detail_key.as_deref(), Some("care.startup.items.enabled.high"));
        assert_eq!(found[0].params.get("seconds").map(String::as_str), Some("1.5"));
    }

    #[test]
    fn microsoft_disabled_and_service_items_are_not_reported() {
        let mut ms = item("ms", StartupSource::Run);
        ms.is_microsoft = true;
        let mut off = item("off", StartupSource::StartupFolder);
        off.state = StartupState::Disabled;
        let svc = item("svc", StartupSource::Service);
        assert!(care_items(&[ms, off, svc], false).is_empty());
    }

    #[test]
    fn broken_entries_are_safe_and_admin_ones_unselected() {
        let mut broken = item("b", StartupSource::Run);
        broken.broken = true;
        broken.can_delete = true;
        let mut machine = broken.clone();
        machine.id = "m".into();
        machine.requires_admin = true;
        let found = care_items(&[broken, machine], false);
        assert_eq!(found[0].id, "remove:b");
        assert_eq!(found[0].risk, CareRisk::Safe);
        assert!(found[0].selected);
        assert!(found[1].needs_admin);
        assert!(!found[1].selected);
    }
}
