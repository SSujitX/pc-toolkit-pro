use serde::Serialize;
use tauri::{AppHandle, Emitter};

pub const CLEANUP_PROGRESS: &str = "cleanup-progress";

pub fn emit<T: Serialize + Clone>(app: &AppHandle, event: &str, payload: T) {
    if let Err(error) = app.emit(event, payload) {
        log::warn!("event_emit_failed event={event} error={error}");
    }
}
