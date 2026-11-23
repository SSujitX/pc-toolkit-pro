//! Parallel, read-only scan of every catalog rule and provider.

use std::collections::{BTreeSet, HashSet};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime};

use super::catalog::{catalog, CatalogRule, RuleKind};
use super::provider::{registered_providers, DeepCleanupProvider, ProviderContext};
use super::resolve::{ownership_exclusions, resolve_rule, BaseDirs, ResolvedRule};
use super::safety::SafetyContext;
use super::special::{measure_special, SpecialAvailability, SpecialContext};
use super::walk::{measure_root, MeasureTally, WalkSpec, MAX_ENTRIES_PER_RULE};
use super::{
    DeepCleanupGroup, DeepCleanupProgress, DeepCleanupRuleExtras, DeepCleanupRuleResult,
    DeepCleanupRuleStatus, DeepCleanupScan,
};
use crate::cleaner::{check_cancel, CANCELLED};
use crate::shared::{CoreError, CoreResult};

const MAX_WORKERS: usize = 4;
const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);
/// Worker-side batching: send a progress delta at most every N files.
const TICK_FILES: u64 = 256;

/// Lowercase image names of `names` that are running now. A failed probe is
/// treated as "none running"; execution still counts locked files honestly.
pub(super) fn running_process_set(names: &BTreeSet<String>) -> HashSet<String> {
    if names.is_empty() {
        return HashSet::new();
    }
    let list: Vec<String> = names.iter().cloned().collect();
    pctoolkit_platform::probe_running_processes(&list)
        .map(|groups| {
            groups
                .into_iter()
                .filter(|g| g.process_count > 0)
                .map(|g| g.image_name.to_lowercase())
                .collect()
        })
        .unwrap_or_default()
}

pub(super) fn catalog_or_error() -> CoreResult<&'static [CatalogRule]> {
    catalog().map_err(|issues| {
        CoreError::OperationFailed(format!("deep cleanup catalog invalid ({} issues)", issues.len()))
    })
}

/// Pure status decision for one scanned rule.
pub(super) fn scan_status(applicable: bool, files: u64, running: bool, limited: bool) -> DeepCleanupRuleStatus {
    if !applicable {
        DeepCleanupRuleStatus::NotApplicable
    } else if files == 0 {
        DeepCleanupRuleStatus::Clean
    } else if running {
        DeepCleanupRuleStatus::RequiresClose
    } else if limited {
        DeepCleanupRuleStatus::Limited
    } else {
        DeepCleanupRuleStatus::Found
    }
}

pub(super) fn running_for(rule: &CatalogRule, running: &HashSet<String>) -> Vec<String> {
    rule.spec
        .processes
        .iter()
        .filter(|p| running.contains(&p.to_lowercase()))
        .cloned()
        .collect()
}

/// Smart selection: recommended, has data, and will not be skipped for elevation.
pub(super) fn initial_selection(recommended: bool, has_data: bool, needs_admin: bool) -> bool {
    recommended && has_data && !needs_admin
}

fn rule_result(
    rule: &CatalogRule,
    tally: MeasureTally,
    applicable: bool,
    refused: u32,
    running: Vec<String>,
    is_admin: bool,
) -> DeepCleanupRuleResult {
    let status = scan_status(applicable, tally.files, !running.is_empty(), tally.limited);
    let has_data = tally.bytes > 0
        && matches!(
            status,
            DeepCleanupRuleStatus::Found | DeepCleanupRuleStatus::Limited | DeepCleanupRuleStatus::RequiresClose
        );
    let needs_admin = rule.spec.requires_elevation && !is_admin;
    DeepCleanupRuleResult {
        id: rule.spec.id.clone(),
        group: rule.spec.group,
        name_key: rule.spec.name_key.clone(),
        detail_key: rule.spec.detail_key.clone(),
        risk: rule.spec.risk,
        bytes: if has_data { tally.bytes } else { 0 },
        item_count: if has_data { tally.files } else { 0 },
        recommended: rule.spec.recommended,
        selected: initial_selection(rule.spec.recommended, has_data, needs_admin),
        status,
        related_processes: rule.spec.processes.clone(),
        running_processes: running,
        requires_elevation: rule.spec.requires_elevation,
        needs_admin,
        limited: tally.limited,
        refused_roots: refused,
        extras: DeepCleanupRuleExtras::default(),
    }
}

