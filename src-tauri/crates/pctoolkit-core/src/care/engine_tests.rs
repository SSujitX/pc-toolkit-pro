use super::*;
use std::sync::atomic::AtomicU64;

/// Test double: sleeps `ms` in small steps (honouring stop / skip), then returns `items`.
struct Fake {
    id: CareCategoryId,
    ms: u64,
    items: Vec<CareItem>,
    fail: bool,
    fixed_calls: Arc<AtomicU64>,
}

impl Fake {
    fn new(id: CareCategoryId, ms: u64, items: Vec<CareItem>) -> Self {
        Self { id, ms, items, fail: false, fixed_calls: Arc::new(AtomicU64::new(0)) }
    }
}

impl CareCategory for Fake {
    fn id(&self) -> CareCategoryId {
        self.id
    }

    fn default_weight_ms(&self) -> u64 {
        self.ms.max(1)
    }

    fn scan(&self, ctx: &CareContext) -> CoreResult<CareCategoryScan> {
        let started = Instant::now();
        while started.elapsed() < Duration::from_millis(self.ms) {
            if ctx.should_stop() {
                return Ok(crate::care::skipped(self.id));
            }
            ctx.current(self.id, "C:\\fake\\path");
            std::thread::sleep(Duration::from_millis(5));
        }
        if self.fail {
            return Err(CoreError::OperationFailed("boom".into()));
        }
        Ok(CareCategoryScan::from_items(self.id, self.items.clone()))
    }

    fn fix(&self, item_ids: &[String], _ctx: &CareContext) -> CoreResult<Vec<CareFixResult>> {
        self.fixed_calls.fetch_add(item_ids.len() as u64, Ordering::SeqCst);
        Ok(item_ids
            .iter()
            .map(|id| CareFixResult {
                item_id: id.clone(),
                category: self.id,
                status: CareFixStatus::Fixed,
                reason: None,
                freed_bytes: 10,
                undoable: false,
            })
            .collect())
    }
}

fn item(category: CareCategoryId, id: &str, risk: CareRisk, fixable: bool, needs_admin: bool) -> CareItem {
    CareItem {
        id: id.into(),
        category,
        title_key: None,
        title: Some(id.into()),
        params: Default::default(),
        detail_key: None,
        detail: None,
        location: None,
        bytes: Some(100),
        count: 2,
        risk,
        selected: risk == CareRisk::Safe,
        fixable,
        needs_admin,
        action: None,
    }
}

fn flags(ids: &[CareCategoryId]) -> HashMap<CareCategoryId, Arc<AtomicBool>> {
    ids.iter().map(|id| (*id, Arc::new(AtomicBool::new(false)))).collect()
}

// --- weighting -------------------------------------------------------------------------------

#[test]
fn weighted_percent_uses_expected_duration() {
    // A 9 s category done and a 1 s category queued → 90 %, not 50 %.
    assert!((weighted_percent(&[(9_000.0, 1.0), (1_000.0, 0.0)]) - 90.0).abs() < 1e-9);
    assert_eq!(weighted_percent(&[]), 0.0);
    assert!((weighted_percent(&[(10.0, 1.0), (10.0, 1.0)]) - 100.0).abs() < 1e-9);
    // Fractions are clamped.
    assert!((weighted_percent(&[(10.0, 5.0)]) - 100.0).abs() < 1e-9);
}

#[test]
fn running_fraction_is_capped() {
    assert!((running_fraction(500, 1_000.0) - 0.5).abs() < 1e-9);
    assert_eq!(running_fraction(10_000, 1_000.0), 0.95);
    assert_eq!(running_fraction(0, 0.0), 0.95);
}

#[test]
fn ema_moves_towards_samples() {
    assert_eq!(ema(None, 1_000.0), 1_000.0);
    assert_eq!(ema(Some(f64::NAN), 1_000.0), 1_000.0);
    let next = ema(Some(1_000.0), 2_000.0);
    assert!((next - 1_300.0).abs() < 1e-9);
}

// --- throttling ------------------------------------------------------------------------------

