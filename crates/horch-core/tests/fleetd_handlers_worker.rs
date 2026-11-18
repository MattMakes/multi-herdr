//! The handlers of the worker kinds: `spawn`, `worker_stop`, `worker_reap`,
//! `worker_new`, `worker_pid`, `orchestrator_claim` and `orchestrator_stop`
//! (docs/tickets/fleetd-p5/19-handlers-worker.md; RR-5, RR-8).
//!
//! Each test sends its request through `requests::handle` to a temp ledger
//! at generation 1 and checks the rows that the command of today writes.

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use horch_core::fleetd::fleet_runtime::adapters::request_wire::{Outcome, Request};
use horch_core::fleetd::fleet_runtime::application::handlers_worker::{register, WorkerDeps};
use horch_core::fleetd::fleet_runtime::application::requests::{handle, Handlers};
use horch_core::fleetd::fleet_runtime::domain::authorize::Decision;
use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;
use horch_core::fleetd::fleet_runtime::domain::ledger::Generation;
use horch_core::fleetd::fleet_runtime::domain::requests::{Kind, RequestId};
use horch_core::fleetd::ideas::application::needs_you::NeedsYouList;
use horch_core::fleetd::task_execution::application::dispatch::CheckSandboxes;
use horch_core::fleetd::task_execution::domain::dispatch::Holds;
use horch_core::taskledger::git::Git;
use horch_core::taskledger::model::{Actor, WorkerKind, WorktreeMode};
use horch_core::taskledger::orchestrator::Holder;
use horch_core::taskledger::runs::{self, NewRun, Run};
use horch_core::taskledger::tasks::{self, NewTask};
use horch_core::taskledger::workers::{self, NewWorker};
use horch_core::taskledger::{schema, util};
use rusqlite::{params, Connection};
use serde_json::{json, Value};

const RUN_BRANCH: &str = "fleet/r1";

struct AllSandboxes;

impl CheckSandboxes for AllSandboxes {
    fn supported(&self, _persona: &str) -> bool {
        true
    }
}

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

