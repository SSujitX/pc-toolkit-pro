//! Deep Cleaner rule catalog, scan, and execute.
//!
//! Owns reclaimable Windows cache targets for the Deep Cleaner page. Path lists
//! are original PC Toolkit Pro definitions (known public Windows / app cache
//! locations) — not copied from third-party GPL sources. Skip-and-continue on
//! denied paths; only remove contents under declared roots.

use pctoolkit_platform::{empty_recycle_bin, query_recycle_bin};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

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
    /// Process image names that should be closed before cleaning this rule.
    pub related_processes: Vec<String>,
    /// True when cleaning typically needs an elevated process (honest skip if not admin).
    pub requires_elevation: bool,
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
    /// Optional case-insensitive substrings; when set, only matching file names count.
    file_name_contains: Option<&'static [&'static str]>,
    /// Optional case-insensitive extensions without a leading dot.
    file_extensions: Option<&'static [&'static str]>,
    /// When set, only files whose modified time is at least this many days ago match.
    min_age_days: Option<u64>,
    /// None = unlimited. Some(0) = root folder only (do not recurse).
    max_depth: Option<u32>,
    related_processes: &'static [&'static str],
    /// When true, execute skips unless the process is elevated.
    requires_elevation: bool,
}

#[derive(Clone, Copy)]
struct TreeFilters {
    file_name_contains: Option<&'static [&'static str]>,
    file_extensions: Option<&'static [&'static str]>,
    min_age_days: Option<u64>,
    max_depth: Option<u32>,
}

impl TreeFilters {
    #[cfg(test)]
    const NONE: Self = Self {
        file_name_contains: None,
        file_extensions: None,
        min_age_days: None,
        max_depth: None,
    };

    fn from_rule(def: &RuleDef) -> Self {
        Self {
            file_name_contains: def.file_name_contains,
            file_extensions: def.file_extensions,
            min_age_days: def.min_age_days,
            max_depth: def.max_depth,
        }
    }
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

fn push_join(roots: &mut Vec<PathBuf>, base: Option<PathBuf>, parts: &[&str]) {
    if let Some(p) = join_opt(base, parts) {
        roots.push(p);
    }
}

/// Chromium / Electron cache children that are safe to empty.
fn electron_cache_roots(base: PathBuf) -> Vec<PathBuf> {
    const NAMES: &[&str] = &["Cache", "Code Cache", "GPUCache", "ShaderCache"];
    NAMES
        .iter()
        .map(|name| base.join(name))
        .filter(|p| p.exists())
        .collect()
}

/// Chromium User Data caches. `prefix` is relative to LOCALAPPDATA
/// (for example `["Google", "Chrome"]` or `["Vivaldi"]`).
/// When `offline_only` is false, also includes top-level ShaderCache / GrShaderCache.
fn chromium_cache_roots(prefix: &[&str], offline_only: bool) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    let mut parts: Vec<&str> = prefix.to_vec();
    parts.push("User Data");
    let Some(user_data) = join_opt(local_app_data(), &parts) else {
        return roots;
    };
    if !offline_only {
        roots.push(user_data.join("ShaderCache"));
        roots.push(user_data.join("GrShaderCache"));
    }
    let Ok(read) = fs::read_dir(&user_data) else {
        return roots;
    };
    for entry in read.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name == "Default" || name.starts_with("Profile ") {
            let p = entry.path();
            if offline_only {
                roots.push(p.join("Offline Cache"));
            } else {
                roots.push(p.join("Cache"));
                roots.push(p.join("Code Cache"));
                roots.push(p.join("GPUCache"));
                roots.push(p.join("ShaderCache"));
                roots.push(p.join("Media Cache"));
            }
        }
    }
    roots
}

fn webview_cache_roots(user_data: PathBuf) -> Vec<PathBuf> {
    let mut roots = electron_cache_roots(user_data.clone());
    let shader = user_data.join("ShaderCache");
    let gr = user_data.join("GrShaderCache");
    if shader.exists() {
        roots.push(shader);
    }
    if gr.exists() {
        roots.push(gr);
    }
    let Ok(read) = fs::read_dir(&user_data) else {
        return roots;
    };
    for entry in read.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name == "Default" || name.starts_with("Profile ") {
            let p = entry.path();
            roots.extend(electron_cache_roots(p.clone()));
            let media = p.join("Media Cache");
            if media.exists() {
                roots.push(media);
            }
        }
    }
    roots
}

fn local_packages_containing(needle: &str) -> Vec<PathBuf> {
    let needle = needle.to_ascii_lowercase();
    let Some(base) = join_opt(local_app_data(), &["Packages"]) else {
        return Vec::new();
    };
    let Ok(read) = fs::read_dir(base) else {
        return Vec::new();
    };
    read.flatten()
        .map(|entry| entry.path())
        .filter(|p| {
            p.file_name()
                .map(|n| n.to_string_lossy().to_ascii_lowercase().contains(&needle))
                .unwrap_or(false)
        })
        .collect()
}

fn windows_update_cleanup_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    let Some(windir) = env_path("WINDIR") else {
        return roots;
    };
    roots.push(windir.join("SoftwareDistribution").join("Download"));
    roots.push(windir.join("Logs").join("WindowsUpdate"));
    roots
}

fn previous_installations_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    let drive = env_path("SystemDrive").unwrap_or_else(|| PathBuf::from("C:"));
    roots.push(drive.join("Windows.old"));
    roots
}

fn android_cache_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(sdk) = join_opt(local_app_data(), &["Android", "Sdk"]) {
        roots.push(sdk.join(".temp"));
        roots.push(sdk.join("temp"));
        roots.push(sdk.join("cache"));
    }
    if let Some(android) = join_opt(user_profile(), &[".android"]) {
        roots.push(android.join("cache"));
        roots.push(android.join("build-cache"));
    }
    roots
}

