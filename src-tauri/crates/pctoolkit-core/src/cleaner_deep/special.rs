//! Special (non-walk) catalog rule kinds.
//!
//! A special kind is a one-off system operation with its own measurement and
//! execution, declared in TOML with `kind = "<snake_case>"` (Recycle Bin in
//! `system.toml`, the rest in `special.toml`). To add one:
//! 1. add a variant here and a `KindSpec` variant in `catalog.rs`;
//! 2. implement it in `measure_special` / `execute_special`;
//! 3. declare the rule in TOML (no roots / matcher); the catalog gate checks group,
//!    elevation, and recommendation against `group` / `requires_elevation` /
//!    `may_be_recommended` below.
//!
//! Disk Cleanup handlers run on a dedicated thread that initializes COM as STA for
//! the call (`pctoolkit_platform::disk_cleanup`), so a pooled thread that already
//! joined an MTA never breaks them. Estimates are time-boxed: a handler that does
//! not answer in time is reported `unmeasured(timedOut)` and left to finish on its
//! own thread (read-only), so one slow handler never stalls the scan. Windows.old is
//! only ever removed through the Previous Installations handler — never by hand.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use pctoolkit_platform::{
    conda_clean_execute, conda_clean_preview, docker_build_cache_preview, docker_build_cache_prune,
    empty_recycle_bin, query_recycle_bin, rust_toolchains_list, rust_toolchains_uninstall,
    volume_cache_estimate_cancellable, volume_cache_purge, windows_old_present, DevToolError,
    DevToolReason, RustToolchainUninstallStatus, VolumeCacheKind, VolumeCachePurgeStatus,
    VolumeCacheStatus,
};

use super::projects::cached_project_roots;
use super::{DeepCleanupExecReason, DeepCleanupExecStatus, DeepCleanupGroup, DeepCleanupScanReason};
use crate::cleaner::CANCELLED;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SpecialKind {
    /// Current user's Recycle Bin via `SHQueryRecycleBinW` / `SHEmptyRecycleBinW`.
    /// Never walks `X:\$Recycle.Bin` (other SIDs, junctions).
    RecycleBin,
    /// A Windows Disk Cleanup handler (`IEmptyVolumeCache`) on the system drive.
    DiskCleanup(VolumeCacheKind),
    /// `docker builder prune --all --force` on the default context.
    DockerBuildCache,
    /// `conda clean --index-cache --tarballs --logfiles`.
    CondaCaches,
    /// rustup toolchains that are not default / active / overridden / pinned by a
    /// discovered project.
    RustToolchains,
}

impl SpecialKind {
    /// The only group a rule of this kind may declare.
    pub(crate) fn group(self) -> DeepCleanupGroup {
        match self {
            Self::RecycleBin | Self::DiskCleanup(_) => DeepCleanupGroup::System,
            Self::DockerBuildCache | Self::CondaCaches | Self::RustToolchains => DeepCleanupGroup::Development,
        }
    }

    /// Whether the rule must declare `requires_elevation = true`.
    pub(crate) fn requires_elevation(self) -> bool {
        match self {
            Self::DiskCleanup(kind) => kind.requires_admin(),
            _ => false,
        }
    }

    /// Kinds Smart selection may preselect. Recycle Bin, servicing / upgrade data,
    /// and tool-owned stores are always an explicit user choice.
    pub(crate) fn may_be_recommended(self) -> bool {
        matches!(
            self,
            Self::DiskCleanup(
                VolumeCacheKind::InternetCache
                    | VolumeCacheKind::DeliveryOptimization
                    | VolumeCacheKind::DefenderCache
                    | VolumeCacheKind::ErrorReports
            )
        )
    }
}

/// Read-only inputs for one special measurement / execution.
pub(crate) struct SpecialContext {
    pub is_admin: bool,
}

/// How a special measurement turned out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SpecialAvailability {
    /// Size is known (possibly zero).
    Measured,
    /// The target exists but its size is unknown.
    Unmeasured(DeepCleanupScanReason),
    /// Nothing to offer on this PC; the reason (if any) is diagnostic.
    NotApplicable(Option<DeepCleanupScanReason>),
}

