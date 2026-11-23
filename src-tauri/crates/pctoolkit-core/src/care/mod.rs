//! Care: one scan over independent PC-health categories (privacy, junk, shortcuts, startup,
//! security, …), each implemented by its own category module, plus a fix pipeline.
//!
//! The engine (scan / fix orchestration, progress, skip / stop) lives in `engine.rs`.
//! Categories implement [`CareCategory`] in `categories/<name>.rs` and must be honest:
//! read-only scans, typed states, no silent elevation, reversible fixes where possible.

pub mod categories;
pub mod engine;
pub mod security_checks;
pub mod undo;

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::shared::CoreResult;

/// Stable category ids (also the UI / locale keys `care.categories.<id>`), in display order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CareCategoryId {
    Privacy,
    Junk,
    Shortcuts,
    Registry,
    Startup,
    Security,
    Protection,
    WindowsUpdate,
    Software,
    DiskHealth,
    DiskOptimize,
}

impl CareCategoryId {
    pub const ALL: [CareCategoryId; 11] = [
        Self::Privacy,
        Self::Junk,
        Self::Shortcuts,
        Self::Registry,
        Self::Startup,
        Self::Security,
        Self::Protection,
        Self::WindowsUpdate,
        Self::Software,
        Self::DiskHealth,
        Self::DiskOptimize,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Privacy => "privacy",
            Self::Junk => "junk",
            Self::Shortcuts => "shortcuts",
            Self::Registry => "registry",
            Self::Startup => "startup",
            Self::Security => "security",
            Self::Protection => "protection",
            Self::WindowsUpdate => "windowsUpdate",
            Self::Software => "software",
            Self::DiskHealth => "diskHealth",
            Self::DiskOptimize => "diskOptimize",
        }
    }
}

/// Final state of one category after a scan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CareCategoryState {
    /// Nothing to do (UI ✓).
    Clean,
    /// Fixable items found (UI count badge).
    Issues,
    /// Needs the user's attention but nothing is auto-fixable (UI ⓘ), e.g. updates available.
    Attention,
    /// Not relevant on this machine (tool missing, feature unavailable).
    NotApplicable,
    /// The user skipped it, or the scan was stopped before it finished.
    Skipped,
    /// The scan of this category failed (diagnostic in `reason`).
    Failed,
}

/// How safe it is to fix an item; drives default selection and Auto fix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CareRisk {
    /// Safe to fix automatically (pure caches, reversible settings).
    Safe,
    /// Fixable but the user should review (history, cookies, startup entries, …). Never auto-fixed.
    Review,
    /// Informational only; `fixable` is false or the fix opens a Windows page.
    Info,
}

/// One finding. Text is either a locale key (+ params) or a literal display string
/// (program / item names). `location` is for display only — never write it to logs/history.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CareItem {
    /// Unique within the category for this scan (stable across rescans when possible).
    pub id: String,
    pub category: CareCategoryId,
    /// Locale key for the title (preferred for fixed items).
    pub title_key: Option<String>,
    /// Literal title (program names, shortcut names…), used when `title_key` is None.
    pub title: Option<String>,
    /// Params for `title_key` / `detail_key` interpolation.
    #[serde(default)]
    pub params: std::collections::BTreeMap<String, String>,
    pub detail_key: Option<String>,
    pub detail: Option<String>,
    /// Display-only location hint (path / registry key / app name).
    pub location: Option<String>,
    /// Reclaimable bytes when meaningful.
    pub bytes: Option<u64>,
    /// Number of underlying entries (files, values…), at least 1.
    pub count: u64,
    pub risk: CareRisk,
    /// Preselected in the result list (only `Safe` items should be).
    pub selected: bool,
    /// False for informational items.
    pub fixable: bool,
    /// Fixing requires an elevated process (honest skip when not elevated).
    pub needs_admin: bool,
    /// Optional action the UI can offer instead of / besides fixing (e.g. "openWindowsUpdate").
    pub action: Option<String>,
}

/// Result of scanning one category.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CareCategoryScan {
    pub category: CareCategoryId,
    pub state: CareCategoryState,
    pub items: Vec<CareItem>,
    /// Typed reason for NotApplicable / Failed / Attention (e.g. "toolUnavailable", "needsAdmin").
    pub reason: Option<String>,
    pub elapsed_ms: u64,
}

impl CareCategoryScan {
    /// Convenience: build from items, choosing Clean / Issues / Attention automatically.
    pub fn from_items(category: CareCategoryId, items: Vec<CareItem>) -> Self {
        let state = if items.iter().any(|i| i.fixable) {
            CareCategoryState::Issues
        } else if items.is_empty() {
            CareCategoryState::Clean
        } else {
            CareCategoryState::Attention
        };
        Self { category, state, items, reason: None, elapsed_ms: 0 }
    }

