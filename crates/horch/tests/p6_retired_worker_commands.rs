#![cfg(unix)]

mod support;

use std::fs::{self, OpenOptions, Permissions};
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::Instant;

use horch_core::fleetd::fleet_runtime::adapters::channel_server::ChannelServer;
use horch_core::fleetd::fleet_runtime::adapters::request_spool::{
    scan, Handled, ScanOutcome, Spool,
};
use horch_core::fleetd::fleet_runtime::adapters::scans::{requests_dir, ScanClock};
use horch_core::fleetd::fleet_runtime::adapters::token_file;
use horch_core::fleetd::fleet_runtime::application::handlers_task::{self, TaskHandlers};
use horch_core::fleetd::fleet_runtime::application::handlers_worker::{self, WorkerDeps};
use horch_core::fleetd::fleet_runtime::application::requests::Handlers;
use horch_core::fleetd::fleet_runtime::domain::budget::{Cursor, Deadline};
use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;
use horch_core::fleetd::fleet_runtime::domain::ledger::Generation;
use horch_core::fleetd::fleet_runtime::domain::requests::{Class, Kind, RETIRED};
use horch_core::taskledger::model::{Actor, WorkerKind};
use horch_core::taskledger::runs::{self, NewRun, Run};
use horch_core::taskledger::tasks::{self, NewTask};
use horch_core::taskledger::workers::{self, NewWorker};
use horch_core::taskledger::{schema, util, Store};
use horch_platform::channel::User;
use rusqlite::{params, Connection};
use serde_json::json;
use tempfile::TempDir;

const SPAWN: &str = "fleetd launches each attempt. To pair with a persona, plan its task with an interactive persona";
const RECORDS: &str = "fleetd records each attempt at its launch";
const REAP: &str =
    r#"fleetd stops an orphan worker. To stop 1 worker: horch ledger stop <worker> "<reason>""#;
const UNBLOCK: &str = "Queue the task again: horch task requeue <task>";
const CLAIM: &str = "horch fleet start starts fleetd and herdr with no orchestrator, and fleetd starts 1 sub-orchestrator for each feature";
const FLEET_STOP: &str = r#"No orchestrator runs, so there is none to stop. fleetd and the herdr server stay under the service manager of the OS. To stop 1 worker: horch ledger stop <worker> "<reason>""#;

const RETIRED_KINDS: [(&str, &str); 8] = [
    ("spawn", SPAWN),
    ("worker_new", RECORDS),
    ("worker_pid", RECORDS),
    ("worker_reap", REAP),
    ("orchestrator_claim", CLAIM),
    ("orchestrator_stop", FLEET_STOP),
    ("task_unblock", UNBLOCK),
    ("spawn_adopt", RECORDS),
];

const DESIGN_OPERATOR: [&str; 16] = [
    "idea_add",
    "idea_amend",
    "idea_approve",
    "idea_revise",
    "idea_park",
    "idea_cancel",
    "task_requeue",
    "task_cancel",
    "job_requeue",
    "feature_abandon",
    "branch_abandon",
    "rescue_drop",
    "worktree_restore",
    "worktree_drop",
    "ledger_reopen",
    "ledger_dismiss",
];

const DESIGN_WORKER: [&str; 4] = ["progress", "result", "ask", "idea_note"];

const KEPT: [&str; 7] = [
    "worker_stop",
    "task_fallback",
    "ledger_waive",
    "ledger_import",
    "ledger_gc",
    "worktree_scratch",
    "worktree_triage",
];

const RECORDS_OF_TODAY: [&str; 8] = [
    "spawn_gate_note",
    "worker_launched",
    "worker_launch_failed",
    "worker_progress",
    "worker_done",
    "run_adopt",
    "question_answer",
    "session_records",
];

struct Out {
    code: i32,
    out: String,
    err: String,
}

struct World {
    server: Option<ChannelServer>,
    dir: TempDir,
}

