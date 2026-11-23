//! Care category: Registry — a deliberately narrow, verifiable set of orphaned entries.
//!
//! Reported (all `Review`, never preselected):
//! * **Uninstall entries** whose uninstaller *and* install folder are gone (and whose icon file,
//!   when it points outside Windows, is gone too). MSI entries, patches (`ParentKeyName`), and
//!   nameless entries are never reported.
//! * **App Paths** entries whose registered executable is gone.
//! * **SharedDLLs** reference counts for files that no longer exist (HKLM, admin).
//!
//! Safety rules: only absolute paths on local **fixed** volumes are judged (removable / network /
//! unmounted drives are "unknown"), anything under the Windows directory is never judged (WOW64
//! redirection makes those checks unreliable), and environment variables must fully expand.
//! Not covered on purpose: COM / CLSID, file associations, `HKLM\SYSTEM`, MRU / history lists
//! (those are Privacy), "empty keys", and anything like registry "fragmentation".
//!
//! Fix re-validates the entry against the live registry first (changed → `itemChanged`), writes a
//! `reg export` backup to `%LOCALAPPDATA%\PC Toolkit Pro\care-backups\<timestamp>\` (no backup →
//! no delete), records an undo entry, and only then deletes.

use std::collections::HashMap;
use std::sync::Mutex;

use pctoolkit_platform::registry_settings::read_registry_data;
use pctoolkit_platform::{
    delete_registry_tree, delete_registry_value, export_registry_key, fixed_drive_roots, read_registry_value,
    read_uninstall_entry, registry_key_exists, registry_subkey_names, registry_values, uninstall_roots, RegistryKeyPath,
    RegistryRoot, RegistryView, UninstallEntry,
};

use crate::care::undo::{care_backups_dir, discard_record, record_change, CareUndoChange};
use crate::care::{
    skipped, CareCategory, CareCategoryId, CareCategoryScan, CareContext, CareFixResult, CareFixStatus, CareItem, CareRisk,
};
use crate::shared::CoreResult;

const ID: CareCategoryId = CareCategoryId::Registry;
const APP_PATHS: &str = "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\App Paths";
const SHARED_DLLS: &str = "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\SharedDLLs";

pub struct RegistryCategory;

// ---------------------------------------------------------------------------------------------
// Pure path judgement
// ---------------------------------------------------------------------------------------------

