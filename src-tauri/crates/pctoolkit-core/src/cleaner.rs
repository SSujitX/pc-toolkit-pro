use pctoolkit_platform::{
    empty_recycle_bin, is_user_admin, query_recycle_bin, MemoryOptimizeResult,
};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use crate::history::{append_history, history_now_ms, HistoryOutcome, HistoryWrite};
use crate::shared::{CoreError, CoreResult};

pub(crate) static CANCELLED: AtomicBool = AtomicBool::new(false);
static BUSY: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum CleanerCategory {
    TempFiles,
    RecycleBin,
    DiskCleanup,
    FreeMemory,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupScanItem {
    pub id: CleanerCategory,
    pub title_key: String,
    pub estimated_bytes: u64,
    pub requires_admin: bool,
    pub selected: bool,
    pub risk_key: String,
    pub detail_key: String,
    pub item_count: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupScan {
    pub items: Vec<CleanupScanItem>,
    pub is_admin: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupProgress {
    pub phase: String,
    pub current: u64,
    pub total: u64,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupResult {
    pub freed_bytes: u64,
    pub files_removed: u64,
    pub log: Vec<String>,
    pub memory: Option<MemoryOptimizeResult>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CleanupExecuteRequest {
    pub categories: Vec<CleanerCategory>,
    /// Product source for history: "cleaner" | "deepCleaner"
    #[serde(default = "default_cleanup_source")]
    pub source: String,
}

fn default_cleanup_source() -> String {
    "cleaner".into()
}

pub fn cancel_cleanup() {
    CANCELLED.store(true, Ordering::SeqCst);
}

pub(crate) fn check_cancel() -> CoreResult<()> {
    if CANCELLED.load(Ordering::SeqCst) {
        Err(CoreError::OperationCancelled)
    } else {
        Ok(())
    }
}

pub(crate) fn acquire_busy() -> CoreResult<()> {
    if BUSY
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Err(CoreError::OperationBusy);
    }
    CANCELLED.store(false, Ordering::SeqCst);
    Ok(())
}

pub(crate) fn release_busy() {
    BUSY.store(false, Ordering::SeqCst);
}

pub fn scan_cleanup() -> CoreResult<CleanupScan> {
    scan_cleanup_with_progress(|_| {})
}

pub fn scan_cleanup_with_progress<F>(mut on_progress: F) -> CoreResult<CleanupScan>
where
    F: FnMut(CleanupProgress),
{
    acquire_busy()?;
    let result = (|| {
        // Scan is read-only and does not require elevation.
        let is_admin = is_user_admin();
        on_progress(CleanupProgress {
            phase: "scanning".into(),
            current: 0,
            total: 4,
            message: "Temp & Prefetch".into(),
        });
        check_cancel()?;
        let (temp_bytes, temp_count) = estimate_temp_bytes(&mut on_progress)?;
        check_cancel()?;
        let recycle = query_recycle_bin().unwrap_or_default();
        on_progress(CleanupProgress {
            phase: "scanning".into(),
            current: 4,
            total: 4,
            message: "Complete".into(),
        });
        Ok(CleanupScan {
            is_admin,
            items: vec![
                CleanupScanItem {
                    id: CleanerCategory::TempFiles,
                    title_key: "deepCleaner.tempFiles".into(),
                    estimated_bytes: temp_bytes,
                    requires_admin: false,
                    selected: true,
                    risk_key: "deepCleaner.riskLow".into(),
                    detail_key: "deepCleaner.tempFilesDetail".into(),
                    item_count: temp_count.max(1),
                },
                CleanupScanItem {
                    id: CleanerCategory::RecycleBin,
                    title_key: "deepCleaner.recycleBin".into(),
                    estimated_bytes: recycle.bytes,
                    requires_admin: false,
                    selected: true,
                    risk_key: "deepCleaner.riskLow".into(),
                    detail_key: "deepCleaner.recycleBinDetail".into(),
                    item_count: recycle.item_count.max(1) as u32,
                },
                CleanupScanItem {
                    id: CleanerCategory::DiskCleanup,
                    title_key: "deepCleaner.diskCleanup".into(),
                    estimated_bytes: 0,
                    requires_admin: false,
                    selected: false,
                    risk_key: "deepCleaner.riskMedium".into(),
                    detail_key: "deepCleaner.diskCleanupDetail".into(),
                    item_count: 1,
                },
                CleanupScanItem {
                    id: CleanerCategory::FreeMemory,
                    title_key: "deepCleaner.freeMemory".into(),
                    estimated_bytes: 0,
                    requires_admin: false,
                    selected: false,
                    risk_key: "deepCleaner.riskLow".into(),
                    detail_key: "deepCleaner.freeMemoryDetail".into(),
                    item_count: 1,
                },
            ],
        })
    })();
    release_busy();
    result
}

pub fn execute_cleanup<F>(
    request: CleanupExecuteRequest,
    mut on_progress: F,
) -> CoreResult<CleanupResult>
where
    F: FnMut(CleanupProgress),
{
    acquire_busy()?;
    let started_at_ms = history_now_ms();
    let mut source = "cleaner";
    let mut title_key = "history.titles.cleaner".to_string();
    if request.source == "deepCleaner" {
        source = "deepCleaner";
        title_key = "history.titles.deepCleaner".into();
    } else if request.source == "memoryCleaner" {
        source = "memoryCleaner";
        title_key = "history.titles.memoryCleaner".into();
    }
    let selected_count = request.categories.len() as u32;
    let result = (|| {
        let mut freed_bytes = 0u64;
        let mut files_removed = 0u64;
        let mut log = Vec::new();
        let mut memory = None;
        let total = request.categories.len() as u64;

        for (index, category) in request.categories.iter().enumerate() {
            check_cancel()?;
            on_progress(CleanupProgress {
                phase: "executing".into(),
                current: index as u64 + 1,
                total,
                message: format!("{category:?}"),
            });

            match category {
                CleanerCategory::TempFiles => {
                    // User-writable temp only by default — no admin gate.
                    let (bytes, count, lines) = clean_temp_paths()?;
                    freed_bytes += bytes;
                    files_removed += count;
                    log.extend(lines);
                }
                CleanerCategory::RecycleBin => {
                    let emptied = empty_recycle_bin()?;
                    freed_bytes += emptied.released_bytes;
                    files_removed += emptied.released_items;
                    log.push("Recycle bin emptied".into());
                }
                CleanerCategory::DiskCleanup => {
                    // Best-effort launch; Windows may show UAC. Do not hard-require admin up front.
                    std::process::Command::new("cleanmgr")
                        .args(["/sagerun:1337"])
                        .spawn()
                        .map_err(|e| CoreError::OperationFailed(e.to_string()))?;
                    log.push("Launched Windows Disk Cleanup".into());
                }
                CleanerCategory::FreeMemory => {
                    let mem = crate::memory::optimize_from_cleaner_category(
                        |current, total, area| {
                            on_progress(CleanupProgress {
                                phase: "executing".into(),
                                current: current as u64,
                                total: total as u64,
                                message: area.as_str().into(),
                            });
                        },
                        || CANCELLED.load(Ordering::SeqCst),
                    )?;
                    freed_bytes += mem.freed_bytes;
                    for line in mem.areas.iter().map(|o| {
                        format!(
                            "{}: {:?}",
                            o.id.as_str(),
                            o.status
                        )
                    }) {
                        log.push(line);
                    }
                    log.push(format!(
                        "Memory optimized: freed {} bytes (measured)",
                        mem.freed_bytes
                    ));
                    memory = Some(mem);
                }
            }
        }

        let result = CleanupResult {
            freed_bytes,
            files_removed,
            log,
            memory,
        };
        let finished_at_ms = history_now_ms();
        append_history(HistoryWrite {
            category: source.into(),
            title_key: title_key.clone(),
            summary: format!(
                "{} categories · {} items processed",
                selected_count, result.files_removed
            ),
            started_at_ms,
            finished_at_ms,
            outcome: HistoryOutcome::Completed,
            planned_bytes: None,
            result_bytes: Some(result.freed_bytes),
            selected_item_count: selected_count,
            affected_item_count: result.files_removed as u32,
            failed_item_count: 0,
            detail_lines: result.log.clone(),
            action: "execute_cleanup".into(),
            detail: Some(format!("freed={}", result.freed_bytes)),
        });
        Ok(result)
    })();
    if let Err(CoreError::OperationCancelled) = &result {
        append_history(HistoryWrite {
            category: source.into(),
            title_key,
            summary: "Cancelled by user".into(),
            started_at_ms,
            finished_at_ms: history_now_ms(),
            outcome: HistoryOutcome::Cancelled,
            planned_bytes: None,
            result_bytes: None,
            selected_item_count: selected_count,
            affected_item_count: 0,
            failed_item_count: 0,
            detail_lines: vec!["Cleanup cancelled before completion.".into()],
            action: "execute_cleanup".into(),
            detail: Some("cancelled".into()),
        });
    }
    release_busy();
    result
}

/// User temp roots — always readable without elevation.
fn user_temp_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Ok(p) = std::env::var("TEMP") {
        roots.push(PathBuf::from(p));
    }
    if let Ok(p) = std::env::var("TMP") {
        roots.push(PathBuf::from(p));
    }
    if let Ok(profile) = std::env::var("USERPROFILE") {
        roots.push(PathBuf::from(profile).join("AppData\\Local\\Temp"));
    }
    // Deduplicate
    roots.sort();
    roots.dedup();
    roots
}

/// System paths — best-effort only; skip silently if Access Denied.
fn system_temp_roots() -> Vec<PathBuf> {
    vec![
        PathBuf::from(r"C:\Windows\Temp"),
        PathBuf::from(r"C:\Windows\Prefetch"),
    ]
}

fn estimate_temp_bytes<F>(on_progress: &mut F) -> CoreResult<(u64, u32)>
where
    F: FnMut(CleanupProgress),
{
    let mut total = 0u64;
    let mut count = 0u32;
    let roots: Vec<PathBuf> = user_temp_roots().into_iter().chain(system_temp_roots()).collect();
    let total_roots = roots.len().max(1) as u64;
    for (index, root) in roots.into_iter().enumerate() {
        check_cancel()?;
        on_progress(CleanupProgress {
            phase: "scanning".into(),
            current: index as u64 + 1,
            total: total_roots,
            message: root.display().to_string(),
        });
        let (bytes, entries) = dir_size_capped(&root, 8000);
        total += bytes;
        count += entries;
    }
    Ok((total, count))
}

fn dir_size_capped(path: &Path, max_entries: usize) -> (u64, u32) {
    let mut total = 0u64;
    let mut count = 0u32;
    let walker = walkdir_shallow(path);
    for entry in walker {
        if CANCELLED.load(Ordering::Relaxed) {
            break;
        }
        if count as usize >= max_entries {
            break;
        }
        count += 1;
        if let Ok(meta) = entry.metadata() {
            if meta.is_file() {
                total += meta.len();
            }
        }
    }
    (total, count)
}

fn walkdir_shallow(path: &Path) -> Vec<std::fs::DirEntry> {
    let mut out = Vec::new();
    // Permission denied → empty (skip and continue)
    let Ok(read) = fs::read_dir(path) else {
        return out;
    };
    for entry in read.flatten() {
        out.push(entry);
        if out.len() > 2000 {
            break;
        }
    }
    out
}

fn clean_temp_paths() -> CoreResult<(u64, u64, Vec<String>)> {
    let mut freed = 0u64;
    let mut files = 0u64;
    let mut log = Vec::new();
    // Prefer user temp; system temp is best-effort without aborting on ACL errors.
    for root in user_temp_roots().into_iter().chain(system_temp_roots()) {
        check_cancel()?;
        let (b, c) = clean_dir_contents(&root);
        freed += b;
        files += c;
        if c > 0 || root_readable(&root) {
            log.push(format!("Cleaned {} ({} files)", root.display(), c));
        } else {
            log.push(format!("Skipped {} (not accessible)", root.display()));
        }
    }
    Ok((freed, files, log))
}

fn root_readable(path: &Path) -> bool {
    fs::read_dir(path).is_ok()
}

fn clean_dir_contents(path: &Path) -> (u64, u64) {
    let mut freed = 0u64;
    let mut files = 0u64;
    let Ok(read) = fs::read_dir(path) else {
        return (0, 0);
    };
    for entry in read.flatten() {
        if CANCELLED.load(Ordering::SeqCst) {
            break;
        }
        let p = entry.path();
        if p.is_file() {
            if let Ok(meta) = fs::metadata(&p) {
                let len = meta.len();
                if fs::remove_file(&p).is_ok() {
                    freed += len;
                    files += 1;
                }
            }
        } else if p.is_dir() {
            if let Ok(meta) = dir_tree_size(&p) {
                if fs::remove_dir_all(&p).is_ok() {
                    freed += meta;
                    files += 1;
                }
            }
        }
    }
    (freed, files)
}

fn dir_tree_size(path: &Path) -> std::io::Result<u64> {
    let mut total = 0u64;
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let meta = entry.metadata()?;
        if meta.is_file() {
            total += meta.len();
        } else if meta.is_dir() {
            total += dir_tree_size(&entry.path()).unwrap_or(0);
        }
    }
    Ok(total)
}

#[allow(dead_code)]
static _UNUSED: Mutex<()> = Mutex::new(());
