//! Care category: Privacy traces.
//!
//! One item per trace group, all `Review` and unselected (never auto-fixed):
//! - Windows Recent items (`*.lnk` in the Recent folder) and jump lists
//!   (`AutomaticDestinations` / `CustomDestinations`, which also hold pinned jump-list entries);
//! - Explorer MRU keys under HKCU (RecentDocs, RunMRU, TypedPaths, WordWheelQuery, common dialog
//!   Open/Save and Last-visited MRUs) — exported to a `.reg` backup before the key is removed;
//! - per browser profile: history and cookies for Chromium browsers (Chrome / Edge / Brave /
//!   Vivaldi), cookies and saved form data for Firefox.
//!
//! What is deliberately never touched: saved passwords (`Login Data`, `logins.json`, `key4.db`),
//! Local Storage / IndexedDB / Service Worker / File System site storage, caches (Junk owns them),
//! Chromium `Web Data` (it mixes form autofill with search engines, addresses and payment
//! methods — removing it would lose more than form data), and Firefox `places.sqlite` (history and
//! bookmarks share one database). Browser files are removed only while the browser is closed.
//!
//! Counts are honest but rough: a browser item reports how many of its database files exist and
//! their size, not the number of rows inside (that would need an SQLite reader).

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use pctoolkit_platform::privacy_traces::{recent_items_dir, registry_trace_count};
use pctoolkit_platform::{
    count_running_images, delete_registry_tree, export_registry_key, RegistryKeyPath, RegistryRoot, RegistryView,
};

use crate::care::{
    fix_all, skipped, CareCategory, CareCategoryId, CareCategoryScan, CareContext, CareFixResult, CareFixStatus,
    CareItem, CareRisk,
};
use crate::shared::CoreResult;

const ID: CareCategoryId = CareCategoryId::Privacy;
const EXPLORER: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer";

/// Explorer MRU keys (relative to [`EXPLORER`]) with their stable item key.
const MRU_KEYS: [(&str, &str); 6] = [
    ("recentDocs", "RecentDocs"),
    ("runMru", "RunMRU"),
    ("typedPaths", "TypedPaths"),
    ("searchHistory", "WordWheelQuery"),
    ("openSaveMru", r"ComDlg32\OpenSavePidlMRU"),
    ("lastVisitedMru", r"ComDlg32\LastVisitedPidlMRU"),
];

/// Profile-relative files that no privacy group may ever include.
pub(crate) const NEVER_TOUCHED: [&str; 12] = [
    "Login Data",
    "Login Data For Account",
    "Web Data",
    "Local Storage",
    "IndexedDB",
    "Service Worker",
    "File System",
    "Session Storage",
    "places.sqlite",
    "logins.json",
    "key4.db",
    "storage",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Engine {
    Chromium,
    Firefox,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BaseDir {
    Local,
    Roaming,
}

struct BrowserDef {
    key: &'static str,
    display: &'static str,
    base: BaseDir,
    /// User Data dir (Chromium) or Profiles dir (Firefox), relative to `base`.
    root: &'static str,
    engine: Engine,
    images: &'static [&'static str],
}

const BROWSERS: [BrowserDef; 5] = [
    BrowserDef { key: "chrome", display: "Google Chrome", base: BaseDir::Local, root: r"Google\Chrome\User Data", engine: Engine::Chromium, images: &["chrome.exe"] },
    BrowserDef { key: "edge", display: "Microsoft Edge", base: BaseDir::Local, root: r"Microsoft\Edge\User Data", engine: Engine::Chromium, images: &["msedge.exe"] },
    BrowserDef { key: "brave", display: "Brave", base: BaseDir::Local, root: r"BraveSoftware\Brave-Browser\User Data", engine: Engine::Chromium, images: &["brave.exe"] },
    BrowserDef { key: "vivaldi", display: "Vivaldi", base: BaseDir::Local, root: r"Vivaldi\User Data", engine: Engine::Chromium, images: &["vivaldi.exe"] },
    BrowserDef { key: "firefox", display: "Mozilla Firefox", base: BaseDir::Roaming, root: r"Mozilla\Firefox\Profiles", engine: Engine::Firefox, images: &["firefox.exe"] },
];

/// Browser trace groups: (group key, locale suffix, profile-relative files).
pub(crate) fn browser_groups(engine_is_firefox: bool) -> &'static [(&'static str, &'static [&'static str])] {
    const CHROMIUM: [(&str, &[&str]); 2] = [
        ("history", &["History", "History-journal", "Visited Links"]),
        ("cookies", &[r"Network\Cookies", r"Network\Cookies-journal", "Cookies", "Cookies-journal"]),
    ];
    const FIREFOX: [(&str, &[&str]); 2] = [
        ("cookies", &["cookies.sqlite", "cookies.sqlite-wal", "cookies.sqlite-shm"]),
        ("formData", &["formhistory.sqlite", "formhistory.sqlite-wal", "formhistory.sqlite-shm"]),
    ];
    if engine_is_firefox {
        &FIREFOX
    } else {
        &CHROMIUM
    }
}

