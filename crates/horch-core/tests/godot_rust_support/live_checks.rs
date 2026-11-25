use std::fs;
use std::path::Path;

fn read_repo_file(rel: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

#[test]
fn grs_18_godot_codex_live_check_is_recorded_and_spec_status_closed() {
    let live_check = read_repo_file("docs/live-checks/godot-codex.md");
    for evidence in [
        "## Codex pane rerun (2026-11-25)",
        "`CODEX_SANDBOX=seatbelt`",
        "`scripts/live/godot-codex.sh`",
        "`godot-run.sh --import`",
        "`PARSE_CHECK checked=4 failed=0`",
        "`HEALTH_TEST passed=7 failed=0`",
        "`GODOT_CODEX_MAIN_READY`",
    ] {
        assert!(
            live_check.contains(evidence),
            "missing live evidence: {evidence}"
        );
    }

    let godot_spec = read_repo_file("docs/specs/godot.md");
    let status = godot_spec
        .lines()
        .find(|line| line.starts_with("| Status |"))
        .expect("Godot spec status row");
    assert!(status.contains("Codex `godot-run.sh` live check is complete"));
    assert!(!status.contains("pending"));
}