/// Filesystem / environment facts, injected so the rules are testable.
pub trait PathFacts {
    /// `C:` style drive is a present local fixed volume.
    fn fixed_volume(&self, drive_letter: char) -> bool;
    fn exists(&self, path: &str) -> bool;
    /// Lower-case Windows directory, e.g. `c:\windows`.
    fn windows_dir(&self) -> String;
    fn env(&self, name: &str) -> Option<String>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathVerdict {
    Present,
    Missing,
    /// Not judged (relative, network, removable, Windows directory, unexpanded variables…).
    Unknown,
}

/// Expand `%VAR%` references. 32-bit registry views get the x86 Program Files folders.
fn expand_env(raw: &str, view: RegistryView, facts: &dyn PathFacts) -> Option<String> {
    let mut out = String::new();
    let mut rest = raw;
    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let end = after.find('%')?;
        let name = &after[..end];
        let lookup = match (view, name.to_ascii_lowercase().as_str()) {
            (RegistryView::Wow32, "programfiles") => "ProgramFiles(x86)",
            (RegistryView::Wow32, "commonprogramfiles") => "CommonProgramFiles(x86)",
            _ => name,
        };
        out.push_str(&facts.env(lookup)?);
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    Some(out)
}

fn judge_expanded(path: &str, facts: &dyn PathFacts) -> PathVerdict {
    let bytes = path.as_bytes();
    if bytes.len() < 4 || !bytes[0].is_ascii_alphabetic() || bytes[1] != b':' || bytes[2] != b'\\' {
        return PathVerdict::Unknown;
    }
    if !facts.fixed_volume((bytes[0] as char).to_ascii_uppercase()) {
        return PathVerdict::Unknown;
    }
    let lower = path.to_ascii_lowercase();
    let windows = facts.windows_dir();
    if !windows.is_empty() && (lower == windows || lower.starts_with(&format!("{windows}\\"))) {
        return PathVerdict::Unknown;
    }
    if facts.exists(path) {
        PathVerdict::Present
    } else {
        PathVerdict::Missing
    }
}

/// Judge one path string (already stripped of arguments / icon index). For 32-bit views both the
/// x86 and the native expansion are tried and any existing one counts as present, because Windows
/// itself registers some 32-bit entries with native-only targets.
pub fn judge_path(raw: &str, view: RegistryView, facts: &dyn PathFacts) -> PathVerdict {
    let trimmed = raw.trim().trim_matches('"').trim();
    if trimmed.is_empty() {
        return PathVerdict::Unknown;
    }
    let mut views = vec![view];
    if view == RegistryView::Wow32 {
        views.push(RegistryView::Native64);
    }
    let mut verdict = PathVerdict::Unknown;
    for candidate in views {
        let Some(path) = expand_env(trimmed, candidate, facts) else {
            return PathVerdict::Unknown;
        };
        match judge_expanded(&path, facts) {
            PathVerdict::Present => return PathVerdict::Present,
            PathVerdict::Unknown => return PathVerdict::Unknown,
            PathVerdict::Missing => verdict = PathVerdict::Missing,
        }
    }
    verdict
}

/// Executable part of a command line: quoted prefix, or the first `.exe` ending at a boundary,
/// or the first token.
pub fn command_target(command: &str) -> String {
    let trimmed = command.trim();
    if let Some(rest) = trimmed.strip_prefix('"') {
        return rest.split('"').next().unwrap_or_default().to_string();
    }
    let lower = trimmed.to_ascii_lowercase();
    let mut from = 0;
    while let Some(pos) = lower[from..].find(".exe") {
        let end = from + pos + 4;
        if trimmed[end..].is_empty() || trimmed[end..].starts_with(' ') {
            return trimmed[..end].to_string();
        }
        from = end;
    }
    trimmed.split(' ').next().unwrap_or_default().to_string()
}

/// `DisplayIcon` without its `,index` suffix.
pub fn icon_target(icon: &str) -> String {
    let unquoted = icon.trim().trim_matches('"');
    match unquoted.rfind(',') {
        Some(idx) if unquoted[idx + 1..].trim().trim_start_matches('-').chars().all(|c| c.is_ascii_digit()) => {
            unquoted[..idx].trim().trim_matches('"').to_string()
        }
        _ => unquoted.to_string(),
    }
}

/// True when an uninstall entry is orphaned: its uninstaller is missing, its install folder is
/// missing or unset, and its icon (when judged) is missing too.
pub fn is_orphan_uninstall(entry: &UninstallEntry, view: RegistryView, facts: &dyn PathFacts) -> bool {
    if entry.display_name.trim().is_empty()
        || entry.windows_installer
        || !entry.parent_key_name.trim().is_empty()
        || entry.uninstall_string.trim().is_empty()
    {
        return false;
    }
    if judge_path(&command_target(&entry.uninstall_string), view, facts) != PathVerdict::Missing {
        return false;
    }
    if !entry.install_location.trim().is_empty()
        && judge_path(&entry.install_location, view, facts) != PathVerdict::Missing
    {
        return false;
    }
    if !entry.display_icon.trim().is_empty()
        && judge_path(&icon_target(&entry.display_icon), view, facts) == PathVerdict::Present
    {
        return false;
    }
    true
}

// ---------------------------------------------------------------------------------------------
// Live facts and Core-held scan state
// ---------------------------------------------------------------------------------------------

struct LiveFacts {
    fixed: Vec<char>,
    windows: String,
}

impl LiveFacts {
    fn new() -> Self {
        let fixed = fixed_drive_roots()
            .iter()
            .filter_map(|p| p.to_string_lossy().chars().next())
            .map(|c| c.to_ascii_uppercase())
            .collect();
        let windows = std::env::var("SystemRoot")
            .or_else(|_| std::env::var("windir"))
            .unwrap_or_default()
            .trim_end_matches('\\')
            .to_ascii_lowercase();
        Self { fixed, windows }
    }
}

impl PathFacts for LiveFacts {
    fn fixed_volume(&self, drive_letter: char) -> bool {
        self.fixed.contains(&drive_letter)
    }
    fn exists(&self, path: &str) -> bool {
        std::path::Path::new(path).exists()
    }
    fn windows_dir(&self) -> String {
        self.windows.clone()
    }
    fn env(&self, name: &str) -> Option<String> {
        std::env::var(name).ok()
    }
}

#[derive(Debug, Clone)]
enum Finding {
    UninstallKey(RegistryKeyPath),
    AppPathKey(RegistryKeyPath),
    SharedDll { key: RegistryKeyPath, value: String },
}

static LAST_SCAN: Mutex<Option<HashMap<String, Finding>>> = Mutex::new(None);

fn fnv(text: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in text.to_lowercase().bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{hash:016x}")
}

fn view_tag(key: &RegistryKeyPath) -> &'static str {
    match (key.root, key.view) {
        (RegistryRoot::LocalMachine, RegistryView::Native64) => "lm64",
        (RegistryRoot::LocalMachine, RegistryView::Wow32) => "lm32",
        (RegistryRoot::CurrentUser, RegistryView::Native64) => "cu64",
        (RegistryRoot::CurrentUser, RegistryView::Wow32) => "cu32",
    }
}

