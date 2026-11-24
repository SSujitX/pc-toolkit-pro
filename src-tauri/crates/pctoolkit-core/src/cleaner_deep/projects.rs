//! Project build artifacts (`projects` group): discovery, measurement, verified clean.
//!
//! Discovery walks the user's non-hidden home folders (not AppData, OneDrive, or
//! media folders) and well-known developer roots on fixed drives (`D:\code`,
//! `C:\src`, ...). A folder is a project when an ecosystem marker sits directly in it
//! (`package.json`, `Cargo.toml`, `*.csproj`, ...); its artifacts are real child
//! directories named by that ecosystem (`node_modules`, `target`, `bin`/`obj`, ...).
//!
//! Safety beyond the shared walk policy:
//! - the walk is bounded (depth / folders / projects / time) and reports
//!   `discoveryLimited` instead of running unbounded;
//! - link-like entries, hidden folders, VCS metadata, virtual environments, and
//!   artifact folders are never descended;
//! - an artifact must be a real directory strictly inside its project; nested
//!   artifacts keep only the shallowest; an artifact that holds `.git` is skipped;
//! - in a git repository root with a `.gitignore`, an artifact whose name the
//!   `.gitignore` never mentions is treated as possibly tracked and skipped;
//! - projects touched in the last 7 days (sources or `.git/index`) are reported
//!   `active` and are never preselected — no project ever is;
//! - execute re-discovers the project from its Core-held path (the frontend sends
//!   only ids), re-confirms the marker and every artifact, then deletes each
//!   artifact through `walk::clean_directory`.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use super::execute::exec_status;
use super::provider::{DeepCleanupProvider, ProviderContext};
use super::safety::{is_link_like, is_strict_descendant, path_key, RootPolicy, RootRefusal, SafetyContext, VCS_SEGMENTS};
use super::walk::{clean_directory, measure_directory, CleanMode, CleanTally, MeasureTally};
use super::{
    DeepCleanupExecReason, DeepCleanupExecStatus, DeepCleanupGroup, DeepCleanupRisk, DeepCleanupRuleExecResult,
    DeepCleanupRuleExtras, DeepCleanupRuleResult, DeepCleanupRuleStatus, DeepCleanupScanReason,
};
use crate::cleaner::CANCELLED;
use crate::shared::CoreResult;

pub(crate) const RULE_PREFIX: &str = "projects.";
const NAME_KEY: &str = "deepCleaner.projects.itemName";
const DETAIL_KEY: &str = "deepCleaner.projects.itemDetail";
/// A project modified within this window counts as active work.
const ACTIVE_WINDOW: Duration = Duration::from_secs(7 * 24 * 60 * 60);
/// Reuse one discovery between the projects provider and the rustup special kind
/// of the same scan.
const SHARED_DISCOVERY_TTL: Duration = Duration::from_secs(15);
/// Results beyond this many projects (largest first) are dropped.
const MAX_RESULTS: usize = 300;
const MEASURE_WORKERS: usize = 4;
const GITIGNORE_CAP: u64 = 64 * 1024;

const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
const FILE_ATTRIBUTE_SYSTEM: u32 = 0x4;

/// How an ecosystem recognizes a project folder (all names lowercase).
#[derive(Debug, Clone, Copy)]
enum Marker {
    File(&'static str),
    /// Any file with this suffix (`.csproj`).
    Suffix(&'static str),
    /// Every listed child folder exists.
    Dirs(&'static [&'static str]),
}

#[derive(Debug)]
pub(crate) struct Ecosystem {
    pub code: &'static str,
    markers: &'static [Marker],
    /// Relative artifact paths (`/`-separated). The first segment is matched
    /// case-insensitively against the project listing.
    artifacts: &'static [&'static str],
    /// Child folder name prefixes that are artifacts (`cmake-build-`).
    artifact_prefixes: &'static [&'static str],
    /// `__pycache__` folders anywhere below the project are artifacts.
    pycache: bool,
}

const fn eco(
    code: &'static str,
    markers: &'static [Marker],
    artifacts: &'static [&'static str],
) -> Ecosystem {
    Ecosystem {
        code,
        markers,
        artifacts,
        artifact_prefixes: &[],
        pycache: false,
    }
}

use Marker::{Dirs, File, Suffix};