fn editor_cache_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    const PRODUCTS: &[&str] = &["Code", "Code - Insiders", "Cursor", "VSCodium", "Windsurf"];
    const CACHE_NAMES: &[&str] = &[
        "Cache",
        "CachedData",
        "Code Cache",
        "GPUCache",
        "CachedExtensionVSIXs",
        "logs",
        "Crashpad",
    ];
    let Some(roaming) = app_data() else {
        return roots;
    };
    for product in PRODUCTS {
        let base = roaming.join(product);
        if !base.exists() {
            continue;
        }
        for name in CACHE_NAMES {
            let p = base.join(name);
            if p.exists() {
                roots.push(p);
            }
        }
    }
    roots
}

fn gpu_shader_cache_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    let Some(local) = local_app_data() else {
        return roots;
    };
    roots.push(local.join("D3DSCache"));
    const VENDORS: &[&str] = &["NVIDIA", "NVIDIA Corporation", "AMD", "Intel"];
    const CACHES: &[&str] = &[
        "DXCache",
        "GLCache",
        "NV_Cache",
        "DxCache",
        "DxcCache",
        "VkCache",
        "ShaderCache",
        "D3DSCache",
    ];
    for vendor in VENDORS {
        for cache in CACHES {
            roots.push(local.join(vendor).join(cache));
        }
    }
    roots
}

fn ide_cache_children(product_dir: PathBuf) -> Vec<PathBuf> {
    ["caches", "index", "tmp", "log", "compile-server"]
        .into_iter()
        .map(|name| product_dir.join(name))
        .collect()
}

