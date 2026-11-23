//! Safety policy for Deep Cleanup roots and entries.
//!
//! Two layers:
//! - `is_link_like` decides per entry whether the walker may follow or delete it.
//! - `validate_root` decides whether a resolved root may be measured or cleaned at
//!   all. It runs before measuring and again right before deleting.
//!
//! All comparisons are case-insensitive (Windows file system semantics).

use serde::Serialize;
use std::fs;
use std::path::{Component, Path, PathBuf};

const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;
const FILE_ATTRIBUTE_OFFLINE: u32 = 0x0000_1000;
const FILE_ATTRIBUTE_RECALL_ON_OPEN: u32 = 0x0004_0000;
const FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS: u32 = 0x0040_0000;

/// Attributes that make an entry "link-like": reparse points (symlinks, junctions,
/// mount points, dedup/cloud reparse tags) and cloud placeholders. Opening a cloud
/// placeholder can trigger a download; deleting one can delete the cloud copy.
const LINK_LIKE_ATTRIBUTES: u32 = FILE_ATTRIBUTE_REPARSE_POINT
    | FILE_ATTRIBUTE_OFFLINE
    | FILE_ATTRIBUTE_RECALL_ON_OPEN
    | FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS;

/// Folders under `%WINDIR%` that catalog rules may target (relative, lowercase, `/`).
/// Anything else under the Windows directory is refused.
pub(crate) const WINDIR_ALLOWLIST: &[&str] = &[
    "softwaredistribution/download",
    "logs/windowsupdate",
    "serviceprofiles/networkservice/appdata/local/microsoft/windows/deliveryoptimization/cache",
    // Log folders for `system.windowsLogs`, which filters by log extensions and age.
    "logs",
    "panther",
    "system32/logfiles",
];

/// Top-level folders of the system drive that catalog rules may target.
pub(crate) const SYSTEM_DRIVE_ALLOWLIST: &[&str] = &["windows.old"];

/// Personal folders directly under the user profile. Never roots (Downloads only
/// for a rule that declares `allow_personal_folder`).
pub(crate) const PERSONAL_FOLDERS: &[&str] = &[
    "desktop",
    "documents",
    "downloads",
    "pictures",
    "videos",
    "music",
    "movies",
    "public",
    "saved games",
    "favorites",
    "contacts",
    "links",
    "searches",
    "3d objects",
];

/// First-level profile folders that hold credentials, sync clients, VMs, or user
/// source trees. Never roots, never ancestors of roots.
pub(crate) const PROTECTED_HOME_PREFIXES: &[&str] = &[
    ".ansible",
    ".aws",
    ".azure",
    ".config",
    ".docker",
    ".env",
    ".gcloud",
    ".git-credentials",
    ".gnupg",
    ".idea",
    ".kube",
    ".netrc",
    ".password-store",
    ".ssh",
    ".vscode",
    "backup",
    "backups",
    "box",
    "code",
    "creative cloud files",
    "dev",
    "dropbox",
    "google drive",
    "icloud drive",
    "icloudrive",
    "my virtual machines",
    "nextcloud",
    "onedrive",
    "owncloud",
    "projects",
    "repos",
    "repositories",
    "source",
    "src",
    "sync",
    "virtualbox vms",
    "vms",
    "workspace",
    "workspaces",
];

/// Version-control metadata folders. Rejected anywhere in a root path.
pub(crate) const VCS_SEGMENTS: &[&str] = &[".git", ".hg", ".svn", ".bzr"];

/// Typed reason a root was refused. Serialized for diagnostics/counters only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RootRefusal {
    NotAbsolute,
    ParentTraversal,
    VolumeRoot,
    SystemDirectory,
    ProgramFiles,
    ProfileRoot,
    BaseFolderRoot,
    PersonalFolder,
    ProtectedHomeFolder,
    VersionControl,
    LinkComponent,
    NotDirectory,
    /// The root does not exist (not a refusal; callers treat it as absent).
    Missing,
    /// A component could not be inspected (access denied). Fails closed; callers
    /// treat it as absent rather than as a policy refusal.
    Inaccessible,
}

/// Extra permissions a caller may request for its roots. Defaults are the strict
/// catalog policy.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RootPolicy {
    /// Allow roots inside `%USERPROFILE%\Downloads` (stale partial downloads only).
    pub allow_personal_downloads: bool,
    /// Allow roots inside personal / protected home folders (Documents, `source`,
    /// `repos`, ...). For project-artifact providers that target build output
    /// folders inside user source trees. System, Program Files, profile root, VCS,
    /// and link checks still apply.
    pub allow_user_content: bool,
}