/// Ecosystem markers → rebuildable artifact folders. Original PC Toolkit Pro list.
pub(crate) const ECOSYSTEMS: &[Ecosystem] = &[
    eco(
        "node",
        &[File("package.json")],
        &["node_modules", ".next/cache", ".nuxt", ".turbo", ".angular/cache"],
    ),
    eco("rust", &[File("cargo.toml")], &["target"]),
    Ecosystem {
        code: "python",
        markers: &[
            File("pyproject.toml"),
            File("setup.py"),
            File("setup.cfg"),
            File("requirements.txt"),
            File("pipfile"),
        ],
        artifacts: &[".pytest_cache", ".mypy_cache", ".ruff_cache", ".tox", ".nox", "__pypackages__"],
        artifact_prefixes: &[],
        pycache: true,
    },
    eco("dotnet", &[Suffix(".csproj"), Suffix(".fsproj")], &["bin", "obj"]),
    eco("gradle", &[File("build.gradle"), File("build.gradle.kts")], &["build", ".gradle"]),
    eco("maven", &[File("pom.xml")], &["target"]),
    Ecosystem {
        code: "cmake",
        markers: &[File("cmakelists.txt")],
        artifacts: &["build"],
        artifact_prefixes: &["cmake-build-"],
        pycache: false,
    },
    eco("flutter", &[File("pubspec.yaml")], &["build", ".dart_tool"]),
    eco("composer", &[File("composer.json")], &["vendor"]),
    eco("unity", &[Dirs(&["assets", "projectsettings"])], &["library", "temp", "obj", "logs"]),
    eco("unreal", &[Suffix(".uproject")], &["binaries", "intermediate", "deriveddatacache"]),
    eco("godot", &[File("project.godot")], &[".godot"]),
    eco("zig", &[File("build.zig")], &[".zig-cache", "zig-out"]),
    eco("swift", &[File("package.swift")], &[".build"]),
    eco("terraform", &[File(".terraform.lock.hcl"), Suffix(".tf")], &[".terraform"]),
    eco(
        "haskell",
        &[File("stack.yaml"), File("cabal.project"), Suffix(".cabal")],
        &[".stack-work", "dist-newstyle"],
    ),
    eco("elixir", &[File("mix.exs")], &["_build"]),
];

/// Folders never descended while searching for projects (dependency stores and
/// generated trees that contain third-party markers).
const NEVER_DESCEND: &[&str] = &[
    "node_modules",
    "__pycache__",
    "__pypackages__",
    "site-packages",
    "bower_components",
    "$recycle.bin",
    "system volume information",
];

/// Home children that are not searched (media, shell folders, synced storage).
const HOME_SKIP: &[&str] = &[
    "appdata",
    "application data",
    "local settings",
    "pictures",
    "videos",
    "music",
    "saved games",
    "contacts",
    "favorites",
    "links",
    "searches",
    "3d objects",
    "cookies",
    "nethood",
    "printhood",
    "recent",
    "sendto",
    "start menu",
    "templates",
    "my documents",
];

/// Top-level folders on fixed drives that usually hold source trees.
const DEV_ROOT_NAMES: &[&str] = &[
    "code",
    "dev",
    "develop",
    "development",
    "projects",
    "repos",
    "src",
    "source",
    "workspace",
    "workspaces",
    "github",
    "git",
];

/// Top-level folders of an extra drive that are never searched for projects (Windows, installed
/// programs, profiles, recovery data, and game libraries hold no user source trees but millions of
/// files).
const DRIVE_SKIP: &[&str] = &[
    "windows",
    "windows.old",
    "program files",
    "program files (x86)",
    "program files (arm)",
    "programdata",
    "users",
    "perflogs",
    "recovery",
    "msocache",
    "intel",
    "amd",
    "nvidia",
    "drivers",
    "onedrivetemp",
    "xboxgames",
    "steamlibrary",
    "steam",
    "epic games",
    "riot games",
    "games",
];

#[derive(Debug, Clone, Copy)]
pub(crate) struct DiscoveryLimits {
    /// Folder depth below each discovery root.
    pub max_depth: u32,
    pub max_dirs: u64,
    pub max_projects: usize,
    pub time_budget: Duration,
}

impl Default for DiscoveryLimits {
    fn default() -> Self {
        Self {
            max_depth: 12,
            max_dirs: 300_000,
            max_projects: 5_000,
            time_budget: Duration::from_secs(25),
        }
    }
}

