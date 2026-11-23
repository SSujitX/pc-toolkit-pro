//! Installed program inventory: which entries are shown, their typed metadata, and how each one
//! is uninstalled.

use std::collections::HashSet;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Instant;

use pctoolkit_platform::{RegistryKeyPath, RegistryRoot, StorePackage, UninstallEntry};
use serde::Serialize;

use super::matching::{first_path, ProtectedRoots};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ProgramSource {
    Desktop,
    Store,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum InstallerKind {
    Msi,
    Inno,
    Nsis,
    Squirrel,
    Store,
    Other,
}

/// Program row sent to the UI.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledProgram {
    pub id: String,
    pub name: String,
    pub publisher: String,
    pub version: String,
    /// `YYYY-MM-DD` when known.
    pub install_date: Option<String>,
    pub size_bytes: u64,
    pub install_location: String,
    pub source: ProgramSource,
    pub installer_kind: InstallerKind,
    /// Machine-wide installs usually need administrator rights to uninstall and clean.
    pub machine_wide: bool,
    pub quiet_supported: bool,
    /// Shared runtime or framework (VC++ redistributable, .NET, WebView2, Windows App Runtime):
    /// other programs depend on it, so the UI hides it unless the user asks to see runtimes.
    pub is_runtime: bool,
    pub registry_path: Option<String>,
}

/// Ownership facts of one raw uninstall entry, whether listed or not (system components,
/// `ParentKeyName` children, updates, de-duplicated copies). Leftover scans use them only to
/// protect folders and names that something else still installed owns.
#[derive(Debug, Clone)]
pub(crate) struct UninstallEntryFacts {
    pub key: Option<RegistryKeyPath>,
    pub key_name: String,
    pub parent_key_name: String,
    pub display_name: String,
    pub display_version: String,
    pub publisher: String,
    pub install_location: String,
    pub install_dir: Option<PathBuf>,
}

impl UninstallEntryFacts {
    pub(crate) fn from_entry(entry: &UninstallEntry, protected: &ProtectedRoots) -> Self {
        Self {
            key: entry.key.clone(),
            key_name: entry.key_name.clone(),
            parent_key_name: entry.parent_key_name.trim().to_string(),
            display_name: entry.display_name.trim().to_string(),
            display_version: entry.display_version.trim().to_string(),
            publisher: entry.publisher.trim().to_string(),
            install_location: entry.install_location.clone(),
            install_dir: derive_install_dir(entry, protected),
        }
    }

    /// The entry describes the target program itself: its own key, a child entry the target
    /// owns (`ParentKeyName`), or a copy that listing de-duplicated (same name and version).
    pub(crate) fn belongs_to(&self, target: &UninstallEntry) -> bool {
        let same_key = self.key.is_some() && self.key == target.key;
        let child = !self.parent_key_name.is_empty()
            && self.parent_key_name.eq_ignore_ascii_case(target.key_name.trim());
        let duplicate = !self.display_name.is_empty()
            && self.display_name.eq_ignore_ascii_case(target.display_name.trim())
            && self.display_version.eq_ignore_ascii_case(target.display_version.trim());
        same_key || child || duplicate
    }
}

/// Internal record kept between listing and uninstalling.
#[derive(Debug, Clone)]
pub(crate) struct ProgramRecord {
    pub program: InstalledProgram,
    pub entry: Option<UninstallEntry>,
    pub store_full_name: Option<String>,
    pub store_family_name: Option<String>,
    /// Store package logo image on disk.
    pub store_logo: Option<PathBuf>,
    /// Best-known install folder (from `InstallLocation`, or derived from the uninstaller path).
    pub install_dir: Option<PathBuf>,
}

const SKIPPED_RELEASE_TYPES: &[&str] = &["update", "hotfix", "security update", "service pack"];

const RUNTIME_MARKERS: &[&str] = &[
    "visual c++",
    "redistributable",
    "desktop runtime",
    ".net runtime",
    ".net framework",
    "asp.net core",
    "webview2 runtime",
    "windows app runtime",
    "winappruntime",
    "directx",
    "vs_coreeditorfonts",
];

pub(crate) fn is_runtime_name(name: &str) -> bool {
    let lower = name.to_lowercase();
    RUNTIME_MARKERS.iter().any(|m| lower.contains(m))
}

