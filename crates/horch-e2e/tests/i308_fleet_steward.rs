//! `horch fleet` starts the worktree steward beside the orchestrator when
//! the project has parked worktrees, and no steward when it has none (#308).
//! The real `horch` binary on the fake herdr.

#![cfg(unix)]

use std::path::PathBuf;

use horch_e2e::harness::{fixtures, Harness};

fn text(o: &std::process::Output) -> String {
    format!(
        "status: {}\nstdout: {}\nstderr: {}",
        o.status,
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

fn harness(name: &str) -> Harness {
    let mut h = Harness::new(name).with_git();
    h.set(
        "HORCH_LEDGER_DB",
        h.root.join("ledger.sqlite3").to_string_lossy(),
    );
    h.set("HORCH_FAULT", "delivery-probe-ms:100");
    h.set("HORCH_NO_WAKE", "1").set("HORCH_NO_WATCHDOG", "1");
    h.set("HORCH_SPAWN_WAIT_MS", "100");
    h.set(
        "HORCH_QUOTA_FILE",
        fixtures().join("quota/all-ok.json").to_string_lossy(),
    );
    h.set("HORCH_FAKE_SCENARIO", "");
    h
}

fn ok(h: &Harness, args: &[&str]) -> String {
    let o = h.run(args);
    assert!(o.status.success(), "horch {}: {}", args.join(" "), text(&o));
    String::from_utf8_lossy(&o.stdout).into_owned()
}

/// A run whose task T-1 was cancelled with its worktree on disk: the
/// worktree is `abandoned`.
fn abandoned_worktree(h: &Harness) -> PathBuf {
    ok(h, &["ledger", "run", "new", "alpha"]);
    let t = h.project.join("tickets").join("01-f.md");
    std::fs::create_dir_all(t.parent().unwrap()).unwrap();
    std::fs::write(&t, "# 01: Feature\n").unwrap();
    ok(
        h,
        &[
            "ledger",
            "add",
            "--file",
            &t.to_string_lossy(),
            "--primary",
            "opus",
        ],
    );
    let w = ok(h, &["ledger", "worker", "new", "--persona", "opus"])
        .trim()
        .to_string();
    let as_worker = |args: &[&str]| {
        let mut cmd = h.horch(args);
        cmd.env("HORCH_WORKER_ID", &w);
        let o = cmd.output().unwrap();
        assert!(o.status.success(), "horch {}: {}", args.join(" "), text(&o));
        String::from_utf8_lossy(&o.stdout).into_owned()
    };
    as_worker(&["ledger", "claim"]);
    let said = as_worker(&["ledger", "worktree", "new", "f"]);
    ok(h, &["ledger", "cancel", "T-1", "not needed now"]);
    PathBuf::from(said.lines().find_map(|l| l.strip_prefix("path: ")).unwrap())
}

/// The command lines the fake herdr was asked to run in a pane of the fleet
/// workspace `w1`.
fn launches(h: &Harness) -> Vec<String> {
    h.calls_of("herdr")
        .iter()
        .filter_map(|c| {
            let argv: Vec<String> = serde_json::from_value(c["argv"].clone()).ok()?;
            let line = argv.join(" ");
            (argv.first().map(String::as_str) == Some("pane")
                && argv.get(1).map(String::as_str) == Some("run")
                && argv.get(2).is_some_and(|p| p.starts_with("w1:")))
            .then_some(line)
        })
        .collect()
}

fn fleet(h: &Harness) -> String {
    let cwd = h.project.to_string_lossy().into_owned();
    let o = h.run(&["fleet", "opus", "--no-update", "--cwd", &cwd]);
    assert!(o.status.success(), "{}", text(&o));
    String::from_utf8_lossy(&o.stdout).into_owned()
}

#[test]
fn a_fleet_with_an_abandoned_worktree_spawns_the_worktree_steward() {
    let h = harness("i308-steward");
    let path = abandoned_worktree(&h);
    assert!(path.exists());
    let said = fleet(&h);
    assert!(
        said.contains("1 parked worktrees: starting the worktree steward"),
        "{said}"
    );
    let runs = launches(&h);
    assert_eq!(runs.len(), 2, "the orchestrator and the steward: {runs:#?}");
    assert_eq!(
        runs.iter()
            .filter(|l| l.contains("worktree-steward"))
            .count(),
        1,
        "{runs:#?}"
    );
}

#[test]
fn a_fleet_with_no_parked_worktree_spawns_no_steward() {
    let h = harness("i308-no-steward");
    let said = fleet(&h);
    assert!(!said.contains("worktree steward"), "{said}");
    let runs = launches(&h);
    assert_eq!(runs.len(), 1, "only the orchestrator: {runs:#?}");
    assert!(!runs[0].contains("worktree-steward"), "{runs:#?}");
}