fn opera_cache_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(base) = join_opt(app_data(), &["Opera Software"]) {
        for product in ["Opera Stable", "Opera GX Stable"] {
            roots.extend(electron_cache_roots(base.join(product)));
        }
    }
    roots
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
            file_name_contains: None,
            file_extensions: None,
            min_age_days: None,
            max_depth: None,
            related_processes: &[],
            requires_elevation: false,
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
            file_name_contains: Some(&["thumbcache_", "iconcache"]),
            file_extensions: None,
            min_age_days: None,
            max_depth: Some(0),
            related_processes: &[],
            requires_elevation: false,
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
            file_name_contains: None,
            file_extensions: None,
            min_age_days: None,
            max_depth: None,
            related_processes: &[],
            requires_elevation: false,
        },
        RuleDef {
            id: "system.userTemp",
            group: DeepCleanupGroup::System,
            name_key: "deepCleaner.rules.userTemp",
            detail_key: "deepCleaner.rules.userTempDetail",
            risk: "safe",
            recommended: true,
            kind: RuleKind::DirectoryContents,
            roots: || {
                let mut roots = Vec::new();
                if let Some(p) = env_path("TEMP") {
                    roots.push(p);
                }
                if let Some(p) = join_opt(local_app_data(), &["Temp"]) {
                    if !roots.iter().any(|existing| existing == &p) {
                        roots.push(p);
                    }
                }
                roots
            },
            file_name_contains: None,
            file_extensions: None,
            min_age_days: None,
            max_depth: None,
            related_processes: &[],
            requires_elevation: false,
        },
        RuleDef {
            id: "system.directxShaderCache",
            group: DeepCleanupGroup::System,
            name_key: "deepCleaner.rules.directxShaderCache",
            detail_key: "deepCleaner.rules.directxShaderCacheDetail",
            risk: "safe",
            recommended: true,
            kind: RuleKind::DirectoryContents,
            roots: gpu_shader_cache_roots,
            file_name_contains: None,
            file_extensions: None,
            min_age_days: None,
            max_depth: None,
            related_processes: &[],
            requires_elevation: false,
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
            file_name_contains: None,
            file_extensions: None,
            min_age_days: None,
            max_depth: None,
            related_processes: &[],
            requires_elevation: false,
        },
        RuleDef {
            id: "system.deliveryOptimization",
            group: DeepCleanupGroup::System,
            name_key: "deepCleaner.rules.deliveryOptimization",
            detail_key: "deepCleaner.rules.deliveryOptimizationDetail",
            risk: "recoverable",
            recommended: false,
            kind: RuleKind::DirectoryContents,
            roots: || {
                let mut roots = Vec::new();
                if let Some(windir) = env_path("WINDIR") {
                    roots.push(
                        windir
                            .join("ServiceProfiles")
                            .join("NetworkService")
                            .join("AppData")
                            .join("Local")
                            .join("Microsoft")
                            .join("Windows")
                            .join("DeliveryOptimization")
                            .join("Cache"),
                    );
                }
                roots
            },
            file_name_contains: None,
            file_extensions: None,
            min_age_days: None,
            max_depth: None,
            related_processes: &[],
            requires_elevation: false,
        },
        RuleDef {
            id: "system.windowsUpdateCleanup",
            group: DeepCleanupGroup::System,
            name_key: "deepCleaner.rules.windowsUpdateCleanup",
            detail_key: "deepCleaner.rules.windowsUpdateCleanupDetail",
            risk: "recoverable",
            recommended: false,
            kind: RuleKind::DirectoryContents,
            roots: windows_update_cleanup_roots,
            file_name_contains: None,
            file_extensions: None,
            min_age_days: None,
            max_depth: None,
            related_processes: &[],
            requires_elevation: true,
        },
        RuleDef {
            id: "system.previousInstallations",
            group: DeepCleanupGroup::System,
            name_key: "deepCleaner.rules.previousInstallations",
            detail_key: "deepCleaner.rules.previousInstallationsDetail",
            risk: "recoverable",
            recommended: false,
            kind: RuleKind::DirectoryContents,
            roots: previous_installations_roots,
            file_name_contains: None,
            file_extensions: None,
            min_age_days: None,
            max_depth: None,
            related_processes: &[],
            requires_elevation: true,
        },
        RuleDef {
            id: "system.stalePartialDownloads",
            group: DeepCleanupGroup::System,
            name_key: "deepCleaner.rules.stalePartialDownloads",
            detail_key: "deepCleaner.rules.stalePartialDownloadsDetail",
            risk: "recoverable",
            recommended: false,
            kind: RuleKind::DirectoryContents,
            roots: || {
                join_opt(user_profile(), &["Downloads"])
                    .into_iter()
                    .collect()
            },
            file_name_contains: None,
            file_extensions: Some(&["crdownload", "download", "partial", "part"]),
            min_age_days: Some(7),
            max_depth: Some(3),
            related_processes: &[],
            requires_elevation: false,
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
                for pkg in local_packages_containing("whatsapp") {
                    roots.extend(webview_cache_roots(pkg.join("LocalCache").join("EBWebView")));
                    roots.push(pkg.join("TempState"));
                }
                roots
            },
            file_name_contains: None,
            file_extensions: None,
            min_age_days: None,
            max_depth: None,
            related_processes: &["WhatsApp.exe", "WhatsApp.Desktop.exe", "WhatsApp.Root.exe"],
            requires_elevation: false,
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
                let mut roots = Vec::new();
                push_join(
                    &mut roots,
                    app_data(),
                    &["Telegram Desktop", "tdata", "temp"],
                );
                push_join(
                    &mut roots,
                    app_data(),
                    &["Telegram Desktop", "tdata", "dumps"],
                );
                roots
            },
            file_name_contains: None,
            file_extensions: None,
            min_age_days: None,
            max_depth: None,
            related_processes: &["Telegram.exe"],
            requires_elevation: false,
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
                if let Some(base) = join_opt(app_data(), &["discord"]) {
                    roots.extend(electron_cache_roots(base.clone()));
                    roots.push(base.join("DawnCache"));
                    roots.push(base.join("DawnWebGPUCache"));
                    roots.push(base.join("logs"));
                }
                roots
            },
            file_name_contains: None,
            file_extensions: None,
            min_age_days: None,
            max_depth: None,
            related_processes: &["Discord.exe"],
            requires_elevation: false,
        },
        RuleDef {
            id: "app.steamCache",
            group: DeepCleanupGroup::Application,
            name_key: "deepCleaner.rules.steamCache",
            detail_key: "deepCleaner.rules.steamCacheDetail",
            risk: "safe",
            recommended: true,
            kind: RuleKind::DirectoryContents,
            roots: || {
                join_opt(local_app_data(), &["Steam", "htmlcache"])
                    .map(electron_cache_roots)
                    .unwrap_or_default()
            },
            file_name_contains: None,
            file_extensions: None,
            min_age_days: None,
            max_depth: None,
            related_processes: &["steam.exe", "steamwebhelper.exe"],
            requires_elevation: false,
        },
        RuleDef {
            id: "app.teamsCache",
            group: DeepCleanupGroup::Application,
            name_key: "deepCleaner.rules.teamsCache",
            detail_key: "deepCleaner.rules.teamsCacheDetail",
            risk: "safe",
            recommended: true,
            kind: RuleKind::DirectoryContents,
            roots: || {
                let mut roots = Vec::new();
                for pkg in local_packages_containing("msteams") {
                    roots.extend(webview_cache_roots(
                        pkg.join("LocalCache")
                            .join("Microsoft")
                            .join("MSTeams")
                            .join("EBWebView"),
                    ));
                }
                roots
            },
            file_name_contains: None,
            file_extensions: None,
            min_age_days: None,
            max_depth: None,
            related_processes: &["ms-teams.exe", "Teams.exe"],
            requires_elevation: false,
        },
        RuleDef {
            id: "app.spotifyCache",
            group: DeepCleanupGroup::Application,
            name_key: "deepCleaner.rules.spotifyCache",
            detail_key: "deepCleaner.rules.spotifyCacheDetail",
            risk: "safe",
            recommended: true,
            kind: RuleKind::DirectoryContents,
            roots: || {
                let mut roots = Vec::new();
                if let Some(spotify) = join_opt(local_app_data(), &["Spotify"]) {
                    roots.extend(electron_cache_roots(spotify.join("Browser")));
                    roots.extend(electron_cache_roots(spotify.join("Storage")));
                }
                roots
            },
            file_name_contains: None,
            file_extensions: None,
            min_age_days: None,
            max_depth: None,
            related_processes: &["Spotify.exe"],
            requires_elevation: false,
        },
        RuleDef {
            id: "app.slackCache",
            group: DeepCleanupGroup::Application,
            name_key: "deepCleaner.rules.slackCache",
            detail_key: "deepCleaner.rules.slackCacheDetail",
            risk: "safe",
            recommended: true,
            kind: RuleKind::DirectoryContents,
            roots: || {
                join_opt(app_data(), &["Slack"])
                    .map(electron_cache_roots)
                    .unwrap_or_default()
            },
            file_name_contains: None,
            file_extensions: None,
            min_age_days: None,
            max_depth: None,
            related_processes: &["slack.exe"],
            requires_elevation: false,
        },
        RuleDef {
            id: "app.notionCache",
            group: DeepCleanupGroup::Application,
            name_key: "deepCleaner.rules.notionCache",
            detail_key: "deepCleaner.rules.notionCacheDetail",
            risk: "safe",
            recommended: true,
            kind: RuleKind::DirectoryContents,
            roots: || {
                join_opt(app_data(), &["Notion"])
                    .map(electron_cache_roots)
                    .unwrap_or_default()
            },
            file_name_contains: None,
            file_extensions: None,
            min_age_days: None,
            max_depth: None,
            related_processes: &["Notion.exe"],
            requires_elevation: false,
        },
        RuleDef {
            id: "app.dockerDesktopCache",
            group: DeepCleanupGroup::Application,
            name_key: "deepCleaner.rules.dockerDesktopCache",
            detail_key: "deepCleaner.rules.dockerDesktopCacheDetail",
            risk: "safe",
            recommended: true,
            kind: RuleKind::DirectoryContents,
            roots: || {
                join_opt(app_data(), &["Docker Desktop"])
                    .map(electron_cache_roots)
                    .unwrap_or_default()
            },
            file_name_contains: None,
            file_extensions: None,
            min_age_days: None,
            max_depth: None,
            related_processes: &["Docker Desktop.exe"],
            requires_elevation: false,
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
            roots: || chromium_cache_roots(&["Google", "Chrome"], false),
            file_name_contains: None,
            file_extensions: None,
            min_age_days: None,
            max_depth: None,
            related_processes: &["chrome.exe"],
            requires_elevation: false,
        },
        RuleDef {
            id: "browser.chromeOfflineCache",
            group: DeepCleanupGroup::Browser,
            name_key: "deepCleaner.rules.chromeOfflineCache",
            detail_key: "deepCleaner.rules.chromeOfflineCacheDetail",
            risk: "safe",
            recommended: true,
            kind: RuleKind::DirectoryContents,
            roots: || chromium_cache_roots(&["Google", "Chrome"], true),
            file_name_contains: None,
            file_extensions: None,
            min_age_days: None,
            max_depth: None,
            related_processes: &["chrome.exe"],
            requires_elevation: false,
        },
        RuleDef {
            id: "browser.edgeCache",
            group: DeepCleanupGroup::Browser,
            name_key: "deepCleaner.rules.edgeCache",
            detail_key: "deepCleaner.rules.edgeCacheDetail",
            risk: "safe",
            recommended: true,
            kind: RuleKind::DirectoryContents,
            roots: || chromium_cache_roots(&["Microsoft", "Edge"], false),
            file_name_contains: None,
            file_extensions: None,
            min_age_days: None,
            max_depth: None,
            related_processes: &["msedge.exe"],
            requires_elevation: false,
        },
        RuleDef {
            id: "browser.edgeOfflineCache",
            group: DeepCleanupGroup::Browser,
            name_key: "deepCleaner.rules.edgeOfflineCache",
            detail_key: "deepCleaner.rules.edgeOfflineCacheDetail",
            risk: "safe",
            recommended: true,
            kind: RuleKind::DirectoryContents,
            roots: || chromium_cache_roots(&["Microsoft", "Edge"], true),
            file_name_contains: None,
            file_extensions: None,
            min_age_days: None,
            max_depth: None,
            related_processes: &["msedge.exe"],
            requires_elevation: false,
        },
        RuleDef {
            id: "browser.braveCache",
            group: DeepCleanupGroup::Browser,
            name_key: "deepCleaner.rules.braveCache",
            detail_key: "deepCleaner.rules.braveCacheDetail",
            risk: "safe",
            recommended: true,
            kind: RuleKind::DirectoryContents,
            roots: || chromium_cache_roots(&["BraveSoftware", "Brave-Browser"], false),
            file_name_contains: None,
            file_extensions: None,
            min_age_days: None,
            max_depth: None,
            related_processes: &["brave.exe"],
            requires_elevation: false,
        },
        RuleDef {
            id: "browser.vivaldiCache",
            group: DeepCleanupGroup::Browser,
            name_key: "deepCleaner.rules.vivaldiCache",
            detail_key: "deepCleaner.rules.vivaldiCacheDetail",
            risk: "safe",
            recommended: true,
            kind: RuleKind::DirectoryContents,
            roots: || chromium_cache_roots(&["Vivaldi"], false),
            file_name_contains: None,
            file_extensions: None,
            min_age_days: None,
            max_depth: None,
            related_processes: &["vivaldi.exe"],
            requires_elevation: false,
        },
        RuleDef {
            id: "browser.chromiumCache",
            group: DeepCleanupGroup::Browser,
            name_key: "deepCleaner.rules.chromiumCache",
            detail_key: "deepCleaner.rules.chromiumCacheDetail",
            risk: "safe",
            recommended: true,
            kind: RuleKind::DirectoryContents,
            roots: || chromium_cache_roots(&["Chromium"], false),
            file_name_contains: None,
            file_extensions: None,
            min_age_days: None,
            max_depth: None,
            related_processes: &["chromium.exe"],
            requires_elevation: false,
        },
        RuleDef {
            id: "browser.operaCache",
            group: DeepCleanupGroup::Browser,
            name_key: "deepCleaner.rules.operaCache",
            detail_key: "deepCleaner.rules.operaCacheDetail",
            risk: "safe",
            recommended: true,
            kind: RuleKind::DirectoryContents,
            roots: opera_cache_roots,
            file_name_contains: None,
            file_extensions: None,
            min_age_days: None,
            max_depth: None,
            related_processes: &["opera.exe", "opera_gx.exe"],
            requires_elevation: false,
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
            file_name_contains: None,
            file_extensions: None,
            min_age_days: None,
            max_depth: None,
            related_processes: &["firefox.exe"],
            requires_elevation: false,
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
            file_name_contains: None,
            file_extensions: None,
            min_age_days: None,
            max_depth: None,
            related_processes: &[],
            requires_elevation: false,
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
                    .chain(join_opt(local_app_data(), &["pnpm-cache"]))
                    .collect()
            },
            file_name_contains: None,
            file_extensions: None,
            min_age_days: None,
            max_depth: None,
            related_processes: &[],
            requires_elevation: false,
        },
        RuleDef {
            id: "dev.yarnCache",
            group: DeepCleanupGroup::Development,
            name_key: "deepCleaner.rules.yarnCache",
            detail_key: "deepCleaner.rules.yarnCacheDetail",
            risk: "recoverable",
            recommended: false,
            kind: RuleKind::DirectoryContents,
            roots: || {
                join_opt(local_app_data(), &["Yarn", "Cache"])
                    .into_iter()
                    .chain(join_opt(local_app_data(), &["Yarn", "Berry", "Cache"]))
                    .collect()
            },
            file_name_contains: None,
            file_extensions: None,
            min_age_days: None,
            max_depth: None,
            related_processes: &[],
            requires_elevation: false,
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
            file_name_contains: None,
            file_extensions: None,
            min_age_days: None,
            max_depth: None,
            related_processes: &[],
            requires_elevation: false,
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
            file_name_contains: None,
            file_extensions: None,
            min_age_days: None,
            max_depth: None,
            related_processes: &[],
            requires_elevation: false,
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
                    roots.push(home.join("registry").join("src"));
                    roots.push(home.join("git").join("db"));
                    roots.push(home.join("git").join("checkouts"));
                }
                let rustup = env_path("RUSTUP_HOME").or_else(|| {
                    user_profile().map(|p| p.join(".rustup"))
                });
                if let Some(rustup) = rustup {
                    roots.push(rustup.join("downloads"));
                }
                roots
            },
            file_name_contains: None,
            file_extensions: None,
            min_age_days: None,
            max_depth: None,
            related_processes: &[],
            requires_elevation: false,
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
            file_name_contains: None,
            file_extensions: None,
            min_age_days: None,
            max_depth: None,
            related_processes: &[],
            requires_elevation: false,
        },
        RuleDef {
            id: "dev.goBuildCache",
            group: DeepCleanupGroup::Development,
            name_key: "deepCleaner.rules.goBuildCache",
            detail_key: "deepCleaner.rules.goBuildCacheDetail",
            risk: "recoverable",
            recommended: false,
            kind: RuleKind::DirectoryContents,
            roots: || {
                if let Some(p) = env_path("GOCACHE") {
                    let value = p.to_string_lossy();
                    if value.eq_ignore_ascii_case("off") {
                        return Vec::new();
                    }
                    return vec![p];
                }
                join_opt(local_app_data(), &["go-build"])
                    .into_iter()
                    .collect()
            },
            file_name_contains: None,
            file_extensions: None,
            min_age_days: None,
            max_depth: None,
            related_processes: &[],
            requires_elevation: false,
        },
        RuleDef {
            id: "dev.nugetCache",
            group: DeepCleanupGroup::Development,
            name_key: "deepCleaner.rules.nugetCache",
            detail_key: "deepCleaner.rules.nugetCacheDetail",
            risk: "recoverable",
            recommended: false,
            kind: RuleKind::DirectoryContents,
            roots: || {
                let mut roots = Vec::new();
                if let Some(p) = join_opt(local_app_data(), &["NuGet", "v3-cache"]) {
                    roots.push(p);
                }
                if let Some(p) = join_opt(local_app_data(), &["NuGet", "plugins-cache"]) {
                    roots.push(p);
                }
                if let Some(p) = join_opt(local_app_data(), &["NuGet", "Scratch"]) {
                    roots.push(p);
                }
                if let Some(p) = env_path("TEMP").map(|t| t.join("NuGetScratch")) {
                    roots.push(p);
                }
                if let Some(p) = join_opt(user_profile(), &[".nuget", "packages"]) {
                    roots.push(p);
                }
                roots
            },
            file_name_contains: None,
            file_extensions: None,
            min_age_days: None,
            max_depth: None,
            related_processes: &[],
            requires_elevation: false,
        },
        RuleDef {
            id: "dev.gradleCaches",
            group: DeepCleanupGroup::Development,
            name_key: "deepCleaner.rules.gradleCaches",
            detail_key: "deepCleaner.rules.gradleCachesDetail",
            risk: "recoverable",
            recommended: false,
            kind: RuleKind::DirectoryContents,
            roots: || {
                let mut roots = Vec::new();
                if let Some(gradle) = join_opt(user_profile(), &[".gradle"]) {
                    roots.push(gradle.join("caches"));
                    roots.push(gradle.join("daemon"));
                    roots.push(gradle.join("workers"));
                    roots.push(gradle.join("notifications"));
                }
                roots
            },
            file_name_contains: None,
            file_extensions: None,
            min_age_days: None,
            max_depth: None,
            related_processes: &[],
            requires_elevation: false,
        },
        RuleDef {
            id: "dev.mavenRepository",
            group: DeepCleanupGroup::Development,
            name_key: "deepCleaner.rules.mavenRepository",
            detail_key: "deepCleaner.rules.mavenRepositoryDetail",
            risk: "recoverable",
            recommended: false,
            kind: RuleKind::DirectoryContents,
            roots: || {
                join_opt(user_profile(), &[".m2", "repository"])
                    .into_iter()
                    .collect()
            },
            file_name_contains: None,
            file_extensions: None,
            min_age_days: None,
            max_depth: None,
            related_processes: &[],
            requires_elevation: false,
        },
        RuleDef {
            id: "dev.condaPkgs",
            group: DeepCleanupGroup::Development,
            name_key: "deepCleaner.rules.condaPkgs",
            detail_key: "deepCleaner.rules.condaPkgsDetail",
            risk: "recoverable",
            recommended: false,
            kind: RuleKind::DirectoryContents,
            roots: || {
                let mut roots = Vec::new();
                if let Some(home) = user_profile() {
                    roots.push(home.join("miniconda3").join("pkgs"));
                    roots.push(home.join("anaconda3").join("pkgs"));
                    roots.push(home.join("mambaforge").join("pkgs"));
                    roots.push(home.join("miniforge3").join("pkgs"));
                }
                if let Some(p) = join_opt(local_app_data(), &["conda", "conda", "pkgs"]) {
                    roots.push(p);
                }
                roots
            },
            file_name_contains: None,
            file_extensions: None,
            min_age_days: None,
            max_depth: None,
            related_processes: &[],
            requires_elevation: false,
        },
        RuleDef {
            id: "dev.jetbrainsCaches",
            group: DeepCleanupGroup::Development,
            name_key: "deepCleaner.rules.jetbrainsCaches",
            detail_key: "deepCleaner.rules.jetbrainsCachesDetail",
            risk: "recoverable",
            recommended: false,
            kind: RuleKind::DirectoryContents,
            roots: || {
                let mut roots = Vec::new();
                if let Some(base) = join_opt(local_app_data(), &["JetBrains"]) {
                    if let Ok(read) = fs::read_dir(base) {
                        for entry in read.flatten() {
                            roots.extend(ide_cache_children(entry.path()));
                        }
                    }
                }
                if let Some(google) = join_opt(local_app_data(), &["Google"]) {
                    if let Ok(read) = fs::read_dir(google) {
                        for entry in read.flatten() {
                            let name = entry.file_name().to_string_lossy().to_string();
                            if name.starts_with("AndroidStudio") {
                                roots.extend(ide_cache_children(entry.path()));
                            }
                        }
                    }
                }
                roots
            },
            file_name_contains: None,
            file_extensions: None,
            min_age_days: None,
            max_depth: None,
            related_processes: &[],
            requires_elevation: false,
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
            file_name_contains: None,
            file_extensions: None,
            min_age_days: None,
            max_depth: None,
            related_processes: &[],
            requires_elevation: false,
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
                let mut roots = Vec::new();
                push_join(&mut roots, local_app_data(), &["node-gyp"]);
                push_join(&mut roots, app_data(), &["nvm-windows", "cache"]);
                push_join(&mut roots, local_app_data(), &["node", "corepack"]);
                push_join(&mut roots, local_app_data(), &["electron", "Cache"]);
                roots
            },
            file_name_contains: None,
            file_extensions: None,
            min_age_days: None,
            max_depth: None,
            related_processes: &[],
            requires_elevation: false,
        },
        RuleDef {
            id: "dev.playwrightCache",
            group: DeepCleanupGroup::Development,
            name_key: "deepCleaner.rules.playwrightCache",
            detail_key: "deepCleaner.rules.playwrightCacheDetail",
            risk: "recoverable",
            recommended: false,
            kind: RuleKind::DirectoryContents,
            roots: || {
                let mut roots = Vec::new();
                if let Some(local) = local_app_data() {
                    if let Ok(read) = fs::read_dir(&local) {
                        for entry in read.flatten() {
                            let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
                            if name.starts_with("ms-playwright") {
                                roots.push(entry.path());
                            }
                        }
                    }
                    roots.push(local.join("Cypress").join("Cache"));
                }
                push_join(
                    &mut roots,
                    user_profile(),
                    &[".cache", "puppeteer"],
                );
                roots
            },
            file_name_contains: None,
            file_extensions: None,
            min_age_days: None,
            max_depth: None,
            related_processes: &[],
            requires_elevation: false,
        },
        RuleDef {
            id: "dev.androidCache",
            group: DeepCleanupGroup::Development,
            name_key: "deepCleaner.rules.androidCache",
            detail_key: "deepCleaner.rules.androidCacheDetail",
            risk: "recoverable",
            recommended: false,
            kind: RuleKind::DirectoryContents,
            roots: android_cache_roots,
            file_name_contains: None,
            file_extensions: None,
            min_age_days: None,
            max_depth: None,
            related_processes: &[],
            requires_elevation: false,
        },
        RuleDef {
            id: "dev.editorCache",
            group: DeepCleanupGroup::Development,
            name_key: "deepCleaner.rules.editorCache",
            detail_key: "deepCleaner.rules.editorCacheDetail",
            risk: "recoverable",
            recommended: false,
            kind: RuleKind::DirectoryContents,
            roots: editor_cache_roots,
            file_name_contains: None,
            file_extensions: None,
            min_age_days: None,
            max_depth: None,
            related_processes: &["Code.exe", "Cursor.exe", "VSCodium.exe", "Windsurf.exe"],
            requires_elevation: false,
        },
    ]
}

