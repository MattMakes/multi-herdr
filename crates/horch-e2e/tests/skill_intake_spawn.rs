//! End-to-end: the spawn path that `intake spawn` uses works from inside a
//! worker (skill-intake T-27, spec 6.1 H-1 and H-6, 6.5).
//!
//! The argv is read from `scripts/skill-intake/tests/fixtures/spawn-argv.json`,
//! the file `test_roles.py` (T-21) reads too, so a change to its shape on the
//! Python side fails here. Only the teammate is swapped for `smoke`
//! (`agent: none`), so no model session starts.

#![cfg(unix)]

use std::process::Output;
use std::time::{Duration, Instant};

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

fn records(h: &Harness) -> Vec<Value> {
    let slug: String = h
        .project
        .to_string_lossy()
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() {
                b as char
            } else {
                '-'
            }
        })
        .collect();
    std::fs::read_to_string(h.state.join(format!("{slug}.json")))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn within(limit: Duration, mut f: impl FnMut() -> bool) -> bool {
    let until = Instant::now() + limit;
    while Instant::now() < until {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    f()
}

#[test]
fn a_worker_spawns_a_role_worker_that_joins_its_ledger_run() {
    let mut h = Harness::new("intake-spawn").with_git();
    let db = h.root.join("ledger.sqlite3");
    h.set("HORCH_LEDGER_DB", db.to_string_lossy());
    h.set("HORCH_NO_WAKE", "1");
    h.set("HORCH_NO_WATCHDOG", "1");
    let quota = fixtures().join("quota/all-ok.json");
    h.set("HORCH_QUOTA_FILE", quota.to_string_lossy());
    h.set("HORCH_WORKSPACE_ID", "w1");
    h.set("HORCH_FAKE_SCENARIO", "exec");
    // The spawn waits for the smoke worker to register; a loaded machine
    // needs time, and the wait only bounds a broken run.
    h.set("HORCH_SPAWN_WAIT_MS", "60000");
    let mut create = std::process::Command::new(h.bin.join("herdr"));
    h.seal(&mut create);
    let o = create
        .args(["workspace", "create", "--label", "intake-spawn"])
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", text(&o));

    let o = h.run(&["ledger", "run", "new", "intake-run"]);
    assert!(o.status.success(), "{}", text(&o));
    let run = json_of(&h, &["ledger", "status", "--json"]);
    let run_id = run["run"]["run_id"].as_str().expect("a run id").to_string();

    let brief = h.project.join("brief.md");
    std::fs::write(&brief, "# brief\n").unwrap();
    let argv = fixture_argv("smoke", &brief.to_string_lossy());
    let args: Vec<&str> = argv.iter().map(String::as_str).collect();

    // The environment of a worker that runs `intake spawn`.
    let mut cmd = h.horch(&args);
    cmd.env("HORCH_ROLE", "intake-writer-1")
        .env("HORCH_RECORD_ID", "parent-record")
        .env("HERDR_PANE_ID", "w1:p1")
        .env("HORCH_RUN_ID", &run_id);
    let o = cmd.output().expect("running horch spawn");
    assert!(o.status.success(), "{}", text(&o));

    // A session record exists for the new worker.
    let record = records(&h)
        .into_iter()
        .find(|r| r["role"] == "smoke-1")
        .unwrap_or_else(|| panic!("no record for smoke-1: {:?}\n{}", records(&h), text(&o)));

    // The ledger row of the new worker has the parent's run id.
    let st = json_of(&h, &["ledger", "status", "--json"]);
    let w = st["workers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|w| &w["worker"])
        .find(|w| w["kind"] == "worker")
        .unwrap_or_else(|| panic!("spawn wrote no worker row: {st}"));
    assert_eq!(w["run_id"], run_id.as_str(), "{w}");
    assert_eq!(w["persona"], "smoke", "{w}");

    // `smoke` is `agent: none`: the worker runs to its end, and no model
    // process starts.
    let finished = || {
        records(&h)
            .iter()
            .any(|r| r["role"] == "smoke-1" && r["state"]["state"] == "done")
    };
    assert!(
        within(Duration::from_secs(30), finished),
        "the smoke worker never finished: {record}"
    );
    for fake in ["claude", "codex", "opencode", "pi", "prime-agent", "ollama"] {
        assert!(
            h.calls_of(fake).is_empty(),
            "{fake} started: {:?}",
            h.calls_of(fake)
        );
    }
}
