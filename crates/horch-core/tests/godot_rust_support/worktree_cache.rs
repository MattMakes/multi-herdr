use std::fs;
use std::path::Path;

fn read_repo_file(rel: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

#[test]
fn grs_15_worktree_timing_report_records_cold_warm_times_and_no_seeding_decision() {
    let report = read_repo_file("ai_docs/reports/godot-rust-worktree-timing.md");
    for evidence in [
        "| Run | check cold | check warm | test cold | test warm |",
        "| mean | 18.34 | 0.04 | 24.98 | 0.05 |",
        "| Profile | Run | cold | warm | seed copy | seeded |",
        "| Profile | cold mean | warm mean | seeded mean + copy | seeded saving | warm bound |",
        "godot --headless --path game --import",
        "Decision: no seeding.",
        "No profile passes the threshold of a saving above 30 percent.",
    ] {
        assert!(
            report.contains(evidence),
            "missing report evidence: {evidence}"
        );
    }
    for profile in ["lossless", "vram", "big"] {
        for run in 1..=3 {
            let row = format!("| {profile} | {run} | ");
            assert!(
                report.lines().any(|line| line.starts_with(&row)),
                "missing import run row: {row}"
            );
        }
    }

    for source in [
        "crates/horch-core/src/taskledger/finish/mod.rs",
        "crates/horch-core/src/vcs/landing.rs",
    ] {
        let code = read_repo_file(source);
        assert!(
            !code.contains(".godot") && !code.contains("CARGO_TARGET_DIR"),
            "{source} must not seed .godot or share CARGO_TARGET_DIR"
        );
    }
}
