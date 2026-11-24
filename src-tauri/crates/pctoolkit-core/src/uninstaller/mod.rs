//! Uninstaller domain: list installed programs, run their own uninstaller, then find and remove
//! what they left behind (files, registry, shortcuts, startup entries, services, tasks, firewall
//! rules).
//!
//! Safety invariants:
//! - Leftover candidates are resolved by id from a scan Core produced; the frontend never sends
//!   paths or registry keys to delete.
//! - Files and folders go to the Recycle Bin. Registry keys/values are exported to a `.reg`
//!   backup first; if the backup fails the key is not deleted.
//! - Items that need administrator rights are skipped with a typed status when not elevated.

mod icons;
mod inventory;
mod leftovers;
mod matching;
mod snapshot;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use pctoolkit_platform::{
    create_restore_point, delete_on_reboot, focus_uninstaller_window, delete_registry_tree, delete_registry_value,
    delete_scheduled_task, delete_service, export_registry_key, is_user_admin,
    list_store_packages, list_uninstall_entries, path_exists, recycle_path, registry_key_exists,
    remove_store_package, run_uninstaller, split_command_line, PlatformError, RestorePointOutcome,
    UninstallerRunOutcome, UninstallerTick, WOULD_DELETE_PERMANENTLY,
};
use serde::{Deserialize, Serialize};

use crate::history::{append_history, history_now_ms, HistoryOutcome, HistoryWrite};
use crate::shared::{CoreError, CoreResult};

pub use icons::ProgramIcon;
pub use inventory::{InstallerKind, InstalledProgram, ProgramSource};
pub use leftovers::{LeftoverConfidence, LeftoverItem, LeftoverKind};
pub use snapshot::SnapshotProgram as RemovedProgram;

use inventory::{
    desktop_records, record_from_snapshot, store_records, uninstall_command, ProgramRecord, UninstallEntryFacts,
};
use leftovers::{
    admin_roots_from_environment, scan_leftovers, Leftover, LeftoverAction, OtherPrograms,
    ProgramSnapshot, ScanContext,
};
use matching::{compact, is_distinctive, NameSet, ProtectedRoots};

static BUSY: AtomicBool = AtomicBool::new(false);
static CANCELLED: AtomicBool = AtomicBool::new(false);
static PROGRAMS: Mutex<Vec<ProgramRecord>> = Mutex::new(Vec::new());
/// Recent leftover scans, oldest first (clean requests resolve item ids against these).
static SCANS: Mutex<Vec<(String, (String, Vec<Leftover>))>> = Mutex::new(Vec::new());
/// Each batch uninstall creates at most a handful of scans.
const MAX_SCANS: usize = 16;
/// Every raw uninstall entry of the last listing, including the ones not shown (system
/// components, child entries, updates, duplicates): what they own stays protected.
static ALL_ENTRIES: Mutex<Vec<UninstallEntryFacts>> = Mutex::new(Vec::new());

/// Remember a scan, evicting only the oldest ones beyond `cap`.
fn remember_scan<T>(scans: &mut Vec<(String, T)>, id: String, found: T, cap: usize) {
    scans.push((id, found));
    if scans.len() > cap {
        let excess = scans.len() - cap;
        scans.drain(..excess);
    }
}

struct BusyGuard;

impl BusyGuard {
    fn acquire() -> CoreResult<Self> {
        if BUSY
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Err(CoreError::OperationBusy);
        }
        CANCELLED.store(false, Ordering::SeqCst);
        Ok(Self)
    }
}

impl Drop for BusyGuard {
    fn drop(&mut self) {
        BUSY.store(false, Ordering::SeqCst);
    }
}

fn cancelled() -> bool {
    CANCELLED.load(Ordering::SeqCst)
}