impl World {
    fn down() -> World {
        let dir = tempfile::Builder::new()
            .prefix("rw")
            .permissions(Permissions::from_mode(0o700))
            .tempdir_in("/tmp")
            .unwrap();
        let w = World { server: None, dir };
        for sub in ["home", "proj", "fleet"] {
            fs::create_dir_all(w.root().join(sub)).unwrap();
        }
        let store = Store::open(&w.ledger()).unwrap();
        store
            .conn()
            .execute(
                "INSERT INTO fleetd (id, generation, pid, pid_start, started_at, fleet_clock_s, clock_ticked_at)
                 VALUES (1, 1, ?1, ?2, ?3, 0, ?3)",
                params![std::process::id(), this_start(), util::now()],
            )
            .unwrap();
        token_file::ensure(&w.fleet()).unwrap();
        w
    }

    fn up() -> World {
        let mut w = World::down();
        let handlers = w.handlers();
        w.server =
            Some(ChannelServer::start(&w.fleet(), w.conn(), Generation(1), handlers).unwrap());
        w
    }

    fn handlers(&self) -> Handlers {
        let mut handlers = Handlers::new();
        handlers_task::register(
            &mut handlers,
            TaskHandlers {
                config: Arc::new(FleetConfig::default()),
                evidence_root: self.root().join("evidence"),
            },
        );
        handlers_worker::register(
            &mut handlers,
            &WorkerDeps {
                guard: Generation(1),
            },
        );
        handlers
    }

    fn root(&self) -> PathBuf {
        horch_platform::path::canonicalize(self.dir.path()).unwrap()
    }

    fn project(&self) -> PathBuf {
        self.root().join("proj")
    }

    fn ledger(&self) -> PathBuf {
        self.root().join("ledger.sqlite3")
    }

    fn fleet(&self) -> PathBuf {
        self.root().join("fleet")
    }

    fn conn(&self) -> Connection {
        let conn = Connection::open(self.ledger()).unwrap();
        conn.busy_timeout(std::time::Duration::from_secs(10))
            .unwrap();
        conn
    }

    fn horch(&self, args: &[&str]) -> Out {
        let root = self.root();
        let out = Command::new(env!("CARGO_BIN_EXE_horch"))
            .args(args)
            .current_dir(self.project())
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", root.join("home"))
            .env("XDG_DATA_HOME", root.join("data"))
            .env("HORCH_DATA_DIR", root.join("data"))
            .env("HORCH_STATE_DIR", root.join("state"))
            .env("HORCH_LEDGER_DB", self.ledger())
            .env("HORCH_PROJECT_DIR", self.project())
            .env("FLEET_ROOT", self.fleet())
            .env("HORCH_WORKSPACE_ID", "w1")
            .env("HORCH_NO_WATCHDOG", "1")
            .env(
                "HORCH_MACHINE_FILE",
                support::fleetd::healthy_machine_file(&root),
            )
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        Out {
            code: out.status.code().unwrap_or(-1),
            out: String::from_utf8_lossy(&out.stdout).into_owned(),
            err: String::from_utf8_lossy(&out.stderr).into_owned(),
        }
    }

    fn assert_retired(&self, args: &[&str], command: &str, instead: &str) {
        let o = self.horch(args);
        let line = format!("{command} is retired. {instead}");
        assert_eq!(
            o.code, 1,
            "{args:?}\nstdout:\n{}\nstderr:\n{}",
            o.out, o.err
        );
        assert!(
            o.err.contains(&line),
            "{args:?} prints `{line}`\nstdout:\n{}\nstderr:\n{}",
            o.out,
            o.err
        );
        assert_eq!(self.request_kinds(), Vec::<String>::new(), "{args:?}");
    }

    fn request_kinds(&self) -> Vec<String> {
        let conn = self.conn();
        let mut stmt = conn.prepare("SELECT kind FROM requests").unwrap();
        let mut kinds: Vec<String> = stmt
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        if let Ok(files) = fs::read_dir(requests_dir(&self.fleet())) {
            for f in files {
                let text = fs::read_to_string(f.unwrap().path()).unwrap();
                let line: serde_json::Value = serde_json::from_str(text.trim()).unwrap();
                kinds.push(line["kind"].as_str().unwrap().to_string());
            }
        }
        kinds
    }

