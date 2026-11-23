//! Care category: Disk health (read-only scan).
//!
//! Per local fixed volume:
//! - the NTFS/ReFS dirty bit — reading it needs administrator rights; when Windows denies the
//!   query the volume is simply not judged and the scan reports `reason = needsAdmin` (partial
//!   check), never a guessed error;
//! - physical disk health from Storage WMI (`Get-Disk` / `Get-PhysicalDisk` HealthStatus, which
//!   includes SMART predictive failure) — Warning / Unhealthy is informational (back up data);
//! - low free space (< 10 % or < 10 GiB on volumes of 50 GiB and more) — informational.
//!
//! The only fix is for a dirty NTFS volume (Review, admin): an online `chkdsk X: /scan`; when it
//! asks for an offline repair, or the volume stays dirty, a check at restart is scheduled with
//! `chkntfs /c X:` and the result says a restart is required.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::sync::Mutex;
use std::time::Duration;

use pctoolkit_platform::disk_health::{
    list_fixed_volumes, online_scan_volume, schedule_boot_check, volume_dirty_state, volume_disk_facts,
    DiskToolStatus, FixedVolume, OnlineScanResult, StorageHealth, VolumeDirtyState, VolumeDiskFacts,
};

use crate::care::{
    fix_all, skipped, CareCategory, CareCategoryId, CareCategoryScan, CareContext, CareFixResult, CareFixStatus,
    CareItem, CareRisk,
};
use crate::shared::CoreResult;

const ID: CareCategoryId = CareCategoryId::DiskHealth;
const GIB: u64 = 1024 * 1024 * 1024;
const SCAN_TIMEOUT: Duration = Duration::from_secs(2 * 60 * 60);

/// Fixable item ids offered by the latest scan.
static OFFERED: Mutex<Option<HashSet<String>>> = Mutex::new(None);

pub struct DiskHealthCategory;

/// Low free space: under 10 % of the volume, or under 10 GiB on volumes of at least 50 GiB
/// (small system / recovery volumes would otherwise always be flagged).
pub(crate) fn is_low_space(total: u64, free: u64) -> bool {
    if total == 0 {
        return false;
    }
    free.saturating_mul(10) < total || (total >= 50 * GIB && free < 10 * GIB)
}

fn checks_dirty_bit(volume: &FixedVolume) -> bool {
    matches!(volume.file_system.as_deref(), Some(fs) if fs.eq_ignore_ascii_case("NTFS") || fs.eq_ignore_ascii_case("ReFS"))
}

fn is_ntfs(volume: &FixedVolume) -> bool {
    volume.file_system.as_deref().is_some_and(|fs| fs.eq_ignore_ascii_case("NTFS"))
}

fn base_item(id: String, title_key: &str, params: BTreeMap<String, String>) -> CareItem {
    CareItem {
        id,
        category: ID,
        title_key: Some(format!("care.diskHealth.{title_key}")),
        title: None,
        params,
        detail_key: Some(format!("care.diskHealth.{title_key}Detail")),
        detail: None,
        location: None,
        bytes: None,
        count: 1,
        risk: CareRisk::Info,
        selected: false,
        fixable: false,
        needs_admin: false,
        action: None,
    }
}

fn params(pairs: &[(&str, String)]) -> BTreeMap<String, String> {
    pairs.iter().map(|(k, v)| (k.to_string(), v.clone())).collect()
}

/// Pure: one Info item per physical disk whose health is Warning / Unhealthy.
pub(crate) fn physical_disk_items(facts: &[VolumeDiskFacts]) -> Vec<CareItem> {
    let mut disks: BTreeSet<u32> = BTreeSet::new();
    let mut out = Vec::new();
    for f in facts {
        if !matches!(f.health, StorageHealth::Warning | StorageHealth::Unhealthy) || !disks.insert(f.disk_number) {
            continue;
        }
        let drives: Vec<String> =
            facts.iter().filter(|o| o.disk_number == f.disk_number).map(|o| format!("{}:", o.letter)).collect();
        let key = if f.health == StorageHealth::Unhealthy { "diskUnhealthy" } else { "diskWarning" };
        out.push(base_item(
            format!("disk:{}", f.disk_number),
            key,
            params(&[
                ("disk", f.friendly_name.clone().unwrap_or_else(|| format!("Disk {}", f.disk_number))),
                ("drives", drives.join(", ")),
                ("status", f.operational_status.join(", ")),
            ]),
        ));
    }
    out
}