pub fn cancel_uninstaller() {
    CANCELLED.store(true, Ordering::SeqCst);
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UninstallerProgress {
    /// listing | restorePoint | uninstalling | scanning | cleaning
    pub phase: String,
    pub program_name: String,
    /// Scanner area code while scanning (folders, registry, shortcuts, startup, services, tasks, firewall).
    pub area: Option<String>,
    pub current: u32,
    pub total: u32,
    pub items_found: u32,
    pub elapsed_ms: u64,
    /// Path / key being scanned or removed right now (shown in the progress dialog).
    pub current_path: Option<String>,
    /// While uninstalling: the vendor uninstaller has a visible window (it may need the user).
    pub window_visible: bool,
    /// While cleaning: id and outcome of the item that just finished.
    pub item_id: Option<String>,
    pub item_status: Option<LeftoverItemStatus>,
    /// While listing: PNG data URL of the program being read (only on events actually sent).
    pub icon: Option<String>,
}

impl UninstallerProgress {
    fn new(phase: &str, program_name: &str, started: Instant) -> Self {
        Self {
            phase: phase.into(),
            program_name: program_name.into(),
            area: None,
            current: 0,
            total: 0,
            items_found: 0,
            elapsed_ms: started.elapsed().as_millis() as u64,
            current_path: None,
            window_visible: false,
            item_id: None,
            item_status: None,
            icon: None,
        }
    }
}

/// Icons for the listed programs (cached; safe to call while another operation runs).
pub fn get_program_icons(ids: &[String]) -> Vec<ProgramIcon> {
    let records = PROGRAMS.lock().unwrap_or_else(|e| e.into_inner()).clone();
    icons::program_icons(&records, ids)
}

/// Bring the running vendor uninstaller's window to the front.
pub fn focus_uninstaller() -> bool {
    focus_uninstaller_window()
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgramInventory {
    pub programs: Vec<InstalledProgram>,
    pub is_admin: bool,
    /// Store apps could not be listed (WinRT unavailable); desktop programs are still complete.
    pub store_unavailable: bool,
    /// Desktop programs that disappeared since the last listing without PC Toolkit Pro removing
    /// them (another uninstaller ran); their leftovers can still be scanned.
    pub removed_elsewhere: Vec<RemovedProgram>,
}

/// Lists installed programs. `on_progress` receives `listing` events: `area` is `registry`
/// (reading uninstall keys), `desktop` (one event per program, `program_name` = the program being
/// read, `current`/`total`, `items_found` = programs listed so far) or `store` (Store packages).
pub fn list_programs(mut on_progress: impl FnMut(UninstallerProgress)) -> CoreResult<ProgramInventory> {
    let _busy = BusyGuard::acquire()?;
    let started = Instant::now();
    let mut last_emit: Option<Instant> = None;
    let mut emit = |area: &str, record: Option<&ProgramRecord>, current: usize, total: usize, found: usize, force: bool| {
        // Throttle per-program events; a fast registry walk would otherwise flood the IPC channel.
        if !force && last_emit.is_some_and(|t| t.elapsed() < Duration::from_millis(30)) {
            return;
        }
        let name = record.map(|r| r.program.name.as_str()).unwrap_or("");
        let mut p = UninstallerProgress::new("listing", name, started);
        p.area = Some(area.into());
        p.current = current as u32;
        p.total = total as u32;
        p.items_found = found as u32;
        if let Some(record) = record {
            // Only programs shown in the feed pay for icon extraction; the result is cached.
            p.item_id = Some(record.program.id.clone());
            p.icon = icons::cached_icon(record);
        }
        on_progress(p);
        last_emit = Some(Instant::now());
    };
    emit("registry", None, 0, 0, 0, true);
    let protected = ProtectedRoots::from_environment();
    let entries = list_uninstall_entries();
    let facts: Vec<UninstallEntryFacts> =
        entries.iter().map(|e| UninstallEntryFacts::from_entry(e, &protected)).collect();
    *ALL_ENTRIES.lock().unwrap_or_else(|e| e.into_inner()) = facts;
    let mut found = 0usize;
    let mut records = desktop_records(entries, &protected, &mut |index, total, record| {
        found += 1;
        emit("desktop", Some(record), index + 1, total, found, index + 1 == total)
    });
    let desktop_count = records.len();
    emit("store", None, desktop_count, desktop_count, desktop_count, true);
    let (store, store_unavailable) = match list_store_packages() {
        Ok(packages) => (store_records(packages), false),
        Err(_) => (Vec::new(), true),
    };
    let found = desktop_count + store.len();
    emit("done", None, found, found, found, true);
    let current: Vec<RemovedProgram> = records
        .iter()
        .map(|r| RemovedProgram {
            id: r.program.id.clone(),
            name: r.program.name.clone(),
            publisher: r.program.publisher.clone(),
            version: r.program.version.clone(),
            key_name: r.entry.as_ref().map(|e| e.key_name.clone()).unwrap_or_default(),
            install_location: r.program.install_location.clone(),
            removed_at_ms: 0,
        })
        .collect();
    let removed_elsewhere = snapshot::update(current, history_now_ms());
    records.extend(store);
    records.sort_by(|a, b| a.program.name.to_lowercase().cmp(&b.program.name.to_lowercase()));
    let programs = records.iter().map(|r| r.program.clone()).collect();
    *PROGRAMS.lock().unwrap_or_else(|e| e.into_inner()) = records;
    Ok(ProgramInventory {
        programs,
        is_admin: is_user_admin(),
        store_unavailable,
        removed_elsewhere,
    })
}

fn find_record(program_id: &str) -> CoreResult<(ProgramRecord, OtherPrograms)> {
    let guard = PROGRAMS.lock().unwrap_or_else(|e| e.into_inner());
    let record = guard
        .iter()
        .find(|r| r.program.id == program_id)
        .cloned()
        .or_else(|| {
            snapshot::find_removed(program_id)
                .map(|p| record_from_snapshot(&p, &ProtectedRoots::from_environment()))
        })
        .ok_or_else(|| CoreError::InvalidInput("unknownProgram".into()))?;
    let protected = ProtectedRoots::from_environment();
    let mut others = OtherPrograms::default();
    let mut add_other = |name: &str, key_name: &str, install_location: &str, publisher: &str, dir: Option<&PathBuf>| {
        if let Some(dir) = dir {
            others.install_dirs.push(protected.normalize_root(dir));
        }
        let publisher = compact(publisher);
        if is_distinctive(&publisher) {
            others.publishers.insert(publisher);
        }
        others.names.push(NameSet::build(name, key_name, install_location));
    };
    for other in guard.iter().filter(|r| r.program.id != program_id) {
        add_other(
            &other.program.name,
            other.entry.as_ref().map(|e| e.key_name.as_str()).unwrap_or_default(),
            &other.program.install_location,
            &other.program.publisher,
            other.install_dir.as_ref(),
        );
    }
    drop(guard);
    // Hidden / child / duplicate uninstall entries are not listed but may still own folders and
    // names; everything except the target's own entries is treated as another program.
    let target_entry = record.entry.clone().unwrap_or_default();
    let has_target_entry = record.entry.is_some();
    for facts in ALL_ENTRIES.lock().unwrap_or_else(|e| e.into_inner()).iter() {
        if has_target_entry && facts.belongs_to(&target_entry) {
            continue;
        }
        add_other(
            &facts.display_name,
            &facts.key_name,
            &facts.install_location,
            &facts.publisher,
            facts.install_dir.as_ref(),
        );
    }
    Ok((record, others))
}

/// Executables the program owns, from its uninstall entry hints and its install folder. Called
/// before the uninstaller runs so the paths are known after the folder is gone.
fn program_exes(record: &ProgramRecord) -> Vec<PathBuf> {
    let mut hints = Vec::new();
    if let Some(entry) = record.entry.as_ref() {
        if let Some((icon, _)) = icons::parse_display_icon(&entry.display_icon) {
            hints.push(icon);
        }
        for command in [&entry.uninstall_string, &entry.quiet_uninstall_string] {
            if let Some(exe) = matching::first_path(command) {
                hints.push(exe);
            }
        }
    }
    leftovers::collect_program_exes(record.install_dir.as_deref(), &hints)
}

/// "Name 1.2.3" for history summaries (version only when the name does not already end with it).
fn display_name(record: &ProgramRecord) -> String {
    let name = record.program.name.trim();
    let version = record.program.version.trim();
    if version.is_empty() || name.ends_with(version) {
        name.to_string()
    } else {
        format!("{name} {version}")
    }
}

fn snapshot(record: &ProgramRecord, exe_paths: Vec<PathBuf>) -> ProgramSnapshot {
    ProgramSnapshot {
        names: NameSet::build(
            &record.program.name,
            record.entry.as_ref().map(|e| e.key_name.as_str()).unwrap_or_default(),
            &record.program.install_location,
        ),
        publisher: record.program.publisher.clone(),
        install_dir: record.install_dir.clone(),
        uninstall_key: record.entry.as_ref().and_then(|e| e.key.clone()),
        store_family_name: record.store_family_name.clone(),
        exe_paths,
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LeftoverScan {
    pub scan_id: String,
    pub program_id: String,
    pub program_name: String,
    pub items: Vec<LeftoverItem>,
    pub total_bytes: u64,
    pub is_admin: bool,
}

fn run_scan<F>(
    record: &ProgramRecord,
    others: &OtherPrograms,
    exe_paths: Vec<PathBuf>,
    on_progress: &mut F,
    started: Instant,
) -> LeftoverScan
where
    F: FnMut(UninstallerProgress),
{
    let program = snapshot(record, exe_paths);
    let protected = ProtectedRoots::from_environment();
    let ctx = ScanContext {
        program: &program,
        others,
        protected: &protected,
        admin_roots: admin_roots_from_environment(),
    };
    let name = record.program.name.clone();
    let found = scan_leftovers(&ctx, &mut |area, count, last| {
        let mut p = UninstallerProgress::new("scanning", &name, started);
        p.area = Some(area.into());
        p.items_found = count as u32;
        p.current_path = last.map(str::to_string);
        on_progress(p);
    });
    let scan_id = uuid::Uuid::new_v4().to_string();
    let items: Vec<LeftoverItem> = found.iter().map(|l| l.item.clone()).collect();
    let total_bytes = items.iter().map(|i| i.bytes).sum();
    remember_scan(
        &mut SCANS.lock().unwrap_or_else(|e| e.into_inner()),
        scan_id.clone(),
        (display_name(record), found),
        MAX_SCANS,
    );
    LeftoverScan {
        scan_id,
        program_id: record.program.id.clone(),
        program_name: record.program.name.clone(),
        items,
        total_bytes,
        is_admin: is_user_admin(),
    }
}

/// Per-user data folders of a still-installed program, shown in the uninstall confirmation.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelatedDataPreview {
    pub program_id: String,
    pub items: Vec<LeftoverItem>,
    pub total_bytes: u64,
}

/// Read-only preview of the program's data folders (app data and `~\.<product>` folders). It
/// takes no busy lock so it can run while the confirmation dialog is open; nothing is remembered
/// for cleaning, the post-uninstall leftover scan stays the only source of clean ids.
pub fn preview_related_data(program_id: &str) -> CoreResult<RelatedDataPreview> {
    let (record, others) = find_record(program_id)?;
    let program = snapshot(&record, Vec::new());
    let protected = ProtectedRoots::from_environment();
    let ctx = ScanContext {
        program: &program,
        others: &others,
        protected: &protected,
        admin_roots: admin_roots_from_environment(),
    };
    let items = leftovers::scan_related_data(&ctx);
    let total_bytes = items.iter().map(|i| i.bytes).sum();
    Ok(RelatedDataPreview {
        program_id: record.program.id.clone(),
        items,
        total_bytes,
    })
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UninstallRequest {
    pub program_id: String,
    /// Use the installer framework's unattended mode when it has one.
    pub quiet: bool,
    pub create_restore_point: bool,
    pub scan_leftovers: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum UninstallStatus {
    Completed,
    CompletedRebootRequired,
    /// The uninstaller finished but the program is still registered (wizard cancelled or failed).
    StillInstalled,
    /// The user stopped waiting; the uninstaller may still be running.
    Cancelled,
    /// The user declined the UAC prompt.
    LaunchDenied,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RestorePointStatus {
    NotRequested,
    Created,
    NotCreated,
    SkippedNeedAdmin,
    Failed,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UninstallReport {
    pub program_id: String,
    pub program_name: String,
    pub status: UninstallStatus,
    pub exit_code: Option<u32>,
    pub restore_point: RestorePointStatus,
    pub leftovers: Option<LeftoverScan>,
}

fn still_installed(record: &ProgramRecord) -> bool {
    if let Some(key) = record.entry.as_ref().and_then(|e| e.key.as_ref()) {
        return registry_key_exists(key);
    }
    if let Some(full_name) = &record.store_full_name {
        return list_store_packages()
            .map(|p| p.iter().any(|p| &p.full_name == full_name))
            .unwrap_or(false);
    }
    false
}

pub fn uninstall_program<F>(request: UninstallRequest, mut on_progress: F) -> CoreResult<UninstallReport>
where
    F: FnMut(UninstallerProgress),
{
    let _busy = BusyGuard::acquire()?;
    let started = Instant::now();
    let started_ms = history_now_ms();
    let (record, others) = find_record(&request.program_id)?;
    // Record the program's executables now: the uninstaller removes its folder.
    let exe_paths = program_exes(&record);
    let name = record.program.name.clone();
    let progress = |phase: &str, started: Instant| UninstallerProgress::new(phase, &name, started);

    let restore_point = if request.create_restore_point {
        on_progress(progress("restorePoint", started));
        match create_restore_point(&format!("Before uninstalling {name}")) {
            Ok(RestorePointOutcome::Created) => RestorePointStatus::Created,
            Ok(RestorePointOutcome::NotCreated) => RestorePointStatus::NotCreated,
            Err(PlatformError::PermissionDenied) => RestorePointStatus::SkippedNeedAdmin,
            Err(_) => RestorePointStatus::Failed,
        }
    } else {
        RestorePointStatus::NotRequested
    };

    on_progress(progress("uninstalling", started));
    let (mut status, exit_code) = if let Some(full_name) = &record.store_full_name {
        match remove_store_package(full_name) {
            Ok(()) => (UninstallStatus::Completed, None),
            Err(PlatformError::PermissionDenied) => (UninstallStatus::LaunchDenied, None),
            Err(_) => (UninstallStatus::Failed, None),
        }
    } else {
        let entry = record
            .entry
            .as_ref()
            .ok_or_else(|| CoreError::InvalidInput("missingUninstallEntry".into()))?;
        let command = uninstall_command(entry, record.program.installer_kind, request.quiet)
            .ok_or_else(|| CoreError::InvalidInput("missingUninstallCommand".into()))?;
        let (exe, args) = split_command_line(&command, path_exists)
            .ok_or_else(|| CoreError::InvalidInput("invalidUninstallCommand".into()))?;
        let mut tick = |tick: UninstallerTick| {
            let mut p = progress("uninstalling", started);
            p.elapsed_ms = tick.elapsed.as_millis() as u64;
            p.window_visible = tick.window_visible;
            p.current = tick.process_count;
            on_progress(p);
        };
        match run_uninstaller(&exe, &args, &cancelled, &mut tick) {
            Ok(UninstallerRunOutcome::Finished { exit_code }) => {
                // MSI: 3010 / 1641 = success, restart required.
                let status = if exit_code == 3010 || exit_code == 1641 {
                    UninstallStatus::CompletedRebootRequired
                } else {
                    UninstallStatus::Completed
                };
                (status, Some(exit_code))
            }
            Ok(UninstallerRunOutcome::StoppedWaiting) => (UninstallStatus::Cancelled, None),
            Err(PlatformError::PermissionDenied) => (UninstallStatus::LaunchDenied, None),
            Err(_) => (UninstallStatus::Failed, None),
        }
    };

    if matches!(status, UninstallStatus::Completed | UninstallStatus::CompletedRebootRequired)
        && still_installed(&record)
    {
        status = UninstallStatus::StillInstalled;
    }

    let removed = matches!(status, UninstallStatus::Completed | UninstallStatus::CompletedRebootRequired);
    let leftovers = (removed && request.scan_leftovers && !cancelled())
        .then(|| run_scan(&record, &others, exe_paths, &mut on_progress, started));

    if removed {
        // Removed by us: never report it as "removed by another uninstaller".
        snapshot::forget(&record.program.id);
        PROGRAMS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .retain(|r| r.program.id != record.program.id);
    }

    append_history(HistoryWrite {
        category: "uninstaller".into(),
        title_key: "history.titles.uninstaller".into(),
        summary: format!(
            "{name} · {}",
            match status {
                UninstallStatus::Completed => "uninstalled",
                UninstallStatus::CompletedRebootRequired => "uninstalled, restart required",
                UninstallStatus::StillInstalled => "still installed",
                UninstallStatus::Cancelled => "cancelled",
                UninstallStatus::LaunchDenied => "permission declined",
                UninstallStatus::Failed => "failed",
            }
        ),
        started_at_ms: started_ms,
        finished_at_ms: history_now_ms(),
        outcome: match status {
            UninstallStatus::Completed => HistoryOutcome::Completed,
            UninstallStatus::CompletedRebootRequired => HistoryOutcome::CompletedWithWarnings,
            UninstallStatus::Cancelled => HistoryOutcome::Cancelled,
            _ => HistoryOutcome::Failed,
        },
        // The program's size (when known) reads better in History than "1 → 1".
        planned_bytes: (record.program.size_bytes > 0).then_some(record.program.size_bytes),
        result_bytes: (record.program.size_bytes > 0).then_some(if removed { record.program.size_bytes } else { 0 }),
        selected_item_count: 1,
        affected_item_count: u32::from(removed),
        failed_item_count: u32::from(!removed),
        detail_lines: leftovers
            .as_ref()
            .map(|l| vec![format!("{} leftover items found", l.items.len())])
            .unwrap_or_default(),
        action: "uninstall_program".into(),
        detail: None,
    });

    Ok(UninstallReport {
        program_id: record.program.id.clone(),
        program_name: name.clone(),
        status,
        exit_code,
        restore_point,
        leftovers,
    })
}

/// Stop offering leftover scans for a program removed outside the app.
pub fn dismiss_removed_program(program_id: &str) {
    snapshot::forget(program_id);
}

/// Forced removal / leftover check without running the uninstaller. The program's own folder and
/// uninstall entry are included, so this also removes programs whose uninstaller is broken.
pub fn scan_program_leftovers<F>(program_id: &str, mut on_progress: F) -> CoreResult<LeftoverScan>
where
    F: FnMut(UninstallerProgress),
{
    let _busy = BusyGuard::acquire()?;
    let (record, others) = find_record(program_id)?;
    let exe_paths = program_exes(&record);
    Ok(run_scan(&record, &others, exe_paths, &mut on_progress, Instant::now()))
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LeftoverCleanRequest {
    pub scan_id: String,
    pub item_ids: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum LeftoverItemStatus {
    Removed,
    PendingReboot,
    SkippedNeedAdmin,
    NotFound,
    BackupFailed,
    Failed,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LeftoverItemOutcome {
    pub id: String,
    pub status: LeftoverItemStatus,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LeftoverCleanResult {
    pub removed_count: u32,
    pub freed_bytes: u64,
    pub pending_reboot_count: u32,
    pub skipped_need_admin_count: u32,
    pub failed_count: u32,
    pub registry_backup_saved: bool,
    pub items: Vec<LeftoverItemOutcome>,
    /// The user cancelled; remaining items were not processed (callers stop any batch).
    pub cancelled: bool,
}

/// Delete a Prefetch trace only if it is still a plain `.pf` file directly inside
/// `%SystemRoot%\Prefetch` (Windows rebuilds Prefetch data; no Recycle Bin needed).
fn delete_prefetch(path: &Path) -> LeftoverItemStatus {
    let Some(dir) = leftovers::prefetch_dir() else { return LeftoverItemStatus::Failed };
    let inside = path
        .parent()
        .is_some_and(|parent| matching::path_key(parent) == matching::path_key(&dir));
    let name_ok = path
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| leftovers::prefetch_exe_name(n).is_some());
    if !inside || !name_ok {
        return LeftoverItemStatus::Failed;
    }
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => LeftoverItemStatus::NotFound,
        Err(_) => LeftoverItemStatus::Failed,
        Ok(meta) if !meta.is_file() || inventory::is_reparse(&meta) => LeftoverItemStatus::Failed,
        Ok(_) => match fs::remove_file(path) {
            Ok(()) => LeftoverItemStatus::Removed,
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => LeftoverItemStatus::SkippedNeedAdmin,
            Err(_) => LeftoverItemStatus::Failed,
        },
    }
}

fn backup_dir() -> Option<PathBuf> {
    let base = std::env::var_os("LOCALAPPDATA")?;
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let dir = PathBuf::from(base)
        .join("PC Toolkit Pro")
        .join("uninstaller-backups")
        .join(stamp.to_string());
    fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

/// Recycle a path; if it is locked and we are elevated, schedule what remains for deletion at
/// restart instead of failing the item.
///
/// The item itself is inspected with `symlink_metadata` so a junction or symbolic link is never
/// followed: only the link is removed, never the folder it points to.
fn remove_path(path: &Path, is_admin: bool) -> LeftoverItemStatus {
    let Ok(meta) = fs::symlink_metadata(path) else {
        return LeftoverItemStatus::NotFound;
    };
    if inventory::is_reparse(&meta) {
        return remove_link(path, &meta);
    }
    match recycle_path(path) {
        Ok(()) if fs::symlink_metadata(path).is_err() => LeftoverItemStatus::Removed,
        // The bin cannot take it (too large, no bin on the volume): never fall back to a
        // permanent delete, not even at reboot.
        Err(PlatformError::OperationFailed(code)) if code == WOULD_DELETE_PERMANENTLY => LeftoverItemStatus::Failed,
        Ok(()) | Err(_) if is_admin => {
            if schedule_reboot_delete(path) {
                LeftoverItemStatus::PendingReboot
            } else {
                LeftoverItemStatus::Failed
            }
        }
        Err(PlatformError::PermissionDenied) => LeftoverItemStatus::SkippedNeedAdmin,
        _ => LeftoverItemStatus::Failed,
    }
}

/// Remove a junction / symbolic link itself. `remove_dir` on a directory link deletes the link
/// without touching the target; a file symlink is removed with `remove_file`. Link metadata
/// reports neither "dir" nor "file", so the directory attribute decides.
fn remove_link(path: &Path, meta: &fs::Metadata) -> LeftoverItemStatus {
    #[cfg(windows)]
    let directory_link = {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;
        meta.file_attributes() & FILE_ATTRIBUTE_DIRECTORY != 0
    };
    #[cfg(not(windows))]
    let directory_link = meta.is_dir();
    let removed = if directory_link { fs::remove_dir(path) } else { fs::remove_file(path) };
    match removed {
        Ok(()) => LeftoverItemStatus::Removed,
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => LeftoverItemStatus::SkippedNeedAdmin,
        Err(_) => LeftoverItemStatus::Failed,
    }
}

fn schedule_reboot_delete(path: &Path) -> bool {
    let mut files = Vec::new();
    let mut dirs = Vec::new();
    let Ok(top) = fs::symlink_metadata(path) else {
        return false;
    };
    if top.is_dir() && !inventory::is_reparse(&top) {
        let mut stack = vec![path.to_path_buf()];
        while let Some(dir) = stack.pop() {
            dirs.push(dir.clone());
            let Ok(entries) = fs::read_dir(&dir) else { continue };
            for entry in entries.flatten() {
                let Ok(meta) = entry.metadata() else { continue };
                if inventory::is_reparse(&meta) {
                    continue;
                }
                if meta.is_dir() {
                    stack.push(entry.path());
                } else {
                    files.push(entry.path());
                }
            }
        }
    } else {
        files.push(path.to_path_buf());
    }
    let mut ok = true;
    for file in &files {
        ok &= delete_on_reboot(file).is_ok();
    }
    // Deepest folders first so each is empty when its turn comes at boot.
    dirs.sort_by_key(|d| std::cmp::Reverse(d.components().count()));
    for dir in &dirs {
        ok &= delete_on_reboot(dir).is_ok();
    }
    ok
}

/// A Start Menu folder left empty after its shortcuts were removed is removed too. `remove_dir`
/// only succeeds on empty folders, and protected roots (Programs, Desktop) are never touched.
fn remove_empty_parent(path: &Path, protected: &ProtectedRoots) {
    let Some(parent) = path.parent() else { return };
    let empty = fs::read_dir(parent).map(|mut d| d.next().is_none()).unwrap_or(false);
    if empty && !protected.is_protected(parent) {
        let _ = fs::remove_dir(parent);
    }
}

fn platform_status(result: Result<(), PlatformError>) -> LeftoverItemStatus {
    match result {
        Ok(()) => LeftoverItemStatus::Removed,
        Err(PlatformError::PermissionDenied) => LeftoverItemStatus::SkippedNeedAdmin,
        Err(_) => LeftoverItemStatus::Failed,
    }
}

pub fn clean_leftovers<F>(request: LeftoverCleanRequest, mut on_progress: F) -> CoreResult<LeftoverCleanResult>
where
    F: FnMut(UninstallerProgress),
{
    let _busy = BusyGuard::acquire()?;
    let started = Instant::now();
    let started_ms = history_now_ms();
    let (program_label, leftovers) = {
        let scans = SCANS.lock().unwrap_or_else(|e| e.into_inner());
        scans
            .iter()
            .find(|(id, _)| id == &request.scan_id)
            .map(|(_, scan)| scan.clone())
            .ok_or_else(|| CoreError::InvalidInput("unknownScan".into()))?
    };
    let wanted: std::collections::HashSet<&str> = request.item_ids.iter().map(String::as_str).collect();
    let selected: Vec<&Leftover> = leftovers.iter().filter(|l| wanted.contains(l.item.id.as_str())).collect();
    let total = selected.len() as u32;
    let is_admin = is_user_admin();
    let protected = ProtectedRoots::from_environment();
    let mut backup: Option<PathBuf> = None;
    let mut backup_index = 0u32;
    let mut exported_keys: std::collections::HashSet<pctoolkit_platform::RegistryKeyPath> =
        std::collections::HashSet::new();
    let mut result = LeftoverCleanResult {
        removed_count: 0,
        freed_bytes: 0,
        pending_reboot_count: 0,
        skipped_need_admin_count: 0,
        failed_count: 0,
        registry_backup_saved: false,
        items: Vec::new(),
        cancelled: false,
    };

    for (index, leftover) in selected.iter().enumerate() {
        if cancelled() {
            break;
        }
        let item = &leftover.item;
        let mut before = UninstallerProgress::new("cleaning", "", started);
        before.current = index as u32;
        before.total = total;
        before.items_found = total;
        before.current_path = Some(item.location.clone());
        on_progress(before);
        let status = if item.requires_admin && !is_admin {
            LeftoverItemStatus::SkippedNeedAdmin
        } else {
            match &leftover.action {
                LeftoverAction::RecyclePath(raw) => {
                    // Re-check the protected policy at execution time (defence in depth) on the
                    // normalized path, and remove exactly what was checked. Store package data
                    // folders (`%LOCALAPPDATA%\Packages\<family>`) are the one sanctioned child
                    // of a protected root.
                    match protected.normalize(raw) {
                        None => LeftoverItemStatus::Failed,
                        Some(path) if protected.is_protected(&path) && !protected.is_store_package_folder(&path) => {
                            LeftoverItemStatus::Failed
                        }
                        Some(path) => {
                            let status = remove_path(&path, is_admin);
                            if status == LeftoverItemStatus::Removed && item.kind == LeftoverKind::Shortcut {
                                remove_empty_parent(&path, &protected);
                            }
                            status
                        }
                    }
                }
                LeftoverAction::DeleteKey(key) | LeftoverAction::DeleteValue(key, _) => {
                    // One export per distinct key: paired values (firewall rules, Run entries)
                    // share a parent key that would otherwise be exported once per value.
                    // A failed export is retried for the next item of the same key.
                    let exported = exported_keys.contains(key) || {
                        if backup.is_none() {
                            backup = backup_dir();
                        }
                        backup_index += 1;
                        let done = backup
                            .as_ref()
                            .map(|dir| export_registry_key(key, &dir.join(format!("{backup_index:03}.reg"))).is_ok())
                            .unwrap_or(false);
                        if done {
                            exported_keys.insert(key.clone());
                        }
                        done
                    };
                    if !registry_key_exists(key) {
                        LeftoverItemStatus::NotFound
                    } else if !exported {
                        LeftoverItemStatus::BackupFailed
                    } else {
                        result.registry_backup_saved = true;
                        match &leftover.action {
                            LeftoverAction::DeleteValue(_, name) => platform_status(delete_registry_value(key, name)),
                            _ => platform_status(delete_registry_tree(key)),
                        }
                    }
                }
                LeftoverAction::DeletePrefetch(path) => delete_prefetch(path),
                LeftoverAction::DeleteService(name) => platform_status(delete_service(name)),
                LeftoverAction::DeleteTask(name) => platform_status(delete_scheduled_task(name)),
            }
        };
        match status {
            LeftoverItemStatus::Removed => {
                result.removed_count += 1;
                result.freed_bytes = result.freed_bytes.saturating_add(item.bytes);
            }
            LeftoverItemStatus::NotFound => result.removed_count += 1,
            LeftoverItemStatus::PendingReboot => result.pending_reboot_count += 1,
            LeftoverItemStatus::SkippedNeedAdmin => result.skipped_need_admin_count += 1,
            LeftoverItemStatus::BackupFailed | LeftoverItemStatus::Failed => result.failed_count += 1,
        }
        result.items.push(LeftoverItemOutcome {
            id: item.id.clone(),
            status,
        });
        let mut after = UninstallerProgress::new("cleaning", "", started);
        after.current = index as u32 + 1;
        after.total = total;
        after.items_found = total;
        after.current_path = Some(item.location.clone());
        after.item_id = Some(item.id.clone());
        after.item_status = Some(status);
        on_progress(after);
    }

    let was_cancelled = cancelled();
    result.cancelled = was_cancelled;
    append_history(HistoryWrite {
        category: "uninstaller".into(),
        title_key: "history.titles.uninstallerCleanup".into(),
        summary: format!(
            "{program_label} · {} removed · {} skipped · {} failed",
            result.removed_count + result.pending_reboot_count,
            result.skipped_need_admin_count,
            result.failed_count
        ),
        started_at_ms: started_ms,
        finished_at_ms: history_now_ms(),
        outcome: if was_cancelled {
            HistoryOutcome::Cancelled
        } else if result.failed_count > 0 || result.skipped_need_admin_count > 0 || result.pending_reboot_count > 0 {
            HistoryOutcome::CompletedWithWarnings
        } else {
            HistoryOutcome::Completed
        },
        planned_bytes: Some(selected.iter().map(|l| l.item.bytes).sum()),
        result_bytes: Some(result.freed_bytes),
        selected_item_count: total,
        affected_item_count: result.removed_count + result.pending_reboot_count,
        failed_item_count: result.failed_count + result.skipped_need_admin_count,
        detail_lines: Vec::new(),
        action: "clean_uninstall_leftovers".into(),
        detail: None,
    });

    // A cancelled clean still returns what was processed so the UI can show honest counts.
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_cache_evicts_only_the_oldest_scans() {
        let mut scans: Vec<(String, u32)> = Vec::new();
        for i in 0..5 {
            remember_scan(&mut scans, format!("s{i}"), i, 3);
        }
        let ids: Vec<&str> = scans.iter().map(|(id, _)| id.as_str()).collect();
        assert_eq!(ids, ["s2", "s3", "s4"]);
    }

    #[cfg(windows)]
    #[test]
    fn junction_leftover_removes_only_the_link() {
        let base = std::env::temp_dir().join(format!("pctk-uninst-link-{}", uuid::Uuid::new_v4()));
        let target = base.join("target");
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("keep.txt"), b"keep").unwrap();
        let link = base.join("link");
        let made = std::process::Command::new(pctoolkit_platform::system32_exe("cmd.exe"))
            .args(["/C", "mklink", "/J"])
            .arg(&link)
            .arg(&target)
            .output()
            .is_ok_and(|o| o.status.success());
        assert!(made, "mklink /J must succeed in a temp dir");
        assert_eq!(remove_path(&link, false), LeftoverItemStatus::Removed);
        assert!(fs::symlink_metadata(&link).is_err());
        assert!(target.join("keep.txt").is_file(), "the junction target must survive");
        let _ = fs::remove_dir_all(&base);
    }
}