/// What a privacy item removes. Paths are re-derived at fix time; only ids cross the boundary.
#[derive(Debug, Clone)]
enum Target {
    RecentItems,
    JumpLists,
    Mru { subkey: &'static str },
    Browser { browser: usize, profile_dir: PathBuf, group: &'static str },
}

static TARGETS: Mutex<Option<HashMap<String, Target>>> = Mutex::new(None);

pub struct PrivacyCategory;

fn env_dir(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).map(PathBuf::from).filter(|p| p.is_absolute())
}

fn base_dir(base: BaseDir) -> Option<PathBuf> {
    match base {
        BaseDir::Local => env_dir("LOCALAPPDATA"),
        BaseDir::Roaming => env_dir("APPDATA"),
    }
}

fn recent_dir() -> Option<PathBuf> {
    recent_items_dir().or_else(|| env_dir("APPDATA").map(|a| a.join(r"Microsoft\Windows\Recent")))
}

/// Regular files directly in `dir` whose name satisfies `keep` (no recursion, no links).
fn files_in(dir: &Path, keep: impl Fn(&str) -> bool) -> Vec<(PathBuf, u64)> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|e| {
            let meta = e.metadata().ok()?;
            let name = e.file_name().to_string_lossy().into_owned();
            (meta.is_file() && keep(&name)).then(|| (e.path(), meta.len()))
        })
        .collect()
}

/// ASCII case-insensitive suffix test on bytes: slicing the `str` could split a multi-byte
/// character (non-ASCII file names) and panic.
fn ends_with_ci(name: &str, suffix: &str) -> bool {
    let (name, suffix) = (name.as_bytes(), suffix.as_bytes());
    name.len() >= suffix.len() && name[name.len() - suffix.len()..].eq_ignore_ascii_case(suffix)
}

fn recent_item_files() -> Vec<(PathBuf, u64)> {
    recent_dir().map_or_else(Vec::new, |d| files_in(&d, |n| ends_with_ci(n, ".lnk")))
}

fn jump_list_files() -> Vec<(PathBuf, u64)> {
    let Some(recent) = recent_dir() else {
        return Vec::new();
    };
    let mut out = files_in(&recent.join("AutomaticDestinations"), |n| ends_with_ci(n, ".automaticDestinations-ms"));
    out.extend(files_in(&recent.join("CustomDestinations"), |n| ends_with_ci(n, ".customDestinations-ms")));
    out
}

/// Browser profile directories under a User Data / Profiles root.
/// Chromium: `Default`, `Profile N`, `Guest Profile` with a `Preferences` file.
/// Firefox: any profile folder with `prefs.js`.
pub(crate) fn discover_profiles(root: &Path, firefox: bool) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };
    let mut out: Vec<PathBuf> = entries
        .flatten()
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .map(|e| e.path())
        .filter(|p| {
            let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            if firefox {
                p.join("prefs.js").is_file()
            } else {
                (name == "Default" || name.starts_with("Profile ") || name == "Guest Profile")
                    && p.join("Preferences").is_file()
            }
        })
        .collect();
    out.sort();
    out
}

