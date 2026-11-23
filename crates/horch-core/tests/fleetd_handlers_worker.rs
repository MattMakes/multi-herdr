#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use horch_core::fleetd::fleet_runtime::adapters::clock_store;
use horch_core::fleetd::fleet_runtime::adapters::request_wire::{Outcome, Request};
use horch_core::fleetd::fleet_runtime::application::handlers_worker::{register, WorkerDeps};
use horch_core::fleetd::fleet_runtime::application::jobs::JobCtx;
use horch_core::fleetd::fleet_runtime::application::requests::{handle, Handlers};
use horch_core::fleetd::fleet_runtime::domain::authorize::Decision;
use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;
use horch_core::fleetd::fleet_runtime::domain::ledger::Generation;
use horch_core::fleetd::fleet_runtime::domain::requests::{Kind, RequestId};
use horch_core::fleetd::ideas::application::needs_you::NeedsYouList;
use horch_core::fleetd::task_execution::application::dispatch::{self, DispatchCtx};
use horch_core::taskledger::git::Git;
use horch_core::taskledger::model::{Actor, WorkerKind, WorktreeMode};
use horch_core::taskledger::runs::{self, NewRun, Run};
use horch_core::taskledger::tasks::{self, NewTask};
use horch_core::taskledger::workers::{self, NewWorker};
use horch_core::taskledger::{schema, util};
use rusqlite::{params, Connection};
use serde_json::{json, Value};

const RUN_BRANCH: &str = "fleet/r1";

struct World {
    _tmp: tempfile::TempDir,
    conn: Connection,
    run: Run,
    handlers: Handlers,
    sent: u32,
}

fn git(dir: &Path, args: &[&str]) {
    let o = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "-c",
            "commit.gpgSign=false",
            "-c",
            "core.hooksPath=/dev/null",
        ])
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("LC_ALL", "C")
        .output()
        .unwrap();
    assert!(o.status.success(), "git {}", args.join(" "));
}

fn world() -> World {
    let tmp = tempfile::tempdir().unwrap();
    let top = horch_platform::path::canonicalize(tmp.path()).unwrap();
    let root: PathBuf = top.join("repo");
    std::fs::create_dir_all(&root).unwrap();
    git(&root, &["init", "--quiet", "-b", "main"]);
    std::fs::write(root.join(".gitignore"), ".worktrees/\n").unwrap();
    git(&root, &["add", ".gitignore"]);
    git(&root, &["commit", "--quiet", "-m", "base"]);
    git(&root, &["branch", RUN_BRANCH]);
    let mut conn = Connection::open(top.join("ledger.sqlite3")).unwrap();
    conn.busy_timeout(Duration::from_secs(10)).unwrap();
    schema::migrate_to(&mut conn, schema::VERSION).unwrap();
    conn.execute(
        "INSERT INTO fleetd (id, generation, pid, pid_start, started_at, fleet_clock_s, clock_ticked_at)
         VALUES (1, 1, ?1, 0, ?2, 0, ?2)",
        params![std::process::id(), util::now()],
    )
    .unwrap();
    let run = runs::create(
        &conn,
        &root,
        &NewRun {
            slug: "r1".into(),
            base_branch: Some("main".into()),
            run_branch: Some(RUN_BRANCH.into()),
            ..Default::default()
        },
    )
    .unwrap();
    horch_core::taskledger::schema::run_idea(
        &conn,
        &run.run_id,
        &horch_core::taskledger::util::now(),
        true,
    )
    .unwrap();
    let deps = WorkerDeps {
        guard: Generation(1),
    };
    let mut handlers = Handlers::new();
    register(&mut handlers, &deps);
    World {
        _tmp: tmp,
        conn,
        run,
        handlers,
        sent: 0,
    }
}

impl World {
    fn bind(&mut self, task: &str) -> String {
        let tx = self.conn.transaction().unwrap();
        let config = FleetConfig::default();
        let git = Git::new("git");
        let ctx = DispatchCtx {
            jobs: JobCtx {
                guard: Some(Generation(1)),
                config: &config,
                needs_you: &NeedsYouList,
            },
            clock: clock_store::read(&tx).unwrap(),
            git: &git,
        };
        let bound = dispatch::bind(&tx, &ctx, task, "opus", &NewWorker::default()).unwrap();
        tx.commit().unwrap();
        bound.worker_id
    }