/// "Update for ... (KB5001716)" style entries are Windows updates, not programs.
fn is_windows_update_name(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    upper.match_indices("KB").any(|(i, _)| {
        let digits = upper[i + 2..].chars().take_while(|c| c.is_ascii_digit()).count();
        digits >= 6 && (i == 0 || !upper.as_bytes()[i - 1].is_ascii_alphanumeric())
    })
}

/// Store package names that are internal identifiers ("Microsoft.Office.ActionsServer").
fn is_identifier_like(name: &str) -> bool {
    !name.contains(' ') && name.contains('.')
}

fn should_list(entry: &UninstallEntry) -> bool {
    !entry.display_name.trim().is_empty()
        && !is_windows_update_name(&entry.display_name)
        && !entry.system_component
        && entry.parent_key_name.trim().is_empty()
        && !SKIPPED_RELEASE_TYPES.contains(&entry.release_type.trim().to_ascii_lowercase().as_str())
        && (!entry.uninstall_string.trim().is_empty() || !entry.quiet_uninstall_string.trim().is_empty())
}

pub(crate) fn detect_installer_kind(entry: &UninstallEntry, is_nsis: impl Fn(&Path) -> bool) -> InstallerKind {
    let command = entry.uninstall_string.to_ascii_lowercase();
    if entry.windows_installer || command.contains("msiexec") {
        return InstallerKind::Msi;
    }
    if command.contains("update.exe") && command.contains("--uninstall") {
        return InstallerKind::Squirrel;
    }
    let exe = first_path(&entry.uninstall_string);
    let file_name = exe
        .as_ref()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    let is_inno_name = file_name.len() == 12
        && file_name.starts_with("unins")
        && file_name.ends_with(".exe")
        && file_name[5..8].chars().all(|c| c.is_ascii_digit());
    if is_inno_name || entry.key_name.ends_with("_is1") {
        return InstallerKind::Inno;
    }
    if let Some(exe) = exe {
        if is_nsis(&exe) {
            return InstallerKind::Nsis;
        }
    }
    InstallerKind::Other
}

/// NSIS executables embed the "Nullsoft" marker near the start of the file.
///
/// Reading up to 512 KB of every uninstaller on every listing is slow, so results are cached
/// per (path, length, modified time); a changed file is re-read. The cache is bounded.
pub(crate) fn file_has_nsis_marker(path: &Path) -> bool {
    let Ok(meta) = fs::metadata(path) else {
        return false;
    };
    let stamp = FileStamp {
        path: path.to_path_buf(),
        len: meta.len(),
        modified: meta.modified().ok(),
    };
    if let Some(found) = NSIS_CACHE.lock().unwrap_or_else(|e| e.into_inner()).get(&stamp) {
        return found;
    }
    let found = read_nsis_marker(path);
    NSIS_CACHE.lock().unwrap_or_else(|e| e.into_inner()).insert(stamp, found);
    found
}

fn read_nsis_marker(path: &Path) -> bool {
    let Ok(file) = fs::File::open(path) else {
        return false;
    };
    let mut buf = Vec::with_capacity(512 * 1024);
    if file.take(512 * 1024).read_to_end(&mut buf).is_err() {
        return false;
    }
    buf.windows(8).any(|w| w == b"Nullsoft")
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct FileStamp {
    path: PathBuf,
    len: u64,
    modified: Option<std::time::SystemTime>,
}

/// Bounded memo of boolean facts about files; cleared wholesale when full (entries are cheap
/// to recompute and the bound only guards against unbounded growth).
struct StampCache {
    entries: std::collections::HashMap<FileStamp, bool>,
    capacity: usize,
}

impl StampCache {
    fn new(capacity: usize) -> Self {
        Self { entries: std::collections::HashMap::new(), capacity }
    }

    fn get(&self, stamp: &FileStamp) -> Option<bool> {
        self.entries.get(stamp).copied()
    }

    fn insert(&mut self, stamp: FileStamp, value: bool) {
        if self.entries.len() >= self.capacity && !self.entries.contains_key(&stamp) {
            self.entries.clear();
        }
        self.entries.insert(stamp, value);
    }
}

const NSIS_CACHE_CAPACITY: usize = 2048;
static NSIS_CACHE: std::sync::LazyLock<std::sync::Mutex<StampCache>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(StampCache::new(NSIS_CACHE_CAPACITY)));

