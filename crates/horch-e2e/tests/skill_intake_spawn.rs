//! End-to-end: the spawn path that `intake spawn` uses works from inside a
//! worker (skill-intake T-27, spec 6.1 H-1 and H-6, 6.5).
//!
//! The argv is read from `scripts/skill-intake/tests/fixtures/spawn-argv.json`,
//! the file `test_roles.py` (T-21) reads too, so a change to its shape on the
//! Python side fails here. The teammates are test-only (`agent: none`), so
//! no model session starts.
//!
//! TRF-43: `horch spawn` refuses task text, except a `spawn_only`
//! controller's brief to a `horch_access: report` role (T-56). The caller
//! must run in the agent process that horch recorded on the controller's
//! ledger row (T-58). No pane here runs an agent, so a test records this
//! test process as the controller's agent: each `horch spawn` it runs is a
//! child of it.

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

/// A controller `ctl-1` in a new run, with its worker row. Its agent is
/// not recorded yet.
fn controller_in_run(h: &Harness) -> (String, String, Value) {
    let run = run_id(h);
    queue_for(h, "ctl");
    let ctl = spawn_pane(h, &["ctl", "--exact"]);
    register(h, "ctl-1", &ctl);
    let row = ledger_workers(h)
        .into_iter()
        .find(|w| w["persona"] == "ctl")
        .expect("the controller has a worker row");
    (run, ctl, row)
}

fn ledger_worker(h: &Harness, worker_id: &str) -> Value {
    json_of(h, &["ledger", "show", worker_id, "--json"])["worker"].clone()
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
    let (run, ctl, ctl_row) = controller_in_run(&h);
    record_agent(
        &h,
        ctl_row["worker_id"].as_str().unwrap(),
        std::process::id(),
    );

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

/// TRF-43 (T-58): a controller outside every run has no ledger row, so
/// horch recorded no agent for it, and its brief is refused. The refusal
/// says to queue the work as a ledger task. No role pane opens, and no row
/// is written, also while the project has an active run.
#[test]
fn trf_43_a_controller_outside_every_run_cannot_send_a_brief() {
    let h = roster_harness("intake-outside");
    // No run is active yet: the controller joins none.
    let ctl = spawn_pane(&h, &["ctl", "--exact"]);
    register(&h, "ctl-1", &ctl);
    run_id(&h);
    assert!(ledger_workers(&h).iter().all(|w| w["persona"] != "ctl"));

    let o = spawn_from(&h, &ctl, "intake-smoke");
    assert!(!o.status.success(), "{}", text(&o));
    let stderr = String::from_utf8_lossy(&o.stderr);
    assert!(
        stderr.contains("horch spawn takes no task text")
            && stderr.contains("ctl-1 is in no ledger run, so it sends no brief")
            && stderr.contains("horch ledger task \"<title>\" --persona ctl, then spawn ctl."),
        "{}",
        text(&o)
    );
    assert!(ledger_workers(&h).is_empty(), "a refused spawn left a row");
    let role_brief = h
        .state
        .join("herdr-orchestration-w1/intake-smoke-1.brief.json");
    assert!(!role_brief.exists(), "a refused spawn wrote a brief");
}

/// TRF-43 (T-58): `HERDR_PANE_ID` alone does not make a caller the
/// controller. A caller outside the process tree of the controller's
/// recorded agent is refused, and no role row is left.
#[test]
fn trf_43_a_spoofed_pane_id_cannot_send_a_brief() {
    let h = roster_harness("intake-spoof");
    let (_, ctl, ctl_row) = controller_in_run(&h);
    let ctl_id = ctl_row["worker_id"].as_str().unwrap();
    // No agent recorded: the spawn is refused.
    let o = spawn_from(&h, &ctl, "intake-smoke");
    assert!(!o.status.success(), "{}", text(&o));
    // The recorded agent is a process that is no ancestor of the caller.
    let mut other = std::process::Command::new("sleep")
        .arg("30")
        .spawn()
        .unwrap();
    record_agent(&h, ctl_id, other.id());
    let spoofed = spawn_from(&h, &ctl, "intake-smoke");
    let _ = other.kill();
    let _ = other.wait();
    for o in [o, spoofed] {
        assert!(!o.status.success(), "{}", text(&o));
        let stderr = String::from_utf8_lossy(&o.stderr);
        assert!(
            stderr.contains("horch spawn takes no task text")
                && stderr.contains("--persona intake-smoke"),
            "{}",
            text(&o)
        );
    }
    assert!(
        ledger_workers(&h)
            .iter()
            .all(|w| w["persona"] != "intake-smoke"),
        "a refused spawn left a row"
    );
}

/// Report 56: a report role with a ledger row and no claimed task runs to
/// `horch done`. Its `agent: none` worker runs `horch done` with the env
/// that the report socket's `done` uses. The row ends `done`, and no
/// BLOCKED line or error reaches the controller.
#[test]
fn a_report_role_with_a_row_and_no_task_runs_to_done() {
    // A short label: the report socket path under the harness state root
    // must stay under the 104-byte macOS `SUN_LEN`.
    let h = roster_harness("i-done");
    let (_, ctl, ctl_row) = controller_in_run(&h);
    record_agent(
        &h,
        ctl_row["worker_id"].as_str().unwrap(),
        std::process::id(),
    );

    // Only the role's pane runs its command.
    let o = spawn_from_with(
        &h,
        &ctl,
        "intake-smoke",
        &[("HORCH_FAKE_SCENARIO", "exec,stay")],
    );
    assert!(o.status.success(), "{}", text(&o));
    let row = ledger_workers(&h)
        .into_iter()
        .find(|w| w["persona"] == "intake-smoke")
        .expect("the role has a worker row");
    let id = row["worker_id"].as_str().unwrap().to_string();
    assert!(row["task_id"].is_null(), "{row}");

    let pane = row["pane_id"].as_str().unwrap().replace(':', "_");
    let mut out = h.log.clone().into_os_string();
    out.push(format!(".pane-{pane}.out"));
    let screen = || std::fs::read_to_string(&out).unwrap_or_default();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    let ended = loop {
        let w = ledger_worker(&h, &id);
        if w["ended_at"].is_string() {
            break w;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the row never ended: {w}\npane output:\n{}",
            screen()
        );
        std::thread::sleep(std::time::Duration::from_millis(200));
    };
    assert_eq!(
        ended["status"],
        "done",
        "{ended}\npane output:\n{}",
        screen()
    );
    assert!(ended["task_id"].is_null(), "{ended}");
    let mail = h.state.join("herdr-orchestration-w1");
    let blocked = std::fs::read_dir(&mail)
        .unwrap()
        .flatten()
        .filter_map(|e| std::fs::read_to_string(e.path()).ok())
        .filter(|t| t.contains("BLOCKED") || t.contains("error"))
        .collect::<Vec<_>>();
    assert!(blocked.is_empty(), "{blocked:?}");
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
