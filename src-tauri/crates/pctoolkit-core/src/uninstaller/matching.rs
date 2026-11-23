//! Pure name matching and path-safety policy for leftover detection.
//!
//! The policy is deliberately conservative: a leftover candidate must match the removed program by
//! an exact normalized name (never a substring), generic words never match on their own, and a
//! path is rejected when it is a protected Windows/user root or is shared with another program
//! that is still installed (the "owned by someone else" guard).

use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Words that are too generic to identify a single program.
const GENERIC_TOKENS: &[&str] = &[
    "app", "apps", "application", "applications", "bin", "cache", "common", "commonfiles", "company",
    "config", "data", "default", "desktop", "documents", "downloads", "driver", "drivers", "files",
    "google", "inc", "installer", "intel", "llc", "local", "locallow", "logs", "ltd", "microsoft",
    "mozilla", "nvidia", "packages", "program", "programdata", "programfiles", "programs", "roaming",
    "runtime", "settings", "setup", "shared", "software", "system", "temp", "tools", "update",
    "updater", "user", "users", "windows", "wow6432node", "x64", "x86", "classes", "policies",
    "clients", "registeredapplications", "amd", "corporation", "corp", "team", "the", "studio",
];

/// Lowercase alphanumeric form used for all comparisons ("My-App 2" → "myapp2").
pub fn compact(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

fn is_version_like(word: &str) -> bool {
    let w = word.trim_start_matches(['v', 'V']);
    !w.is_empty() && w.chars().all(|c| c.is_ascii_digit() || c == '.' || c == '-' || c == '_')
        && w.chars().any(|c| c.is_ascii_digit())
}

/// Strip version numbers, architecture and edition noise from a display name:
/// "Zed 0.150.4 (x64)" → "Zed", "7-Zip 24.08 (x64 edition)" → "7-Zip".
pub fn product_name(display_name: &str) -> String {
    let mut cleaned = String::with_capacity(display_name.len());
    let mut depth = 0u32;
    for ch in display_name.chars() {
        match ch {
            '(' | '[' => depth += 1,
            ')' | ']' => depth = depth.saturating_sub(1),
            '™' | '®' | '©' => {}
            _ if depth == 0 => cleaned.push(ch),
            _ => {}
        }
    }
    let noise = ["x64", "x86", "64-bit", "32-bit", "amd64", "arm64", "version", "edition"];
    let words: Vec<&str> = cleaned
        .split_whitespace()
        .filter(|w| !is_version_like(w) && !noise.contains(&w.to_ascii_lowercase().as_str()))
        .collect();
    let name = words.join(" ").trim_matches(['-', ' ', ',']).to_string();
    if name.is_empty() {
        display_name.trim().to_string()
    } else {
        name
    }
}

/// True when a compacted token is specific enough to identify a program on its own.
pub fn is_distinctive(token: &str) -> bool {
    token.chars().count() >= 3
        && !GENERIC_TOKENS.contains(&token)
        && !token.chars().all(|c| c.is_ascii_digit())
}

fn looks_like_guid(text: &str) -> bool {
    let t = text.trim_matches(['{', '}']);
    t.len() == 36 && t.chars().filter(|c| *c == '-').count() == 4
}

/// Distinctive compacted names a leftover folder or key may carry for this program.
#[derive(Debug, Clone, Default)]
pub struct NameSet {
    names: HashSet<String>,
}

impl NameSet {
    pub fn build(display_name: &str, key_name: &str, install_location: &str) -> Self {
        let mut names = HashSet::new();
        let mut add = |raw: &str| {
            let token = compact(raw);
            if is_distinctive(&token) {
                names.insert(token);
            }
        };
        add(display_name);
        add(&product_name(display_name));
        // Inno Setup keys end with "_is1"; MSI keys are GUIDs and never folder names.
        let key = key_name.trim_end_matches("_is1");
        if !looks_like_guid(key) {
            add(key);
            add(&product_name(key));
        }
        if let Some(leaf) = Path::new(install_location.trim_end_matches(['\\', '/'])).file_name() {
            let leaf = leaf.to_string_lossy();
            add(&leaf);
            add(&product_name(&leaf));
        }
        Self { names }
    }

    pub fn matches(&self, candidate: &str) -> bool {
        let token = compact(candidate);
        is_distinctive(&token) && self.names.contains(&token)
    }

    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }
}