#[derive(Debug, Clone)]
pub(crate) struct SpecialMeasure {
    pub availability: SpecialAvailability,
    pub bytes: u64,
    pub items: u64,
    /// Display-only names of what would be removed (toolchain names).
    pub artifacts: Vec<String>,
}

impl SpecialMeasure {
    fn measured(bytes: u64, items: u64) -> Self {
        Self {
            availability: SpecialAvailability::Measured,
            bytes,
            items,
            artifacts: Vec::new(),
        }
    }

    fn unmeasured(reason: DeepCleanupScanReason) -> Self {
        Self {
            availability: SpecialAvailability::Unmeasured(reason),
            bytes: 0,
            items: 0,
            artifacts: Vec::new(),
        }
    }

    fn not_applicable(reason: Option<DeepCleanupScanReason>) -> Self {
        Self {
            availability: SpecialAvailability::NotApplicable(reason),
            bytes: 0,
            items: 0,
            artifacts: Vec::new(),
        }
    }
}

pub(crate) struct SpecialOutcome {
    pub status: DeepCleanupExecStatus,
    pub reason: Option<DeepCleanupExecReason>,
    pub freed_bytes: u64,
    pub removed: u64,
    pub failed: u64,
}

impl SpecialOutcome {
    fn new(status: DeepCleanupExecStatus, reason: Option<DeepCleanupExecReason>) -> Self {
        Self {
            status,
            reason,
            freed_bytes: 0,
            removed: 0,
            failed: 0,
        }
    }
}

fn user_cancelled() -> bool {
    CANCELLED.load(Ordering::Relaxed)
}

/// Time budget for one Disk Cleanup estimate. Servicing-store and Windows.old
/// handlers walk huge trees, so they get longer (they only run elevated).
fn estimate_budget(kind: VolumeCacheKind) -> Duration {
    match kind {
        VolumeCacheKind::UpdateCleanup | VolumeCacheKind::PreviousInstallations => Duration::from_secs(20),
        _ => Duration::from_secs(3),
    }
}

/// Toolchain names offered by the last scan; execute only removes names that were
/// reviewed (a toolchain installed after the scan is never removed).
static SCANNED_TOOLCHAINS: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Pure mapping of a handler estimate to a measurement.
pub(crate) fn estimate_measure(kind: VolumeCacheKind, status: VolumeCacheStatus, bytes: u64) -> SpecialMeasure {
    use DeepCleanupScanReason as R;
    match status {
        VolumeCacheStatus::Ready => SpecialMeasure::measured(bytes, u64::from(bytes > 0)),
        VolumeCacheStatus::Empty => SpecialMeasure::measured(0, 0),
        VolumeCacheStatus::HandlerMissing => SpecialMeasure::not_applicable(Some(R::HandlerMissing)),
        // The component store always exists; Windows.old presence was checked before
        // the handler ran. Other admin-only kinds stay hidden when not measurable.
        VolumeCacheStatus::NeedsAdmin => match kind {
            VolumeCacheKind::UpdateCleanup
            | VolumeCacheKind::PreviousInstallations
            | VolumeCacheKind::DeliveryOptimization => SpecialMeasure::unmeasured(R::NeedsAdmin),
            _ => SpecialMeasure::not_applicable(Some(R::NeedsAdmin)),
        },
        VolumeCacheStatus::SizeUnknown => SpecialMeasure::unmeasured(R::Failed),
        VolumeCacheStatus::Cancelled => SpecialMeasure::unmeasured(R::TimedOut),
        VolumeCacheStatus::Failed => SpecialMeasure::not_applicable(Some(R::Failed)),
    }
}

