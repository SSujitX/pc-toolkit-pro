//! Memory Cleaner settings + optimize orchestration (no Tauri).

use pctoolkit_platform::{
    memory_stats, optimize_memory_areas, ordered_areas, AreaOutcome, AreaStatus, MemoryArea,
    MemoryOptimizeResult, MemoryStats,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};

use crate::history::{append_history, history_now_ms, HistoryOutcome, HistoryWrite};
use crate::shared::{CoreError, CoreResult};

static CANCELLED: AtomicBool = AtomicBool::new(false);
static BUSY: AtomicBool = AtomicBool::new(false);
/// Wall-clock ms of the last finished optimize (any reason). The interval schedule counts from it,
/// so a manual or tray clean postpones the next scheduled one, like a reset timer.
static LAST_OPTIMIZE_MS: AtomicI64 = AtomicI64::new(0);
/// Wall-clock ms of the last low-memory triggered optimize (cooldown anchor).
static LAST_LOW_MEMORY_MS: AtomicI64 = AtomicI64::new(0);

/// Minimum time between two low-memory triggered optimizations.
pub const LOW_MEMORY_COOLDOWN_MS: i64 = 5 * 60 * 1000;
/// How often the background scheduler wakes up to evaluate the auto-clean policy.
pub const AUTO_CLEAN_TICK_MS: u64 = 15_000;

pub const AUTO_INTERVAL_STEPS_MINUTES: [u32; 14] =
    [0, 5, 10, 15, 30, 45, 60, 90, 120, 180, 240, 360, 720, 1440];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OptimizeReason {
    Manual,
    Schedule,
    LowMemory,
    Tray,
}

