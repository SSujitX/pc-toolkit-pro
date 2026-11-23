//! Verified execution of selected Deep Cleanup rules.
//!
//! Roots are re-resolved from the catalog (the frontend only sends ids), ownership
//! is recomputed over every rule, each root is re-validated right before it is
//! cleaned, and every file is re-checked right before it is deleted.

use std::collections::{BTreeSet, HashSet};
use std::path::Path;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant, SystemTime};

use super::catalog::{CatalogRule, RuleKind};
use super::provider::{registered_providers, ProviderContext};
use super::resolve::{ownership_exclusions, resolve_rule, root_policy, BaseDirs, ResolvedRule};
use super::safety::{validate_root, RootRefusal, SafetyContext};
use super::scan::{catalog_or_error, running_for, running_process_set};
use super::special::{execute_special, SpecialContext};
use super::walk::{clean_root, CleanTally, WalkSpec};
use super::{
    DeepCleanupExecReason, DeepCleanupExecStatus, DeepCleanupExecuteRequest, DeepCleanupProgress,
    DeepCleanupResult, DeepCleanupRuleExecResult,
};
use crate::cleaner::CANCELLED;
use crate::history::{append_history, history_now_ms, HistoryOutcome, HistoryWrite};
use crate::shared::{CoreError, CoreResult};

const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);

/// Pure status decision from a clean tally.
pub(super) fn exec_status(tally: &CleanTally) -> (DeepCleanupExecStatus, Option<DeepCleanupExecReason>) {
    let problems = tally.failed + tally.busy;
    let problem_reason = if tally.failed == 0 {
        DeepCleanupExecReason::ItemsBusy
    } else {
        DeepCleanupExecReason::ItemsFailed
    };
    if tally.cancelled {
        let status = if tally.removed > 0 {
            DeepCleanupExecStatus::Partial
        } else {
            DeepCleanupExecStatus::Skipped
        };
        return (status, Some(DeepCleanupExecReason::Cancelled));
    }
    if problems == 0 {
        (DeepCleanupExecStatus::Completed, None)
    } else if tally.removed > 0 {
        (DeepCleanupExecStatus::Partial, Some(problem_reason))
    } else {
        (DeepCleanupExecStatus::Failed, Some(problem_reason))
    }
}

/// Pure history outcome over all rule results.
pub(super) fn history_outcome(results: &[DeepCleanupRuleExecResult], cancelled: bool) -> HistoryOutcome {
    if cancelled {
        return HistoryOutcome::Cancelled;
    }
    if results.is_empty() || results.iter().all(|r| r.status == DeepCleanupExecStatus::Completed) {
        return HistoryOutcome::Completed;
    }
    let any_progress = results.iter().any(|r| {
        matches!(r.status, DeepCleanupExecStatus::Completed | DeepCleanupExecStatus::Partial) || r.removed_count > 0
    });
    if any_progress {
        HistoryOutcome::CompletedWithWarnings
    } else {
        HistoryOutcome::Failed
    }
}

pub(super) fn status_code(status: DeepCleanupExecStatus) -> &'static str {
    match status {
        DeepCleanupExecStatus::Completed => "completed",
        DeepCleanupExecStatus::Partial => "partial",
        DeepCleanupExecStatus::Blocked => "blocked",
        DeepCleanupExecStatus::Skipped => "skipped",
        DeepCleanupExecStatus::Failed => "failed",
    }
}

fn reason_code(reason: DeepCleanupExecReason) -> &'static str {
    match reason {
        DeepCleanupExecReason::RunningProcesses => "runningProcesses",
        DeepCleanupExecReason::NeedsAdmin => "needsAdmin",
        DeepCleanupExecReason::ItemsBusy => "itemsBusy",
        DeepCleanupExecReason::ItemsFailed => "itemsFailed",
        DeepCleanupExecReason::Cancelled => "cancelled",
        DeepCleanupExecReason::UnknownRule => "unknownRule",
        DeepCleanupExecReason::NotApplicable => "notApplicable",
        DeepCleanupExecReason::ChangedSinceScan => "changedSinceScan",
        DeepCleanupExecReason::ToolFailed => "toolFailed",
    }
}

/// Diagnostic line: rule id, typed status/reason, and counts. Never a path.
pub(super) fn detail_line(r: &DeepCleanupRuleExecResult) -> String {
    let reason = r.reason.map(reason_code).map(|c| format!(" ({c})")).unwrap_or_default();
    format!(
        "{}: {}{} removed={} failed={} busy={} freedBytes={}",
        r.id,
        status_code(r.status),
        reason,
        r.removed_count,
        r.failed_count,
        r.busy_count,
        r.freed_bytes
    )
}