/// Run one estimate on its own STA thread; give up waiting after the budget.
fn measure_disk_cleanup(kind: VolumeCacheKind, ctx: &SpecialContext) -> SpecialMeasure {
    if kind == VolumeCacheKind::PreviousInstallations && !windows_old_present() {
        return SpecialMeasure::not_applicable(None);
    }
    if kind.requires_admin() && !ctx.is_admin {
        return estimate_measure(kind, VolumeCacheStatus::NeedsAdmin, 0);
    }
    let budget = estimate_budget(kind);
    let gave_up = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::channel();
    let flag = Arc::clone(&gave_up);
    let started = Instant::now();
    let spawned = std::thread::Builder::new()
        .name("deep-cleanup-volume-cache".into())
        .spawn(move || {
            let should_cancel = || {
                user_cancelled() || flag.load(Ordering::Relaxed) || started.elapsed() >= budget
            };
            let _ = tx.send(volume_cache_estimate_cancellable(kind, &should_cancel));
        });
    if spawned.is_err() {
        return SpecialMeasure::not_applicable(Some(DeepCleanupScanReason::Failed));
    }
    // A little grace beyond the budget for the handler to notice the cancel flag.
    match rx.recv_timeout(budget + Duration::from_millis(500)) {
        Ok(Ok(estimate)) => estimate_measure(kind, estimate.status, estimate.bytes),
        Ok(Err(_)) => SpecialMeasure::not_applicable(Some(DeepCleanupScanReason::Failed)),
        Err(_) => {
            gave_up.store(true, Ordering::Relaxed);
            SpecialMeasure::unmeasured(DeepCleanupScanReason::TimedOut)
        }
    }
}

/// Pure mapping of a tool error to a scan result. Missing tools stay hidden.
pub(crate) fn tool_error_measure(error: &DevToolError) -> SpecialMeasure {
    use DeepCleanupScanReason as R;
    match error.reason {
        DevToolReason::ToolUnavailable => SpecialMeasure::not_applicable(Some(R::ToolUnavailable)),
        DevToolReason::DaemonUnavailable => SpecialMeasure::not_applicable(Some(R::DaemonUnavailable)),
        DevToolReason::Timeout => SpecialMeasure::not_applicable(Some(R::TimedOut)),
        DevToolReason::Cancelled => SpecialMeasure::not_applicable(None),
        DevToolReason::InvalidOutput | DevToolReason::Failed => SpecialMeasure::not_applicable(Some(R::Failed)),
    }
}

pub(crate) fn measure_special(kind: SpecialKind, ctx: &SpecialContext) -> SpecialMeasure {
    let cancel = || user_cancelled();
    match kind {
        SpecialKind::RecycleBin => match query_recycle_bin() {
            // A non-empty bin always reports at least one item.
            Ok(info) => SpecialMeasure::measured(
                info.bytes,
                if info.bytes > 0 { info.item_count.max(1) } else { info.item_count },
            ),
            Err(_) => SpecialMeasure::not_applicable(Some(DeepCleanupScanReason::Failed)),
        },
        SpecialKind::DiskCleanup(volume) => measure_disk_cleanup(volume, ctx),
        SpecialKind::DockerBuildCache => match docker_build_cache_preview(&cancel) {
            Ok(preview) => SpecialMeasure::measured(preview.reclaimable_bytes, u64::from(preview.record_count)),
            Err(e) => tool_error_measure(&e),
        },
        SpecialKind::CondaCaches => match conda_clean_preview(&cancel) {
            Ok(preview) => SpecialMeasure::measured(preview.total_bytes, u64::from(preview.total_bytes > 0)),
            Err(e) => tool_error_measure(&e),
        },
        SpecialKind::RustToolchains => {
            let roots = cached_project_roots();
            match rust_toolchains_list(&roots) {
                Ok(inventory) => {
                    let names: Vec<String> = inventory
                        .toolchains
                        .iter()
                        .filter(|t| !t.protected)
                        .map(|t| t.name.clone())
                        .collect();
                    if let Ok(mut slot) = SCANNED_TOOLCHAINS.lock() {
                        slot.clone_from(&names);
                    }
                    let mut measure = SpecialMeasure::measured(inventory.removable_bytes, names.len() as u64);
                    measure.artifacts = names;
                    measure
                }
                Err(e) => tool_error_measure(&e),
            }
        }
    }
}

