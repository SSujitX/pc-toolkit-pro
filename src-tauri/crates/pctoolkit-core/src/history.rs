use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

const HISTORY_CAP: usize = 100;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum HistoryOutcome {
    Completed,
    CompletedWithWarnings,
    Cancelled,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryRecord {
    pub id: String,
    /// Product entry: cleaner | deepCleaner | power
    pub category: String,
    /// i18n key for the primary title, e.g. history.titles.cleaner
    pub title_key: String,
    pub summary: String,
    pub started_at_ms: i64,
    pub finished_at_ms: i64,
    pub outcome: HistoryOutcome,
    pub planned_bytes: Option<u64>,
    pub result_bytes: Option<u64>,
    pub selected_item_count: u32,
    pub affected_item_count: u32,
    pub failed_item_count: u32,
    pub detail_lines: Vec<String>,
    /// Legacy-friendly fields kept for older callers / smoke tests.
    pub domain: String,
    pub action: String,
    pub success: bool,
    pub detail: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct HistoryWrite {
    pub category: String,
    pub title_key: String,
    pub summary: String,
    pub started_at_ms: i64,
    pub finished_at_ms: i64,
    pub outcome: HistoryOutcome,
    pub planned_bytes: Option<u64>,
    pub result_bytes: Option<u64>,
    pub selected_item_count: u32,
    pub affected_item_count: u32,
    pub failed_item_count: u32,
    pub detail_lines: Vec<String>,
    pub action: String,
    pub detail: Option<String>,
}

static HISTORY: Mutex<Vec<HistoryRecord>> = Mutex::new(Vec::new());
static LOADED: Mutex<bool> = Mutex::new(false);

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn history_path() -> Option<PathBuf> {
    let base = std::env::var_os("LOCALAPPDATA").or_else(|| std::env::var_os("APPDATA"))?;
    let mut path = PathBuf::from(base);
    path.push("PC Toolkit Pro");
    path.push("history.json");
    Some(path)
}

fn ensure_loaded(guard: &mut Vec<HistoryRecord>) {
    let mut loaded = LOADED.lock().unwrap_or_else(|e| e.into_inner());
    if *loaded {
        return;
    }
    *loaded = true;
    let Some(path) = history_path() else {
        return;
    };
    let Ok(raw) = fs::read_to_string(path) else {
        return;
    };
    if let Ok(items) = serde_json::from_str::<Vec<HistoryRecord>>(&raw) {
        *guard = items;
        guard.truncate(HISTORY_CAP);
    }
}

fn persist(guard: &[HistoryRecord]) {
    let Some(path) = history_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(raw) = serde_json::to_string_pretty(guard) {
        let _ = fs::write(path, raw);
    }
}

/// Rich append used by product domains.
pub fn append_history(write: HistoryWrite) {
    let mut guard = HISTORY.lock().unwrap_or_else(|e| e.into_inner());
    ensure_loaded(&mut guard);
    let success = matches!(
        write.outcome,
        HistoryOutcome::Completed | HistoryOutcome::CompletedWithWarnings
    );
    guard.insert(
        0,
        HistoryRecord {
            id: uuid::Uuid::new_v4().to_string(),
            category: write.category.clone(),
            title_key: write.title_key,
            summary: write.summary,
            started_at_ms: write.started_at_ms,
            finished_at_ms: write.finished_at_ms,
            outcome: write.outcome,
            planned_bytes: write.planned_bytes,
            result_bytes: write.result_bytes,
            selected_item_count: write.selected_item_count,
            affected_item_count: write.affected_item_count,
            failed_item_count: write.failed_item_count,
            detail_lines: write.detail_lines,
            domain: write.category,
            action: write.action,
            success,
            detail: write.detail,
            created_at: Utc::now(),
        },
    );
    guard.truncate(HISTORY_CAP);
    persist(&guard);
}

/// Backward-compatible helper for simple callers / tests.
pub fn record_history(domain: &str, action: String, success: bool, detail: Option<String>) {
    let ts = now_ms();
    append_history(HistoryWrite {
        category: domain.to_string(),
        title_key: format!("history.titles.{domain}"),
        summary: detail.clone().unwrap_or_else(|| action.clone()),
        started_at_ms: ts,
        finished_at_ms: ts,
        outcome: if success {
            HistoryOutcome::Completed
        } else {
            HistoryOutcome::Failed
        },
        planned_bytes: None,
        result_bytes: None,
        selected_item_count: 0,
        affected_item_count: 0,
        failed_item_count: if success { 0 } else { 1 },
        detail_lines: detail.clone().into_iter().collect(),
        action,
        detail,
    });
}

pub fn list_history() -> Vec<HistoryRecord> {
    let mut guard = HISTORY.lock().unwrap_or_else(|e| e.into_inner());
    ensure_loaded(&mut guard);
    guard.clone()
}

pub fn clear_history() {
    let mut guard = HISTORY.lock().unwrap_or_else(|e| e.into_inner());
    ensure_loaded(&mut guard);
    guard.clear();
    persist(&guard);
}

pub fn history_now_ms() -> i64 {
    now_ms()
}