    fn requests(&self) -> Vec<(String, String)> {
        let conn = self.conn();
        let mut stmt = conn
            .prepare("SELECT kind, state FROM requests ORDER BY received_at, rowid")
            .unwrap();
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    }

    fn run(&self) -> Run {
        let conn = self.conn();
        let run = runs::create(
            &conn,
            &self.project(),
            &NewRun {
                slug: "r1".to_string(),
                workspace_id: Some("w1".to_string()),
                base_branch: Some("main".to_string()),
                ..NewRun::default()
            },
        )
        .unwrap();
        schema::run_idea(&conn, &run.run_id, &util::now(), true).unwrap();
        run
    }

    fn task(&self, run: &Run, primary: &str, fallback: Option<&str>) -> String {
        let mut t = NewTask::work("alpha", "docs/alpha.md", primary);
        t.fallback = fallback.map(str::to_string);
        t.ticket_snapshot = Some("# A ticket\n".into());
        tasks::add(&self.conn(), run, &t, Actor::Orchestrator)
            .unwrap()
            .task_id
    }

    fn worker(&self, run: &Run, persona: &str, role: &str) -> String {
        workers::insert(
            &self.conn(),
            &NewWorker {
                run_id: run.run_id.clone(),
                kind: Some(WorkerKind::Worker),
                persona: persona.into(),
                role: Some(role.into()),
                workspace_id: Some("w1".into()),
                ..Default::default()
            },
        )
        .unwrap()
    }

    fn one(&self, sql: &str, id: &str) -> Option<String> {
        self.conn().query_row(sql, [id], |r| r.get(0)).unwrap()
    }
}

impl Drop for World {
    fn drop(&mut self) {
        if let Some(server) = self.server.take() {
            let _ = server.stop();
        }
    }
}

fn this_start() -> i64 {
    let start = horch_platform::process::start_time(std::process::id())
        .expect("the start time of the test");
    i64::try_from(start).unwrap()
}

fn write_0600(path: &Path, text: &str) {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .unwrap();
    file.write_all(text.as_bytes()).unwrap();
    fs::set_permissions(path, Permissions::from_mode(0o600)).unwrap();
}

#[test]
fn kind_catalog_is_the_table_of_the_design_and_the_kept_kinds() {
    let mut want: Vec<&str> = DESIGN_OPERATOR
        .iter()
        .chain(&DESIGN_WORKER)
        .chain(&KEPT)
        .chain(&RECORDS_OF_TODAY)
        .copied()
        .collect();
    want.sort_unstable();
    let mut have: Vec<&str> = Kind::ALL.iter().map(|k| k.as_str()).collect();
    have.sort_unstable();
    assert_eq!(have, want, "Kind::ALL");
    for name in DESIGN_WORKER {
        assert_eq!(Kind::parse(name).unwrap().class(), Class::Worker, "{name}");
    }
    for name in DESIGN_OPERATOR.iter().chain(&KEPT).chain(&RECORDS_OF_TODAY) {
        assert_eq!(
            Kind::parse(name).unwrap().class(),
            Class::Operator,
            "{name}"
        );
    }
    for (kind, instead) in RETIRED_KINDS {
        let r = RETIRED.iter().find(|r| r.kind == kind);
        assert_eq!(r.map(|r| r.instead), Some(instead), "{kind}");
        assert_eq!(
            Kind::parse(kind).unwrap_err().to_string(),
            format!("retired kind: {instead}")
        );
    }
    assert_eq!(
        RETIRED.len(),
        15,
        "MAP.md retires 14 kinds, and T-302 retires spawn_adopt"
    );
}

