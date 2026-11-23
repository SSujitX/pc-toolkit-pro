//! Care category: Protection — antivirus status (Windows Security Center) and Windows Defender
//! Firewall profiles (`INetFwPolicy2`).
//!
//! Antivirus problems are never "fixed" from here: toggling Defender through PowerShell fails under
//! Tamper Protection and third-party products must be handled in their own UI, so those items
//! are Attention with the `openWindowsSecurity` action. A disabled firewall profile is fixable
//! (admin) with an undo record; a profile that stays off because of Group Policy is reported as
//! `Partial` / `overriddenByPolicy`.

use pctoolkit_platform::security_center::{
    firewall_profiles, list_security_products, set_firewall_profile_enabled, FirewallProfileFact, FirewallProfileKind,
    SecurityProductFact, SecurityProductKind, SecurityProductState,
};
use pctoolkit_platform::PlatformError;

use crate::care::undo::{discard_record, record_change, CareUndoChange};
use crate::care::{
    skipped, CareCategory, CareCategoryId, CareCategoryScan, CareContext, CareFixResult, CareFixStatus,
    CareItem, CareRisk,
};
use crate::shared::CoreResult;

const ID: CareCategoryId = CareCategoryId::Protection;
const OPEN_SECURITY: &str = "openWindowsSecurity";

pub struct ProtectionCategory;

/// Antivirus finding (pure decision over WSC facts).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AntivirusFinding {
    NoAntivirus,
    Off { name: String },
    Expired { name: String },
    Outdated { name: String },
}

/// Decide antivirus findings. When at least one product is on, products that are off are expected
/// (Defender steps aside for a third-party antivirus) and not reported.
pub fn antivirus_findings(products: &[SecurityProductFact]) -> Vec<AntivirusFinding> {
    if products.is_empty() {
        return vec![AntivirusFinding::NoAntivirus];
    }
    let any_on = products.iter().any(|p| p.state == SecurityProductState::On);
    let mut out = Vec::new();
    for p in products {
        match p.state {
            SecurityProductState::On => {
                if p.signatures_up_to_date == Some(false) {
                    out.push(AntivirusFinding::Outdated { name: p.name.clone() });
                }
            }
            SecurityProductState::Expired => out.push(AntivirusFinding::Expired { name: p.name.clone() }),
            SecurityProductState::Off | SecurityProductState::Snoozed if !any_on => {
                out.push(AntivirusFinding::Off { name: p.name.clone() })
            }
            _ => {}
        }
    }
    out
}

/// Firewall profiles to report. A third-party firewall that Security Center reports as on owns
/// protection, so Windows Firewall profiles being off is expected then.
pub fn firewall_findings(
    profiles: &[FirewallProfileFact],
    firewall_products: &[SecurityProductFact],
) -> Vec<FirewallProfileKind> {
    let third_party_on = firewall_products
        .iter()
        .any(|p| p.state == SecurityProductState::On && !p.name.to_ascii_lowercase().contains("windows"));
    if third_party_on {
        return Vec::new();
    }
    profiles.iter().filter(|p| !p.enabled).map(|p| p.profile).collect()
}

fn short_hash(text: &str) -> String {
    // FNV-1a: stable id without exposing the product name in ids.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in text.to_ascii_lowercase().bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{hash:016x}")
}

fn info_item(id: String, key: &str, name: Option<&str>) -> CareItem {
    let mut params = std::collections::BTreeMap::new();
    if let Some(name) = name {
        params.insert("name".to_string(), name.to_string());
    }
    CareItem {
        id,
        category: ID,
        title_key: Some(format!("care.protection.{key}.title")),
        title: None,
        params,
        detail_key: Some(format!("care.protection.{key}.detail")),
        detail: None,
        location: name.map(str::to_string),
        bytes: None,
        count: 1,
        risk: CareRisk::Info,
        selected: false,
        fixable: false,
        needs_admin: false,
        action: Some(OPEN_SECURITY.to_string()),
    }
}