/// Compare two paths case-insensitively with normalized separators.
pub fn path_key(path: &Path) -> String {
    let text = path.to_string_lossy().replace('/', "\\");
    text.trim_end_matches('\\').to_lowercase()
}

/// `child` is `parent` or lives below it.
pub fn is_same_or_inside(child: &Path, parent: &Path) -> bool {
    let c = path_key(child);
    let p = path_key(parent);
    !p.is_empty() && (c == p || c.starts_with(&format!("{p}\\")))
}

/// Expands 8.3 short components of an existing path (`GetLongPathNameW` in production).
pub type ShortNameExpander = fn(&Path) -> Option<PathBuf>;

/// An 8.3 alias component such as `PROGRA~1` or `MICROS~2.EXE`.
fn looks_like_short_name(component: &str) -> bool {
    let (base, ext) = match component.rfind('.') {
        Some(i) => (&component[..i], &component[i + 1..]),
        None => (component, ""),
    };
    let Some(tilde) = base.find('~') else { return false };
    let digits = &base[tilde + 1..];
    tilde > 0
        && base.len() <= 8
        && ext.len() <= 3
        && !digits.is_empty()
        && digits.chars().all(|c| c.is_ascii_digit())
}

/// Split an absolute drive path (`C:\...`, `\\?\C:\...`, `/` separators) into its drive prefix
/// (`C:`) and non-empty components. `None` for relative, UNC, device, or drive-relative paths.
fn split_drive_path(raw: &Path) -> Option<(String, Vec<String>)> {
    let text = raw.to_string_lossy().replace('/', "\\");
    let text = text.trim();
    let text = text.strip_prefix("\\\\?\\").unwrap_or(text);
    let bytes = text.as_bytes();
    if bytes.len() < 3 || !bytes[0].is_ascii_alphabetic() || bytes[1] != b':' || bytes[2] != b'\\' {
        return None;
    }
    let components = text[3..].split('\\').filter(|c| !c.is_empty()).map(str::to_string).collect();
    Some((text[..2].to_ascii_uppercase(), components))
}

fn join_drive_path(drive: &str, components: &[String]) -> PathBuf {
    PathBuf::from(format!("{drive}\\{}", components.join("\\")))
}

/// Policy form of a leftover candidate, or `None` when it must be rejected.
///
/// Raw strings compared against protected roots can be bypassed by spellings Win32 resolves to
/// the same object: `C:\PROGRA~1`, `C:\Program Files\Vendor\..`, `C:\Windows.` (trailing dots and
/// spaces are stripped per component). Candidates are therefore normalized before every policy
/// check: `.` components collapse; `..` components, trailing dot/space components, stream or
/// wildcard characters reject the path; 8.3 components are expanded with `expand` (only possible
/// when the path exists) and the path is rejected when they cannot be expanded.
pub fn normalize_candidate(raw: &Path, expand: Option<ShortNameExpander>) -> Option<PathBuf> {
    let (drive, components) = split_drive_path(raw)?;
    let mut kept = Vec::with_capacity(components.len());
    for component in components {
        if component == "." {
            continue;
        }
        let ambiguous = component == ".."
            || component.ends_with('.')
            || component.ends_with(' ')
            || component.contains([':', '*', '?', '"', '<', '>', '|']);
        if ambiguous {
            return None;
        }
        kept.push(component);
    }
    let path = join_drive_path(&drive, &kept);
    if !kept.iter().any(|c| looks_like_short_name(c)) {
        return Some(path);
    }
    let expanded = expand?(&path)?;
    let (drive, components) = split_drive_path(&expanded)?;
    let still_ambiguous = components.iter().any(|c| {
        c == "." || c == ".." || c.ends_with('.') || c.ends_with(' ') || looks_like_short_name(c)
    });
    (!still_ambiguous).then(|| join_drive_path(&drive, &components))
}

