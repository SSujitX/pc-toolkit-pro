//! Startup Manager: what starts with Windows, how much it slows boot, and reversible switches.
//!
//! Sources: Run / RunOnce values (HKCU, HKLM native + `WOW6432Node`), Startup folders (user +
//! all users), scheduled tasks with logon / boot triggers, auto-start Win32 services, and the
//! current user's packaged (MSIX / Store) app startup tasks. Listing is read-only and needs no
//! admin. Changes use exactly what Windows uses: `StartupApproved` markers (Task Manager),
//! `IRegisteredTask::put_Enabled`, service start types, and the packaged task `State` value
//! (Settings → Startup apps; policy-controlled tasks are read-only). Machine-wide changes need
//! an elevated process and report `skippedNeedAdmin` instead of elevating. Microsoft services / tasks are shown on request but never changed.
//!
//! Persistence under `%LOCALAPPDATA%\PC Toolkit Pro\`:
//! - `startup-undo.json` (version 1): previous service start types and delayed-start tasks we
//!   created. Unreadable / unknown version → empty (an enabled service then returns to Automatic).
//! - `startup-known.json` (version 1): first-seen time per item id for "new" flags. Missing or
//!   unreadable → rebuilt as a baseline (nothing is flagged on the first run).
//! - `startup-backups\<timestamp>\`: `.reg` / task XML / JSON backups written before a broken
//!   entry is deleted (no backup → no delete).
//!
//! Logs and history never carry command lines or paths; ids are opaque to the frontend.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use pctoolkit_platform::startup_items::{
    self as items, ApprovedList, BootPerfAccess, BootPerfEvent, FileVersionFacts, ServiceStartKind,
    StartupEntryFact, StartupLocation, StartupScope, StartupSource,
};
use pctoolkit_platform::{
    export_registry_key, icon_png_from_file, is_user_admin, recycle_path, split_command_line, PlatformError,
    RegistryRoot, RegistryView,
};
use serde::{Deserialize, Serialize};

use crate::shared::{CoreError, CoreResult};

/// Task folder (or name prefix when the folder cannot be created) of our delayed-start tasks.
const DELAYED_TASK_FOLDER: &str = "PC Toolkit Pro Delayed";
pub const DEFAULT_DELAY_SECONDS: u32 = 60;
/// A new item stays flagged "new" this long after it first appeared.
const NEW_ITEM_WINDOW_MS: i64 = 14 * 24 * 60 * 60 * 1000;
/// Boot degradation events older than this are ignored for impact.
const IMPACT_WINDOW_MS: i64 = 30 * 24 * 60 * 60 * 1000;
const BOOT_EVENTS_MAX: usize = 400;
const BOOT_CACHE_TTL: Duration = Duration::from_secs(300);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StartupState {
    Enabled,
    Disabled,
    /// Starts later: a delayed logon task we created, or a service set to Automatic (Delayed).
    Delayed,
}

