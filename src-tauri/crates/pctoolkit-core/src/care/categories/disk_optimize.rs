//! Care category: Disk optimization, through Windows' own Optimize Drives.
//!
//! Per local fixed NTFS / ReFS volume: media type (Storage WMI, falling back to the device seek
//! penalty), and when Optimize Drives last ran (`Dfrg\Statistics`). A volume not optimized for
//! more than 30 days is offered: SSD → ReTrim (Safe, quick), HDD → defragment (Review, can take
//! long). Both run `defrag X: /O`, which lets Windows pick the right operation. Volumes with an
//! unknown media type or no recorded run are not judged (honest unknown).
//!
//! Also checked: TRIM turned off for NTFS while an SSD is present (fix re-enables it — the Windows
//! default), and the built-in `ScheduledDefrag` task being disabled (fix re-enables it). We never
//! disable scheduled optimization: on SSDs it is what sends periodic ReTrim. Fragmentation
//! analysis (`defrag /A`) is not run during the scan — it needs admin and is slow.
//!
//! Every fix needs administrator rights and reports `needsAdmin` otherwise (no silent elevation).

use std::collections::{BTreeMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use pctoolkit_platform::disk_health::{
    enable_ntfs_trim, enable_scheduled_optimization, last_optimization_unix, list_fixed_volumes, optimize_volume,
    scheduled_optimization_state, trim_state, volume_disk_facts, volume_incurs_seek_penalty, DiskToolStatus,
    FixedVolume, ScheduledOptimizationState, StorageMediaKind, VolumeDiskFacts,
};

use crate::care::{
    fix_all, skipped, CareCategory, CareCategoryId, CareCategoryScan, CareContext, CareFixResult, CareFixStatus,
    CareItem, CareRisk,
};
use crate::shared::CoreResult;

const ID: CareCategoryId = CareCategoryId::DiskOptimize;
const DUE_AFTER_DAYS: i64 = 30;
const OPTIMIZE_TIMEOUT: Duration = Duration::from_secs(4 * 60 * 60);
const TRIM_ID: &str = "trim";
const SCHEDULE_ID: &str = "schedule";

static OFFERED: Mutex<Option<HashSet<String>>> = Mutex::new(None);

pub struct DiskOptimizeCategory;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Media {
    Ssd,
    Hdd,
    Unknown,
}

/// Media from Storage WMI, else the seek penalty (`true` = rotational).
pub(crate) fn media_for(letter: char, facts: &[VolumeDiskFacts], seek_penalty: impl Fn(char) -> Option<bool>) -> Media {
    match facts.iter().find(|f| f.letter == letter).map(|f| f.media) {
        Some(StorageMediaKind::Ssd) => Media::Ssd,
        Some(StorageMediaKind::Hdd) => Media::Hdd,
        // Storage-class memory needs no optimization pass we would recommend.
        Some(StorageMediaKind::Scm) => Media::Unknown,
        Some(StorageMediaKind::Unspecified) | None => match seek_penalty(letter) {
            Some(true) => Media::Hdd,
            Some(false) => Media::Ssd,
            None => Media::Unknown,
        },
    }
}

/// Whole days since the last optimization, when known and not in the future.
pub(crate) fn days_since(last_unix: Option<i64>, now_unix: i64) -> Option<i64> {
    let last = last_unix?;
    (now_unix >= last).then(|| (now_unix - last) / 86_400)
}

fn optimizable_fs(volume: &FixedVolume) -> bool {
    matches!(volume.file_system.as_deref(), Some(fs) if fs.eq_ignore_ascii_case("NTFS") || fs.eq_ignore_ascii_case("ReFS"))
}

fn item(id: String, title_key: &str, params: BTreeMap<String, String>, risk: CareRisk, is_admin: bool) -> CareItem {
    CareItem {
        id,
        category: ID,
        title_key: Some(format!("care.diskOptimize.{title_key}")),
        title: None,
        params,
        detail_key: Some(format!("care.diskOptimize.{title_key}Detail")),
        detail: None,
        location: None,
        bytes: None,
        count: 1,
        risk,
        // Only Safe items are preselected, and admin-only items only when already elevated.
        selected: risk == CareRisk::Safe && is_admin,
        fixable: risk != CareRisk::Info,
        needs_admin: risk != CareRisk::Info,
        action: None,
    }
}

/// Pure: the optimize item for one volume, if it is due.
pub(crate) fn optimize_item(letter: char, media: Media, days: Option<i64>, is_admin: bool) -> Option<CareItem> {
    let days = days?;
    if days <= DUE_AFTER_DAYS {
        return None;
    }
    let (key, risk) = match media {
        Media::Ssd => ("retrimDue", CareRisk::Safe),
        Media::Hdd => ("defragDue", CareRisk::Review),
        Media::Unknown => return None,
    };
    let params = [("drive".to_string(), format!("{letter}:")), ("days".to_string(), days.to_string())].into();
    let mut it = item(format!("optimize:{letter}"), key, params, risk, is_admin);
    it.action = Some("optimizeDrive".to_string());
    Some(it)
}

pub(crate) fn schedule_item(state: ScheduledOptimizationState, is_admin: bool) -> Option<CareItem> {
    match state {
        ScheduledOptimizationState::Disabled => {
            let mut it = item(SCHEDULE_ID.into(), "scheduleDisabled", BTreeMap::new(), CareRisk::Review, is_admin);
            it.action = Some("enableSchedule".to_string());
            Some(it)
        }
        ScheduledOptimizationState::Missing => {
            Some(item("scheduleMissing".into(), "scheduleMissing", BTreeMap::new(), CareRisk::Info, is_admin))
        }
        ScheduledOptimizationState::Enabled | ScheduledOptimizationState::Unknown => None,
    }
}

fn letter_of(id: &str) -> Option<char> {
    let rest = id.strip_prefix("optimize:")?;
    let mut chars = rest.chars();
    let letter = chars.next().filter(char::is_ascii_uppercase)?;
    chars.next().is_none().then_some(letter)
}

fn tool_result(status: DiskToolStatus) -> (CareFixStatus, Option<&'static str>) {
    match status {
        DiskToolStatus::Ok => (CareFixStatus::Fixed, None),
        DiskToolStatus::Cancelled => (CareFixStatus::Skipped, Some("cancelled")),
        DiskToolStatus::Timeout => (CareFixStatus::Failed, Some("timeout")),
        DiskToolStatus::ToolUnavailable => (CareFixStatus::Failed, Some("toolUnavailable")),
        DiskToolStatus::InvalidOutput | DiskToolStatus::Failed => (CareFixStatus::Failed, Some("failed")),
    }
}

impl CareCategory for DiskOptimizeCategory {
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
        let volumes: Vec<FixedVolume> = list_fixed_volumes().into_iter().filter(optimizable_fs).collect();
        let facts = volume_disk_facts(&|| ctx.should_stop()).unwrap_or_default();
        if ctx.should_stop() {
            return Ok(skipped(ID));
        }
        let now = chrono::Utc::now().timestamp();
        let mut items = Vec::new();
        let mut any_ssd = false;
        for volume in &volumes {
            ctx.current(ID, format!("{}:", volume.letter));
            let media = media_for(volume.letter, &facts, volume_incurs_seek_penalty);
            any_ssd |= media == Media::Ssd && volume.file_system.as_deref().is_some_and(|f| f.eq_ignore_ascii_case("NTFS"));
            let last = volume.volume_guid.as_deref().and_then(last_optimization_unix);
            items.extend(optimize_item(volume.letter, media, days_since(last, now), ctx.is_admin));
        }
        if any_ssd && trim_state().ntfs_enabled == Some(false) {
            items.push(item(TRIM_ID.into(), "trimDisabled", BTreeMap::new(), CareRisk::Review, ctx.is_admin));
        }
        if ctx.should_stop() {
            return Ok(skipped(ID));
        }
        items.extend(schedule_item(scheduled_optimization_state(), ctx.is_admin));
        ctx.found(ID, items.len() as u64);
        let offered = items.iter().filter(|i| i.fixable).map(|i| i.id.clone()).collect();
        *OFFERED.lock().unwrap_or_else(|e| e.into_inner()) = Some(offered);
        Ok(CareCategoryScan::from_items(ID, items))
    }

    fn fix(&self, item_ids: &[String], ctx: &CareContext) -> CoreResult<Vec<CareFixResult>> {
        let offered = OFFERED.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let Some(offered) = offered else {
            return Ok(fix_all(ID, item_ids, CareFixStatus::Skipped, Some("unknownItem")));
        };
        let mut out = Vec::new();
        for id in item_ids {
            let (status, reason) = if ctx.should_stop() {
                (CareFixStatus::Skipped, Some("cancelled"))
            } else if !offered.contains(id) {
                (CareFixStatus::Skipped, Some("unknownItem"))
            } else if !ctx.is_admin {
                (CareFixStatus::Skipped, Some("needsAdmin"))
            } else if id == TRIM_ID {
                match tool_result(enable_ntfs_trim().status) {
                    // Verify instead of trusting the exit code.
                    (CareFixStatus::Fixed, _) if trim_state().ntfs_enabled != Some(true) => {
                        (CareFixStatus::Failed, Some("verifyFailed"))
                    }
                    other => other,
                }
            } else if id == SCHEDULE_ID {
                match tool_result(enable_scheduled_optimization().status) {
                    (CareFixStatus::Fixed, _) if scheduled_optimization_state() != ScheduledOptimizationState::Enabled => {
                        (CareFixStatus::Failed, Some("verifyFailed"))
                    }
                    other => other,
                }
            } else if let Some(letter) = letter_of(id) {
                let progress_ctx = ctx.clone();
                ctx.current(ID, format!("{letter}:"));
                let on_progress: Arc<dyn Fn(u8) + Send + Sync> =
                    Arc::new(move |pct| progress_ctx.current(ID, format!("{letter}: {pct}%")));
                // Stopping ends the defrag client; Optimize Drives stops between moves safely.
                tool_result(optimize_volume(letter, OPTIMIZE_TIMEOUT, &|| ctx.should_stop(), on_progress).status)
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
    use pctoolkit_platform::disk_health::StorageHealth;
    use std::time::Instant;

    fn facts(letter: char, media: StorageMediaKind) -> VolumeDiskFacts {
        VolumeDiskFacts {
            letter,
            disk_number: 0,
            media,
            bus_type: None,
            health: StorageHealth::Healthy,
            operational_status: Vec::new(),
            friendly_name: None,
        }
    }

    #[test]
    fn media_prefers_wmi_then_seek_penalty() {
        let f = [facts('C', StorageMediaKind::Ssd), facts('D', StorageMediaKind::Unspecified)];
        assert_eq!(media_for('C', &f, |_| Some(true)), Media::Ssd);
        assert_eq!(media_for('D', &f, |_| Some(true)), Media::Hdd);
        assert_eq!(media_for('E', &f, |_| Some(false)), Media::Ssd);
        assert_eq!(media_for('E', &f, |_| None), Media::Unknown);
    }

    #[test]
    fn optimize_due_only_when_known_and_old() {
        assert!(optimize_item('C', Media::Ssd, Some(5), true).is_none());
        assert!(optimize_item('C', Media::Ssd, None, true).is_none(), "unknown last run is not judged");
        assert!(optimize_item('C', Media::Unknown, Some(90), true).is_none());
        let ssd = optimize_item('C', Media::Ssd, Some(45), false).unwrap();
        assert_eq!(ssd.risk, CareRisk::Safe);
        assert!(ssd.needs_admin && ssd.fixable && !ssd.selected, "not preselected without admin");
        assert!(optimize_item('C', Media::Ssd, Some(45), true).unwrap().selected);
        let hdd = optimize_item('D', Media::Hdd, Some(45), true).unwrap();
        assert_eq!(hdd.risk, CareRisk::Review);
        assert!(!hdd.selected);
        assert_eq!(letter_of(&hdd.id), Some('D'));
    }

    #[test]
    fn schedule_is_only_ever_enabled() {
        let disabled = schedule_item(ScheduledOptimizationState::Disabled, true).unwrap();
        assert_eq!(disabled.action.as_deref(), Some("enableSchedule"));
        assert!(disabled.fixable && disabled.needs_admin);
        let missing = schedule_item(ScheduledOptimizationState::Missing, true).unwrap();
        assert!(!missing.fixable);
        assert!(schedule_item(ScheduledOptimizationState::Enabled, true).is_none());
        assert!(schedule_item(ScheduledOptimizationState::Unknown, true).is_none());
    }

    #[test]
    fn days_since_is_safe_for_clock_skew() {
        assert_eq!(days_since(Some(0), 86_400 * 31), Some(31));
        assert_eq!(days_since(Some(100), 50), None);
        assert_eq!(days_since(None, 50), None);
    }

    #[test]
    #[ignore = "reads this machine's disks (read-only)"]
    fn smoke_scan() {
        let started = Instant::now();
        let scan = DiskOptimizeCategory.scan(&CareContext::detached(false)).unwrap();
        eprintln!(
            "diskOptimize state={:?} items={:?} elapsed={:?}",
            scan.state,
            scan.items.iter().map(|i| &i.id).collect::<Vec<_>>(),
            started.elapsed()
        );
    }
}