/// Well-known directories the policy compares against. `from_env` reads them from
/// the environment; tests construct one explicitly.
#[derive(Debug, Clone, Default)]
pub struct SafetyContext {
    pub windir: Option<PathBuf>,
    pub system_drive: Option<PathBuf>,
    pub program_files: Vec<PathBuf>,
    pub profile: Option<PathBuf>,
    /// Base folders that may hold roots but are never roots themselves
    /// (`%LOCALAPPDATA%`, `%APPDATA%`, `%PROGRAMDATA%`, `AppData`).
    pub base_folders: Vec<PathBuf>,
}

fn env_dir(key: &str) -> Option<PathBuf> {
    std::env::var_os(key)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

impl SafetyContext {
    pub fn from_env() -> Self {
        let mut program_files = Vec::new();
        for key in ["ProgramFiles", "ProgramFiles(x86)", "ProgramW6432"] {
            if let Some(p) = env_dir(key) {
                program_files.push(p);
            }
        }
        let system_drive = env_dir("SystemDrive").map(|d| drive_root(&d));
        if let Some(drive) = &system_drive {
            // Fallbacks in case the environment is stripped.
            program_files.push(drive.join("Program Files"));
            program_files.push(drive.join("Program Files (x86)"));
        }
        let profile = env_dir("USERPROFILE");
        let mut base_folders = Vec::new();
        for key in ["LOCALAPPDATA", "APPDATA", "ProgramData"] {
            if let Some(p) = env_dir(key) {
                base_folders.push(p);
            }
        }
        if let Some(home) = &profile {
            base_folders.push(home.join("AppData"));
        }
        Self {
            windir: env_dir("WINDIR").or_else(|| env_dir("SystemRoot")),
            system_drive,
            program_files,
            profile,
            base_folders,
        }
    }
}

/// `C:` → `C:\` (a bare drive letter is drive-relative, not the root).
pub(crate) fn drive_root(value: &Path) -> PathBuf {
    let text = value.to_string_lossy();
    if text.len() == 2 && text.ends_with(':') {
        PathBuf::from(format!("{text}\\"))
    } else {
        value.to_path_buf()
    }
}

/// True when the attribute bits mark a reparse point or cloud placeholder.
pub fn is_link_like_attributes(attributes: u32) -> bool {
    attributes & LINK_LIKE_ATTRIBUTES != 0
}

/// True for symlinks, junctions, any reparse point, and cloud placeholders.
/// `meta` must come from `symlink_metadata` or `DirEntry::metadata` (no follow).
pub fn is_link_like(meta: &fs::Metadata) -> bool {
    if meta.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        is_link_like_attributes(meta.file_attributes())
    }
    #[cfg(not(windows))]
    {
        false
    }
}

/// Lowercased path components; the drive prefix is kept as e.g. `c:`.
pub(crate) fn normalized_components(path: &Path) -> Vec<String> {
    path.components()
        .filter_map(|c| match c {
            Component::Prefix(p) => Some(p.as_os_str().to_string_lossy().to_lowercase()),
            Component::Normal(s) => Some(s.to_string_lossy().to_lowercase()),
            Component::RootDir | Component::CurDir | Component::ParentDir => None,
        })
        .collect()
}

/// Lowercase key for set membership / ownership comparisons.
pub(crate) fn path_key(path: &Path) -> String {
    normalized_components(path).join("\\")
}

/// `Some(rest)` when `path` equals or lies under `base` (component-wise, case-insensitive).
fn relative_components(path: &[String], base: &[String]) -> Option<Vec<String>> {
    if base.is_empty() || path.len() < base.len() {
        return None;
    }
    if path[..base.len()] == base[..] {
        Some(path[base.len()..].to_vec())
    } else {
        None
    }
}

/// True when `child` lies strictly below `parent`.
pub(crate) fn is_strict_descendant(child: &Path, parent: &Path) -> bool {
    let c = normalized_components(child);
    let p = normalized_components(parent);
    c.len() > p.len() && c[..p.len()] == p[..]
}

fn in_allowlist(rest: &[String], allowlist: &[&str]) -> bool {
    let joined = rest.join("/");
    allowlist
        .iter()
        .any(|allowed| joined == *allowed || joined.starts_with(&format!("{allowed}/")))
}

fn is_onedrive_folder(name: &str) -> bool {
    name.starts_with("onedrive")
}