/// Profile names as the browser shows them, keyed by profile folder name.
/// Chromium keeps them in `<User Data>\Local State` → `profile.info_cache.<dir>.name`;
/// Firefox folder names look like `abcd1234.default-release`, so the part after the dot is used.
pub(crate) fn profile_display_names(root: &Path, firefox: bool, profiles: &[PathBuf]) -> HashMap<String, String> {
    let mut names = HashMap::new();
    if firefox {
        for p in profiles {
            if let Some(dir) = p.file_name().map(|n| n.to_string_lossy().into_owned()) {
                let label = dir.split_once('.').map(|(_, rest)| rest.to_string()).unwrap_or_else(|| dir.clone());
                names.insert(dir, label);
            }
        }
        return names;
    }
    let Ok(raw) = fs::read_to_string(root.join("Local State")) else {
        return names;
    };
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&raw) {
        if let Some(cache) = json.pointer("/profile/info_cache").and_then(|v| v.as_object()) {
            for (dir, info) in cache {
                if let Some(name) = info.get("name").and_then(|v| v.as_str()).filter(|n| !n.trim().is_empty()) {
                    names.insert(dir.clone(), name.trim().to_string());
                }
            }
        }
    }
    names
}

pub(crate) fn is_never_touched(rel: &str) -> bool {
    let first = rel.split(['\\', '/']).next().unwrap_or(rel);
    NEVER_TOUCHED.iter().any(|n| n.eq_ignore_ascii_case(first) || n.eq_ignore_ascii_case(rel))
}

/// Existing files of one browser group in a profile.
pub(crate) fn group_files(profile_dir: &Path, rel_files: &[&str]) -> Vec<(PathBuf, u64)> {
    rel_files
        .iter()
        // Defense in depth: even a mistaken group definition can never reach these.
        .filter(|rel| !is_never_touched(rel))
        .filter_map(|rel| {
            let path = profile_dir.join(rel);
            let meta = fs::symlink_metadata(&path).ok()?;
            meta.is_file().then(|| (path, meta.len()))
        })
        .collect()
}

fn group_rel_files(browser: &BrowserDef, group: &str) -> &'static [&'static str] {
    browser_groups(browser.engine == Engine::Firefox)
        .iter()
        .find(|(g, _)| *g == group)
        .map(|(_, files)| *files)
        .unwrap_or(&[])
}

fn mru_path(subkey: &str) -> RegistryKeyPath {
    RegistryKeyPath::new(RegistryRoot::CurrentUser, RegistryView::Native64, format!(r"{EXPLORER}\{subkey}"))
}

fn item(id: String, title_key: &str, params: BTreeMap<String, String>, count: u64, bytes: Option<u64>) -> CareItem {
    CareItem {
        id,
        category: ID,
        title_key: Some(format!("care.privacy.{title_key}")),
        title: None,
        params,
        detail_key: Some(format!("care.privacy.{title_key}Detail")),
        detail: None,
        location: None,
        bytes,
        count: count.max(1),
        risk: CareRisk::Review,
        selected: false,
        fixable: true,
        needs_admin: false,
        action: None,
    }
}

fn totals(files: &[(PathBuf, u64)]) -> (u64, u64) {
    (files.len() as u64, files.iter().map(|(_, b)| *b).sum())
}