    fn add(&self, title: &str, mode: WorktreeMode) -> String {
        let mut t = NewTask::work(title, &format!("docs/{title}.md"), "opus");
        t.ticket_snapshot = Some("# A ticket\n".into());
        t.worktree_mode = mode;
        tasks::add(&self.conn, &self.run, &t, Actor::Orchestrator)
            .unwrap()
            .task_id
    }

    fn request(&mut self, kind: Kind, payload: Value) -> Request {
        self.sent += 1;
        let Value::Object(payload) = payload else {
            panic!("a payload is an object");
        };
        Request {
            request_id: RequestId::parse(&format!("00000000-0000-4000-8000-{:012}", self.sent))
                .unwrap(),
            kind,
            payload,
            token: Some("t".into()),
        }
    }

    fn send_request(&mut self, req: &Request) -> Outcome {
        handle(
            &mut self.conn,
            Generation(1),
            req,
            Some("me"),
            &Decision::Authorized {
                auth: "token; channel".into(),
            },
            &self.handlers,
        )
        .unwrap()
        .outcome
    }

    fn send(&mut self, kind: Kind, payload: Value) -> Value {
        let req = self.request(kind, payload);
        match self.send_request(&req) {
            Outcome::Applied(v) => v,
            Outcome::Refused(why) => panic!("{} refused: {why}", kind.as_str()),
        }
    }

    fn refused(&mut self, kind: Kind, payload: Value) -> String {
        let req = self.request(kind, payload);
        match self.send_request(&req) {
            Outcome::Refused(why) => why,
            Outcome::Applied(v) => panic!("{} applied: {v}", kind.as_str()),
        }
    }

    fn task_status(&self, id: &str) -> String {
        tasks::get(&self.conn, id).unwrap().status
    }