/// Policy form of a trusted root (environment folders such as `%TEMP%`, which is often an 8.3
/// path): `.` / `..` collapse lexically, trailing dots and spaces are stripped as Win32 does, and
/// short components are expanded when possible. Never rejects; unparsable roots stay as given.
fn normalize_root(raw: &Path, expand: Option<ShortNameExpander>) -> PathBuf {
    let Some((drive, components)) = split_drive_path(raw) else { return raw.to_path_buf() };
    let mut kept: Vec<String> = Vec::with_capacity(components.len());
    for component in components {
        let trimmed = component.trim_end_matches(['.', ' ']);
        match (component.as_str(), trimmed) {
            (".", _) => {}
            ("..", _) => {
                kept.pop();
            }
            (_, "") => {}
            _ => kept.push(trimmed.to_string()),
        }
    }
    let path = join_drive_path(&drive, &kept);
    if kept.iter().any(|c| looks_like_short_name(c)) {
        if let Some(long) = expand.and_then(|f| f(&path)) {
            return normalize_root(&long, None);
        }
    }
    path
}

/// Roots that must never be removed themselves (their children may be).
#[derive(Debug, Clone, Default)]
pub struct ProtectedRoots {
    exact: Vec<PathBuf>,
    /// Everything at or under these is off limits (for example `C:\Windows`).
    subtree: Vec<PathBuf>,
    /// Shell folders that live inside a protected subtree (Start Menu, pinned taskbar items under
    /// `AppData\Roaming\Microsoft`) but whose *children* are per-program shortcuts that may be
    /// removed. The folders themselves stay protected via `exact`.
    shell_children: Vec<PathBuf>,
    /// `%LOCALAPPDATA%\Packages`: its direct children are Store package data folders.
    store_packages: Option<PathBuf>,
    /// 8.3 expansion used by [`Self::normalize`]; `None` rejects every short-name candidate.
    expand: Option<ShortNameExpander>,
}

impl ProtectedRoots {
    #[cfg(test)]
    pub fn new(exact: Vec<PathBuf>, subtree: Vec<PathBuf>) -> Self {
        Self {
            exact,
            subtree,
            shell_children: Vec::new(),
            store_packages: None,
            expand: None,
        }
        .normalized()
    }

    #[cfg(test)]
    pub fn with_shell_children(mut self, roots: Vec<PathBuf>) -> Self {
        self.shell_children = roots;
        self.normalized()
    }

    #[cfg(test)]
    pub fn with_expander(mut self, expand: ShortNameExpander) -> Self {
        self.expand = Some(expand);
        self.normalized()
    }

    #[cfg(test)]
    pub fn with_store_packages(mut self, root: PathBuf) -> Self {
        self.store_packages = Some(root);
        self.normalized()
    }

    fn normalized(mut self) -> Self {
        let expand = self.expand;
        for list in [&mut self.exact, &mut self.subtree, &mut self.shell_children] {
            for root in list.iter_mut() {
                *root = normalize_root(root, expand);
            }
        }
        self.store_packages = self.store_packages.map(|p| normalize_root(&p, expand));
        self
    }

    /// Normalize a candidate with this policy's short-name expansion (see [`normalize_candidate`]).
    pub fn normalize(&self, path: &Path) -> Option<PathBuf> {
        normalize_candidate(path, self.expand)
    }

    /// Normalize a trusted root (another program's install folder) for policy comparisons.
    pub fn normalize_root(&self, path: &Path) -> PathBuf {
        normalize_root(path, self.expand)
    }

    /// `path` is exactly `%LOCALAPPDATA%\Packages\<family>`, the one sanctioned child of a
    /// protected root (Store package data).
    pub fn is_store_package_folder(&self, path: &Path) -> bool {
        let Some(root) = &self.store_packages else { return false };
        let Some(path) = self.normalize(path) else { return false };
        path.parent().is_some_and(|parent| path_key(parent) == path_key(root))
            && path.file_name().is_some()
    }