    pub fn with_state(category: CareCategoryId, state: CareCategoryState, reason: Option<&str>) -> Self {
        Self { category, state, items: Vec::new(), reason: reason.map(str::to_string), elapsed_ms: 0 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CareFixStatus {
    Fixed,
    Partial,
    Skipped,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CareFixResult {
    pub item_id: String,
    pub category: CareCategoryId,
    pub status: CareFixStatus,
    /// Typed reason: needsAdmin | runningProcesses | itemChanged | cancelled | failed | …
    pub reason: Option<String>,
    pub freed_bytes: u64,
    /// True when an undo record was stored for this fix.
    pub undoable: bool,
}

/// Live events a category may report while scanning / fixing (thread-safe; categories run in parallel).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CareEvent {
    /// Path / item currently being examined (display only).
    Current { category: CareCategoryId, text: String },
    /// Running count of issues found so far in this category.
    Found { category: CareCategoryId, count: u64 },
}

/// Shared context handed to categories. Cheap to clone.
#[derive(Clone)]
pub struct CareContext {
    pub is_admin: bool,
    stop: Arc<AtomicBool>,
    skip: Arc<AtomicBool>,
    sink: Arc<dyn Fn(CareEvent) + Send + Sync>,
}

impl CareContext {
    pub fn new(
        is_admin: bool,
        stop: Arc<AtomicBool>,
        skip: Arc<AtomicBool>,
        sink: Arc<dyn Fn(CareEvent) + Send + Sync>,
    ) -> Self {
        Self { is_admin, stop, skip, sink }
    }

    /// For tests: a context that never cancels and drops events.
    pub fn detached(is_admin: bool) -> Self {
        Self::new(
            is_admin,
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicBool::new(false)),
            Arc::new(|_| {}),
        )
    }

    /// True when the whole operation was stopped or this category was skipped.
    /// Long loops must check this often and return early (state Skipped).
    pub fn should_stop(&self) -> bool {
        self.stop.load(Ordering::Relaxed) || self.skip.load(Ordering::Relaxed)
    }

    pub fn current(&self, category: CareCategoryId, text: impl Into<String>) {
        (self.sink)(CareEvent::Current { category, text: text.into() });
    }

    pub fn found(&self, category: CareCategoryId, count: u64) {
        (self.sink)(CareEvent::Found { category, count });
    }
}

/// A Care category. Implementations live in `categories/<name>.rs`.
pub trait CareCategory: Send + Sync {
    fn id(&self) -> CareCategoryId;

    /// Typical scan duration in ms on a normal PC; seeds progress weighting before
    /// the engine has measured real timings.
    fn default_weight_ms(&self) -> u64 {
        1_000
    }

    /// Read-only scan. Must not change the system. Return state Skipped when `ctx.should_stop()`.
    fn scan(&self, ctx: &CareContext) -> CoreResult<CareCategoryScan>;

    /// Fix the given item ids from this category's latest scan. Implementations must re-validate
    /// each item (it may have changed since the scan) and store undo data where the change is
    /// a setting/registry/startup change.
    fn fix(&self, item_ids: &[String], ctx: &CareContext) -> CoreResult<Vec<CareFixResult>>;
}

/// Helper for categories: a skipped scan result.
pub fn skipped(category: CareCategoryId) -> CareCategoryScan {
    CareCategoryScan::with_state(category, CareCategoryState::Skipped, Some("skipped"))
}

/// Helper for categories: mark every requested id with the same status.
pub fn fix_all(
    category: CareCategoryId,
    item_ids: &[String],
    status: CareFixStatus,
    reason: Option<&str>,
) -> Vec<CareFixResult> {
    item_ids
        .iter()
        .map(|id| CareFixResult {
            item_id: id.clone(),
            category,
            status,
            reason: reason.map(str::to_string),
            freed_bytes: 0,
            undoable: false,
        })
        .collect()
}

/// Allowlisted `CareItem::action` ids the UI may open, mapped to Windows settings URIs.
/// Categories should only emit action ids listed here; anything else is rejected by the adapter.
pub const CARE_ACTIONS: [(&str, &str); 10] = [
    ("openWindowsUpdate", "ms-settings:windowsupdate"),
    ("openWindowsSecurity", "windowsdefender:"),
    ("openVirusProtection", "windowsdefender://threat"),
    ("openFirewall", "windowsdefender://network"),
    ("openStartupApps", "ms-settings:startupapps"),
    ("openStorage", "ms-settings:storagesense"),
    ("openAppsFeatures", "ms-settings:appsfeatures"),
    ("openPrivacy", "ms-settings:privacy"),
    ("openRecovery", "ms-settings:recovery"),
    ("openBackup", "ms-settings:backup"),
];

/// URI for an allowlisted Care action id (exact match).
pub fn care_action_uri(action: &str) -> Option<&'static str> {
    CARE_ACTIONS.iter().find(|(id, _)| *id == action).map(|(_, uri)| *uri)
}
