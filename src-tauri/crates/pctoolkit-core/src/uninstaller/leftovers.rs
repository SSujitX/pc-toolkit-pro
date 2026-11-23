//! Leftover detection after an uninstall (or for a forced removal).
//!
//! Every scanner is read-only. Candidates are tied to the removed program by an exact
//! normalized name or by referencing its install folder, then filtered through the protected-root
//! and shared-folder guards in `matching`.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use pctoolkit_platform::{
    list_scheduled_tasks, list_services, read_registry_value, registry_key_exists,
    registry_subkey_names, registry_values, shortcut_batch::resolve_shortcuts, user_desktop_dir,
    RegistryKeyPath, RegistryRoot, RegistryView,
};
use serde::Serialize;

use super::inventory::{folder_size, is_reparse};
use super::matching::{
    compact, is_distinctive, is_same_or_inside, is_shared_with, mentions_folder, path_key,
    NameSet, ProtectedRoots,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum LeftoverKind {
    Folder,
    File,
    RegistryKey,
    RegistryValue,
    Shortcut,
    StartupEntry,
    Service,
    ScheduledTask,
    FirewallRule,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum LeftoverConfidence {
    /// Directly owned by the program (its folder, its uninstall key, items pointing inside it).
    High,
    /// Name match only; shown unselected so the user decides.
    Medium,
}

/// What Core will do when the item is cleaned. Never sent to the frontend.
#[derive(Debug, Clone)]
pub(crate) enum LeftoverAction {
    RecyclePath(PathBuf),
    DeleteKey(RegistryKeyPath),
    DeleteValue(RegistryKeyPath, String),
    DeleteService(String),
    DeleteTask(String),
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LeftoverItem {
    pub id: String,
    pub kind: LeftoverKind,
    /// Path, registry key, service, or task name shown to the user.
    pub location: String,
    /// Extra context such as a registry value name or shortcut target.
    pub detail: Option<String>,
    pub bytes: u64,
    pub confidence: LeftoverConfidence,
    pub requires_admin: bool,
    pub selected: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct Leftover {
    pub item: LeftoverItem,
    pub action: LeftoverAction,
}

/// Facts about the program being removed, captured before its uninstaller ran.
#[derive(Debug, Clone, Default)]
pub(crate) struct ProgramSnapshot {
    pub names: NameSet,
    pub publisher: String,
    pub install_dir: Option<PathBuf>,
    pub uninstall_key: Option<RegistryKeyPath>,
    pub store_family_name: Option<String>,
}

/// Facts about everything else that stays installed.
#[derive(Debug, Clone, Default)]
pub(crate) struct OtherPrograms {
    pub install_dirs: Vec<PathBuf>,
    pub publishers: HashSet<String>,
    pub names: Vec<NameSet>,
}

impl OtherPrograms {
    fn owns_name(&self, candidate: &str) -> bool {
        self.names.iter().any(|set| set.matches(candidate))
    }
}

pub(crate) struct ScanContext<'a> {
    pub program: &'a ProgramSnapshot,
    pub others: &'a OtherPrograms,
    pub protected: &'a ProtectedRoots,
    pub admin_roots: Vec<PathBuf>,
}

struct Collector {
    items: Vec<Leftover>,
    seen: HashSet<String>,
}

impl Collector {
    fn push(
        &mut self,
        kind: LeftoverKind,
        location: String,
        detail: Option<String>,
        bytes: u64,
        confidence: LeftoverConfidence,
        requires_admin: bool,
        action: LeftoverAction,
    ) {
        // Values are identified by their registry value name: paired firewall rules (TCP + UDP)
        // share the displayed Name and App, so location + detail alone would drop one of them.
        let value_name = match &action {
            LeftoverAction::DeleteValue(_, name) => name.to_lowercase(),
            _ => String::new(),
        };
        let dedupe = format!(
            "{kind:?}|{}|{}|{value_name}",
            location.to_lowercase(),
            detail.clone().unwrap_or_default().to_lowercase()
        );
        if !self.seen.insert(dedupe) {
            return;
        }
        let id = format!("l{}", self.items.len() + 1);
        self.items.push(Leftover {
            item: LeftoverItem {
                id,
                kind,
                location,
                detail,
                bytes,
                confidence,
                requires_admin,
                selected: confidence == LeftoverConfidence::High,
            },
            action,
        });
    }

    fn has_path(&self, path: &Path) -> bool {
        self.items.iter().any(|l| match &l.action {
            LeftoverAction::RecyclePath(p) => is_same_or_inside(path, p),
            _ => false,
        })
    }
}

impl ScanContext<'_> {
    fn path_needs_admin(&self, path: &Path) -> bool {
        self.admin_roots.iter().any(|root| is_same_or_inside(path, root))
    }

    /// The path may be offered for deletion at all. The candidate is normalized first (8.3
    /// names, `.` / `..`, trailing dots) so no spelling of a protected or shared folder slips by.
    fn path_allowed(&self, path: &Path) -> bool {
        let Some(path) = self.protected.normalize(path) else { return false };
        !self.protected.is_protected(&path) && !is_shared_with(&path, &self.others.install_dirs)
    }
}

pub(crate) fn admin_roots_from_environment() -> Vec<PathBuf> {
    ["ProgramFiles", "ProgramFiles(x86)", "ProgramW6432", "ProgramData", "SystemRoot"]
        .iter()
        .filter_map(|k| std::env::var_os(k).map(PathBuf::from))
        .collect()
}

/// Run every scanner. `on_area` reports the scanner currently running (typed code for the UI).
/// `on_area(area, items_found, last_found_location)` reports the scanner currently running.
pub(crate) fn scan_leftovers(
    ctx: &ScanContext<'_>,
    on_area: &mut dyn FnMut(&'static str, usize, Option<&str>),
) -> Vec<Leftover> {
    let mut out = Collector {
        items: Vec::new(),
        seen: HashSet::new(),
    };
    fn report(
        out: &Collector,
        on_area: &mut dyn FnMut(&'static str, usize, Option<&str>),
        area: &'static str,
    ) {
        let last = out.items.last().map(|l| l.item.location.as_str());
        on_area(area, out.items.len(), last);
    }
    report(&out, on_area, "registry");
    scan_uninstall_key(ctx, &mut out);
    report(&out, on_area, "folders");
    scan_install_dir(ctx, &mut out);
    scan_named_folders(ctx, &mut out);
    report(&out, on_area, "registry");
    scan_software_keys(ctx, &mut out);
    scan_app_paths(ctx, &mut out);
    report(&out, on_area, "shortcuts");
    scan_shortcuts(ctx, &mut out);
    report(&out, on_area, "startup");
    scan_startup(ctx, &mut out);
    report(&out, on_area, "services");
    scan_services(ctx, &mut out);
    report(&out, on_area, "tasks");
    scan_tasks(ctx, &mut out);
    report(&out, on_area, "firewall");
    scan_firewall(ctx, &mut out);
    out.items
}

fn scan_uninstall_key(ctx: &ScanContext<'_>, out: &mut Collector) {
    let Some(key) = &ctx.program.uninstall_key else { return };
    if registry_key_exists(key) {
        out.push(
            LeftoverKind::RegistryKey,
            key.display(),
            None,
            0,
            LeftoverConfidence::High,
            key.root == RegistryRoot::LocalMachine,
            LeftoverAction::DeleteKey(key.clone()),
        );
    }
}

fn push_folder(ctx: &ScanContext<'_>, out: &mut Collector, path: PathBuf, confidence: LeftoverConfidence) {
    // Offer (and later remove) the normalized spelling that the policy actually checked.
    let Some(path) = ctx.protected.normalize(&path) else { return };
    if out.has_path(&path) || !ctx.path_allowed(&path) {
        return;
    }
    let Ok(meta) = fs::symlink_metadata(&path) else { return };
    if is_reparse(&meta) {
        return;
    }
    let (kind, bytes) = if meta.is_dir() {
        let mut budget = 200_000;
        (LeftoverKind::Folder, folder_size(&path, &mut budget))
    } else {
        (LeftoverKind::File, meta.len())
    };
    let requires_admin = ctx.path_needs_admin(&path);
    out.push(
        kind,
        path.to_string_lossy().into_owned(),
        None,
        bytes,
        confidence,
        requires_admin,
        LeftoverAction::RecyclePath(path),
    );
}

fn scan_install_dir(ctx: &ScanContext<'_>, out: &mut Collector) {
    if let Some(dir) = &ctx.program.install_dir {
        if dir.exists() {
            push_folder(ctx, out, dir.clone(), LeftoverConfidence::High);
        }
    }
}

fn data_roots() -> Vec<PathBuf> {
    let env = |k: &str| std::env::var_os(k).map(PathBuf::from);
    let mut roots = Vec::new();
    for key in ["ProgramFiles", "ProgramFiles(x86)", "ProgramData", "APPDATA", "LOCALAPPDATA"] {
        if let Some(path) = env(key) {
            roots.push(path);
        }
    }
    if let Some(local) = env("LOCALAPPDATA") {
        roots.push(local.join("Programs"));
    }
    if let Some(profile) = env("USERPROFILE") {
        roots.push(profile.join("AppData\\LocalLow"));
    }
    let mut seen = HashSet::new();
    roots.retain(|p| seen.insert(path_key(p)));
    roots
}

fn child_dirs(root: &Path) -> Vec<(String, PathBuf)> {
    let Ok(entries) = fs::read_dir(root) else { return Vec::new() };
    entries
        .flatten()
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .map(|e| (e.file_name().to_string_lossy().into_owned(), e.path()))
        .collect()
}

/// `<root>\<Product>` and `<root>\<Publisher>\<Product>` folders in the common data roots.
fn scan_named_folders(ctx: &ScanContext<'_>, out: &mut Collector) {
    let names = &ctx.program.names;
    if names.is_empty() {
        return;
    }
    let publisher = compact(&ctx.program.publisher);
    let publisher_usable = is_distinctive(&publisher);
    for root in data_roots() {
        for (name, path) in child_dirs(&root) {
            if names.matches(&name) {
                if !ctx.others.owns_name(&name) {
                    push_folder(ctx, out, path, LeftoverConfidence::High);
                }
                continue;
            }
            if publisher_usable && compact(&name) == publisher {
                let children = child_dirs(&path);
                for (child_name, child_path) in &children {
                    if names.matches(child_name) && !ctx.others.owns_name(child_name) {
                        push_folder(ctx, out, child_path.clone(), LeftoverConfidence::High);
                    }
                }
                // The vendor folder itself only when no other installed program shares the
                // publisher and nothing else of substance is inside it.
                let only_product = children.iter().all(|(n, _)| names.matches(n));
                if !ctx.others.publishers.contains(&publisher) && only_product {
                    push_folder(ctx, out, path, LeftoverConfidence::Medium);
                }
            }
        }
    }
    if let (Some(family), Some(local)) = (
        &ctx.program.store_family_name,
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from),
    ) {
        let packages = local.join("Packages").join(family);
        // `symlink_metadata`: a reparse point here is never followed or offered.
        let real_dir = fs::symlink_metadata(&packages).is_ok_and(|m| m.is_dir() && !is_reparse(&m));
        if real_dir {
            // Store app data folders live under the protected Packages root; the exact family
            // folder is owned by this package alone.
            let mut budget = 200_000;
            let bytes = folder_size(&packages, &mut budget);
            out.push(
                LeftoverKind::Folder,
                packages.to_string_lossy().into_owned(),
                None,
                bytes,
                LeftoverConfidence::High,
                false,
                LeftoverAction::RecyclePath(packages),
            );
        }
    }
}

fn software_roots() -> Vec<RegistryKeyPath> {
    vec![
        RegistryKeyPath::new(RegistryRoot::CurrentUser, RegistryView::Native64, "Software"),
        RegistryKeyPath::new(RegistryRoot::LocalMachine, RegistryView::Native64, "SOFTWARE"),
        RegistryKeyPath::new(RegistryRoot::LocalMachine, RegistryView::Wow32, "SOFTWARE"),
    ]
}

/// `Software\<Product>` and `Software\<Publisher>\<Product>` keys.
fn scan_software_keys(ctx: &ScanContext<'_>, out: &mut Collector) {
    let names = &ctx.program.names;
    if names.is_empty() {
        return;
    }
    let publisher = compact(&ctx.program.publisher);
    let publisher_usable = is_distinctive(&publisher);
    for root in software_roots() {
        let machine = root.root == RegistryRoot::LocalMachine;
        let Ok(children) = registry_subkey_names(&root) else { continue };
        for name in children {
            let key = root.child(&name);
            if names.matches(&name) && !ctx.others.owns_name(&name) {
                out.push(
                    LeftoverKind::RegistryKey,
                    key.display(),
                    None,
                    0,
                    LeftoverConfidence::High,
                    machine,
                    LeftoverAction::DeleteKey(key),
                );
                continue;
            }
            if publisher_usable && compact(&name) == publisher {
                let grand = registry_subkey_names(&key).unwrap_or_default();
                for child in &grand {
                    if names.matches(child) && !ctx.others.owns_name(child) {
                        let child_key = key.child(child);
                        out.push(
                            LeftoverKind::RegistryKey,
                            child_key.display(),
                            None,
                            0,
                            LeftoverConfidence::High,
                            machine,
                            LeftoverAction::DeleteKey(child_key),
                        );
                    }
                }
                let only_product = grand.iter().all(|n| names.matches(n));
                if !ctx.others.publishers.contains(&publisher) && only_product {
                    out.push(
                        LeftoverKind::RegistryKey,
                        key.display(),
                        None,
                        0,
                        LeftoverConfidence::Medium,
                        machine,
                        LeftoverAction::DeleteKey(key),
                    );
                }
            }
        }
    }
}

/// `App Paths\<exe>` registrations whose target lives in the program folder.
fn scan_app_paths(ctx: &ScanContext<'_>, out: &mut Collector) {
    let Some(dir) = ctx.program.install_dir.as_ref().filter(|d| ctx.path_allowed(d)) else { return };
    let sub = "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\App Paths";
    for root in [
        RegistryKeyPath::new(RegistryRoot::CurrentUser, RegistryView::Native64, sub),
        RegistryKeyPath::new(RegistryRoot::LocalMachine, RegistryView::Native64, sub),
        RegistryKeyPath::new(RegistryRoot::LocalMachine, RegistryView::Wow32, sub),
    ] {
        for name in registry_subkey_names(&root).unwrap_or_default() {
            let key = root.child(&name);
            let target = read_registry_value(&key, "").unwrap_or_default();
            if mentions_folder(&target, dir) {
                out.push(
                    LeftoverKind::RegistryKey,
                    key.display(),
                    Some(target),
                    0,
                    LeftoverConfidence::High,
                    root.root == RegistryRoot::LocalMachine,
                    LeftoverAction::DeleteKey(key),
                );
            }
        }
    }
}

fn shortcut_roots() -> Vec<(PathBuf, bool)> {
    let env = |k: &str| std::env::var_os(k).map(PathBuf::from);
    let mut roots = Vec::new();
    if let Some(appdata) = env("APPDATA") {
        roots.push((appdata.join("Microsoft\\Windows\\Start Menu\\Programs"), false));
        roots.push((appdata.join("Microsoft\\Internet Explorer\\Quick Launch\\User Pinned\\TaskBar"), false));
    }
    if let Some(data) = env("ProgramData") {
        roots.push((data.join("Microsoft\\Windows\\Start Menu\\Programs"), true));
    }
    if let Some(desktop) = user_desktop_dir() {
        roots.push((desktop, false));
    }
    if let Some(public) = env("PUBLIC") {
        roots.push((public.join("Desktop"), true));
    }
    roots
}

/// Start Menu / Desktop / taskbar shortcuts pointing into the program folder, or broken
/// shortcuts named after the program, plus Start Menu folders named after it.
fn scan_shortcuts(ctx: &ScanContext<'_>, out: &mut Collector) {
    let names = &ctx.program.names;
    let dir = ctx.program.install_dir.as_ref().filter(|d| ctx.path_allowed(d));
    // `.lnk` files are collected first and resolved in one batch: per-link COM setup made this
    // scanner slow on profiles with hundreds of shortcuts.
    let mut links: Vec<(PathBuf, u64, bool, String)> = Vec::new();
    for (root, machine) in shortcut_roots() {
        let mut stack = vec![(root.clone(), 0u32)];
        while let Some((folder, depth)) = stack.pop() {
            let Ok(entries) = fs::read_dir(&folder) else { continue };
            for entry in entries.flatten() {
                let path = entry.path();
                let Ok(meta) = entry.metadata() else { continue };
                if is_reparse(&meta) {
                    continue;
                }
                let file_name = entry.file_name().to_string_lossy().into_owned();
                if ctx.protected.is_protected(&path) {
                    if meta.is_dir() && depth < 2 {
                        stack.push((path, depth + 1));
                    }
                    continue;
                }
                if meta.is_dir() {
                    if depth == 0 && names.matches(&file_name) && !ctx.others.owns_name(&file_name) {
                        out.push(
                            LeftoverKind::Folder,
                            path.to_string_lossy().into_owned(),
                            None,
                            0,
                            LeftoverConfidence::High,
                            machine,
                            LeftoverAction::RecyclePath(path.clone()),
                        );
                    } else if depth < 2 {
                        stack.push((path, depth + 1));
                    }
                    continue;
                }
                if !file_name.to_ascii_lowercase().ends_with(".lnk") {
                    continue;
                }
                let stem = file_name[..file_name.len() - 4].to_string();
                links.push((path, meta.len(), machine, stem));
            }
        }
    }
    let paths: Vec<PathBuf> = links.iter().map(|(p, ..)| p.clone()).collect();
    let targets = resolve_shortcuts(&paths, &|| false);
    for ((path, len, machine, stem), target) in links.into_iter().zip(targets) {
        let points_inside = matches!((&target, dir), (Some(t), Some(d)) if is_same_or_inside(t, d));
        let broken_named = target.as_ref().is_some_and(|t| !t.exists()) && names.matches(&stem);
        if (points_inside || broken_named) && !out.has_path(&path) {
            out.push(
                LeftoverKind::Shortcut,
                path.to_string_lossy().into_owned(),
                target.map(|t| t.to_string_lossy().into_owned()),
                len,
                LeftoverConfidence::High,
                machine,
                LeftoverAction::RecyclePath(path),
            );
        }
    }
}

/// `Run` / `RunOnce` values that launch something from the program folder.
fn scan_startup(ctx: &ScanContext<'_>, out: &mut Collector) {
    let Some(dir) = ctx.program.install_dir.as_ref().filter(|d| ctx.path_allowed(d)) else { return };
    for (root, view) in [
        (RegistryRoot::CurrentUser, RegistryView::Native64),
        (RegistryRoot::LocalMachine, RegistryView::Native64),
        (RegistryRoot::LocalMachine, RegistryView::Wow32),
    ] {
        for leaf in ["Run", "RunOnce"] {
            let key = RegistryKeyPath::new(root, view, format!("SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\{leaf}"));
            for value in registry_values(&key).unwrap_or_default() {
                if mentions_folder(&value.text, dir) {
                    out.push(
                        LeftoverKind::StartupEntry,
                        key.display(),
                        Some(value.name.clone()),
                        0,
                        LeftoverConfidence::High,
                        root == RegistryRoot::LocalMachine,
                        LeftoverAction::DeleteValue(key.clone(), value.name),
                    );
                }
            }
        }
    }
}

fn scan_services(ctx: &ScanContext<'_>, out: &mut Collector) {
    let Some(dir) = ctx.program.install_dir.as_ref().filter(|d| ctx.path_allowed(d)) else { return };
    for service in list_services() {
        if mentions_folder(&service.image_path, dir) {
            out.push(
                LeftoverKind::Service,
                service.display_name.clone(),
                Some(service.name.clone()),
                0,
                LeftoverConfidence::High,
                true,
                LeftoverAction::DeleteService(service.name),
            );
        }
    }
}

fn scan_tasks(ctx: &ScanContext<'_>, out: &mut Collector) {
    let dir = ctx.program.install_dir.as_ref().filter(|d| ctx.path_allowed(d));
    let names = &ctx.program.names;
    for task in list_scheduled_tasks() {
        let leaf = task.name.rsplit('\\').next().unwrap_or_default().to_string();
        let by_folder = dir.is_some_and(|d| mentions_folder(&task.commands, d));
        // A task that only shares the program's name (without pointing into its folder) is a
        // medium-confidence hint the user confirms explicitly.
        let by_name = !by_folder
            && names.matches(&leaf)
            && !ctx.others.owns_name(&leaf);
        if by_folder || by_name {
            out.push(
                LeftoverKind::ScheduledTask,
                task.name.clone(),
                None,
                0,
                if by_folder { LeftoverConfidence::High } else { LeftoverConfidence::Medium },
                true,
                LeftoverAction::DeleteTask(task.name),
            );
        }
    }
}

fn scan_firewall(ctx: &ScanContext<'_>, out: &mut Collector) {
    let Some(dir) = ctx.program.install_dir.as_ref().filter(|d| ctx.path_allowed(d)) else { return };
    let key = RegistryKeyPath::new(
        RegistryRoot::LocalMachine,
        RegistryView::Native64,
        "SYSTEM\\CurrentControlSet\\Services\\SharedAccess\\Parameters\\FirewallPolicy\\FirewallRules",
    );
    for value in registry_values(&key).unwrap_or_default() {
        let app = value
            .text
            .split('|')
            .find_map(|part| part.strip_prefix("App="))
            .unwrap_or_default();
        if !app.is_empty() && mentions_folder(app, dir) {
            let label = value
                .text
                .split('|')
                .find_map(|part| part.strip_prefix("Name="))
                .unwrap_or(&value.name)
                .to_string();
            out.push(
                LeftoverKind::FirewallRule,
                label,
                Some(app.to_string()),
                0,
                LeftoverConfidence::High,
                true,
                LeftoverAction::DeleteValue(key.clone(), value.name),
            );
        }
    }
}
