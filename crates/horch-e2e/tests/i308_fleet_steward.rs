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

fn as_worker(h: &Harness, worker: &str, args: &[&str]) -> std::process::Output {
    h.horch(args)
        .env("HORCH_WORKER_ID", worker)
        .output()
        .unwrap()
}

fn worker_ok(h: &Harness, worker: &str, args: &[&str]) -> String {
    let o = as_worker(h, worker, args);
    assert!(o.status.success(), "horch {}: {}", args.join(" "), text(&o));
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn git(h: &Harness, dir: &std::path::Path, args: &[&str]) -> String {
    let o = h.git_cmd(args).current_dir(dir).output().unwrap();
    assert!(o.status.success(), "git {}: {}", args.join(" "), text(&o));
    String::from_utf8_lossy(&o.stdout).trim().to_string()
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
    worker_ok(h, &w, &["ledger", "claim"]);
    let said = worker_ok(h, &w, &["ledger", "worktree", "new", "f"]);
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

/// Fix B (T-5): the codex sandbox cannot write the ledger, so the steward
/// has no codex fallback. An exhausted claude pool starts no steward, and
/// `horch fleet` says why.
#[test]
fn an_exhausted_claude_pool_starts_no_codex_steward() {
    let mut h = harness("i308-no-codex-steward");
    h.set(
        "HORCH_QUOTA_FILE",
        fixtures()
            .join("quota/claude-exhausted-codex-ok.json")
            .to_string_lossy(),
    );
    // The fixture's quota is current at this time.
    h.set("HORCH_NOW", "2026-09-28T18:00:00Z");
    abandoned_worktree(&h);
    let cwd = h.project.to_string_lossy().into_owned();
    let o = h.run(&["fleet", "opus", "--no-update", "--cwd", &cwd]);
    assert!(o.status.success(), "{}", text(&o));
    let said = String::from_utf8_lossy(&o.stdout);
    assert!(
        said.contains("REFUSED: worktree-steward cannot start. claude exhausted"),
        "{said}"
    );
    assert!(
        said.contains("the worktree steward did not start: spawn refused over usage limits"),
        "{said}"
    );
    let runs = launches(&h);
    assert_eq!(runs.len(), 1, "only the orchestrator: {runs:#?}");
    assert!(!runs[0].contains("worktree-steward"), "{runs:#?}");
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

/// Fix B keeps the read-only listing (T-5): a caller that cannot write the
/// ledger still lists the parked rows. The full listing skips its bytes cache.
#[test]
fn triage_lists_rows_from_a_ledger_it_cannot_write() {
    use std::os::unix::fs::PermissionsExt;
    let h = harness("i308-read-only-triage");
    let path = abandoned_worktree(&h);
    let db = h.root.join("ledger.sqlite3");
    std::fs::set_permissions(&db, std::fs::Permissions::from_mode(0o444)).unwrap();
    let o = h.run(&["worktree", "triage", "--json"]);
    std::fs::set_permissions(&db, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert!(o.status.success(), "{}", text(&o));
    let listed: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    let rows = listed["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 1, "{listed:#}");
    assert_eq!(rows[0]["path"], path.to_string_lossy().as_ref());
}

#[test]
fn triage_delete_accepts_a_clean_detached_head_reachable_from_a_branch() {
    let h = harness("i308-detached-reachable");
    let path = abandoned_worktree(&h);
    git(&h, &path, &["checkout", "--detach"]);
    let listed: serde_json::Value =
        serde_json::from_str(&ok(&h, &["worktree", "triage", "--json"])).unwrap();
    let row = &listed["rows"].as_array().unwrap()[0];
    let id = row["id"].as_str().unwrap();
    let name = row["name"].as_str().unwrap();
    let reply = h.root.join("reply.md");
    std::fs::write(
        &reply,
        format!(
            "| ID | Worktree | Suggested | Decision | Direction |\n|----|----------|-----------|----------|-----------|\n| {id} | {name} | DELETE | DELETE | |\n"
        ),
    )
    .unwrap();
    ok(
        &h,
        &["worktree", "triage", "apply", &reply.to_string_lossy()],
    );
    assert!(!path.exists());
}

#[test]
fn triage_delete_refuses_a_detached_unique_commit() {
    let h = harness("i308-detached-unique");
    let path = abandoned_worktree(&h);
    git(&h, &path, &["checkout", "--detach"]);
    std::fs::write(path.join("unique.txt"), "unique\n").unwrap();
    git(&h, &path, &["add", "unique.txt"]);
    git(&h, &path, &["commit", "-q", "-m", "unique detached"]);
    let listed: serde_json::Value =
        serde_json::from_str(&ok(&h, &["worktree", "triage", "--json"])).unwrap();
    let row = &listed["rows"].as_array().unwrap()[0];
    let reply = h.root.join("reply.md");
    std::fs::write(
        &reply,
        format!(
            "| ID | Worktree | Suggested | Decision | Direction |\n|----|----------|-----------|----------|-----------|\n| {} | {} | DELETE | DELETE | |\n",
            row["id"].as_str().unwrap(),
            row["name"].as_str().unwrap()
        ),
    )
    .unwrap();
    let o = h.run(&["worktree", "triage", "apply", &reply.to_string_lossy()]);
    assert!(!o.status.success(), "{}", text(&o));
    assert!(
        text(&o).contains("detached at a unique commit"),
        "{}",
        text(&o)
    );
    assert!(path.exists());
}

/// Bug 3: a steward in its solo run drops a clean scratch worktree of
/// another run when it names that run with `--run`.
#[test]
fn a_worker_can_drop_a_clean_worktree_from_an_explicit_other_run() {
    let h = harness("i308-cross-run-drop");
    ok(&h, &["ledger", "run", "new", "alpha"]);
    let owner = ok(&h, &["ledger", "worker", "new", "--persona", "opus"])
        .trim()
        .to_string();
    let said = worker_ok(&h, &owner, &["ledger", "worktree", "scratch", "bisect"]);
    let path = PathBuf::from(said.lines().find_map(|l| l.strip_prefix("path: ")).unwrap());
    assert!(path.exists(), "{said}");
    ok(&h, &["ledger", "run", "new", "beta"]);
    let steward = ok(
        &h,
        &[
            "ledger",
            "worker",
            "new",
            "--persona",
            "opus",
            "--run",
            "beta",
        ],
    )
    .trim()
    .to_string();
    let drop = [
        "ledger",
        "worktree",
        "drop",
        "bisect",
        "--reason",
        "steward cleanup",
    ];
    let own_run = as_worker(&h, &steward, &drop);
    assert!(!own_run.status.success(), "{}", text(&own_run));
    assert!(path.exists());
    let mut other_run = drop.to_vec();
    other_run.extend(["--run", "alpha"]);
    worker_ok(&h, &steward, &other_run);
    assert!(!path.exists());
}

/// The `ledger status --json` board of `run`. Status exits 1 while the run
/// is not complete, so only its JSON counts.
fn board(h: &Harness, run: &str) -> serde_json::Value {
    let o = h.run(&["ledger", "status", "--run", run, "--json"]);
    serde_json::from_slice(&o.stdout).unwrap_or_else(|e| panic!("{e}: {}", text(&o)))
}

/// Bug 4: the steward task changes no file and makes no worktree. Its
/// finish passes and its done records the task as done, not UNLANDED.
#[test]
fn the_no_change_steward_task_finishes_without_a_worktree() {
    let h = harness("i308-no-worktree-finish");
    abandoned_worktree(&h);
    fleet(&h);
    let status = board(&h, "solo-w1");
    let worker = status["workers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|w| w["worker"]["persona"] == "worktree-steward")
        .and_then(|w| w["worker"]["worker_id"].as_str())
        .unwrap();
    worker_ok(&h, worker, &["ledger", "claim"]);
    let finished = worker_ok(&h, worker, &["ledger", "finish", "--wait"]);
    assert!(finished.contains("every finish step passed"), "{finished}");
    worker_ok(&h, worker, &["ledger", "done", "triage complete"]);
    let status = board(&h, "solo-w1");
    assert_eq!(status["tasks"][0]["task"]["status"], "done");
    assert!(status["worktrees"].as_array().unwrap().is_empty());
}