#[test]
fn spawn_is_retired_and_names_the_launch_of_fleetd() {
    let w = World::down();
    w.assert_retired(&["spawn", "opus"], "horch spawn", SPAWN);
    let run = w.run();
    let target = w.task(&run, "opus", None);
    let o = w.horch(&[
        "spawn",
        "opus",
        "--run",
        "r1",
        "--from-pane",
        "w1:p1",
        "--exact",
    ]);
    assert_eq!(o.code, 1, "stdout:\n{}\nstderr:\n{}", o.out, o.err);
    assert!(
        o.err.contains(&format!("horch spawn is retired. {SPAWN}")),
        "{}",
        o.err
    );
    assert_eq!(w.request_kinds(), Vec::<String>::new());
    assert_eq!(
        w.one("SELECT status FROM tasks WHERE task_id = ?1", &target),
        Some("ready".into()),
        "a retired spawn binds no task"
    );
}

#[test]
fn ledger_worker_new_is_retired_and_names_the_records_of_fleetd() {
    let w = World::down();
    w.run();
    w.assert_retired(
        &[
            "ledger",
            "worker",
            "new",
            "--persona",
            "opus",
            "--run",
            "r1",
        ],
        "horch ledger worker new",
        RECORDS,
    );
}

#[test]
fn ledger_worker_pid_is_retired_and_names_the_records_of_fleetd() {
    let w = World::down();
    let run = w.run();
    let worker = w.worker(&run, "opus", "opus-1");
    w.assert_retired(
        &["ledger", "worker", "pid", &worker, "--agent", "1"],
        "horch ledger worker pid",
        RECORDS,
    );
    assert_eq!(
        w.one(
            "SELECT CAST(agent_pid AS TEXT) FROM workers WHERE worker_id = ?1",
            &worker
        ),
        None
    );
}

#[test]
fn ledger_reap_is_retired_and_names_ledger_stop() {
    let w = World::down();
    let run = w.run();
    let worker = w.worker(&run, "opus", "opus-1");
    w.assert_retired(
        &["ledger", "reap", "opus-1", "gone", "--run", "r1"],
        "horch ledger reap",
        REAP,
    );
    assert_eq!(
        w.one("SELECT status FROM workers WHERE worker_id = ?1", &worker),
        Some("spawning".into())
    );
}

#[test]
fn ledger_unblock_is_retired_and_names_task_requeue() {
    let w = World::down();
    w.run();
    w.assert_retired(
        &["ledger", "unblock", "T-2", "--from", "T-1", "--run", "r1"],
        "horch ledger unblock",
        UNBLOCK,
    );
}

#[test]
fn fleet_stop_is_retired_and_names_the_service_manager() {
    let w = World::down();
    w.run();
    w.assert_retired(&["fleet", "stop"], "horch fleet stop", FLEET_STOP);
}

#[test]
fn ledger_stop_still_stops_1_worker() {
    let w = World::up();
    let run = w.run();
    let one = w.worker(&run, "opus", "opus-1");
    let two = w.worker(&run, "opus", "opus-2");
    let o = w.horch(&["ledger", "stop", "opus-1", "done with it", "--run", "r1"]);
    assert_eq!(o.code, 0, "stdout:\n{}\nstderr:\n{}", o.out, o.err);
    assert!(o.out.contains("Stopped opus-1"), "{}", o.out);
    let status = "SELECT status FROM workers WHERE worker_id = ?1";
    assert_eq!(w.one(status, &one), Some("stopped".into()));
    assert_eq!(w.one(status, &two), Some("spawning".into()));
    assert_eq!(
        w.requests(),
        [("worker_stop".to_string(), "applied".to_string())]
    );
}

#[test]
fn ledger_fallback_still_moves_a_task() {
    let w = World::up();
    let run = w.run();
    let task = w.task(&run, "opus", Some("codex-sol"));
    let short = w
        .one("SELECT 'T-' || seq FROM tasks WHERE task_id = ?1", &task)
        .unwrap();
    let o = w.horch(&["ledger", "fallback", &short, "opus is busy", "--run", "r1"]);
    assert_eq!(o.code, 0, "stdout:\n{}\nstderr:\n{}", o.out, o.err);
    assert!(
        o.out
            .contains(&format!("{short} can now be claimed by codex-sol.")),
        "{}",
        o.out
    );
    assert_eq!(
        w.one("SELECT claimable_by FROM tasks WHERE task_id = ?1", &task),
        Some("codex-sol".into())
    );
    assert_eq!(
        w.requests(),
        [("task_fallback".to_string(), "applied".to_string())]
    );
}

