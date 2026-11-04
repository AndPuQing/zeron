//! Run the interface's Luau regression suite with the pinned renderer's
//! headless host, so it is covered by the same `cargo test` as native code.
use std::path::PathBuf;

#[test]
fn luau_interface_regressions() {
    let ui = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("ui");
    let filter = std::env::var("ZERUN_ZGX_TEST_FILTER").ok();
    let report = zgx_script::test_runner::run(&ui, filter.as_deref(), |_| {})
        .expect("the interface test suite must load");
    assert!(!report.outcomes.is_empty(), "no interface tests matched");
    let failures: Vec<_> = report.outcomes.iter().filter_map(|o| {
        o.failure.as_ref().map(|why| format!("{}: {}: {why}",o.file.display(),o.name))
    }).collect();
    eprintln!("Luau interface: {} tests, {} failures", report.outcomes.len(), failures.len());
    assert!(failures.is_empty(), "{}",failures.join("\n\n"));
}
