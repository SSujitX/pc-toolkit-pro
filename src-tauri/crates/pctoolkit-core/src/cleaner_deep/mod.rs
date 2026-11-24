//! Deep Cleanup: data-driven rule catalog, parallel scan, and verified execution.
//!
//! The catalog lives in embedded TOML (`rules/deep_cleanup/*.toml`) and is
//! validated on load (`catalog`). Every root is re-validated against the safety
//! policy (`safety`) before it is measured and again before anything under it is
//! deleted. Link-like entries (symlinks, junctions, any reparse point, cloud
//! placeholders) are never followed and never deleted.
//!
//! Path lists are original PC Toolkit Pro definitions of public Windows / app
//! cache locations — not copied from third-party GPL sources.
//!
//! Extension points for other rule sources:
//! - `RuleKind::Special(SpecialKind)` for one-off system operations (`special`).
//!   Windows Disk Cleanup handlers, Docker build cache, Conda, and rustup
//!   toolchains are special kinds declared in `rules/deep_cleanup/special.toml`.
//! - `DeepCleanupProvider` for dynamic rule sources: project build artifacts
//!   (`projects`) and AI model stores (`ai_provider`). Providers reuse the safe
//!   walk primitives in `walk` (`measure_directory` / `clean_directory`).

mod ai_provider;
mod catalog;
mod execute;
mod matcher;
mod projects;
pub mod provider;
mod resolve;
pub mod safety;
mod scan;
mod scope;
mod special;
pub mod walk;

use serde::{Deserialize, Serialize};

use crate::cleaner::acquire_busy;
use crate::shared::CoreResult;

pub use catalog::{validate_embedded_catalog, CatalogIssue};
pub use matcher::MatcherSpec;
pub use provider::{DeepCleanupProvider, ProviderContext};
pub use safety::{RootPolicy, RootRefusal, SafetyContext};
pub use scope::{list_deep_cleanup_drives, set_deep_cleanup_project_drives, DeepCleanupDrive};
pub use special::SpecialKind;
pub use walk::{clean_directory, measure_directory, CleanMode, CleanTally, MeasureTally};

/// Category shown in the Deep Cleanup sidebar. Order here is display order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DeepCleanupGroup {
    System,
    Application,
    Browser,
    Development,
    /// Project build artifacts (filled by a dedicated provider).
    Projects,
    /// AI tool caches and model stores (`ai.toml` + `cleaner_ai_models`).
    Ai,
}

impl DeepCleanupGroup {
    pub const ALL: [DeepCleanupGroup; 6] = [
        Self::System,
        Self::Application,
        Self::Browser,
        Self::Development,
        Self::Projects,
        Self::Ai,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Application => "application",
            Self::Browser => "browser",
            Self::Development => "development",
            Self::Projects => "projects",
            Self::Ai => "ai",
        }
    }
}

/// `safe` = rebuilt transparently; `recoverable` = re-downloaded or regenerated on demand.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DeepCleanupRisk {
    Safe,
    Recoverable,
}

/// Scan status of one rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DeepCleanupRuleStatus {
    /// Reclaimable data found.
    Found,
    /// Roots exist but nothing matched.
    Clean,
    /// No valid root exists on this PC (app not installed, root refused, ...).
    NotApplicable,
    /// Data found but the per-rule entry safety limit stopped the walk; size is a lower bound.
    Limited,
    /// Data found and a related process is running; cleaning is blocked until it closes.
    RequiresClose,
    /// The target exists but its size could not be measured (needs administrator,
    /// or the measurement timed out). `bytes` is 0; `extras.reason` says why.
    Unmeasured,
}

/// Typed reason attached to a scan result whose status is not plain `found` / `clean`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DeepCleanupScanReason {
    /// Measuring or cleaning needs an elevated process.
    NeedsAdmin,
    /// The tool (docker, conda, rustup) is not installed.
    ToolUnavailable,
    /// The Docker CLI exists but the engine is not running.
    DaemonUnavailable,
    /// The Windows Disk Cleanup handler is not registered on this build.
    HandlerMissing,
    /// The measurement did not finish within its time budget.
    TimedOut,
    /// The tool or handler failed or returned output we could not trust.
    Failed,
    /// Project discovery stopped at a depth / entry / time limit; more projects may exist.
    DiscoveryLimited,
}

