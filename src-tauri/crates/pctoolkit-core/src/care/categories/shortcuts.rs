//! Care category: Invalid shortcuts.
//!
//! Scans `.lnk` files on the Desktop (user + public), in Start Menu Programs (user + common) and
//! in Quick Launch (which holds the pinned-taskbar folder). A shortcut is reported only when its
//! target is an absolute path on a **local fixed drive that is mounted and readable** and the
//! target is definitely missing (`NotFound`). Everything else is unknown and not reported:
//! network / removable / unplugged / locked drives, UNC paths, URL and shell-folder shortcuts
//! (no file-system path), Windows Installer advertised shortcuts (Darwin id, no plain target),
//! and any lookup that fails for another reason.
//!
//! Fix moves the `.lnk` to the Recycle Bin (restorable). Public Desktop / common Start Menu need
//! an elevated process; that is reported honestly as `needsAdmin`.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use pctoolkit_platform::shortcut_batch::resolve_shortcuts;
use pctoolkit_platform::{fixed_drive_roots, recycle_path, resolve_shortcut, user_desktop_dir, PlatformError};

use crate::care::{
    fix_all, skipped, CareCategory, CareCategoryId, CareCategoryScan, CareContext, CareFixResult, CareFixStatus,
    CareItem, CareRisk,
};
use crate::shared::CoreResult;