fn app_path_orphan(key: &RegistryKeyPath, facts: &dyn PathFacts) -> bool {
    read_registry_value(key, "")
        .is_some_and(|target| judge_path(&target, key.view, facts) == PathVerdict::Missing)
}

fn shared_dll_orphan(key: &RegistryKeyPath, value: &str, facts: &dyn PathFacts) -> bool {
    matches!(read_registry_data(key, value), Ok(Some(_))) && judge_path(value, key.view, facts) == PathVerdict::Missing
}

fn still_orphan(finding: &Finding, facts: &dyn PathFacts) -> bool {
    match finding {
        Finding::UninstallKey(key) => {
            read_uninstall_entry(key).is_some_and(|entry| is_orphan_uninstall(&entry, key.view, facts))
        }
        Finding::AppPathKey(key) => registry_key_exists(key) && app_path_orphan(key, facts),
        Finding::SharedDll { key, value } => shared_dll_orphan(key, value, facts),
    }
}

fn needs_admin(key: &RegistryKeyPath) -> bool {
    key.root == RegistryRoot::LocalMachine
}

fn item(id: String, kind: &str, name: String, location: String, admin: bool) -> CareItem {
    let mut params = std::collections::BTreeMap::new();
    params.insert("name".to_string(), name);
    CareItem {
        id,
        category: ID,
        title_key: Some(format!("care.registry.{kind}.title")),
        title: None,
        params,
        detail_key: Some(format!("care.registry.{kind}.detail")),
        detail: None,
        location: Some(location),
        bytes: None,
        count: 1,
        risk: CareRisk::Review,
        selected: false,
        fixable: true,
        needs_admin: admin,
        action: None,
    }
}

fn leaf(key: &RegistryKeyPath) -> String {
    key.subkey.rsplit('\\').next().unwrap_or_default().to_string()
}