/// Pure mapping of a purge result to an outcome.
pub(crate) fn purge_outcome(status: VolumeCachePurgeStatus, released: u64) -> SpecialOutcome {
    use DeepCleanupExecReason as R;
    use DeepCleanupExecStatus as S;
    let mut outcome = match status {
        VolumeCachePurgeStatus::Completed | VolumeCachePurgeStatus::NothingToPurge => SpecialOutcome::new(S::Completed, None),
        VolumeCachePurgeStatus::Partial => SpecialOutcome::new(S::Partial, Some(R::ItemsFailed)),
        VolumeCachePurgeStatus::Cancelled => SpecialOutcome::new(S::Skipped, Some(R::Cancelled)),
        VolumeCachePurgeStatus::NeedsAdmin => SpecialOutcome::new(S::Skipped, Some(R::NeedsAdmin)),
        VolumeCachePurgeStatus::Unavailable => SpecialOutcome::new(S::Skipped, Some(R::NotApplicable)),
        VolumeCachePurgeStatus::Failed => SpecialOutcome::new(S::Failed, Some(R::ItemsFailed)),
    };
    outcome.freed_bytes = released;
    outcome.removed = u64::from(released > 0);
    if matches!(outcome.status, S::Partial | S::Failed) {
        outcome.failed = 1;
    }
    outcome
}

fn tool_failure(error: &DevToolError) -> SpecialOutcome {
    use DeepCleanupExecReason as R;
    use DeepCleanupExecStatus as S;
    match error.reason {
        DevToolReason::Cancelled => SpecialOutcome::new(S::Skipped, Some(R::Cancelled)),
        DevToolReason::ToolUnavailable | DevToolReason::DaemonUnavailable => {
            SpecialOutcome::new(S::Skipped, Some(R::NotApplicable))
        }
        _ => SpecialOutcome {
            failed: 1,
            ..SpecialOutcome::new(S::Failed, Some(R::ToolFailed))
        },
    }
}

fn verified_release(before: u64, remaining: u64) -> SpecialOutcome {
    let released = before.saturating_sub(remaining);
    if remaining == 0 || released > 0 {
        let mut outcome = SpecialOutcome::new(
            if remaining == 0 {
                DeepCleanupExecStatus::Completed
            } else {
                DeepCleanupExecStatus::Partial
            },
            (remaining > 0).then_some(DeepCleanupExecReason::ItemsFailed),
        );
        outcome.freed_bytes = released;
        outcome.removed = u64::from(released > 0);
        outcome.failed = u64::from(remaining > 0);
        outcome
    } else {
        SpecialOutcome {
            failed: 1,
            ..SpecialOutcome::new(DeepCleanupExecStatus::Failed, Some(DeepCleanupExecReason::ToolFailed))
        }
    }
}

/// Pure tally of rustup uninstall results.
pub(crate) fn toolchain_outcome(results: &[(RustToolchainUninstallStatus, u64)]) -> SpecialOutcome {
    use DeepCleanupExecReason as R;
    use DeepCleanupExecStatus as S;
    let mut outcome = SpecialOutcome::new(S::Completed, None);
    let mut cancelled = false;
    for (status, released) in results {
        match status {
            RustToolchainUninstallStatus::Removed => {
                outcome.removed += 1;
                outcome.freed_bytes += released;
            }
            // Became protected or vanished since the scan: nothing to do, not a failure.
            RustToolchainUninstallStatus::Protected | RustToolchainUninstallStatus::NotFound => {}
            RustToolchainUninstallStatus::Cancelled => cancelled = true,
            RustToolchainUninstallStatus::StillPresent | RustToolchainUninstallStatus::Failed => outcome.failed += 1,
        }
    }
    if cancelled {
        outcome.status = if outcome.removed > 0 { S::Partial } else { S::Skipped };
        outcome.reason = Some(R::Cancelled);
    } else if outcome.failed > 0 {
        outcome.status = if outcome.removed > 0 { S::Partial } else { S::Failed };
        outcome.reason = Some(R::ToolFailed);
    }
    outcome
}