/// A git repo with the run branch, a ledger at generation 1 with 1 active
/// run, and the handlers of this module.
fn world() -> World {
    let tmp = tempfile::tempdir().unwrap();
    let top = tmp.path().canonicalize().unwrap();
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
    let deps = WorkerDeps {
        guard: Generation(1),
        config: Arc::new(FleetConfig::default()),
        needs_you: Arc::new(NeedsYouList),
        git: Arc::new(Git::new("git")),
        holds: Arc::new(Holds::default),
        sandboxes: Arc::new(AllSandboxes),
        lock_holder: Arc::new(|_: &str| None::<Holder>),
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

    /// Send 1 new request and give its result; a refusal fails the test.
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

    /// A worker row of the run with the agent `pid`, as a spawn of today
    /// leaves it.
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

    /// The rows that an effect of a request changes, as 1 text.
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
fn spawn_binds_the_task_and_writes_the_rows_of_today() {
    let mut w = world();
    let id = w.add("T", WorktreeMode::Worktree);
    let out = w.send(
        Kind::Spawn,
        json!({ "run_id": w.run.run_id, "persona": "opus", "workspace_id": "ws1" }),
    );
    assert_eq!(out["task_id"], json!(id));
    assert_eq!(out["attempt"], json!(1));
    let wid = out["worker_id"].as_str().unwrap();
    assert_eq!(w.task_status(&id), "leased");
    let t = tasks::get(&w.conn, &id).unwrap();
    assert_eq!(t.current_worker_id.as_deref(), Some(wid));
    assert_eq!(t.attempts, 1);
    let worker = workers::get(&w.conn, wid).unwrap();
    assert_eq!(worker.persona.as_deref(), Some("opus"));
    assert_eq!(worker.kind, "worker");
    assert_eq!(worker.workspace_id.as_deref(), Some("ws1"));
    assert_eq!(worker.task_id.as_deref(), Some(id.as_str()));
    assert_eq!(worker.agent_pid, None, "the pass launches the attempt");
    let (attempt, state, via): (String, String, String) = w
        .conn
        .query_row(
            "SELECT id, state, via FROM attempts WHERE task_id = ?1",
            [&id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(out["attempt_id"], json!(attempt));
    assert_eq!((state.as_str(), via.as_str()), ("launching", "push"));
    let tree = &out["worktree"];
    assert!(tree["path"].as_str().unwrap().contains(".worktrees"));
    assert!(
        tree["add_job"].is_i64(),
        "the job adds the tree after the commit"
    );
    assert!(
        !Path::new(tree["path"].as_str().unwrap()).exists(),
        "the handler makes no tree"
    );

    let why = w.refused(
        Kind::Spawn,
        json!({ "run_id": w.run.run_id, "persona": "opus" }),
    );
    assert!(why.contains("no ready task for persona opus"), "{why}");
}

#[test]
fn spawn_of_a_role_writes_its_row_and_its_no_claim_verdict() {
    let mut w = world();
    let task = w.add("T", WorktreeMode::Shared);
    let out = w.send(
        Kind::Spawn,
        json!({ "run_id": w.run.run_id, "persona": "opus", "controller": "W-ctl" }),
    );
    let wid = out["worker_id"].as_str().unwrap();
    assert_eq!(workers::get(&w.conn, wid).unwrap().status, "spawning");
    assert_eq!(w.task_status(&task), "ready", "a role binds no task");
    let verdicts = w.events(wid, "verdict");
    assert_eq!(verdicts.len(), 1);
    assert!(verdicts[0].starts_with("No claim: a role of controller worker W-ctl"));
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
fn worker_reap_requeues_as_today() {
    let mut w = world();
    let task = w.add("T", WorktreeMode::Shared);
    let wid = w.worker(Some(&task));
    let out = w.send(
        Kind::WorkerReap,
        json!({ "run_id": w.run.run_id, "worker": wid, "reason": "no heartbeat" }),
    );
    assert_eq!(
        out["requeued"],
        json!(tasks::get(&w.conn, &task).unwrap().short())
    );
    let row = workers::get(&w.conn, &wid).unwrap();
    assert_eq!(row.status, "incomplete");
    assert_eq!(row.end_reason.as_deref(), Some("dead: no heartbeat"));
    assert_eq!(w.task_status(&task), "ready");
    let reaped = w.events(&wid, "reaped");
    assert_eq!(reaped.len(), 1);
    assert!(
        reaped[0].ends_with(" reaped: no heartbeat"),
        "{}",
        reaped[0]
    );

    let gone = w.worker(None);
    w.send(
        Kind::WorkerStop,
        json!({ "run_id": w.run.run_id, "worker": gone, "reason": "done" }),
    );
    let why = w.refused(
        Kind::WorkerReap,
        json!({ "run_id": w.run.run_id, "worker": gone, "reason": "again" }),
    );
    assert!(
        why.contains("only a live or incomplete worker is reaped"),
        "{why}"
    );
}

#[test]
fn worker_new_and_worker_pid_write_the_rows_of_today() {
    let mut w = world();
    let out = w.send(
        Kind::WorkerNew,
        json!({ "run_id": w.run.run_id, "persona": "opus", "role": "opus-1", "pane": "p-7" }),
    );
    let wid = out["worker_id"].as_str().unwrap().to_string();
    let row = workers::get(&w.conn, &wid).unwrap();
    assert_eq!(row.kind, "worker");
    assert_eq!(row.persona.as_deref(), Some("opus"));
    assert_eq!(row.role.as_deref(), Some("opus-1"));
    assert_eq!(row.pane_id.as_deref(), Some("p-7"));
    assert_eq!(row.run_id.as_deref(), Some(w.run.run_id.as_str()));

    w.send(
        Kind::WorkerPid,
        json!({ "worker": wid, "supervisor": { "pid": 4000, "started": 11 },
                "agent": { "pid": 4001, "started": 12 } }),
    );
    let row = workers::get(&w.conn, &wid).unwrap();
    assert_eq!(
        (row.supervisor_pid, row.supervisor_started),
        (Some(4000), Some(11))
    );
    assert_eq!((row.agent_pid, row.agent_started), (Some(4001), Some(12)));
}

#[test]
fn orchestrator_claim_and_stop_write_the_rows_of_fleet_start_and_stop() {
    let mut w = world();
    let out = w.send(
        Kind::OrchestratorClaim,
        json!({ "run_id": w.run.run_id, "workspace": "ws1", "pane": "p-1",
                "harness": "claude", "model": "opus" }),
    );
    assert_eq!(out["claim"]["state"], json!("changed"));
    assert!(out["claim"]["text"]
        .as_str()
        .unwrap()
        .starts_with("An orchestrator launch claims run r1: workspace (none) -> ws1"));
    assert_eq!(out["harness_rows"], json!(1));
    let run = runs::get(&w.conn, &w.run.run_id).unwrap();
    assert_eq!(run.workspace_id.as_deref(), Some("ws1"));
    let orch = workers::get(&w.conn, run.orchestrator_worker_id.as_deref().unwrap()).unwrap();
    assert_eq!(orch.kind, "orchestrator");
    assert_eq!(orch.status, "working");
    assert_eq!(orch.pane_id.as_deref(), Some("p-1"));
    assert_eq!(orch.harness.as_deref(), Some("claude"));
    assert_eq!(orch.model.as_deref(), Some("opus"));

    let out = w.send(Kind::OrchestratorStop, json!({ "run_ids": [w.run.run_id] }));
    assert_eq!(
        out["runs"],
        json!([{ "run_id": w.run.run_id, "slug": "r1", "recorded": true }])
    );
    let stopped: Option<String> = w
        .conn
        .query_row(
            "SELECT orchestrator_stopped_at FROM runs WHERE run_id = ?1",
            [&w.run.run_id],
            |r| r.get(0),
        )
        .unwrap();
    assert!(stopped.is_some());
    let out = w.send(Kind::OrchestratorStop, json!({ "run_ids": [w.run.run_id] }));
    assert_eq!(out["runs"][0]["recorded"], json!(false), "recorded before");

    let out = w.send(
        Kind::OrchestratorClaim,
        json!({ "run_id": w.run.run_id, "workspace": "ws1" }),
    );
    assert!(out["claim"]["text"]
        .as_str()
        .unwrap()
        .contains("-> cleared"));
}

#[test]
fn each_kind_twice_with_1_request_id_has_1_effect() {
    let mut w = world();
    w.add("T", WorktreeMode::Worktree);
    let held = w.add("H", WorktreeMode::Shared);
    let stopped = w.worker(Some(&held));
    let reaped = w.worker(None);
    let pid_of = w.worker(None);
    let run = w.run.run_id.clone();
    let cases = [
        (Kind::Spawn, json!({ "run_id": run, "persona": "opus" })),
        (
            Kind::WorkerStop,
            json!({ "run_id": run, "worker": stopped, "reason": "r" }),
        ),
        (
            Kind::WorkerReap,
            json!({ "run_id": run, "worker": reaped, "reason": "r" }),
        ),
        (Kind::WorkerNew, json!({ "run_id": run, "persona": "opus" })),
        (
            Kind::WorkerPid,
            json!({ "worker": pid_of, "supervisor": { "pid": 7, "started": 1 } }),
        ),
        (
            Kind::OrchestratorClaim,
            json!({ "run_id": run, "workspace": "ws1" }),
        ),
        (Kind::OrchestratorStop, json!({ "run_ids": [run] })),
    ];
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