pub(crate) fn dirty_item(volume: &FixedVolume) -> CareItem {
    let mut item = base_item(
        format!("dirty:{}", volume.letter),
        "volumeDirty",
        params(&[("drive", format!("{}:", volume.letter))]),
    );
    let ntfs = is_ntfs(volume);
    item.risk = if ntfs { CareRisk::Review } else { CareRisk::Info };
    item.fixable = ntfs;
    item.needs_admin = true;
    item.action = ntfs.then(|| "onlineScan".to_string());
    item
}

pub(crate) fn low_space_item(volume: &FixedVolume) -> CareItem {
    let percent = if volume.total_bytes == 0 { 0 } else { volume.free_bytes.saturating_mul(100) / volume.total_bytes };
    let mut item = base_item(
        format!("space:{}", volume.letter),
        "lowSpace",
        params(&[
            ("drive", format!("{}:", volume.letter)),
            ("freeBytes", volume.free_bytes.to_string()),
            ("totalBytes", volume.total_bytes.to_string()),
            ("percent", percent.to_string()),
        ]),
    );
    item.action = Some("openStorageSettings".to_string());
    item
}

fn letter_of(id: &str) -> Option<char> {
    let rest = id.strip_prefix("dirty:")?;
    let mut chars = rest.chars();
    let letter = chars.next().filter(char::is_ascii_uppercase)?;
    chars.next().is_none().then_some(letter)
}

fn fix_dirty(letter: char, ctx: &CareContext) -> (CareFixStatus, Option<&'static str>) {
    if volume_dirty_state(letter) == VolumeDirtyState::Clean {
        return (CareFixStatus::Skipped, Some("itemChanged"));
    }
    ctx.current(ID, format!("{letter}:"));
    let scan = online_scan_volume(letter, SCAN_TIMEOUT, &|| ctx.should_stop());
    match scan.tool.status {
        DiskToolStatus::Ok => {}
        DiskToolStatus::Cancelled => return (CareFixStatus::Skipped, Some("cancelled")),
        DiskToolStatus::Timeout => return (CareFixStatus::Failed, Some("timeout")),
        DiskToolStatus::ToolUnavailable => return (CareFixStatus::Failed, Some("toolUnavailable")),
        DiskToolStatus::InvalidOutput | DiskToolStatus::Failed => return (CareFixStatus::Failed, Some("failed")),
    }
    let needs_offline = scan.result == OnlineScanResult::OfflineRepairNeeded
        || volume_dirty_state(letter) == VolumeDirtyState::Dirty;
    if !needs_offline {
        return (CareFixStatus::Fixed, None);
    }
    if schedule_boot_check(letter).status == DiskToolStatus::Ok {
        (CareFixStatus::Partial, Some("restartRequired"))
    } else {
        (CareFixStatus::Failed, Some("failed"))
    }
}

impl CareCategory for DiskHealthCategory {
    fn id(&self) -> CareCategoryId {
        ID
    }

    fn default_weight_ms(&self) -> u64 {
        3_000
    }

    fn scan(&self, ctx: &CareContext) -> CoreResult<CareCategoryScan> {
        if ctx.should_stop() {
            return Ok(skipped(ID));
        }
        let volumes = list_fixed_volumes();
        let mut items = Vec::new();
        let mut denied = false;
        for volume in &volumes {
            if ctx.should_stop() {
                return Ok(skipped(ID));
            }
            ctx.current(ID, format!("{}:", volume.letter));
            if checks_dirty_bit(volume) {
                match volume_dirty_state(volume.letter) {
                    VolumeDirtyState::Dirty => items.push(dirty_item(volume)),
                    VolumeDirtyState::AccessDenied => denied = true,
                    VolumeDirtyState::Clean | VolumeDirtyState::Unknown => {}
                }
            }
            if is_low_space(volume.total_bytes, volume.free_bytes) {
                items.push(low_space_item(volume));
            }
        }
        // Storage WMI is optional: without it physical health is simply not judged.
        if let Ok(facts) = volume_disk_facts(&|| ctx.should_stop()) {
            items.extend(physical_disk_items(&facts));
        }
        if ctx.should_stop() {
            return Ok(skipped(ID));
        }
        ctx.found(ID, items.len() as u64);
        let offered = items.iter().filter(|i| i.fixable).map(|i| i.id.clone()).collect();
        *OFFERED.lock().unwrap_or_else(|e| e.into_inner()) = Some(offered);
        let mut scan = CareCategoryScan::from_items(ID, items);
        if denied && !ctx.is_admin {
            // Partial check: the dirty bit could not be read without elevation.
            scan.reason = Some("needsAdmin".to_string());
        }
        Ok(scan)
    }

