use pctoolkit_platform::{
    empty_recycle_bin, optimize_memory, require_admin, MemoryOptimizeResult,
};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use crate::history::record_history;
use crate::shared::{CoreError, CoreResult};

static CANCELLED: AtomicBool = AtomicBool::new(false);
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
}

pub fn cancel_cleanup() {
    CANCELLED.store(true, Ordering::SeqCst);
}

fn check_cancel() -> CoreResult<()> {
    if CANCELLED.load(Ordering::SeqCst) {
        Err(CoreError::OperationCancelled)
    } else {
        Ok(())
    }
}

fn acquire_busy() -> CoreResult<()> {
    if BUSY
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Err(CoreError::OperationBusy);
    }
    CANCELLED.store(false, Ordering::SeqCst);
    Ok(())
}

fn release_busy() {
    BUSY.store(false, Ordering::SeqCst);
}

pub fn scan_cleanup() -> CoreResult<CleanupScan> {
    let is_admin = require_admin().is_ok();
    let temp_bytes = estimate_temp_bytes();
    Ok(CleanupScan {
        is_admin,
        items: vec![
            CleanupScanItem {
                id: CleanerCategory::TempFiles,
                title_key: "cleaner.tempFiles".into(),
                estimated_bytes: temp_bytes,
                requires_admin: true,
                selected: true,
            },
            CleanupScanItem {
                id: CleanerCategory::RecycleBin,
                title_key: "cleaner.recycleBin".into(),
                estimated_bytes: 0,
                requires_admin: false,
                selected: true,
            },
            CleanupScanItem {
                id: CleanerCategory::DiskCleanup,
                title_key: "cleaner.diskCleanup".into(),
                estimated_bytes: 0,
                requires_admin: true,
                selected: false,
            },
            CleanupScanItem {
                id: CleanerCategory::FreeMemory,
                title_key: "cleaner.freeMemory".into(),
                estimated_bytes: 0,
                requires_admin: false,
                selected: false,
            },
        ],
    })
}

pub fn execute_cleanup<F>(
    request: CleanupExecuteRequest,
    mut on_progress: F,
) -> CoreResult<CleanupResult>
where
    F: FnMut(CleanupProgress),
{
    acquire_busy()?;
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
                    require_admin().map_err(CoreError::from)?;
                    let (bytes, count, lines) = clean_temp_paths()?;
                    freed_bytes += bytes;
                    files_removed += count;
                    log.extend(lines);
                }
                CleanerCategory::RecycleBin => {
                    empty_recycle_bin()?;
                    log.push("Recycle bin emptied".into());
                }
                CleanerCategory::DiskCleanup => {
                    require_admin().map_err(CoreError::from)?;
                    std::process::Command::new("cleanmgr")
                        .args(["/sagerun:1337"])
                        .spawn()
                        .map_err(|e| CoreError::OperationFailed(e.to_string()))?;
                    log.push("Launched Windows Disk Cleanup".into());
                }
                CleanerCategory::FreeMemory => {
                    let mem = optimize_memory()?;
                    freed_bytes += mem.freed_bytes;
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
        record_history(
            "cleaner",
            "execute_cleanup".into(),
            true,
            Some(format!("freed={}", result.freed_bytes)),
        );
        Ok(result)
    })();
    release_busy();
    result
}

fn temp_roots() -> Vec<PathBuf> {
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
    roots.push(PathBuf::from(r"C:\Windows\Temp"));
    roots.push(PathBuf::from(r"C:\Windows\Prefetch"));
    roots
}

fn estimate_temp_bytes() -> u64 {
    let mut total = 0u64;
    for root in temp_roots() {
        total += dir_size_capped(&root, 5000);
    }
    total
}

fn dir_size_capped(path: &Path, max_entries: usize) -> u64 {
    let mut total = 0u64;
    let mut count = 0usize;
    let walker = walkdir_shallow(path);
    for entry in walker {
        if count >= max_entries {
            break;
        }
        count += 1;
        if let Ok(meta) = entry.metadata() {
            if meta.is_file() {
                total += meta.len();
            }
        }
    }
    total
}

fn walkdir_shallow(path: &Path) -> Vec<std::fs::DirEntry> {
    let mut out = Vec::new();
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
    for root in temp_roots() {
        check_cancel()?;
        let (b, c) = clean_dir_contents(&root);
        freed += b;
        files += c;
        log.push(format!("Cleaned {} ({} files)", root.display(), c));
    }
    let _ = empty_recycle_bin();
    log.push("Recycle bin emptied after temp clean".into());
    Ok((freed, files, log))
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

// Silence unused import if Mutex not needed
#[allow(dead_code)]
static _UNUSED: Mutex<()> = Mutex::new(());