    fn events(&self, worker: &str, kind: &str) -> Vec<String> {
        let mut stmt = self
            .conn
            .prepare("SELECT text FROM events WHERE worker_id = ?1 AND kind = ?2 ORDER BY event_id")
            .unwrap();
        stmt.query_map(params![worker, kind], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    }

    fn worker(&self, task: Option<&str>) -> String {
        let id = workers::insert(
            &self.conn,
            &NewWorker {
                run_id: self.run.run_id.clone(),
                kind: Some(WorkerKind::Worker),
                persona: "opus".into(),
                ..Default::default()
            },
        )
        .unwrap();
        workers::agent_started(&self.conn, &id, 4001, Some(99)).unwrap();
        if let Some(t) = task {
            self.conn
                .execute(
                    "UPDATE tasks SET status = 'running', current_worker_id = ?2 WHERE task_id = ?1",
                    params![t, id],
                )
                .unwrap();
            self.conn
                .execute(
                    "UPDATE workers SET task_id = ?2 WHERE worker_id = ?1",
                    params![id, t],
                )
                .unwrap();
        }
        id
    }

    fn rows(&self) -> String {
        let mut out = String::new();
        for sql in [
            "SELECT worker_id, status, task_id, attempt, agent_pid, supervisor_pid, harness, end_reason FROM workers ORDER BY worker_id",
            "SELECT task_id, status, attempts, current_worker_id FROM tasks ORDER BY task_id",
            "SELECT id, state FROM attempts ORDER BY id",
            "SELECT worktree_id, status FROM worktrees ORDER BY worktree_id",
            "SELECT run_id, workspace_id, orchestrator_stopped_at FROM runs ORDER BY run_id",
            "SELECT count(*) FROM events",
        ] {
            let mut stmt = self.conn.prepare(sql).unwrap();
            let n = stmt.column_count();
            let rows = stmt
                .query_map([], |r| {
                    let mut line = Vec::new();
                    for i in 0..n {
                        line.push(format!("{:?}", r.get_ref(i)?));
                    }
                    Ok(line.join("|"))
                })
                .unwrap();
            for row in rows {
                out.push_str(&row.unwrap());
                out.push('\n');
            }
        }
        out
    }
}

#[test]
fn worker_stop_records_an_intended_stop_and_signals_nothing() {
    let mut w = world();
    let task = w.add("T", WorktreeMode::Shared);
    let wid = w.worker(Some(&task));
    let out = w.send(
        Kind::WorkerStop,
        json!({ "run_id": w.run.run_id, "worker": wid, "reason": "wrong plan" }),
    );
    assert_eq!(
        out["requeued"],
        json!(tasks::get(&w.conn, &task).unwrap().short())
    );
    let row = workers::get(&w.conn, &wid).unwrap();
    assert_eq!(row.status, "stopped");
    assert_eq!(row.end_reason.as_deref(), Some("wrong plan"));
    assert_eq!(
        (row.agent_pid, row.agent_started),
        (Some(4001), Some(99)),
        "the pass stops the process by its recorded PID and start time"
    );
    assert_eq!(w.task_status(&task), "ready");
    let stopped = w.events(&wid, "stopped");
    assert_eq!(stopped.len(), 1);
    assert!(stopped[0].starts_with("Stopping ") && stopped[0].ends_with(": wrong plan"));
    let stops: i64 = w
        .conn
        .query_row("SELECT count(*) FROM stops", [], |r| r.get(0))
        .unwrap();
    assert_eq!(stops, 0, "no stop step ran: the handler signals nothing");

    let orch = workers::insert(
        &w.conn,
        &NewWorker {
            run_id: w.run.run_id.clone(),
            kind: Some(WorkerKind::Orchestrator),
            persona: "orchestrator".into(),
            ..Default::default()
        },
    )
    .unwrap();
    let why = w.refused(
        Kind::WorkerStop,
        json!({ "run_id": w.run.run_id, "worker": orch, "reason": "x" }),
    );
    assert!(why.contains("does not stop the orchestrator"), "{why}");
}

#[test]
fn each_kind_twice_with_1_request_id_has_1_effect() {
    let mut w = world();
    w.add("T", WorktreeMode::Worktree);
    let held = w.add("H", WorktreeMode::Shared);
    let stopped = w.worker(Some(&held));
    let run = w.run.run_id.clone();
    let cases = [(
        Kind::WorkerStop,
        json!({ "run_id": run, "worker": stopped, "reason": "r" }),
    )];
    for (kind, payload) in cases {
        let req = w.request(kind, payload);
        let first = w.send_request(&req);
        assert!(
            matches!(first, Outcome::Applied(_)),
            "{}: {first:?}",
            kind.as_str()
        );
        let rows = w.rows();
        let second = w.send_request(&req);
        assert_eq!(second, first, "{}: the stored result", kind.as_str());
        assert_eq!(w.rows(), rows, "{}: a second effect", kind.as_str());
    }
}

fn worker_of_other_run(w: &World) -> String {
    let root = std::path::PathBuf::from(&w.run.project_root);
    let other = runs::create(
        &w.conn,
        &root,
        &NewRun {
            slug: "r2".into(),
            ..Default::default()
        },
    )
    .unwrap();
    horch_core::taskledger::schema::run_idea(
        &w.conn,
        &other.run_id,
        &horch_core::taskledger::util::now(),
        true,
    )
    .unwrap();
    workers::insert(
        &w.conn,
        &NewWorker {
            run_id: other.run_id,
            kind: Some(WorkerKind::Worker),
            persona: "opus".into(),
            ..Default::default()
        },
    )
    .unwrap()
}

#[test]
fn spawn_gate_note_writes_1_note_about_the_run_and_the_worker() {
    let mut w = world();
    let wid = w.worker(None);
    let text = "spawn gate: teammate=opus outcome=NOTE pool=claude reason=near the limit";
    w.send(
        Kind::SpawnGateNote,
        json!({ "run_id": w.run.run_id, "worker_id": wid, "text": text }),
    );
    assert_eq!(w.events(&wid, "note"), [text]);
    w.send(Kind::SpawnGateNote, json!({ "text": text }));
    let notes: i64 = w
        .conn
        .query_row(
            "SELECT count(*) FROM events WHERE kind = 'note' AND text = ?1",
            [text],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(notes, 2, "a spawn outside each run records its note too");
    let why = w.refused(Kind::SpawnGateNote, json!({ "text": "free text" }));
    assert!(why.contains("does not start with `spawn gate: `"), "{why}");
    let other = worker_of_other_run(&w);
    let why = w.refused(
        Kind::SpawnGateNote,
        json!({ "run_id": w.run.run_id, "worker_id": other, "text": text }),
    );
    assert!(why.contains("is not a worker of run"), "{why}");
}

#[test]
fn worker_launched_records_the_pane_and_the_launch() {
    let mut w = world();
    let wid = w.worker(None);
    w.send(
        Kind::WorkerLaunched,
        json!({
            "run_id": w.run.run_id, "worker_id": wid, "pane": "p-7", "role": "opus-3",
            "harness": "claude", "model": "m1", "effort": "high",
        }),
    );
    type Cols = Option<String>;
    let row: (Cols, Cols, Cols, Cols, Cols) = w
        .conn
        .query_row(
            "SELECT pane_id, role, harness, model, effort FROM workers WHERE worker_id = ?1",
            [&wid],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .unwrap();
    assert_eq!(
        row,
        (
            Some("p-7".into()),
            Some("opus-3".into()),
            Some("claude".into()),
            Some("m1".into()),
            Some("high".into())
        )
    );
    let why = w.refused(
        Kind::WorkerLaunched,
        json!({
            "run_id": w.run.run_id, "worker_id": "W-none", "pane": "p", "role": "r",
            "harness": "claude", "model": "m",
        }),
    );
    assert!(why.contains("no worker row has the id W-none"), "{why}");
    let why = w.refused(
        Kind::WorkerLaunched,
        json!({ "run_id": w.run.run_id, "worker_id": wid, "pane": "p" }),
    );
    assert!(why.contains("no payload field `role`"), "{why}");
}

#[test]
fn worker_launch_failed_ends_an_unclaimed_row() {
    let mut w = world();
    let wid = workers::insert(
        &w.conn,
        &NewWorker {
            run_id: w.run.run_id.clone(),
            kind: Some(WorkerKind::Worker),
            persona: "opus".into(),
            ..Default::default()
        },
    )
    .unwrap();
    w.send(
        Kind::WorkerLaunchFailed,
        json!({ "run_id": w.run.run_id, "worker_id": wid, "reason": "REFUSED: pool closed" }),
    );
    assert_eq!(workers::get(&w.conn, &wid).unwrap().status, "launch_failed");
    let other = worker_of_other_run(&w);
    let why = w.refused(
        Kind::WorkerLaunchFailed,
        json!({ "run_id": w.run.run_id, "worker_id": other, "reason": "x" }),
    );
    assert!(why.contains("is not a worker of run"), "{why}");
    assert_eq!(workers::get(&w.conn, &other).unwrap().status, "spawning");
}

#[test]
fn worker_launch_failed_of_a_push_attempt_ends_it_crashed_and_queues_the_task() {
    let mut w = world();
    let task = w.add("T", WorktreeMode::Shared);
    let wid = w.bind(&task);
    let attempt = format!("{task}/A1");
    w.send(
        Kind::WorkerLaunchFailed,
        json!({ "run_id": w.run.run_id, "worker_id": wid, "reason": "no record matches 'x'" }),
    );
    assert_eq!(workers::get(&w.conn, &wid).unwrap().status, "launch_failed");
    let (state, outcome): (String, Option<String>) = w
        .conn
        .query_row(
            "SELECT state, outcome FROM attempts WHERE id = ?1",
            [&attempt],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(
        (state.as_str(), outcome.as_deref()),
        ("ended", Some("crashed")),
        "{}",
        w.rows()
    );
    let t = tasks::get(&w.conn, &task).unwrap();
    assert_eq!(
        (t.status.as_str(), t.attempts),
        ("ready", 1),
        "{}",
        w.rows()
    );
    assert_eq!(t.current_worker_id, None, "{}", w.rows());

    w.send(
        Kind::WorkerLaunchFailed,
        json!({ "run_id": w.run.run_id, "worker_id": wid, "reason": "again" }),
    );
    let ended: i64 = w
        .conn
        .query_row(
            "SELECT COUNT(*) FROM attempts WHERE task_id = ?1 AND state = 'ended'",
            [&task],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(ended, 1, "{}", w.rows());
    assert_eq!(
        tasks::get(&w.conn, &task).unwrap().attempts,
        1,
        "{}",
        w.rows()
    );
}
