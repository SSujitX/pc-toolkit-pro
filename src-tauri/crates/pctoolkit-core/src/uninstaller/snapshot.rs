//! Inventory snapshot used to notice programs removed outside PC Toolkit Pro (Windows Settings,
//! the program's own uninstaller, another tool) so their leftovers can still be scanned.
//!
//! Persistence: `%LOCALAPPDATA%\PC Toolkit Pro\uninstaller-inventory.json`.
//! Policy: read → on any parse error start over with an empty snapshot (it is a cache, not user
//! data); the first run records a baseline and reports nothing.

use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

const SNAPSHOT_VERSION: u32 = 1;
/// Keep the "removed elsewhere" list short; older entries are dropped first.
const MAX_PENDING: usize = 30;

/// What is needed to scan a program's leftovers after it is gone.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotProgram {
    pub id: String,
    pub name: String,
    pub publisher: String,
    pub version: String,
    pub key_name: String,
    pub install_location: String,
    /// Removal time (Unix ms) for pending entries; 0 in the installed list.
    #[serde(default)]
    pub removed_at_ms: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SnapshotFile {
    version: u32,
    installed: Vec<SnapshotProgram>,
    removed_elsewhere: Vec<SnapshotProgram>,
}

fn snapshot_path() -> Option<PathBuf> {
    let base = std::env::var_os("LOCALAPPDATA")?;
    Some(
        PathBuf::from(base)
            .join("PC Toolkit Pro")
            .join("uninstaller-inventory.json"),
    )
}

fn read() -> Option<SnapshotFile> {
    let raw = fs::read_to_string(snapshot_path()?).ok()?;
    serde_json::from_str::<SnapshotFile>(&raw)
        .ok()
        .filter(|f| f.version == SNAPSHOT_VERSION)
}

fn write(file: &SnapshotFile) {
    let Some(path) = snapshot_path() else { return };
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(raw) = serde_json::to_string_pretty(file) {
        let _ = fs::write(path, raw);
    }
}

/// Pure diff: programs in `previous` that are no longer in `current`, merged with the earlier
/// pending list (minus anything that is installed again), newest first, capped.
pub(crate) fn merge_removed(
    previous: &[SnapshotProgram],
    pending: &[SnapshotProgram],
    current: &[SnapshotProgram],
    now_ms: i64,
) -> Vec<SnapshotProgram> {
    let installed: HashSet<&str> = current.iter().map(|p| p.id.as_str()).collect();
    let mut out: Vec<SnapshotProgram> = previous
        .iter()
        .filter(|p| !installed.contains(p.id.as_str()))
        .map(|p| SnapshotProgram {
            removed_at_ms: now_ms,
            ..p.clone()
        })
        .collect();
    let mut seen: HashSet<String> = out.iter().map(|p| p.id.clone()).collect();
    for p in pending {
        if !installed.contains(p.id.as_str()) && seen.insert(p.id.clone()) {
            out.push(p.clone());
        }
    }
    out.truncate(MAX_PENDING);
    out
}

/// Record the current desktop inventory and return the programs removed outside the app.
pub(crate) fn update(current: Vec<SnapshotProgram>, now_ms: i64) -> Vec<SnapshotProgram> {
    let previous = read();
    let removed = match &previous {
        Some(file) => merge_removed(&file.installed, &file.removed_elsewhere, &current, now_ms),
        // First run: baseline only.
        None => Vec::new(),
    };
    write(&SnapshotFile {
        version: SNAPSHOT_VERSION,
        installed: current,
        removed_elsewhere: removed.clone(),
    });
    removed
}

/// Forget a program everywhere in the snapshot: it was removed by PC Toolkit Pro itself, its
/// leftovers were handled, or the user dismissed it.
pub(crate) fn forget(id: &str) {
    let Some(mut file) = read() else { return };
    file.installed.retain(|p| p.id != id);
    file.removed_elsewhere.retain(|p| p.id != id);
    write(&file);
}

pub(crate) fn find_removed(id: &str) -> Option<SnapshotProgram> {
    read()?.removed_elsewhere.into_iter().find(|p| p.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn program(id: &str) -> SnapshotProgram {
        SnapshotProgram {
            id: id.into(),
            name: id.into(),
            publisher: String::new(),
            version: String::new(),
            key_name: id.into(),
            install_location: String::new(),
            removed_at_ms: 0,
        }
    }

    #[test]
    fn reports_programs_that_disappeared_and_keeps_pending() {
        let previous = vec![program("a"), program("b"), program("c")];
        let pending = vec![SnapshotProgram { removed_at_ms: 5, ..program("old") }];
        let current = vec![program("a")];
        let removed = merge_removed(&previous, &pending, &current, 100);
        let ids: Vec<&str> = removed.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, vec!["b", "c", "old"]);
        assert_eq!(removed[0].removed_at_ms, 100);
        assert_eq!(removed[2].removed_at_ms, 5);
    }

    #[test]
    fn reinstalled_programs_leave_the_pending_list() {
        let pending = vec![program("b")];
        let current = vec![program("b")];
        assert!(merge_removed(&[], &pending, &current, 1).is_empty());
    }

    #[test]
    fn pending_list_is_capped() {
        let previous: Vec<_> = (0..50).map(|i| program(&format!("p{i}"))).collect();
        assert_eq!(merge_removed(&previous, &[], &[], 1).len(), MAX_PENDING);
    }
}