pub(crate) fn execute_special(kind: SpecialKind, ctx: &SpecialContext) -> SpecialOutcome {
    let cancel = || user_cancelled();
    match kind {
        SpecialKind::RecycleBin => match empty_recycle_bin() {
            Ok(result) => SpecialOutcome {
                freed_bytes: result.released_bytes,
                removed: result.released_items,
                ..SpecialOutcome::new(DeepCleanupExecStatus::Completed, None)
            },
            Err(_) => SpecialOutcome {
                failed: 1,
                ..SpecialOutcome::new(DeepCleanupExecStatus::Failed, Some(DeepCleanupExecReason::ItemsFailed))
            },
        },
        SpecialKind::DiskCleanup(volume) => {
            if volume.requires_admin() && !ctx.is_admin {
                return SpecialOutcome::new(DeepCleanupExecStatus::Skipped, Some(DeepCleanupExecReason::NeedsAdmin));
            }
            if volume == VolumeCacheKind::PreviousInstallations && !windows_old_present() {
                return SpecialOutcome::new(DeepCleanupExecStatus::Skipped, Some(DeepCleanupExecReason::NotApplicable));
            }
            // Fresh thread so COM is initialized STA regardless of the caller's apartment.
            let joined = std::thread::Builder::new()
                .name("deep-cleanup-volume-purge".into())
                .spawn(move || volume_cache_purge(volume, &|| user_cancelled()))
                .map(|handle| handle.join());
            match joined {
                Ok(Ok(Ok(purge))) => purge_outcome(purge.status, purge.released_bytes),
                _ => SpecialOutcome {
                    failed: 1,
                    ..SpecialOutcome::new(DeepCleanupExecStatus::Failed, Some(DeepCleanupExecReason::ItemsFailed))
                },
            }
        }
        SpecialKind::DockerBuildCache => match docker_build_cache_prune(&cancel) {
            Ok(prune) => verified_release(prune.before_bytes, prune.remaining_bytes),
            Err(e) => tool_failure(&e),
        },
        SpecialKind::CondaCaches => {
            let before = match conda_clean_preview(&cancel) {
                Ok(preview) => preview.total_bytes,
                Err(e) => return tool_failure(&e),
            };
            match conda_clean_execute(&cancel) {
                Ok(remaining) => verified_release(before, remaining.total_bytes),
                Err(e) => tool_failure(&e),
            }
        }
        SpecialKind::RustToolchains => {
            let names = SCANNED_TOOLCHAINS.lock().map(|n| n.clone()).unwrap_or_default();
            if names.is_empty() {
                return SpecialOutcome::new(DeepCleanupExecStatus::Skipped, Some(DeepCleanupExecReason::NotApplicable));
            }
            // Protection is recomputed by rustup_uninstall from a fresh listing and the
            // current project roots; names that became protected are refused per item.
            let roots = cached_project_roots();
            match rust_toolchains_uninstall(&names, &roots, &cancel) {
                Ok(results) => {
                    let pairs: Vec<_> = results.iter().map(|r| (r.status, r.released_bytes)).collect();
                    toolchain_outcome(&pairs)
                }
                Err(e) => tool_failure(&e),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use DeepCleanupScanReason as R;

    #[test]
    fn recommendation_is_limited_to_user_level_handlers() {
        assert!(SpecialKind::DiskCleanup(VolumeCacheKind::InternetCache).may_be_recommended());
        assert!(SpecialKind::DiskCleanup(VolumeCacheKind::ErrorReports).may_be_recommended());
        for kind in [
            SpecialKind::RecycleBin,
            SpecialKind::DiskCleanup(VolumeCacheKind::UpdateCleanup),
            SpecialKind::DiskCleanup(VolumeCacheKind::PreviousInstallations),
            SpecialKind::DiskCleanup(VolumeCacheKind::TemporarySetupFiles),
            SpecialKind::DockerBuildCache,
            SpecialKind::CondaCaches,
            SpecialKind::RustToolchains,
        ] {
            assert!(!kind.may_be_recommended(), "{kind:?}");
        }
        for kind in VolumeCacheKind::ALL {
            let special = SpecialKind::DiskCleanup(kind);
            assert!(!(special.may_be_recommended() && special.requires_elevation()), "{kind:?}");
        }
    }

    #[test]
    fn estimate_status_maps_honestly() {
        let k = VolumeCacheKind::DefenderCache;
        assert_eq!(estimate_measure(k, VolumeCacheStatus::Ready, 10).availability, SpecialAvailability::Measured);
        assert_eq!(estimate_measure(k, VolumeCacheStatus::Ready, 10).bytes, 10);
        assert_eq!(estimate_measure(k, VolumeCacheStatus::Empty, 0).bytes, 0);
        assert_eq!(
            estimate_measure(k, VolumeCacheStatus::HandlerMissing, 0).availability,
            SpecialAvailability::NotApplicable(Some(R::HandlerMissing))
        );
        assert_eq!(
            estimate_measure(VolumeCacheKind::UpdateCleanup, VolumeCacheStatus::NeedsAdmin, 0).availability,
            SpecialAvailability::Unmeasured(R::NeedsAdmin)
        );
        assert_eq!(
            estimate_measure(VolumeCacheKind::TemporarySetupFiles, VolumeCacheStatus::NeedsAdmin, 0).availability,
            SpecialAvailability::NotApplicable(Some(R::NeedsAdmin))
        );
        assert_eq!(
            estimate_measure(k, VolumeCacheStatus::Cancelled, 0).availability,
            SpecialAvailability::Unmeasured(R::TimedOut)
        );
    }

    #[test]
    fn missing_tools_are_hidden_and_daemon_down_has_a_reason() {
        let e = |reason| DevToolError { reason, exit_code: None };
        assert_eq!(
            tool_error_measure(&e(DevToolReason::ToolUnavailable)).availability,
            SpecialAvailability::NotApplicable(Some(R::ToolUnavailable))
        );
        assert_eq!(
            tool_error_measure(&e(DevToolReason::DaemonUnavailable)).availability,
            SpecialAvailability::NotApplicable(Some(R::DaemonUnavailable))
        );
    }

    #[test]
    fn purge_outcomes_never_claim_unverified_success() {
        use DeepCleanupExecStatus as S;
        let done = purge_outcome(VolumeCachePurgeStatus::Completed, 100);
        assert_eq!((done.status, done.freed_bytes, done.removed), (S::Completed, 100, 1));
        let partial = purge_outcome(VolumeCachePurgeStatus::Partial, 0);
        assert_eq!((partial.status, partial.failed), (S::Partial, 1));
        assert_eq!(purge_outcome(VolumeCachePurgeStatus::NeedsAdmin, 0).reason, Some(DeepCleanupExecReason::NeedsAdmin));
        assert_eq!(purge_outcome(VolumeCachePurgeStatus::Failed, 0).status, S::Failed);
        let nothing = verified_release(100, 100);
        assert_eq!(nothing.status, S::Failed);
        let half = verified_release(100, 40);
        assert_eq!((half.status, half.freed_bytes), (S::Partial, 60));
    }

    #[test]
    fn toolchain_tally_counts_each_item() {
        use DeepCleanupExecStatus as S;
        use RustToolchainUninstallStatus as T;
        let o = toolchain_outcome(&[(T::Removed, 5), (T::Protected, 0), (T::Removed, 7)]);
        assert_eq!((o.status, o.removed, o.freed_bytes), (S::Completed, 2, 12));
        let o = toolchain_outcome(&[(T::Removed, 5), (T::Failed, 0)]);
        assert_eq!((o.status, o.reason), (S::Partial, Some(DeepCleanupExecReason::ToolFailed)));
        let o = toolchain_outcome(&[(T::Cancelled, 0)]);
        assert_eq!((o.status, o.reason), (S::Skipped, Some(DeepCleanupExecReason::Cancelled)));
    }
}
