//! Care category: Security — a reversible hardening checklist (see `care::security_checks`).
//!
//! Scan reports every applicable check that is not compliant. Fix re-evaluates the check, stores
//! the previous value (or "absent") in the undo journal, writes the recommended value, and then
//! verifies the effective state: a policy that still overrides the setting is reported as
//! `Partial` / `overriddenByPolicy` instead of pretending success.

use std::cell::OnceCell;

use pctoolkit_platform::registry_settings::{read_registry_data, write_registry_data, RegistryData};
use pctoolkit_platform::security_center::{list_security_products, SecurityProductKind, SecurityProductState};
use pctoolkit_platform::{delete_registry_value, registry_key_exists, PlatformError, RegistryKeyPath, RegistryRoot, RegistryView};

use crate::care::security_checks::{
    evaluate, find_check, Evaluation, SecurityCheck, SecurityFact, SecurityFix, SettingReader, SettingRef, SECURITY_CHECKS,
};
use crate::care::undo::{discard_record, record_change, CareUndoChange};
use crate::care::{
    skipped, CareCategory, CareCategoryId, CareCategoryScan, CareContext, CareFixResult, CareFixStatus, CareItem, CareRisk,
};
use crate::shared::CoreResult;

const ID: CareCategoryId = CareCategoryId::Security;

pub struct SecurityCategory;

/// Reads the live registry; the Defender fact is resolved lazily once per reader.
struct LiveReader {
    defender_active: OnceCell<bool>,
}

impl LiveReader {
    fn new() -> Self {
        Self { defender_active: OnceCell::new() }
    }
}

impl SettingReader for LiveReader {
    fn read(&self, setting: &SettingRef) -> Result<Option<RegistryData>, ()> {
        read_registry_data(&setting.key_path(), setting.name).map_err(|_| ())
    }

    fn key_exists(&self, root: RegistryRoot, subkey: &str) -> bool {
        registry_key_exists(&RegistryKeyPath::new(root, RegistryView::Native64, subkey))
    }

    fn fact(&self, fact: SecurityFact) -> bool {
        match fact {
            SecurityFact::DefenderActive => *self.defender_active.get_or_init(|| {
                list_security_products(SecurityProductKind::Antivirus).is_ok_and(|products| {
                    products.iter().any(|p| p.is_microsoft_defender && p.state == SecurityProductState::On)
                })
            }),
        }
    }
}

/// Display text for a registry value in the details panel ("1", "Warn", "not set", …).
fn value_text(data: &RegistryData) -> String {
    match data {
        RegistryData::Dword(v) => v.to_string(),
        RegistryData::String(s) | RegistryData::ExpandString(s) => s.clone(),
        RegistryData::Other(kind) => format!("type {kind}"),
    }
}

fn item_for(check: &SecurityCheck, ctx: &CareContext, reader: &dyn SettingReader) -> CareItem {
    let fixable = check.fix.is_some();
    // Facts for the details panel: what is set now, and what Fix would change it to.
    let mut params = std::collections::BTreeMap::new();
    let current = match reader.read(&check.setting) {
        Ok(Some(data)) => value_text(&data),
        Ok(None) => "notSet".to_string(),
        Err(()) => "unreadable".to_string(),
    };
    params.insert("currentValue".to_string(), current);
    if let Some(fix) = check.fix {
        let change = match fix {
            SecurityFix::Write(desired) => value_text(&desired.to_data()),
            SecurityFix::Delete => "removePolicy".to_string(),
        };
        params.insert("fixValue".to_string(), change);
    }
    CareItem {
        id: check.id.to_string(),
        category: ID,
        title_key: Some(format!("care.security.checks.{}.title", check.id)),
        title: None,
        params,
        detail_key: Some(format!("care.security.checks.{}.detail", check.id)),
        detail: None,
        location: Some(check.setting.display()),
        bytes: None,
        count: 1,
        risk: check.risk,
        // Smart selection never preselects what this process cannot apply.
        selected: fixable && check.risk == CareRisk::Safe && (!check.needs_admin || ctx.is_admin),
        fixable,
        needs_admin: check.needs_admin && fixable,
        action: check.action.map(str::to_string),
    }
}

fn result(item_id: &str, status: CareFixStatus, reason: Option<&str>, undoable: bool) -> CareFixResult {
    CareFixResult {
        item_id: item_id.to_string(),
        category: ID,
        status,
        reason: reason.map(str::to_string),
        freed_bytes: 0,
        undoable,
    }
}