impl OptimizeReason {
    fn as_str(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Schedule => "schedule",
            Self::LowMemory => "lowMemory",
            Self::Tray => "tray",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryCleanerSettings {
    pub areas: BTreeMap<String, bool>,
    /// 0 = off; otherwise minutes (min 5 when on).
    pub auto_interval_minutes: u32,
    /// 0 = off; trigger when free physical % is below this.
    pub auto_free_below_percent: u32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OptimizeMemoryRequest {
    pub areas: Option<Vec<MemoryArea>>,
    pub reason: OptimizeReason,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryProgress {
    pub phase: String,
    pub current: u64,
    pub total: u64,
    pub message: String,
    pub area: Option<MemoryArea>,
}

fn settings_path() -> Option<PathBuf> {
    let base = std::env::var_os("LOCALAPPDATA").or_else(|| std::env::var_os("APPDATA"))?;
    let mut path = PathBuf::from(base);
    path.push("PC Toolkit Pro");
    path.push("memory-cleaner.json");
    Some(path)
}

pub fn default_settings() -> MemoryCleanerSettings {
    let mut areas = BTreeMap::new();
    for area in MemoryArea::ALL {
        areas.insert(
            area.as_str().to_string(),
            area != MemoryArea::StandbyListLowPriority,
        );
    }
    MemoryCleanerSettings {
        areas,
        auto_interval_minutes: 0,
        auto_free_below_percent: 0,
    }
}

pub fn normalize_settings(mut settings: MemoryCleanerSettings) -> MemoryCleanerSettings {
    // Ensure every known area key exists.
    for area in MemoryArea::ALL {
        settings
            .areas
            .entry(area.as_str().to_string())
            .or_insert(area != MemoryArea::StandbyListLowPriority);
    }

    let standby = *settings
        .areas
        .get(MemoryArea::StandbyList.as_str())
        .unwrap_or(&true);
    let standby_low = *settings
        .areas
        .get(MemoryArea::StandbyListLowPriority.as_str())
        .unwrap_or(&false);
    if standby && standby_low {
        settings
            .areas
            .insert(MemoryArea::StandbyListLowPriority.as_str().into(), false);
    }

    if settings.auto_interval_minutes != 0 {
        if settings.auto_interval_minutes < 5 {
            settings.auto_interval_minutes = 5;
        }
        if settings.auto_interval_minutes > 1440 {
            settings.auto_interval_minutes = 1440;
        }
        // Snap to nearest allowed step.
        settings.auto_interval_minutes = snap_interval(settings.auto_interval_minutes);
    }

    if settings.auto_free_below_percent > 100 {
        settings.auto_free_below_percent = 100;
    }

    settings
}

fn snap_interval(minutes: u32) -> u32 {
    let mut best = AUTO_INTERVAL_STEPS_MINUTES[1];
    let mut best_dist = u32::MAX;
    for step in AUTO_INTERVAL_STEPS_MINUTES.iter().copied().skip(1) {
        let dist = minutes.abs_diff(step);
        if dist < best_dist {
            best_dist = dist;
            best = step;
        }
    }
    best
}

pub fn load_settings() -> MemoryCleanerSettings {
    let Some(path) = settings_path() else {
        return default_settings();
    };
    let Ok(raw) = fs::read_to_string(path) else {
        return default_settings();
    };
    match serde_json::from_str::<MemoryCleanerSettings>(&raw) {
        Ok(settings) => normalize_settings(settings),
        Err(_) => default_settings(),
    }
}

pub fn save_settings(settings: MemoryCleanerSettings) -> CoreResult<MemoryCleanerSettings> {
    let settings = normalize_settings(settings);
    let Some(path) = settings_path() else {
        return Err(CoreError::OperationFailed(
            "could not resolve settings path".into(),
        ));
    };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| CoreError::OperationFailed(e.to_string()))?;
    }
    let raw = serde_json::to_string_pretty(&settings)
        .map_err(|e| CoreError::OperationFailed(e.to_string()))?;
    fs::write(path, raw).map_err(|e| CoreError::OperationFailed(e.to_string()))?;
    Ok(settings)
}

pub fn selected_areas_from_settings(settings: &MemoryCleanerSettings) -> Vec<MemoryArea> {
    let mut selected = Vec::new();
    for area in MemoryArea::ALL {
        if *settings.areas.get(area.as_str()).unwrap_or(&false) {
            selected.push(area);
        }
    }
    // Mutual exclusion safety.
    let has_standby = selected.contains(&MemoryArea::StandbyList);
    let has_low = selected.contains(&MemoryArea::StandbyListLowPriority);
    if has_standby && has_low {
        selected.retain(|a| *a != MemoryArea::StandbyListLowPriority);
    }
    ordered_areas(&selected)
}

pub fn get_memory_stats() -> CoreResult<MemoryStats> {
    Ok(memory_stats()?)
}

pub fn cancel_memory_optimize() {
    CANCELLED.store(true, Ordering::SeqCst);
}

/// Holds the memory-optimize busy flag; dropping it releases the flag, so an early
/// return or a panic never leaves optimize permanently "busy".
struct BusyGuard(());

impl Drop for BusyGuard {
    fn drop(&mut self) {
        BUSY.store(false, Ordering::SeqCst);
    }
}

fn acquire_busy() -> CoreResult<BusyGuard> {
    if BUSY.swap(true, Ordering::SeqCst) {
        return Err(CoreError::OperationBusy);
    }
    CANCELLED.store(false, Ordering::SeqCst);
    Ok(BusyGuard(()))
}

pub fn optimize_memory(
    request: OptimizeMemoryRequest,
    mut on_progress: impl FnMut(MemoryProgress),
) -> CoreResult<MemoryOptimizeResult> {
    let _busy = acquire_busy()?;
    let started_at_ms = history_now_ms();
    let reason = request.reason;
    let areas = match request.areas {
        Some(list) if !list.is_empty() => ordered_areas(&list),
        _ => selected_areas_from_settings(&load_settings()),
    };

    if areas.is_empty() {
        return Err(CoreError::InvalidInput(
            "select at least one memory area".into(),
        ));
    }

    let result = (|| {
        on_progress(MemoryProgress {
            phase: "executing".into(),
            current: 0,
            total: areas.len() as u64,
            message: "Preparing".into(),
            area: None,
        });

        let mem = optimize_memory_areas(
            &areas,
            |current, total, area| {
                on_progress(MemoryProgress {
                    phase: "executing".into(),
                    current: current as u64,
                    total: total as u64,
                    message: area.as_str().into(),
                    area: Some(area),
                });
            },
            || CANCELLED.load(Ordering::SeqCst),
        )?;

        let cancelled = mem
            .areas
            .iter()
            .any(|a| a.detail.as_deref() == Some("cancelled"));
        if cancelled && mem.areas.iter().all(|a| a.status != AreaStatus::Ok) {
            return Err(CoreError::OperationCancelled);
        }

        let finished_at_ms = history_now_ms();
        let failed = mem
            .areas
            .iter()
            .filter(|a| a.status == AreaStatus::Failed)
            .count() as u32;
        let ok = mem
            .areas
            .iter()
            .filter(|a| a.status == AreaStatus::Ok)
            .count() as u32;
        let skipped = mem
            .areas
            .iter()
            .filter(|a| a.status == AreaStatus::Skipped)
            .count() as u32;

        let outcome = if cancelled {
            HistoryOutcome::Cancelled
        } else if failed > 0 || skipped > 0 {
            HistoryOutcome::CompletedWithWarnings
        } else {
            HistoryOutcome::Completed
        };

        let detail_lines = mem
            .areas
            .iter()
            .map(format_area_line)
            .collect::<Vec<_>>();

        append_history(HistoryWrite {
            category: "memoryCleaner".into(),
            title_key: "history.titles.memoryCleaner".into(),
            summary: format!(
                "{} · freed {} · {} ok / {} skipped / {} failed",
                reason.as_str(),
                crate::shared::format_size(mem.freed_bytes),
                ok,
                skipped,
                failed
            ),
            started_at_ms,
            finished_at_ms,
            outcome,
            planned_bytes: None,
            result_bytes: Some(mem.freed_bytes),
            selected_item_count: areas.len() as u32,
            affected_item_count: ok,
            failed_item_count: failed,
            detail_lines,
            action: format!("optimize_memory:{}", reason.as_str()),
            detail: Some(format!("freed={}", mem.freed_bytes)),
        });

        LAST_OPTIMIZE_MS.store(finished_at_ms, Ordering::SeqCst);
        if reason == OptimizeReason::LowMemory {
            LAST_LOW_MEMORY_MS.store(finished_at_ms, Ordering::SeqCst);
        }
        Ok(mem)
    })();

    if let Err(CoreError::OperationCancelled) = &result {
        append_history(HistoryWrite {
            category: "memoryCleaner".into(),
            title_key: "history.titles.memoryCleaner".into(),
            summary: "Cancelled by user".into(),
            started_at_ms,
            finished_at_ms: history_now_ms(),
            outcome: HistoryOutcome::Cancelled,
            planned_bytes: None,
            result_bytes: None,
            selected_item_count: areas.len() as u32,
            affected_item_count: 0,
            failed_item_count: 0,
            detail_lines: vec!["Memory optimization cancelled.".into()],
            action: format!("optimize_memory:{}", reason.as_str()),
            detail: Some("cancelled".into()),
        });
    }

    result
}

/// Pure auto-clean policy. Times are wall-clock ms (they keep counting across sleep/hibernate).
///
/// - Interval: fire when `interval_minutes > 0` and a full interval passed since `last_optimize_ms`
///   (the scheduler seeds that with its start time, so the first run waits one full interval).
/// - Low memory: fire when `free_below_percent > 0`, free physical memory is below it, and the
///   low-memory cooldown has elapsed.
pub fn auto_clean_decision(
    settings: &MemoryCleanerSettings,
    free_percent: f64,
    now_ms: i64,
    last_optimize_ms: i64,
    last_low_memory_ms: i64,
) -> Option<OptimizeReason> {
    if settings.auto_interval_minutes > 0 {
        let interval_ms = i64::from(settings.auto_interval_minutes) * 60_000;
        if now_ms.saturating_sub(last_optimize_ms) >= interval_ms {
            return Some(OptimizeReason::Schedule);
        }
    }
    if settings.auto_free_below_percent > 0
        && free_percent < f64::from(settings.auto_free_below_percent)
        && (last_low_memory_ms == 0
            || now_ms.saturating_sub(last_low_memory_ms) >= LOW_MEMORY_COOLDOWN_MS)
    {
        return Some(OptimizeReason::LowMemory);
    }
    None
}

/// Call once when the background scheduler starts: the first interval counts from now.
pub fn start_auto_clean_schedule() {
    let now = history_now_ms();
    let _ = LAST_OPTIMIZE_MS.compare_exchange(0, now, Ordering::SeqCst, Ordering::SeqCst);
}

/// One scheduler tick: reads the saved settings and free memory, runs the selected areas when the
/// policy says so. Returns `None` when nothing was due (or another optimize is running).
/// Runs without UI, so privileged areas are skipped honestly when the process is not elevated.
pub fn auto_clean_tick() -> Option<(OptimizeReason, CoreResult<MemoryOptimizeResult>)> {
    let settings = load_settings();
    if settings.auto_interval_minutes == 0 && settings.auto_free_below_percent == 0 {
        return None;
    }
    let free_percent = memory_stats()
        .ok()
        .filter(|s| s.physical_total > 0)
        .map(|s| s.physical_avail as f64 / s.physical_total as f64 * 100.0)
        .unwrap_or(100.0);
    let now = history_now_ms();
    let reason = auto_clean_decision(
        &settings,
        free_percent,
        now,
        LAST_OPTIMIZE_MS.load(Ordering::SeqCst),
        LAST_LOW_MEMORY_MS.load(Ordering::SeqCst),
    )?;
    let result = optimize_memory(
        OptimizeMemoryRequest {
            areas: None,
            reason,
        },
        |_| {},
    );
    match &result {
        // A manual / tray clean is running; it resets the schedule when it finishes.
        Err(CoreError::OperationBusy) => return None,
        // No areas selected or a hard failure: wait a full period instead of retrying every tick.
        Err(_) => {
            LAST_OPTIMIZE_MS.store(now, Ordering::SeqCst);
            if reason == OptimizeReason::LowMemory {
                LAST_LOW_MEMORY_MS.store(now, Ordering::SeqCst);
            }
        }
        Ok(_) => {}
    }
    Some((reason, result))
}

fn format_area_line(outcome: &AreaOutcome) -> String {
    let status = match outcome.status {
        AreaStatus::Ok => "ok",
        AreaStatus::Skipped => "skipped",
        AreaStatus::Failed => "failed",
    };
    match &outcome.detail {
        Some(detail) => format!("{}: {} ({})", outcome.id.as_str(), status, detail),
        None => format!("{}: {}", outcome.id.as_str(), status),
    }
}

/// Used by Cleaner / Deep Cleaner `freeMemory` category — same engine + saved areas.
pub fn optimize_from_cleaner_category(
    mut on_progress: impl FnMut(usize, usize, MemoryArea),
    should_cancel: impl Fn() -> bool,
) -> CoreResult<MemoryOptimizeResult> {
    let areas = selected_areas_from_settings(&load_settings());
    let areas = if areas.is_empty() {
        ordered_areas(
            &MemoryArea::ALL
                .iter()
                .copied()
                .filter(|a| *a != MemoryArea::StandbyListLowPriority)
                .collect::<Vec<_>>(),
        )
    } else {
        areas
    };
    Ok(optimize_memory_areas(
        &areas,
        |c, t, a| on_progress(c, t, a),
        should_cancel,
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn busy_guard_releases_on_drop_and_on_panic() {
        let guard = acquire_busy().expect("first acquire");
        assert!(matches!(acquire_busy(), Err(CoreError::OperationBusy)));
        drop(guard);
        let panicked = std::panic::catch_unwind(|| {
            let _busy = acquire_busy().expect("acquire after drop");
            panic!("simulated optimize panic");
        });
        assert!(panicked.is_err());
        let _again = acquire_busy().expect("flag released by unwinding");
    }

    #[test]
    fn normalize_clears_standby_mutex() {
        let mut settings = default_settings();
        settings
            .areas
            .insert("standbyList".into(), true);
        settings
            .areas
            .insert("standbyListLowPriority".into(), true);
        let normalized = normalize_settings(settings);
        assert_eq!(
            normalized.areas.get("standbyListLowPriority"),
            Some(&false)
        );
    }

    fn auto_settings(interval: u32, below: u32) -> MemoryCleanerSettings {
        MemoryCleanerSettings {
            auto_interval_minutes: interval,
            auto_free_below_percent: below,
            ..default_settings()
        }
    }

    #[test]
    fn auto_clean_interval_waits_a_full_period() {
        let s = auto_settings(5, 0);
        let start = 1_000_000;
        assert_eq!(auto_clean_decision(&s, 50.0, start + 4 * 60_000, start, 0), None);
        assert_eq!(
            auto_clean_decision(&s, 50.0, start + 5 * 60_000, start, 0),
            Some(OptimizeReason::Schedule)
        );
        // Off means never.
        assert_eq!(auto_clean_decision(&auto_settings(0, 0), 1.0, i64::MAX / 2, 0, 0), None);
    }

    #[test]
    fn auto_clean_low_memory_respects_threshold_and_cooldown() {
        let s = auto_settings(0, 20);
        let now = 10_000_000;
        assert_eq!(auto_clean_decision(&s, 25.0, now, now, 0), None);
        assert_eq!(auto_clean_decision(&s, 15.0, now, now, 0), Some(OptimizeReason::LowMemory));
        assert_eq!(auto_clean_decision(&s, 15.0, now, now, now - 60_000), None);
        assert_eq!(
            auto_clean_decision(&s, 15.0, now, now, now - LOW_MEMORY_COOLDOWN_MS),
            Some(OptimizeReason::LowMemory)
        );
    }

    #[test]
    fn normalize_clamps_interval() {
        let mut settings = default_settings();
        settings.auto_interval_minutes = 7;
        let normalized = normalize_settings(settings);
        assert_eq!(normalized.auto_interval_minutes, 5);
    }
}