fn existing_roots(roots: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for p in roots {
        if p.exists() && !out.iter().any(|seen| seen == &p) {
            out.push(p);
        }
    }
    out
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
                    let existing = existing_roots((def.roots)());
                    if existing.is_empty() {
                        (0, 0, "notApplicable")
                    } else {
                        let filters = TreeFilters::from_rule(&def);
                        let mut bytes = 0u64;
                        let mut count = 0u32;
                        for root in existing {
                            check_cancel()?;
                            let (b, c) = measure_tree(
                                &root,
                                &filters,
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
                related_processes: def
                    .related_processes
                    .iter()
                    .map(|name| (*name).to_string())
                    .collect(),
                requires_elevation: def.requires_elevation,
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

            if def.requires_elevation && !pctoolkit_platform::is_user_admin() {
                log.push(format!(
                    "Skipped {}: administrator required (no files removed)",
                    def.id
                ));
                continue;
            }

            match def.kind {
                RuleKind::RecycleBin => match empty_recycle_bin() {
                    Ok(result) => {
                        freed_bytes += result.released_bytes;
                        files_removed += result.released_items;
                        log.push("Recycle Bin emptied".into());
                    }
                    Err(e) => log.push(format!("Recycle Bin: {e}")),
                },
                RuleKind::DirectoryContents => {
                    let filters = TreeFilters::from_rule(def);
                    for root in existing_roots((def.roots)()) {
                        check_cancel()?;
                        let (b, c) = clean_tree_contents(&root, &filters, &mut || {
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
    // Explorer Shell query for the current user — never walk X:\$Recycle.Bin
    // (that overcounts other SIDs and follows junctions into huge unrelated trees).
    match query_recycle_bin() {
        Ok(info) if info.bytes == 0 && info.item_count == 0 => (0, 0, "clean"),
        Ok(info) => {
            let count = info.item_count.min(u32::MAX as u64) as u32;
            (info.bytes, count.max(1), "found")
        }
        Err(_) => (0, 0, "notApplicable"),
    }
}

fn file_name_matches(path: &Path, filters: Option<&[&str]>) -> bool {
    let Some(filters) = filters else {
        return true;
    };
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    filters
        .iter()
        .any(|needle| name.contains(&needle.to_ascii_lowercase()))
}

fn file_extension_matches(path: &Path, filters: Option<&[&str]>) -> bool {
    let Some(filters) = filters else {
        return true;
    };
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    filters.iter().any(|want| {
        let want = want.trim_start_matches('.').to_ascii_lowercase();
        ext == want
    })
}

fn file_age_matches(meta: &fs::Metadata, min_age_days: Option<u64>) -> bool {
    let Some(days) = min_age_days else {
        return true;
    };
    let Ok(modified) = meta.modified() else {
        return false;
    };
    let Ok(elapsed) = SystemTime::now().duration_since(modified) else {
        return false;
    };
    elapsed >= Duration::from_secs(days.saturating_mul(24 * 60 * 60))
}

fn file_matches_filters(path: &Path, meta: &fs::Metadata, filters: &TreeFilters) -> bool {
    file_name_matches(path, filters.file_name_contains)
        && file_extension_matches(path, filters.file_extensions)
        && file_age_matches(meta, filters.min_age_days)
}

fn effective_max_depth(filters: &TreeFilters) -> Option<u32> {
    if filters.max_depth.is_some() {
        return filters.max_depth;
    }
    // Name-only filters without an explicit depth stay root-only (thumbnail cache).
    if filters.file_name_contains.is_some()
        && filters.file_extensions.is_none()
        && filters.min_age_days.is_none()
    {
        return Some(0);
    }
    None
}

fn uses_selective_walk(filters: &TreeFilters) -> bool {
    filters.file_name_contains.is_some()
        || filters.file_extensions.is_some()
        || filters.min_age_days.is_some()
        || filters.max_depth.is_some()
}

fn measure_tree(
    path: &Path,
    filters: &TreeFilters,
    on_file: &mut dyn FnMut(&Path, u64),
    max_entries: usize,
) -> (u64, u32) {
    let mut bytes = 0u64;
    let mut count = 0u32;
    let max_depth = effective_max_depth(filters);
    let mut stack = vec![(path.to_path_buf(), 0u32)];
    while let Some((dir, depth)) = stack.pop() {
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
            if is_reparse_entry(&entry) {
                continue;
            }
            let Ok(meta) = entry.metadata() else {
                continue;
            };
            if meta.is_dir() {
                let may_recurse = match max_depth {
                    Some(max) => depth < max,
                    None => true,
                };
                if may_recurse {
                    stack.push((p, depth + 1));
                }
            } else if meta.is_file() && file_matches_filters(&p, &meta, filters) {
                let len = meta.len();
                bytes += len;
                count += 1;
                on_file(&p, len);
            }
        }
    }
    (bytes, count)
}

fn clean_tree_contents(
    path: &Path,
    filters: &TreeFilters,
    should_stop: &mut dyn FnMut() -> bool,
) -> (u64, u64) {
    if uses_selective_walk(filters) {
        return clean_tree_selective(path, filters, should_stop);
    }

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
        if is_reparse_entry(&entry) {
            continue;
        }
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

fn clean_tree_selective(
    path: &Path,
    filters: &TreeFilters,
    should_stop: &mut dyn FnMut() -> bool,
) -> (u64, u64) {
    let mut freed = 0u64;
    let mut files = 0u64;
    let max_depth = effective_max_depth(filters);
    let mut stack = vec![(path.to_path_buf(), 0u32)];
    while let Some((dir, depth)) = stack.pop() {
        if should_stop() {
            break;
        }
        let Ok(read) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in read.flatten() {
            if should_stop() {
                break;
            }
            let p = entry.path();
            if is_reparse_entry(&entry) {
                continue;
            }
            let Ok(meta) = entry.metadata() else {
                continue;
            };
            if meta.is_dir() {
                let may_recurse = match max_depth {
                    Some(max) => depth < max,
                    None => true,
                };
                if may_recurse {
                    stack.push((p, depth + 1));
                }
            } else if meta.is_file() && file_matches_filters(&p, &meta, filters) {
                let len = meta.len();
                if fs::remove_file(&p).is_ok() {
                    freed += len;
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
        if is_reparse_entry(&entry) {
            continue;
        }
        let meta = entry.metadata()?;
        if meta.is_file() {
            total += meta.len();
        } else if meta.is_dir() {
            total += dir_tree_size(&entry.path()).unwrap_or(0);
        }
    }
    Ok(total)
}

fn is_reparse_entry(entry: &fs::DirEntry) -> bool {
    entry.file_type().map(|t| t.is_symlink()).unwrap_or(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn test_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "pctoolkit-deep-{}-{}",
            name,
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    fn write_file(path: &Path, bytes: &[u8]) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        let mut f = fs::File::create(path).unwrap();
        f.write_all(bytes).unwrap();
    }

    fn set_mtime_days_ago(path: &Path, days: u64) {
        let file = fs::File::options().write(true).open(path).unwrap();
        let stamp = SystemTime::now() - Duration::from_secs(days.saturating_mul(24 * 60 * 60));
        file.set_modified(stamp).unwrap();
    }

    #[test]
    fn wave1_catalog_contains_new_rule_ids() {
        let ids: Vec<_> = rule_catalog().iter().map(|r| r.id).collect();
        for id in [
            "system.stalePartialDownloads",
            "browser.braveCache",
            "browser.vivaldiCache",
            "browser.chromiumCache",
            "browser.operaCache",
            "app.steamCache",
            "app.teamsCache",
            "app.spotifyCache",
            "app.slackCache",
            "app.notionCache",
            "dev.goBuildCache",
            "dev.playwrightCache",
        ] {
            assert!(ids.contains(&id), "missing catalog id {id}");
        }
        let stale = rule_catalog()
            .into_iter()
            .find(|r| r.id == "system.stalePartialDownloads")
            .unwrap();
        assert!(!stale.recommended);
        assert_eq!(stale.risk, "recoverable");
        assert_eq!(stale.min_age_days, Some(7));
        assert_eq!(stale.max_depth, Some(3));
    }

    #[test]
    fn thumbnail_rule_is_root_only() {
        let thumb = rule_catalog()
            .into_iter()
            .find(|r| r.id == "system.thumbnailCache")
            .unwrap();
        assert_eq!(thumb.max_depth, Some(0));
        assert!(thumb.recommended);
    }

    #[test]
    fn recycle_bin_is_present_and_not_recommended() {
        let recycle = rule_catalog()
            .into_iter()
            .find(|r| r.id == "system.recycleBin")
            .unwrap();
        assert!(!recycle.recommended);
        assert!(matches!(recycle.kind, RuleKind::RecycleBin));
        assert_eq!(recycle.risk, "recoverable");
    }

    #[test]
    fn measure_tree_skips_symlinks() {
        let root = test_dir("symlink-skip");
        write_file(&root.join("real.bin"), b"hello");
        let outside = test_dir("symlink-skip-outside");
        write_file(&outside.join("huge.bin"), &[0u8; 1024]);
        let link = root.join("link-out");
        let linked = {
            #[cfg(windows)]
            {
                std::os::windows::fs::symlink_file(&outside.join("huge.bin"), &link).is_ok()
            }
            #[cfg(unix)]
            {
                std::os::unix::fs::symlink(&outside.join("huge.bin"), &link).is_ok()
            }
            #[cfg(not(any(windows, unix)))]
            {
                false
            }
        };
        if !linked {
            let _ = fs::remove_dir_all(&root);
            let _ = fs::remove_dir_all(&outside);
            return;
        }
        let (bytes, count) = measure_tree(&root, &TreeFilters::NONE, &mut |_, _| {}, 100);
        assert_eq!(count, 1, "symlink target must not be counted");
        assert_eq!(bytes, 5);
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&outside);
    }

    #[test]
    fn measure_tree_honors_extension_age_and_depth() {
        let root = test_dir("filters");
        write_file(&root.join("old.crdownload"), b"aaaa");
        write_file(&root.join("fresh.crdownload"), b"bbbb");
        write_file(&root.join("keep.txt"), b"cccc");
        write_file(&root.join("nested").join("deep.partial"), b"dddd");
        write_file(
            &root.join("a").join("b").join("c").join("d").join("too-deep.part"),
            b"eeee",
        );
        set_mtime_days_ago(&root.join("old.crdownload"), 10);
        set_mtime_days_ago(&root.join("fresh.crdownload"), 1);
        set_mtime_days_ago(&root.join("nested").join("deep.partial"), 10);
        set_mtime_days_ago(
            &root.join("a").join("b").join("c").join("d").join("too-deep.part"),
            10,
        );

        let filters = TreeFilters {
            file_name_contains: None,
            file_extensions: Some(&["crdownload", "download", "partial", "part"]),
            min_age_days: Some(7),
            max_depth: Some(3),
        };
        let (bytes, count) = measure_tree(&root, &filters, &mut |_, _| {}, 1_000);
        assert_eq!(count, 2);
        assert_eq!(bytes, 8);

        let (freed, removed) = clean_tree_contents(&root, &filters, &mut || false);
        assert_eq!(removed, 2);
        assert_eq!(freed, 8);
        assert!(root.join("fresh.crdownload").exists());
        assert!(root.join("keep.txt").exists());
        assert!(!root.join("old.crdownload").exists());
        assert!(!root.join("nested").join("deep.partial").exists());
        assert!(root
            .join("a")
            .join("b")
            .join("c")
            .join("d")
            .join("too-deep.part")
            .exists());

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn name_filter_without_max_depth_stays_shallow() {
        let root = test_dir("shallow");
        write_file(&root.join("thumbcache_123.db"), b"aa");
        write_file(&root.join("sub").join("thumbcache_nested.db"), b"bb");
        let filters = TreeFilters {
            file_name_contains: Some(&["thumbcache_"]),
            file_extensions: None,
            min_age_days: None,
            max_depth: None,
        };
        let (_, count) = measure_tree(&root, &filters, &mut |_, _| {}, 1_000);
        assert_eq!(count, 1);
        let _ = fs::remove_dir_all(&root);
    }
}