#[test]
fn throttle_limits_non_forced_events() {
    let mut t = Throttle::new(Duration::from_millis(50));
    let t0 = Instant::now();
    assert!(t.ready(t0, false), "first event passes");
    assert!(!t.ready(t0 + Duration::from_millis(10), false));
    assert!(t.ready(t0 + Duration::from_millis(20), true), "forced events always pass");
    assert!(!t.ready(t0 + Duration::from_millis(60), false), "forced event resets the window");
    assert!(t.ready(t0 + Duration::from_millis(75), false));
}

#[test]
fn scan_events_are_throttled() {
    let a = Fake::new(CareCategoryId::Privacy, 300, vec![]);
    let cats: Vec<&dyn CareCategory> = vec![&a];
    let events = AtomicU64::new(0);
    let report = run_scan(&cats, &HashMap::new(), false, Arc::new(AtomicBool::new(false)), &flags(&[a.id]), 2, |_| {
        events.fetch_add(1, Ordering::SeqCst);
    });
    assert_eq!(report.categories[0].state, CareCategoryState::Clean);
    // The fake reports a path every ~5 ms (~60 times); 300 ms at 50 ms spacing allows ~6 + transitions.
    let n = events.load(Ordering::SeqCst);
    assert!(n >= 3 && n <= 14, "unexpected event count {n}");
}

// --- scan orchestration ----------------------------------------------------------------------

#[test]
fn scan_keeps_display_order_and_runs_in_parallel() {
    let slow = Fake::new(CareCategoryId::Privacy, 200, vec![item(CareCategoryId::Privacy, "p", CareRisk::Review, true, false)]);
    let fast = Fake::new(CareCategoryId::Junk, 200, vec![]);
    let info = Fake::new(CareCategoryId::Security, 200, vec![item(CareCategoryId::Security, "s", CareRisk::Info, false, false)]);
    let cats: Vec<&dyn CareCategory> = vec![&slow, &fast, &info];
    let started = Instant::now();
    let last = Mutex::new(None);
    let report = run_scan(&cats, &HashMap::new(), false, Arc::new(AtomicBool::new(false)), &flags(&[slow.id, fast.id, info.id]), 3, |p| {
        *last.lock().unwrap() = Some(p);
    });
    assert!(started.elapsed() < Duration::from_millis(550), "categories must overlap");
    let ids: Vec<_> = report.categories.iter().map(|c| c.category).collect();
    assert_eq!(ids, vec![CareCategoryId::Privacy, CareCategoryId::Junk, CareCategoryId::Security]);
    assert_eq!(report.categories[0].state, CareCategoryState::Issues);
    assert_eq!(report.categories[1].state, CareCategoryState::Clean);
    assert_eq!(report.categories[2].state, CareCategoryState::Attention);
    let last = last.lock().unwrap().clone().unwrap();
    assert_eq!(last.phase, CarePhase::Complete);
    assert_eq!(last.percent, 100.0);
    assert!(last.categories.iter().all(|c| c.status == CareRunStatus::Done));
    assert_eq!(last.categories[0].count, 2, "badge counts underlying entries");
}

#[test]
fn skip_marks_one_category_skipped() {
    let a = Fake::new(CareCategoryId::Privacy, 2_000, vec![]);
    let b = Fake::new(CareCategoryId::Junk, 50, vec![]);
    let cats: Vec<&dyn CareCategory> = vec![&a, &b];
    let skips = flags(&[a.id, b.id]);
    let skip_a = skips[&a.id].clone();
    let report = run_scan(&cats, &HashMap::new(), false, Arc::new(AtomicBool::new(false)), &skips, 2, |p| {
        if p.categories[0].status == CareRunStatus::Running {
            skip_a.store(true, Ordering::SeqCst);
        }
    });
    assert_eq!(report.categories[0].state, CareCategoryState::Skipped);
    assert_eq!(report.categories[1].state, CareCategoryState::Clean);
    assert!(!report.stopped);
}

