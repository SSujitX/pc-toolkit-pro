//! Embedded TOML rule catalog: schema, load, and validation.
//!
//! The catalog is compiled into the binary (`include_str!`). Validation runs on
//! first use and in `tests::embedded_catalog_is_valid`, which is the gate every
//! contributor-added rule must pass. Schema reference: `rules/deep_cleanup/README.md`.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::OnceLock;

use super::matcher::MatcherSpec;
use super::safety::{PERSONAL_FOLDERS, PROTECTED_HOME_PREFIXES, SYSTEM_DRIVE_ALLOWLIST, VCS_SEGMENTS, WINDIR_ALLOWLIST};
use super::special::SpecialKind;
use super::walk::CleanMode;
use super::{DeepCleanupGroup, DeepCleanupRisk};

/// Embedded catalog files and the group every rule in each file must declare.
/// `None` = `special.toml`: special kinds only, each in the group its kind owns.
const CATALOG_SOURCES: &[(&str, Option<DeepCleanupGroup>, &str)] = &[
    ("system.toml", Some(DeepCleanupGroup::System), include_str!("../../rules/deep_cleanup/system.toml")),
    (
        "application.toml",
        Some(DeepCleanupGroup::Application),
        include_str!("../../rules/deep_cleanup/application.toml"),
    ),
    ("browser.toml", Some(DeepCleanupGroup::Browser), include_str!("../../rules/deep_cleanup/browser.toml")),
    (
        "development.toml",
        Some(DeepCleanupGroup::Development),
        include_str!("../../rules/deep_cleanup/development.toml"),
    ),
    ("ai.toml", Some(DeepCleanupGroup::Ai), include_str!("../../rules/deep_cleanup/ai.toml")),
    ("special.toml", None, include_str!("../../rules/deep_cleanup/special.toml")),
];

const MAX_ROOTS_PER_RULE: usize = 32;

/// Base folder a root is relative to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BaseVar {
    /// `%LOCALAPPDATA%`
    Local,
    /// `%APPDATA%`
    Roaming,
    /// `%USERPROFILE%`
    Home,
    /// `%TEMP%` (falls back to the OS temp dir)
    Temp,
    /// `%PROGRAMDATA%`
    ProgramData,
    /// `%WINDIR%` — only paths in the Windows allow list.
    Windir,
    /// `%SystemDrive%\` — only paths in the system-drive allow list.
    SystemDrive,
}

/// Expand the direct child folders of the current path.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ChildSelector {
    #[serde(default)]
    pub names: Vec<String>,
    #[serde(default)]
    pub prefixes: Vec<String>,
    #[serde(default)]
    pub contains: Vec<String>,
    #[serde(default)]
    pub all: bool,
}

impl ChildSelector {
    pub(crate) fn is_empty(&self) -> bool {
        !self.all && self.names.is_empty() && self.prefixes.is_empty() && self.contains.is_empty()
    }

    pub(crate) fn matches(&self, name_lower: &str) -> bool {
        self.all
            || self.names.iter().any(|n| n == name_lower)
            || self.prefixes.iter().any(|p| name_lower.starts_with(p.as_str()))
            || self.contains.iter().any(|c| name_lower.contains(c.as_str()))
    }

    fn normalize(&mut self) {
        for list in [&mut self.names, &mut self.prefixes, &mut self.contains] {
            for item in list.iter_mut() {
                *item = item.to_lowercase();
            }
        }
    }
}