fn scan_items(ctx: &CareContext) -> Option<(Vec<CareItem>, HashMap<String, Target>)> {
    let mut items = Vec::new();
    let mut targets = HashMap::new();

    ctx.current(ID, "Windows");
    let recent = recent_item_files();
    if !recent.is_empty() {
        let (count, bytes) = totals(&recent);
        items.push(item("windows:recentItems".into(), "recentItems", BTreeMap::new(), count, Some(bytes)));
        targets.insert("windows:recentItems".into(), Target::RecentItems);
    }
    let jumps = jump_list_files();
    if !jumps.is_empty() {
        let (count, bytes) = totals(&jumps);
        items.push(item("windows:jumpLists".into(), "jumpLists", BTreeMap::new(), count, Some(bytes)));
        targets.insert("windows:jumpLists".into(), Target::JumpLists);
    }
    for (key, subkey) in MRU_KEYS {
        if ctx.should_stop() {
            return None;
        }
        let count = registry_trace_count(&mru_path(subkey)).unwrap_or(0);
        if count > 0 {
            let id = format!("explorer:{key}");
            items.push(item(id.clone(), &format!("mru.{key}"), BTreeMap::new(), count, None));
            targets.insert(id, Target::Mru { subkey });
        }
        ctx.found(ID, items.len() as u64);
    }

    for (index, browser) in BROWSERS.iter().enumerate() {
        if ctx.should_stop() {
            return None;
        }
        let Some(root) = base_dir(browser.base).map(|b| b.join(browser.root)) else {
            continue;
        };
        let firefox = browser.engine == Engine::Firefox;
        let profiles = discover_profiles(&root, firefox);
        if profiles.is_empty() {
            continue;
        }
        ctx.current(ID, browser.display);
        let running = count_running_images(browser.images) > 0;
        let display_names = profile_display_names(&root, firefox, &profiles);
        // Two profiles with the same visible name ("Sujit" twice): add the folder so rows differ.
        let mut label_counts: HashMap<String, usize> = HashMap::new();
        for p in &profiles {
            let dir = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            *label_counts.entry(display_names.get(&dir).cloned().unwrap_or(dir)).or_default() += 1;
        }
        for profile in profiles {
            let profile_name = profile.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let base_label = display_names.get(&profile_name).cloned().unwrap_or_else(|| profile_name.clone());
            let profile_label = if label_counts.get(&base_label).copied().unwrap_or(0) > 1 && base_label != profile_name {
                format!("{base_label} ({profile_name})")
            } else {
                base_label
            };
            for (group, rel) in browser_groups(firefox) {
                let files = group_files(&profile, rel);
                if files.is_empty() {
                    continue;
                }
                let (_file_count, bytes) = totals(&files);
                let id = format!("browser:{}:{}:{}", browser.key, profile_name, group);
                let mut params = BTreeMap::new();
                params.insert("browser".to_string(), browser.display.to_string());
                params.insert("profile".to_string(), profile_label.clone());
                params.insert("profileFolder".to_string(), profile_name.clone());
                if running {
                    params.insert("running".to_string(), "true".to_string());
                }
                // One history / cookie store per profile: a file count would only confuse.
                items.push(item(id.clone(), &format!("browser.{group}"), params, 1, Some(bytes)));
                targets.insert(id, Target::Browser { browser: index, profile_dir: profile.clone(), group });
            }
        }
        ctx.found(ID, items.len() as u64);
    }
    Some((items, targets))
}

fn result(id: &str, status: CareFixStatus, reason: Option<&str>, freed: u64, undoable: bool) -> CareFixResult {
    CareFixResult {
        item_id: id.to_string(),
        category: ID,
        status,
        reason: reason.map(str::to_string),
        freed_bytes: freed,
        undoable,
    }
}

/// Permanently delete files (privacy traces are not sent to the Recycle Bin, where they would
/// remain readable). Locked files are skipped and reported as a partial fix.
fn delete_files(id: &str, files: Vec<(PathBuf, u64)>) -> CareFixResult {
    if files.is_empty() {
        return result(id, CareFixStatus::Skipped, Some("itemChanged"), 0, false);
    }
    let total = files.len();
    let mut freed = 0u64;
    let mut removed = 0usize;
    for (path, bytes) in files {
        if fs::remove_file(&path).is_ok() {
            removed += 1;
            freed += bytes;
        }
    }
    match removed {
        0 => result(id, CareFixStatus::Failed, Some("itemsLocked"), 0, false),
        n if n < total => result(id, CareFixStatus::Partial, Some("itemsLocked"), freed, false),
        _ => result(id, CareFixStatus::Fixed, None, freed, false),
    }
}

fn backup_dir() -> Option<PathBuf> {
    let base = env_dir("LOCALAPPDATA").or_else(|| env_dir("APPDATA"))?;
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
    Some(base.join("PC Toolkit Pro").join("care-backups").join(stamp))
}