#[test]
fn fleetd_review_spawn_does_not_use_the_spawn_kind() {
    let w = World::down();
    let run = w.run();
    let task = w.task(&run, "opus-reviewer", None);
    let worker = w.worker(&run, "opus-reviewer", "opus-reviewer-1");
    let attempt = format!("{task}/A1");
    w.conn()
        .execute(
            "UPDATE workers SET attempt = 1 WHERE worker_id = ?1",
            [&worker],
        )
        .unwrap();
    w.conn()
        .execute(
            "INSERT INTO attempts (id, task_id, worker_id, state, via, started_at)
             VALUES (?1, ?2, ?3, 'launching', 'push', ?4)",
            params![attempt, task, worker, util::now()],
        )
        .unwrap();
    let o = w.horch(&[
        "spawn",
        "opus-reviewer",
        "--run",
        "r1",
        "--exact",
        "--attempt",
        &attempt,
        "--from-pane",
        "w1:p1",
    ]);
    assert!(
        !o.err.contains("is retired") && !o.out.contains("is retired"),
        "the spawn of fleetd is not retired\nstdout:\n{}\nstderr:\n{}",
        o.out,
        o.err
    );
    let kinds = w.request_kinds();
    assert!(
        !kinds.iter().any(|k| k == "spawn"),
        "the spawn of fleetd sends no spawn request: {kinds:?}\nstderr:\n{}",
        o.err
    );
    let start = fs::read_dir(w.fleet().join("tasks"))
        .into_iter()
        .flatten()
        .any(|_| true);
    assert!(
        start,
        "the spawn wrote the task files of its attempt\nstderr:\n{}",
        o.err
    );
    assert_eq!(
        w.one("SELECT status FROM tasks WHERE task_id = ?1", &task),
        Some("ready".into()),
        "the spawn binds nothing"
    );
}

#[test]
fn retired_worker_kind_request_file_is_refused_with_the_new_action() {
    let w = World::down();
    let fleet = w.fleet();
    let token = token_file::ensure(&fleet).unwrap();
    let mut conn = w.conn();
    let dir = requests_dir(&fleet);
    fs::create_dir_all(&dir).unwrap();
    fs::set_permissions(&dir, Permissions::from_mode(0o700)).unwrap();
    for (n, (kind, _)) in RETIRED_KINDS.iter().enumerate() {
        let id = format!("7a2c3b4d-1e0f-4a66-9f80-2b3c4d5e6f{n:02}");
        let line = json!({
            "request_id": id,
            "kind": kind,
            "payload": {},
            "token": token.text(),
        });
        write_0600(&dir.join(format!("{id}.json")), &format!("{line}\n"));
    }
    let owner = User::current().unwrap();
    let spool = Spool {
        fleet_root: &fleet,
        owner: &owner,
        token: Some(&token),
        max_bytes: 64 * 1024,
        guard: Generation(1),
    };
    let start = Instant::now();
    let mut now = move || start;
    let mut clock = ScanClock {
        deadline: Deadline::pass(start, 3600),
        now: &mut now,
        stamp: "2026-11-18T00:00:00Z",
    };
    let outcome = scan(
        &mut conn,
        &spool,
        &mut Cursor::new("2026-11-18T00:00:00Z"),
        &mut clock,
        &Handlers::new(),
    )
    .unwrap();
    let ScanOutcome::Read(read) = outcome else {
        panic!("expected a scan of the folder, got {outcome:?}");
    };
    assert_eq!(read.facts.len(), RETIRED_KINDS.len());
    for (n, (kind, instead)) in RETIRED_KINDS.iter().enumerate() {
        let name = format!("7a2c3b4d-1e0f-4a66-9f80-2b3c4d5e6f{n:02}.json");
        let fact = read.facts.iter().find(|f| f.name == name).unwrap();
        assert_eq!(
            fact.handled,
            Handled::Rejected(format!("retired kind: {instead}")),
            "the request file of {kind}"
        );
    }
    assert_eq!(w.requests(), Vec::<(String, String)>::new());
}