/// MSI product code from the key name (`{GUID}`) or the uninstall string (`/I{GUID}` / `/X{GUID}`).
pub(crate) fn msi_product_code(entry: &UninstallEntry) -> Option<String> {
    let is_guid = |s: &str| s.len() == 38 && s.starts_with('{') && s.ends_with('}');
    if is_guid(&entry.key_name) {
        return Some(entry.key_name.clone());
    }
    let text = &entry.uninstall_string;
    let start = text.find('{')?;
    let candidate = text.get(start..start + 38)?;
    is_guid(candidate).then(|| candidate.to_string())
}

/// The command used to uninstall. `quiet` asks for the vendor's unattended mode when the installer
/// framework has a documented switch; otherwise the normal interactive uninstaller runs.
pub(crate) fn uninstall_command(entry: &UninstallEntry, kind: InstallerKind, quiet: bool) -> Option<String> {
    let base = entry.uninstall_string.trim();
    if kind == InstallerKind::Msi {
        if let Some(code) = msi_product_code(entry) {
            // /qb shows progress without prompts; the interactive form keeps the full MSI UI.
            return Some(if quiet {
                format!("MsiExec.exe /X{code} /qb /norestart")
            } else {
                format!("MsiExec.exe /X{code}")
            });
        }
    }
    if quiet {
        let vendor_quiet = entry.quiet_uninstall_string.trim();
        if !vendor_quiet.is_empty() {
            return Some(vendor_quiet.to_string());
        }
        let switch = match kind {
            InstallerKind::Inno => Some("/SILENT /SUPPRESSMSGBOXES /NORESTART"),
            InstallerKind::Nsis => Some("/S"),
            InstallerKind::Squirrel => Some("-s"),
            _ => None,
        };
        if let (Some(switch), false) = (switch, base.is_empty()) {
            let lower = base.to_ascii_lowercase();
            if !lower.contains(&switch.split(' ').next().unwrap_or_default().to_ascii_lowercase()) {
                return Some(format!("{base} {switch}"));
            }
            return Some(base.to_string());
        }
    }
    if base.is_empty() {
        let vendor_quiet = entry.quiet_uninstall_string.trim();
        return (!vendor_quiet.is_empty()).then(|| vendor_quiet.to_string());
    }
    Some(base.to_string())
}

pub(crate) fn quiet_supported(entry: &UninstallEntry, kind: InstallerKind) -> bool {
    !entry.quiet_uninstall_string.trim().is_empty()
        || matches!(
            kind,
            InstallerKind::Msi | InstallerKind::Inno | InstallerKind::Nsis | InstallerKind::Squirrel
        )
}

/// `YYYYMMDD` → `YYYY-MM-DD`; anything else → `None`.
pub(crate) fn format_install_date(raw: &str) -> Option<String> {
    let digits: String = raw.trim().chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() != 8 {
        return None;
    }
    let (y, m, d) = (&digits[0..4], &digits[4..6], &digits[6..8]);
    let month: u32 = m.parse().ok()?;
    let day: u32 = d.parse().ok()?;
    ((1..=12).contains(&month) && (1..=31).contains(&day) && y > "1990").then(|| format!("{y}-{m}-{d}"))
}

/// The folder a program lives in: `InstallLocation`, else the folder of its uninstaller or icon
/// when that folder is not a protected root (MsiExec in System32 is ignored this way). The result
/// is the normalized policy form; an ambiguous spelling (`..`, unexpandable 8.3 alias, trailing
/// dot) yields no install folder at all rather than a raw path that could alias a protected root.
pub(crate) fn derive_install_dir(entry: &UninstallEntry, protected: &ProtectedRoots) -> Option<PathBuf> {
    let usable = |path: &Path| protected.normalize(path).filter(|p| !protected.is_protected(p));
    let declared = entry.install_location.trim().trim_matches('"');
    if !declared.is_empty() {
        return usable(Path::new(declared));
    }
    for source in [&entry.uninstall_string, &entry.display_icon] {
        if let Some(parent) = first_path(source).and_then(|p| p.parent().map(Path::to_path_buf)) {
            if let Some(dir) = usable(&parent) {
                return Some(dir);
            }
        }
    }
    None
}

