//! Care scan / fix orchestration.
//!
//! - Scans run the selected categories in parallel on a small pool (min(cpus, 4) threads).
//!   Execution order is longest-expected-first so one slow category does not finish last on an
//!   otherwise idle pool; display order is always [`CareCategoryId::ALL`].
//! - Overall percent is weighted by each category's expected duration, learned per category as an
//!   exponential moving average persisted in `%LOCALAPPDATA%\PC Toolkit Pro\care-timings.json`
//!   (unreadable / missing → `default_weight_ms`). Running categories interpolate by elapsed /
//!   expected, capped at 95 %, so the bar never stalls on one slow category.
//! - Progress events are full snapshots, throttled to ~50 ms; state transitions emit at once.
//! - The last scan stays in Core. Fix requests carry category + item ids only; ids that are not in
//!   the last scan are skipped (`unknownItem`). Auto fix = `Safe && fixable && (!needs_admin || admin)`.
//! - History gets one record per scan and per fix with counts only (no paths, no item names).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use super::categories;
use super::{
    CareCategory, CareCategoryId, CareCategoryScan, CareCategoryState, CareContext, CareEvent, CareFixResult,
    CareFixStatus, CareItem, CareRisk,
};
use crate::history::{append_history, history_now_ms, HistoryOutcome, HistoryWrite};
use crate::power::{execute_power, PowerAction};
use crate::shared::{CoreError, CoreResult};

/// Minimum spacing between non-forced progress events.
pub const EVENT_INTERVAL: Duration = Duration::from_millis(50);
/// Running categories never report more than this fraction of their weight.
const RUNNING_CAP: f64 = 0.95;
/// EMA smoothing for learned timings (weight of the newest sample).
const TIMING_ALPHA: f64 = 0.3;
const MAX_WORKERS: usize = 4;
const TIMINGS_VERSION: u32 = 1;

// ---------------------------------------------------------------------------------------------
// Protocol
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CarePhase {
    Scanning,
    Fixing,
    Complete,
}

/// Per-category lifecycle inside one operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CareRunStatus {
    Queued,
    Running,
    Done,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CareCategoryProgress {
    pub category: CareCategoryId,
    pub status: CareRunStatus,
    /// Final scan state once `status == done` (fix phase: Clean / Issues summarizing the fix).
    pub state: Option<CareCategoryState>,
    /// Issues found so far (scan) or items fixed so far (fix).
    pub count: u64,
    pub elapsed_ms: u64,
}