#[test]
fn spawn_adopt_request_file_is_refused_as_retired_and_changes_no_row() {
    let w = World::down();
    let run = w.run();
    let task = w.task(&run, "opus", None);
    let worker = w.worker(&run, "opus", "opus-1");
    let conn = w.conn();
    conn.execute(
        "UPDATE workers SET target_task_id = ?2, attempt = 1 WHERE worker_id = ?1",
        params![worker, task],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO attempts (id, task_id, worker_id, state, via, started_at)
         VALUES (?1, ?2, ?3, 'launching', 'push', ?4)",
        params![format!("{task}/A1"), task, worker, util::now()],
    )
    .unwrap();
    let rows = |c: &Connection| -> (i64, String, String) {
        c.query_row(
            "SELECT (SELECT COUNT(*) FROM events WHERE entity != 'needs_you'),
                    (SELECT status || '|' || IFNULL(agent_pid, '') FROM workers WHERE worker_id = ?1),
                    (SELECT state FROM attempts WHERE worker_id = ?1)",
            [&worker],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap()
    };
    let before = rows(&conn);
    let fleet = w.fleet();
    let token = token_file::ensure(&fleet).unwrap();
    let dir = requests_dir(&fleet);
    fs::create_dir_all(&dir).unwrap();
    fs::set_permissions(&dir, Permissions::from_mode(0o700)).unwrap();
    let id = "5b1d2c3e-4f50-4a61-8b72-9c0d1e2f3a4b";
    let line = json!({
        "request_id": id,
        "kind": "spawn_adopt",
        "payload": {
            "run_id": run.run_id, "worker_id": worker, "persona": "opus",
            "target_task_id": task, "spawn_pid": "4242",
        },
        "token": token.text(),
    });
    let path = dir.join(format!("{id}.json"));
    write_0600(&path, &format!("{line}\n"));
    let owner = User::current().unwrap();
    let spool = Spool {
        fleet_root: &fleet,
        owner: &owner,
        token: Some(&token),
        max_bytes: 64 * 1024,
        guard: Generation(1),
    };
    let start = Instant::now();
    let mut now = move || start;
    let mut clock = ScanClock {
        deadline: Deadline::pass(start, 3600),
        now: &mut now,
        stamp: "2026-11-20T00:00:00Z",
    };
    let mut scan_conn = w.conn();
    let outcome = scan(
        &mut scan_conn,
        &spool,
        &mut Cursor::new("2026-11-20T00:00:00Z"),
        &mut clock,
        &w.handlers(),
    )
    .unwrap();
    let ScanOutcome::Read(read) = outcome else {
        panic!("expected a scan of the folder, got {outcome:?}");
    };
    let [fact] = read.facts.as_slice() else {
        panic!("expected 1 fact, got {:?}", read.facts);
    };
    let reason = format!("retired kind: {RECORDS}");
    assert_eq!(fact.handled, Handled::Rejected(reason.clone()));
    assert_eq!(w.requests(), Vec::<(String, String)>::new());
    let ingested: (String, String) = conn
        .query_row(
            "SELECT outcome, reason FROM ingested WHERE path = ?1",
            [path.display().to_string()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(ingested, ("rejected".to_string(), reason));
    let adopted: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM events WHERE cause = 'adopted_by_spawn'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(adopted, 0, "no handler adopted the row");
    let item: (String, String, String) = conn
        .query_row(
            "SELECT kind, entity, state FROM needs_you WHERE entity_id = ?1",
            [path.display().to_string()],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(
        item,
        (
            "request_file_rejected".to_string(),
            "request_file".to_string(),
            "open".to_string()
        )
    );
    assert_eq!(rows(&conn), before, "the refusal changes no row");
}
