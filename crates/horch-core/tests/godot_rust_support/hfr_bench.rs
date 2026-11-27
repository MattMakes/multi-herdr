use std::path::{Path, PathBuf};
use std::process::Command;

use horch_core::roster::{ProjectFacts, Roster};

const BENCH: &str = "skills/godot-hfr-hunt/scripts/hfr_bench.py";

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(rel: &str) -> String {
    std::fs::read_to_string(repo().join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"))
}

fn run(program: &str, args: &[&str]) -> (bool, String) {
    let out = Command::new(program)
        .current_dir(repo())
        .args(args)
        .env_remove("GODOT_PATH")
        .output()
        .unwrap_or_else(|e| panic!("{program} runs: {e}"));
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    (out.status.success(), text)
}

#[test]
fn grs_20_hfr_bench_python_suite_passes() {
    let (ok, out) = run(
        "python3",
        &[
            "-m",
            "unittest",
            "discover",
            "-s",
            "scripts/godot/tests",
            "-p",
            "test_hfr_bench.py",
            "-v",
        ],
    );
    assert!(ok, "{out}");
    for test in [
        "test_hfr_bench_summarize_uses_nearest_rank_percentiles",
        "test_hfr_bench_compare_flags_regression_on_four_of_five_pairs",
        "test_hfr_bench_compare_ignores_delta_below_noise_floor",
        "test_hfr_bench_compare_reports_budget_failure_without_regression",
        "test_hfr_bench_compare_is_infrastructure_invalid_below_three_pairs",
        "test_hfr_bench_runs_fake_export_without_godot",
    ] {
        assert!(
            out.contains(&format!("{test} ")),
            "{test} did not run: {out}"
        );
    }
    assert!(out.contains("Ran 6 tests"), "{out}");
}

#[test]
fn grs_43_performance_engineer_expects_hfr_hunt_within_eleven_skill_cap() {
    let roster =
        Roster::load_layered(None, None, Some(repo().join("teammates").to_str().unwrap())).unwrap();
    let t = roster.get("godot-performance-engineer").unwrap();
    assert!(t.skills.iter().any(|s| s == "godot-hfr-hunt"));
    assert!(!t.skills.iter().any(|s| s == "godot-assets-pipeline"));
    assert!(t
        .available_skills
        .iter()
        .any(|s| s == "godot-assets-pipeline"));
    assert_eq!(t.skills.len(), 9, "{:?}", t.skills);

    for (facts, expected) in [
        (vec!["project.godot", "player.gd"], 9),
        (vec!["project.godot", "Game.csproj", "Player.cs"], 11),
    ] {
        let launch = roster
            .clone()
            .with_project_facts(ProjectFacts::from_names(facts.clone()))
            .for_launch(t.clone());
        assert_eq!(
            launch.skills.len(),
            expected,
            "{facts:?}: {:?}",
            launch.skills
        );
        assert!(launch.skills.len() <= 11);
    }

    for seat in ["godot-release-engineer", "godot-technical-artist"] {
        let other = roster.get(seat).unwrap();
        assert!(
            other.skills.iter().any(|s| s == "godot-assets-pipeline"),
            "{seat}"
        );
    }
}

#[test]
fn grs_44_hfr_hunt_is_registered_exempt_and_executable() {
    let copied: serde_json::Value = serde_json::from_str(&read("skills/copied.json")).unwrap();
    let entry = copied["skills"]
        .as_array()
        .expect("copied.json lists skills")
        .iter()
        .find(|e| e["name"] == "godot-hfr-hunt")
        .expect("godot-hfr-hunt is in skills/copied.json");
    assert_eq!(entry["copied_files"], serde_json::json!([]));
    assert_eq!(entry["verbatim"], serde_json::json!(false));

    let catalog = read("crates/horch-core/tests/skills_catalog.rs");
    let list = |name: &str| {
        let from = catalog
            .find(&format!("const {name}: &[&str] = &["))
            .unwrap();
        let rest = &catalog[from..];
        rest[..rest.find("];").unwrap()].to_string()
    };
    assert!(list("REPO_ORIGINAL").contains("\"godot-hfr-hunt\","));
    assert!(list("EXEMPT_FILES").contains("\"godot-hfr-hunt/scripts/hfr_bench.py\","));
    assert!(read("skills/README.md")
        .contains("| [godot-hfr-hunt](godot-hfr-hunt/SKILL.md) | own text |"));

    use std::os::unix::fs::PermissionsExt;
    let mode = std::fs::metadata(repo().join(BENCH))
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o755);
    assert!(read(BENCH).starts_with("#!/usr/bin/env python3\n"));

    let (ok, out) = run(
        "python3",
        &["scripts/comments/comment_lint.py", "list", BENCH],
    );
    assert!(ok, "{out}");
    assert_eq!(out.trim(), "", "hfr_bench.py holds comments: {out}");

    let (ok, out) = run("python3", &[BENCH, "compare", "--help"]);
    assert!(ok, "{out}");
    assert!(out.contains("--target-fps"), "{out}");
}