impl DiscoveryLimits {
    /// Extra drives are searched from their top level, so allow a larger walk.
    fn with_extra_drives(extra: usize) -> Self {
        if extra == 0 {
            return Self::default();
        }
        let base = Self::default();
        Self {
            max_dirs: base.max_dirs + 300_000 * extra as u64,
            time_budget: base.time_budget + Duration::from_secs(20 * extra as u64),
            ..base
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Project {
    pub path: PathBuf,
    pub ecosystems: Vec<&'static str>,
    /// Real artifact directories strictly inside `path` (shallowest only).
    pub artifacts: Vec<PathBuf>,
    pub newest_mtime: Option<SystemTime>,
    parent: Option<usize>,
    /// The `.gitignore` guard allows descendant `__pycache__` folders.
    pycache_ok: bool,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Discovery {
    pub projects: Vec<Project>,
    /// A limit stopped the walk; more projects may exist.
    pub limited: bool,
}

fn cancelled() -> bool {
    CANCELLED.load(std::sync::atomic::Ordering::Relaxed)
}

fn attributes(meta: &fs::Metadata) -> u32 {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        meta.file_attributes()
    }
    #[cfg(not(windows))]
    {
        let _ = meta;
        0
    }
}

fn is_hidden(name_lower: &str, meta: &fs::Metadata) -> bool {
    name_lower.starts_with('.') || attributes(meta) & (FILE_ATTRIBUTE_HIDDEN | FILE_ATTRIBUTE_SYSTEM) != 0
}

/// A real (non-link) directory, checked without following links.
fn real_dir(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|m| m.is_dir() && !is_link_like(&m))
}

struct ChildDir {
    lower: String,
    path: PathBuf,
    hidden: bool,
}

struct Listing {
    files: HashSet<String>,
    dirs: Vec<ChildDir>,
    newest_file: Option<SystemTime>,
}

fn list_dir(dir: &Path) -> Option<Listing> {
    let read = fs::read_dir(dir).ok()?;
    let mut listing = Listing {
        files: HashSet::new(),
        dirs: Vec::new(),
        newest_file: None,
    };
    for entry in read.flatten() {
        // Windows: from the directory enumeration, links are not followed.
        let Ok(meta) = entry.metadata() else { continue };
        if is_link_like(&meta) {
            continue;
        }
        let lower = entry.file_name().to_string_lossy().to_lowercase();
        if meta.is_dir() {
            listing.dirs.push(ChildDir {
                hidden: is_hidden(&lower, &meta),
                lower,
                path: entry.path(),
            });
        } else if meta.is_file() {
            if let Ok(modified) = meta.modified() {
                listing.newest_file = Some(listing.newest_file.map_or(modified, |n| n.max(modified)));
            }
            listing.files.insert(lower);
        }
    }
    Some(listing)
}

fn marker_present(marker: &Marker, listing: &Listing) -> bool {
    match marker {
        Marker::File(name) => listing.files.contains(*name),
        Marker::Suffix(suffix) => listing.files.iter().any(|f| f.len() > suffix.len() && f.ends_with(suffix)),
        Marker::Dirs(names) => names.iter().all(|n| listing.dirs.iter().any(|d| d.lower == *n)),
    }
}

fn detect(listing: &Listing) -> Vec<&'static Ecosystem> {
    ECOSYSTEMS
        .iter()
        .filter(|e| e.markers.iter().any(|m| marker_present(m, listing)))
        .collect()
}

/// `.gitignore` of a repository root, lowercased with `[]` classes flattened
/// (`[Bb]in` → `bbin`) so plain-substring checks work. `None` = no guard applies.
fn gitignore_text(project: &Path, listing: &Listing) -> Option<String> {
    let is_repo_root = listing.dirs.iter().any(|d| d.lower == ".git") || listing.files.contains(".git");
    if !is_repo_root || !listing.files.contains(".gitignore") {
        return None;
    }
    let path = project.join(".gitignore");
    let meta = fs::symlink_metadata(&path).ok()?;
    if is_link_like(&meta) || meta.len() > GITIGNORE_CAP {
        return None;
    }
    let text = fs::read_to_string(path).ok()?;
    Some(text.to_lowercase().replace(['[', ']'], ""))
}

/// True when the `.gitignore` guard allows `needle` (an artifact name) to be cleaned.
fn ignore_allows(gitignore: Option<&str>, needle: &str) -> bool {
    gitignore.is_none_or(|text| text.contains(needle))
}

/// Artifact candidates of one project folder. Each must be a real directory, not
/// hold `.git`, and pass the `.gitignore` guard.
fn resolve_artifacts(
    project: &Path,
    listing: &Listing,
    ecosystems: &[&'static Ecosystem],
    gitignore: Option<&str>,
) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    let mut push = |path: PathBuf, needle: &str| {
        if ignore_allows(gitignore, needle)
            && real_dir(&path)
            && is_strict_descendant(&path, project)
            && fs::symlink_metadata(path.join(".git")).is_err()
            && !out.iter().any(|p| path_key(p) == path_key(&path))
        {
            out.push(path);
        }
    };
    for eco in ecosystems {
        for rel in eco.artifacts {
            let mut segments = rel.split('/');
            let Some(first) = segments.next() else { continue };
            let Some(child) = listing.dirs.iter().find(|d| d.lower == first) else { continue };
            let mut path = child.path.clone();
            let mut real = true;
            for seg in segments {
                path.push(seg);
                if !real_dir(&path) {
                    real = false;
                    break;
                }
            }
            if real {
                push(path, first);
            }
        }
        for prefix in eco.artifact_prefixes {
            for child in listing.dirs.iter().filter(|d| d.lower.starts_with(prefix)) {
                push(child.path.clone(), prefix.trim_end_matches('-'));
            }
        }
    }
    out
}

/// First path segments of every artifact of `ecosystems` (never descended).
fn artifact_names(ecosystems: &[&'static Ecosystem]) -> Vec<&'static str> {
    ecosystems
        .iter()
        .flat_map(|e| e.artifacts.iter().filter_map(|a| a.split('/').next()))
        .collect()
}

fn is_artifact_prefix(ecosystems: &[&'static Ecosystem], name: &str) -> bool {
    ecosystems.iter().any(|e| e.artifact_prefixes.iter().any(|p| name.starts_with(p)))
}

struct Frame {
    dir: PathBuf,
    depth: u32,
    /// Nearest enclosing project.
    owner: Option<usize>,
    /// Nearest enclosing Python project (owns `__pycache__` folders).
    py_owner: Option<usize>,
}

fn bump_newest(projects: &mut [Project], mut owner: Option<usize>, time: SystemTime) {
    while let Some(index) = owner {
        let project = &mut projects[index];
        if project.newest_mtime.is_some_and(|n| n >= time) {
            // Ancestors already saw a time at least this new through this child.
            break;
        }
        project.newest_mtime = Some(time);
        owner = project.parent;
    }
}

/// Drop artifacts that lie inside another artifact (keep the shallowest).
pub(crate) fn keep_shallowest(projects: &mut [Project]) {
    let all: Vec<PathBuf> = projects.iter().flat_map(|p| p.artifacts.iter().cloned()).collect();
    for project in projects.iter_mut() {
        project
            .artifacts
            .retain(|a| !all.iter().any(|other| is_strict_descendant(a, other)));
    }
}

/// Bounded walk over `roots`. `on_dir` receives every visited folder (UI progress).
pub(crate) fn discover(roots: &[PathBuf], limits: DiscoveryLimits, on_dir: &dyn Fn(&Path)) -> Discovery {
    let started = Instant::now();
    let mut out = Discovery::default();
    let mut visited = 0u64;
    let mut seen_roots = HashSet::new();
    let mut stack: Vec<Frame> = Vec::new();
    for root in roots.iter().rev() {
        if seen_roots.insert(path_key(root)) && real_dir(root) {
            stack.push(Frame {
                dir: root.clone(),
                depth: 0,
                owner: None,
                py_owner: None,
            });
        }
    }

    while let Some(frame) = stack.pop() {
        if cancelled() {
            break;
        }
        if visited >= limits.max_dirs
            || out.projects.len() >= limits.max_projects
            || started.elapsed() >= limits.time_budget
        {
            out.limited = true;
            break;
        }
        visited += 1;
        if visited % 256 == 0 {
            on_dir(&frame.dir);
        }
        let Some(listing) = list_dir(&frame.dir) else { continue };
        // Virtual environments carry thousands of third-party markers.
        if listing.files.contains("pyvenv.cfg") {
            continue;
        }

        let mut owner = frame.owner;
        let mut py_owner = frame.py_owner;
        let ecosystems = detect(&listing);
        let mut skip: Vec<&'static str> = Vec::new();
        if !ecosystems.is_empty() {
            let index = out.projects.len();
            let mut newest = None;
            if let Ok(meta) = fs::symlink_metadata(frame.dir.join(".git").join("index")) {
                newest = meta.modified().ok();
            }
            let gitignore = gitignore_text(&frame.dir, &listing);
            out.projects.push(Project {
                path: frame.dir.clone(),
                ecosystems: ecosystems.iter().map(|e| e.code).collect(),
                artifacts: resolve_artifacts(&frame.dir, &listing, &ecosystems, gitignore.as_deref()),
                newest_mtime: None,
                parent: frame.owner,
                pycache_ok: ignore_allows(gitignore.as_deref(), "pycache"),
            });
            if let Some(time) = newest {
                bump_newest(&mut out.projects, Some(index), time);
            }
            owner = Some(index);
            if ecosystems.iter().any(|e| e.pycache) {
                py_owner = Some(index);
            }
            skip = artifact_names(&ecosystems);
        }
        if let Some(time) = listing.newest_file {
            bump_newest(&mut out.projects, owner, time);
        }
        if let Some(py) = py_owner {
            if let Some(cache) = listing.dirs.iter().find(|d| d.lower == "__pycache__") {
                let project = &out.projects[py];
                if project.pycache_ok && is_strict_descendant(&cache.path, &project.path) {
                    out.projects[py].artifacts.push(cache.path.clone());
                }
            }
        }

        if frame.depth >= limits.max_depth {
            continue;
        }
        for child in listing.dirs.into_iter().rev() {
            if child.hidden
                || VCS_SEGMENTS.contains(&child.lower.as_str())
                || NEVER_DESCEND.contains(&child.lower.as_str())
                || skip.contains(&child.lower.as_str())
                || is_artifact_prefix(&ecosystems, &child.lower)
            {
                continue;
            }
            stack.push(Frame {
                dir: child.path,
                depth: frame.depth + 1,
                owner,
                py_owner,
            });
        }
    }
    keep_shallowest(&mut out.projects);
    out
}

fn is_onedrive(name_lower: &str) -> bool {
    name_lower.starts_with("onedrive")
}

/// Search roots: non-hidden home children (minus AppData, media, OneDrive) and
/// developer roots directly on fixed drives. Link-like folders are never roots.
pub(crate) fn discovery_roots(home: Option<&Path>, drives: &[PathBuf]) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(home) = home {
        if let Some(listing) = list_dir(home) {
            let mut dirs = listing.dirs;
            dirs.sort_by(|a, b| a.lower.cmp(&b.lower));
            for dir in dirs {
                if !dir.hidden && !HOME_SKIP.contains(&dir.lower.as_str()) && !is_onedrive(&dir.lower) {
                    roots.push(dir.path);
                }
            }
        }
    }
    for drive in drives {
        let Some(listing) = list_dir(drive) else { continue };
        let mut dirs = listing.dirs;
        dirs.sort_by(|a, b| a.lower.cmp(&b.lower));
        for dir in dirs {
            if !dir.hidden && DEV_ROOT_NAMES.contains(&dir.lower.as_str()) {
                roots.push(dir.path);
            }
        }
    }
    // A root inside another root would be walked twice.
    let snapshot = roots.clone();
    roots.retain(|r| !snapshot.iter().any(|o| is_strict_descendant(r, o)));
    roots
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE").filter(|v| !v.is_empty()).map(PathBuf::from)
}

/// Search roots for drives the user added in the scope picker: every non-hidden top-level folder
/// except system, program, profile and game folders. Folders already covered by `roots` are left
/// out by the caller's nesting filter.
pub(crate) fn extra_drive_roots(drives: &[PathBuf]) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    for drive in drives {
        let Some(listing) = list_dir(drive) else { continue };
        let mut dirs = listing.dirs;
        dirs.sort_by(|a, b| a.lower.cmp(&b.lower));
        for dir in dirs {
            if !dir.hidden
                && !dir.lower.starts_with('$')
                && !DRIVE_SKIP.contains(&dir.lower.as_str())
                && !NEVER_DESCEND.contains(&dir.lower.as_str())
            {
                roots.push(dir.path);
            }
        }
    }
    roots
}

fn default_roots() -> Vec<PathBuf> {
    discovery_roots(home_dir().as_deref(), &pctoolkit_platform::fixed_drive_roots())
}

/// Default roots plus the extra drives chosen for this scan, without nested duplicates.
fn scoped_roots(extra_drives: &[PathBuf]) -> Vec<PathBuf> {
    let mut roots = default_roots();
    roots.extend(extra_drive_roots(extra_drives));
    let mut seen = HashSet::new();
    roots.retain(|r| seen.insert(path_key(r)));
    let snapshot = roots.clone();
    roots.retain(|r| !snapshot.iter().any(|o| is_strict_descendant(r, o)));
    roots
}

struct SharedDiscovery {
    at: Instant,
    /// Extra drives the discovery covered; a scope change invalidates it.
    extra_drives: Vec<PathBuf>,
    discovery: Arc<Discovery>,
}

static SHARED: Mutex<Option<SharedDiscovery>> = Mutex::new(None);

/// One discovery shared by the projects provider and the rustup special kind of
/// the same scan. Concurrent callers wait for the first.
fn shared_discovery(on_dir: &dyn Fn(&Path)) -> Arc<Discovery> {
    let mut slot = match SHARED.lock() {
        Ok(slot) => slot,
        Err(poisoned) => poisoned.into_inner(),
    };
    let extra_drives = super::scope::selected_project_drive_roots();
    if let Some(cached) = slot.as_ref() {
        if cached.at.elapsed() < SHARED_DISCOVERY_TTL && cached.extra_drives == extra_drives {
            return Arc::clone(&cached.discovery);
        }
    }
    let limits = DiscoveryLimits::with_extra_drives(extra_drives.len());
    let discovery = Arc::new(discover(&scoped_roots(&extra_drives), limits, on_dir));
    // A cancelled walk is incomplete; do not let the next scan reuse it.
    if !cancelled() {
        *slot = Some(SharedDiscovery {
            at: Instant::now(),
            extra_drives,
            discovery: Arc::clone(&discovery),
        });
    }
    discovery
}

/// Project roots for toolchain pin detection (`rust-toolchain(.toml)`).
pub(crate) fn cached_project_roots() -> Vec<PathBuf> {
    shared_discovery(&|_| {}).projects.iter().map(|p| p.path.clone()).collect()
}

/// Stable id: FNV-1a over the normalized project path (no path in the id).
pub(crate) fn project_id(path: &Path) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in path_key(path).as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{RULE_PREFIX}{hash:016x}")
}

fn unix_ms(time: SystemTime) -> Option<i64> {
    time.duration_since(UNIX_EPOCH).ok().map(|d| d.as_millis() as i64)
}

pub(crate) fn is_active(newest: Option<SystemTime>, now: SystemTime) -> bool {
    newest.is_some_and(|t| now.duration_since(t).map_or(true, |age| age < ACTIVE_WINDOW))
}

/// `~\code\app` under the profile, else the full path. UI only.
fn path_hint(path: &Path, home: Option<&Path>) -> String {
    if let Some(home) = home {
        if let Ok(rest) = path.strip_prefix(home) {
            return format!("~\\{}", rest.display());
        }
    }
    path.display().to_string()
}

fn relative_name(artifact: &Path, project: &Path) -> String {
    artifact
        .strip_prefix(project)
        .map(|r| r.to_string_lossy().replace('\\', "/"))
        .unwrap_or_default()
}

pub(crate) fn artifact_policy() -> RootPolicy {
    RootPolicy {
        allow_user_content: true,
        ..RootPolicy::default()
    }
}

/// Core-held id → project path from the last scan. Execute never takes paths
/// from the frontend.
static PROJECT_INDEX: Mutex<Option<HashMap<String, PathBuf>>> = Mutex::new(None);

fn remember(index: HashMap<String, PathBuf>) {
    if let Ok(mut slot) = PROJECT_INDEX.lock() {
        *slot = Some(index);
    }
}

fn remembered(id: &str) -> Option<PathBuf> {
    PROJECT_INDEX.lock().ok()?.as_ref()?.get(id).cloned()
}

#[derive(Default)]
struct ArtifactMeasure {
    tally: MeasureTally,
    refused: u32,
}

/// Measure every artifact on a small worker pool.
fn measure_artifacts(paths: &[PathBuf], ctx: &ProviderContext<'_>) -> Vec<ArtifactMeasure> {
    let next = std::sync::atomic::AtomicUsize::new(0);
    let results: Mutex<Vec<(usize, ArtifactMeasure)>> = Mutex::new(Vec::with_capacity(paths.len()));
    std::thread::scope(|scope| {
        for _ in 0..MEASURE_WORKERS.min(paths.len().max(1)) {
            scope.spawn(|| loop {
                if ctx.is_cancelled() {
                    break;
                }
                let i = next.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let Some(path) = paths.get(i) else { break };
                let mut pending = (0u64, 0u64);
                let measured = measure_directory(path, ctx.safety, artifact_policy(), &mut |file, len| {
                    pending.0 += 1;
                    pending.1 += len;
                    if pending.0 >= 512 {
                        ctx.report("projects", file, pending.0, pending.1);
                        pending = (0, 0);
                    }
                });
                if pending.0 > 0 {
                    ctx.report("projects", path, pending.0, pending.1);
                }
                let item = match measured {
                    Ok(tally) => ArtifactMeasure { tally, refused: 0 },
                    Err(RootRefusal::Missing | RootRefusal::Inaccessible) => ArtifactMeasure::default(),
                    Err(_) => ArtifactMeasure {
                        refused: 1,
                        ..ArtifactMeasure::default()
                    },
                };
                if let Ok(mut out) = results.lock() {
                    out.push((i, item));
                }
            });
        }
    });
    let mut collected = results.into_inner().unwrap_or_default();
    collected.sort_by_key(|(i, _)| *i);
    let mut by_index: Vec<ArtifactMeasure> = (0..paths.len()).map(|_| ArtifactMeasure::default()).collect();
    for (i, item) in collected {
        by_index[i] = item;
    }
    by_index
}

fn project_result(
    project: &Project,
    measures: &[&ArtifactMeasure],
    limited_discovery: bool,
    home: Option<&Path>,
    now: SystemTime,
) -> DeepCleanupRuleResult {
    let mut tally = MeasureTally::default();
    let mut refused = 0;
    for m in measures {
        tally.absorb(m.tally);
        refused += m.refused;
    }
    let status = if tally.files == 0 {
        DeepCleanupRuleStatus::Clean
    } else if tally.limited {
        DeepCleanupRuleStatus::Limited
    } else {
        DeepCleanupRuleStatus::Found
    };
    let has_data = tally.files > 0;
    DeepCleanupRuleResult {
        id: project_id(&project.path),
        group: DeepCleanupGroup::Projects,
        name_key: NAME_KEY.into(),
        detail_key: DETAIL_KEY.into(),
        risk: DeepCleanupRisk::Recoverable,
        bytes: if has_data { tally.bytes } else { 0 },
        item_count: if has_data { tally.files } else { 0 },
        recommended: false,
        selected: false,
        status,
        related_processes: Vec::new(),
        running_processes: Vec::new(),
        requires_elevation: false,
        needs_admin: false,
        limited: tally.limited,
        refused_roots: refused,
        extras: DeepCleanupRuleExtras {
            display_name: project.path.file_name().map(|n| n.to_string_lossy().into_owned()),
            path_hint: Some(path_hint(&project.path, home)),
            tags: project.ecosystems.iter().map(|e| e.to_string()).collect(),
            artifacts: project.artifacts.iter().map(|a| relative_name(a, &project.path)).collect(),
            last_modified_ms: project.newest_mtime.and_then(unix_ms),
            active: is_active(project.newest_mtime, now),
            reason: limited_discovery.then_some(DeepCleanupScanReason::DiscoveryLimited),
        },
    }
}

/// Scan: discover, measure, and build one result per project that has data.
pub(crate) fn scan_projects(
    discovery: &Discovery,
    ctx: &ProviderContext<'_>,
    home: Option<&Path>,
) -> Vec<(DeepCleanupRuleResult, PathBuf)> {
    let now = SystemTime::now();
    let mut owners: Vec<usize> = Vec::new();
    let mut paths: Vec<PathBuf> = Vec::new();
    for (i, project) in discovery.projects.iter().enumerate() {
        for artifact in &project.artifacts {
            owners.push(i);
            paths.push(artifact.clone());
        }
    }
    let measures = measure_artifacts(&paths, ctx);
    let mut per_project: Vec<Vec<&ArtifactMeasure>> = vec![Vec::new(); discovery.projects.len()];
    for (m, owner) in measures.iter().zip(&owners) {
        per_project[*owner].push(m);
    }
    let mut results: Vec<(DeepCleanupRuleResult, PathBuf)> = discovery
        .projects
        .iter()
        .zip(per_project)
        .filter(|(p, _)| !p.artifacts.is_empty())
        .map(|(p, m)| (project_result(p, &m, discovery.limited, home, now), p.path.clone()))
        .filter(|(r, _)| r.status != DeepCleanupRuleStatus::Clean)
        .collect();
    results.sort_by(|a, b| b.0.bytes.cmp(&a.0.bytes));
    results.truncate(MAX_RESULTS);
    results
}

/// Re-confirm one project right before cleaning: still a valid root, same id,
/// marker still present. Returns its current artifacts.
pub(crate) fn reconfirm(id: &str, path: &Path, safety: &SafetyContext) -> Result<Vec<PathBuf>, DeepCleanupExecReason> {
    if project_id(path) != id {
        return Err(DeepCleanupExecReason::ChangedSinceScan);
    }
    match super::safety::validate_root(path, safety, artifact_policy()) {
        Ok(()) => {}
        Err(RootRefusal::Missing) => return Err(DeepCleanupExecReason::NotApplicable),
        Err(_) => return Err(DeepCleanupExecReason::ChangedSinceScan),
    }
    let limits = DiscoveryLimits {
        time_budget: Duration::from_secs(10),
        max_dirs: 50_000,
        ..DiscoveryLimits::default()
    };
    let discovery = discover(&[path.to_path_buf()], limits, &|_| {});
    let key = path_key(path);
    let project = discovery
        .projects
        .into_iter()
        .find(|p| path_key(&p.path) == key)
        .ok_or(DeepCleanupExecReason::ChangedSinceScan)?;
    Ok(project.artifacts)
}

/// Clean one project's artifacts after re-confirmation.
pub(crate) fn clean_project(id: &str, path: &Path, safety: &SafetyContext) -> DeepCleanupRuleExecResult {
    let artifacts = match reconfirm(id, path, safety) {
        Ok(artifacts) => artifacts,
        Err(reason) => return DeepCleanupRuleExecResult::without_work(id, DeepCleanupExecStatus::Skipped, Some(reason)),
    };
    let mut tally = CleanTally::default();
    for artifact in &artifacts {
        if cancelled() {
            tally.cancelled = true;
            break;
        }
        match clean_directory(artifact, CleanMode::WholeRoot, safety, artifact_policy(), &mut |_, _| {}) {
            Ok(t) => tally.absorb(t),
            Err(RootRefusal::Missing | RootRefusal::Inaccessible) => {}
            Err(_) => tally.failed += 1,
        }
    }
    let (status, reason) = exec_status(&tally);
    DeepCleanupRuleExecResult {
        id: id.to_string(),
        status,
        reason,
        freed_bytes: tally.freed_bytes,
        removed_count: tally.removed,
        failed_count: tally.failed + tally.busy,
        busy_count: tally.busy,
    }
}

pub(crate) struct ProjectsProvider;

impl DeepCleanupProvider for ProjectsProvider {
    fn id(&self) -> &'static str {
        "projects"
    }