fn firewall_item(profile: FirewallProfileKind, ctx: &CareContext) -> CareItem {
    let key = format!("firewallOff.{}", profile.as_str());
    CareItem {
        id: format!("firewall:{}", profile.as_str()),
        category: ID,
        title_key: Some(format!("care.protection.{key}.title")),
        title: None,
        params: Default::default(),
        detail_key: Some(format!("care.protection.{key}.detail")),
        detail: None,
        location: None,
        bytes: None,
        count: 1,
        risk: CareRisk::Safe,
        selected: ctx.is_admin,
        fixable: true,
        needs_admin: true,
        action: Some(OPEN_SECURITY.to_string()),
    }
}

fn profile_from_id(id: &str) -> Option<FirewallProfileKind> {
    let name = id.strip_prefix("firewall:")?;
    FirewallProfileKind::ALL.into_iter().find(|p| p.as_str() == name)
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

fn profile_enabled(profile: FirewallProfileKind) -> Result<Option<bool>, PlatformError> {
    Ok(firewall_profiles()?.into_iter().find(|p| p.profile == profile).map(|p| p.enabled))
}

fn fix_firewall(item_id: &str, profile: FirewallProfileKind, ctx: &CareContext) -> CareFixResult {
    match profile_enabled(profile) {
        Ok(Some(false)) => {}
        Ok(_) => return result(item_id, CareFixStatus::Skipped, Some("itemChanged"), false),
        Err(_) => return result(item_id, CareFixStatus::Failed, Some("firewallUnavailable"), false),
    }
    if !ctx.is_admin {
        return result(item_id, CareFixStatus::Skipped, Some("needsAdmin"), false);
    }
    let change = CareUndoChange::FirewallProfile { profile, previous_enabled: false, applied_enabled: true };
    let Ok(record_id) = record_change(ID, item_id, true, change) else {
        return result(item_id, CareFixStatus::Failed, Some("undoUnavailable"), false);
    };
    if let Err(e) = set_firewall_profile_enabled(profile, true) {
        discard_record(&record_id);
        let reason = if matches!(e, PlatformError::PermissionDenied) { "permissionDenied" } else { "failed" };
        return result(item_id, CareFixStatus::Failed, Some(reason), false);
    }
    match profile_enabled(profile) {
        Ok(Some(true)) => result(item_id, CareFixStatus::Fixed, None, true),
        _ => result(item_id, CareFixStatus::Partial, Some("overriddenByPolicy"), true),
    }
}

impl CareCategory for ProtectionCategory {
    fn id(&self) -> CareCategoryId {
        ID
    }

    fn default_weight_ms(&self) -> u64 {
        600
    }

    fn scan(&self, ctx: &CareContext) -> CoreResult<CareCategoryScan> {
        let mut items = Vec::new();

        ctx.current(ID, "Windows Security Center");
        let antivirus = list_security_products(SecurityProductKind::Antivirus);
        if ctx.should_stop() {
            return Ok(skipped(ID));
        }
        if antivirus.is_err() {
            // Security Center is missing (for example Server SKUs): say so instead of a green check.
            items.push(info_item("av:unavailable".into(), "securityCenterUnavailable", None));
        }
        if let Ok(products) = &antivirus {
            for finding in antivirus_findings(products) {
                items.push(match finding {
                    AntivirusFinding::NoAntivirus => info_item("av:none".into(), "noAntivirus", None),
                    AntivirusFinding::Off { name } => {
                        info_item(format!("av:{}:off", short_hash(&name)), "antivirusOff", Some(&name))
                    }
                    AntivirusFinding::Expired { name } => {
                        info_item(format!("av:{}:expired", short_hash(&name)), "antivirusExpired", Some(&name))
                    }
                    AntivirusFinding::Outdated { name } => {
                        info_item(format!("av:{}:outdated", short_hash(&name)), "antivirusOutdated", Some(&name))
                    }
                });
            }
        }

        ctx.current(ID, "Windows Defender Firewall");
        let firewall_products = list_security_products(SecurityProductKind::Firewall).unwrap_or_default();
        match firewall_profiles() {
            Ok(profiles) => {
                for profile in firewall_findings(&profiles, &firewall_products) {
                    items.push(firewall_item(profile, ctx));
                }
            }
            Err(_) => items.push(info_item("firewall:unavailable".into(), "firewallUnavailable", None)),
        }
        ctx.found(ID, items.len() as u64);

        Ok(CareCategoryScan::from_items(ID, items))
    }

    fn fix(&self, item_ids: &[String], ctx: &CareContext) -> CoreResult<Vec<CareFixResult>> {
        let mut results = Vec::new();
        for id in item_ids {
            if ctx.should_stop() {
                results.push(result(id, CareFixStatus::Skipped, Some("cancelled"), false));
                continue;
            }
            match profile_from_id(id) {
                Some(profile) => results.push(fix_firewall(id, profile, ctx)),
                None => results.push(result(id, CareFixStatus::Skipped, Some("notFixable"), false)),
            }
        }
        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn av(name: &str, state: SecurityProductState, sigs: Option<bool>) -> SecurityProductFact {
        SecurityProductFact {
            kind: SecurityProductKind::Antivirus,
            name: name.into(),
            guid: String::new(),
            state,
            signatures_up_to_date: sigs,
            is_microsoft_defender: name.contains("Defender"),
        }
    }

    #[test]
    fn antivirus_decisions() {
        assert_eq!(antivirus_findings(&[]), vec![AntivirusFinding::NoAntivirus]);
        assert!(antivirus_findings(&[av("Windows Defender", SecurityProductState::On, Some(true))]).is_empty());
        assert_eq!(
            antivirus_findings(&[av("Windows Defender", SecurityProductState::Off, Some(true))]),
            vec![AntivirusFinding::Off { name: "Windows Defender".into() }]
        );
        // Defender off while a third-party antivirus is on is expected.
        assert!(antivirus_findings(&[
            av("Windows Defender", SecurityProductState::Off, None),
            av("Vendor AV", SecurityProductState::On, Some(true)),
        ])
        .is_empty());
        assert_eq!(
            antivirus_findings(&[av("Vendor AV", SecurityProductState::On, Some(false))]),
            vec![AntivirusFinding::Outdated { name: "Vendor AV".into() }]
        );
        assert_eq!(
            antivirus_findings(&[av("Vendor AV", SecurityProductState::Expired, None)]),
            vec![AntivirusFinding::Expired { name: "Vendor AV".into() }]
        );
    }

    #[test]
    fn firewall_decisions() {
        let profiles = [
            FirewallProfileFact { profile: FirewallProfileKind::Domain, enabled: true, active: false },
            FirewallProfileFact { profile: FirewallProfileKind::Public, enabled: false, active: true },
        ];
        assert_eq!(firewall_findings(&profiles, &[]), vec![FirewallProfileKind::Public]);
        let mut vendor = av("Vendor Firewall", SecurityProductState::On, None);
        vendor.kind = SecurityProductKind::Firewall;
        assert!(firewall_findings(&profiles, &[vendor]).is_empty());
        assert_eq!(profile_from_id("firewall:public"), Some(FirewallProfileKind::Public));
        assert_eq!(profile_from_id("firewall:bogus"), None);
    }

    #[test]
    #[ignore = "reads live Security Center / firewall state (read-only)"]
    fn smoke_protection_scan() {
        let ctx = CareContext::detached(pctoolkit_platform::is_user_admin());
        let started = std::time::Instant::now();
        let scan = ProtectionCategory.scan(&ctx).unwrap();
        println!("protection: state={:?} reason={:?} elapsed={:?}", scan.state, scan.reason, started.elapsed());
        for item in &scan.items {
            println!("  {} fixable={} admin={}", item.id, item.fixable, item.needs_admin);
        }
    }
}