    /// Build from the current process environment.
    pub fn from_environment() -> Self {
        let env = |key: &str| std::env::var_os(key).map(PathBuf::from);
        let mut exact = Vec::new();
        let mut subtree = Vec::new();
        for key in [
            "ProgramFiles",
            "ProgramFiles(x86)",
            "ProgramW6432",
            "ProgramData",
            "APPDATA",
            "LOCALAPPDATA",
            "USERPROFILE",
            "PUBLIC",
            "ALLUSERSPROFILE",
            "CommonProgramFiles",
            "CommonProgramFiles(x86)",
            "TEMP",
            "TMP",
        ] {
            if let Some(path) = env(key) {
                exact.push(path);
            }
        }
        if let Some(profile) = env("USERPROFILE") {
            for child in [
                "Desktop",
                "Documents",
                "Downloads",
                "Music",
                "Pictures",
                "Videos",
                "OneDrive",
                "AppData",
                "AppData\\LocalLow",
                "AppData\\Local\\Programs",
                "AppData\\Local\\Packages",
                "AppData\\Local\\Temp",
            ] {
                exact.push(profile.join(child));
            }
            if let Some(users) = profile.parent() {
                exact.push(users.to_path_buf());
            }
        }
        let mut shell_children = Vec::new();
        if let Some(appdata) = env("APPDATA") {
            shell_children.push(appdata.join("Microsoft\\Windows\\Start Menu\\Programs"));
            shell_children.push(
                appdata.join("Microsoft\\Internet Explorer\\Quick Launch\\User Pinned\\TaskBar"),
            );
            exact.push(appdata.join("Microsoft\\Internet Explorer\\Quick Launch\\User Pinned\\TaskBar"));
            exact.push(appdata.join("Microsoft\\Windows\\Start Menu"));
            exact.push(appdata.join("Microsoft\\Windows\\Start Menu\\Programs"));
            exact.push(appdata.join("Microsoft\\Windows\\Start Menu\\Programs\\Startup"));
        }
        if let Some(data) = env("ProgramData") {
            shell_children.push(data.join("Microsoft\\Windows\\Start Menu\\Programs"));
            exact.push(data.join("Microsoft\\Windows\\Start Menu"));
            exact.push(data.join("Microsoft\\Windows\\Start Menu\\Programs"));
            exact.push(data.join("Microsoft\\Windows\\Start Menu\\Programs\\Startup"));
            subtree.push(data.join("Microsoft"));
            subtree.push(data.join("Packages"));
        }
        if let Some(windows) = env("SystemRoot").or_else(|| env("windir")) {
            subtree.push(windows);
        }
        if let Some(local) = env("LOCALAPPDATA") {
            subtree.push(local.join("Microsoft"));
        }
        if let Some(appdata) = env("APPDATA") {
            subtree.push(appdata.join("Microsoft"));
        }
        if let Some(files) = env("ProgramFiles") {
            subtree.push(files.join("WindowsApps"));
            subtree.push(files.join("Windows Defender"));
        }
        Self {
            exact,
            subtree,
            shell_children,
            store_packages: env("LOCALAPPDATA").map(|local| local.join("Packages")),
            expand: Some(pctoolkit_platform::long_path_name),
        }
        .normalized()
    }

    /// The candidate is protected (or too ambiguous to judge). Normalizes first, so callers may
    /// pass raw paths from the registry or the file system.
    pub fn is_protected(&self, path: &Path) -> bool {
        match self.normalize(path) {
            Some(normalized) => self.is_protected_normalized(&normalized),
            None => true,
        }
    }

    fn is_protected_normalized(&self, path: &Path) -> bool {
        let key = path_key(path);
        // Only absolute drive paths below a drive root are ever eligible; drive roots ("c:"),
        // relative, UNC, and empty paths are always protected.
        let bytes = key.as_bytes();
        let absolute =
            bytes.len() > 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] == b'\\';
        if !absolute {
            return true;
        }
        if self.exact.iter().any(|root| path_key(root) == key) {
            return true;
        }
        // An ancestor of a protected root (for example "C:\Users") is protected as well.
        if self
            .exact
            .iter()
            .chain(self.subtree.iter())
            .any(|root| is_same_or_inside(root, path) && path_key(root) != key)
        {
            return true;
        }
        // Shortcuts and folders inside the Start Menu / pinned taskbar folders are eligible even
        // though those folders sit under the protected `AppData\Roaming\Microsoft` subtree.
        if self
            .shell_children
            .iter()
            .any(|root| is_same_or_inside(path, root) && path_key(root) != key)
        {
            return false;
        }
        self.subtree.iter().any(|root| is_same_or_inside(path, root))
    }
}