enum Msg {
    Tick { rule_id: String, path: String, items: u64, bytes: u64 },
    Done { job: usize, results: CoreResult<Vec<DeepCleanupRuleResult>> },
}

#[derive(Clone, Copy)]
enum Job {
    Catalog(usize),
    Provider(usize),
}

struct ScanEnv<'a> {
    rules: &'a [CatalogRule],
    resolved: &'a [ResolvedRule],
    excluded: &'a [HashSet<String>],
    providers: &'a [&'static dyn DeepCleanupProvider],
    running: &'a HashSet<String>,
    safety: &'a SafetyContext,
    is_admin: bool,
    now: SystemTime,
}

fn scan_catalog_rule(env: &ScanEnv<'_>, index: usize, tx: &mpsc::Sender<Msg>) -> DeepCleanupRuleResult {
    let rule = &env.rules[index];
    let running = running_for(rule, env.running);
    match rule.kind {
        RuleKind::Special(kind) => {
            let ctx = SpecialContext { is_admin: env.is_admin };
            let measured = measure_special(kind, &ctx);
            let tally = MeasureTally {
                bytes: measured.bytes,
                files: measured.items,
                ..MeasureTally::default()
            };
            let applicable = !matches!(measured.availability, SpecialAvailability::NotApplicable(_));
            let mut result = rule_result(rule, tally, applicable, 0, running, env.is_admin);
            result.extras.artifacts = measured.artifacts;
            match measured.availability {
                SpecialAvailability::Measured => {}
                SpecialAvailability::Unmeasured(reason) => {
                    result.status = DeepCleanupRuleStatus::Unmeasured;
                    result.selected = false;
                    result.extras.reason = Some(reason);
                }
                SpecialAvailability::NotApplicable(reason) => result.extras.reason = reason,
            }
            result
        }
        RuleKind::Files => {
            let resolved = &env.resolved[index];
            let spec = WalkSpec {
                matcher: &rule.spec.matcher,
                excluded: &env.excluded[index],
            };
            let mut total = MeasureTally::default();
            let mut pending_items = 0u64;
            let mut pending_bytes = 0u64;
            for root in &resolved.roots {
                if total.cancelled || total.limited {
                    break;
                }
                let budget = MAX_ENTRIES_PER_RULE.saturating_sub(total.entries);
                let tally = measure_root(root, &spec, budget, env.now, &mut |path: &Path, len| {
                    pending_items += 1;
                    pending_bytes += len;
                    if pending_items >= TICK_FILES {
                        let _ = tx.send(Msg::Tick {
                            rule_id: rule.spec.id.clone(),
                            path: path.display().to_string(),
                            items: pending_items,
                            bytes: pending_bytes,
                        });
                        pending_items = 0;
                        pending_bytes = 0;
                    }
                });
                total.absorb(tally);
            }
            if pending_items > 0 {
                let _ = tx.send(Msg::Tick {
                    rule_id: rule.spec.id.clone(),
                    path: String::new(),
                    items: pending_items,
                    bytes: pending_bytes,
                });
            }
            rule_result(rule, total, !resolved.roots.is_empty(), resolved.refused, running, env.is_admin)
        }
    }
}

fn run_job(env: &ScanEnv<'_>, job: Job, tx: &mpsc::Sender<Msg>) -> CoreResult<Vec<DeepCleanupRuleResult>> {
    match job {
        Job::Catalog(i) => Ok(vec![scan_catalog_rule(env, i, tx)]),
        Job::Provider(i) => {
            let progress = |rule_id: &str, path: &Path, items: u64, bytes: u64| {
                let _ = tx.send(Msg::Tick {
                    rule_id: rule_id.to_string(),
                    path: path.display().to_string(),
                    items,
                    bytes,
                });
            };
            let ctx = ProviderContext::new(env.is_admin, env.running, env.safety, &progress);
            env.providers[i].scan(&ctx)
        }
    }
}

