//! Workspace smoke test: proves the dasho-core harness runs in CI.

#[test]
fn engine_reports_a_version() {
    let v = dasho_core::VERSION;
    assert!(!v.is_empty());
    assert!(v.contains('.'), "expected semver-shaped version, got {v}");
}