    fn groups(&self) -> &'static [DeepCleanupGroup] {
        &[DeepCleanupGroup::Projects]
    }

    fn owns_rule(&self, rule_id: &str) -> bool {
        rule_id.starts_with(RULE_PREFIX)
    }

    fn scan(&self, ctx: &ProviderContext<'_>) -> CoreResult<Vec<DeepCleanupRuleResult>> {
        let discovery = shared_discovery(&|dir| ctx.report("projects", dir, 0, 0));
        let home = home_dir();
        let results = scan_projects(&discovery, ctx, home.as_deref());
        remember(results.iter().map(|(r, p)| (r.id.clone(), p.clone())).collect());
        Ok(results.into_iter().map(|(r, _)| r).collect())
    }

    fn execute(&self, rule_ids: &[String], ctx: &ProviderContext<'_>) -> CoreResult<Vec<DeepCleanupRuleExecResult>> {
        Ok(rule_ids
            .iter()
            .map(|id| match remembered(id) {
                Some(path) => clean_project(id, &path, ctx.safety),
                None => DeepCleanupRuleExecResult::without_work(
                    id.clone(),
                    DeepCleanupExecStatus::Skipped,
                    Some(DeepCleanupExecReason::NotApplicable),
                ),
            })
            .collect())
    }
}

#[cfg(test)]
pub(crate) fn remember_for_test(id: &str, path: &Path) {
    if let Ok(mut slot) = PROJECT_INDEX.lock() {
        slot.get_or_insert_with(HashMap::new).insert(id.to_string(), path.to_path_buf());
    }
}

#[cfg(test)]
#[path = "projects_tests.rs"]
mod tests;