const ID: CareCategoryId = CareCategoryId::Shortcuts;
/// Upper bound on examined shortcuts so a pathological Start Menu cannot stall the scan.
const MAX_SHORTCUTS: usize = 5_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ShortcutVerdict {
    Broken,
    Valid,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TargetPresence {
    Exists,
    Missing,
    Unknown,
}

/// Pure classification of one resolved shortcut.
/// `fixed_ready(letter)` is true for a mounted, readable local fixed drive.
pub(crate) fn classify_target(
    target: Option<&Path>,
    advertised: bool,
    fixed_ready: &dyn Fn(char) -> bool,
    presence: &dyn Fn(&Path) -> TargetPresence,
) -> ShortcutVerdict {
    if advertised {
        return ShortcutVerdict::Unknown;
    }
    let Some(target) = target else {
        return ShortcutVerdict::Unknown;
    };
    let text = target.to_string_lossy();
    let bytes = text.as_bytes();
    // Only `X:\...` absolute drive paths; UNC (`\\server`), device (`\\?\`) and relative paths
    // are unknown.
    if bytes.len() < 4 || !bytes[0].is_ascii_alphabetic() || bytes[1] != b':' || bytes[2] != b'\\' {
        return ShortcutVerdict::Unknown;
    }
    if text.contains('%') {
        // Unexpanded environment variable: we cannot tell where it points.
        return ShortcutVerdict::Unknown;
    }
    if !fixed_ready((bytes[0] as char).to_ascii_uppercase()) {
        return ShortcutVerdict::Unknown;
    }
    match presence(target) {
        TargetPresence::Exists => ShortcutVerdict::Valid,
        TargetPresence::Missing => ShortcutVerdict::Broken,
        TargetPresence::Unknown => ShortcutVerdict::Unknown,
    }
}

/// True when a Shell Link header declares a Windows Installer Darwin id (advertised shortcut).
/// Layout per MS-SHLLINK: HeaderSize (0x4C) at 0, LinkFlags at 0x14, `HasDarwinID` = 0x1000.
pub(crate) fn lnk_is_advertised(header: &[u8]) -> bool {
    if header.len() < 0x18 {
        return false;
    }
    let size = u32::from_le_bytes([header[0], header[1], header[2], header[3]]);
    let flags = u32::from_le_bytes([header[0x14], header[0x15], header[0x16], header[0x17]]);
    size == 0x4C && flags & 0x1000 != 0
}

fn read_header(link: &Path) -> Vec<u8> {
    let mut buf = vec![0u8; 0x4C];
    match fs::File::open(link).and_then(|mut f| f.read(&mut buf)) {
        Ok(n) => {
            buf.truncate(n);
            buf
        }
        Err(_) => Vec::new(),
    }
}

fn presence(path: &Path) -> TargetPresence {
    match fs::metadata(path) {
        Ok(_) => TargetPresence::Exists,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => TargetPresence::Missing,
        Err(_) => TargetPresence::Unknown,
    }
}

/// Fixed drive letters whose root can be listed right now (memoized per scan).
struct FixedDrives(HashMap<char, bool>);

impl FixedDrives {
    fn probe() -> Self {
        let map = fixed_drive_roots()
            .into_iter()
            .filter_map(|root| {
                let letter = root.to_string_lossy().chars().next()?.to_ascii_uppercase();
                Some((letter, fs::read_dir(&root).is_ok()))
            })
            .collect();
        Self(map)
    }

    fn ready(&self, letter: char) -> bool {
        self.0.get(&letter).copied().unwrap_or(false)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Place {
    Desktop,
    StartMenu,
    QuickLaunch,
}

impl Place {
    fn key(self) -> &'static str {
        match self {
            Self::Desktop => "desktop",
            Self::StartMenu => "startMenu",
            Self::QuickLaunch => "taskbar",
        }
    }
}

struct Root {
    dir: PathBuf,
    place: Place,
    common: bool,
    max_depth: u32,
}

fn env_dir(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).map(PathBuf::from).filter(|p| p.is_absolute())
}

fn roots() -> Vec<Root> {
    let mut out = Vec::new();
    if let Some(desktop) = user_desktop_dir() {
        out.push(Root { dir: desktop, place: Place::Desktop, common: false, max_depth: 0 });
    }
    if let Some(public) = env_dir("PUBLIC") {
        out.push(Root { dir: public.join("Desktop"), place: Place::Desktop, common: true, max_depth: 0 });
    }
    if let Some(roaming) = env_dir("APPDATA") {
        out.push(Root {
            dir: roaming.join(r"Microsoft\Windows\Start Menu\Programs"),
            place: Place::StartMenu,
            common: false,
            max_depth: 6,
        });
        out.push(Root {
            dir: roaming.join(r"Microsoft\Internet Explorer\Quick Launch"),
            place: Place::QuickLaunch,
            common: false,
            max_depth: 4,
        });
    }
    if let Some(program_data) = env_dir("ProgramData") {
        out.push(Root {
            dir: program_data.join(r"Microsoft\Windows\Start Menu\Programs"),
            place: Place::StartMenu,
            common: true,
            max_depth: 6,
        });
    }
    out
}

fn is_link_like(meta: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    {
        meta.file_type().is_symlink()
    }
}

fn collect_links(dir: &Path, depth: u32, max_depth: u32, out: &mut Vec<PathBuf>, ctx: &CareContext) {
    if out.len() >= MAX_SHORTCUTS || ctx.should_stop() {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(meta) = fs::symlink_metadata(entry.path()) else {
            continue;
        };
        let path = entry.path();
        if meta.is_dir() {
            if depth < max_depth && !is_link_like(&meta) {
                collect_links(&path, depth + 1, max_depth, out, ctx);
            }
        } else if meta.is_file()
            && path.extension().map(|e| e.eq_ignore_ascii_case("lnk")).unwrap_or(false)
            && out.len() < MAX_SHORTCUTS
        {
            out.push(path);
        }
    }
}

#[derive(Debug, Clone)]
struct Target {
    link: PathBuf,
    common: bool,
}

static TARGETS: Mutex<Option<HashMap<String, Target>>> = Mutex::new(None);

/// Stable id from the shortcut path (FNV-1a of the lower-cased path) so the path itself never
/// crosses the process boundary as an id.
fn link_id(link: &Path) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in link.to_string_lossy().to_lowercase().bytes() {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("lnk:{hash:016x}")
}

fn verdict_for(link: &Path, drives: &FixedDrives) -> (ShortcutVerdict, Option<PathBuf>) {
    let advertised = lnk_is_advertised(&read_header(link));
    let target = if advertised { None } else { resolve_shortcut(link) };
    let verdict = classify_target(target.as_deref(), advertised, &|l| drives.ready(l), &presence);
    (verdict, target)
}

pub struct ShortcutsCategory;

impl CareCategory for ShortcutsCategory {
    fn id(&self) -> CareCategoryId {
        ID
    }

    fn default_weight_ms(&self) -> u64 {
        2_500
    }

    fn scan(&self, ctx: &CareContext) -> CoreResult<CareCategoryScan> {
        if ctx.should_stop() {
            return Ok(skipped(ID));
        }
        let drives = FixedDrives::probe();
        let mut items = Vec::new();
        let mut targets = HashMap::new();
        for root in roots() {
            let mut links = Vec::new();
            collect_links(&root.dir, 0, root.max_depth, &mut links, ctx);
            let advertised: Vec<bool> = links.iter().map(|l| lnk_is_advertised(&read_header(l))).collect();
            let to_resolve: Vec<PathBuf> =
                links.iter().zip(&advertised).filter(|(_, adv)| !**adv).map(|(l, _)| l.clone()).collect();
            if let Some(first) = to_resolve.first() {
                ctx.current(ID, first.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default());
            }
            let mut resolved = resolve_shortcuts(&to_resolve, &|| ctx.should_stop()).into_iter();
            for (link, adv) in links.into_iter().zip(advertised) {
                if ctx.should_stop() {
                    return Ok(skipped(ID));
                }
                let target = if adv { None } else { resolved.next().flatten() };
                let verdict = classify_target(target.as_deref(), adv, &|l| drives.ready(l), &presence);
                if verdict != ShortcutVerdict::Broken {
                    continue;
                }
                let name = link.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                ctx.current(ID, name.clone());
                let id = link_id(&link);
                if targets.contains_key(&id) {
                    continue;
                }
                let mut params = BTreeMap::new();
                params.insert("place".to_string(), root.place.key().to_string());
                if let Some(t) = &target {
                    params.insert("target".to_string(), t.to_string_lossy().into_owned());
                }
                items.push(CareItem {
                    id: id.clone(),
                    category: ID,
                    title_key: None,
                    title: Some(name),
                    params,
                    detail_key: Some(format!("care.shortcuts.missingTarget.{}", root.place.key())),
                    detail: None,
                    location: Some(link.to_string_lossy().into_owned()),
                    bytes: None,
                    count: 1,
                    risk: CareRisk::Safe,
                    selected: !root.common || ctx.is_admin,
                    fixable: true,
                    needs_admin: root.common,
                    action: None,
                });
                targets.insert(id, Target { link: link.clone(), common: root.common });
                ctx.found(ID, items.len() as u64);
            }
        }
        *TARGETS.lock().unwrap_or_else(|e| e.into_inner()) = Some(targets);
        Ok(CareCategoryScan::from_items(ID, items))
    }

    fn fix(&self, item_ids: &[String], ctx: &CareContext) -> CoreResult<Vec<CareFixResult>> {
        let targets = TARGETS.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let Some(targets) = targets else {
            return Ok(fix_all(ID, item_ids, CareFixStatus::Skipped, Some("unknownItem")));
        };
        let drives = FixedDrives::probe();
        let mut out = Vec::new();
        for id in item_ids {
            let (status, reason, freed) = 'item: {
                if ctx.should_stop() {
                    break 'item (CareFixStatus::Skipped, Some("cancelled"), 0);
                }
                let Some(target) = targets.get(id) else {
                    break 'item (CareFixStatus::Skipped, Some("unknownItem"), 0);
                };
                if target.common && !ctx.is_admin {
                    break 'item (CareFixStatus::Skipped, Some("needsAdmin"), 0);
                }
                let Ok(meta) = fs::symlink_metadata(&target.link) else {
                    break 'item (CareFixStatus::Skipped, Some("itemChanged"), 0);
                };
                // Re-validate: the target may have come back (drive re-mounted, app reinstalled).
                if verdict_for(&target.link, &drives).0 != ShortcutVerdict::Broken {
                    break 'item (CareFixStatus::Skipped, Some("itemChanged"), 0);
                }
                match recycle_path(&target.link) {
                    Ok(()) => (CareFixStatus::Fixed, None, meta.len()),
                    Err(PlatformError::PermissionDenied) if !ctx.is_admin => {
                        (CareFixStatus::Skipped, Some("needsAdmin"), 0)
                    }
                    Err(PlatformError::PermissionDenied) => (CareFixStatus::Failed, Some("accessDenied"), 0),
                    Err(_) => (CareFixStatus::Failed, Some("failed"), 0),
                }
            };
            out.push(CareFixResult {
                item_id: id.clone(),
                category: ID,
                status,
                reason: reason.map(str::to_string),
                freed_bytes: freed,
                undoable: false,
            });
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    fn fixed_c(letter: char) -> bool {
        letter == 'C'
    }

    #[test]
    fn missing_target_on_ready_fixed_drive_is_broken() {
        let missing = |_: &Path| TargetPresence::Missing;
        let exists = |_: &Path| TargetPresence::Exists;
        let unknown = |_: &Path| TargetPresence::Unknown;
        let t = Path::new(r"C:\Program Files\Gone\app.exe");
        assert_eq!(classify_target(Some(t), false, &fixed_c, &missing), ShortcutVerdict::Broken);
        assert_eq!(classify_target(Some(t), false, &fixed_c, &exists), ShortcutVerdict::Valid);
        assert_eq!(classify_target(Some(t), false, &fixed_c, &unknown), ShortcutVerdict::Unknown);
    }

    #[test]
    fn non_fixed_and_non_path_targets_are_unknown() {
        let missing = |_: &Path| TargetPresence::Missing;
        // Removable / network / unplugged drive letter.
        assert_eq!(classify_target(Some(Path::new(r"E:\tool.exe")), false, &fixed_c, &missing), ShortcutVerdict::Unknown);
        // UNC share.
        assert_eq!(
            classify_target(Some(Path::new(r"\\server\share\tool.exe")), false, &fixed_c, &missing),
            ShortcutVerdict::Unknown
        );
        // URL / shell folder: no file-system target.
        assert_eq!(classify_target(None, false, &fixed_c, &missing), ShortcutVerdict::Unknown);
        // MSI advertised shortcut.
        assert_eq!(
            classify_target(Some(Path::new(r"C:\Windows\Installer\x.ico")), true, &fixed_c, &missing),
            ShortcutVerdict::Unknown
        );
        // Unexpanded variable / relative path.
        assert_eq!(classify_target(Some(Path::new(r"C:\%FOO%\a.exe")), false, &fixed_c, &missing), ShortcutVerdict::Unknown);
        assert_eq!(classify_target(Some(Path::new(r"app.exe")), false, &fixed_c, &missing), ShortcutVerdict::Unknown);
        // Lower-case drive letter is normalized.
        assert_eq!(classify_target(Some(Path::new(r"c:\gone.exe")), false, &fixed_c, &missing), ShortcutVerdict::Broken);
    }

    #[test]
    fn darwin_flag_is_read_from_header() {
        let mut header = vec![0u8; 0x4C];
        header[0] = 0x4C;
        assert!(!lnk_is_advertised(&header));
        header[0x15] = 0x10; // 0x1000 HasDarwinID
        assert!(lnk_is_advertised(&header));
        assert!(!lnk_is_advertised(&header[..0x10]));
        header[0] = 0x10;
        assert!(!lnk_is_advertised(&header), "not a shell link header");
    }

    #[test]
    fn ids_are_stable_and_case_insensitive() {
        assert_eq!(link_id(Path::new(r"C:\A\b.lnk")), link_id(Path::new(r"c:\a\B.LNK")));
        assert_ne!(link_id(Path::new(r"C:\A\b.lnk")), link_id(Path::new(r"C:\A\c.lnk")));
    }

    #[test]
    #[ignore = "reads this machine's shortcuts (read-only)"]
    fn smoke_scan() {
        let started = Instant::now();
        let scan = ShortcutsCategory.scan(&CareContext::detached(false)).unwrap();
        eprintln!("shortcuts state={:?} items={} elapsed={:?}", scan.state, scan.items.len(), started.elapsed());
    }
}