fn result_from_tally(id: &str, tally: &CleanTally) -> DeepCleanupRuleExecResult {
    let (status, reason) = exec_status(tally);
    DeepCleanupRuleExecResult {
        id: id.to_string(),
        status,
        reason,
        freed_bytes: tally.freed_bytes,
        removed_count: tally.removed,
        failed_count: tally.failed + tally.busy,
        busy_count: tally.busy,
    }
}

struct ExecEnv<'a> {
    safety: &'a SafetyContext,
    running: &'a HashSet<String>,
    is_admin: bool,
    now: SystemTime,
}

fn exec_catalog_rule(
    env: &ExecEnv<'_>,
    rule: &CatalogRule,
    resolved: &ResolvedRule,
    excluded: &HashSet<String>,
    on_removed: &mut dyn FnMut(&Path, u64),
) -> DeepCleanupRuleExecResult {
    let id = rule.spec.id.as_str();
    if rule.spec.requires_elevation && !env.is_admin {
        return DeepCleanupRuleExecResult::without_work(
            id,
            DeepCleanupExecStatus::Skipped,
            Some(DeepCleanupExecReason::NeedsAdmin),
        );
    }
    if !running_for(rule, env.running).is_empty() {
        return DeepCleanupRuleExecResult::without_work(
            id,
            DeepCleanupExecStatus::Blocked,
            Some(DeepCleanupExecReason::RunningProcesses),
        );
    }
    match rule.kind {
        RuleKind::Special(kind) => {
            let outcome = execute_special(kind, &SpecialContext { is_admin: env.is_admin });
            DeepCleanupRuleExecResult {
                id: id.to_string(),
                status: outcome.status,
                reason: outcome.reason,
                freed_bytes: outcome.freed_bytes,
                removed_count: outcome.removed,
                failed_count: outcome.failed,
                busy_count: 0,
            }
        }
        RuleKind::Files => {
            if resolved.roots.is_empty() {
                return DeepCleanupRuleExecResult::without_work(
                    id,
                    DeepCleanupExecStatus::Skipped,
                    Some(DeepCleanupExecReason::NotApplicable),
                );
            }
            let policy = root_policy(rule);
            let spec = WalkSpec {
                matcher: &rule.spec.matcher,
                excluded,
            };
            let mut tally = CleanTally::default();
            for root in &resolved.roots {
                if CANCELLED.load(Ordering::Relaxed) {
                    tally.cancelled = true;
                    break;
                }
                // Re-validate right before deleting: the tree may have changed since scan.
                match validate_root(root, env.safety, policy) {
                    Ok(()) => {}
                    Err(RootRefusal::Missing | RootRefusal::Inaccessible) => continue,
                    Err(_) => {
                        tally.failed += 1;
                        continue;
                    }
                }
                tally.absorb(clean_root(root, &spec, rule.spec.mode, env.now, on_removed));
            }
            result_from_tally(id, &tally)
        }
    }
}

fn dedupe_ids(ids: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    ids.iter()
        .filter(|id| seen.insert(id.as_str()))
        .cloned()
        .collect()
}

