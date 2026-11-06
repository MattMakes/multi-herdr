//! End-to-end: the spawn path that `intake spawn` uses works from inside a
//! worker (skill-intake T-27, spec 6.1 H-1 and H-6, 6.5).
//!
//! The argv is read from `scripts/skill-intake/tests/fixtures/spawn-argv.json`,
//! the file `test_roles.py` (T-21) reads too, so a change to its shape on the
//! Python side fails here. The teammates are test-only (`agent: none`), so
//! no model session starts.
//!
//! TRF-43: `horch spawn` refuses task text, except a `spawn_only`
//! controller's brief to a `horch_access: report` role (T-56).

#![cfg(unix)]

use std::process::Output;

use horch_e2e::harness::{fixtures, repo_root, Harness};
use serde_json::Value;

fn text(o: &Output) -> String {
    format!(
        "status: {}\nstdout: {}\nstderr: {}",
        o.status,
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

fn json_of(h: &Harness, args: &[&str]) -> Value {
    let o = h.run(args);
    serde_json::from_slice(&o.stdout).unwrap_or_else(|e| panic!("{e}: {}", text(&o)))
}

/// The spawn argv of the shared fixture, with `{teammate}` and `{brief}`
/// replaced as `intake spawn` replaces them.
fn fixture_argv(teammate: &str, brief: &str) -> Vec<String> {
    let path = repo_root().join("scripts/skill-intake/tests/fixtures/spawn-argv.json");
    let raw = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let doc: Value = serde_json::from_str(&raw).expect("spawn-argv.json is JSON");
    doc["argv"]
        .as_array()
        .expect("spawn-argv.json has an argv array")
        .iter()
        .map(|a| {
            a.as_str()
                .expect("argv items are strings")
                .replace("{teammate}", teammate)
                .replace("{brief}", brief)
        })
        .collect()
}

/// A ledger harness with 1 workspace (`w1`, root pane `w1:p1`) and its own
/// roster: `ctl` may spawn only `intake-*`, `intake-smoke` is a report
/// role, and none of them starts an agent. No pane runs a command, so every spawned worker stays as the
/// spawn left it: its role registered, its brief on disk.
fn roster_harness(label: &str) -> Harness {
    let mut h = Harness::new(label).with_git();
    let db = h.root.join("ledger.sqlite3");
    h.set("HORCH_LEDGER_DB", db.to_string_lossy());
    h.set("HORCH_NO_WAKE", "1");
    h.set("HORCH_NO_WATCHDOG", "1");
    let quota = fixtures().join("quota/all-ok.json");
    h.set("HORCH_QUOTA_FILE", quota.to_string_lossy());
    h.set("HORCH_WORKSPACE_ID", "w1");
    let teammates = h.root.join("teammates");
    std::fs::create_dir_all(&teammates).unwrap();
    let smoke = |name: &str, extra: &str| {
        format!(
            "---\nname: {name}\nbrief_description: Test-only {name}.\nhidden: true\n\
             base: fleet-worker\nagent: none\nmodel: none\n{extra}---\nTest-only teammate.\n"
        )
    };
    for (name, extra) in [("ctl", "spawn_only: [\"intake-*\"]\n"), ("smoke", "")] {
        std::fs::write(teammates.join(format!("{name}.md")), smoke(name, extra)).unwrap();
    }
    let role = "---\nname: intake-smoke\nbrief_description: Test-only intake-smoke.\n\
                hidden: true\nbase: report-worker\nagent: none\nmodel: none\n\
                horch_access: report\nsandbox: {}\n---\nTest-only report role.\n";
    std::fs::write(teammates.join("intake-smoke.md"), role).unwrap();
    h.set("HORCH_TEAMMATES_DIR", teammates.to_string_lossy());
    let mut create = std::process::Command::new(h.bin.join("herdr"));
    h.seal(&mut create);
    let o = create
        .args(["workspace", "create", "--label", label])
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", text(&o));
    h
}

/// Spawn `args` from the operator pane `w1:p1`; return the new pane id.
fn spawn_pane(h: &Harness, args: &[&str]) -> String {
    let mut all = vec!["spawn"];
    all.extend_from_slice(args);
    all.extend_from_slice(&["--from-pane", "w1:p1", "--no-tile"]);
    let o = h.run(&all);
    assert!(o.status.success(), "{}", text(&o));
    String::from_utf8_lossy(&o.stdout)
        .lines()
        .last()
        .expect("spawn prints the pane id")
        .trim()
        .to_string()
}

/// Register `role` on `pane`, as its `horch worker` does when the pane
/// starts (no pane runs a command here).
fn register(h: &Harness, role: &str, pane: &str) {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let path = h.state.join(format!("herdr-orchestration-w1/{role}.id"));
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    f.write_all(pane.as_bytes()).unwrap();
}

/// Run the `intake spawn` argv for `teammate` from inside pane `pane`, the
/// way a worker in that pane runs it. No `HORCH_RUN_ID` is set.
fn spawn_from(h: &Harness, pane: &str, teammate: &str) -> Output {
    let brief = h.project.join("brief.md");
    std::fs::write(&brief, "# brief\n").unwrap();
    let argv = fixture_argv(teammate, &brief.to_string_lossy());
    let args: Vec<&str> = argv.iter().map(String::as_str).collect();
    let mut cmd = h.horch(&args);
    cmd.env("HERDR_PANE_ID", pane);
    cmd.output().expect("running horch spawn")
}

fn run_id(h: &Harness) -> String {
    let o = h.run(&["ledger", "run", "new", "intake-run"]);
    assert!(o.status.success(), "{}", text(&o));
    let run = json_of(h, &["ledger", "status", "--json"]);
    run["run"]["run_id"].as_str().expect("a run id").to_string()
}

/// The ledger worker rows (kind `worker`) of the active run.
fn ledger_workers(h: &Harness) -> Vec<Value> {
    let st = json_of(h, &["ledger", "status", "--json"]);
    st["workers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|w| w["worker"].clone())
        .filter(|w| w["kind"] == "worker")
        .collect()
}

fn brief_of(h: &Harness, role: &str) -> Value {
    let path = h
        .state
        .join(format!("herdr-orchestration-w1/{role}.brief.json"));
    let raw = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    serde_json::from_str(&raw).expect("the brief is JSON")
}

/// Queue 1 startable task for `persona` in the active run: a task-less
/// spawn into a run needs one (TRF-31).
fn queue_for(h: &Harness, persona: &str) {
    let file = h.project.join(format!("{persona}-ticket.md"));
    std::fs::write(&file, format!("# {persona}\n**Check:** `true`\n")).unwrap();
    let file = file.to_string_lossy().into_owned();
    let o = h.run(&["ledger", "add", "--file", &file, "--primary", persona]);
    assert!(o.status.success(), "{}", text(&o));
}

/// T-38: a `spawn_only` controller that is a worker of a ledger run spawns
/// a role with task text, and no one set `HORCH_RUN_ID`: the role has a
/// worker row in the controller's run, and its brief carries the row id,
/// which its pane gets as `HORCH_WORKER_ID`. The controller itself claims
/// its task, as every worker does (TRF-43).
#[test]
fn a_controller_in_a_run_spawns_a_role_that_joins_the_run() {
    let h = roster_harness("intake-join");
    let run = run_id(&h);
    queue_for(&h, "ctl");
    let ctl = spawn_pane(&h, &["ctl", "--exact"]);
    register(&h, "ctl-1", &ctl);
    let ctl_row = ledger_workers(&h)
        .into_iter()
        .find(|w| w["persona"] == "ctl")
        .expect("the controller has a worker row");

    let o = spawn_from(&h, &ctl, "intake-smoke");
    assert!(o.status.success(), "{}", text(&o));

    let row = ledger_workers(&h)
        .into_iter()
        .find(|w| w["persona"] == "intake-smoke")
        .unwrap_or_else(|| panic!("the role has no worker row: {}", text(&o)));
    assert_eq!(row["run_id"], run.as_str(), "{row}");
    assert_eq!(row["kind"], "worker", "{row}");
    assert_ne!(row["worker_id"], ctl_row["worker_id"]);
    let brief = brief_of(&h, "intake-smoke-1");
    assert_eq!(brief["worker_id"], row["worker_id"], "{brief}");
    assert_eq!(brief["controller"], "ctl-1", "{brief}");
    assert_eq!(
        brief["task"],
        "Read and follow ".to_string()
            + &h.project.join("brief.md").to_string_lossy()
            + " exactly.",
        "the role keeps its task text and is not told to claim"
    );
}

/// T-38: a controller outside every run spawns its role outside every run,
/// with exit 0, also while the project has an active run.
#[test]
fn a_controller_outside_every_run_spawns_a_role_outside_every_run() {
    let h = roster_harness("intake-outside");
    // No run is active yet: the controller joins none.
    let ctl = spawn_pane(&h, &["ctl", "--exact"]);
    register(&h, "ctl-1", &ctl);
    run_id(&h);
    assert!(ledger_workers(&h).iter().all(|w| w["persona"] != "ctl"));

    let o = spawn_from(&h, &ctl, "intake-smoke");
    assert!(o.status.success(), "{}", text(&o));
    assert!(
        ledger_workers(&h)
            .iter()
            .all(|w| w["persona"] != "intake-smoke"),
        "the role joined a run: {}",
        text(&o)
    );
    assert!(brief_of(&h, "intake-smoke-1").get("worker_id").is_none());
}

/// TRF-43: a worker without `spawn_only` cannot send task text, also to a
/// report role. The refusal names `horch ledger task`, and no row is left.
#[test]
fn trf_43_a_worker_without_spawn_only_cannot_send_a_brief() {
    let h = roster_harness("intake-plain");
    let plain = spawn_pane(&h, &["smoke", "--exact"]);
    register(&h, "smoke-1", &plain);
    run_id(&h);

    for teammate in ["intake-smoke", "smoke"] {
        let o = spawn_from(&h, &plain, teammate);
        assert!(!o.status.success(), "{}", text(&o));
        let stderr = String::from_utf8_lossy(&o.stderr);
        assert!(
            stderr.contains("horch spawn takes no task text")
                && stderr.contains(&format!("--persona {teammate}")),
            "{}",
            text(&o)
        );
    }
    assert!(ledger_workers(&h).is_empty(), "a refused spawn left a row");
}
