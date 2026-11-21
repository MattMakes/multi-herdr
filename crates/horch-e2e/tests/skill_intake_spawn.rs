#![cfg(unix)]

use std::process::Output;

use horch_core::taskledger::workers::NewWorker;
use horch_e2e::bind::bind_worker;
use horch_e2e::harness::{fixtures, repo_root, Harness};
use horch_e2e::ledger_seed::{seed_run, seed_task};
use serde_json::Value;

fn text(o: &Output) -> String {
    format!(
        "status: {}\nstdout: {}\nstderr: {}",
        o.status,
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
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
    h.serve_fleetd();
    h
}

/// Register `role` on `pane`, as its `horch worker` does when the pane
/// starts (no pane runs a command here).
fn register(h: &Harness, role: &str, pane: &str) {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let dir = h.state.join("herdr-orchestration-w1");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{role}.id"));
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
    spawn_from_with(h, pane, teammate, &[])
}

/// [`spawn_from`] with extra variables for the spawn (and its fake herdr).
fn spawn_from_with(h: &Harness, pane: &str, teammate: &str, env: &[(&str, &str)]) -> Output {
    let brief = h.project.join("brief.md");
    std::fs::write(&brief, "# brief\n").unwrap();
    let argv = fixture_argv(teammate, &brief.to_string_lossy());
    let args: Vec<&str> = argv.iter().map(String::as_str).collect();
    let mut cmd = h.horch(&args);
    cmd.env("HERDR_PANE_ID", pane);
    cmd.envs(env.iter().copied());
    cmd.output().expect("running horch spawn")
}

/// Record `pid` as the agent process of ledger row `worker_id`, as the
/// row's `horch worker` does when its agent starts.
fn record_agent(h: &Harness, worker_id: &str, pid: u32) {
    let db = h.root.join("ledger.sqlite3");
    let store = horch_core::taskledger::Store::open(&db).unwrap();
    horch_core::taskledger::workers::agent_started(
        store.conn(),
        worker_id,
        pid,
        horch_core::procid::start_time(pid),
    )
    .unwrap();
}

fn worker_in_run(h: &Harness, persona: &str, pane: &str) -> String {
    let run = seed_run(h, "intake-run", None, "w1:p1");
    let file = h.project.join(format!("{persona}-ticket.md"));
    std::fs::write(&file, format!("# {persona}\n**Check:** `true`\n")).unwrap();
    seed_task(h, &run, &file, persona);
    let worker = NewWorker {
        persona: persona.to_string(),
        pane_id: Some(pane.to_string()),
        workspace_id: Some("w1".to_string()),
        ..Default::default()
    };
    bind_worker(h, worker, None).worker_id
}

fn worker_rows(h: &Harness) -> i64 {
    let store = horch_core::taskledger::Store::open(&h.ledger_db()).unwrap();
    store
        .conn()
        .query_row(
            "SELECT count(*) FROM workers WHERE kind = 'worker'",
            [],
            |r| r.get(0),
        )
        .unwrap()
}

fn assert_refused_and_nothing_left(h: &Harness, pane: &str, teammate: &str) {
    let rows = worker_rows(h);
    let calls = h.calls_of("herdr").len();
    let o = spawn_from(h, pane, teammate);
    assert!(!o.status.success(), "{}", text(&o));
    let stderr = String::from_utf8_lossy(&o.stderr);
    assert!(stderr.contains("horch spawn is retired"), "{}", text(&o));
    let briefs: Vec<_> = std::fs::read_dir(h.state.join("herdr-orchestration-w1"))
        .map(|d| {
            d.flatten()
                .filter(|e| e.file_name().to_string_lossy().ends_with(".brief.json"))
                .collect()
        })
        .unwrap_or_default();
    assert!(
        briefs.is_empty(),
        "a refused spawn wrote a brief: {briefs:?}"
    );
    let split = h.calls_of("herdr").into_iter().skip(calls).any(|c| {
        let argv: Vec<String> = serde_json::from_value(c["argv"].clone()).unwrap_or_default();
        argv.iter().any(|a| a == "split" || a == "run")
    });
    assert!(!split, "a refused spawn split or ran a pane");
    assert_eq!(worker_rows(h), rows, "a refused spawn left a worker row");
}

#[test]
fn trf_43_a_controller_outside_every_run_cannot_send_a_brief() {
    let h = roster_harness("intake-outside");
    register(&h, "ctl-1", "w1:p1");
    assert_eq!(worker_rows(&h), 0);
    assert_refused_and_nothing_left(&h, "w1:p1", "intake-smoke");
}

#[test]
fn trf_43_a_spoofed_pane_id_cannot_send_a_brief() {
    let h = roster_harness("intake-spoof");
    let ctl = worker_in_run(&h, "ctl", "w1:p2");
    register(&h, "ctl-1", "w1:p2");
    // The recorded agent is a process that is no ancestor of the caller.
    let mut other = std::process::Command::new("sleep")
        .arg("30")
        .spawn()
        .unwrap();
    record_agent(&h, &ctl, other.id());
    assert_refused_and_nothing_left(&h, "w1:p2", "intake-smoke");
    let _ = other.kill();
    let _ = other.wait();
}

#[test]
fn trf_43_a_worker_without_spawn_only_cannot_send_a_brief() {
    let h = roster_harness("intake-plain");
    worker_in_run(&h, "smoke", "w1:p2");
    register(&h, "smoke-1", "w1:p2");
    for teammate in ["intake-smoke", "smoke"] {
        assert_refused_and_nothing_left(&h, "w1:p2", teammate);
    }
}