/// Policy-only checks (no file system access). Split out for unit tests.
pub(crate) fn check_root_policy(
    path: &Path,
    ctx: &SafetyContext,
    policy: RootPolicy,
) -> Result<(), RootRefusal> {
    if !path.is_absolute() {
        return Err(RootRefusal::NotAbsolute);
    }
    if path
        .components()
        .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
    {
        return Err(RootRefusal::ParentTraversal);
    }
    let comps = normalized_components(path);
    let named = path
        .components()
        .filter(|c| matches!(c, Component::Normal(_)))
        .count();
    if named == 0 {
        return Err(RootRefusal::VolumeRoot);
    }
    if comps.iter().any(|c| VCS_SEGMENTS.contains(&c.as_str())) {
        return Err(RootRefusal::VersionControl);
    }

    if let Some(windir) = &ctx.windir {
        if let Some(rest) = relative_components(&comps, &normalized_components(windir)) {
            if rest.is_empty() || !in_allowlist(&rest, WINDIR_ALLOWLIST) {
                return Err(RootRefusal::SystemDirectory);
            }
        }
    }
    for pf in &ctx.program_files {
        if relative_components(&comps, &normalized_components(pf)).is_some() {
            return Err(RootRefusal::ProgramFiles);
        }
    }
    if let Some(drive) = &ctx.system_drive {
        if let Some(rest) = relative_components(&comps, &normalized_components(drive)) {
            // Direct children of the system drive: only explicit allow-list entries
            // (Windows.old) or folders that hold user/app data further down.
            if rest.len() == 1
                && !SYSTEM_DRIVE_ALLOWLIST.contains(&rest[0].as_str())
                && !ctx.profile.as_ref().is_some_and(|p| {
                    relative_components(&normalized_components(p), &comps).is_some()
                })
            {
                return Err(RootRefusal::SystemDirectory);
            }
        }
    }
    for base in &ctx.base_folders {
        if relative_components(&comps, &normalized_components(base))
            .is_some_and(|rest| rest.is_empty())
        {
            return Err(RootRefusal::BaseFolderRoot);
        }
    }
    if let Some(profile) = &ctx.profile {
        let profile_comps = normalized_components(profile);
        if let Some(rest) = relative_components(&comps, &profile_comps) {
            let Some(first) = rest.first() else {
                return Err(RootRefusal::ProfileRoot);
            };
            if !policy.allow_user_content {
                if PERSONAL_FOLDERS.contains(&first.as_str()) || is_onedrive_folder(first) {
                    let downloads_ok = policy.allow_personal_downloads && first == "downloads";
                    if !downloads_ok {
                        return Err(RootRefusal::PersonalFolder);
                    }
                } else if PROTECTED_HOME_PREFIXES.contains(&first.as_str()) {
                    return Err(RootRefusal::ProtectedHomeFolder);
                }
            }
        } else if relative_components(&profile_comps, &comps).is_some() {
            // An ancestor of the profile (C:\Users) would contain every profile.
            return Err(RootRefusal::ProfileRoot);
        }
    }
    Ok(())
}