fn platform_reason(error: &PlatformError) -> &'static str {
    match error {
        PlatformError::PermissionDenied => "permissionDenied",
        PlatformError::Unsupported => "unsupported",
        PlatformError::OperationFailed(_) => "failed",
    }
}

fn fix_one(check: &SecurityCheck, ctx: &CareContext) -> CareFixResult {
    let Some(fix) = check.fix else {
        return result(check.id, CareFixStatus::Skipped, Some("notFixable"), false);
    };
    let reader = LiveReader::new();
    match evaluate(check, &reader) {
        Evaluation::NonCompliant => {}
        _ => return result(check.id, CareFixStatus::Skipped, Some("itemChanged"), false),
    }
    if check.needs_admin && !ctx.is_admin {
        return result(check.id, CareFixStatus::Skipped, Some("needsAdmin"), false);
    }
    let key = check.setting.key_path();
    let previous = match read_registry_data(&key, check.setting.name) {
        Ok(v) => v,
        Err(e) => return result(check.id, CareFixStatus::Failed, Some(platform_reason(&e)), false),
    };
    if previous.as_ref().is_some_and(|v| !v.is_writable()) {
        // Could not be restored exactly, so do not change it.
        return result(check.id, CareFixStatus::Skipped, Some("unsupportedValueType"), false);
    }
    let applied = match fix {
        SecurityFix::Write(desired) => Some(desired.to_data()),
        SecurityFix::Delete => None,
    };
    let change = CareUndoChange::RegistryValue {
        key: key.clone(),
        name: check.setting.name.to_string(),
        previous,
        applied: applied.clone(),
    };
    let record_id = match record_change(ID, check.id, check.needs_admin, change) {
        Ok(id) => id,
        // No undo record → no change.
        Err(_) => return result(check.id, CareFixStatus::Failed, Some("undoUnavailable"), false),
    };
    let outcome = match &applied {
        Some(value) => write_registry_data(&key, check.setting.name, value),
        None => delete_registry_value(&key, check.setting.name),
    };
    if let Err(e) = outcome {
        discard_record(&record_id);
        return result(check.id, CareFixStatus::Failed, Some(platform_reason(&e)), false);
    }
    match evaluate(check, &LiveReader::new()) {
        Evaluation::Compliant => result(check.id, CareFixStatus::Fixed, None, true),
        _ => result(check.id, CareFixStatus::Partial, Some("overriddenByPolicy"), true),
    }
}

impl CareCategory for SecurityCategory {
    fn id(&self) -> CareCategoryId {
        ID
    }

    fn default_weight_ms(&self) -> u64 {
        400
    }

    fn scan(&self, ctx: &CareContext) -> CoreResult<CareCategoryScan> {
        let reader = LiveReader::new();
        let mut items = Vec::new();
        for check in SECURITY_CHECKS {
            if ctx.should_stop() {
                return Ok(skipped(ID));
            }
            ctx.current(ID, check.setting.display());
            if evaluate(check, &reader) == Evaluation::NonCompliant {
                items.push(item_for(check, ctx, &reader));
                ctx.found(ID, items.len() as u64);
            }
        }
        Ok(CareCategoryScan::from_items(ID, items))
    }

    fn fix(&self, item_ids: &[String], ctx: &CareContext) -> CoreResult<Vec<CareFixResult>> {
        let mut results = Vec::new();
        for id in item_ids {
            if ctx.should_stop() {
                results.push(result(id, CareFixStatus::Skipped, Some("cancelled"), false));
                continue;
            }
            match find_check(id) {
                Some(check) => results.push(fix_one(check, ctx)),
                None => results.push(result(id, CareFixStatus::Skipped, Some("itemChanged"), false)),
            }
        }
        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "reads the live registry and Security Center (read-only)"]
    fn smoke_security_scan() {
        let ctx = CareContext::detached(pctoolkit_platform::is_user_admin());
        let started = std::time::Instant::now();
        let scan = SecurityCategory.scan(&ctx).unwrap();
        println!("security: state={:?} items={} elapsed={:?}", scan.state, scan.items.len(), started.elapsed());
        for item in &scan.items {
            println!("  {} risk={:?} selected={} admin={} fixable={}", item.id, item.risk, item.selected, item.needs_admin, item.fixable);
        }
    }
}
