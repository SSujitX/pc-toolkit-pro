//! File matcher for catalog rules.
//!
//! Flat fields are AND-ed; values inside one list field are OR-ed. `any_of`,
//! `all_of`, and `not` combine nested matchers. An empty matcher matches every
//! regular file. `max_depth` is a traversal limit and is only valid at the top level.

use serde::Deserialize;
use std::time::Duration;

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MatcherSpec {
    /// File name contains any of these (case-insensitive).
    #[serde(default)]
    pub name_contains: Vec<String>,
    /// File name matches any of these globs (`*`, `?`; case-insensitive).
    #[serde(default)]
    pub name_glob: Vec<String>,
    /// Extension (without dot) is any of these (case-insensitive).
    #[serde(default)]
    pub extensions: Vec<String>,
    /// A folder between the rule root and the file is named any of these.
    #[serde(default)]
    pub path_segments: Vec<String>,
    /// Last modified at least this many days ago.
    #[serde(default)]
    pub older_than_days: Option<u64>,
    /// None = unlimited. 0 = files directly in the root only.
    #[serde(default)]
    pub max_depth: Option<u32>,
    #[serde(default)]
    pub any_of: Vec<MatcherSpec>,
    #[serde(default)]
    pub all_of: Vec<MatcherSpec>,
    #[serde(default)]
    pub not: Option<Box<MatcherSpec>>,
}

/// Facts about one file, all lowercase.
pub(crate) struct FileFacts<'a> {
    pub name: &'a str,
    pub extension: &'a str,
    /// Folder names between the root and the file.
    pub rel_dirs: &'a [String],
    /// None when the modified time is unknown or in the future.
    pub age: Option<Duration>,
}

impl MatcherSpec {
    /// True when no filter is set (`max_depth` does not count as a filter).
    pub fn is_match_all(&self) -> bool {
        self.name_contains.is_empty()
            && self.name_glob.is_empty()
            && self.extensions.is_empty()
            && self.path_segments.is_empty()
            && self.older_than_days.is_none()
            && self.any_of.is_empty()
            && self.all_of.is_empty()
            && self.not.is_none()
    }

    /// Lowercase every pattern once at load so matching stays allocation-free.
    pub(crate) fn normalize(&mut self) {
        for list in [
            &mut self.name_contains,
            &mut self.name_glob,
            &mut self.path_segments,
        ] {
            for item in list.iter_mut() {
                *item = item.to_lowercase();
            }
        }
        for ext in self.extensions.iter_mut() {
            *ext = ext.trim_start_matches('.').to_lowercase();
        }
        for nested in self.any_of.iter_mut().chain(self.all_of.iter_mut()) {
            nested.normalize();
        }
        if let Some(not) = self.not.as_mut() {
            not.normalize();
        }
    }

    pub(crate) fn uses_path_segments(&self) -> bool {
        !self.path_segments.is_empty()
            || self.any_of.iter().any(Self::uses_path_segments)
            || self.all_of.iter().any(Self::uses_path_segments)
            || self.not.as_deref().is_some_and(Self::uses_path_segments)
    }

    pub(crate) fn uses_age(&self) -> bool {
        self.older_than_days.is_some()
            || self.any_of.iter().any(Self::uses_age)
            || self.all_of.iter().any(Self::uses_age)
            || self.not.as_deref().is_some_and(Self::uses_age)
    }

