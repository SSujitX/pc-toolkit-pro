//! Root resolution: base folders, env overrides, glob / child expansion, safety
//! validation, and cross-rule ownership.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use super::catalog::{BaseVar, CatalogRule, ChildSelector, RootSpec, RuleKind};
use super::matcher::glob_match;
use super::safety::{
    drive_root, is_link_like, is_strict_descendant, path_key, validate_root, RootPolicy,
    RootRefusal, SafetyContext,
};

/// Resolved base folders. `from_env` reads the process environment.
#[derive(Debug, Clone, Default)]
pub(crate) struct BaseDirs {
    pub local: Option<PathBuf>,
    pub roaming: Option<PathBuf>,
    pub home: Option<PathBuf>,
    pub temp: Option<PathBuf>,
    pub program_data: Option<PathBuf>,
    pub windir: Option<PathBuf>,
    pub system_drive: Option<PathBuf>,
}

fn env_dir(key: &str) -> Option<PathBuf> {
    std::env::var_os(key)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

impl BaseDirs {
    pub fn from_env() -> Self {
        Self {
            local: env_dir("LOCALAPPDATA"),
            roaming: env_dir("APPDATA"),
            home: env_dir("USERPROFILE"),
            temp: env_dir("TEMP").or_else(|| Some(std::env::temp_dir())),
            program_data: env_dir("ProgramData"),
            windir: env_dir("WINDIR").or_else(|| env_dir("SystemRoot")),
            system_drive: env_dir("SystemDrive").map(|d| drive_root(&d)),
        }
    }

    fn get(&self, base: BaseVar) -> Option<&PathBuf> {
        match base {
            BaseVar::Local => self.local.as_ref(),
            BaseVar::Roaming => self.roaming.as_ref(),
            BaseVar::Home => self.home.as_ref(),
            BaseVar::Temp => self.temp.as_ref(),
            BaseVar::ProgramData => self.program_data.as_ref(),
            BaseVar::Windir => self.windir.as_ref(),
            BaseVar::SystemDrive => self.system_drive.as_ref(),
        }
    }
}

enum Segment<'a> {
    Literal(&'a str),
    Glob(String),
    Children(&'a ChildSelector),
}

fn parse_segments(path: &str) -> impl Iterator<Item = Segment<'_>> {
    path.split('/').filter(|s| !s.is_empty()).map(|s| {
        if s.contains('*') || s.contains('?') {
            Segment::Glob(s.to_lowercase())
        } else {
            Segment::Literal(s)
        }
    })
}

/// Real (non-link) child directories of `dir` whose lowercase name passes `keep`.
fn child_dirs(dir: &Path, keep: &dyn Fn(&str) -> bool) -> Vec<PathBuf> {
    let Ok(read) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in read.flatten() {
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if is_link_like(&meta) || !meta.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_lowercase();
        if keep(&name) {
            out.push(entry.path());
        }
    }
    out.sort();
    out
}

fn expand(start: Vec<PathBuf>, segments: Vec<Segment<'_>>) -> Vec<PathBuf> {
    let mut current = start;
    for segment in segments {
        let mut next = Vec::new();
        for dir in &current {
            match &segment {
                Segment::Literal(name) => next.push(dir.join(name)),
                Segment::Glob(pattern) => next.extend(child_dirs(dir, &|n| glob_match(pattern, n))),
                Segment::Children(selector) => next.extend(child_dirs(dir, &|n| selector.matches(n))),
            }
        }
        current = next;
        if current.is_empty() {
            break;
        }
    }
    current
}

/// Candidate paths for one root (unvalidated; may not exist). `lookup` reads env vars.
pub(crate) fn resolve_root_candidates(
    root: &RootSpec,
    bases: &BaseDirs,
    lookup: &dyn Fn(&str) -> Option<String>,
) -> Vec<PathBuf> {
    let mut start: Option<PathBuf> = None;
    for over in &root.env {
        let Some(value) = lookup(&over.var).filter(|v| !v.trim().is_empty()) else {
            continue;
        };
        if value.trim().eq_ignore_ascii_case("off") {
            return Vec::new();
        }
        // GOPATH-style lists: the first entry is the active one.
        let first = value.split(';').next().unwrap_or("").trim();
        let mut candidate = PathBuf::from(first);
        if !candidate.is_absolute() {
            continue;
        }
        // `env.path` is literal (validation rejects globs there).
        for seg in over.path.split('/').filter(|s| !s.is_empty()) {
            candidate.push(seg);
        }
        start = Some(candidate);
        break;
    }
    let mut segments: Vec<Segment<'_>> = Vec::new();
    let base_dir = match start {
        Some(dir) => dir,
        None => {
            let Some(base) = bases.get(root.base) else {
                return Vec::new();
            };
            segments.extend(parse_segments(&root.path));
            base.clone()
        }
    };
    if let Some(children) = &root.children {
        segments.push(Segment::Children(children));
    }
    let parents = expand(vec![base_dir], segments);
    if root.suffixes.is_empty() {
        return parents;
    }
    let mut out = Vec::new();
    for suffix in &root.suffixes {
        out.extend(expand(parents.clone(), parse_segments(suffix).collect()));
    }
    out
}

/// A rule's validated roots plus diagnostics.
#[derive(Debug, Clone, Default)]
pub(crate) struct ResolvedRule {
    pub roots: Vec<PathBuf>,
    /// Roots that exist but failed the safety policy.
    pub refused: u32,
}

pub(crate) fn root_policy(rule: &CatalogRule) -> RootPolicy {
    RootPolicy {
        allow_personal_downloads: rule.spec.allow_personal_folder,
        allow_user_content: false,
    }
}

/// Resolve, validate, and deduplicate a rule's roots. Nested roots of the same
/// rule collapse into the outer root.
pub(crate) fn resolve_rule(rule: &CatalogRule, bases: &BaseDirs, safety: &SafetyContext) -> ResolvedRule {
    if rule.kind != RuleKind::Files {
        return ResolvedRule::default();
    }
    let lookup = |key: &str| std::env::var(key).ok();
    let policy = root_policy(rule);
    let mut seen = HashSet::new();
    let mut valid: Vec<PathBuf> = Vec::new();
    let mut refused = 0u32;
    for spec in &rule.spec.roots {
        for candidate in resolve_root_candidates(spec, bases, &lookup) {
            if !seen.insert(path_key(&candidate)) {
                continue;
            }
            match validate_root(&candidate, safety, policy) {
                Ok(()) => valid.push(candidate),
                Err(RootRefusal::Missing | RootRefusal::Inaccessible) => {}
                Err(_) => refused += 1,
            }
        }
    }
    let roots = valid
        .iter()
        .filter(|r| !valid.iter().any(|other| is_strict_descendant(r, other)))
        .cloned()
        .collect();
    ResolvedRule { roots, refused }
}

/// For each rule, the roots of *other* rules that lie strictly inside its roots.
/// Those subtrees belong to the more specific rule and are skipped by the walk,
/// whether or not that rule is selected, so an unselected child rule protects
/// its files and nothing is counted twice.
pub(crate) fn ownership_exclusions(resolved: &[ResolvedRule]) -> Vec<HashSet<String>> {
    resolved
        .iter()
        .enumerate()
        .map(|(i, rule)| {
            let mut excluded = HashSet::new();
            for (j, other) in resolved.iter().enumerate() {
                if i == j {
                    continue;
                }
                for inner in &other.roots {
                    if rule.roots.iter().any(|outer| is_strict_descendant(inner, outer)) {
                        excluded.insert(path_key(inner));
                    }
                }
            }
            excluded
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cleaner_deep::catalog::EnvOverride;

    fn temp_base(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("pctoolkit-resolve-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn root(path: &str) -> RootSpec {
        RootSpec {
            base: BaseVar::Local,
            path: path.into(),
            env: Vec::new(),
            children: None,
            suffixes: Vec::new(),
        }
    }

    #[test]
    fn expands_children_globs_and_suffixes() {
        let base = temp_base("expand");
        for dir in ["User Data/Default", "User Data/Profile 2", "User Data/System Profile", "User Data/Other"] {
            fs::create_dir_all(base.join(dir)).unwrap();
        }
        let bases = BaseDirs { local: Some(base.clone()), ..BaseDirs::default() };
        let mut spec = root("User Data");
        spec.children = Some(ChildSelector {
            names: vec!["default".into()],
            prefixes: vec!["profile ".into()],
            ..ChildSelector::default()
        });
        spec.suffixes = vec!["Cache".into(), "Code Cache".into()];
        let got = resolve_root_candidates(&spec, &bases, &|_| None);
        let names: Vec<String> = got
            .iter()
            .map(|p| p.strip_prefix(&base).unwrap().to_string_lossy().replace('\\', "/"))
            .collect();
        assert_eq!(names.len(), 4, "{names:?}");
        assert!(names.contains(&"User Data/Default/Cache".to_string()));
        assert!(names.contains(&"User Data/Profile 2/Code Cache".to_string()));

        let globbed = resolve_root_candidates(&root("User Data/Profile */Cache"), &bases, &|_| None);
        assert_eq!(globbed.len(), 1);
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn env_override_replaces_base_and_off_disables() {
        let base = temp_base("env");
        let custom = base.join("custom-home");
        fs::create_dir_all(&custom).unwrap();
        let bases = BaseDirs { local: Some(base.join("local")), ..BaseDirs::default() };
        let mut spec = root("go-build");
        spec.env = vec![EnvOverride { var: "GOCACHE".into(), path: String::new() }];
        let custom_text = custom.to_string_lossy().to_string();
        let got = resolve_root_candidates(&spec, &bases, &|k| (k == "GOCACHE").then(|| custom_text.clone()));
        assert_eq!(got, vec![custom.clone()]);
        let off = resolve_root_candidates(&spec, &bases, &|k| (k == "GOCACHE").then(|| "off".to_string()));
        assert!(off.is_empty());
        let unset = resolve_root_candidates(&spec, &bases, &|_| None);
        assert_eq!(unset, vec![base.join("local").join("go-build")]);
        // Relative env values are ignored (fall through to the default base).
        let rel = resolve_root_candidates(&spec, &bases, &|_| Some("relative\\dir".into()));
        assert_eq!(rel, vec![base.join("local").join("go-build")]);
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn ownership_assigns_nested_roots_to_the_inner_rule() {
        let outer = ResolvedRule { roots: vec![PathBuf::from(r"C:\T\Temp")], refused: 0 };
        let inner = ResolvedRule { roots: vec![PathBuf::from(r"C:\T\Temp\Low")], refused: 0 };
        let unrelated = ResolvedRule { roots: vec![PathBuf::from(r"C:\T\Other")], refused: 0 };
        let ex = ownership_exclusions(&[outer, inner, unrelated]);
        assert!(ex[0].contains(&path_key(Path::new(r"C:\T\Temp\Low"))));
        assert!(ex[1].is_empty());
        assert!(ex[2].is_empty());
    }
}