pub(super) fn run<F>(request: DeepCleanupExecuteRequest, mut on_progress: F) -> CoreResult<DeepCleanupResult>
where
    F: FnMut(DeepCleanupProgress),
{
    let started_at_ms = history_now_ms();
    let started = Instant::now();
    let ids = dedupe_ids(&request.rule_ids);
    if ids.is_empty() {
        return Err(CoreError::InvalidInput("no deep cleanup rules selected".into()));
    }
    let rules = catalog_or_error()?;
    let providers = registered_providers();
    let is_admin = pctoolkit_platform::is_user_admin();
    let safety = SafetyContext::from_env();
    let bases = BaseDirs::from_env();

    // Fresh process snapshot for the selected rules only.
    let mut names: BTreeSet<String> = BTreeSet::new();
    for id in &ids {
        if let Some(rule) = rules.iter().find(|r| &r.spec.id == id) {
            names.extend(rule.spec.processes.iter().cloned());
        }
    }
    for provider in &providers {
        if ids.iter().any(|id| provider.owns_rule(id)) {
            names.extend(provider.processes());
        }
    }
    let running = running_process_set(&names);

    // Ownership is recomputed over the whole catalog so unselected inner rules
    // keep protecting their subtrees.
    let resolved: Vec<ResolvedRule> = rules.iter().map(|r| resolve_rule(r, &bases, &safety)).collect();
    let excluded = ownership_exclusions(&resolved);
    let env = ExecEnv {
        safety: &safety,
        running: &running,
        is_admin,
        now: SystemTime::now(),
    };

    let total = ids.len() as u64;
    let mut slots: Vec<Option<DeepCleanupRuleExecResult>> = vec![None; ids.len()];
    let mut provider_batches: Vec<Vec<usize>> = vec![Vec::new(); providers.len()];
    let mut freed_so_far = 0u64;
    let mut removed_so_far = 0u64;
    let mut done = 0u64;

    for (slot, id) in ids.iter().enumerate() {
        if CANCELLED.load(Ordering::SeqCst) {
            break;
        }
        if let Some(index) = rules.iter().position(|r| &r.spec.id == id) {
            let rule = &rules[index];
            on_progress(DeepCleanupProgress {
                phase: "executing".into(),
                current_path: String::new(),
                items_scanned: removed_so_far,
                bytes_scanned: freed_so_far,
                elapsed_ms: started.elapsed().as_millis() as u64,
                message: rule.spec.name_key.clone(),
                current: done,
                total,
                rule_id: Some(id.clone()),
            });
            let mut last_emit = Instant::now();
            let base_freed = freed_so_far;
            let base_removed = removed_so_far;
            let mut rule_freed = 0u64;
            let mut rule_removed = 0u64;
            let result = exec_catalog_rule(&env, rule, &resolved[index], &excluded[index], &mut |path, len| {
                rule_freed += len;
                rule_removed += 1;
                if last_emit.elapsed() >= PROGRESS_INTERVAL {
                    last_emit = Instant::now();
                    on_progress(DeepCleanupProgress {
                        phase: "executing".into(),
                        current_path: path.display().to_string(),
                        items_scanned: base_removed + rule_removed,
                        bytes_scanned: base_freed + rule_freed,
                        elapsed_ms: started.elapsed().as_millis() as u64,
                        message: rule.spec.name_key.clone(),
                        current: done,
                        total,
                        rule_id: Some(rule.spec.id.clone()),
                    });
                }
            });
            freed_so_far += result.freed_bytes;
            removed_so_far += result.removed_count;
            done += 1;
            slots[slot] = Some(result);
        } else if let Some(p) = providers.iter().position(|p| p.owns_rule(id)) {
            provider_batches[p].push(slot);
        } else {
            done += 1;
            slots[slot] = Some(DeepCleanupRuleExecResult::without_work(
                id.clone(),
                DeepCleanupExecStatus::Skipped,
                Some(DeepCleanupExecReason::UnknownRule),
            ));
        }
    }

    // Provider rules run one id at a time so the progress card advances per item.
    for (p, batch) in provider_batches.iter().enumerate() {
        for &slot in batch {
            if CANCELLED.load(Ordering::SeqCst) {
                break;
            }
            let id = &ids[slot];
            on_progress(DeepCleanupProgress {
                phase: "executing".into(),
                current_path: String::new(),
                items_scanned: removed_so_far,
                bytes_scanned: freed_so_far,
                elapsed_ms: started.elapsed().as_millis() as u64,
                message: "deepCleaner.cleaning".into(),
                current: done,
                total,
                rule_id: Some(id.clone()),
            });
            let progress = |_: &str, _: &Path, _: u64, _: u64| {};
            let ctx = ProviderContext::new(is_admin, &running, &safety, &progress);
            let outcome = providers[p].execute(std::slice::from_ref(id), &ctx);
            let result = outcome
                .ok()
                .and_then(|list| list.into_iter().find(|r| &r.id == id))
                .unwrap_or_else(|| {
                    DeepCleanupRuleExecResult::without_work(
                        id.clone(),
                        DeepCleanupExecStatus::Failed,
                        Some(DeepCleanupExecReason::ItemsFailed),
                    )
                });
            freed_so_far += result.freed_bytes;
            removed_so_far += result.removed_count;
            done += 1;
            slots[slot] = Some(result);
        }
    }

    let cancelled = CANCELLED.load(Ordering::SeqCst)
        || slots
            .iter()
            .flatten()
            .any(|r| r.reason == Some(DeepCleanupExecReason::Cancelled));
    let results: Vec<DeepCleanupRuleExecResult> = slots
        .into_iter()
        .enumerate()
        .map(|(slot, r)| {
            r.unwrap_or_else(|| {
                DeepCleanupRuleExecResult::without_work(
                    ids[slot].clone(),
                    DeepCleanupExecStatus::Skipped,
                    Some(DeepCleanupExecReason::Cancelled),
                )
            })
        })
        .collect();

    let freed_bytes: u64 = results.iter().map(|r| r.freed_bytes).sum();
    let files_removed: u64 = results.iter().map(|r| r.removed_count).sum();
    let failed_count: u64 = results.iter().map(|r| r.failed_count).sum();
    let busy_count: u64 = results.iter().map(|r| r.busy_count).sum();
    let log: Vec<String> = results.iter().map(detail_line).collect();
    let outcome = history_outcome(&results, cancelled);

    on_progress(DeepCleanupProgress {
        phase: "complete".into(),
        current_path: String::new(),
        items_scanned: files_removed,
        bytes_scanned: freed_bytes,
        elapsed_ms: started.elapsed().as_millis() as u64,
        message: "complete".into(),
        current: total,
        total,
        rule_id: None,
    });

    append_history(HistoryWrite {
        category: "deepCleaner".into(),
        title_key: "history.titles.deepCleaner".into(),
        summary: format!("{} rules · {} items · {} bytes", results.len(), files_removed, freed_bytes),
        started_at_ms,
        finished_at_ms: history_now_ms(),
        outcome,
        planned_bytes: None,
        result_bytes: Some(freed_bytes),
        selected_item_count: results.len().min(u32::MAX as usize) as u32,
        affected_item_count: files_removed.min(u32::MAX as u64) as u32,
        failed_item_count: failed_count.min(u32::MAX as u64) as u32,
        detail_lines: log.clone(),
        action: "execute_deep_cleanup".into(),
        detail: Some(format!(
            "freed={freed_bytes} removed={files_removed} failed={failed_count} busy={busy_count} cancelled={cancelled}"
        )),
    });

    Ok(DeepCleanupResult {
        freed_bytes,
        files_removed,
        failed_count,
        busy_count,
        cancelled,
        rules: results,
        log,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tally(removed: u64, failed: u64, busy: u64, cancelled: bool) -> CleanTally {
        CleanTally {
            freed_bytes: removed * 10,
            removed,
            failed,
            busy,
            dirs_removed: 0,
            cancelled,
        }
    }

    fn result(status: DeepCleanupExecStatus, removed: u64) -> DeepCleanupRuleExecResult {
        DeepCleanupRuleExecResult {
            removed_count: removed,
            ..DeepCleanupRuleExecResult::without_work("x.y", status, None)
        }
    }

    #[test]
    fn exec_status_counts_partial_failures_honestly() {
        use DeepCleanupExecReason as R;
        use DeepCleanupExecStatus as S;
        assert_eq!(exec_status(&tally(5, 0, 0, false)), (S::Completed, None));
        assert_eq!(exec_status(&tally(0, 0, 0, false)), (S::Completed, None));
        assert_eq!(exec_status(&tally(5, 0, 2, false)), (S::Partial, Some(R::ItemsBusy)));
        assert_eq!(exec_status(&tally(5, 1, 2, false)), (S::Partial, Some(R::ItemsFailed)));
        assert_eq!(exec_status(&tally(0, 0, 3, false)), (S::Failed, Some(R::ItemsBusy)));
        assert_eq!(exec_status(&tally(0, 4, 0, false)), (S::Failed, Some(R::ItemsFailed)));
        assert_eq!(exec_status(&tally(2, 0, 0, true)), (S::Partial, Some(R::Cancelled)));
        assert_eq!(exec_status(&tally(0, 0, 0, true)), (S::Skipped, Some(R::Cancelled)));
    }

    #[test]
    fn history_outcome_is_honest() {
        use DeepCleanupExecStatus as S;
        assert_eq!(history_outcome(&[result(S::Completed, 3)], false), HistoryOutcome::Completed);
        assert_eq!(
            history_outcome(&[result(S::Completed, 3), result(S::Blocked, 0)], false),
            HistoryOutcome::CompletedWithWarnings
        );
        assert_eq!(
            history_outcome(&[result(S::Partial, 1)], false),
            HistoryOutcome::CompletedWithWarnings
        );
        assert_eq!(
            history_outcome(&[result(S::Blocked, 0), result(S::Skipped, 0), result(S::Failed, 0)], false),
            HistoryOutcome::Failed
        );
        assert_eq!(history_outcome(&[result(S::Completed, 3)], true), HistoryOutcome::Cancelled);
    }

    #[test]
    fn detail_lines_carry_ids_and_counts_only() {
        let r = DeepCleanupRuleExecResult {
            id: "browser.chromeCache".into(),
            status: DeepCleanupExecStatus::Partial,
            reason: Some(DeepCleanupExecReason::ItemsBusy),
            freed_bytes: 1024,
            removed_count: 12,
            failed_count: 2,
            busy_count: 2,
        };
        let line = detail_line(&r);
        assert_eq!(
            line,
            "browser.chromeCache: partial (itemsBusy) removed=12 failed=2 busy=2 freedBytes=1024"
        );
        assert!(!line.contains('\\'));
    }

    #[test]
    fn duplicate_ids_are_collapsed_in_order() {
        let ids = vec!["a.b".to_string(), "c.d".into(), "a.b".into()];
        assert_eq!(dedupe_ids(&ids), vec!["a.b".to_string(), "c.d".into()]);
    }
}
