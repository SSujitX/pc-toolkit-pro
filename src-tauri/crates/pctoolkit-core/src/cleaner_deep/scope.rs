//! Deep Cleanup scan scope: extra drives searched for development projects.
//!
//! Cache rules always look at their fixed Windows / app locations, so the scope only widens the
//! Projects provider. The system drive is always searched through the user's home folders and the
//! usual developer roots; each extra drive chosen here is searched from its top-level folders
//! (system and game folders excluded). The choice lives in
//! `%LOCALAPPDATA%\PC Toolkit Pro\deep-cleanup-scope.json` (unreadable → no extra drives).

use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use pctoolkit_platform::disk_health::list_fixed_volumes;
use serde::{Deserialize, Serialize};

use crate::shared::{CoreError, CoreResult};

/// One fixed drive offered in the scope picker.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeepCleanupDrive {
    /// Upper-case drive letter ("D").
    pub letter: String,
    pub label: Option<String>,
    pub total_bytes: u64,
    pub free_bytes: u64,
    /// The Windows drive; always searched, cannot be toggled.
    pub system: bool,
    /// Searched for projects in the next scan.
    pub selected: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct ScopeFile {
    /// Extra drive letters (never the system drive).
    #[serde(default)]
    project_drives: Vec<String>,
}

static SCOPE: Mutex<Option<ScopeFile>> = Mutex::new(None);

fn scope_path() -> Option<PathBuf> {
    let base = std::env::var_os("LOCALAPPDATA").or_else(|| std::env::var_os("APPDATA"))?;
    Some(PathBuf::from(base).join("PC Toolkit Pro").join("deep-cleanup-scope.json"))
}

fn system_letter() -> Option<char> {
    std::env::var("SystemDrive")
        .ok()
        .and_then(|d| d.chars().next())
        .map(|c| c.to_ascii_uppercase())
}

/// Normalize user input ("d", "D:", "D:\\") to an upper-case letter.
pub(crate) fn normalize_letter(raw: &str) -> Option<char> {
    let trimmed = raw.trim().trim_end_matches(['\\', '/']).trim_end_matches(':');
    let mut chars = trimmed.chars();
    let letter = chars.next()?.to_ascii_uppercase();
    (letter.is_ascii_alphabetic() && chars.next().is_none()).then_some(letter)
}

/// Keep valid, distinct, non-system letters in drive order.
pub(crate) fn clean_letters(raw: &[String], available: &[char], system: Option<char>) -> Vec<char> {
    let mut out: Vec<char> = raw
        .iter()
        .filter_map(|r| normalize_letter(r))
        .filter(|l| Some(*l) != system && available.contains(l))
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

fn load() -> ScopeFile {
    let mut cached = SCOPE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(scope) = cached.as_ref() {
        return scope.clone();
    }
    let scope = scope_path()
        .and_then(|p| fs::read_to_string(p).ok())
        .and_then(|raw| serde_json::from_str::<ScopeFile>(&raw).ok())
        .unwrap_or_default();
    *cached = Some(scope.clone());
    scope
}

/// Extra drives to search for projects that are still present (`D:\`, …).
pub(crate) fn selected_project_drive_roots() -> Vec<PathBuf> {
    let available: Vec<char> = list_fixed_volumes().iter().map(|v| v.letter).collect();
    clean_letters(&load().project_drives, &available, system_letter())
        .into_iter()
        .map(|l| PathBuf::from(format!("{l}:\\")))
        .collect()
}

/// Fixed drives with their selection state.
pub fn list_deep_cleanup_drives() -> Vec<DeepCleanupDrive> {
    let volumes = list_fixed_volumes();
    let available: Vec<char> = volumes.iter().map(|v| v.letter).collect();
    let system = system_letter();
    let selected = clean_letters(&load().project_drives, &available, system);
    volumes
        .into_iter()
        .map(|v| DeepCleanupDrive {
            letter: v.letter.to_string(),
            label: v.label,
            total_bytes: v.total_bytes,
            free_bytes: v.free_bytes,
            system: Some(v.letter) == system,
            selected: Some(v.letter) == system || selected.contains(&v.letter),
        })
        .collect()
}

/// Save the extra drives to search. Unknown letters are rejected; the system drive is ignored
/// (it is always searched).
pub fn set_deep_cleanup_project_drives(letters: Vec<String>) -> CoreResult<Vec<DeepCleanupDrive>> {
    if letters.len() > 26 {
        return Err(CoreError::InvalidInput("tooManyDrives".into()));
    }
    let available: Vec<char> = list_fixed_volumes().iter().map(|v| v.letter).collect();
    if letters
        .iter()
        .any(|raw| normalize_letter(raw).map_or(true, |l| !available.contains(&l)))
    {
        return Err(CoreError::InvalidInput("unknownDrive".into()));
    }
    let scope = ScopeFile {
        project_drives: clean_letters(&letters, &available, system_letter())
            .into_iter()
            .map(String::from)
            .collect(),
    };
    if let Some(path) = scope_path() {
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(raw) = serde_json::to_string_pretty(&scope) {
            let _ = fs::write(path, raw);
        }
    }
    *SCOPE.lock().unwrap_or_else(|e| e.into_inner()) = Some(scope);
    Ok(list_deep_cleanup_drives())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letters_are_normalized_and_filtered() {
        assert_eq!(normalize_letter("d"), Some('D'));
        assert_eq!(normalize_letter("D:"), Some('D'));
        assert_eq!(normalize_letter("D:\\"), Some('D'));
        assert_eq!(normalize_letter("DD"), None);
        assert_eq!(normalize_letter("1"), None);
        assert_eq!(normalize_letter(""), None);
        let raw = vec!["e".to_string(), "C:".into(), "D".into(), "d:\\".into(), "Z".into()];
        assert_eq!(clean_letters(&raw, &['C', 'D', 'E'], Some('C')), vec!['D', 'E']);
    }
}