/// Environment override for a root base (for tools with configurable homes).
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct EnvOverride {
    pub var: String,
    /// Relative path appended to the variable's value.
    #[serde(default)]
    pub path: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RootSpec {
    pub base: BaseVar,
    /// Relative path with `/`. Segments may contain `*` / `?` globs over child folders.
    #[serde(default)]
    pub path: String,
    /// Tried in order; the first set variable replaces `base` + `path`. Value `off` disables the root.
    #[serde(default)]
    pub env: Vec<EnvOverride>,
    #[serde(default)]
    pub children: Option<ChildSelector>,
    /// Relative paths appended after `children` (or after `path`); one root per suffix.
    #[serde(default)]
    pub suffixes: Vec<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KindSpec {
    #[default]
    Files,
    RecycleBin,
    DiskCleanupInternetCache,
    DiskCleanupDeliveryOptimization,
    DiskCleanupDefenderCache,
    DiskCleanupUpdateCleanup,
    DiskCleanupPreviousInstallations,
    DiskCleanupErrorReports,
    DiskCleanupTemporarySetupFiles,
    DockerBuildCache,
    CondaCaches,
    RustToolchains,
}

impl KindSpec {
    /// The special kind this spec names; `None` for file rules.
    pub(crate) fn special(self) -> Option<SpecialKind> {
        use pctoolkit_platform::VolumeCacheKind as V;
        let disk = |kind| Some(SpecialKind::DiskCleanup(kind));
        match self {
            Self::Files => None,
            Self::RecycleBin => Some(SpecialKind::RecycleBin),
            Self::DiskCleanupInternetCache => disk(V::InternetCache),
            Self::DiskCleanupDeliveryOptimization => disk(V::DeliveryOptimization),
            Self::DiskCleanupDefenderCache => disk(V::DefenderCache),
            Self::DiskCleanupUpdateCleanup => disk(V::UpdateCleanup),
            Self::DiskCleanupPreviousInstallations => disk(V::PreviousInstallations),
            Self::DiskCleanupErrorReports => disk(V::ErrorReports),
            Self::DiskCleanupTemporarySetupFiles => disk(V::TemporarySetupFiles),
            Self::DockerBuildCache => Some(SpecialKind::DockerBuildCache),
            Self::CondaCaches => Some(SpecialKind::CondaCaches),
            Self::RustToolchains => Some(SpecialKind::RustToolchains),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuleSpec {
    pub id: String,
    pub group: DeepCleanupGroup,
    pub name_key: String,
    pub detail_key: String,
    pub risk: DeepCleanupRisk,
    #[serde(default)]
    pub recommended: bool,
    #[serde(default)]
    pub requires_elevation: bool,
    #[serde(default)]
    pub processes: Vec<String>,
    #[serde(default)]
    pub kind: KindSpec,
    #[serde(default)]
    pub mode: CleanMode,
    #[serde(default)]
    pub allow_personal_folder: bool,
    #[serde(default)]
    pub roots: Vec<RootSpec>,
    #[serde(default)]
    pub matcher: MatcherSpec,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CatalogFile {
    #[serde(default)]
    rule: Vec<RuleSpec>,
}

/// How a catalog rule is executed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleKind {
    /// Walk roots and delete matching files.
    Files,
    /// A dedicated system operation (see `special`).
    Special(SpecialKind),
}

#[derive(Debug, Clone)]
pub struct CatalogRule {
    pub spec: RuleSpec,
    pub kind: RuleKind,
}

/// One validation problem: rule id (or file name) + stable code + short detail.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogIssue {
    pub rule: String,
    pub code: &'static str,
    pub detail: String,
}

fn issue(rule: &str, code: &'static str, detail: impl Into<String>) -> CatalogIssue {
    CatalogIssue {
        rule: rule.to_string(),
        code,
        detail: detail.into(),
    }
}

static CATALOG: OnceLock<Result<Vec<CatalogRule>, Vec<CatalogIssue>>> = OnceLock::new();

/// The validated embedded catalog. An invalid catalog fails closed (no rules run).
pub(crate) fn catalog() -> Result<&'static [CatalogRule], &'static [CatalogIssue]> {
    match CATALOG.get_or_init(load_embedded) {
        Ok(rules) => Ok(rules.as_slice()),
        Err(issues) => Err(issues.as_slice()),
    }
}

/// Parse + validate the embedded catalog; returns every issue found.
pub fn validate_embedded_catalog() -> Result<usize, Vec<CatalogIssue>> {
    load_embedded().map(|rules| rules.len())
}

fn load_embedded() -> Result<Vec<CatalogRule>, Vec<CatalogIssue>> {
    let mut specs = Vec::new();
    let mut issues = Vec::new();
    for (file, group, text) in CATALOG_SOURCES {
        match parse_catalog_source(text) {
            Ok(rules) => {
                for rule in rules {
                    match group {
                        Some(group) if rule.group != *group => issues.push(issue(
                            &rule.id,
                            "groupMismatch",
                            format!("{file} only holds group {}", group.as_str()),
                        )),
                        None if rule.kind == KindSpec::Files => issues.push(issue(
                            &rule.id,
                            "fileRuleInSpecialFile",
                            format!("{file} only holds special kinds"),
                        )),
                        _ => {}
                    }
                    specs.push(rule);
                }
            }
            Err(e) => issues.push(issue(file, "parseError", e)),
        }
    }
    issues.extend(validate_rules(&specs));
    if !issues.is_empty() {
        return Err(issues);
    }
    Ok(specs.into_iter().map(into_catalog_rule).collect())
}

pub(crate) fn parse_catalog_source(text: &str) -> Result<Vec<RuleSpec>, String> {
    toml::from_str::<CatalogFile>(text)
        .map(|f| f.rule)
        .map_err(|e| e.to_string())
}

fn into_catalog_rule(mut spec: RuleSpec) -> CatalogRule {
    spec.matcher.normalize();
    for root in spec.roots.iter_mut() {
        if let Some(children) = root.children.as_mut() {
            children.normalize();
        }
    }
    let kind = spec.kind.special().map_or(RuleKind::Files, RuleKind::Special);
    CatalogRule { spec, kind }
}

fn is_glob(segment: &str) -> bool {
    segment.contains('*') || segment.contains('?')
}

fn segments(path: &str) -> Vec<&str> {
    if path.is_empty() {
        Vec::new()
    } else {
        path.split('/').collect()
    }
}

/// Lexical checks shared by `path`, `suffixes`, and `env.path`.
fn check_relative_path(rule: &str, path: &str, issues: &mut Vec<CatalogIssue>) {
    if path.is_empty() {
        return;
    }
    if path.contains('\\') || path.contains(':') || path.contains('%') || path.starts_with('/') {
        issues.push(issue(rule, "pathNotRelative", "use relative paths with '/' and no drive or %VAR%"));
        return;
    }
    for seg in path.split('/') {
        let lower = seg.to_lowercase();
        if seg.is_empty() || seg.trim() != seg {
            issues.push(issue(rule, "pathEmptySegment", "empty or padded path segment"));
        } else if seg == "." || seg == ".." {
            issues.push(issue(rule, "pathTraversal", "'.' and '..' are not allowed"));
        } else if VCS_SEGMENTS.contains(&lower.as_str()) {
            issues.push(issue(rule, "versionControl", "version-control folders are never roots"));
        } else if is_glob(seg) && seg.chars().all(|c| c == '*' || c == '?') {
            issues.push(issue(rule, "globTooBroad", "a glob segment needs at least one literal character"));
        }
    }
}

/// Segment names that must never appear in a broad (match-all) root.
const FORBIDDEN_BROAD_SEGMENTS: &[&str] = &[
    "accounts", "addressbook", "calendars", "checkpoints", "cloudstorage", "cookies", "credentials",
    "datasets", "keychains", "keys", "mail", "messages", "mobile documents", "models", "notes",
    "photos", "projects", "reminders", "sessions", "wallet", "wallets",
];

/// Folder names a `whole_root` rule may delete entirely.
fn is_cache_like_name(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower.contains("cache")
        || matches!(
            lower.as_str(),
            "crashdumps" | "log" | "logs" | "reportarchive" | "reportqueue" | "reports" | "temp" | "tmp"
        )
}

pub(crate) fn validate_rules(rules: &[RuleSpec]) -> Vec<CatalogIssue> {
    let mut issues = Vec::new();
    let mut ids = HashSet::new();
    for rule in rules {
        let id = rule.id.as_str();
        if id.is_empty()
            || !id.contains('.')
            || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_')
        {
            issues.push(issue(id, "invalidId", "ids look like 'group.camelName'"));
        }
        if !ids.insert(id.to_string()) {
            issues.push(issue(id, "duplicateId", "rule ids must be unique"));
        }
        for key in [&rule.name_key, &rule.detail_key] {
            if !key.starts_with("deepCleaner.") {
                issues.push(issue(id, "invalidLocaleKey", "locale keys live under deepCleaner.*"));
            }
        }
        for process in &rule.processes {
            let lower = process.to_lowercase();
            if !lower.ends_with(".exe") || process.contains('\\') || process.contains('/') {
                issues.push(issue(id, "invalidProcess", "process entries are image names like app.exe"));
            }
        }
        validate_matcher(id, &rule.matcher, true, &mut issues);

        match rule.kind {
            KindSpec::Files => {
                if rule.roots.is_empty() || rule.roots.len() > MAX_ROOTS_PER_RULE {
                    issues.push(issue(id, "rootCount", "file rules need 1..=32 roots"));
                }
            }
            _ => {
                if !rule.roots.is_empty() || !rule.matcher.is_match_all() || rule.mode != CleanMode::Contents {
                    issues.push(issue(id, "specialWithRoots", "special kinds take no roots, matcher, or mode"));
                }
                if let Some(special) = rule.kind.special() {
                    validate_special(rule, special, &mut issues);
                }
            }
        }

        if rule.allow_personal_folder {
            let m = &rule.matcher;
            if m.extensions.is_empty()
                || m.older_than_days.is_none()
                || m.max_depth.is_none()
                || rule.recommended
                || rule.mode != CleanMode::Contents
            {
                issues.push(issue(
                    id,
                    "personalFolderTooBroad",
                    "allow_personal_folder needs extensions + older_than_days + max_depth, contents mode, not recommended",
                ));
            }
        }

        let broad = rule.matcher.is_match_all();
        for root in &rule.roots {
            validate_root_spec(rule, root, broad, &mut issues);
        }
    }
    issues
}

/// Group, elevation, and recommendation of a special rule must match its kind.
fn validate_special(rule: &RuleSpec, special: SpecialKind, issues: &mut Vec<CatalogIssue>) {
    let id = rule.id.as_str();
    if rule.group != special.group() {
        issues.push(issue(id, "specialGroupMismatch", format!("this kind belongs to group {}", special.group().as_str())));
    }
    if rule.requires_elevation != special.requires_elevation() {
        issues.push(issue(id, "elevationMismatch", "requires_elevation must match the kind"));
    }
    if rule.recommended && !special.may_be_recommended() {
        issues.push(issue(id, "mustNotBeRecommended", "this kind is never preselected"));
    }
    if rule.allow_personal_folder {
        issues.push(issue(id, "personalFolderTooBroad", "special kinds take no personal folder access"));
    }
}

fn validate_matcher(id: &str, m: &MatcherSpec, top: bool, issues: &mut Vec<CatalogIssue>) {
    if !top && m.max_depth.is_some() {
        issues.push(issue(id, "nestedMaxDepth", "max_depth is only valid on the top-level matcher"));
    }
    if m.older_than_days == Some(0) {
        issues.push(issue(id, "invalidAge", "older_than_days must be at least 1"));
    }
    let lists = [&m.name_contains, &m.name_glob, &m.extensions, &m.path_segments];
    if lists
        .iter()
        .any(|list| list.iter().any(|v| v.is_empty() || v.contains('/') || v.contains('\\')))
    {
        issues.push(issue(id, "invalidPattern", "patterns are non-empty file/folder names"));
    }
    for nested in m.any_of.iter().chain(m.all_of.iter()) {
        validate_matcher(id, nested, false, issues);
    }
    if let Some(not) = &m.not {
        validate_matcher(id, not, false, issues);
    }
}

fn validate_root_spec(rule: &RuleSpec, root: &RootSpec, broad: bool, issues: &mut Vec<CatalogIssue>) {
    let id = rule.id.as_str();
    check_relative_path(id, &root.path, issues);
    for suffix in &root.suffixes {
        if suffix.is_empty() {
            issues.push(issue(id, "pathEmptySegment", "empty suffix"));
        }
        check_relative_path(id, suffix, issues);
    }
    for env in &root.env {
        if env.var.is_empty() || !env.var.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            issues.push(issue(id, "invalidEnvVar", "env overrides name a variable like CARGO_HOME"));
        }
        check_relative_path(id, &env.path, issues);
        if is_glob(&env.path) {
            issues.push(issue(id, "envPathGlob", "env.path is a literal relative path"));
        }
    }
    let path_segs = segments(&root.path);
    if let Some(children) = &root.children {
        if children.is_empty() {
            issues.push(issue(id, "emptyChildren", "children needs names, prefixes, contains, or all"));
        }
        let names = children.names.iter().chain(&children.prefixes).chain(&children.contains);
        if names.clone().any(|n| n.is_empty() || n.contains('/') || n.contains('\\')) {
            issues.push(issue(id, "invalidPattern", "child selectors are plain folder names"));
        }
        if path_segs.is_empty() && children.all {
            issues.push(issue(id, "broadExpansionUnderBase", "children.all directly under a base folder"));
        }
    }
    if path_segs.first().is_some_and(|s| is_glob(s)) {
        issues.push(issue(id, "broadExpansionUnderBase", "glob directly under a base folder"));
    }
    let bare = path_segs.is_empty() && root.children.is_none() && root.suffixes.is_empty();
    if bare && root.base != BaseVar::Temp {
        issues.push(issue(id, "bareBase", "only the temp base may be a root by itself"));
    }

    // First segment below the base, used for personal / protected checks.
    let first_literal: Vec<String> = if let Some(first) = path_segs.first() {
        vec![first.to_lowercase()]
    } else if let Some(children) = &root.children {
        children.names.iter().map(|n| n.to_lowercase()).collect()
    } else {
        root.suffixes
            .iter()
            .filter_map(|s| s.split('/').next().map(str::to_lowercase))
            .collect()
    };

    match root.base {
        BaseVar::Home => {
            if root.children.as_ref().is_some_and(|c| !c.prefixes.is_empty() || !c.contains.is_empty())
                && path_segs.is_empty()
            {
                issues.push(issue(id, "broadExpansionUnderBase", "prefix/contains children directly under home"));
            }
            for first in &first_literal {
                let personal = PERSONAL_FOLDERS.contains(&first.as_str()) || first.starts_with("onedrive");
                if personal && !(rule.allow_personal_folder && first == "downloads") {
                    issues.push(issue(id, "personalFolder", format!("home/{first} is a personal folder")));
                }
                if PROTECTED_HOME_PREFIXES.contains(&first.as_str()) {
                    issues.push(issue(id, "protectedHomeFolder", format!("home/{first} is protected")));
                }
            }
        }
        BaseVar::Windir | BaseVar::SystemDrive => {
            let allow = if root.base == BaseVar::Windir {
                WINDIR_ALLOWLIST
            } else {
                SYSTEM_DRIVE_ALLOWLIST
            };
            if root.children.is_some()
                || path_segs.iter().any(|s| is_glob(s))
                || root.suffixes.iter().any(|s| is_glob(s))
            {
                issues.push(issue(id, "systemExpansion", "no globs or children under Windows / system drive"));
            }
            let candidates: Vec<String> = if root.suffixes.is_empty() {
                vec![root.path.to_lowercase()]
            } else {
                root.suffixes
                    .iter()
                    .map(|s| format!("{}/{}", root.path, s).trim_start_matches('/').to_lowercase())
                    .collect()
            };
            for candidate in candidates {
                let ok = allow
                    .iter()
                    .any(|a| candidate == *a || candidate.starts_with(&format!("{a}/")));
                if !ok {
                    issues.push(issue(id, "systemPathNotAllowed", format!("{candidate} is not in the allow list")));
                }
            }
        }
        BaseVar::ProgramData => {
            if path_segs.is_empty() {
                issues.push(issue(id, "bareBase", "program_data roots need a path"));
            }
        }
        BaseVar::Local | BaseVar::Roaming | BaseVar::Temp => {}
    }

    if broad {
        let all_segments = path_segs
            .iter()
            .copied()
            .chain(root.suffixes.iter().flat_map(|s| s.split('/')));
        for seg in all_segments {
            let lower = seg.to_lowercase();
            if FORBIDDEN_BROAD_SEGMENTS.contains(&lower.as_str()) {
                issues.push(issue(id, "forbiddenSegment", format!("'{lower}' never appears in a match-all root")));
            }
        }
    }

    if rule.mode == CleanMode::WholeRoot {
        let last = root
            .suffixes
            .first()
            .map(|s| s.rsplit('/').next().unwrap_or(s).to_string())
            .or_else(|| path_segs.last().map(|s| s.to_string()))
            .unwrap_or_default();
        let static_root = root.children.is_none()
            && !path_segs.iter().any(|s| is_glob(s))
            && !root.suffixes.iter().any(|s| is_glob(s));
        let base_ok = matches!(root.base, BaseVar::Local | BaseVar::Roaming | BaseVar::Home);
        let suffixes_ok = root.suffixes.iter().all(|s| is_cache_like_name(s.rsplit('/').next().unwrap_or(s)));
        if !rule.matcher.is_match_all() || !static_root || !base_ok || !is_cache_like_name(&last) || !suffixes_ok {
            issues.push(issue(
                id,
                "wholeRootNotAllowed",
                "whole_root needs a static cache/log/temp root under local, roaming, or home and an empty matcher",
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // `dev.condaPkgs` was retired on purpose: environments hardlink into `pkgs`, so emptying it
    // freed far less than shown; the `conda clean` special cleaner replaces it.
    const PREVIOUS_RULE_IDS: &[&str] = &[
        "system.recycleBin", "system.thumbnailCache", "system.internetTemp", "system.userTemp",
        "system.directxShaderCache", "system.errorReports", "system.deliveryOptimization",
        "system.windowsUpdateCleanup", "system.previousInstallations", "system.stalePartialDownloads",
        "app.whatsappCache", "app.telegramTemp", "app.discordCache", "app.steamCache", "app.teamsCache",
        "app.spotifyCache", "app.slackCache", "app.notionCache", "app.dockerDesktopCache",
        "browser.chromeCache", "browser.chromeOfflineCache", "browser.edgeCache",
        "browser.edgeOfflineCache", "browser.braveCache", "browser.vivaldiCache",
        "browser.chromiumCache", "browser.operaCache", "browser.firefoxCache", "dev.npmCache",
        "dev.pnpmStore", "dev.yarnCache", "dev.pipCache", "dev.uvCache", "dev.cargoCache",
        "dev.goModuleCache", "dev.goBuildCache", "dev.nugetCache", "dev.gradleCaches",
        "dev.mavenRepository", "dev.jetbrainsCaches", "dev.vsCache",
        "dev.nodeTooling", "dev.playwrightCache", "dev.androidCache", "dev.editorCache",
    ];

    fn rule(text: &str) -> Vec<CatalogIssue> {
        let specs = parse_catalog_source(text).expect("parse");
        validate_rules(&specs)
    }

    fn codes(text: &str) -> Vec<&'static str> {
        rule(text).into_iter().map(|i| i.code).collect()
    }

    const HEAD: &str = r#"
[[rule]]
id = "test.rule"
group = "system"
name_key = "deepCleaner.rules.x"
detail_key = "deepCleaner.rules.xDetail"
risk = "safe"
"#;

    /// Contributor gate: every embedded TOML file parses and passes validation.
    #[test]
    fn embedded_catalog_is_valid() {
        match validate_embedded_catalog() {
            Ok(count) => assert!(count >= PREVIOUS_RULE_IDS.len()),
            Err(issues) => panic!("invalid deep cleanup catalog: {issues:#?}"),
        }
    }

    #[test]
    fn embedded_catalog_keeps_every_previous_rule_id_and_locale_key() {
        let rules = catalog().expect("valid catalog");
        for id in PREVIOUS_RULE_IDS {
            let found = rules.iter().find(|r| r.spec.id == *id);
            let found = found.unwrap_or_else(|| panic!("missing rule {id}"));
            let short = id.split('.').nth(1).unwrap();
            assert_eq!(found.spec.name_key, format!("deepCleaner.rules.{short}"));
            assert_eq!(found.spec.detail_key, format!("deepCleaner.rules.{short}Detail"));
        }
    }

    #[test]
    fn locale_keys_exist_in_english_resources() {
        let en: serde_json::Value =
            serde_json::from_str(include_str!("../../../../../src/locales/en.json")).expect("en.json");
        let rules = catalog().expect("valid catalog");
        for rule in rules {
            for key in [&rule.spec.name_key, &rule.spec.detail_key] {
                let mut node = &en;
                for part in key.split('.') {
                    node = node.get(part).unwrap_or_else(|| panic!("missing locale key {key}"));
                }
                assert!(node.is_string(), "{key} is not a string");
            }
        }
    }

    #[test]
    fn previous_rule_semantics_survive_the_move() {
        let rules = catalog().expect("valid catalog");
        let get = |id: &str| rules.iter().find(|r| r.spec.id == id).unwrap();
        let recycle = get("system.recycleBin");
        assert_eq!(recycle.kind, RuleKind::Special(SpecialKind::RecycleBin));
        assert!(!recycle.spec.recommended);
        let thumb = get("system.thumbnailCache");
        assert_eq!(thumb.spec.matcher.max_depth, Some(0));
        assert!(thumb.spec.recommended);
        let stale = get("system.stalePartialDownloads");
        assert!(!stale.spec.recommended);
        assert_eq!(stale.spec.risk, DeepCleanupRisk::Recoverable);
        assert_eq!(stale.spec.matcher.older_than_days, Some(7));
        assert_eq!(stale.spec.matcher.max_depth, Some(3));
        assert!(get("system.windowsUpdateCleanup").spec.requires_elevation);
        assert!(get("system.previousInstallations").spec.requires_elevation);
        assert_eq!(get("dev.editorCache").spec.processes.len(), 4);
    }

    #[test]
    fn rejects_duplicate_ids() {
        let text = format!("{HEAD}roots = [{{ base = \"local\", path = \"A/Cache\" }}]\n{HEAD}roots = [{{ base = \"local\", path = \"B/Cache\" }}]");
        assert!(codes(&text).contains(&"duplicateId"));
    }

    #[test]
    fn rejects_traversal_absolute_and_backslash_paths() {
        for path in ["../Cache", "A/../B", "C:/x", r"A\\B", "/abs", "%TEMP%/x", "A//B"] {
            let text = format!("{HEAD}roots = [{{ base = \"local\", path = \"{path}\" }}]");
            assert!(!codes(&text).is_empty(), "{path} should be rejected");
        }
    }

    #[test]
    fn rejects_forbidden_system_and_home_roots() {
        let cases = [
            ("windir", "Temp", "systemPathNotAllowed"),
            ("windir", "System32", "systemPathNotAllowed"),
            ("system_drive", "Program Files/App", "systemPathNotAllowed"),
            ("home", "Documents/Cache", "personalFolder"),
            ("home", "Downloads", "personalFolder"),
            ("home", "OneDrive - Contoso/cache", "personalFolder"),
            ("home", ".ssh", "protectedHomeFolder"),
            ("home", "source/repos", "protectedHomeFolder"),
            ("local", "Tool/.git", "versionControl"),
            ("local", "Mail/Cache", "forbiddenSegment"),
            ("local", "*", "globTooBroad"),
            ("local", "*cache*", "broadExpansionUnderBase"),
            ("home", "", "bareBase"),
            ("program_data", "", "bareBase"),
        ];
        for (base, path, code) in cases {
            let text = format!("{HEAD}roots = [{{ base = \"{base}\", path = \"{path}\" }}]");
            assert!(codes(&text).contains(&code), "{base}/{path} should report {code}: {:?}", codes(&text));
        }
    }

    #[test]
    fn personal_folder_exception_requires_narrow_matcher() {
        let narrow = format!(
            "{HEAD}allow_personal_folder = true\nroots = [{{ base = \"home\", path = \"Downloads\" }}]\nmatcher = {{ extensions = [\"part\"], older_than_days = 7, max_depth = 3 }}"
        );
        assert!(codes(&narrow).is_empty(), "{:?}", codes(&narrow));
        let broad = format!(
            "{HEAD}allow_personal_folder = true\nroots = [{{ base = \"home\", path = \"Downloads\" }}]"
        );
        assert!(codes(&broad).contains(&"personalFolderTooBroad"));
    }

    #[test]
    fn whole_root_requires_static_cache_root() {
        let ok = format!("{HEAD}mode = \"whole_root\"\nroots = [{{ base = \"local\", path = \"Tool/Cache\" }}]");
        assert!(codes(&ok).is_empty(), "{:?}", codes(&ok));
        let not_cache = format!("{HEAD}mode = \"whole_root\"\nroots = [{{ base = \"local\", path = \"Tool/Data\" }}]");
        assert!(codes(&not_cache).contains(&"wholeRootNotAllowed"));
        let with_matcher = format!(
            "{HEAD}mode = \"whole_root\"\nroots = [{{ base = \"local\", path = \"Tool/Cache\" }}]\nmatcher = {{ extensions = [\"tmp\"] }}"
        );
        assert!(codes(&with_matcher).contains(&"wholeRootNotAllowed"));
    }

    #[test]
    fn rejects_unknown_fields() {
        let text = format!("{HEAD}roots = [{{ base = \"local\", path = \"A/Cache\" }}]\ncolor = \"red\"");
        assert!(parse_catalog_source(&text).is_err());
    }

    #[test]
    fn special_kinds_take_no_roots() {
        let text = format!("{HEAD}kind = \"recycle_bin\"\nroots = [{{ base = \"local\", path = \"A/Cache\" }}]");
        assert!(codes(&text).contains(&"specialWithRoots"));
    }
}