/// A path is shared when another still-installed program lives at, inside, or above it.
pub fn is_shared_with(path: &Path, other_locations: &[PathBuf]) -> bool {
    other_locations
        .iter()
        .any(|other| is_same_or_inside(other, path) || is_same_or_inside(path, other))
}

/// Reduce an image path / command line to the first absolute path it mentions:
/// `"C:\App\svc.exe" -k` → `C:\App\svc.exe`, `C:\App\app.exe,0` → `C:\App\app.exe`.
pub fn first_path(command: &str) -> Option<PathBuf> {
    let text = command.trim();
    let text = text.strip_prefix(r"\??\").unwrap_or(text);
    let candidate = if let Some(rest) = text.strip_prefix('"') {
        rest.split('"').next()?.to_string()
    } else {
        let lower = text.to_ascii_lowercase();
        match lower.find(".exe") {
            Some(i) => text[..i + 4].to_string(),
            None => text.split(',').next()?.trim().to_string(),
        }
    };
    let candidate = candidate.trim().to_string();
    let bytes = candidate.as_bytes();
    (bytes.len() > 3 && bytes[1] == b':' && (bytes[2] == b'\\' || bytes[2] == b'/'))
        .then(|| PathBuf::from(candidate))
}

/// `haystack` mentions a path at or below `folder` (case-insensitive, separator-normalized).
pub fn mentions_folder(haystack: &str, folder: &Path) -> bool {
    let folder = path_key(folder);
    if folder.len() <= 3 {
        return false;
    }
    let hay = haystack.replace('/', "\\").to_lowercase();
    let needle_dir = format!("{folder}\\");
    hay.contains(&needle_dir) || hay.ends_with(&folder) || hay.contains(&format!("{folder}\""))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn product_name_strips_versions_and_arch() {
        assert_eq!(product_name("Zed 0.150.4 (x64)"), "Zed");
        assert_eq!(product_name("7-Zip 24.08 (x64 edition)"), "7-Zip");
        assert_eq!(product_name("Bitvise SSH Client 9.66 (remove only)"), "Bitvise SSH Client");
        assert_eq!(product_name("Notepad++ (64-bit x64)"), "Notepad++");
        assert_eq!(product_name("App v2.1"), "App");
    }

    #[test]
    fn generic_tokens_never_match() {
        let set = NameSet::build("Microsoft Update Health Tools", "", "C:\\Program Files\\Microsoft");
        assert!(!set.matches("Microsoft"));
        assert!(!set.matches("Update"));
        assert!(set.matches("Microsoft Update Health Tools"));
    }

    #[test]
    fn name_set_uses_install_folder_and_inno_key() {
        let set = NameSet::build(
            "Ghost Downloader version 4.2.6",
            "{C7E5C3F5-8579-4C76-BC5F-2D18D82FD9B8}_is1",
            "C:\\Users\\u\\AppData\\Local\\Programs\\Ghost Downloader",
        );
        assert!(set.matches("Ghost Downloader"));
        assert!(set.matches("ghost-downloader"));
        assert!(!set.matches("Ghost"));
        assert!(!set.matches("C7E5C3F5-8579-4C76-BC5F-2D18D82FD9B8"));
    }

    #[test]
    fn exact_match_only_no_substring() {
        let set = NameSet::build("Docker Desktop", "Docker Desktop", "");
        assert!(set.matches("Docker Desktop"));
        assert!(!set.matches("Docker"));
        assert!(!set.matches("Docker Desktop Helper"));
    }

    fn roots() -> ProtectedRoots {
        ProtectedRoots::new(
            vec![
                PathBuf::from("C:\\Program Files"),
                PathBuf::from("C:\\Users\\u\\AppData\\Roaming"),
                PathBuf::from("C:\\Users\\u"),
            ],
            vec![PathBuf::from("C:\\Windows")],
        )
    }

    #[test]
    fn protected_roots_block_roots_ancestors_and_windows() {
        let r = roots();
        assert!(r.is_protected(Path::new("C:\\")));
        assert!(r.is_protected(Path::new("C:")));
        assert!(r.is_protected(Path::new("C:\\Program Files\\")));
        assert!(r.is_protected(Path::new("c:/program files")));
        assert!(r.is_protected(Path::new("C:\\Users")));
        assert!(r.is_protected(Path::new("C:\\Windows\\System32\\drivers")));
        assert!(r.is_protected(Path::new("relative\\path")));
        assert!(!r.is_protected(Path::new("C:\\Program Files\\Zed")));
        assert!(!r.is_protected(Path::new("C:\\Users\\u\\AppData\\Roaming\\Zed")));
    }

    /// Stand-in for `GetLongPathNameW`: knows two 8.3 aliases, fails for everything else.
    fn fake_expand(path: &Path) -> Option<PathBuf> {
        let key = path_key(path);
        for (short, long) in [("c:\\progra~1", "C:\\Program Files"), ("c:\\users\\longna~1", "C:\\Users\\Long Name")] {
            if key == short || key.starts_with(&format!("{short}\\")) {
                return Some(PathBuf::from(format!("{long}{}", &key[short.len()..])));
            }
        }
        None
    }

    #[test]
    fn short_names_dot_segments_and_trailing_dots_cannot_bypass_protection() {
        let plain = roots();
        let expanding = roots().with_expander(fake_expand);
        for r in [&plain, &expanding] {
            assert!(r.is_protected(Path::new("C:\\PROGRA~1")));
            assert!(r.is_protected(Path::new("C:\\Program Files\\Vendor\\..")));
            assert!(r.is_protected(Path::new("C:\\Program Files\\Zed\\..\\..\\Windows")));
            assert!(r.is_protected(Path::new("C:\\Windows.")));
            assert!(r.is_protected(Path::new("C:\\Windows. .")));
            assert!(r.is_protected(Path::new("C:\\Windows \\System32")));
            assert!(r.is_protected(Path::new("\\\\?\\C:\\Windows\\Temp")));
            assert!(r.is_protected(Path::new("C:\\Program Files\\Zed::$DATA")));
            assert!(r.is_protected(Path::new("C:\\Program Files\\.")));
        }
        // An unexpandable 8.3 alias is rejected; an expandable one is judged by its long form.
        assert!(plain.is_protected(Path::new("C:\\PROGRA~1\\Zed")));
        assert!(!expanding.is_protected(Path::new("C:\\PROGRA~1\\Zed")));
        assert!(expanding.is_protected(Path::new("C:\\NOTKNO~1\\Zed")));
        // `.` components collapse instead of rejecting the path.
        assert!(!plain.is_protected(Path::new("C:\\Program Files\\.\\Zed")));
    }

    #[test]
    fn normalize_candidate_collapses_separators_and_rejects_ambiguity() {
        assert_eq!(
            normalize_candidate(Path::new("c:/Program Files/./Zed/"), None),
            Some(PathBuf::from("C:\\Program Files\\Zed"))
        );
        assert_eq!(
            normalize_candidate(Path::new("C:\\PROGRA~1\\Zed"), Some(fake_expand)),
            Some(PathBuf::from("C:\\Program Files\\zed"))
        );
        assert_eq!(normalize_candidate(Path::new("C:\\A\\..\\B"), None), None);
        assert_eq!(normalize_candidate(Path::new("C:\\A.\\B"), None), None);
        assert_eq!(normalize_candidate(Path::new("C:\\A \\B"), None), None);
        assert_eq!(normalize_candidate(Path::new("relative\\x"), None), None);
        assert_eq!(normalize_candidate(Path::new("\\\\server\\share\\x"), None), None);
        // Names that merely contain '~' are not 8.3 aliases.
        assert!(normalize_candidate(Path::new("C:\\Tools\\~cache"), None).is_some());
        assert!(normalize_candidate(Path::new("C:\\Tools\\my~app-data"), None).is_some());
    }

    #[test]
    fn protected_roots_from_short_or_dotted_environment_values_are_normalized() {
        let r = ProtectedRoots::new(
            vec![PathBuf::from("C:\\Users\\LONGNA~1\\AppData\\Local\\Temp"), PathBuf::from("C:\\Data\\X\\..\\Shared.")],
            vec![],
        )
        .with_expander(fake_expand);
        assert!(r.is_protected(Path::new("C:\\Users\\Long Name\\AppData\\Local\\Temp")));
        assert!(r.is_protected(Path::new("C:\\Data\\Shared")));
        assert!(!r.is_protected(Path::new("C:\\Data\\Shared\\App")));
    }

    #[test]
    fn only_direct_children_of_local_packages_are_store_folders() {
        let packages = PathBuf::from("C:\\Users\\u\\AppData\\Local\\Packages");
        let r = roots().with_store_packages(packages.clone());
        assert!(r.is_store_package_folder(&packages.join("Vendor.App_abc123")));
        assert!(!r.is_store_package_folder(&packages));
        assert!(!r.is_store_package_folder(&packages.join("Vendor.App_abc123\\LocalState")));
        assert!(!r.is_store_package_folder(Path::new("C:\\Windows\\Packages\\X")));
        assert!(!r.is_store_package_folder(&packages.join("..\\..\\Roaming")));
    }

    #[test]
    fn start_menu_children_are_allowed_inside_protected_appdata_microsoft() {
        let programs = PathBuf::from("C:\\Users\\u\\AppData\\Roaming\\Microsoft\\Windows\\Start Menu\\Programs");
        let r = ProtectedRoots::new(
            vec![programs.clone(), programs.join("Startup")],
            vec![PathBuf::from("C:\\Users\\u\\AppData\\Roaming\\Microsoft")],
        )
        .with_shell_children(vec![programs.clone()]);
        assert!(!r.is_protected(&programs.join("Telegram Desktop")));
        assert!(!r.is_protected(&programs.join("Vendor\\App.lnk")));
        assert!(!r.is_protected(&programs.join("Startup\\App.lnk")));
        assert!(r.is_protected(&programs));
        assert!(r.is_protected(&programs.join("Startup")));
        assert!(r.is_protected(Path::new("C:\\Users\\u\\AppData\\Roaming\\Microsoft\\Windows\\Themes")));
    }

    #[test]
    fn shared_guard_rejects_parent_and_child_of_other_programs() {
        let others = vec![PathBuf::from("C:\\Program Files\\EaseUS\\Partition Master")];
        assert!(is_shared_with(Path::new("C:\\Program Files\\EaseUS"), &others));
        assert!(is_shared_with(
            Path::new("C:\\Program Files\\EaseUS\\Partition Master\\bin"),
            &others
        ));
        assert!(!is_shared_with(
            Path::new("C:\\Program Files\\EaseUS\\Data Recovery Wizard"),
            &others
        ));
    }

    #[test]
    fn same_or_inside_does_not_confuse_prefix_siblings() {
        assert!(!is_same_or_inside(
            Path::new("C:\\Program Files\\AppX"),
            Path::new("C:\\Program Files\\App")
        ));
        assert!(is_same_or_inside(
            Path::new("C:\\Program Files\\App\\x"),
            Path::new("C:\\Program Files\\App")
        ));
    }

    #[test]
    fn first_path_handles_quotes_args_and_icon_index() {
        assert_eq!(
            first_path("\"C:\\Program Files\\App\\svc.exe\" -k run"),
            Some(PathBuf::from("C:\\Program Files\\App\\svc.exe"))
        );
        assert_eq!(
            first_path("C:\\Program Files\\App\\app.exe,0"),
            Some(PathBuf::from("C:\\Program Files\\App\\app.exe"))
        );
        assert_eq!(
            first_path("\\??\\C:\\Drivers\\x.exe"),
            Some(PathBuf::from("C:\\Drivers\\x.exe"))
        );
        assert_eq!(first_path("MsiExec.exe /X{1}"), None);
        assert_eq!(first_path("%SystemRoot%\\x.exe"), None);
    }

    #[test]
    fn mentions_folder_requires_boundary() {
        let folder = Path::new("C:\\Program Files\\App");
        assert!(mentions_folder("\"C:\\Program Files\\App\\app.exe\" --tray", folder));
        assert!(mentions_folder("App=C:\\Program Files\\App\\app.exe|", folder));
        assert!(!mentions_folder("C:\\Program Files\\AppX\\app.exe", folder));
        assert!(!mentions_folder("anything", Path::new("C:\\")));
    }
}