/// Export the MRU key to `.reg` (restorable by double-click), then remove it. No backup → no delete.
fn clear_mru(id: &str, subkey: &str, backup_root: &mut Option<PathBuf>) -> CareFixResult {
    let path = mru_path(subkey);
    if registry_trace_count(&path).unwrap_or(0) == 0 {
        return result(id, CareFixStatus::Skipped, Some("itemChanged"), 0, false);
    }
    if backup_root.is_none() {
        *backup_root = backup_dir();
    }
    let Some(dir) = backup_root.as_ref() else {
        return result(id, CareFixStatus::Failed, Some("backupFailed"), 0, false);
    };
    if fs::create_dir_all(dir).is_err() {
        return result(id, CareFixStatus::Failed, Some("backupFailed"), 0, false);
    }
    let file_name = format!("privacy-{}.reg", id.trim_start_matches("explorer:"));
    if export_registry_key(&path, &dir.join(file_name)).is_err() {
        return result(id, CareFixStatus::Failed, Some("backupFailed"), 0, false);
    }
    match delete_registry_tree(&path) {
        Ok(()) => result(id, CareFixStatus::Fixed, None, 0, true),
        Err(_) => result(id, CareFixStatus::Failed, Some("failed"), 0, false),
    }
}

impl CareCategory for PrivacyCategory {
    fn id(&self) -> CareCategoryId {
        ID
    }

    fn default_weight_ms(&self) -> u64 {
        800
    }

    fn scan(&self, ctx: &CareContext) -> CoreResult<CareCategoryScan> {
        if ctx.should_stop() {
            return Ok(skipped(ID));
        }
        let Some((items, targets)) = scan_items(ctx) else {
            return Ok(skipped(ID));
        };
        *TARGETS.lock().unwrap_or_else(|e| e.into_inner()) = Some(targets);
        Ok(CareCategoryScan::from_items(ID, items))
    }

