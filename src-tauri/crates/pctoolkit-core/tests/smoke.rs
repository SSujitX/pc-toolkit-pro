use std::fs;

use pctoolkit_core::scan_cleanup;

#[test]
fn cleanup_scan_returns_categories() {
    let scan = scan_cleanup().expect("scan should work without admin for listing");
    assert!(!scan.items.is_empty());
    // Default scan categories must not gate on elevation.
    assert!(
        scan.items.iter().all(|item| !item.requires_admin),
        "default cleanup scan categories must not require admin"
    );
}

#[test]
fn history_records_roundtrip() {
    pctoolkit_core::record_history("test", "unit".into(), true, None);
    let items = pctoolkit_core::list_history();
    assert!(!items.is_empty());
}

#[test]
fn temp_estimate_does_not_panic() {
    let _ = fs::read_dir(std::env::temp_dir());
    let _ = scan_cleanup();
}