/// Size of a folder tree without following junctions; stops at `budget` entries.
pub(crate) fn folder_size(path: &Path, budget: &mut u32) -> u64 {
    let mut total = 0u64;
    let mut stack = vec![path.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            if *budget == 0 {
                return total;
            }
            *budget -= 1;
            let Ok(meta) = entry.metadata() else { continue };
            if is_reparse(&meta) {
                continue;
            }
            if meta.is_dir() {
                stack.push(entry.path());
            } else {
                total = total.saturating_add(meta.len());
            }
        }
    }
    total
}

#[cfg(windows)]
pub(crate) fn is_reparse(meta: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    meta.file_attributes() & 0x400 != 0
}

#[cfg(not(windows))]
pub(crate) fn is_reparse(meta: &fs::Metadata) -> bool {
    meta.file_type().is_symlink()
}

/// Rebuild the uninstall key from a desktop program id (`d:<lm|cu>:<64|32>:<leaf>`).
pub(crate) fn key_from_id(id: &str) -> Option<RegistryKeyPath> {
    let mut parts = id.splitn(4, ':');
    if parts.next()? != "d" {
        return None;
    }
    let root = match parts.next()? {
        "lm" => RegistryRoot::LocalMachine,
        "cu" => RegistryRoot::CurrentUser,
        _ => return None,
    };
    let view = match parts.next()? {
        "64" => pctoolkit_platform::RegistryView::Native64,
        "32" => pctoolkit_platform::RegistryView::Wow32,
        _ => return None,
    };
    let leaf = parts.next().filter(|l| !l.is_empty() && !l.contains('\\'))?;
    Some(RegistryKeyPath::new(
        root,
        view,
        format!("{}\\{leaf}", pctoolkit_platform::UNINSTALL_SUBKEY),
    ))
}

/// A record for a program that is already gone (removed outside the app), enough to scan leftovers.
pub(crate) fn record_from_snapshot(
    p: &super::snapshot::SnapshotProgram,
    protected: &ProtectedRoots,
) -> ProgramRecord {
    let install_dir = {
        let declared = p.install_location.trim();
        (!declared.is_empty())
            .then(|| protected.normalize(Path::new(declared)))
            .flatten()
            .filter(|d| !protected.is_protected(d))
    };
    let key = key_from_id(&p.id);
    ProgramRecord {
        program: InstalledProgram {
            id: p.id.clone(),
            name: p.name.clone(),
            publisher: p.publisher.clone(),
            version: p.version.clone(),
            install_date: None,
            size_bytes: 0,
            install_location: p.install_location.clone(),
            source: ProgramSource::Desktop,
            installer_kind: InstallerKind::Other,
            machine_wide: key.as_ref().is_some_and(|k| k.root == RegistryRoot::LocalMachine),
            quiet_supported: false,
            is_runtime: false,
            registry_path: key.as_ref().map(RegistryKeyPath::display),
        },
        entry: Some(UninstallEntry {
            key,
            key_name: p.key_name.clone(),
            display_name: p.name.clone(),
            publisher: p.publisher.clone(),
            install_location: p.install_location.clone(),
            ..Default::default()
        }),
        store_full_name: None,
        store_family_name: None,
        store_logo: None,
        install_dir,
    }
}

fn entry_id(key: &RegistryKeyPath) -> String {
    let root = match key.root {
        RegistryRoot::LocalMachine => "lm",
        RegistryRoot::CurrentUser => "cu",
    };
    let view = match key.view {
        pctoolkit_platform::RegistryView::Native64 => "64",
        pctoolkit_platform::RegistryView::Wow32 => "32",
    };
    let leaf = key.subkey.rsplit('\\').next().unwrap_or_default();
    format!("d:{root}:{view}:{leaf}")
}