    fn fix(&self, item_ids: &[String], ctx: &CareContext) -> CoreResult<Vec<CareFixResult>> {
        let targets = TARGETS.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let Some(targets) = targets else {
            return Ok(fix_all(ID, item_ids, CareFixStatus::Skipped, Some("unknownItem")));
        };
        let mut backup_root = None;
        let mut out = Vec::new();
        for id in item_ids {
            if ctx.should_stop() {
                out.push(result(id, CareFixStatus::Skipped, Some("cancelled"), 0, false));
                continue;
            }
            let Some(target) = targets.get(id) else {
                out.push(result(id, CareFixStatus::Skipped, Some("unknownItem"), 0, false));
                continue;
            };
            let outcome = match target {
                Target::RecentItems => delete_files(id, recent_item_files()),
                Target::JumpLists => delete_files(id, jump_list_files()),
                Target::Mru { subkey } => clear_mru(id, subkey, &mut backup_root),
                Target::Browser { browser, profile_dir, group } => {
                    let def = &BROWSERS[*browser];
                    ctx.current(ID, def.display);
                    // Re-check at fix time: deleting a database the browser holds open would fail
                    // or be rewritten from memory.
                    if count_running_images(def.images) > 0 {
                        result(id, CareFixStatus::Skipped, Some("runningProcesses"), 0, false)
                    } else {
                        delete_files(id, group_files(profile_dir, group_rel_files(def, group)))
                    }
                }
            };
            out.push(outcome);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn ends_with_ci_handles_non_ascii_names_without_panicking() {
        assert!(ends_with_ci("Report.LNK", ".lnk"));
        assert!(ends_with_ci("Résumé.lnk", ".lnk"));
        // The suffix boundary falls inside the multi-byte "é"; a str slice would panic here.
        assert!(!ends_with_ci("aé", "xé"));
        assert!(!ends_with_ci("日本語", ".lnk"));
        assert!(!ends_with_ci("k", ".lnk"));
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("pctoolkit-care-privacy-{tag}-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn chromium_profile_names_come_from_local_state() {
        let dir = std::env::temp_dir().join(format!("ptk-privacy-names-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("Default")).unwrap();
        fs::create_dir_all(dir.join("Profile 1")).unwrap();
        fs::write(
            dir.join("Local State"),
            r#"{"profile":{"info_cache":{"Default":{"name":"Sujit"},"Profile 1":{"name":"Work"}}}}"#,
        )
        .unwrap();
        let names = profile_display_names(&dir, false, &[dir.join("Default"), dir.join("Profile 1")]);
        assert_eq!(names.get("Default").map(String::as_str), Some("Sujit"));
        assert_eq!(names.get("Profile 1").map(String::as_str), Some("Work"));
        let ff = profile_display_names(&dir, true, &[dir.join("ab12.default-release")]);
        assert_eq!(ff.get("ab12.default-release").map(String::as_str), Some("default-release"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn groups_never_include_passwords_or_site_storage() {
        for firefox in [false, true] {
            for (_, files) in browser_groups(firefox) {
                for file in *files {
                    assert!(!is_never_touched(file), "{file} must never be a privacy target");
                }
            }
        }
        // Firefox history lives with bookmarks; it is intentionally not offered.
        assert!(browser_groups(true).iter().all(|(g, _)| *g != "history"));
        // Chromium form data shares Web Data with search engines / payment methods.
        assert!(browser_groups(false).iter().all(|(g, _)| *g != "formData"));
        // The runtime guard rejects protected stores even when asked for them directly.
        let dir = temp_dir("guard");
        fs::write(dir.join("Login Data"), b"x").unwrap();
        fs::create_dir_all(dir.join("Local Storage")).unwrap();
        fs::write(dir.join(r"Local Storage\leveldb"), b"x").unwrap();
        assert!(group_files(&dir, &["Login Data", r"Local Storage\leveldb"]).is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn chromium_profiles_and_groups_are_discovered() {
        let root = temp_dir("chromium");
        for name in ["Default", "Profile 2", "System Profile", "Crashpad"] {
            fs::create_dir_all(root.join(name)).unwrap();
            fs::write(root.join(name).join("Preferences"), b"{}").unwrap();
        }
        fs::create_dir_all(root.join("Profile 3")).unwrap(); // no Preferences → not a profile
        let profiles = discover_profiles(&root, false);
        let names: Vec<_> = profiles.iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect();
        assert_eq!(names, vec!["Default", "Profile 2"]);

        let default = &profiles[0];
        fs::write(default.join("History"), vec![0u8; 100]).unwrap();
        fs::write(default.join("Login Data"), b"secret").unwrap();
        fs::create_dir_all(default.join("Network")).unwrap();
        fs::write(default.join(r"Network\Cookies"), vec![0u8; 10]).unwrap();
        let history = group_files(default, browser_groups(false)[0].1);
        assert_eq!(totals(&history), (1, 100));
        let cookies = group_files(default, browser_groups(false)[1].1);
        assert_eq!(totals(&cookies), (1, 10));

        let res = delete_files("x", history);
        assert_eq!(res.status, CareFixStatus::Fixed);
        assert_eq!(res.freed_bytes, 100);
        assert!(default.join("Login Data").is_file(), "passwords stay");
        assert_eq!(delete_files("x", Vec::new()).status, CareFixStatus::Skipped);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn firefox_profiles_need_prefs() {
        let root = temp_dir("firefox");
        fs::create_dir_all(root.join("abc.default-release")).unwrap();
        fs::write(root.join("abc.default-release").join("prefs.js"), b"").unwrap();
        fs::create_dir_all(root.join("empty")).unwrap();
        assert_eq!(discover_profiles(&root, true).len(), 1);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    #[ignore = "reads this machine's privacy traces (read-only)"]
    fn smoke_scan() {
        let started = Instant::now();
        let scan = PrivacyCategory.scan(&CareContext::detached(false)).unwrap();
        eprintln!(
            "privacy state={:?} items={} ids={:?} elapsed={:?}",
            scan.state,
            scan.items.len(),
            scan.items.iter().map(|i| (&i.id, i.count, i.bytes)).collect::<Vec<_>>(),
            started.elapsed()
        );
    }
}