fn scan_live(ctx: &CareContext) -> Option<(Vec<CareItem>, HashMap<String, Finding>)> {
    let facts = LiveFacts::new();
    let mut items = Vec::new();
    let mut findings = HashMap::new();
    let mut push = |id: String, finding: Finding, item: CareItem, items: &mut Vec<CareItem>| {
        findings.insert(id, finding);
        items.push(item);
        ctx.found(ID, items.len() as u64);
    };

    // (a) Orphaned uninstall entries.
    for root in uninstall_roots() {
        let Ok(names) = registry_subkey_names(&root) else { continue };
        for name in names {
            if ctx.should_stop() {
                return None;
            }
            let key = root.child(&name);
            let Some(entry) = read_uninstall_entry(&key) else { continue };
            ctx.current(ID, entry.display_name.clone());
            if is_orphan_uninstall(&entry, key.view, &facts) {
                let id = format!("uninstall:{}:{}", view_tag(&key), fnv(&key.subkey));
                let it = item(id.clone(), "orphanUninstall", entry.display_name.clone(), key.display(), needs_admin(&key));
                push(id, Finding::UninstallKey(key), it, &mut items);
            }
        }
    }

    // (b) App Paths pointing to missing executables.
    for root in [
        RegistryKeyPath::new(RegistryRoot::LocalMachine, RegistryView::Native64, APP_PATHS),
        RegistryKeyPath::new(RegistryRoot::LocalMachine, RegistryView::Wow32, APP_PATHS),
        RegistryKeyPath::new(RegistryRoot::CurrentUser, RegistryView::Native64, APP_PATHS),
    ] {
        let Ok(names) = registry_subkey_names(&root) else { continue };
        for name in names {
            if ctx.should_stop() {
                return None;
            }
            let key = root.child(&name);
            ctx.current(ID, name.clone());
            if app_path_orphan(&key, &facts) {
                let id = format!("appPath:{}:{}", view_tag(&key), fnv(&key.subkey));
                let it = item(id.clone(), "orphanAppPath", leaf(&key), key.display(), needs_admin(&key));
                push(id, Finding::AppPathKey(key), it, &mut items);
            }
        }
    }

    // (d) SharedDLLs reference counts for missing files.
    for key in [
        RegistryKeyPath::new(RegistryRoot::LocalMachine, RegistryView::Native64, SHARED_DLLS),
        RegistryKeyPath::new(RegistryRoot::LocalMachine, RegistryView::Wow32, SHARED_DLLS),
    ] {
        if ctx.should_stop() {
            return None;
        }
        ctx.current(ID, "SharedDLLs");
        let Ok(values) = registry_values(&key) else { continue };
        for value in values {
            if judge_path(&value.name, key.view, &facts) == PathVerdict::Missing {
                let id = format!("sharedDll:{}:{}", view_tag(&key), fnv(&value.name));
                let file = value.name.rsplit('\\').next().unwrap_or_default().to_string();
                let it = item(id.clone(), "orphanSharedDll", file, value.name.clone(), true);
                push(id, Finding::SharedDll { key: key.clone(), value: value.name }, it, &mut items);
            }
        }
    }
    Some((items, findings))
}

// ---------------------------------------------------------------------------------------------
// Fix
// ---------------------------------------------------------------------------------------------

fn result(item_id: &str, status: CareFixStatus, reason: Option<&str>, undoable: bool) -> CareFixResult {
    CareFixResult {
        item_id: item_id.to_string(),
        category: ID,
        status,
        reason: reason.map(str::to_string),
        freed_bytes: 0,
        undoable,
    }
}

fn delete_key_with_backup(item_id: &str, key: &RegistryKeyPath, stamp: &str) -> CareFixResult {
    let Some(dir) = care_backups_dir().map(|d| d.join(stamp)) else {
        return result(item_id, CareFixStatus::Failed, Some("backupFailed"), false);
    };
    if std::fs::create_dir_all(&dir).is_err() {
        return result(item_id, CareFixStatus::Failed, Some("backupFailed"), false);
    }
    let file_name = format!("registry-{}-{}.reg", view_tag(key), fnv(&key.subkey));
    let file = dir.join(&file_name);
    let backed_up = export_registry_key(key, &file).is_ok() && std::fs::metadata(&file).is_ok_and(|m| m.len() > 0);
    if !backed_up {
        return result(item_id, CareFixStatus::Failed, Some("backupFailed"), false);
    }
    let change = CareUndoChange::RegistryKeyDeleted { key: key.clone(), backup_file: format!("{stamp}\\{file_name}") };
    let Ok(record_id) = record_change(ID, item_id, needs_admin(key), change) else {
        return result(item_id, CareFixStatus::Failed, Some("undoUnavailable"), false);
    };
    match delete_registry_tree(key) {
        Ok(()) if !registry_key_exists(key) => result(item_id, CareFixStatus::Fixed, None, true),
        Ok(()) => {
            discard_record(&record_id);
            result(item_id, CareFixStatus::Failed, Some("failed"), false)
        }
        Err(e) => {
            discard_record(&record_id);
            let reason = if matches!(e, pctoolkit_platform::PlatformError::PermissionDenied) {
                "permissionDenied"
            } else {
                "failed"
            };
            result(item_id, CareFixStatus::Failed, Some(reason), false)
        }
    }
}

