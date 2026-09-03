//! Deep Cleaner rule catalog, scan, and execute.
//!
//! Owns reclaimable Windows cache targets for the Deep Cleaner page. Path lists
//! are original PC Toolkit Pro definitions (known public Windows / app cache
//! locations) — not copied from third-party GPL sources. Skip-and-continue on
//! denied paths; only remove contents under declared roots.

use pctoolkit_platform::empty_recycle_bin;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::cleaner::{acquire_busy, check_cancel, release_busy, CANCELLED};
use crate::history::{append_history, history_now_ms, HistoryOutcome, HistoryWrite};
use crate::shared::{CoreError, CoreResult};
use std::sync::atomic::Ordering;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DeepCleanupGroup {
    System,
    Application,
    Browser,
    Development,
}

impl DeepCleanupGroup {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Application => "application",
            Self::Browser => "browser",
            Self::Development => "development",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeepCleanupProgress {
    pub phase: String,
    pub current_path: String,
    pub items_scanned: u64,
    pub bytes_scanned: u64,
    pub elapsed_ms: u64,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeepCleanupRuleResult {
    pub id: String,
    pub group: DeepCleanupGroup,
    pub name_key: String,
    pub detail_key: String,
    /// "safe" | "recoverable"
    pub risk: String,
    pub bytes: u64,
    pub item_count: u32,
    pub recommended: bool,
    pub selected: bool,
    /// found | clean | notApplicable
    pub status: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeepCleanupScan {
    pub rules: Vec<DeepCleanupRuleResult>,
    pub is_admin: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeepCleanupExecuteRequest {
    pub rule_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeepCleanupResult {
    pub freed_bytes: u64,
    pub files_removed: u64,
    pub log: Vec<String>,
}

#[derive(Clone, Copy)]
enum RuleKind {
    DirectoryContents,
    RecycleBin,
}

struct RuleDef {
    id: &'static str,
    group: DeepCleanupGroup,
    name_key: &'static str,
    detail_key: &'static str,
    risk: &'static str,
    recommended: bool,
    kind: RuleKind,
    /// Relative to env roots; resolved at scan time.
    roots: fn() -> Vec<PathBuf>,
}

fn env_path(key: &str) -> Option<PathBuf> {
    std::env::var_os(key).map(PathBuf::from)
}

fn local_app_data() -> Option<PathBuf> {
    env_path("LOCALAPPDATA")
}

fn app_data() -> Option<PathBuf> {
    env_path("APPDATA")
}

fn user_profile() -> Option<PathBuf> {
    env_path("USERPROFILE")
}

fn join_opt(base: Option<PathBuf>, parts: &[&str]) -> Option<PathBuf> {
    let mut p = base?;
    for part in parts {
        p.push(part);
    }
    Some(p)
}

fn rule_catalog() -> Vec<RuleDef> {
    vec![
        // —— System ——
        RuleDef {
            id: "system.recycleBin",
            group: DeepCleanupGroup::System,
            name_key: "deepCleaner.rules.recycleBin",
            detail_key: "deepCleaner.rules.recycleBinDetail",
            risk: "recoverable",
            recommended: false,
            kind: RuleKind::RecycleBin,
            roots: || Vec::new(),
        },
        RuleDef {
            id: "system.thumbnailCache",
            group: DeepCleanupGroup::System,
            name_key: "deepCleaner.rules.thumbnailCache",
            detail_key: "deepCleaner.rules.thumbnailCacheDetail",
            risk: "safe",
            recommended: true,
            kind: RuleKind::DirectoryContents,
            roots: || {
                join_opt(local_app_data(), &["Microsoft", "Windows", "Explorer"])
                    .into_iter()
                    .collect()
            },
        },
        RuleDef {
            id: "system.internetTemp",
            group: DeepCleanupGroup::System,
            name_key: "deepCleaner.rules.internetTemp",
            detail_key: "deepCleaner.rules.internetTempDetail",
            risk: "safe",
            recommended: true,
            kind: RuleKind::DirectoryContents,
            roots: || {
                let mut roots = Vec::new();
                if let Some(p) = join_opt(local_app_data(), &["Microsoft", "Windows", "INetCache"]) {
                    roots.push(p);
                }
                if let Some(p) = join_opt(local_app_data(), &["Temp", "Low"]) {
                    roots.push(p);
                }
                roots
            },
        },
        RuleDef {
            id: "system.directxShaderCache",
            group: DeepCleanupGroup::System,
            name_key: "deepCleaner.rules.directxShaderCache",
            detail_key: "deepCleaner.rules.directxShaderCacheDetail",
            risk: "safe",
            recommended: true,
            kind: RuleKind::DirectoryContents,
            roots: || {
                join_opt(local_app_data(), &["D3DSCache"])
                    .into_iter()
                    .collect()
            },
        },
        RuleDef {
            id: "system.errorReports",
            group: DeepCleanupGroup::System,
            name_key: "deepCleaner.rules.errorReports",
            detail_key: "deepCleaner.rules.errorReportsDetail",
            risk: "safe",
            recommended: true,
            kind: RuleKind::DirectoryContents,
            roots: || {
                let mut roots = Vec::new();
                if let Some(p) = join_opt(local_app_data(), &["CrashDumps"]) {
                    roots.push(p);
                }
                if let Some(p) =
                    join_opt(local_app_data(), &["Microsoft", "Windows", "WER", "ReportArchive"])
                {
                    roots.push(p);
                }
                roots
            },
        },
        // —— Application ——
        RuleDef {
            id: "app.whatsappCache",
            group: DeepCleanupGroup::Application,
            name_key: "deepCleaner.rules.whatsappCache",
            detail_key: "deepCleaner.rules.whatsappCacheDetail",
            risk: "safe",
            recommended: true,
            kind: RuleKind::DirectoryContents,
            roots: || {
                let mut roots = Vec::new();
                if let Some(base) = join_opt(local_app_data(), &["Packages"]) {
                    if let Ok(read) = fs::read_dir(&base) {
                        for entry in read.flatten() {
                            let name = entry.file_name().to_string_lossy().to_lowercase();
                            if name.contains("whatsapp") {
                                roots.push(entry.path().join("LocalCache"));
                                roots.push(entry.path().join("TempState"));
                            }
                        }
                    }
                }
                roots
            },
        },
        RuleDef {
            id: "app.telegramTemp",
            group: DeepCleanupGroup::Application,
            name_key: "deepCleaner.rules.telegramTemp",
            detail_key: "deepCleaner.rules.telegramTempDetail",
            risk: "safe",
            recommended: true,
            kind: RuleKind::DirectoryContents,
            roots: || {
                join_opt(app_data(), &["Telegram Desktop", "tdata", "temp"])
                    .into_iter()
                    .collect()
            },
        },
        RuleDef {
            id: "app.discordCache",
            group: DeepCleanupGroup::Application,
            name_key: "deepCleaner.rules.discordCache",
            detail_key: "deepCleaner.rules.discordCacheDetail",
            risk: "safe",
            recommended: false,
            kind: RuleKind::DirectoryContents,
            roots: || {
                let mut roots = Vec::new();
                if let Some(p) = join_opt(app_data(), &["discord", "Cache"]) {
                    roots.push(p);
                }
                if let Some(p) = join_opt(app_data(), &["discord", "Code Cache"]) {
                    roots.push(p);
                }
                if let Some(p) = join_opt(app_data(), &["discord", "GPUCache"]) {
                    roots.push(p);
                }
                roots
            },
        },
        // —— Browser ——
        RuleDef {
            id: "browser.chromeCache",
            group: DeepCleanupGroup::Browser,
            name_key: "deepCleaner.rules.chromeCache",
            detail_key: "deepCleaner.rules.chromeCacheDetail",
            risk: "safe",
            recommended: true,
            kind: RuleKind::DirectoryContents,
            roots: || chromium_cache_roots("Google", "Chrome"),
        },
        RuleDef {
            id: "browser.edgeCache",
            group: DeepCleanupGroup::Browser,
            name_key: "deepCleaner.rules.edgeCache",
            detail_key: "deepCleaner.rules.edgeCacheDetail",
            risk: "safe",
            recommended: true,
            kind: RuleKind::DirectoryContents,
            roots: || chromium_cache_roots("Microsoft", "Edge"),
        },
        RuleDef {
            id: "browser.firefoxCache",
            group: DeepCleanupGroup::Browser,
            name_key: "deepCleaner.rules.firefoxCache",
            detail_key: "deepCleaner.rules.firefoxCacheDetail",
            risk: "safe",
            recommended: false,
            kind: RuleKind::DirectoryContents,
            roots: || {
                let mut roots = Vec::new();
                if let Some(profiles) = join_opt(local_app_data(), &["Mozilla", "Firefox", "Profiles"])
                {
                    if let Ok(read) = fs::read_dir(profiles) {
                        for entry in read.flatten() {
                            let p = entry.path();
                            roots.push(p.join("cache2"));
                            roots.push(p.join("shader-cache"));
                        }
                    }
                }
                roots
            },
        },
        // —— Development ——
        RuleDef {
            id: "dev.npmCache",
            group: DeepCleanupGroup::Development,
            name_key: "deepCleaner.rules.npmCache",
            detail_key: "deepCleaner.rules.npmCacheDetail",
            risk: "recoverable",
            recommended: false,
            kind: RuleKind::DirectoryContents,
            roots: || {
                join_opt(app_data(), &["npm-cache"])
                    .into_iter()
                    .chain(join_opt(local_app_data(), &["npm-cache"]))
                    .collect()
            },
        },
        RuleDef {
            id: "dev.pnpmStore",
            group: DeepCleanupGroup::Development,
            name_key: "deepCleaner.rules.pnpmStore",
            detail_key: "deepCleaner.rules.pnpmStoreDetail",
            risk: "recoverable",
            recommended: false,
            kind: RuleKind::DirectoryContents,
            roots: || {
                join_opt(local_app_data(), &["pnpm", "store"])
                    .into_iter()
                    .collect()
            },
        },
        RuleDef {
            id: "dev.pipCache",
            group: DeepCleanupGroup::Development,
            name_key: "deepCleaner.rules.pipCache",
            detail_key: "deepCleaner.rules.pipCacheDetail",
            risk: "recoverable",
            recommended: false,
            kind: RuleKind::DirectoryContents,
            roots: || {
                join_opt(local_app_data(), &["pip", "Cache"])
                    .into_iter()
                    .collect()
            },
        },
        RuleDef {
            id: "dev.uvCache",
            group: DeepCleanupGroup::Development,
            name_key: "deepCleaner.rules.uvCache",
            detail_key: "deepCleaner.rules.uvCacheDetail",
            risk: "recoverable",
            recommended: true,
            kind: RuleKind::DirectoryContents,
            roots: || {
                join_opt(local_app_data(), &["uv", "cache"])
                    .into_iter()
                    .collect()
            },
        },
        RuleDef {
            id: "dev.cargoCache",
            group: DeepCleanupGroup::Development,
            name_key: "deepCleaner.rules.cargoCache",
            detail_key: "deepCleaner.rules.cargoCacheDetail",
            risk: "recoverable",
            recommended: false,
            kind: RuleKind::DirectoryContents,
            roots: || {
                let mut roots = Vec::new();
                if let Some(home) = env_path("CARGO_HOME").or_else(|| {
                    user_profile().map(|p| p.join(".cargo"))
                }) {
                    roots.push(home.join("registry").join("cache"));
                    roots.push(home.join("git").join("db"));
                }
                roots
            },
        },
        RuleDef {
            id: "dev.goModuleCache",
            group: DeepCleanupGroup::Development,
            name_key: "deepCleaner.rules.goModuleCache",
            detail_key: "deepCleaner.rules.goModuleCacheDetail",
            risk: "recoverable",
            recommended: false,
            kind: RuleKind::DirectoryContents,
            roots: || {
                env_path("GOMODCACHE")
                    .or_else(|| {
                        env_path("GOPATH")
                            .map(|p| p.join("pkg").join("mod"))
                            .or_else(|| user_profile().map(|p| p.join("go").join("pkg").join("mod")))
                    })
                    .into_iter()
                    .collect()
            },
        },
        RuleDef {
            id: "dev.vsCache",
            group: DeepCleanupGroup::Development,
            name_key: "deepCleaner.rules.vsCache",
            detail_key: "deepCleaner.rules.vsCacheDetail",
            risk: "recoverable",
            recommended: false,
            kind: RuleKind::DirectoryContents,
            roots: || {
                let mut roots = Vec::new();
                if let Some(p) = join_opt(local_app_data(), &["Microsoft", "VisualStudio"]) {
                    if let Ok(read) = fs::read_dir(p) {
                        for entry in read.flatten() {
                            roots.push(entry.path().join("ComponentModelCache"));
                        }
                    }
                }
                roots
            },
        },
        RuleDef {
            id: "dev.nodeTooling",
            group: DeepCleanupGroup::Development,
            name_key: "deepCleaner.rules.nodeTooling",
            detail_key: "deepCleaner.rules.nodeToolingDetail",
            risk: "recoverable",
            recommended: false,
            kind: RuleKind::DirectoryContents,
            roots: || {
                join_opt(local_app_data(), &["node-gyp"])
                    .into_iter()
                    .chain(join_opt(app_data(), &["nvm-windows", "cache"]))
                    .collect()
            },
        },
    ]
}

fn chromium_cache_roots(vendor: &str, product: &str) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    let Some(user_data) = join_opt(local_app_data(), &[vendor, product, "User Data"]) else {
        return roots;
    };
    let Ok(read) = fs::read_dir(&user_data) else {
        return roots;
    };
    for entry in read.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name == "Default" || name.starts_with("Profile ") {
            let p = entry.path();
            roots.push(p.join("Cache"));
            roots.push(p.join("Code Cache"));
            roots.push(p.join("GPUCache"));
            roots.push(p.join("ShaderCache"));
            roots.push(p.join("Media Cache"));
        }
    }
    roots
}

const MAX_ENTRIES_PER_RULE: usize = 40_000;

pub fn scan_deep_cleanup_with_progress<F>(mut on_progress: F) -> CoreResult<DeepCleanupScan>
where
    F: FnMut(DeepCleanupProgress),
{
    acquire_busy()?;
    let started = Instant::now();
    let result = (|| {
        let is_admin = pctoolkit_platform::is_user_admin();
        let mut rules = Vec::new();
        let mut items_scanned = 0u64;
        let mut bytes_scanned = 0u64;

        for def in rule_catalog() {
            check_cancel()?;
            let elapsed_ms = started.elapsed().as_millis() as u64;
            on_progress(DeepCleanupProgress {
                phase: "scanning".into(),
                current_path: def.id.into(),
                items_scanned,
                bytes_scanned,
                elapsed_ms,
                message: def.name_key.into(),
            });

            let (bytes, count, status) = match def.kind {
                RuleKind::RecycleBin => estimate_recycle_bin(),
                RuleKind::DirectoryContents => {
                    let roots = (def.roots)();
                    let existing: Vec<_> = roots.into_iter().filter(|p| p.exists()).collect();
                    if existing.is_empty() {
                        (0, 0, "notApplicable")
                    } else {
                        let mut bytes = 0u64;
                        let mut count = 0u32;
                        for root in existing {
                            check_cancel()?;
                            let (b, c) = measure_tree(
                                &root,
                                &mut |path, file_bytes| {
                                    items_scanned += 1;
                                    bytes_scanned += file_bytes;
                                    if items_scanned % 250 == 0 {
                                        on_progress(DeepCleanupProgress {
                                            phase: "scanning".into(),
                                            current_path: path.display().to_string(),
                                            items_scanned,
                                            bytes_scanned,
                                            elapsed_ms: started.elapsed().as_millis() as u64,
                                            message: def.name_key.into(),
                                        });
                                    }
                                },
                                MAX_ENTRIES_PER_RULE.saturating_sub(count as usize),
                            );
                            bytes += b;
                            count += c;
                        }
                        if count == 0 {
                            (0, 0, "clean")
                        } else {
                            (bytes, count, "found")
                        }
                    }
                }
            };

            let selected = def.recommended && status == "found" && bytes > 0;
            rules.push(DeepCleanupRuleResult {
                id: def.id.into(),
                group: def.group,
                name_key: def.name_key.into(),
                detail_key: def.detail_key.into(),
                risk: def.risk.into(),
                bytes,
                item_count: count,
                recommended: def.recommended,
                selected,
                status: status.into(),
            });
        }

        on_progress(DeepCleanupProgress {
            phase: "complete".into(),
            current_path: String::new(),
            items_scanned,
            bytes_scanned,
            elapsed_ms: started.elapsed().as_millis() as u64,
            message: "complete".into(),
        });

        Ok(DeepCleanupScan { rules, is_admin })
    })();
    release_busy();
    result
}

pub fn execute_deep_cleanup<F>(
    request: DeepCleanupExecuteRequest,
    mut on_progress: F,
) -> CoreResult<DeepCleanupResult>
where
    F: FnMut(DeepCleanupProgress),
{
    acquire_busy()?;
    let started_at_ms = history_now_ms();
    let started = Instant::now();
    let selected_count = request.rule_ids.len() as u32;
    let catalog = rule_catalog();
    let result = (|| {
        let mut freed_bytes = 0u64;
        let mut files_removed = 0u64;
        let mut log = Vec::new();
        let mut items_scanned = 0u64;

        for (index, rule_id) in request.rule_ids.iter().enumerate() {
            check_cancel()?;
            let Some(def) = catalog.iter().find(|r| r.id == rule_id.as_str()) else {
                log.push(format!("Unknown rule skipped: {rule_id}"));
                continue;
            };
            on_progress(DeepCleanupProgress {
                phase: "executing".into(),
                current_path: def.id.into(),
                items_scanned: index as u64 + 1,
                bytes_scanned: freed_bytes,
                elapsed_ms: started.elapsed().as_millis() as u64,
                message: def.name_key.into(),
            });

            match def.kind {
                RuleKind::RecycleBin => match empty_recycle_bin() {
                    Ok(()) => {
                        log.push("Recycle Bin emptied".into());
                        files_removed += 1;
                    }
                    Err(e) => log.push(format!("Recycle Bin: {e}")),
                },
                RuleKind::DirectoryContents => {
                    for root in (def.roots)() {
                        check_cancel()?;
                        if !root.exists() {
                            continue;
                        }
                        let (b, c) = clean_tree_contents(&root, &mut || {
                            items_scanned += 1;
                            CANCELLED.load(Ordering::SeqCst)
                        });
                        freed_bytes += b;
                        files_removed += c;
                        if c > 0 {
                            log.push(format!("Cleaned {} ({} items)", root.display(), c));
                        }
                    }
                }
            }
        }

        let result = DeepCleanupResult {
            freed_bytes,
            files_removed,
            log,
        };
        append_history(HistoryWrite {
            category: "deepCleaner".into(),
            title_key: "history.titles.deepCleaner".into(),
            summary: format!(
                "{} rules · {} items · {} bytes",
                selected_count, result.files_removed, result.freed_bytes
            ),
            started_at_ms,
            finished_at_ms: history_now_ms(),
            outcome: HistoryOutcome::Completed,
            planned_bytes: None,
            result_bytes: Some(result.freed_bytes),
            selected_item_count: selected_count,
            affected_item_count: result.files_removed as u32,
            failed_item_count: 0,
            detail_lines: result.log.clone(),
            action: "execute_deep_cleanup".into(),
            detail: Some(format!("freed={}", result.freed_bytes)),
        });
        Ok(result)
    })();

    if let Err(CoreError::OperationCancelled) = &result {
        append_history(HistoryWrite {
            category: "deepCleaner".into(),
            title_key: "history.titles.deepCleaner".into(),
            summary: "Cancelled by user".into(),
            started_at_ms,
            finished_at_ms: history_now_ms(),
            outcome: HistoryOutcome::Cancelled,
            planned_bytes: None,
            result_bytes: None,
            selected_item_count: selected_count,
            affected_item_count: 0,
            failed_item_count: 0,
            detail_lines: vec!["Deep cleanup cancelled.".into()],
            action: "execute_deep_cleanup".into(),
            detail: Some("cancelled".into()),
        });
    }
    release_busy();
    result
}

fn estimate_recycle_bin() -> (u64, u32, &'static str) {
    // Best-effort size; empty still offered when inaccessible.
    let mut bytes = 0u64;
    let mut count = 0u32;
    for letter in b'C'..=b'Z' {
        let root = PathBuf::from(format!(r"{}:\$Recycle.Bin", letter as char));
        if !root.exists() {
            continue;
        }
        let (b, c) = measure_tree(&root, &mut |_, _| {}, 8_000);
        bytes += b;
        count += c;
    }
    if count == 0 {
        (0, 1, "found")
    } else {
        (bytes, count, "found")
    }
}

fn measure_tree(
    path: &Path,
    on_file: &mut dyn FnMut(&Path, u64),
    max_entries: usize,
) -> (u64, u32) {
    let mut bytes = 0u64;
    let mut count = 0u32;
    let mut stack = vec![path.to_path_buf()];
    while let Some(dir) = stack.pop() {
        if CANCELLED.load(Ordering::Relaxed) || count as usize >= max_entries {
            break;
        }
        let Ok(read) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in read.flatten() {
            if CANCELLED.load(Ordering::Relaxed) || count as usize >= max_entries {
                break;
            }
            let p = entry.path();
            let Ok(meta) = entry.metadata() else {
                continue;
            };
            if meta.is_dir() {
                stack.push(p);
            } else if meta.is_file() {
                let len = meta.len();
                bytes += len;
                count += 1;
                on_file(&p, len);
            }
        }
    }
    (bytes, count)
}

fn clean_tree_contents(path: &Path, should_stop: &mut dyn FnMut() -> bool) -> (u64, u64) {
    let mut freed = 0u64;
    let mut files = 0u64;
    let Ok(read) = fs::read_dir(path) else {
        return (0, 0);
    };
    for entry in read.flatten() {
        if should_stop() {
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
            if let Ok(size) = dir_tree_size(&p) {
                if fs::remove_dir_all(&p).is_ok() {
                    freed += size;
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