/// Boot impact from Windows' own Diagnostics-Performance measurements.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StartupImpact {
    /// Windows reported it delaying boot by ≥ 1 s.
    High,
    /// ≥ 300 ms.
    Medium,
    Low,
    /// The log is readable and Windows did not report this item slowing boot.
    NotFlagged,
    /// The log is not readable (needs admin) or unavailable.
    Unknown,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartupItem {
    pub id: String,
    pub name: String,
    /// Registry value / file / task path / service key name (display only).
    pub entry_name: String,
    pub publisher: Option<String>,
    pub description: Option<String>,
    /// Command line (display only).
    pub command: String,
    /// Resolved executable (display only).
    pub target_path: Option<String>,
    pub source: StartupSource,
    pub scope: StartupScope,
    pub state: StartupState,
    pub service_start: Option<ServiceStartKind>,
    pub is_microsoft: bool,
    /// Changing this item needs an elevated process.
    pub requires_admin: bool,
    pub can_toggle: bool,
    pub can_delay: bool,
    pub can_delete: bool,
    /// Target file is missing on a present local drive.
    pub broken: bool,
    pub is_new: bool,
    pub impact: StartupImpact,
    /// Worst boot delay Windows attributed to it (recent boots), ms.
    pub impact_ms: Option<u64>,
    pub disabled_at_ms: Option<i64>,
    pub delay_seconds: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BootSummary {
    pub access: BootPerfAccess,
    pub last_boot_ms: Option<u64>,
    pub last_boot_at_ms: Option<i64>,
    /// Boots recorded in the log (sampled window).
    pub boots_recorded: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartupInventory {
    pub items: Vec<StartupItem>,
    pub is_admin: bool,
    pub boot: BootSummary,
    /// Microsoft tasks / services hidden by the current filter.
    pub hidden_microsoft_count: u32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StartupListRequest {
    #[serde(default)]
    pub include_microsoft: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StartupActionKind {
    Enable,
    Disable,
    Delay,
    Undelay,
    Delete,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StartupActionRequest {
    pub id: String,
    pub action: StartupActionKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StartupActionStatus {
    Done,
    SkippedNeedAdmin,
    /// The item vanished or changed since the listing.
    NotFound,
    /// The action is not offered for this item (Microsoft component, RunOnce, not broken, …).
    Unsupported,
    Failed,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartupActionResult {
    pub id: String,
    pub action: StartupActionKind,
    pub status: StartupActionStatus,
    /// Typed diagnostic (`backupFailed`, `taskRegisterFailed`, …).
    pub reason: Option<String>,
    /// The item after the change (absent after delete / when it disappeared).
    pub item: Option<StartupItem>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartupIcon {
    pub id: String,
    pub data_url: Option<String>,
}

// ---------------------------------------------------------------------------------------------
// Persistence
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ServiceUndo {
    previous: ServiceStartKind,
    changed_at_ms: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DelayedUndo {
    task_path: String,
    delay_seconds: u32,
    changed_at_ms: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UndoFile {
    version: u32,
    #[serde(default)]
    services: BTreeMap<String, ServiceUndo>,
    #[serde(default)]
    delayed: BTreeMap<String, DelayedUndo>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct KnownSnapshot {
    version: u32,
    baseline_at_ms: i64,
    /// Item id → first time it was seen (Unix ms).
    items: BTreeMap<String, i64>,
}

fn app_dir() -> Option<PathBuf> {
    let base = std::env::var_os("LOCALAPPDATA").or_else(|| std::env::var_os("APPDATA"))?;
    Some(PathBuf::from(base).join("PC Toolkit Pro"))
}

fn read_json<T: for<'de> Deserialize<'de>>(name: &str) -> Option<T> {
    let text = fs::read_to_string(app_dir()?.join(name)).ok()?;
    serde_json::from_str(&text).ok()
}

fn write_json<T: Serialize>(name: &str, value: &T) -> CoreResult<()> {
    let dir = app_dir().ok_or_else(|| CoreError::OperationFailed("appDataUnavailable".into()))?;
    fs::create_dir_all(&dir).map_err(|e| CoreError::OperationFailed(format!("startupStateWrite:{}", e.kind())))?;
    let text = serde_json::to_string_pretty(value).map_err(|_| CoreError::OperationFailed("startupStateEncode".into()))?;
    let target = dir.join(name);
    let temp = dir.join(format!("{name}.tmp"));
    fs::write(&temp, text).map_err(|e| CoreError::OperationFailed(format!("startupStateWrite:{}", e.kind())))?;
    fs::rename(&temp, &target).map_err(|e| CoreError::OperationFailed(format!("startupStateWrite:{}", e.kind())))
}

const UNDO_FILE: &str = "startup-undo.json";
const KNOWN_FILE: &str = "startup-known.json";

fn load_undo() -> UndoFile {
    match read_json::<UndoFile>(UNDO_FILE) {
        Some(file) if file.version == 1 => file,
        _ => UndoFile { version: 1, ..Default::default() },
    }
}

fn save_undo(file: &UndoFile) -> CoreResult<()> {
    write_json(UNDO_FILE, file)
}

/// Merge the current item ids into the known-items snapshot. Returns the new snapshot and the
/// ids that count as "new" (appeared after the baseline and within the window).
fn merge_known(previous: Option<KnownSnapshot>, ids: &[String], now_ms: i64) -> (KnownSnapshot, HashSet<String>) {
    let Some(previous) = previous.filter(|p| p.version == 1) else {
        let snapshot = KnownSnapshot {
            version: 1,
            baseline_at_ms: now_ms,
            items: ids.iter().map(|id| (id.clone(), now_ms)).collect(),
        };
        return (snapshot, HashSet::new());
    };
    let mut items = BTreeMap::new();
    let mut fresh = HashSet::new();
    for id in ids {
        let first_seen = previous.items.get(id).copied().unwrap_or(now_ms);
        if first_seen > previous.baseline_at_ms && now_ms - first_seen < NEW_ITEM_WINDOW_MS {
            fresh.insert(id.clone());
        }
        items.insert(id.clone(), first_seen);
    }
    (KnownSnapshot { version: 1, baseline_at_ms: previous.baseline_at_ms, items }, fresh)
}

// ---------------------------------------------------------------------------------------------
// Pure policy helpers
// ---------------------------------------------------------------------------------------------

/// Stable id of an entry, unique across sources.
fn item_id(fact: &StartupEntryFact) -> String {
    let root = |r: RegistryRoot| if r == RegistryRoot::CurrentUser { "cu" } else { "lm" };
    match &fact.location {
        StartupLocation::Registry { key, value_name, .. } => format!(
            "{}:{}:{}:{}",
            if fact.source == StartupSource::RunOnce { "runonce" } else { "run" },
            root(key.root),
            if key.view == RegistryView::Wow32 { "32" } else { "64" },
            value_name
        ),
        StartupLocation::Folder { .. } => format!(
            "folder:{}:{}",
            if fact.scope == StartupScope::User { "cu" } else { "lm" },
            fact.entry_name
        ),
        StartupLocation::Task { path } => format!("task:{path}"),
        StartupLocation::Service { name } => format!("svc:{name}"),
        StartupLocation::Package { family, task_id } => format!("pkg:{family}:{task_id}"),
    }
}

fn impact_from_ms(ms: u64) -> StartupImpact {
    if ms >= 1_000 {
        StartupImpact::High
    } else if ms >= 300 {
        StartupImpact::Medium
    } else {
        StartupImpact::Low
    }
}

/// A target counts as broken only when it is an absolute local path on a drive that is present
/// and the file is gone. Network / removable / unresolvable targets are never "broken".
fn is_broken_target(target: &Path, exists: &dyn Fn(&Path) -> bool) -> bool {
    let text = target.to_string_lossy();
    let bytes = text.as_bytes();
    let drive_path = bytes.len() > 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] == b'\\';
    if !drive_path {
        return false;
    }
    let root = PathBuf::from(&text[..3]);
    exists(&root) && !exists(target)
}

fn is_microsoft_company(company: Option<&str>) -> bool {
    company.is_some_and(|c| c.to_ascii_lowercase().contains("microsoft"))
}

/// File name safe for a task name.
fn task_safe_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| if "\\/:*?\"<>|".contains(c) || c.is_control() { '_' } else { c })
        .collect();
    let trimmed = cleaned.trim().trim_matches('.').to_string();
    if trimmed.is_empty() {
        "Startup entry".into()
    } else {
        trimmed.chars().take(120).collect()
    }
}

// ---------------------------------------------------------------------------------------------
// Inventory
// ---------------------------------------------------------------------------------------------

struct Record {
    item: StartupItem,
    fact: StartupEntryFact,
}

static CACHE: Mutex<Vec<Record>> = Mutex::new(Vec::new());
static OP_LOCK: Mutex<()> = Mutex::new(());
static VERSION_CACHE: Mutex<Option<HashMap<PathBuf, FileVersionFacts>>> = Mutex::new(None);
static ICON_CACHE: Mutex<Option<HashMap<PathBuf, Option<String>>>> = Mutex::new(None);
static BOOT_CACHE: Mutex<Option<(Instant, BootData)>> = Mutex::new(None);

#[derive(Clone)]
struct BootData {
    summary: BootSummary,
    /// Lowercase file / friendly name → worst degradation ms.
    degradations: HashMap<String, u64>,
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn version_facts(path: &Path) -> FileVersionFacts {
    let mut guard = VERSION_CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let cache = guard.get_or_insert_with(HashMap::new);
    if let Some(found) = cache.get(path) {
        return found.clone();
    }
    let facts = if path.exists() { items::file_version_facts(path) } else { FileVersionFacts::default() };
    cache.insert(path.to_path_buf(), facts.clone());
    facts
}

fn parse_event_time(text: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(text).ok().map(|t| t.timestamp_millis())
}

fn summarize_boot(access: BootPerfAccess, events: &[BootPerfEvent], now: i64) -> BootData {
    let mut summary = BootSummary { access, last_boot_ms: None, last_boot_at_ms: None, boots_recorded: 0 };
    let mut degradations: HashMap<String, u64> = HashMap::new();
    for event in events {
        match event {
            BootPerfEvent::Boot { system_time, boot_ms, .. } => {
                summary.boots_recorded += 1;
                if summary.last_boot_ms.is_none() {
                    summary.last_boot_ms = Some(*boot_ms);
                    summary.last_boot_at_ms = parse_event_time(system_time);
                }
            }
            BootPerfEvent::Degradation { system_time, file_name, friendly_name, degradation_ms, .. } => {
                if parse_event_time(system_time).is_some_and(|t| now - t > IMPACT_WINDOW_MS) {
                    continue;
                }
                for key in [file_name, friendly_name] {
                    let key = key.trim().to_ascii_lowercase();
                    if key.is_empty() {
                        continue;
                    }
                    let slot = degradations.entry(key).or_insert(0);
                    *slot = (*slot).max(*degradation_ms);
                }
            }
        }
    }
    BootData { summary, degradations }
}

fn boot_data(force: bool) -> BootData {
    let mut guard = BOOT_CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if !force {
        if let Some((at, data)) = guard.as_ref() {
            if at.elapsed() < BOOT_CACHE_TTL {
                return data.clone();
            }
        }
    }
    let (access, events) = items::read_boot_performance(BOOT_EVENTS_MAX);
    let data = summarize_boot(access, &events, now_ms());
    *guard = Some((Instant::now(), data.clone()));
    data
}

fn collect_facts(sources: &[StartupSource]) -> Vec<StartupEntryFact> {
    let mut facts = Vec::new();
    if sources.iter().any(|s| matches!(s, StartupSource::Run | StartupSource::RunOnce)) {
        facts.extend(items::list_run_entries());
    }
    if sources.contains(&StartupSource::StartupFolder) {
        facts.extend(items::list_startup_folder_entries());
    }
    if sources.contains(&StartupSource::ScheduledTask) {
        facts.extend(items::list_task_entries());
    }
    if sources.contains(&StartupSource::Service) {
        facts.extend(items::list_service_entries());
    }
    if sources.contains(&StartupSource::PackagedApp) {
        facts.extend(items::list_packaged_entries());
    }
    facts
}

const ALL_SOURCES: [StartupSource; 6] = [
    StartupSource::Run,
    StartupSource::RunOnce,
    StartupSource::StartupFolder,
    StartupSource::ScheduledTask,
    StartupSource::Service,
    StartupSource::PackagedApp,
];

/// Turn platform facts into items (policy: what is shown, changeable, broken, delayed).
fn build_records(facts: Vec<StartupEntryFact>, undo: &UndoFile, boot: &BootData) -> Vec<Record> {
    let delayed_tasks: HashSet<String> = undo.delayed.values().map(|d| d.task_path.to_ascii_lowercase()).collect();
    let delayed_prefix = format!("\\{}", DELAYED_TASK_FOLDER.to_ascii_lowercase());
    let mut out = Vec::new();
    for mut fact in facts {
        if fact.source == StartupSource::ScheduledTask {
            let lower = fact.entry_name.to_ascii_lowercase();
            // Our own delayed-start tasks are shown as the "Delayed" state of their entry.
            if delayed_tasks.contains(&lower) || lower.starts_with(&delayed_prefix) {
                continue;
            }
        }
        if fact.source == StartupSource::Service {
            let tracked = undo.services.contains_key(&fact.entry_name);
            let auto = matches!(
                fact.service_start,
                Some(ServiceStartKind::Automatic | ServiceStartKind::AutomaticDelayed)
            );
            if !auto && !tracked {
                continue;
            }
            fact.label = items::resolve_indirect_string(&fact.label);
        }
        let id = item_id(&fact);
        let version = fact.target.as_deref().map(version_facts).unwrap_or_default();
        let package = fact.package.as_ref();
        let publisher = package
            .map(|p| p.publisher.trim())
            .filter(|p| !p.is_empty())
            .map(str::to_string)
            .or_else(|| version.company.clone());
        let is_microsoft = fact.microsoft_task
            || package.is_some_and(|p| p.is_system)
            || is_microsoft_company(publisher.as_deref())
            || is_microsoft_company(version.company.as_deref());
        // Packages are installed and repaired by Windows; a missing package file is never offered
        // as a "broken entry" to delete.
        let broken = package.is_none() && fact.target.as_deref().is_some_and(|t| is_broken_target(t, &|p| p.exists()));
        let policy_locked = package.is_some_and(|p| p.state.is_policy_locked());
        let delayed = undo.delayed.get(&id);
        let state = match fact.source {
            StartupSource::Service => match fact.service_start {
                Some(ServiceStartKind::AutomaticDelayed) => StartupState::Delayed,
                Some(ServiceStartKind::Automatic) => StartupState::Enabled,
                _ => StartupState::Disabled,
            },
            _ if delayed.is_some() && !fact.enabled => StartupState::Delayed,
            _ if fact.enabled => StartupState::Enabled,
            _ => StartupState::Disabled,
        };
        let requires_admin = match fact.source {
            StartupSource::Run | StartupSource::RunOnce | StartupSource::StartupFolder => fact.scope == StartupScope::Machine,
            StartupSource::ScheduledTask => fact.task_privileged,
            StartupSource::Service => true,
            // Per-user state, exactly what Settings → Startup apps changes.
            StartupSource::PackagedApp => false,
        };
        let protected = is_microsoft && matches!(fact.source, StartupSource::Service | StartupSource::ScheduledTask);
        let can_toggle = !protected && !policy_locked && fact.source != StartupSource::RunOnce;
        let can_delay = !protected
            && !broken
            && match fact.source {
                StartupSource::Run | StartupSource::StartupFolder => {
                    fact.scope == StartupScope::User && fact.target.is_some()
                }
                StartupSource::Service => state != StartupState::Disabled,
                _ => false,
            };
        let can_delete =
            broken && !protected && !matches!(fact.source, StartupSource::Service | StartupSource::PackagedApp);
        let (impact, impact_ms) = if boot.summary.access == BootPerfAccess::Available {
            let mut keys = vec![fact.label.to_ascii_lowercase(), fact.entry_name.to_ascii_lowercase()];
            if let Some(name) = fact.target.as_deref().and_then(|t| t.file_name()) {
                keys.push(name.to_string_lossy().to_ascii_lowercase());
            }
            if let Some(desc) = version.description.as_deref() {
                keys.push(desc.to_ascii_lowercase());
            }
            match keys.iter().filter_map(|k| boot.degradations.get(k)).max() {
                Some(ms) => (impact_from_ms(*ms), Some(*ms)),
                None => (StartupImpact::NotFlagged, None),
            }
        } else {
            (StartupImpact::Unknown, None)
        };
        // A packaged task's own name ("SamsungNotesSysTray") says more than the exe description.
        let description = package
            .and_then(|p| p.task_name.clone())
            .filter(|name| !name.eq_ignore_ascii_case(&fact.label))
            .or_else(|| version.description.clone());
        let item = StartupItem {
            id,
            name: fact.label.clone(),
            entry_name: fact.entry_name.clone(),
            publisher,
            description,
            command: fact.command.clone(),
            target_path: fact.target.as_ref().map(|t| t.display().to_string()),
            source: fact.source,
            scope: fact.scope,
            state,
            service_start: fact.service_start,
            is_microsoft,
            requires_admin,
            can_toggle,
            can_delay,
            can_delete,
            broken,
            is_new: false,
            impact,
            impact_ms,
            disabled_at_ms: fact.disabled_at_ms,
            delay_seconds: delayed.map(|d| d.delay_seconds),
        };
        out.push(Record { item, fact });
    }
    out
}

fn visible(item: &StartupItem, include_microsoft: bool) -> bool {
    include_microsoft
        || !(item.is_microsoft && matches!(item.source, StartupSource::Service | StartupSource::ScheduledTask))
}

/// Full read-only inventory (also used by the Care category). Updates the known-items snapshot
/// and the id cache used by actions and icons.
fn refresh_all(force_boot: bool) -> Vec<StartupItem> {
    let boot = boot_data(force_boot);
    let undo = load_undo();
    let mut records = build_records(collect_facts(&ALL_SOURCES), &undo, &boot);
    let ids: Vec<String> = records.iter().map(|r| r.item.id.clone()).collect();
    let (snapshot, fresh) = merge_known(read_json::<KnownSnapshot>(KNOWN_FILE), &ids, now_ms());
    let _ = write_json(KNOWN_FILE, &snapshot);
    for record in &mut records {
        record.item.is_new = fresh.contains(&record.item.id);
    }
    let items: Vec<StartupItem> = records.iter().map(|r| r.item.clone()).collect();
    *CACHE.lock().unwrap_or_else(|e| e.into_inner()) = records;
    items
}

pub fn list_startup_items(request: StartupListRequest) -> CoreResult<StartupInventory> {
    let _guard = OP_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let all = refresh_all(true);
    let boot = boot_data(false).summary;
    let total = all.len();
    let items: Vec<StartupItem> = all.into_iter().filter(|i| visible(i, request.include_microsoft)).collect();
    Ok(StartupInventory {
        hidden_microsoft_count: (total - items.len()) as u32,
        items,
        is_admin: is_user_admin(),
        boot,
    })
}

/// Every item (Microsoft ones included) from a fresh read; for the Care category.
pub(crate) fn current_items() -> Vec<StartupItem> {
    let _guard = OP_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    refresh_all(false)
}

fn cached(id: &str) -> Option<(StartupItem, StartupEntryFact)> {
    CACHE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .find(|r| r.item.id == id)
        .map(|r| (r.item.clone(), r.fact.clone()))
}

/// Re-read one item's source and return its fresh record (re-validation before / after a change).
fn reread(id: &str, source: StartupSource) -> Option<(StartupItem, StartupEntryFact)> {
    let undo = load_undo();
    let boot = boot_data(false);
    let records = build_records(collect_facts(&[source]), &undo, &boot);
    let found = records.into_iter().find(|r| r.item.id == id)?;
    let mut item = found.item.clone();
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(slot) = cache.iter_mut().find(|r| r.item.id == id) {
        item.is_new = slot.item.is_new;
        slot.item = item.clone();
        slot.fact = found.fact.clone();
    }
    Some((item, found.fact))
}

// ---------------------------------------------------------------------------------------------
// Actions
// ---------------------------------------------------------------------------------------------

fn platform_status(error: &PlatformError) -> (StartupActionStatus, Option<String>) {
    match error {
        PlatformError::PermissionDenied => (StartupActionStatus::SkippedNeedAdmin, Some("needsAdmin".into())),
        PlatformError::Unsupported => (StartupActionStatus::Unsupported, Some("unsupported".into())),
        PlatformError::OperationFailed(code) => {
            let code = code.split(':').next().unwrap_or("failed").to_string();
            if code == "taskNotFound" {
                (StartupActionStatus::NotFound, Some(code))
            } else {
                (StartupActionStatus::Failed, Some(code))
            }
        }
    }
}

fn approved_target(fact: &StartupEntryFact) -> Option<(RegistryRoot, ApprovedList, String)> {
    match &fact.location {
        StartupLocation::Registry { key, value_name, approved: Some(list) } => Some((key.root, *list, value_name.clone())),
        StartupLocation::Folder { approved, .. } => Some((
            if fact.scope == StartupScope::User { RegistryRoot::CurrentUser } else { RegistryRoot::LocalMachine },
            *approved,
            fact.entry_name.clone(),
        )),
        _ => None,
    }
}

/// Run one action on an item from the latest listing. The item is re-read first; admin-only
/// changes in a non-elevated process are skipped without trying to elevate.
pub fn run_startup_action(request: StartupActionRequest) -> CoreResult<StartupActionResult> {
    let _guard = OP_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let id = request.id.clone();
    let action = request.action;
    let result = |status: StartupActionStatus, reason: Option<String>, item: Option<StartupItem>| StartupActionResult {
        id: id.clone(),
        action,
        status,
        reason,
        item,
    };
    let Some((cached_item, _)) = cached(&id) else {
        return Ok(result(StartupActionStatus::NotFound, Some("itemChanged".into()), None));
    };
    let Some((item, fact)) = reread(&id, cached_item.source) else {
        return Ok(result(StartupActionStatus::NotFound, Some("itemChanged".into()), None));
    };
    let allowed = match action {
        StartupActionKind::Enable | StartupActionKind::Disable => item.can_toggle,
        StartupActionKind::Delay => item.can_delay,
        StartupActionKind::Undelay => item.state == StartupState::Delayed && item.can_toggle,
        StartupActionKind::Delete => item.can_delete,
    };
    if !allowed {
        return Ok(result(StartupActionStatus::Unsupported, None, Some(item)));
    }
    if item.requires_admin && !is_user_admin() {
        return Ok(result(StartupActionStatus::SkippedNeedAdmin, Some("needsAdmin".into()), Some(item)));
    }
    let outcome = match action {
        StartupActionKind::Enable => set_enabled(&id, &item, &fact, true),
        StartupActionKind::Disable => set_enabled(&id, &item, &fact, false),
        StartupActionKind::Delay => set_delayed(&id, &item, &fact, true),
        StartupActionKind::Undelay => set_delayed(&id, &item, &fact, false),
        StartupActionKind::Delete => delete_broken(&item, &fact),
    };
    match outcome {
        Ok(()) => {
            let after = if action == StartupActionKind::Delete {
                CACHE.lock().unwrap_or_else(|e| e.into_inner()).retain(|r| r.item.id != id);
                None
            } else {
                reread(&id, item.source).map(|(i, _)| i)
            };
            Ok(result(StartupActionStatus::Done, None, after))
        }
        Err(error) => {
            let (status, reason) = platform_status(&error);
            Ok(result(status, reason, reread(&id, item.source).map(|(i, _)| i)))
        }
    }
}

fn set_enabled(id: &str, item: &StartupItem, fact: &StartupEntryFact, enabled: bool) -> Result<(), PlatformError> {
    match &fact.location {
        StartupLocation::Registry { .. } | StartupLocation::Folder { .. } => {
            let (root, list, name) = approved_target(fact).ok_or(PlatformError::Unsupported)?;
            // A delayed entry stops being delayed either way: remove our task first.
            let mut undo = load_undo();
            if let Some(delayed) = undo.delayed.get(id).cloned() {
                items::delete_task(&delayed.task_path)?;
                undo.delayed.remove(id);
                save_undo(&undo).map_err(core_to_platform)?;
            }
            items::set_approved(root, list, &name, enabled, now_ms())
        }
        StartupLocation::Task { path } => items::set_task_enabled(path, enabled),
        // The stored state is the switch itself; no undo record is needed.
        StartupLocation::Package { family, task_id } => items::set_packaged_task_enabled(family, task_id, enabled),
        StartupLocation::Service { name } => {
            let mut undo = load_undo();
            let current = fact.service_start.unwrap_or(ServiceStartKind::Automatic);
            if enabled {
                let target = undo
                    .services
                    .get(name)
                    .map(|u| u.previous)
                    .filter(|p| matches!(p, ServiceStartKind::Automatic | ServiceStartKind::AutomaticDelayed))
                    .unwrap_or(ServiceStartKind::Automatic);
                items::set_service_start(name, target)?;
                undo.services.remove(name);
            } else {
                if item.state == StartupState::Disabled {
                    return Ok(());
                }
                items::set_service_start(name, ServiceStartKind::Manual)?;
                undo.services
                    .entry(name.clone())
                    .or_insert(ServiceUndo { previous: current, changed_at_ms: now_ms() });
            }
            save_undo(&undo).map_err(core_to_platform)
        }
    }
}

fn core_to_platform(error: CoreError) -> PlatformError {
    match error {
        CoreError::PermissionDenied => PlatformError::PermissionDenied,
        _ => PlatformError::OperationFailed("undoStateWrite".into()),
    }
}

/// Executable + arguments the delayed task should run for an entry.
fn delayed_command(fact: &StartupEntryFact) -> Option<(String, String)> {
    match &fact.location {
        StartupLocation::Registry { .. } => {
            let expanded = items::expand_env_vars(fact.command.trim(), &|n| std::env::var(n).ok());
            let (exe, args) = split_command_line(&expanded, |p| Path::new(p).exists())?;
            let exe = fact
                .target
                .as_ref()
                .filter(|_| !exe.contains('\\'))
                .map(|t| t.display().to_string())
                .unwrap_or(exe);
            Some((exe, args))
        }
        // Explorer opens the shortcut with its own arguments and working folder.
        StartupLocation::Folder { file, .. } => {
            let root = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into());
            Some((format!("{root}\\explorer.exe"), format!("\"{}\"", file.display())))
        }
        _ => None,
    }
}

fn set_delayed(id: &str, item: &StartupItem, fact: &StartupEntryFact, delayed: bool) -> Result<(), PlatformError> {
    match &fact.location {
        StartupLocation::Service { name } => {
            let mut undo = load_undo();
            let current = fact.service_start.unwrap_or(ServiceStartKind::Automatic);
            let target = if delayed { ServiceStartKind::AutomaticDelayed } else { ServiceStartKind::Automatic };
            items::set_service_start(name, target)?;
            let record = undo
                .services
                .entry(name.clone())
                .or_insert(ServiceUndo { previous: current, changed_at_ms: now_ms() })
                .clone();
            if record.previous == target {
                undo.services.remove(name);
            }
            save_undo(&undo).map_err(core_to_platform)
        }
        StartupLocation::Registry { .. } | StartupLocation::Folder { .. } => {
            if !delayed {
                // "Start normally again": remove the task and re-enable the entry.
                return set_enabled(id, item, fact, true);
            }
            let (root, list, name) = approved_target(fact).ok_or(PlatformError::Unsupported)?;
            let (exe, args) = delayed_command(fact).ok_or(PlatformError::OperationFailed("commandUnresolved".into()))?;
            let user = items::current_user_id().ok_or(PlatformError::OperationFailed("userUnavailable".into()))?;
            let xml = items::delayed_logon_task_xml(&user, &exe, &args, DEFAULT_DELAY_SECONDS, "Delayed startup entry created by PC Toolkit Pro.");
            let task_path = items::register_user_logon_task(DELAYED_TASK_FOLDER, &task_safe_name(&item.name), &xml)
                .map_err(|e| match e {
                    PlatformError::OperationFailed(_) => PlatformError::OperationFailed("taskRegisterFailed".into()),
                    other => other,
                })?;
            if let Err(error) = items::set_approved(root, list, &name, false, now_ms()) {
                // Never leave the program starting twice.
                let _ = items::delete_task(&task_path);
                return Err(error);
            }
            let mut undo = load_undo();
            undo.delayed.insert(
                id.to_string(),
                DelayedUndo { task_path, delay_seconds: DEFAULT_DELAY_SECONDS, changed_at_ms: now_ms() },
            );
            save_undo(&undo).map_err(core_to_platform)
        }
        StartupLocation::Task { .. } | StartupLocation::Package { .. } => Err(PlatformError::Unsupported),
    }
}

fn backup_dir() -> Option<PathBuf> {
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S%3f");
    let dir = app_dir()?.join("startup-backups").join(stamp.to_string());
    fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct EntryBackup<'a> {
    version: u32,
    source: StartupSource,
    scope: StartupScope,
    entry_name: &'a str,
    command: &'a str,
    location: &'a StartupLocation,
    deleted_at_ms: i64,
}

/// Remove an entry whose target is missing, after writing a backup (no backup → no delete).
fn delete_broken(item: &StartupItem, fact: &StartupEntryFact) -> Result<(), PlatformError> {
    if !item.broken {
        return Err(PlatformError::Unsupported);
    }
    let dir = backup_dir().ok_or(PlatformError::OperationFailed("backupFailed".into()))?;
    let manifest = EntryBackup {
        version: 1,
        source: fact.source,
        scope: fact.scope,
        entry_name: &fact.entry_name,
        command: &fact.command,
        location: &fact.location,
        deleted_at_ms: now_ms(),
    };
    let json = serde_json::to_string_pretty(&manifest).map_err(|_| PlatformError::OperationFailed("backupFailed".into()))?;
    fs::write(dir.join("entry.json"), json).map_err(|_| PlatformError::OperationFailed("backupFailed".into()))?;
    match &fact.location {
        StartupLocation::Registry { key, value_name, approved } => {
            export_registry_key(key, &dir.join("run-key.reg"))
                .map_err(|_| PlatformError::OperationFailed("backupFailed".into()))?;
            pctoolkit_platform::delete_registry_value(key, value_name)?;
            if let Some(list) = approved {
                let _ = items::clear_approved(key.root, *list, value_name);
            }
            Ok(())
        }
        StartupLocation::Folder { file, approved } => {
            // The Recycle Bin is the backup of the file itself; entry.json records where it was.
            recycle_path(file)?;
            let root = if fact.scope == StartupScope::User { RegistryRoot::CurrentUser } else { RegistryRoot::LocalMachine };
            let _ = items::clear_approved(root, *approved, &fact.entry_name);
            Ok(())
        }
        StartupLocation::Task { path } => {
            let xml = items::task_xml(path).ok_or(PlatformError::OperationFailed("backupFailed".into()))?;
            fs::write(dir.join("task.xml"), xml).map_err(|_| PlatformError::OperationFailed("backupFailed".into()))?;
            items::delete_task(path)
        }
        StartupLocation::Service { .. } | StartupLocation::Package { .. } => Err(PlatformError::Unsupported),
    }
}

/// Path to reveal in Explorer for "Open file location" (the target, else the Startup folder file).
pub fn startup_item_reveal_path(id: &str) -> CoreResult<PathBuf> {
    let (_, fact) = cached(id).ok_or_else(|| CoreError::InvalidInput("unknownItem".into()))?;
    if let Some(target) = fact.target.as_ref().filter(|t| t.exists()) {
        return Ok(target.clone());
    }
    if let StartupLocation::Folder { file, .. } = &fact.location {
        return Ok(file.clone());
    }
    Err(CoreError::OperationFailed("targetMissing".into()))
}

/// Package logos larger than this are not inlined (the executable icon is used instead).
const MAX_LOGO_BYTES: u64 = 256 * 1024;

/// Data URL of a packaged app's logo image (PNG / JPEG only).
fn package_logo_data_url(path: &Path) -> Option<String> {
    let ext = path.extension()?.to_string_lossy().to_ascii_lowercase();
    let mime = match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        _ => return None,
    };
    if fs::metadata(path).ok()?.len() > MAX_LOGO_BYTES {
        return None;
    }
    let bytes = fs::read(path).ok()?;
    Some(format!("data:{mime};base64,{}", base64(&bytes)))
}

pub fn get_startup_icons(ids: &[String]) -> Vec<StartupIcon> {
    let targets: Vec<(String, Option<PathBuf>, Option<PathBuf>)> = {
        let cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
        ids.iter()
            .take(64)
            .map(|id| {
                let record = cache.iter().find(|r| &r.item.id == id);
                let target = record.and_then(|r| r.fact.target.clone());
                let logo = record.and_then(|r| r.fact.package.as_ref()).and_then(|p| p.logo_path.clone());
                (id.clone(), target, logo)
            })
            .collect()
    };
    targets
        .into_iter()
        .map(|(id, target, logo)| {
            if let Some(url) = logo.as_deref().and_then(package_logo_data_url) {
                return StartupIcon { id, data_url: Some(url) };
            }
            let data_url = target.and_then(|path| {
                // Check, release, extract, insert: icon extraction can be slow and must not
                // serialize every other icon request behind the cache lock.
                {
                    let guard = ICON_CACHE.lock().unwrap_or_else(|e| e.into_inner());
                    if let Some(found) = guard.as_ref().and_then(|cache| cache.get(&path)) {
                        return found.clone();
                    }
                }
                let url = path
                    .exists()
                    .then(|| icon_png_from_file(&path, 0, 32))
                    .flatten()
                    .map(|png| format!("data:image/png;base64,{}", base64(&png)));
                let mut guard = ICON_CACHE.lock().unwrap_or_else(|e| e.into_inner());
                guard.get_or_insert_with(HashMap::new).insert(path, url.clone());
                url
            });
            StartupIcon { id, data_url }
        })
        .collect()
}

fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(TABLE[(n >> 18) as usize & 63] as char);
        out.push(TABLE[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { TABLE[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { TABLE[n as usize & 63] as char } else { '=' });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use pctoolkit_platform::startup_items::{PackagedTaskFacts, PackagedTaskState};
    use pctoolkit_platform::RegistryKeyPath;

    fn ids(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn first_snapshot_is_a_baseline_without_new_items() {
        let (snapshot, fresh) = merge_known(None, &ids(&["run:cu:64:A", "svc:B"]), 1_000);
        assert!(fresh.is_empty());
        assert_eq!(snapshot.baseline_at_ms, 1_000);
        assert_eq!(snapshot.items.len(), 2);
    }

    #[test]
    fn items_added_after_baseline_are_new_until_the_window_ends() {
        let (base, _) = merge_known(None, &ids(&["a"]), 1_000);
        let (next, fresh) = merge_known(Some(base), &ids(&["a", "b"]), 2_000);
        assert_eq!(fresh, HashSet::from(["b".to_string()]));
        assert_eq!(next.items["b"], 2_000);
        // Still new on the next listing.
        let (later, fresh) = merge_known(Some(next), &ids(&["a", "b"]), 3_000);
        assert!(fresh.contains("b"));
        // Gone after the window.
        let (_, fresh) = merge_known(Some(later), &ids(&["a", "b"]), 2_000 + NEW_ITEM_WINDOW_MS);
        assert!(fresh.is_empty());
    }

    #[test]
    fn removed_items_are_forgotten_and_unknown_versions_rebuild() {
        let (base, _) = merge_known(None, &ids(&["a", "b"]), 1_000);
        let (next, _) = merge_known(Some(base), &ids(&["a"]), 2_000);
        assert!(!next.items.contains_key("b"));
        let bad = KnownSnapshot { version: 9, ..Default::default() };
        let (rebuilt, fresh) = merge_known(Some(bad), &ids(&["a"]), 5_000);
        assert!(fresh.is_empty());
        assert_eq!(rebuilt.baseline_at_ms, 5_000);
    }

    #[test]
    fn broken_only_for_missing_files_on_present_drives() {
        let present = |p: &Path| {
            let s = p.to_string_lossy().to_ascii_lowercase();
            s == "c:\\" || s == "c:\\app\\ok.exe"
        };
        assert!(is_broken_target(Path::new("C:\\Gone\\old.exe"), &present));
        assert!(!is_broken_target(Path::new("C:\\App\\ok.exe"), &present));
        // Drive not present (unplugged USB) → unknown, not broken.
        assert!(!is_broken_target(Path::new("E:\\Tools\\x.exe"), &present));
        // Network and relative paths are never broken.
        assert!(!is_broken_target(Path::new("\\\\server\\share\\x.exe"), &present));
        assert!(!is_broken_target(Path::new("x.exe"), &present));
    }

    #[test]
    fn impact_thresholds() {
        assert_eq!(impact_from_ms(1_000), StartupImpact::High);
        assert_eq!(impact_from_ms(999), StartupImpact::Medium);
        assert_eq!(impact_from_ms(300), StartupImpact::Medium);
        assert_eq!(impact_from_ms(10), StartupImpact::Low);
    }

    #[test]
    fn boot_summary_takes_newest_boot_and_worst_recent_degradation() {
        let now = parse_event_time("2026-11-24T00:00:00Z").unwrap();
        let events = vec![
            BootPerfEvent::Boot { system_time: "2026-11-23T08:00:00.1234567Z".into(), boot_ms: 48_000, main_path_ms: None },
            BootPerfEvent::Degradation {
                system_time: "2026-11-23T08:00:00Z".into(),
                event_id: 101,
                file_name: "App.exe".into(),
                friendly_name: "Vendor App".into(),
                total_ms: 2_000,
                degradation_ms: 700,
            },
            BootPerfEvent::Boot { system_time: "2026-11-20T08:00:00Z".into(), boot_ms: 60_000, main_path_ms: None },
            BootPerfEvent::Degradation {
                system_time: "2026-11-20T08:00:00Z".into(),
                event_id: 101,
                file_name: "app.exe".into(),
                friendly_name: String::new(),
                total_ms: 3_000,
                degradation_ms: 1_200,
            },
            BootPerfEvent::Degradation {
                system_time: "2026-01-01T08:00:00Z".into(),
                event_id: 101,
                file_name: "old.exe".into(),
                friendly_name: String::new(),
                total_ms: 9_000,
                degradation_ms: 9_000,
            },
        ];
        let data = summarize_boot(BootPerfAccess::Available, &events, now);
        assert_eq!(data.summary.last_boot_ms, Some(48_000));
        assert_eq!(data.summary.boots_recorded, 2);
        assert!(data.summary.last_boot_at_ms.is_some());
        assert_eq!(data.degradations.get("app.exe"), Some(&1_200));
        assert_eq!(data.degradations.get("vendor app"), Some(&700));
        assert!(!data.degradations.contains_key("old.exe"));
    }

    fn run_fact(name: &str, target: Option<&str>, root: RegistryRoot) -> StartupEntryFact {
        StartupEntryFact {
            source: StartupSource::Run,
            scope: if root == RegistryRoot::CurrentUser { StartupScope::User } else { StartupScope::Machine },
            entry_name: name.into(),
            label: name.into(),
            command: target.unwrap_or("").into(),
            target: target.map(PathBuf::from),
            location: StartupLocation::Registry {
                key: RegistryKeyPath::new(root, RegistryView::Native64, "Software\\Microsoft\\Windows\\CurrentVersion\\Run"),
                value_name: name.into(),
                approved: Some(ApprovedList::Run),
            },
            enabled: true,
            disabled_at_ms: None,
            service_start: None,
            microsoft_task: false,
            task_privileged: false,
            package: None,
        }
    }

    fn packaged_fact(family: &str, task_id: &str, state: PackagedTaskState, publisher: &str) -> StartupEntryFact {
        StartupEntryFact {
            source: StartupSource::PackagedApp,
            scope: StartupScope::User,
            entry_name: task_id.into(),
            label: "Vendor App".into(),
            command: "app\\Missing.exe".into(),
            target: Some(PathBuf::from("C:\\Definitely\\Missing\\Package\\app\\Missing.exe")),
            location: StartupLocation::Package { family: family.into(), task_id: task_id.into() },
            enabled: state.is_enabled(),
            disabled_at_ms: None,
            service_start: None,
            microsoft_task: false,
            task_privileged: false,
            package: Some(PackagedTaskFacts {
                family_name: family.into(),
                full_name: format!("{family}_1.0.0.0_x64__"),
                task_id: task_id.into(),
                task_name: Some("VendorTray".into()),
                publisher: publisher.into(),
                is_system: false,
                logo_path: None,
                state,
            }),
        }
    }

    #[test]
    fn packaged_ids_round_trip_family_and_task() {
        let fact = packaged_fact("Vendor.App_abc123", "Tray.Start", PackagedTaskState::Enabled, "Vendor");
        let id = item_id(&fact);
        assert_eq!(id, "pkg:Vendor.App_abc123:Tray.Start");
        let (family, task) = id.strip_prefix("pkg:").and_then(|rest| rest.split_once(':')).unwrap();
        assert_eq!(
            StartupLocation::Package { family: family.into(), task_id: task.into() },
            fact.location,
            "family names never contain ':', so the id splits back into its location"
        );
    }

    #[test]
    fn packaged_records_toggle_per_user_and_never_delay_or_delete() {
        let boot = summarize_boot(BootPerfAccess::NeedsAdmin, &[], 0);
        let undo = UndoFile { version: 1, ..Default::default() };
        let facts = vec![
            packaged_fact("Vendor.App_a", "Tray", PackagedTaskState::Enabled, "Vendor Inc."),
            packaged_fact("Vendor.App_b", "Tray", PackagedTaskState::DisabledByUser, "Vendor Inc."),
            packaged_fact("Vendor.App_c", "Tray", PackagedTaskState::EnabledByPolicy, "Vendor Inc."),
            packaged_fact("Microsoft.YourPhone_x", "Start", PackagedTaskState::Enabled, "Microsoft Corporation"),
        ];
        let records = build_records(facts, &undo, &boot);
        assert_eq!(records.len(), 4);
        let enabled = &records[0].item;
        assert_eq!(enabled.state, StartupState::Enabled);
        assert_eq!(enabled.publisher.as_deref(), Some("Vendor Inc."));
        assert_eq!(enabled.description.as_deref(), Some("VendorTray"));
        assert!(enabled.can_toggle && !enabled.requires_admin);
        assert!(!enabled.can_delay && !enabled.can_delete && !enabled.broken, "missing package files are not broken entries");
        assert_eq!(records[1].item.state, StartupState::Disabled);
        let policy = &records[2].item;
        assert_eq!(policy.state, StartupState::Enabled);
        assert!(!policy.can_toggle, "policy-controlled tasks are read-only");
        let microsoft = &records[3].item;
        assert!(microsoft.is_microsoft);
        assert!(microsoft.can_toggle, "Settings lets users switch Microsoft packaged apps too");
        assert!(visible(microsoft, false), "only Microsoft tasks / services are hidden by default");
    }

    #[test]
    fn ids_are_stable_and_source_specific() {
        let fact = run_fact("Vendor", None, RegistryRoot::CurrentUser);
        assert_eq!(item_id(&fact), "run:cu:64:Vendor");
        let mut service = run_fact("svc", None, RegistryRoot::LocalMachine);
        service.source = StartupSource::Service;
        service.location = StartupLocation::Service { name: "VendorSvc".into() };
        assert_eq!(item_id(&service), "svc:VendorSvc");
    }

    #[test]
    fn records_apply_admin_delay_and_microsoft_policy() {
        let boot = summarize_boot(BootPerfAccess::NeedsAdmin, &[], 0);
        let undo = UndoFile { version: 1, ..Default::default() };
        let user = run_fact("UserApp", Some("C:\\Definitely\\Missing\\user.exe"), RegistryRoot::CurrentUser);
        let machine = run_fact("MachineApp", None, RegistryRoot::LocalMachine);
        let mut ms_service = run_fact("ms", Some("C:\\Windows\\System32\\svchost.exe"), RegistryRoot::LocalMachine);
        ms_service.source = StartupSource::Service;
        ms_service.microsoft_task = true; // stand-in for a Microsoft publisher
        ms_service.service_start = Some(ServiceStartKind::Automatic);
        ms_service.location = StartupLocation::Service { name: "MsSvc".into() };
        let manual = {
            let mut s = ms_service.clone();
            s.microsoft_task = false;
            s.service_start = Some(ServiceStartKind::Manual);
            s.location = StartupLocation::Service { name: "Untracked".into() };
            s
        };
        let records = build_records(vec![user, machine, ms_service, manual], &undo, &boot);
        assert_eq!(records.len(), 3, "manual services we did not change are not startup items");
        let user = &records[0].item;
        assert!(!user.requires_admin);
        assert_eq!(user.impact, StartupImpact::Unknown);
        let machine = &records[1].item;
        assert!(machine.requires_admin);
        assert!(!machine.can_delay, "delay is offered for per-user entries only");
        let service = &records[2].item;
        assert!(service.is_microsoft);
        assert!(!service.can_toggle, "Microsoft services are never changed");
    }

    /// Manual read-only smoke check (no state files written): `cargo test -p pctoolkit-core
    /// startup_records_smoke -- --ignored --nocapture`. Prints counts only.
    #[cfg(windows)]
    #[test]
    #[ignore]
    fn startup_records_smoke() {
        let boot = boot_data(true);
        let undo = UndoFile { version: 1, ..Default::default() };
        let records = build_records(collect_facts(&ALL_SOURCES), &undo, &boot);
        for source in ALL_SOURCES {
            let of: Vec<_> = records.iter().map(|r| &r.item).filter(|i| i.source == source).collect();
            println!(
                "{source:?}: items={} startsWithWindows={} microsoft={} thirdParty={} broken={} toggle={} delay={} adminToChange={} withPublisher={}",
                of.len(),
                of.iter().filter(|i| i.state != StartupState::Disabled).count(),
                of.iter().filter(|i| i.is_microsoft).count(),
                of.iter().filter(|i| !i.is_microsoft).count(),
                of.iter().filter(|i| i.broken).count(),
                of.iter().filter(|i| i.can_toggle).count(),
                of.iter().filter(|i| i.can_delay).count(),
                of.iter().filter(|i| i.requires_admin).count(),
                of.iter().filter(|i| i.publisher.is_some()).count(),
            );
        }
        let items: Vec<StartupItem> = records.into_iter().map(|r| r.item).collect();
        let care = crate::care::categories::startup::care_items(&items, is_user_admin());
        println!("boot access={:?} lastBootMs={:?}; care findings={}", boot.summary.access, boot.summary.last_boot_ms, care.len());
    }

    #[test]
    fn task_names_are_sanitized() {
        assert_eq!(task_safe_name("A/B:C*"), "A_B_C_");
        assert_eq!(task_safe_name("  ..  "), "Startup entry");
    }

    #[test]
    fn base64_matches_known_vectors() {
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64(b"fo"), "Zm8=");
    }
}