fn delete_shared_dll(item_id: &str, key: &RegistryKeyPath, value: &str) -> CareFixResult {
    let previous = match read_registry_data(key, value) {
        Ok(Some(data)) if data.is_writable() => data,
        Ok(Some(_)) => return result(item_id, CareFixStatus::Skipped, Some("unsupportedValueType"), false),
        Ok(None) => return result(item_id, CareFixStatus::Skipped, Some("itemChanged"), false),
        Err(_) => return result(item_id, CareFixStatus::Failed, Some("permissionDenied"), false),
    };
    let change = CareUndoChange::RegistryValue {
        key: key.clone(),
        name: value.to_string(),
        previous: Some(previous),
        applied: None,
    };
    let Ok(record_id) = record_change(ID, item_id, true, change) else {
        return result(item_id, CareFixStatus::Failed, Some("undoUnavailable"), false);
    };
    match delete_registry_value(key, value) {
        Ok(()) => result(item_id, CareFixStatus::Fixed, None, true),
        Err(_) => {
            discard_record(&record_id);
            result(item_id, CareFixStatus::Failed, Some("failed"), false)
        }
    }
}

impl CareCategory for RegistryCategory {
    fn id(&self) -> CareCategoryId {
        ID
    }

    fn default_weight_ms(&self) -> u64 {
        1_500
    }

    fn scan(&self, ctx: &CareContext) -> CoreResult<CareCategoryScan> {
        let Some((items, findings)) = scan_live(ctx) else {
            return Ok(skipped(ID));
        };
        *LAST_SCAN.lock().unwrap_or_else(|e| e.into_inner()) = Some(findings);
        Ok(CareCategoryScan::from_items(ID, items))
    }

    fn fix(&self, item_ids: &[String], ctx: &CareContext) -> CoreResult<Vec<CareFixResult>> {
        let findings = LAST_SCAN.lock().unwrap_or_else(|e| e.into_inner()).clone().unwrap_or_default();
        let facts = LiveFacts::new();
        let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
        let mut results = Vec::new();
        for id in item_ids {
            if ctx.should_stop() {
                results.push(result(id, CareFixStatus::Skipped, Some("cancelled"), false));
                continue;
            }
            let Some(finding) = findings.get(id) else {
                results.push(result(id, CareFixStatus::Skipped, Some("itemChanged"), false));
                continue;
            };
            if !still_orphan(finding, &facts) {
                results.push(result(id, CareFixStatus::Skipped, Some("itemChanged"), false));
                continue;
            }
            let admin = match finding {
                Finding::UninstallKey(key) | Finding::AppPathKey(key) => needs_admin(key),
                Finding::SharedDll { .. } => true,
            };
            if admin && !ctx.is_admin {
                results.push(result(id, CareFixStatus::Skipped, Some("needsAdmin"), false));
                continue;
            }
            let outcome = match finding {
                Finding::UninstallKey(key) | Finding::AppPathKey(key) => delete_key_with_backup(id, key, &stamp),
                Finding::SharedDll { key, value } => delete_shared_dll(id, key, value),
            };
            if outcome.status == CareFixStatus::Fixed {
                if let Some(map) = LAST_SCAN.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
                    map.remove(id);
                }
            }
            results.push(outcome);
        }
        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fake {
        existing: Vec<&'static str>,
    }

    impl PathFacts for Fake {
        fn fixed_volume(&self, drive_letter: char) -> bool {
            drive_letter == 'C'
        }
        fn exists(&self, path: &str) -> bool {
            self.existing.iter().any(|e| e.eq_ignore_ascii_case(path))
        }
        fn windows_dir(&self) -> String {
            "c:\\windows".into()
        }
        fn env(&self, name: &str) -> Option<String> {
            match name {
                "ProgramFiles" => Some("C:\\Program Files".into()),
                "ProgramFiles(x86)" => Some("C:\\Program Files (x86)".into()),
                _ => None,
            }
        }
    }

    fn entry(uninstall: &str, location: &str, icon: &str) -> UninstallEntry {
        UninstallEntry {
            display_name: "Example App".into(),
            uninstall_string: uninstall.into(),
            install_location: location.into(),
            display_icon: icon.into(),
            ..Default::default()
        }
    }

    const N: RegistryView = RegistryView::Native64;

    #[test]
    fn command_and_icon_targets() {
        assert_eq!(command_target("\"C:\\A B\\unins000.exe\" /S"), "C:\\A B\\unins000.exe");
        assert_eq!(command_target("C:\\Program Files\\App\\uninst.exe /S"), "C:\\Program Files\\App\\uninst.exe");
        assert_eq!(command_target("MsiExec.exe /X{GUID}"), "MsiExec.exe");
        assert_eq!(icon_target("\"C:\\App\\app.exe\",0"), "C:\\App\\app.exe");
        assert_eq!(icon_target("C:\\App\\app.ico"), "C:\\App\\app.ico");
        assert_eq!(icon_target("C:\\App\\app.exe,-101"), "C:\\App\\app.exe");
    }