#[test]
fn stop_skips_running_and_queued_categories() {
    let a = Fake::new(CareCategoryId::Privacy, 2_000, vec![]);
    let b = Fake::new(CareCategoryId::Junk, 2_000, vec![]);
    let c = Fake::new(CareCategoryId::Shortcuts, 2_000, vec![]);
    let cats: Vec<&dyn CareCategory> = vec![&a, &b, &c];
    let stop = Arc::new(AtomicBool::new(false));
    let stop_cb = stop.clone();
    let started = Instant::now();
    let report = run_scan(&cats, &HashMap::new(), false, stop.clone(), &flags(&[a.id, b.id, c.id]), 1, |p| {
        if p.categories.iter().any(|c| c.status == CareRunStatus::Running) {
            stop_cb.store(true, Ordering::SeqCst);
        }
    });
    assert!(started.elapsed() < Duration::from_millis(1_000));
    assert!(report.stopped);
    assert!(report.categories.iter().all(|c| c.state == CareCategoryState::Skipped));
}

#[test]
fn failing_category_is_reported_not_fatal() {
    let mut a = Fake::new(CareCategoryId::Privacy, 10, vec![]);
    a.fail = true;
    let b = Fake::new(CareCategoryId::Junk, 10, vec![]);
    let cats: Vec<&dyn CareCategory> = vec![&a, &b];
    let report = run_scan(&cats, &HashMap::new(), false, Arc::new(AtomicBool::new(false)), &flags(&[a.id, b.id]), 2, |_| {});
    assert_eq!(report.categories[0].state, CareCategoryState::Failed);
    assert_eq!(report.categories[0].reason.as_deref(), Some("failed"));
    assert_eq!(report.categories[1].state, CareCategoryState::Clean);
}

#[test]
fn percent_never_decreases() {
    let a = Fake::new(CareCategoryId::Privacy, 150, vec![]);
    let b = Fake::new(CareCategoryId::Junk, 300, vec![]);
    let cats: Vec<&dyn CareCategory> = vec![&a, &b];
    let seen = Mutex::new(Vec::new());
    run_scan(&cats, &HashMap::new(), false, Arc::new(AtomicBool::new(false)), &flags(&[a.id, b.id]), 2, |p| {
        seen.lock().unwrap().push(p.percent);
    });
    let seen = seen.into_inner().unwrap();
    assert!(seen.windows(2).all(|w| w[1] + 1e-9 >= w[0]), "{seen:?}");
}

// --- fix routing -----------------------------------------------------------------------------

fn report_with(categories: Vec<CareCategoryScan>) -> CareScanReport {
    CareScanReport {
        scan_id: "s".into(),
        categories,
        is_admin: false,
        stopped: false,
        started_at_ms: 0,
        elapsed_ms: 0,
    }
}

fn sample_scan() -> CareScanReport {
    use CareCategoryId::*;
    report_with(vec![
        CareCategoryScan::from_items(
            Privacy,
            vec![
                item(Privacy, "cookies", CareRisk::Review, true, false),
                item(Privacy, "cache", CareRisk::Safe, true, false),
            ],
        ),
        CareCategoryScan::from_items(
            Junk,
            vec![
                item(Junk, "temp", CareRisk::Safe, true, false),
                item(Junk, "wu", CareRisk::Safe, true, true),
            ],
        ),
        CareCategoryScan::from_items(WindowsUpdate, vec![item(WindowsUpdate, "pending", CareRisk::Info, false, false)]),
    ])
}

#[test]
fn auto_fix_takes_only_safe_fixable_allowed_items() {
    let scan = sample_scan();
    let request = CareFixRequest { items: vec![], auto: true, after_action: CareAfterAction::None };
    let plan = plan_fix(&scan, &request, false);
    assert_eq!(
        plan.work,
        vec![(CareCategoryId::Privacy, vec!["cache".to_string()]), (CareCategoryId::Junk, vec!["temp".to_string()])]
    );
    let plan = plan_fix(&scan, &request, true);
    assert_eq!(plan.work[1], (CareCategoryId::Junk, vec!["temp".to_string(), "wu".to_string()]));
    assert!(plan.rejected.is_empty());
}