/// Optional per-item facts for provider results (projects, AI models, toolchains).
/// Catalog walk rules leave these empty; the UI then renders `name_key` / `detail_key`.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeepCleanupRuleExtras {
    /// Literal item name (project folder name, model id). Replaces `name_key` in the UI.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    /// Short location hint for the UI only (`~\code\app`). Never logged or persisted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path_hint: Option<String>,
    /// Typed codes: project ecosystems (`node`, `rust`) or the AI model provider.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    /// What will be removed, relative to the item (`node_modules`, `.next/cache`) or
    /// toolchain names. Display only.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub artifacts: Vec<String>,
    /// Newest modification time seen (Unix ms).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_modified_ms: Option<i64>,
    /// The project was modified recently (active work); never preselected.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub active: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<DeepCleanupScanReason>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeepCleanupProgress {
    /// `scanning` | `executing` | `complete`
    pub phase: String,
    /// Path being read (UI only; never logged or persisted).
    pub current_path: String,
    pub items_scanned: u64,
    pub bytes_scanned: u64,
    pub elapsed_ms: u64,
    /// Locale key of the rule being processed, or `complete`.
    pub message: String,
    /// Work units completed (rules finished). UI percent = current / total.
    #[serde(default)]
    pub current: u64,
    /// Work units in this operation (rules).
    #[serde(default)]
    pub total: u64,
    /// Rule currently reported, when one is active.
    pub rule_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeepCleanupRuleResult {
    pub id: String,
    pub group: DeepCleanupGroup,
    pub name_key: String,
    pub detail_key: String,
    pub risk: DeepCleanupRisk,
    pub bytes: u64,
    /// Matching files (logical count; hardlinks are not deduplicated).
    pub item_count: u64,
    pub recommended: bool,
    /// Initial selection (Smart = recommended with bytes > 0).
    pub selected: bool,
    pub status: DeepCleanupRuleStatus,
    /// Process image names that must be closed before cleaning this rule.
    pub related_processes: Vec<String>,
    /// Subset of `related_processes` running at scan time.
    pub running_processes: Vec<String>,
    /// True when cleaning needs an elevated process (honest skip if not admin).
    pub requires_elevation: bool,
    /// `requires_elevation` and the current process is not elevated.
    pub needs_admin: bool,
    /// The entry safety limit stopped the walk (size is a lower bound).
    pub limited: bool,
    /// Roots that existed but were refused by the safety policy.
    pub refused_roots: u32,
    #[serde(flatten)]
    pub extras: DeepCleanupRuleExtras,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeepCleanupScan {
    pub rules: Vec<DeepCleanupRuleResult>,
    /// Groups that have at least one rule (catalog or provider), in display order.
    pub groups: Vec<DeepCleanupGroup>,
    pub is_admin: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeepCleanupExecuteRequest {
    pub rule_ids: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DeepCleanupExecStatus {
    Completed,
    Partial,
    Blocked,
    Skipped,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DeepCleanupExecReason {
    RunningProcesses,
    NeedsAdmin,
    ItemsBusy,
    ItemsFailed,
    Cancelled,
    UnknownRule,
    NotApplicable,
    /// The item changed on disk after the scan (AI model fingerprint, project marker); nothing removed.
    ChangedSinceScan,
    /// The external tool (docker, conda, rustup) failed or is not reachable.
    ToolFailed,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeepCleanupRuleExecResult {
    pub id: String,
    pub status: DeepCleanupExecStatus,
    pub reason: Option<DeepCleanupExecReason>,
    /// Sum of sizes of files actually removed (from live metadata before delete).
    pub freed_bytes: u64,
    pub removed_count: u64,
    /// Items that could not be removed (includes `busy_count`).
    pub failed_count: u64,
    /// Items skipped because another process holds them open (sharing violation).
    pub busy_count: u64,
}

impl DeepCleanupRuleExecResult {
    pub fn without_work(
        id: impl Into<String>,
        status: DeepCleanupExecStatus,
        reason: Option<DeepCleanupExecReason>,
    ) -> Self {
        Self {
            id: id.into(),
            status,
            reason,
            freed_bytes: 0,
            removed_count: 0,
            failed_count: 0,
            busy_count: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeepCleanupResult {
    pub freed_bytes: u64,
    pub files_removed: u64,
    pub failed_count: u64,
    pub busy_count: u64,
    /// The user cancelled; rules not reached are reported as `skipped(cancelled)`.
    pub cancelled: bool,
    pub rules: Vec<DeepCleanupRuleExecResult>,
    /// Diagnostic lines: rule ids, statuses, and counts only (no paths).
    pub log: Vec<String>,
}

/// Scan every catalog rule and provider. Shares the cleaner busy/cancel flags.
pub fn scan_deep_cleanup_with_progress<F>(on_progress: F) -> CoreResult<DeepCleanupScan>
where
    F: FnMut(DeepCleanupProgress),
{
    let _busy = acquire_busy()?;
    let result = scan::run(on_progress);
    result
}

/// Scan only the catalog rules of `groups` (no providers: no project discovery, no AI
/// models). Used by Care Junk, which needs plain caches quickly. Same busy/cancel flags.
pub fn scan_deep_cleanup_groups<F>(groups: &[DeepCleanupGroup], on_progress: F) -> CoreResult<DeepCleanupScan>
where
    F: FnMut(DeepCleanupProgress),
{
    let _busy = acquire_busy()?;
    let result = scan::run_filtered(Some(groups), on_progress);
    result
}

/// Clean the requested rule ids. Ids are resolved against the Core catalog and
/// providers; the frontend never sends paths.
pub fn execute_deep_cleanup<F>(
    request: DeepCleanupExecuteRequest,
    on_progress: F,
) -> CoreResult<DeepCleanupResult>
where
    F: FnMut(DeepCleanupProgress),
{
    let _busy = acquire_busy()?;
    let result = execute::run(request, on_progress);
    result
}