/// Full root validation: policy, then every existing component is checked for
/// link-like attributes without following, and the root must be a real directory.
pub fn validate_root(path: &Path, ctx: &SafetyContext, policy: RootPolicy) -> Result<(), RootRefusal> {
    check_root_policy(path, ctx, policy)?;
    let mut current = PathBuf::new();
    let mut named_seen = 0usize;
    for component in path.components() {
        current.push(component.as_os_str());
        if !matches!(component, Component::Normal(_)) {
            continue;
        }
        named_seen += 1;
        let meta = match fs::symlink_metadata(&current) {
            Ok(meta) => meta,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(RootRefusal::Missing),
            // Cannot prove the component is not a link: fail closed.
            Err(_) => return Err(RootRefusal::Inaccessible),
        };
        if is_link_like(&meta) {
            return Err(RootRefusal::LinkComponent);
        }
        if !meta.is_dir() {
            return Err(RootRefusal::NotDirectory);
        }
    }
    if named_seen == 0 {
        return Err(RootRefusal::VolumeRoot);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> SafetyContext {
        SafetyContext {
            windir: Some(PathBuf::from(r"C:\Windows")),
            system_drive: Some(PathBuf::from(r"C:\")),
            program_files: vec![
                PathBuf::from(r"C:\Program Files"),
                PathBuf::from(r"C:\Program Files (x86)"),
            ],
            profile: Some(PathBuf::from(r"C:\Users\Tester")),
            base_folders: vec![
                PathBuf::from(r"C:\Users\Tester\AppData\Local"),
                PathBuf::from(r"C:\Users\Tester\AppData\Roaming"),
                PathBuf::from(r"C:\ProgramData"),
                PathBuf::from(r"C:\Users\Tester\AppData"),
            ],
        }
    }

    fn policy_check(path: &str) -> Result<(), RootRefusal> {
        check_root_policy(Path::new(path), &ctx(), RootPolicy::default())
    }

    #[test]
    fn link_like_attribute_mask_covers_reparse_and_cloud_placeholders() {
        assert!(is_link_like_attributes(0x400));
        assert!(is_link_like_attributes(0x1000));
        assert!(is_link_like_attributes(0x40000));
        assert!(is_link_like_attributes(0x400000));
        assert!(is_link_like_attributes(0x10 | 0x400));
        assert!(!is_link_like_attributes(0x10)); // plain directory
        assert!(!is_link_like_attributes(0x20 | 0x01)); // archive + read-only file
    }

    #[cfg(windows)]
    #[test]
    fn refuses_relative_and_traversal_paths() {
        assert_eq!(policy_check(r"Users\Tester\cache"), Err(RootRefusal::NotAbsolute));
        assert_eq!(
            policy_check(r"C:\Users\Tester\AppData\Local\..\..\Documents"),
            Err(RootRefusal::ParentTraversal)
        );
    }

    #[cfg(windows)]
    #[test]
    fn refuses_volume_roots() {
        assert_eq!(policy_check(r"C:\"), Err(RootRefusal::VolumeRoot));
        assert_eq!(policy_check(r"D:\"), Err(RootRefusal::VolumeRoot));
    }

    #[cfg(windows)]
    #[test]
    fn refuses_windows_directory_except_allow_list() {
        assert_eq!(policy_check(r"C:\Windows"), Err(RootRefusal::SystemDirectory));
        assert_eq!(policy_check(r"C:\Windows\System32"), Err(RootRefusal::SystemDirectory));
        assert_eq!(policy_check(r"c:\windows\Temp"), Err(RootRefusal::SystemDirectory));
        assert_eq!(policy_check(r"C:\Windows\SoftwareDistribution"), Err(RootRefusal::SystemDirectory));
        assert_eq!(policy_check(r"C:\Windows\SoftwareDistribution\Download"), Ok(()));
        assert_eq!(policy_check(r"C:\WINDOWS\Logs\WindowsUpdate"), Ok(()));
        assert_eq!(
            policy_check(
                r"C:\Windows\ServiceProfiles\NetworkService\AppData\Local\Microsoft\Windows\DeliveryOptimization\Cache"
            ),
            Ok(())
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_log_folders_are_allowed_but_not_their_parents() {
        assert_eq!(policy_check(r"C:\Windows\Logs"), Ok(()));
        assert_eq!(policy_check(r"C:\Windows\Logs\CBS"), Ok(()));
        assert_eq!(policy_check(r"C:\Windows\Panther"), Ok(()));
        assert_eq!(policy_check(r"C:\Windows\System32\LogFiles"), Ok(()));
        assert_eq!(policy_check(r"C:\Windows\System32\LogFiles\WMI"), Ok(()));
        // Siblings and parents of the log folders stay refused.
        assert_eq!(policy_check(r"C:\Windows\System32\config"), Err(RootRefusal::SystemDirectory));
        assert_eq!(policy_check(r"C:\Windows\LogsBackup"), Err(RootRefusal::SystemDirectory));
        assert_eq!(policy_check(r"C:\Windows\PantherOld"), Err(RootRefusal::SystemDirectory));
    }

    #[cfg(windows)]
    #[test]
    fn refuses_program_files_both_views() {
        assert_eq!(policy_check(r"C:\Program Files"), Err(RootRefusal::ProgramFiles));
        assert_eq!(policy_check(r"C:\Program Files\Vendor\cache"), Err(RootRefusal::ProgramFiles));
        assert_eq!(policy_check(r"C:\Program Files (x86)\Steam\htmlcache"), Err(RootRefusal::ProgramFiles));
    }

    #[cfg(windows)]
    #[test]
    fn system_drive_children_only_via_allow_list() {
        assert_eq!(policy_check(r"C:\Windows.old"), Ok(()));
        // C:\Users holds every profile.
        assert_eq!(policy_check(r"C:\Users"), Err(RootRefusal::ProfileRoot));
        assert_eq!(policy_check(r"C:\ProgramData"), Err(RootRefusal::SystemDirectory));
        assert_eq!(policy_check(r"C:\Data"), Err(RootRefusal::SystemDirectory));
    }

    #[cfg(windows)]
    #[test]
    fn refuses_profile_root_and_base_folders() {
        assert_eq!(policy_check(r"C:\Users\Tester"), Err(RootRefusal::ProfileRoot));
        assert_eq!(policy_check(r"C:\Users\Tester\AppData"), Err(RootRefusal::BaseFolderRoot));
        assert_eq!(policy_check(r"C:\Users\Tester\AppData\Local"), Err(RootRefusal::BaseFolderRoot));
        assert_eq!(policy_check(r"C:\Users\Tester\AppData\Roaming"), Err(RootRefusal::BaseFolderRoot));
        assert_eq!(policy_check(r"C:\Users\Tester\AppData\Local\Temp"), Ok(()));
    }

    #[cfg(windows)]
    #[test]
    fn refuses_personal_folders_and_onedrive() {
        for folder in [
            "Desktop", "Documents", "Downloads", "Pictures", "Videos", "Music", "Saved Games",
            "OneDrive", "OneDrive - Contoso",
        ] {
            let path = format!(r"C:\Users\Tester\{folder}\cache");
            assert_eq!(policy_check(&path), Err(RootRefusal::PersonalFolder), "{folder}");
        }
    }

    #[cfg(windows)]
    #[test]
    fn downloads_allowed_only_with_declared_exception() {
        let downloads = Path::new(r"C:\Users\Tester\Downloads");
        let strict = check_root_policy(downloads, &ctx(), RootPolicy::default());
        assert_eq!(strict, Err(RootRefusal::PersonalFolder));
        let stale = RootPolicy {
            allow_personal_downloads: true,
            ..RootPolicy::default()
        };
        assert_eq!(check_root_policy(downloads, &ctx(), stale), Ok(()));
        // The exception covers Downloads only.
        assert_eq!(
            check_root_policy(Path::new(r"C:\Users\Tester\Documents"), &ctx(), stale),
            Err(RootRefusal::PersonalFolder)
        );
    }

    #[cfg(windows)]
    #[test]
    fn refuses_protected_home_prefixes() {
        for folder in [".ssh", ".aws", ".config", "source", "repos", "Dropbox", "Google Drive", "projects"] {
            let path = format!(r"C:\Users\Tester\{folder}\cache");
            assert_eq!(policy_check(&path), Err(RootRefusal::ProtectedHomeFolder), "{folder}");
        }
        // Dev caches in the profile that are not protected.
        assert_eq!(policy_check(r"C:\Users\Tester\.cargo\registry\cache"), Ok(()));
        assert_eq!(policy_check(r"C:\Users\Tester\.gradle\caches"), Ok(()));
    }

    #[cfg(windows)]
    #[test]
    fn refuses_version_control_anywhere() {
        assert_eq!(
            policy_check(r"C:\Users\Tester\AppData\Local\Tool\.git\objects"),
            Err(RootRefusal::VersionControl)
        );
        assert_eq!(policy_check(r"D:\work\.svn"), Err(RootRefusal::VersionControl));
        assert_eq!(policy_check(r"D:\work\.HG\store"), Err(RootRefusal::VersionControl));
    }

    #[cfg(windows)]
    #[test]
    fn user_content_policy_still_refuses_system_and_links() {
        let projects = RootPolicy {
            allow_user_content: true,
            ..RootPolicy::default()
        };
        let c = ctx();
        assert_eq!(
            check_root_policy(Path::new(r"C:\Users\Tester\source\app\node_modules"), &c, projects),
            Ok(())
        );
        assert_eq!(
            check_root_policy(Path::new(r"C:\Users\Tester"), &c, projects),
            Err(RootRefusal::ProfileRoot)
        );
        assert_eq!(
            check_root_policy(Path::new(r"C:\Windows\Temp"), &c, projects),
            Err(RootRefusal::SystemDirectory)
        );
    }

    #[test]
    fn validate_root_reports_missing_and_files() {
        let base = std::env::temp_dir().join(format!("pctoolkit-safety-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        let file = base.join("plain.bin");
        fs::write(&file, b"x").unwrap();
        let empty = SafetyContext::default();
        assert_eq!(validate_root(&base, &empty, RootPolicy::default()), Ok(()));
        assert_eq!(
            validate_root(&base.join("absent"), &empty, RootPolicy::default()),
            Err(RootRefusal::Missing)
        );
        assert_eq!(
            validate_root(&file, &empty, RootPolicy::default()),
            Err(RootRefusal::NotDirectory)
        );
        let _ = fs::remove_dir_all(&base);
    }
}
