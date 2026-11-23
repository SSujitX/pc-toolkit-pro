//! Care undo journal: `%LOCALAPPDATA%\PC Toolkit Pro\care-undo.json`.
//!
//! Every reversible Care fix (registry setting, deleted registry key / value, firewall profile)
//! appends a record **before** the change is applied, so a crash mid-fix still leaves a way back.
//! When the change then fails, the record is discarded again.
//!
//! Persistence policy: versioned JSON `{ "version": 1, "records": [...] }`.
//! * missing file → empty journal;
//! * unreadable / unparsable / newer version → the file is moved aside to
//!   `care-undo.json.corrupt` (never silently overwritten) and an empty journal is used;
//! * at most [`MAX_RECORDS`] records are kept (oldest dropped first).
//!
//! Records hold registry locations because restoring needs them; they are local state, never
//! written to logs or history.
//!
//! Public API (the engine owner exposes it as commands):
//! * [`list_care_undo_records`] — newest first;
//! * [`undo_care_fixes`] — restore by record id; each restored record is removed. A record whose
//!   target changed again since the fix is skipped (`itemChanged`) instead of clobbering it.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use pctoolkit_platform::registry_settings::{import_registry_file, read_registry_data, write_registry_data, RegistryData};
use pctoolkit_platform::security_center::{firewall_profiles, set_firewall_profile_enabled, FirewallProfileKind};
use pctoolkit_platform::{delete_registry_value, registry_key_exists, PlatformError, RegistryKeyPath};

use super::CareCategoryId;
use crate::shared::{CoreError, CoreResult};

pub const UNDO_FILE_NAME: &str = "care-undo.json";
const CURRENT_VERSION: u32 = 1;
pub const MAX_RECORDS: usize = 1000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum CareUndoChange {
    /// One registry value was written or deleted. `previous: None` = the value did not exist;
    /// `applied: None` = the fix deleted it.
    RegistryValue {
        key: RegistryKeyPath,
        name: String,
        previous: Option<RegistryData>,
        applied: Option<RegistryData>,
    },
    /// A whole key was deleted after a `reg export` backup. `backupFile` is relative to
    /// `%LOCALAPPDATA%\PC Toolkit Pro\care-backups\`.
    RegistryKeyDeleted { key: RegistryKeyPath, backup_file: String },
    /// A Windows Firewall profile was switched.
    FirewallProfile { profile: FirewallProfileKind, previous_enabled: bool, applied_enabled: bool },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CareUndoRecord {
    pub id: String,
    pub category: CareCategoryId,
    pub item_id: String,
    /// Unix milliseconds.
    pub created_at_ms: i64,
    /// Restoring needs an elevated process.
    pub needs_admin: bool,
    pub change: CareUndoChange,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CareUndoStatus {
    Restored,
    Skipped,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CareUndoOutcome {
    pub record_id: String,
    pub status: CareUndoStatus,
    /// Typed reason: notFound | needsAdmin | itemChanged | backupMissing | permissionDenied | failed
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UndoStore {
    version: u32,
    records: Vec<CareUndoRecord>,
}

static JOURNAL_LOCK: Mutex<()> = Mutex::new(());

fn app_data_dir() -> Option<PathBuf> {
    let base = std::env::var_os("LOCALAPPDATA")?;
    Some(PathBuf::from(base).join("PC Toolkit Pro"))
}

fn journal_path() -> CoreResult<PathBuf> {
    app_data_dir()
        .map(|d| d.join(UNDO_FILE_NAME))
        .ok_or_else(|| CoreError::OperationFailed("appDataUnavailable".into()))
}

/// `%LOCALAPPDATA%\PC Toolkit Pro\care-backups\` (registry `.reg` exports made by Care fixes).
pub fn care_backups_dir() -> Option<PathBuf> {
    app_data_dir().map(|d| d.join("care-backups"))
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// Parse a journal; `None` when the text is not a journal this version understands.
fn parse_store(text: &str) -> Option<UndoStore> {
    let store: UndoStore = serde_json::from_str(text).ok()?;
    (store.version == CURRENT_VERSION).then_some(store)
}

/// Load the journal. Only a missing file means "no records yet"; any other read failure
/// (sharing violation, access denied) is an error so callers never overwrite a journal they
/// could not read, and a fix without a recorded undo is not applied.
fn load_store(path: &Path) -> CoreResult<UndoStore> {
    let empty = UndoStore { version: CURRENT_VERSION, records: Vec::new() };
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(empty),
        Err(e) => return Err(CoreError::OperationFailed(format!("undoRead:{}", e.kind()))),
    };
    match parse_store(&text) {
        Some(store) => Ok(store),
        None => {
            // Keep the unreadable journal for manual recovery instead of overwriting it.
            let _ = fs::rename(path, path.with_extension("json.corrupt"));
            Ok(empty)
        }
    }
}

fn save_store(path: &Path, store: &UndoStore) -> CoreResult<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| CoreError::OperationFailed(format!("undoDir:{}", e.kind())))?;
    }
    let json = serde_json::to_string_pretty(store).map_err(|_| CoreError::OperationFailed("undoEncode".into()))?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, json).map_err(|e| CoreError::OperationFailed(format!("undoWrite:{}", e.kind())))?;
    fs::rename(&tmp, path).map_err(|e| CoreError::OperationFailed(format!("undoRename:{}", e.kind())))
}