/// Builds desktop program records. `on_entry(index, total, record)` reports each program as soon as
/// its record is built so the UI can show live progress.
pub(crate) fn desktop_records(
    entries: Vec<UninstallEntry>,
    protected: &ProtectedRoots,
    on_entry: &mut dyn FnMut(usize, usize, &ProgramRecord),
) -> Vec<ProgramRecord> {
    let started = Instant::now();
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    let listed: Vec<UninstallEntry> = entries.into_iter().filter(should_list).collect();
    let total = listed.len();
    for (index, entry) in listed.into_iter().enumerate() {
        let dedupe = format!(
            "{}|{}",
            entry.display_name.trim().to_lowercase(),
            entry.display_version.trim().to_lowercase()
        );
        if !seen.insert(dedupe) {
            continue;
        }
        let Some(key) = entry.key.clone() else { continue };
        let kind = detect_installer_kind(&entry, file_has_nsis_marker);
        let install_dir = derive_install_dir(&entry, protected);
        let mut size_bytes = entry.estimated_size_kb.saturating_mul(1024);
        // Measure only when the installer did not report a size, within a global time budget so
        // listing stays responsive on machines with hundreds of programs.
        if size_bytes == 0 && started.elapsed().as_secs() < 2 {
            if let Some(dir) = install_dir.as_ref().filter(|d| d.is_dir()) {
                let mut budget = 40_000;
                size_bytes = folder_size(dir, &mut budget);
            }
        }
        let program = InstalledProgram {
            id: entry_id(&key),
            name: entry.display_name.trim().to_string(),
            publisher: entry.publisher.trim().to_string(),
            version: entry.display_version.trim().to_string(),
            install_date: format_install_date(&entry.install_date).or_else(|| {
                (entry.key_written_ms > 0)
                    .then(|| chrono::DateTime::from_timestamp_millis(entry.key_written_ms))
                    .flatten()
                    .map(|d| d.with_timezone(&chrono::Local).format("%Y-%m-%d").to_string())
            }),
            size_bytes,
            install_location: install_dir
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default(),
            source: ProgramSource::Desktop,
            installer_kind: kind,
            machine_wide: key.root == RegistryRoot::LocalMachine,
            quiet_supported: quiet_supported(&entry, kind),
            is_runtime: is_runtime_name(&entry.display_name),
            registry_path: Some(key.display()),
        };
        out.push(ProgramRecord {
            program,
            entry: Some(entry),
            store_full_name: None,
            store_family_name: None,
            store_logo: None,
            install_dir,
        });
        if let Some(record) = out.last() {
            on_entry(index, total, record);
        }
    }
    out
}