/// Full progress snapshot (cheap: at most 11 rows).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CareProgress {
    pub phase: CarePhase,
    /// Weighted overall percent, 0..=100.
    pub percent: f64,
    /// Selected categories in display order.
    pub categories: Vec<CareCategoryProgress>,
    /// Category whose `current_text` is shown.
    pub current_category: Option<CareCategoryId>,
    /// Display-only path / item text. Never logged or persisted.
    pub current_text: String,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CareScanReport {
    pub scan_id: String,
    /// Selected categories in display order.
    pub categories: Vec<CareCategoryScan>,
    pub is_admin: bool,
    /// The user pressed Stop; unfinished categories are `skipped`.
    pub stopped: bool,
    pub started_at_ms: i64,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CareFixSelection {
    pub category: CareCategoryId,
    pub item_ids: Vec<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CareAfterAction {
    #[default]
    None,
    Shutdown,
    Restart,
    Sleep,
    Hibernate,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CareFixRequest {
    /// Explicit picks (ignored when `auto`).
    #[serde(default)]
    pub items: Vec<CareFixSelection>,
    /// Fix every auto-fixable item of the last scan.
    #[serde(default)]
    pub auto: bool,
    #[serde(default)]
    pub after_action: CareAfterAction,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CareFixReport {
    pub results: Vec<CareFixResult>,
    pub fixed: u64,
    pub partial: u64,
    pub failed: u64,
    pub skipped: u64,
    pub freed_bytes: u64,
    pub undoable: u64,
    pub stopped: bool,
    pub elapsed_ms: u64,
    pub after_action: CareAfterAction,
    /// Typed failure of the after-action (`failed`), if it was attempted and failed.
    pub after_action_error: Option<String>,
    /// Seconds until a scheduled shutdown / restart (cancellable via `cancel_care_after_action`).
    pub after_action_delay_secs: Option<u64>,
}

/// Grace period before an Auto fix shutdown / restart so the user can save work or cancel.
pub const CARE_AFTER_ACTION_DELAY_SECS: u64 = 60;

/// Cancel a pending Auto fix shutdown / restart.
pub fn cancel_care_after_action() -> CoreResult<()> {
    pctoolkit_platform::cancel_scheduled_shutdown().map_err(Into::into)
}

// ---------------------------------------------------------------------------------------------
// Global operation state
// ---------------------------------------------------------------------------------------------

static BUSY: AtomicBool = AtomicBool::new(false);
static STOP: OnceLock<Arc<AtomicBool>> = OnceLock::new();
static SKIPS: Mutex<Option<HashMap<CareCategoryId, Arc<AtomicBool>>>> = Mutex::new(None);
static LAST_SCAN: Mutex<Option<CareScanReport>> = Mutex::new(None);
static REGISTRY: OnceLock<Vec<Box<dyn CareCategory>>> = OnceLock::new();

fn stop_flag() -> Arc<AtomicBool> {
    STOP.get_or_init(|| Arc::new(AtomicBool::new(false))).clone()
}

fn registry() -> &'static [Box<dyn CareCategory>] {
    REGISTRY.get_or_init(categories::all)
}

struct BusyGuard;

impl BusyGuard {
    fn acquire() -> CoreResult<Self> {
        if BUSY.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).is_err() {
            return Err(CoreError::OperationBusy);
        }
        stop_flag().store(false, Ordering::SeqCst);
        Ok(Self)
    }
}

impl Drop for BusyGuard {
    fn drop(&mut self) {
        *SKIPS.lock().unwrap_or_else(|e| e.into_inner()) = None;
        BUSY.store(false, Ordering::SeqCst);
    }
}

fn install_skips(flags: &HashMap<CareCategoryId, Arc<AtomicBool>>) {
    *SKIPS.lock().unwrap_or_else(|e| e.into_inner()) = Some(flags.clone());
}

/// Stop the running scan or fix. Categories not reached are reported `skipped`.
pub fn stop_care() {
    stop_flag().store(true, Ordering::SeqCst);
}

/// Skip one category of the running scan / fix (queued or running). No-op when idle.
pub fn skip_care_category(category: CareCategoryId) {
    if let Some(map) = SKIPS.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        if let Some(flag) = map.get(&category) {
            flag.store(true, Ordering::SeqCst);
        }
    }
}

/// The last completed scan held in Core (for re-opening the results view).
pub fn last_care_scan() -> Option<CareScanReport> {
    LAST_SCAN.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

// ---------------------------------------------------------------------------------------------
// Pure helpers (unit tested)
// ---------------------------------------------------------------------------------------------

/// Overall percent from per-category weights and fractions (0..=1, already capped for running).
pub fn weighted_percent(parts: &[(f64, f64)]) -> f64 {
    let total: f64 = parts.iter().map(|(w, _)| w.max(1.0)).sum();
    if total <= 0.0 {
        return 0.0;
    }
    let done: f64 = parts.iter().map(|(w, f)| w.max(1.0) * f.clamp(0.0, 1.0)).sum();
    (done / total * 100.0).clamp(0.0, 100.0)
}

/// Fraction of a running category: elapsed / expected, capped at [`RUNNING_CAP`].
pub fn running_fraction(elapsed_ms: u64, expected_ms: f64) -> f64 {
    if expected_ms <= 0.0 {
        return RUNNING_CAP;
    }
    (elapsed_ms as f64 / expected_ms).min(RUNNING_CAP)
}

/// New moving average from the previous value (if any) and a fresh sample.
pub fn ema(previous: Option<f64>, sample_ms: f64) -> f64 {
    match previous {
        Some(prev) if prev.is_finite() && prev > 0.0 => prev + TIMING_ALPHA * (sample_ms - prev),
        _ => sample_ms,
    }
}

/// Event throttle: non-forced events pass at most once per interval.
#[derive(Debug)]
pub struct Throttle {
    interval: Duration,
    last: Option<Instant>,
}

impl Throttle {
    pub fn new(interval: Duration) -> Self {
        Self { interval, last: None }
    }

    pub fn ready(&mut self, now: Instant, force: bool) -> bool {
        let due = force || self.last.is_none_or(|t| now.saturating_duration_since(t) >= self.interval);
        if due {
            self.last = Some(now);
        }
        due
    }
}

/// Items Auto fix may touch.
pub fn auto_fixable(item: &CareItem, is_admin: bool) -> bool {
    item.risk == CareRisk::Safe && item.fixable && (!item.needs_admin || is_admin)
}

/// Badge count of one category: underlying entries of fixable items, or item count for info rows.
pub fn issue_count(scan: &CareCategoryScan) -> u64 {
    let fixable: u64 = scan.items.iter().filter(|i| i.fixable).map(|i| i.count.max(1)).sum();
    if fixable > 0 {
        fixable
    } else {
        scan.items.len() as u64
    }
}

/// Planned fix work: ids per category (display order) plus results for ids rejected up front.
#[derive(Debug, Default)]
pub struct FixPlan {
    pub work: Vec<(CareCategoryId, Vec<String>)>,
    pub rejected: Vec<CareFixResult>,
}

fn rejected(category: CareCategoryId, id: &str, reason: &str) -> CareFixResult {
    CareFixResult {
        item_id: id.to_string(),
        category,
        status: CareFixStatus::Skipped,
        reason: Some(reason.to_string()),
        freed_bytes: 0,
        undoable: false,
    }
}

/// Route a fix request against the last scan. Never trusts anything but ids.
pub fn plan_fix(scan: &CareScanReport, request: &CareFixRequest, is_admin: bool) -> FixPlan {
    let mut plan = FixPlan::default();
    let by_category: HashMap<CareCategoryId, &CareCategoryScan> =
        scan.categories.iter().map(|c| (c.category, c)).collect();
    let mut wanted: BTreeMap<usize, (CareCategoryId, Vec<String>)> = BTreeMap::new();
    let order = |id: CareCategoryId| CareCategoryId::ALL.iter().position(|c| *c == id).unwrap_or(usize::MAX);

    if request.auto {
        for cat in &scan.categories {
            let ids: Vec<String> = cat
                .items
                .iter()
                .filter(|i| auto_fixable(i, is_admin))
                .map(|i| i.id.clone())
                .collect();
            if !ids.is_empty() {
                wanted.insert(order(cat.category), (cat.category, ids));
            }
        }
    } else {
        for selection in &request.items {
            let Some(cat) = by_category.get(&selection.category) else {
                plan.rejected
                    .extend(selection.item_ids.iter().map(|id| rejected(selection.category, id, "unknownItem")));
                continue;
            };
            let items: HashMap<&str, &CareItem> = cat.items.iter().map(|i| (i.id.as_str(), i)).collect();
            let mut seen = HashSet::new();
            for id in &selection.item_ids {
                if !seen.insert(id.as_str()) {
                    continue;
                }
                match items.get(id.as_str()) {
                    None => plan.rejected.push(rejected(selection.category, id, "unknownItem")),
                    Some(item) if !item.fixable => plan.rejected.push(rejected(selection.category, id, "notFixable")),
                    Some(item) if item.needs_admin && !is_admin => {
                        plan.rejected.push(rejected(selection.category, id, "needsAdmin"))
                    }
                    Some(_) => wanted
                        .entry(order(selection.category))
                        .or_insert_with(|| (selection.category, Vec::new()))
                        .1
                        .push(id.clone()),
                }
            }
        }
    }
    plan.work = wanted.into_values().collect();
    plan
}

/// Make sure every requested id has exactly one result (category bugs must not lose items).
fn normalize_results(category: CareCategoryId, ids: &[String], results: Vec<CareFixResult>) -> Vec<CareFixResult> {
    let mut by_id: HashMap<String, CareFixResult> = HashMap::new();
    for mut r in results {
        if ids.contains(&r.item_id) {
            r.category = category;
            by_id.entry(r.item_id.clone()).or_insert(r);
        }
    }
    ids.iter()
        .map(|id| by_id.remove(id).unwrap_or_else(|| rejected(category, id, "noResult")))
        .collect()
}

// ---------------------------------------------------------------------------------------------
// Timings persistence
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TimingsFile {
    version: u32,
    /// Category id → expected scan duration (ms).
    categories: BTreeMap<String, f64>,
}

fn timings_path() -> Option<PathBuf> {
    let base = std::env::var_os("LOCALAPPDATA").or_else(|| std::env::var_os("APPDATA"))?;
    Some(PathBuf::from(base).join("PC Toolkit Pro").join("care-timings.json"))
}

/// Read policy: missing / unreadable / other version → empty (defaults apply).
fn load_timings() -> BTreeMap<String, f64> {
    timings_path()
        .and_then(|p| fs::read_to_string(p).ok())
        .and_then(|raw| serde_json::from_str::<TimingsFile>(&raw).ok())
        .filter(|t| t.version == TIMINGS_VERSION)
        .map(|t| t.categories.into_iter().filter(|(_, v)| v.is_finite() && *v > 0.0).collect())
        .unwrap_or_default()
}

fn save_timings(categories: BTreeMap<String, f64>) {
    let Some(path) = timings_path() else { return };
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let file = TimingsFile { version: TIMINGS_VERSION, categories };
    if let Ok(raw) = serde_json::to_string_pretty(&file) {
        let _ = fs::write(path, raw);
    }
}

// ---------------------------------------------------------------------------------------------
// Scan
// ---------------------------------------------------------------------------------------------

enum Msg {
    Started(usize),
    Event(CareEvent),
    Done(usize, CareCategoryScan),
}

struct Row {
    category: CareCategoryId,
    weight: f64,
    status: CareRunStatus,
    state: Option<CareCategoryState>,
    count: u64,
    started: Option<Instant>,
    elapsed_ms: u64,
}

fn snapshot(phase: CarePhase, rows: &[Row], current: &Option<(CareCategoryId, String)>, started: Instant) -> CareProgress {
    let now = Instant::now();
    let parts: Vec<(f64, f64)> = rows
        .iter()
        .map(|r| {
            let fraction = match r.status {
                CareRunStatus::Queued => 0.0,
                CareRunStatus::Done => 1.0,
                CareRunStatus::Running => {
                    let elapsed = r.started.map(|s| now.duration_since(s).as_millis() as u64).unwrap_or(0);
                    running_fraction(elapsed, r.weight)
                }
            };
            (r.weight, fraction)
        })
        .collect();
    CareProgress {
        phase,
        percent: if phase == CarePhase::Complete { 100.0 } else { weighted_percent(&parts) },
        categories: rows
            .iter()
            .map(|r| CareCategoryProgress {
                category: r.category,
                status: r.status,
                state: r.state,
                count: r.count,
                elapsed_ms: match (r.status, r.started) {
                    (CareRunStatus::Running, Some(s)) => now.duration_since(s).as_millis() as u64,
                    _ => r.elapsed_ms,
                },
            })
            .collect(),
        current_category: current.as_ref().map(|(c, _)| *c),
        current_text: current.as_ref().map(|(_, t)| t.clone()).unwrap_or_default(),
        elapsed_ms: started.elapsed().as_millis() as u64,
    }
}

/// Run one category's scan, converting panics and errors into honest states.
fn scan_one(category: &dyn CareCategory, ctx: &CareContext) -> CareCategoryScan {
    let id = category.id();
    let started = Instant::now();
    let outcome = catch_unwind(AssertUnwindSafe(|| category.scan(ctx)));
    let mut scan = match outcome {
        Ok(Ok(scan)) => scan,
        Ok(Err(CoreError::OperationCancelled)) => super::skipped(id),
        Ok(Err(CoreError::OperationBusy)) => {
            CareCategoryScan::with_state(id, CareCategoryState::Skipped, Some("busy"))
        }
        Ok(Err(CoreError::PermissionDenied)) => {
            CareCategoryScan::with_state(id, CareCategoryState::Failed, Some("needsAdmin"))
        }
        Ok(Err(_)) => CareCategoryScan::with_state(id, CareCategoryState::Failed, Some("failed")),
        Err(_) => CareCategoryScan::with_state(id, CareCategoryState::Failed, Some("panicked")),
    };
    // A category that was skipped / stopped mid-way may return partial items; never present
    // partial results as a finished scan.
    if ctx.should_stop() && scan.state != CareCategoryState::Skipped {
        scan = super::skipped(id);
    }
    scan.category = id;
    scan.elapsed_ms = started.elapsed().as_millis() as u64;
    scan
}

/// Engine core with injectable categories (tests use fakes). Returns the report; the caller
/// owns busy state, history, and persistence.
pub(crate) fn run_scan<E>(
    categories: &[&dyn CareCategory],
    weights: &HashMap<CareCategoryId, f64>,
    is_admin: bool,
    stop: Arc<AtomicBool>,
    skips: &HashMap<CareCategoryId, Arc<AtomicBool>>,
    workers: usize,
    on_event: E,
) -> CareScanReport
where
    E: Fn(CareProgress) + Send + Sync,
{
    let started = Instant::now();
    let started_at_ms = history_now_ms();
    let mut rows: Vec<Row> = categories
        .iter()
        .map(|c| Row {
            category: c.id(),
            weight: weights
                .get(&c.id())
                .copied()
                .unwrap_or_else(|| c.default_weight_ms() as f64)
                .max(1.0),
            status: CareRunStatus::Queued,
            state: None,
            count: 0,
            started: None,
            elapsed_ms: 0,
        })
        .collect();
    // Longest expected first; display order is untouched.
    let mut order: Vec<usize> = (0..categories.len()).collect();
    order.sort_by(|a, b| rows[*b].weight.total_cmp(&rows[*a].weight).then(a.cmp(b)));

    let mut results: Vec<Option<CareCategoryScan>> = vec![None; categories.len()];
    let next = AtomicUsize::new(0);
    let mut current: Option<(CareCategoryId, String)> = None;
    let mut throttle = Throttle::new(EVENT_INTERVAL);
    on_event(snapshot(CarePhase::Scanning, &rows, &current, started));

    std::thread::scope(|scope| {
        let (tx, rx) = mpsc::channel::<Msg>();
        for _ in 0..workers.clamp(1, MAX_WORKERS) {
            let tx = tx.clone();
            let order = &order;
            let next = &next;
            let stop = stop.clone();
            scope.spawn(move || loop {
                let n = next.fetch_add(1, Ordering::SeqCst);
                let Some(&index) = order.get(n) else { break };
                let category = categories[index];
                let id = category.id();
                let skip = skips.get(&id).cloned().unwrap_or_else(|| Arc::new(AtomicBool::new(false)));
                if stop.load(Ordering::SeqCst) || skip.load(Ordering::SeqCst) {
                    let reason = if stop.load(Ordering::SeqCst) { "stopped" } else { "skipped" };
                    let _ = tx.send(Msg::Done(index, CareCategoryScan::with_state(id, CareCategoryState::Skipped, Some(reason))));
                    continue;
                }
                let _ = tx.send(Msg::Started(index));
                let event_tx = Mutex::new(tx.clone());
                let sink: Arc<dyn Fn(CareEvent) + Send + Sync> = Arc::new(move |event| {
                    let _ = event_tx.lock().unwrap_or_else(|e| e.into_inner()).send(Msg::Event(event));
                });
                let ctx = CareContext::new(is_admin, stop.clone(), skip, sink);
                let scan = scan_one(category, &ctx);
                if tx.send(Msg::Done(index, scan)).is_err() {
                    break;
                }
            });
        }
        drop(tx);

        loop {
            let mut force = false;
            match rx.recv_timeout(EVENT_INTERVAL) {
                Ok(Msg::Started(i)) => {
                    rows[i].status = CareRunStatus::Running;
                    rows[i].started = Some(Instant::now());
                    force = true;
                }
                Ok(Msg::Event(CareEvent::Current { category, text })) => current = Some((category, text)),
                Ok(Msg::Event(CareEvent::Found { category, count })) => {
                    if let Some(row) = rows.iter_mut().find(|r| r.category == category && r.status == CareRunStatus::Running) {
                        row.count = count;
                    }
                }
                Ok(Msg::Done(i, scan)) => {
                    let row = &mut rows[i];
                    row.status = CareRunStatus::Done;
                    row.state = Some(scan.state);
                    row.count = issue_count(&scan);
                    row.elapsed_ms = scan.elapsed_ms;
                    if current.as_ref().is_some_and(|(c, _)| *c == row.category) {
                        current = None;
                    }
                    results[i] = Some(scan);
                    force = true;
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
            if current.is_none() {
                // Show the first running category (display order) while it has not reported text.
                current = rows
                    .iter()
                    .find(|r| r.status == CareRunStatus::Running)
                    .map(|r| (r.category, String::new()));
            }
            if throttle.ready(Instant::now(), force) {
                on_event(snapshot(CarePhase::Scanning, &rows, &current, started));
            }
        }
    });

    let stopped = stop.load(Ordering::SeqCst);
    let categories: Vec<CareCategoryScan> = results
        .into_iter()
        .zip(categories.iter())
        .map(|(r, c)| r.unwrap_or_else(|| CareCategoryScan::with_state(c.id(), CareCategoryState::Skipped, Some("stopped"))))
        .collect();
    on_event(snapshot(CarePhase::Complete, &rows, &None, started));
    CareScanReport {
        scan_id: uuid::Uuid::new_v4().to_string(),
        categories,
        is_admin,
        stopped,
        started_at_ms,
        elapsed_ms: started.elapsed().as_millis() as u64,
    }
}

fn state_code(state: CareCategoryState) -> &'static str {
    match state {
        CareCategoryState::Clean => "clean",
        CareCategoryState::Issues => "issues",
        CareCategoryState::Attention => "attention",
        CareCategoryState::NotApplicable => "notApplicable",
        CareCategoryState::Skipped => "skipped",
        CareCategoryState::Failed => "failed",
    }
}

fn pool_size() -> usize {
    std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1).clamp(1, MAX_WORKERS)
}

fn selected_categories(selected: &[CareCategoryId]) -> Vec<&'static dyn CareCategory> {
    let wanted: HashSet<CareCategoryId> = selected.iter().copied().collect();
    let by_id: HashMap<CareCategoryId, &'static dyn CareCategory> =
        registry().iter().map(|c| (c.id(), c.as_ref())).collect();
    CareCategoryId::ALL
        .iter()
        .filter(|id| wanted.contains(id))
        .filter_map(|id| by_id.get(id).copied())
        .collect()
}

/// Scan the selected categories (read-only). Display order follows `CareCategoryId::ALL`.
pub fn scan_care<E>(selected: &[CareCategoryId], on_event: E) -> CoreResult<CareScanReport>
where
    E: Fn(CareProgress) + Send + Sync,
{
    let cats = selected_categories(selected);
    if cats.is_empty() {
        return Err(CoreError::InvalidInput("no care categories selected".into()));
    }
    let _guard = BusyGuard::acquire()?;
    let skips: HashMap<CareCategoryId, Arc<AtomicBool>> =
        cats.iter().map(|c| (c.id(), Arc::new(AtomicBool::new(false)))).collect();
    install_skips(&skips);

    let mut timings = load_timings();
    let weights: HashMap<CareCategoryId, f64> = cats
        .iter()
        .filter_map(|c| timings.get(c.id().as_str()).map(|w| (c.id(), *w)))
        .collect();
    let is_admin = pctoolkit_platform::is_user_admin();
    let report = run_scan(&cats, &weights, is_admin, stop_flag(), &skips, pool_size(), on_event);

    // Learn only from categories that ran to a real conclusion.
    for scan in &report.categories {
        if !matches!(scan.state, CareCategoryState::Skipped | CareCategoryState::Failed) {
            let key = scan.category.as_str().to_string();
            let next = ema(timings.get(&key).copied(), scan.elapsed_ms.max(1) as f64);
            timings.insert(key, next);
        }
    }
    save_timings(timings);
    record_scan_history(&report);
    *LAST_SCAN.lock().unwrap_or_else(|e| e.into_inner()) = Some(report.clone());
    Ok(report)
}

fn record_scan_history(report: &CareScanReport) {
    let issues: u64 = report
        .categories
        .iter()
        .filter(|c| c.state == CareCategoryState::Issues)
        .map(issue_count)
        .sum();
    let bytes: u64 = report.categories.iter().flat_map(|c| c.items.iter()).filter_map(|i| i.bytes).sum();
    let failed = report.categories.iter().filter(|c| c.state == CareCategoryState::Failed).count() as u32;
    let detail_lines = report
        .categories
        .iter()
        .map(|c| {
            format!(
                "{}: {} items={} count={} elapsedMs={}{}",
                c.category.as_str(),
                state_code(c.state),
                c.items.len(),
                issue_count(c),
                c.elapsed_ms,
                c.reason.as_deref().map(|r| format!(" ({r})")).unwrap_or_default()
            )
        })
        .collect();
    append_history(HistoryWrite {
        category: "care".into(),
        title_key: "history.titles.care".into(),
        summary: format!("Care scan: {} categories, {} issues", report.categories.len(), issues),
        started_at_ms: report.started_at_ms,
        finished_at_ms: history_now_ms(),
        outcome: if report.stopped {
            HistoryOutcome::Cancelled
        } else if failed > 0 {
            HistoryOutcome::CompletedWithWarnings
        } else {
            HistoryOutcome::Completed
        },
        planned_bytes: Some(bytes),
        result_bytes: None,
        selected_item_count: report.categories.len() as u32,
        affected_item_count: issues.min(u32::MAX as u64) as u32,
        failed_item_count: failed,
        detail_lines,
        action: "care_scan".into(),
        detail: Some(format!("elapsedMs={}", report.elapsed_ms)),
    });
}

// ---------------------------------------------------------------------------------------------
// Fix
// ---------------------------------------------------------------------------------------------

fn fix_one(category: &dyn CareCategory, ids: &[String], ctx: &CareContext) -> Vec<CareFixResult> {
    let id = category.id();
    let outcome = catch_unwind(AssertUnwindSafe(|| category.fix(ids, ctx)));
    let results = match outcome {
        Ok(Ok(results)) => results,
        Ok(Err(CoreError::OperationBusy)) => super::fix_all(id, ids, CareFixStatus::Skipped, Some("busy")),
        Ok(Err(CoreError::OperationCancelled)) => super::fix_all(id, ids, CareFixStatus::Skipped, Some("cancelled")),
        Ok(Err(CoreError::PermissionDenied)) => super::fix_all(id, ids, CareFixStatus::Skipped, Some("needsAdmin")),
        Ok(Err(_)) => super::fix_all(id, ids, CareFixStatus::Failed, Some("failed")),
        Err(_) => super::fix_all(id, ids, CareFixStatus::Failed, Some("panicked")),
    };
    normalize_results(id, ids, results)
}

/// Totals over fix results.
pub fn summarize_fix(results: Vec<CareFixResult>, stopped: bool, elapsed_ms: u64, after_action: CareAfterAction) -> CareFixReport {
    let count = |s: CareFixStatus| results.iter().filter(|r| r.status == s).count() as u64;
    CareFixReport {
        fixed: count(CareFixStatus::Fixed),
        partial: count(CareFixStatus::Partial),
        failed: count(CareFixStatus::Failed),
        skipped: count(CareFixStatus::Skipped),
        freed_bytes: results.iter().map(|r| r.freed_bytes).sum(),
        undoable: results.iter().filter(|r| r.undoable).count() as u64,
        results,
        stopped,
        elapsed_ms,
        after_action,
        after_action_error: None,
        after_action_delay_secs: None,
    }
}

/// Engine core of a fix with injectable categories. Categories run sequentially in display order
/// (fixers may share global resources such as the Deep Cleanup busy flag).
pub(crate) fn run_fix<E>(
    categories: &[&dyn CareCategory],
    plan: FixPlan,
    is_admin: bool,
    stop: Arc<AtomicBool>,
    skips: &HashMap<CareCategoryId, Arc<AtomicBool>>,
    on_event: E,
) -> (Vec<CareFixResult>, bool)
where
    E: Fn(CareProgress) + Send + Sync,
{
    let started = Instant::now();
    let by_id: HashMap<CareCategoryId, &dyn CareCategory> = categories.iter().map(|c| (c.id(), *c)).collect();
    let mut rows: Vec<Row> = plan
        .work
        .iter()
        .map(|(c, ids)| Row {
            category: *c,
            // Rough expected fix time; only used to interpolate the running row.
            weight: 500.0 + 50.0 * ids.len() as f64,
            status: CareRunStatus::Queued,
            state: None,
            count: 0,
            started: None,
            elapsed_ms: 0,
        })
        .collect();
    let mut results = plan.rejected;
    let mut throttle = Throttle::new(EVENT_INTERVAL);
    on_event(snapshot(CarePhase::Fixing, &rows, &None, started));

    for (index, (category, ids)) in plan.work.iter().enumerate() {
        let skip = skips.get(category).cloned().unwrap_or_else(|| Arc::new(AtomicBool::new(false)));
        let Some(handler) = by_id.get(category).copied() else {
            results.extend(super::fix_all(*category, ids, CareFixStatus::Skipped, Some("unknownItem")));
            rows[index].status = CareRunStatus::Done;
            rows[index].state = Some(CareCategoryState::Skipped);
            continue;
        };
        if stop.load(Ordering::SeqCst) || skip.load(Ordering::SeqCst) {
            results.extend(super::fix_all(*category, ids, CareFixStatus::Skipped, Some("cancelled")));
            rows[index].status = CareRunStatus::Done;
            rows[index].state = Some(CareCategoryState::Skipped);
            continue;
        }
        rows[index].status = CareRunStatus::Running;
        rows[index].started = Some(Instant::now());
        on_event(snapshot(CarePhase::Fixing, &rows, &None, started));

        let (tx, rx) = mpsc::channel::<CareEvent>();
        let tx = Mutex::new(tx);
        let sink: Arc<dyn Fn(CareEvent) + Send + Sync> = Arc::new(move |event| {
            let _ = tx.lock().unwrap_or_else(|e| e.into_inner()).send(event);
        });
        let ctx = CareContext::new(is_admin, stop.clone(), skip.clone(), sink);
        let mut current: Option<(CareCategoryId, String)> = None;
        let fixed = std::thread::scope(|scope| {
            // The worker owns the context, so the channel disconnects when the fix returns.
            let worker = scope.spawn(move || fix_one(handler, ids, &ctx));
            loop {
                match rx.recv_timeout(EVENT_INTERVAL) {
                    Ok(CareEvent::Current { category, text }) => current = Some((category, text)),
                    Ok(CareEvent::Found { count, .. }) => rows[index].count = count,
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
                if throttle.ready(Instant::now(), false) {
                    on_event(snapshot(CarePhase::Fixing, &rows, &current, started));
                }
            }
            worker
                .join()
                .unwrap_or_else(|_| super::fix_all(*category, ids, CareFixStatus::Failed, Some("panicked")))
        });
        let done = fixed.iter().filter(|r| matches!(r.status, CareFixStatus::Fixed | CareFixStatus::Partial)).count() as u64;
        let row = &mut rows[index];
        row.status = CareRunStatus::Done;
        row.count = done;
        row.elapsed_ms = row.started.map(|s| s.elapsed().as_millis() as u64).unwrap_or(0);
        row.state = Some(if done as usize == ids.len() {
            CareCategoryState::Clean
        } else if skip.load(Ordering::SeqCst) || stop.load(Ordering::SeqCst) {
            CareCategoryState::Skipped
        } else {
            // Some items were not fixed: the UI shows the attention mark for this row.
            CareCategoryState::Attention
        });
        results.extend(fixed);
        on_event(snapshot(CarePhase::Fixing, &rows, &None, started));
    }
    on_event(snapshot(CarePhase::Complete, &rows, &None, started));
    (results, stop.load(Ordering::SeqCst))
}

fn power_action(after: CareAfterAction) -> Option<PowerAction> {
    match after {
        CareAfterAction::None => None,
        CareAfterAction::Shutdown => Some(PowerAction::Shutdown),
        CareAfterAction::Restart => Some(PowerAction::Restart),
        CareAfterAction::Sleep => Some(PowerAction::Sleep),
        CareAfterAction::Hibernate => Some(PowerAction::Hibernate),
    }
}

/// Fix items of the last scan. Only ids are trusted; every category re-validates its items.
/// The optional after-action runs only after all fixes finished and only when not stopped.
pub fn fix_care<E>(request: CareFixRequest, on_event: E) -> CoreResult<CareFixReport>
where
    E: Fn(CareProgress) + Send + Sync,
{
    let _guard = BusyGuard::acquire()?;
    let scan = last_care_scan().ok_or_else(|| CoreError::InvalidInput("no care scan to fix".into()))?;
    let started = Instant::now();
    let started_at_ms = history_now_ms();
    let is_admin = pctoolkit_platform::is_user_admin();
    let plan = plan_fix(&scan, &request, is_admin);
    let skips: HashMap<CareCategoryId, Arc<AtomicBool>> =
        plan.work.iter().map(|(c, _)| (*c, Arc::new(AtomicBool::new(false)))).collect();
    install_skips(&skips);
    let cats: Vec<&dyn CareCategory> = registry().iter().map(|c| c.as_ref()).collect();
    let (results, stopped) = run_fix(&cats, plan, is_admin, stop_flag(), &skips, on_event);

    // Forget fixed items so a second fix of the same scan does not retry them.
    if let Some(last) = LAST_SCAN.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
        let done: HashSet<(CareCategoryId, &str)> = results
            .iter()
            .filter(|r| r.status == CareFixStatus::Fixed)
            .map(|r| (r.category, r.item_id.as_str()))
            .collect();
        for cat in &mut last.categories {
            cat.items.retain(|i| !done.contains(&(cat.category, i.id.as_str())));
        }
    }

    let mut report = summarize_fix(results, stopped, started.elapsed().as_millis() as u64, request.after_action);
    record_fix_history(&report, started_at_ms, request.auto);
    if !stopped {
        // Shutdown / restart never fire instantly: schedule them with a grace period the UI can
        // cancel. Sleep / hibernate keep the session, so they run right away.
        let outcome = match request.after_action {
            CareAfterAction::Shutdown => {
                pctoolkit_platform::schedule_shutdown(CARE_AFTER_ACTION_DELAY_SECS).map(|_| true).map_err(|_| ())
            }
            CareAfterAction::Restart => {
                pctoolkit_platform::schedule_restart(CARE_AFTER_ACTION_DELAY_SECS).map(|_| true).map_err(|_| ())
            }
            other => match power_action(other) {
                Some(action) => execute_power(action).map(|_| false).map_err(|_| ()),
                None => Ok(false),
            },
        };
        match outcome {
            Ok(true) => report.after_action_delay_secs = Some(CARE_AFTER_ACTION_DELAY_SECS),
            Ok(false) => {}
            Err(()) => report.after_action_error = Some("failed".into()),
        }
    }
    Ok(report)
}

fn record_fix_history(report: &CareFixReport, started_at_ms: i64, auto: bool) {
    let mut per_category: BTreeMap<&'static str, [u64; 4]> = BTreeMap::new();
    for r in &report.results {
        let slot = per_category.entry(r.category.as_str()).or_default();
        match r.status {
            CareFixStatus::Fixed => slot[0] += 1,
            CareFixStatus::Partial => slot[1] += 1,
            CareFixStatus::Failed => slot[2] += 1,
            CareFixStatus::Skipped => slot[3] += 1,
        }
    }
    let detail_lines = per_category
        .iter()
        .map(|(c, [f, p, x, s])| format!("{c}: fixed={f} partial={p} failed={x} skipped={s}"))
        .collect();
    let selected = report.results.len() as u32;
    append_history(HistoryWrite {
        category: "care".into(),
        title_key: "history.titles.careFix".into(),
        summary: format!(
            "Care fix{}: {} fixed, {} failed, {} skipped",
            if auto { " (auto)" } else { "" },
            report.fixed + report.partial,
            report.failed,
            report.skipped
        ),
        started_at_ms,
        finished_at_ms: history_now_ms(),
        outcome: if report.stopped {
            HistoryOutcome::Cancelled
        } else if report.failed == 0 && report.partial == 0 {
            HistoryOutcome::Completed
        } else if report.fixed + report.partial > 0 {
            HistoryOutcome::CompletedWithWarnings
        } else {
            HistoryOutcome::Failed
        },
        planned_bytes: None,
        result_bytes: Some(report.freed_bytes),
        selected_item_count: selected,
        affected_item_count: (report.fixed + report.partial) as u32,
        failed_item_count: report.failed as u32,
        detail_lines,
        action: if auto { "care_fix_auto".into() } else { "care_fix".into() },
        detail: Some(format!("afterAction={:?}", report.after_action)),
    });
}

#[cfg(test)]
#[path = "engine_tests.rs"]
mod tests;