fn append_at(path: &Path, record: CareUndoRecord) -> CoreResult<()> {
    let mut store = load_store(path)?;
    store.records.push(record);
    if store.records.len() > MAX_RECORDS {
        let excess = store.records.len() - MAX_RECORDS;
        store.records.drain(..excess);
    }
    save_store(path, &store)
}

fn remove_at(path: &Path, ids: &[String]) -> CoreResult<()> {
    let mut store = load_store(path)?;
    let before = store.records.len();
    store.records.retain(|r| !ids.contains(&r.id));
    if store.records.len() != before {
        save_store(path, &store)?;
    }
    Ok(())
}

/// Store an undo record before applying a change. Returns the record id.
pub(crate) fn record_change(
    category: CareCategoryId,
    item_id: &str,
    needs_admin: bool,
    change: CareUndoChange,
) -> CoreResult<String> {
    let record = CareUndoRecord {
        id: uuid::Uuid::new_v4().to_string(),
        category,
        item_id: item_id.to_string(),
        created_at_ms: now_ms(),
        needs_admin,
        change,
    };
    let id = record.id.clone();
    let _guard = JOURNAL_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    append_at(&journal_path()?, record)?;
    Ok(id)
}

/// Drop a record whose change was not applied after all.
pub(crate) fn discard_record(record_id: &str) {
    let _guard = JOURNAL_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    if let Ok(path) = journal_path() {
        let _ = remove_at(&path, &[record_id.to_string()]);
    }
}

/// All undo records, newest first.
pub fn list_care_undo_records() -> CoreResult<Vec<CareUndoRecord>> {
    let _guard = JOURNAL_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut records = load_store(&journal_path()?)?.records;
    records.reverse();
    Ok(records)
}

fn platform_reason(error: &PlatformError) -> String {
    match error {
        PlatformError::PermissionDenied => "permissionDenied".into(),
        PlatformError::Unsupported => "unsupported".into(),
        PlatformError::OperationFailed(_) => "failed".into(),
    }
}

fn restore(record: &CareUndoRecord) -> (CareUndoStatus, Option<String>) {
    let fail = |e: PlatformError| (CareUndoStatus::Failed, Some(platform_reason(&e)));
    match &record.change {
        CareUndoChange::RegistryValue { key, name, previous, applied } => {
            let current = match read_registry_data(key, name) {
                Ok(v) => v,
                Err(e) => return fail(e),
            };
            if &current != applied {
                return (CareUndoStatus::Skipped, Some("itemChanged".into()));
            }
            let result = match previous {
                Some(value) => write_registry_data(key, name, value),
                None => delete_registry_value(key, name),
            };
            match result {
                Ok(()) => (CareUndoStatus::Restored, None),
                Err(e) => fail(e),
            }
        }
        CareUndoChange::RegistryKeyDeleted { key, backup_file } => {
            if registry_key_exists(key) {
                return (CareUndoStatus::Skipped, Some("itemChanged".into()));
            }
            let Some(file) = care_backups_dir().map(|d| d.join(backup_file)) else {
                return (CareUndoStatus::Failed, Some("backupMissing".into()));
            };
            if !file.is_file() {
                return (CareUndoStatus::Failed, Some("backupMissing".into()));
            }
            match import_registry_file(&file, key.view) {
                Ok(()) if registry_key_exists(key) => (CareUndoStatus::Restored, None),
                Ok(()) => (CareUndoStatus::Failed, Some("failed".into())),
                Err(e) => fail(e),
            }
        }
        CareUndoChange::FirewallProfile { profile, previous_enabled, applied_enabled } => {
            let current = match firewall_profiles() {
                Ok(list) => list.into_iter().find(|p| p.profile == *profile).map(|p| p.enabled),
                Err(e) => return fail(e),
            };
            if current != Some(*applied_enabled) {
                return (CareUndoStatus::Skipped, Some("itemChanged".into()));
            }
            match set_firewall_profile_enabled(*profile, *previous_enabled) {
                Ok(()) => (CareUndoStatus::Restored, None),
                Err(e) => fail(e),
            }
        }
    }
}