/// Groups that can hold rules (catalog + providers), in display order.
pub(super) fn available_groups(
    rules: &[CatalogRule],
    providers: &[&'static dyn DeepCleanupProvider],
) -> Vec<DeepCleanupGroup> {
    let mut groups: BTreeSet<DeepCleanupGroup> = rules.iter().map(|r| r.spec.group).collect();
    for provider in providers {
        groups.extend(provider.groups().iter().copied());
    }
    groups.into_iter().collect()
}

pub(super) fn run<F>(on_progress: F) -> CoreResult<DeepCleanupScan>
where
    F: FnMut(DeepCleanupProgress),
{
    run_filtered(None, on_progress)
}

/// Scan catalog rules whose group is in `only_groups` (providers are skipped), or
/// everything when `only_groups` is `None`. Ownership exclusions are still computed
/// over the whole catalog so a filtered scan measures exactly what a full scan would.
pub(super) fn run_filtered<F>(only_groups: Option<&[DeepCleanupGroup]>, mut on_progress: F) -> CoreResult<DeepCleanupScan>
where
    F: FnMut(DeepCleanupProgress),
{
    let started = Instant::now();
    let rules = catalog_or_error()?;
    let in_scope = |rule: &CatalogRule| only_groups.is_none_or(|groups| groups.contains(&rule.spec.group));
    let providers = if only_groups.is_some() { Vec::new() } else { registered_providers() };
    let is_admin = pctoolkit_platform::is_user_admin();
    let safety = SafetyContext::from_env();
    let bases = BaseDirs::from_env();

    let mut process_names: BTreeSet<String> = rules
        .iter()
        .filter(|r| in_scope(r))
        .flat_map(|r| r.spec.processes.iter().cloned())
        .collect();
    for provider in &providers {
        process_names.extend(provider.processes());
    }
    let running = running_process_set(&process_names);

    on_progress(DeepCleanupProgress {
        phase: "scanning".into(),
        current_path: String::new(),
        items_scanned: 0,
        bytes_scanned: 0,
        elapsed_ms: started.elapsed().as_millis() as u64,
        message: "deepCleaner.preparing".into(),
        current: 0,
        total: 1,
        rule_id: None,
    });

    let resolved: Vec<ResolvedRule> = rules.iter().map(|r| resolve_rule(r, &bases, &safety)).collect();
    let excluded = ownership_exclusions(&resolved);
    check_cancel()?;

    let jobs: Vec<Job> = (0..rules.len())
        .filter(|&i| in_scope(&rules[i]))
        .map(Job::Catalog)
        .chain((0..providers.len()).map(Job::Provider))
        .collect();
    let total = jobs.len() as u64;
    let env = ScanEnv {
        rules,
        resolved: &resolved,
        excluded: &excluded,
        providers: &providers,
        running: &running,
        safety: &safety,
        is_admin,
        now: SystemTime::now(),
    };
    let workers = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .clamp(1, MAX_WORKERS);
    let next = AtomicUsize::new(0);
    let mut outputs: Vec<Option<Vec<DeepCleanupRuleResult>>> = vec![None; jobs.len()];
    let mut first_error: Option<CoreError> = None;

    std::thread::scope(|scope| {
        let (tx, rx) = mpsc::channel::<Msg>();
        for _ in 0..workers {
            let tx = tx.clone();
            let env = &env;
            let jobs = &jobs;
            let next = &next;
            scope.spawn(move || loop {
                if CANCELLED.load(Ordering::Relaxed) {
                    break;
                }
                let i = next.fetch_add(1, Ordering::SeqCst);
                let Some(job) = jobs.get(i).copied() else {
                    break;
                };
                let results = run_job(env, job, &tx);
                if tx.send(Msg::Done { job: i, results }).is_err() {
                    break;
                }
            });
        }
        drop(tx);

        let mut items = 0u64;
        let mut bytes = 0u64;
        let mut done = 0u64;
        let mut last_path = String::new();
        let mut last_rule: Option<String> = None;
        let mut last_emit: Option<Instant> = None;
        loop {
            let mut force = false;
            match rx.recv_timeout(PROGRESS_INTERVAL) {
                Ok(Msg::Tick { rule_id, path, items: i, bytes: b }) => {
                    items += i;
                    bytes += b;
                    if !path.is_empty() {
                        last_path = path;
                    }
                    last_rule = Some(rule_id);
                }
                Ok(Msg::Done { job, results }) => {
                    done += 1;
                    force = true;
                    match results {
                        Ok(list) => outputs[job] = Some(list),
                        Err(CoreError::OperationCancelled) => {}
                        // A failing provider must not sink the whole scan.
                        Err(e) => {
                            if matches!(jobs[job], Job::Catalog(_)) && first_error.is_none() {
                                first_error = Some(e);
                            }
                        }
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
            if force || last_emit.is_none_or(|t| t.elapsed() >= PROGRESS_INTERVAL) {
                last_emit = Some(Instant::now());
                let message = last_rule
                    .as_deref()
                    .and_then(|id| rules.iter().find(|r| r.spec.id == id))
                    .map(|r| r.spec.name_key.clone())
                    .unwrap_or_else(|| "deepCleaner.preparing".into());
                on_progress(DeepCleanupProgress {
                    phase: "scanning".into(),
                    current_path: last_path.clone(),
                    items_scanned: items,
                    bytes_scanned: bytes,
                    elapsed_ms: started.elapsed().as_millis() as u64,
                    message,
                    current: done,
                    total,
                    rule_id: last_rule.clone(),
                });
            }
        }
    });

    check_cancel()?;
    if let Some(error) = first_error {
        return Err(error);
    }

    let rules_out: Vec<DeepCleanupRuleResult> = outputs.into_iter().flatten().flatten().collect();
    let (items, bytes) = rules_out
        .iter()
        .fold((0u64, 0u64), |(i, b), r| (i + r.item_count, b + r.bytes));
    on_progress(DeepCleanupProgress {
        phase: "complete".into(),
        current_path: String::new(),
        items_scanned: items,
        bytes_scanned: bytes,
        elapsed_ms: started.elapsed().as_millis() as u64,
        message: "complete".into(),
        current: total,
        total,
        rule_id: None,
    });
    Ok(DeepCleanupScan {
        groups: available_groups(rules, &providers),
        rules: rules_out,
        is_admin,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smart_selection_skips_items_that_need_admin() {
        assert!(initial_selection(true, true, false));
        assert!(!initial_selection(true, true, true));
        assert!(!initial_selection(true, false, false));
        assert!(!initial_selection(false, true, false));
    }

    #[test]
    fn scan_status_precedence() {
        use DeepCleanupRuleStatus::*;
        assert_eq!(scan_status(false, 10, true, true), NotApplicable);
        assert_eq!(scan_status(true, 0, true, false), Clean);
        assert_eq!(scan_status(true, 3, true, true), RequiresClose);
        assert_eq!(scan_status(true, 3, false, true), Limited);
        assert_eq!(scan_status(true, 3, false, false), Found);
    }

    /// Read-only scan of this machine. `cargo test -p pctoolkit-core -- --ignored scan_smoke`
    #[test]
    #[ignore]
    fn scan_smoke_on_this_machine() {
        let started = Instant::now();
        let mut events = 0u32;
        let scan = run(|_| events += 1).expect("scan");
        assert!(events >= 2);
        let mut projects = 0usize;
        let mut ai = 0usize;
        for rule in &scan.rules {
            match rule.group {
                // Item-level results: counted only, no names or paths in the log.
                DeepCleanupGroup::Projects => projects += 1,
                DeepCleanupGroup::Ai if rule.extras.display_name.is_some() => ai += 1,
                _ => println!(
                    "{:<34} {:?} files={} bytes={} refused={} limited={} running={:?} reason={:?} artifacts={}",
                    rule.id,
                    rule.status,
                    rule.item_count,
                    rule.bytes,
                    rule.refused_roots,
                    rule.limited,
                    rule.running_processes,
                    rule.extras.reason,
                    rule.extras.artifacts.len()
                ),
            }
        }
        println!(
            "projects={projects} aiModels={ai} rules={} isAdmin={} totalMs={}",
            scan.rules.len(),
            scan.is_admin,
            started.elapsed().as_millis()
        );
    }

    #[test]
    fn groups_are_data_driven() {
        let rules = catalog().expect("valid catalog");
        let groups = available_groups(rules, &[]);
        assert!(groups.contains(&DeepCleanupGroup::System));
        assert!(groups.contains(&DeepCleanupGroup::Development));
        // Projects come only from the projects provider.
        assert!(!groups.contains(&DeepCleanupGroup::Projects));
        let with_providers = available_groups(rules, &registered_providers());
        assert!(with_providers.contains(&DeepCleanupGroup::Projects));
        assert!(with_providers.contains(&DeepCleanupGroup::Ai));
        let mut sorted = groups.clone();
        sorted.sort();
        assert_eq!(groups, sorted, "display order follows the enum order");
    }
}