pub(crate) fn store_records(packages: Vec<StorePackage>) -> Vec<ProgramRecord> {
    packages
        .into_iter()
        .filter(|p| {
            !p.is_framework
                && !p.is_resource
                && !p.is_system
                && !p.display_name.trim().is_empty()
                && !p.display_name.starts_with("ms-resource:")
                && !is_identifier_like(p.display_name.trim())
        })
        .map(|p| {
            let install_date = (p.installed_at_ms > 0)
                .then(|| chrono::DateTime::from_timestamp_millis(p.installed_at_ms))
                .flatten()
                .map(|d| d.format("%Y-%m-%d").to_string());
            ProgramRecord {
                program: InstalledProgram {
                    id: format!("s:{}", p.full_name),
                    name: p.display_name.trim().to_string(),
                    publisher: p.publisher.trim().to_string(),
                    version: p.version.clone(),
                    install_date,
                    size_bytes: 0,
                    install_location: p.install_path.clone(),
                    source: ProgramSource::Store,
                    installer_kind: InstallerKind::Store,
                    machine_wide: false,
                    quiet_supported: true,
                    is_runtime: is_runtime_name(&p.display_name),
                    registry_path: None,
                },
                entry: None,
                store_full_name: Some(p.full_name),
                store_family_name: Some(p.family_name),
                store_logo: (!p.logo_path.is_empty()).then(|| PathBuf::from(&p.logo_path)),
                install_dir: None,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(uninstall: &str) -> UninstallEntry {
        UninstallEntry {
            display_name: "App".into(),
            uninstall_string: uninstall.into(),
            ..Default::default()
        }
    }

    #[test]
    fn detects_installer_families() {
        let no_nsis = |_: &Path| false;
        assert_eq!(
            detect_installer_kind(&entry("MsiExec.exe /I{11111111-2222-3333-4444-555555555555}"), no_nsis),
            InstallerKind::Msi
        );
        assert_eq!(
            detect_installer_kind(&entry("\"C:\\Program Files\\App\\unins000.exe\""), no_nsis),
            InstallerKind::Inno
        );
        assert_eq!(
            detect_installer_kind(
                &entry("C:\\Users\\u\\AppData\\Local\\app\\Update.exe --uninstall"),
                no_nsis
            ),
            InstallerKind::Squirrel
        );
        assert_eq!(
            detect_installer_kind(&entry("C:\\Program Files\\App\\uninst.exe"), |_: &Path| true),
            InstallerKind::Nsis
        );
        assert_eq!(
            detect_installer_kind(&entry("C:\\Program Files\\App\\remove.exe"), no_nsis),
            InstallerKind::Other
        );
    }

    #[test]
    fn msi_commands_use_product_code_and_x_switch() {
        let mut e = entry("MsiExec.exe /I{11111111-2222-3333-4444-555555555555}");
        e.windows_installer = true;
        assert_eq!(
            uninstall_command(&e, InstallerKind::Msi, false).as_deref(),
            Some("MsiExec.exe /X{11111111-2222-3333-4444-555555555555}")
        );
        assert_eq!(
            uninstall_command(&e, InstallerKind::Msi, true).as_deref(),
            Some("MsiExec.exe /X{11111111-2222-3333-4444-555555555555} /qb /norestart")
        );
    }

    #[test]
    fn quiet_switches_follow_framework_and_vendor_string() {
        let inno = entry("\"C:\\App\\unins000.exe\"");
        assert_eq!(
            uninstall_command(&inno, InstallerKind::Inno, true).as_deref(),
            Some("\"C:\\App\\unins000.exe\" /SILENT /SUPPRESSMSGBOXES /NORESTART")
        );
        let already = entry("\"C:\\App\\unins000.exe\" /SILENT");
        assert_eq!(
            uninstall_command(&already, InstallerKind::Inno, true).as_deref(),
            Some("\"C:\\App\\unins000.exe\" /SILENT")
        );
        let mut vendor = entry("\"C:\\App\\uninstall.exe\"");
        vendor.quiet_uninstall_string = "\"C:\\App\\uninstall.exe\" --quiet".into();
        assert_eq!(
            uninstall_command(&vendor, InstallerKind::Other, true).as_deref(),
            Some("\"C:\\App\\uninstall.exe\" --quiet")
        );
        let other = entry("\"C:\\App\\remove.exe\"");
        assert_eq!(
            uninstall_command(&other, InstallerKind::Other, true).as_deref(),
            Some("\"C:\\App\\remove.exe\"")
        );
        assert!(!quiet_supported(&other, InstallerKind::Other));
    }

    #[test]
    fn classifies_updates_runtimes_and_identifiers() {
        assert!(is_windows_update_name("Update for x64-based Windows Systems (KB5001716)"));
        assert!(is_windows_update_name("KB4023057"));
        assert!(!is_windows_update_name("KB926L 1.0.3"));
        assert!(!is_windows_update_name("Keyboard KB12"));
        assert!(is_runtime_name("Microsoft Visual C++ v14 Redistributable (x64) - 14.51"));
        assert!(is_runtime_name("Microsoft Windows Desktop Runtime - 8.0.23 (x64)"));
        assert!(is_runtime_name("Microsoft Edge WebView2 Runtime"));
        assert!(!is_runtime_name("Node.js"));
        assert!(is_identifier_like("Microsoft.Office.ActionsServer"));
        assert!(!is_identifier_like("Node.js Tools"));
    }

    #[test]
    fn rebuilds_uninstall_key_from_id() {
        let key = key_from_id("d:lm:32:{ABC}_is1").expect("key");
        assert_eq!(key.root, RegistryRoot::LocalMachine);
        assert_eq!(key.view, pctoolkit_platform::RegistryView::Wow32);
        assert!(key.subkey.ends_with("\\Uninstall\\{ABC}_is1"));
        assert!(key_from_id("s:Some.Package").is_none());
        assert!(key_from_id("d:xx:64:a").is_none());
        assert!(key_from_id("d:cu:64:").is_none());
    }

    #[test]
    fn formats_install_dates() {
        assert_eq!(format_install_date("20261103").as_deref(), Some("2026-11-03"));
        assert_eq!(format_install_date("2026113"), None);
        assert_eq!(format_install_date("20261340"), None);
        assert_eq!(format_install_date(""), None);
    }

    #[test]
    fn hides_components_updates_and_entries_without_uninstaller() {
        let mut component = entry("x.exe");
        component.system_component = true;
        assert!(!should_list(&component));
        let mut update = entry("x.exe");
        update.parent_key_name = "Office".into();
        assert!(!should_list(&update));
        let mut hotfix = entry("x.exe");
        hotfix.release_type = "Security Update".into();
        assert!(!should_list(&hotfix));
        assert!(!should_list(&entry("")));
        assert!(should_list(&entry("x.exe")));
    }

    #[test]
    fn derive_install_dir_ignores_protected_system_folders() {
        let protected = ProtectedRoots::new(
            vec![PathBuf::from("C:\\Program Files")],
            vec![PathBuf::from("C:\\Windows")],
        );
        let msi = entry("C:\\Windows\\System32\\MsiExec.exe /X{1}");
        assert_eq!(derive_install_dir(&msi, &protected), None);
        let inno = entry("\"C:\\Program Files\\App\\unins000.exe\"");
        assert_eq!(
            derive_install_dir(&inno, &protected),
            Some(PathBuf::from("C:\\Program Files\\App"))
        );
        let mut root = entry("");
        root.install_location = "C:\\Program Files\\".into();
        assert_eq!(derive_install_dir(&root, &protected), None);
    }

    #[test]
    fn stamp_cache_is_bounded_and_keyed_by_file_identity() {
        let stamp = |n: u64| FileStamp { path: PathBuf::from(format!("C:\\x{n}.exe")), len: n, modified: None };
        let mut cache = StampCache::new(2);
        cache.insert(stamp(1), true);
        cache.insert(stamp(2), false);
        assert_eq!(cache.get(&stamp(1)), Some(true));
        assert_eq!(cache.get(&FileStamp { len: 99, ..stamp(1) }), None, "a changed file is a new key");
        cache.insert(stamp(3), true);
        assert!(cache.entries.len() <= 2);
        assert_eq!(cache.get(&stamp(3)), Some(true));
    }

    #[test]
    fn nsis_marker_is_detected_and_rechecked_after_a_change() {
        let dir = std::env::temp_dir().join(format!("pctk-nsis-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("uninst.exe");
        fs::write(&exe, b"MZ....Nullsoft Install System").unwrap();
        assert!(file_has_nsis_marker(&exe));
        assert!(file_has_nsis_marker(&exe), "cached answer");
        fs::write(&exe, b"MZ plain executable with another size").unwrap();
        assert!(!file_has_nsis_marker(&exe), "length changed, so the file is read again");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn entry_facts_recognize_the_target_its_children_and_duplicates_only() {
        let protected = ProtectedRoots::new(vec![], vec![]);
        let key = |leaf: &str| {
            Some(RegistryKeyPath::new(RegistryRoot::LocalMachine, pctoolkit_platform::RegistryView::Native64, leaf))
        };
        let mut target = entry("x.exe");
        target.key = key("Target");
        target.key_name = "Target".into();
        target.display_name = "Target App".into();
        target.display_version = "1.0".into();
        let facts = |f: &dyn Fn(&mut UninstallEntry)| {
            let mut e = entry("y.exe");
            e.key = key("Other");
            e.key_name = "Other".into();
            e.display_name = "Other App".into();
            e.display_version = "1.0".into();
            f(&mut e);
            UninstallEntryFacts::from_entry(&e, &protected)
        };
        assert!(facts(&|e| e.key = key("Target")).belongs_to(&target));
        assert!(facts(&|e| e.parent_key_name = "target".into()).belongs_to(&target));
        assert!(facts(&|e| e.display_name = "target app".into()).belongs_to(&target));
        // Hidden components of other products still count as "someone else".
        assert!(!facts(&|e| e.system_component = true).belongs_to(&target));
        assert!(!facts(&|e| e.parent_key_name = "Another".into()).belongs_to(&target));
        assert!(!facts(&|e| e.display_name = "Target App".into()).belongs_to(&{
            let mut t = target.clone();
            t.display_version = "2.0".into();
            t
        }));
    }

    #[test]
    fn derive_install_dir_rejects_aliases_of_protected_roots() {
        let protected = ProtectedRoots::new(
            vec![PathBuf::from("C:\\Program Files")],
            vec![PathBuf::from("C:\\Windows")],
        );
        for alias in ["C:\\PROGRA~1", "C:\\Program Files\\Vendor\\..", "C:\\Windows.", "C:\\Windows \\App"] {
            let mut e = entry("");
            e.install_location = alias.into();
            assert_eq!(derive_install_dir(&e, &protected), None, "{alias}");
        }
        let mut dotted = entry("");
        dotted.install_location = "C:/Program Files/./App/".into();
        assert_eq!(derive_install_dir(&dotted, &protected), Some(PathBuf::from("C:\\Program Files\\App")));
    }
}
