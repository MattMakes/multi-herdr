//! The gate of the list of expected failures (design amendment A22): each
//! list file has a test file, and each entry names a `#[test]` that runs its
//! body through `horch_e2e::expected::check`.

use horch_e2e::expected::audit;
use horch_e2e::harness::repo_root;

#[test]
fn expected_failures_list_gates_the_run() {
    let crate_dir = repo_root().join("crates/horch-e2e");
    let faults = audit(
        &crate_dir.join("expected-failures"),
        &crate_dir.join("tests"),
    );
    assert!(
        faults.is_empty(),
        "the list of expected failures has faults:\n{}",
        faults.join("\n")
    );
}