#[test]
fn manual_fix_routes_ids_and_rejects_unknown() {
    let scan = sample_scan();
    let request = CareFixRequest {
        items: vec![
            CareFixSelection { category: CareCategoryId::Junk, item_ids: vec!["wu".into(), "temp".into(), "temp".into(), "ghost".into()] },
            CareFixSelection { category: CareCategoryId::Privacy, item_ids: vec!["cookies".into()] },
            CareFixSelection { category: CareCategoryId::WindowsUpdate, item_ids: vec!["pending".into()] },
            CareFixSelection { category: CareCategoryId::Registry, item_ids: vec!["x".into()] },
        ],
        auto: false,
        after_action: CareAfterAction::None,
    };
    let plan = plan_fix(&scan, &request, false);
    // Display order (Privacy before Junk), duplicates removed.
    assert_eq!(
        plan.work,
        vec![(CareCategoryId::Privacy, vec!["cookies".to_string()]), (CareCategoryId::Junk, vec!["temp".to_string()])]
    );
    let reasons: Vec<(&str, &str)> =
        plan.rejected.iter().map(|r| (r.item_id.as_str(), r.reason.as_deref().unwrap_or(""))).collect();
    assert!(reasons.contains(&("wu", "needsAdmin")));
    assert!(reasons.contains(&("ghost", "unknownItem")));
    assert!(reasons.contains(&("pending", "notFixable")));
    assert!(reasons.contains(&("x", "unknownItem")));
}

#[test]
fn run_fix_routes_to_owning_category_and_totals() {
    let a = Fake::new(CareCategoryId::Privacy, 0, vec![]);
    let b = Fake::new(CareCategoryId::Junk, 0, vec![]);
    let cats: Vec<&dyn CareCategory> = vec![&a, &b];
    let plan = FixPlan {
        work: vec![(CareCategoryId::Junk, vec!["t1".into(), "t2".into()])],
        rejected: vec![rejected(CareCategoryId::Privacy, "z", "unknownItem")],
    };
    let (results, stopped) = run_fix(&cats, plan, false, Arc::new(AtomicBool::new(false)), &flags(&[b.id]), |_| {});
    assert!(!stopped);
    assert_eq!(a.fixed_calls.load(Ordering::SeqCst), 0);
    assert_eq!(b.fixed_calls.load(Ordering::SeqCst), 2);
    let report = summarize_fix(results, stopped, 1, CareAfterAction::None);
    assert_eq!((report.fixed, report.skipped, report.failed), (2, 1, 0));
    assert_eq!(report.freed_bytes, 20);
}

#[test]
fn stopped_fix_does_not_call_categories() {
    let b = Fake::new(CareCategoryId::Junk, 0, vec![]);
    let cats: Vec<&dyn CareCategory> = vec![&b];
    let plan = FixPlan { work: vec![(CareCategoryId::Junk, vec!["t1".into()])], rejected: vec![] };
    let (results, stopped) = run_fix(&cats, plan, false, Arc::new(AtomicBool::new(true)), &flags(&[b.id]), |_| {});
    assert!(stopped);
    assert_eq!(b.fixed_calls.load(Ordering::SeqCst), 0);
    assert_eq!(results[0].status, CareFixStatus::Skipped);
}

#[test]
fn missing_results_are_filled_in() {
    let out = normalize_results(CareCategoryId::Junk, &["a".into(), "b".into()], vec![]);
    assert_eq!(out.len(), 2);
    assert!(out.iter().all(|r| r.reason.as_deref() == Some("noResult")));
}

#[test]
fn fix_request_rejects_unknown_fields() {
    let ok: CareFixRequest = serde_json::from_str(r#"{"auto":true,"afterAction":"restart"}"#).unwrap();
    assert!(ok.auto);
    assert_eq!(ok.after_action, CareAfterAction::Restart);
    assert!(serde_json::from_str::<CareFixRequest>(r#"{"paths":["C:\\x"]}"#).is_err());
}

/// Real read-only scan of every category on this machine. Prints only states, counts, timings.
/// `cargo test -p pctoolkit-core -- --ignored care_scan_smoke --nocapture`
#[test]
#[ignore]
fn care_scan_smoke_on_this_machine() {
    let started = Instant::now();
    let report = scan_care(&CareCategoryId::ALL, |_| {}).expect("care scan");
    for c in &report.categories {
        println!(
            "{:<14} {:<14} items={:<4} count={:<7} bytes={:<12} elapsedMs={:<6} reason={:?}",
            c.category.as_str(),
            state_code(c.state),
            c.items.len(),
            issue_count(c),
            c.items.iter().filter_map(|i| i.bytes).sum::<u64>(),
            c.elapsed_ms,
            c.reason
        );
    }
    println!("isAdmin={} totalMs={}", report.is_admin, started.elapsed().as_millis());
}