    fn fix(&self, item_ids: &[String], ctx: &CareContext) -> CoreResult<Vec<CareFixResult>> {
        let offered = OFFERED.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let Some(offered) = offered else {
            return Ok(fix_all(ID, item_ids, CareFixStatus::Skipped, Some("unknownItem")));
        };
        let mut out = Vec::new();
        for id in item_ids {
            let (status, reason) = match letter_of(id) {
                _ if ctx.should_stop() => (CareFixStatus::Skipped, Some("cancelled")),
                Some(letter) if offered.contains(id) => {
                    if ctx.is_admin {
                        fix_dirty(letter, ctx)
                    } else {
                        (CareFixStatus::Skipped, Some("needsAdmin"))
                    }
                }
                _ => (CareFixStatus::Skipped, Some("unknownItem")),
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
    use pctoolkit_platform::disk_health::StorageMediaKind;
    use std::time::Instant;

    fn volume(letter: char, fs: &str, total: u64, free: u64) -> FixedVolume {
        FixedVolume { letter, file_system: Some(fs.into()), total_bytes: total, free_bytes: free, volume_guid: None }
    }

    fn facts(letter: char, disk: u32, health: StorageHealth) -> VolumeDiskFacts {
        VolumeDiskFacts {
            letter,
            disk_number: disk,
            media: StorageMediaKind::Ssd,
            bus_type: None,
            health,
            operational_status: vec!["OK".into()],
            friendly_name: None,
        }
    }

    #[test]
    fn low_space_thresholds() {
        assert!(is_low_space(100 * GIB, 9 * GIB));
        assert!(is_low_space(500 * GIB, 9 * GIB)); // < 10 GiB on a large volume
        assert!(!is_low_space(500 * GIB, 60 * GIB));
        assert!(!is_low_space(20 * GIB, 5 * GIB)); // small volume, 25 % free
        assert!(is_low_space(20 * GIB, GIB)); // small volume, 5 % free
        assert!(!is_low_space(0, 0));
    }

    #[test]
    fn unhealthy_disks_are_informational_and_grouped() {
        let items = physical_disk_items(&[
            facts('C', 0, StorageHealth::Healthy),
            facts('D', 1, StorageHealth::Warning),
            facts('E', 1, StorageHealth::Warning),
            facts('F', 2, StorageHealth::Unknown),
        ]);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, "disk:1");
        assert_eq!(items[0].params.get("drives").map(String::as_str), Some("D:, E:"));
        assert!(!items[0].fixable);
        assert_eq!(items[0].risk, CareRisk::Info);
    }

    #[test]
    fn dirty_ntfs_is_review_admin_fix_and_refs_is_info() {
        let ntfs = dirty_item(&volume('C', "NTFS", 100, 50));
        assert!(ntfs.fixable && ntfs.needs_admin && !ntfs.selected);
        assert_eq!(ntfs.risk, CareRisk::Review);
        let refs = dirty_item(&volume('D', "ReFS", 100, 50));
        assert!(!refs.fixable);
        assert!(!checks_dirty_bit(&volume('E', "FAT32", 100, 50)));
        assert_eq!(letter_of("dirty:C"), Some('C'));
        assert_eq!(letter_of("dirty:CC"), None);
        assert_eq!(letter_of("space:C"), None);
    }

    #[test]
    #[ignore = "reads this machine's disks (read-only)"]
    fn smoke_scan() {
        let started = Instant::now();
        let scan = DiskHealthCategory.scan(&CareContext::detached(false)).unwrap();
        eprintln!(
            "diskHealth state={:?} reason={:?} items={:?} elapsed={:?}",
            scan.state,
            scan.reason,
            scan.items.iter().map(|i| &i.id).collect::<Vec<_>>(),
            started.elapsed()
        );
    }
}