    #[test]
    fn paths_are_judged_only_when_reliable() {
        let fs = Fake { existing: vec!["C:\\Present\\a.exe"] };
        assert_eq!(judge_path("C:\\Present\\a.exe", N, &fs), PathVerdict::Present);
        assert_eq!(judge_path("C:\\Gone\\a.exe", N, &fs), PathVerdict::Missing);
        assert_eq!(judge_path("D:\\Gone\\a.exe", N, &fs), PathVerdict::Unknown, "non-fixed volume");
        assert_eq!(judge_path("\\\\server\\share\\a.exe", N, &fs), PathVerdict::Unknown);
        assert_eq!(judge_path("C:\\Windows\\System32\\x.exe", N, &fs), PathVerdict::Unknown);
        assert_eq!(judge_path("a.exe", N, &fs), PathVerdict::Unknown);
        assert_eq!(judge_path("%UNKNOWN%\\a.exe", N, &fs), PathVerdict::Unknown);
        assert_eq!(judge_path("%ProgramFiles%\\Gone\\a.exe", RegistryView::Wow32, &Fake { existing: vec![] }), PathVerdict::Missing);
        let x86 = Fake { existing: vec!["C:\\Program Files (x86)\\App\\a.exe"] };
        assert_eq!(judge_path("%ProgramFiles%\\App\\a.exe", RegistryView::Wow32, &x86), PathVerdict::Present);
        // A 32-bit entry whose target only exists in the native folder is not orphaned.
        let native = Fake { existing: vec!["C:\\Program Files\\App\\a.exe"] };
        assert_eq!(judge_path("%ProgramFiles%\\App\\a.exe", RegistryView::Wow32, &native), PathVerdict::Present);
    }

    #[test]
    fn orphan_uninstall_requires_every_trace_to_be_gone() {
        let none = Fake { existing: vec![] };
        assert!(is_orphan_uninstall(&entry("\"C:\\Gone\\unins000.exe\"", "C:\\Gone", ""), N, &none));
        assert!(is_orphan_uninstall(&entry("C:\\Gone\\uninst.exe /S", "", "C:\\Gone\\app.exe,0"), N, &none));

        let folder = Fake { existing: vec!["C:\\Kept"] };
        assert!(!is_orphan_uninstall(&entry("C:\\Kept\\uninst.exe", "C:\\Kept", ""), N, &folder));
        let icon = Fake { existing: vec!["C:\\Other\\app.exe"] };
        assert!(!is_orphan_uninstall(&entry("C:\\Gone\\uninst.exe", "", "C:\\Other\\app.exe,0"), N, &icon));
        // Unknown locations never make an entry orphaned.
        assert!(!is_orphan_uninstall(&entry("D:\\Gone\\uninst.exe", "", ""), N, &none));
        assert!(!is_orphan_uninstall(&entry("C:\\Gone\\uninst.exe", "\\\\nas\\apps", ""), N, &none));
        assert!(!is_orphan_uninstall(&entry("MsiExec.exe /X{1}", "", ""), N, &none));

        let mut msi = entry("C:\\Gone\\uninst.exe", "", "");
        msi.windows_installer = true;
        assert!(!is_orphan_uninstall(&msi, N, &none));
        let mut patch = entry("C:\\Gone\\uninst.exe", "", "");
        patch.parent_key_name = "Parent".into();
        assert!(!is_orphan_uninstall(&patch, N, &none));
        let mut nameless = entry("C:\\Gone\\uninst.exe", "", "");
        nameless.display_name.clear();
        assert!(!is_orphan_uninstall(&nameless, N, &none));
    }

    #[test]
    #[ignore = "reads the live registry (read-only)"]
    fn smoke_registry_scan() {
        let ctx = CareContext::detached(pctoolkit_platform::is_user_admin());
        let started = std::time::Instant::now();
        let scan = RegistryCategory.scan(&ctx).unwrap();
        println!("registry: state={:?} items={} elapsed={:?}", scan.state, scan.items.len(), started.elapsed());
        for item in &scan.items {
            println!("  {} admin={}", item.title_key.as_deref().unwrap_or(""), item.needs_admin);
        }
    }
}