/// Undo the given fixes. Restored records are removed from the journal; skipped / failed ones stay
/// so the user can retry (for example after restarting as administrator).
pub fn undo_care_fixes(record_ids: &[String], is_admin: bool) -> CoreResult<Vec<CareUndoOutcome>> {
    let _guard = JOURNAL_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let path = journal_path()?;
    let store = load_store(&path)?;
    let mut restored = Vec::new();
    let mut outcomes = Vec::new();
    for id in record_ids {
        let Some(record) = store.records.iter().find(|r| &r.id == id) else {
            outcomes.push(CareUndoOutcome {
                record_id: id.clone(),
                status: CareUndoStatus::Skipped,
                reason: Some("notFound".into()),
            });
            continue;
        };
        let (status, reason) = if record.needs_admin && !is_admin {
            (CareUndoStatus::Skipped, Some("needsAdmin".into()))
        } else {
            restore(record)
        };
        if status == CareUndoStatus::Restored {
            restored.push(id.clone());
        }
        outcomes.push(CareUndoOutcome { record_id: id.clone(), status, reason });
    }
    if !restored.is_empty() {
        remove_at(&path, &restored)?;
    }
    Ok(outcomes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pctoolkit_platform::{RegistryRoot, RegistryView};

    fn temp_journal() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("pctk-care-undo-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        dir.join(UNDO_FILE_NAME)
    }

    fn sample(id: &str) -> CareUndoRecord {
        CareUndoRecord {
            id: id.into(),
            category: CareCategoryId::Security,
            item_id: "showFileExtensions".into(),
            created_at_ms: 1,
            needs_admin: false,
            change: CareUndoChange::RegistryValue {
                key: RegistryKeyPath::new(RegistryRoot::CurrentUser, RegistryView::Native64, "Software\\Test"),
                name: "HideFileExt".into(),
                previous: None,
                applied: Some(RegistryData::Dword(0)),
            },
        }
    }

    #[test]
    fn records_round_trip_through_the_journal() {
        let path = temp_journal();
        append_at(&path, sample("a")).unwrap();
        let mut b = sample("b");
        b.change = CareUndoChange::FirewallProfile {
            profile: FirewallProfileKind::Public,
            previous_enabled: false,
            applied_enabled: true,
        };
        append_at(&path, b.clone()).unwrap();
        let mut c = sample("c");
        c.change = CareUndoChange::RegistryKeyDeleted {
            key: RegistryKeyPath::new(RegistryRoot::LocalMachine, RegistryView::Wow32, "SOFTWARE\\X"),
            backup_file: "20260101-000000\\registry-1.reg".into(),
        };
        append_at(&path, c.clone()).unwrap();
        let store = load_store(&path).unwrap();
        assert_eq!(store.records, vec![sample("a"), b, c]);
        remove_at(&path, &["a".into()]).unwrap();
        assert_eq!(load_store(&path).unwrap().records.len(), 2);
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn journal_is_capped_oldest_first() {
        let path = temp_journal();
        let mut store = UndoStore { version: CURRENT_VERSION, records: Vec::new() };
        for i in 0..MAX_RECORDS {
            store.records.push(sample(&i.to_string()));
        }
        save_store(&path, &store).unwrap();
        append_at(&path, sample("newest")).unwrap();
        let records = load_store(&path).unwrap().records;
        assert_eq!(records.len(), MAX_RECORDS);
        assert_eq!(records.first().unwrap().id, "1");
        assert_eq!(records.last().unwrap().id, "newest");
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn unreadable_journal_fails_the_record_step_instead_of_resetting() {
        let path = temp_journal();
        // A directory at the journal path cannot be read as text and is not "not found".
        fs::create_dir_all(&path).unwrap();
        assert!(load_store(&path).is_err());
        assert!(append_at(&path, sample("a")).is_err());
        assert!(path.is_dir(), "the unreadable journal must not be replaced");
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn missing_journal_is_empty() {
        let path = temp_journal();
        assert!(load_store(&path).unwrap().records.is_empty());
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn corrupt_journal_is_moved_aside_not_overwritten() {
        let path = temp_journal();
        fs::write(&path, "{ not json").unwrap();
        assert!(load_store(&path).unwrap().records.is_empty());
        assert!(path.with_extension("json.corrupt").is_file());
        assert!(parse_store(r#"{"version":99,"records":[]}"#).is_none());
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }
}
