use chrono::{DateTime, Utc};
use serde::Serialize;
use std::sync::Mutex;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryRecord {
    pub id: String,
    pub domain: String,
    pub action: String,
    pub success: bool,
    pub detail: Option<String>,
    pub created_at: DateTime<Utc>,
}

static HISTORY: Mutex<Vec<HistoryRecord>> = Mutex::new(Vec::new());

pub fn record_history(domain: &str, action: String, success: bool, detail: Option<String>) {
    let mut guard = HISTORY.lock().unwrap_or_else(|e| e.into_inner());
    guard.insert(
        0,
        HistoryRecord {
            id: uuid::Uuid::new_v4().to_string(),
            domain: domain.to_string(),
            action,
            success,
            detail,
            created_at: Utc::now(),
        },
    );
    guard.truncate(200);
}

pub fn list_history() -> Vec<HistoryRecord> {
    HISTORY
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}