    pub(crate) fn matches(&self, facts: &FileFacts<'_>) -> bool {
        if !self.name_contains.is_empty()
            && !self.name_contains.iter().any(|n| facts.name.contains(n.as_str()))
        {
            return false;
        }
        if !self.name_glob.is_empty() && !self.name_glob.iter().any(|g| glob_match(g, facts.name)) {
            return false;
        }
        if !self.extensions.is_empty() && !self.extensions.iter().any(|e| e == facts.extension) {
            return false;
        }
        if !self.path_segments.is_empty()
            && !facts
                .rel_dirs
                .iter()
                .any(|d| self.path_segments.iter().any(|s| s == d))
        {
            return false;
        }
        if let Some(days) = self.older_than_days {
            let threshold = Duration::from_secs(days.saturating_mul(24 * 60 * 60));
            match facts.age {
                Some(age) if age >= threshold => {}
                _ => return false,
            }
        }
        if !self.any_of.is_empty() && !self.any_of.iter().any(|m| m.matches(facts)) {
            return false;
        }
        if !self.all_of.iter().all(|m| m.matches(facts)) {
            return false;
        }
        if let Some(not) = &self.not {
            if not.matches(facts) {
                return false;
            }
        }
        true
    }
}

/// Case-sensitive glob over chars with `*` (any run) and `?` (one char). Callers
/// lowercase both sides for case-insensitive matching.
pub(crate) fn glob_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    let (mut pi, mut ti) = (0usize, 0usize);
    let mut star: Option<usize> = None;
    let mut mark = 0usize;
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ti;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts<'a>(name: &'a str, ext: &'a str, dirs: &'a [String], days: Option<u64>) -> FileFacts<'a> {
        FileFacts {
            name,
            extension: ext,
            rel_dirs: dirs,
            age: days.map(|d| Duration::from_secs(d * 86_400)),
        }
    }

    #[test]
    fn glob_handles_star_and_question() {
        assert!(glob_match("*.log", "app.log"));
        assert!(glob_match("thumbcache_*.db", "thumbcache_256.db"));
        assert!(glob_match("f?o", "foo"));
        assert!(!glob_match("f?o", "fooo"));
        assert!(glob_match("profile *", "profile 12"));
        assert!(!glob_match("*.log", "app.log.bak"));
        assert!(glob_match("*", ""));
    }

    #[test]
    fn empty_matcher_matches_everything() {
        let m = MatcherSpec::default();
        assert!(m.is_match_all());
        assert!(m.matches(&facts("x.bin", "bin", &[], None)));
    }

    #[test]
    fn flat_fields_are_and_lists_are_or() {
        let mut m = MatcherSpec {
            extensions: vec!["CRDOWNLOAD".into(), ".part".into()],
            older_than_days: Some(7),
            ..MatcherSpec::default()
        };
        m.normalize();
        assert!(m.matches(&facts("a.crdownload", "crdownload", &[], Some(8))));
        assert!(m.matches(&facts("a.part", "part", &[], Some(30))));
        assert!(!m.matches(&facts("a.part", "part", &[], Some(1))));
        assert!(!m.matches(&facts("a.part", "part", &[], None)));
        assert!(!m.matches(&facts("a.zip", "zip", &[], Some(30))));
    }

    #[test]
    fn combinators_and_path_segments() {
        let mut m = MatcherSpec {
            path_segments: vec!["Cache".into()],
            any_of: vec![
                MatcherSpec { extensions: vec!["tmp".into()], ..MatcherSpec::default() },
                MatcherSpec { name_glob: vec!["f_*".into()], ..MatcherSpec::default() },
            ],
            not: Some(Box::new(MatcherSpec {
                name_contains: vec!["keep".into()],
                ..MatcherSpec::default()
            })),
            ..MatcherSpec::default()
        };
        m.normalize();
        assert!(m.uses_path_segments());
        let in_cache = vec!["cache".to_string()];
        let elsewhere = vec!["data".to_string()];
        assert!(m.matches(&facts("f_0001", "", &in_cache, None)));
        assert!(m.matches(&facts("x.tmp", "tmp", &in_cache, None)));
        assert!(!m.matches(&facts("keep.tmp", "tmp", &in_cache, None)));
        assert!(!m.matches(&facts("x.tmp", "tmp", &elsewhere, None)));
        assert!(!m.matches(&facts("x.bin", "bin", &in_cache, None)));
    }
}
