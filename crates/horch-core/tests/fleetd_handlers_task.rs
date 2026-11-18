//! The fleetd handlers of the task kinds: `task_requeue`, `task_cancel`,
//! `task_unblock`, `task_fallback`, `ledger_waive`, `ledger_reopen` and
//! `ledger_dismiss` (docs/tickets/fleetd-p5/18-handlers-task.md; RR-5,
//! RR-8).
//!
//! Each test sends 1 request through `requests::handle` to a temp ledger at
//! generation 1, and checks the rows that the command of today writes.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use horch_core::fleetd::fleet_runtime::adapters::request_wire::{Outcome, Reply, Request};
use horch_core::fleetd::fleet_runtime::application::handlers_task::{self, TaskHandlers};
use horch_core::fleetd::fleet_runtime::application::requests::{handle, Handlers};
use horch_core::fleetd::fleet_runtime::domain::authorize::Decision;
use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;
use horch_core::fleetd::fleet_runtime::domain::ledger::Generation;
use horch_core::fleetd::fleet_runtime::domain::requests::{Kind, RequestId};
use horch_core::fleetd::task_execution::domain::task::Recorded;
use horch_core::taskledger::model::Actor;
use horch_core::taskledger::runs::{self, NewRun, Run};
use horch_core::taskledger::tasks::{self, NewTask};
use horch_core::taskledger::testing;
use horch_core::taskledger::workers::NewWorker;
use horch_core::taskledger::{util, Store};
use rusqlite::{params, Connection};
use serde_json::{json, Value};

const ID: &str = "6f1c2a3e-0b9d-4c55-8e7f-1a2b3c4d5e6f";

/// A temp ledger at generation 1 with the run `r1` and 2 tasks: T-1 of
/// `dev` with the fallback `alt`, and T-2 that waits for T-1.
struct World {
    _dir: tempfile::TempDir,
    store: Store,
    conn: Connection,
    root: PathBuf,
    evidence: PathBuf,
    run: Run,
    t1: String,
    t2: String,
    handlers: Handlers,
}

fn world() -> World {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("project");
    std::fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let evidence = dir.path().join("evidence");
    let db = dir.path().join("ledger.sqlite3");
    let mut store = Store::open(&db).unwrap();
    let new_run = NewRun {
        slug: "r1".into(),
        ..Default::default()
    };
    let run = store
        .immediate(|tx| {
            let run = runs::create(tx, &root, &new_run)?;
            // FR-12: a task needs an idea in flight of the feature of its run.
            horch_core::taskledger::schema::run_idea(
                tx,
                &run.run_id,
                &horch_core::taskledger::util::now(),
                true,
            )?;
            Ok(run)
        })
        .unwrap();
    let (t1, t2) = store
        .immediate(|tx| {
            let mut a = NewTask::work("a", "tickets/a.md", "dev");
            a.fallback = Some("alt".into());
            let t1 = tasks::add(tx, &run, &a, Actor::Orchestrator)?;
            let mut b = NewTask::work("b", "tickets/b.md", "dev");
            b.blocked_by = vec!["T-1".into()];
            let t2 = tasks::add(tx, &run, &b, Actor::Orchestrator)?;
            Ok((t1.task_id, t2.task_id))
        })
        .unwrap();
    store
        .conn()
        .execute(
            "INSERT INTO fleetd (id, generation, pid, pid_start, started_at, fleet_clock_s, clock_ticked_at)
             VALUES (1, 1, ?1, 0, ?2, 0, ?2)",
            params![std::process::id(), util::now()],
        )
        .unwrap();
    let conn = Connection::open(&db).unwrap();
    conn.busy_timeout(Duration::from_secs(10)).unwrap();
    let mut handlers = Handlers::new();
    handlers_task::register(
        &mut handlers,
        TaskHandlers {
            config: Arc::new(FleetConfig::default()),
            evidence_root: evidence.clone(),
        },
    );
    World {
        _dir: dir,
        store,
        conn,
        root,
        evidence,
        run,
        t1,
        t2,
        handlers,
    }
}

impl World {
    /// The payload `args` with the project root and the run of the world.
    fn payload(&self, args: Value) -> Value {
        let mut p = json!({ "project": self.root, "run": "r1" });
        for (k, v) in args.as_object().unwrap() {
            p[k] = v.clone();
        }
        p
    }

    fn send_as(&mut self, request_id: &str, kind: Kind, args: Value) -> Reply {
        let Value::Object(payload) = self.payload(args) else {
            unreachable!("a payload is an object");
        };
        let req = Request {
            request_id: RequestId::parse(request_id).unwrap(),
            kind,
            payload,
            token: Some("t".into()),
        };
        let decision = Decision::Authorized {
            auth: "token; channel".into(),
        };
        handle(
            &mut self.conn,
            Generation(1),
            &req,
            Some("me"),
            &decision,
            &self.handlers,
        )
        .unwrap()
    }

    fn send(&mut self, kind: Kind, args: Value) -> Reply {
        self.send_as(ID, kind, args)
    }

    fn sql(&self, statement: &str) {
        self.store.conn().execute(statement, []).unwrap();
    }

    /// Each row of `sql`, its columns joined with `|`.
    fn rows(&self, sql: &str) -> Vec<String> {
        let mut stmt = self.conn.prepare(sql).unwrap();
        let n = stmt.column_count();
        stmt.query_map([], |r| {
            (0..n)
                .map(|i| {
                    let v: rusqlite::types::Value = r.get(i)?;
                    Ok(match v {
                        rusqlite::types::Value::Null => "-".to_string(),
                        rusqlite::types::Value::Integer(i) => i.to_string(),
                        rusqlite::types::Value::Real(f) => f.to_string(),
                        rusqlite::types::Value::Text(t) => t,
                        rusqlite::types::Value::Blob(_) => "<blob>".to_string(),
                    })
                })
                .collect::<rusqlite::Result<Vec<_>>>()
                .map(|cols| cols.join("|"))
        })
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
    }

    fn one(&self, sql: &str) -> String {
        let rows = self.rows(sql);
        assert_eq!(rows.len(), 1, "{sql}: {rows:?}");
        rows[0].clone()
    }

    /// fleetd binds T-1 to a worker of `dev` with a recorded agent process: 1 live
    /// attempt. No process runs: the handler reads only the rows.
    fn claimed(&mut self) -> String {
        let new = NewWorker {
            run_id: self.run.run_id.clone(),
            persona: "dev".into(),
            pane_id: Some("w0:p1".into()),
            ..Default::default()
        };
        // The project is no git repository: T-1 works in the shared
        // checkout, so its bind reserves no tree.
        self.store
            .conn()
            .execute(
                "UPDATE tasks SET worktree_mode = 'shared' WHERE task_id = ?1",
                [&self.t1],
            )
            .unwrap();
        let agent = Recorded {
            pid: 4_000_001,
            pid_start: Some(7),
        };
        testing::bind_live(self.store.conn(), &self.t1, &new, Some(agent))
            .unwrap()
            .worker_id
    }

    /// 1 open finding of the kind `ended_without_done` about T-1.
    fn finding(&self) -> i64 {
        self.store
            .conn()
            .execute(
                "INSERT INTO import_findings (kind, task_id, detail, status, created_at)
                 VALUES ('ended_without_done', ?1, 'it ended', 'open', 'now')",
                [&self.t1],
            )
            .unwrap();
        self.store.conn().last_insert_rowid()
    }

    /// Round 1 of the code sweep of T-1, whose report names F1.
    fn code_sweep_round(&self) {
        let task_dir = self.evidence.join(&self.t1);
        let round = task_dir.join("round-1");
        std::fs::create_dir_all(&round).unwrap();
        std::fs::write(
            round.join("report.md"),
            "# Code sweep\n\n- F1 [Important] a finding\n",
        )
        .unwrap();
        let state = json!({
            "round": 1,
            "tree": "abc",
            "dir": round,
            "reviews": [],
        });
        std::fs::write(
            task_dir.join("code-sweep-state.json"),
            serde_json::to_vec(&state).unwrap(),
        )
        .unwrap();
    }
}

fn applied(reply: &Reply) -> &Value {
    match &reply.outcome {
        Outcome::Applied(result) => result,
        other => panic!("not applied: {other:?}"),
    }
}

fn refused(reply: &Reply) -> &str {
    match &reply.outcome {
        Outcome::Refused(reason) => reason,
        other => panic!("not refused: {other:?}"),
    }
}

fn lines(result: &Value) -> Vec<&str> {
    result["lines"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l.as_str().unwrap())
        .collect()
}

#[test]
fn task_requeue_resets_the_attempt_count_as_today() {
    let mut w = world();
    w.sql(
        "UPDATE tasks SET status = 'escalated', attempts = 3, held_reason = 'stuck' WHERE seq = 1",
    );
    let reply = w.send(
        Kind::TaskRequeue,
        json!({ "task": "T-1", "reason": "try again" }),
    );
    let result = applied(&reply);
    assert_eq!(lines(result), ["T-1 is queued again."]);
    assert_eq!(result["task"], "T-1");
    assert_eq!(
        w.one("SELECT status, attempts, attempt_floor, held_reason FROM tasks WHERE seq = 1"),
        "ready|3|3|-"
    );
    assert_eq!(
        w.one(&format!(
            "SELECT from_state, to_state FROM events WHERE entity_id = '{}' AND to_state = 'ready'",
            w.t1
        )),
        "escalated|ready"
    );
    // A task that is not held is refused with the words of today, and
    // nothing changes.
    let reply = w.send_as(
        "7f1c2a3e-0b9d-4c55-8e7f-1a2b3c4d5e6f",
        Kind::TaskRequeue,
        json!({ "task": "T-2", "reason": "x" }),
    );
    assert!(
        refused(&reply).contains("only a held task is requeued"),
        "{reply:?}"
    );
    assert_eq!(w.one("SELECT status FROM tasks WHERE seq = 2"), "pending");
}

#[test]
fn task_cancel_of_a_running_task_records_the_intended_stop_first() {
    let mut w = world();
    let worker = w.claimed();
    let reply = w.send(
        Kind::TaskCancel,
        json!({ "task": "T-1", "reason": "dropped" }),
    );
    let result = applied(&reply);
    assert_eq!(result["status"], "stopping");
    assert_eq!(result["held"], json!([worker]));
    assert_eq!(
        lines(result),
        ["T-1 is not cancelled yet: a process of its attempt still runs. The next pass of fleetd cancels it when the process is gone."]
    );
    // The intended stop: the attempt ends `killed`, the worker row ends
    // `stopped`, the task lets go of its worker and waits with
    // `cancel_requested`. The task is not `cancelled` before the process
    // is gone, and the handler stops no process.
    assert_eq!(
        w.one(
            "SELECT status, cancel_requested, current_worker_id IS NULL FROM tasks WHERE seq = 1"
        ),
        "running|dropped|1"
    );
    assert_eq!(w.one("SELECT outcome FROM attempts"), "killed");
    assert_eq!(
        w.one(&format!(
            "SELECT status FROM workers WHERE worker_id = '{worker}'"
        )),
        "stopped"
    );
    assert_eq!(w.one("SELECT count(*) FROM leases"), "0");
}

#[test]
fn task_cancel_of_a_queued_task_cancels_it_at_once() {
    let mut w = world();
    let reply = w.send(
        Kind::TaskCancel,
        json!({ "task": "T-2", "reason": "not needed" }),
    );
    let result = applied(&reply);
    assert_eq!(result["status"], "cancelled");
    assert_eq!(lines(result), ["T-2 cancelled."]);
    assert_eq!(
        w.one("SELECT status, summary FROM tasks WHERE seq = 2"),
        "cancelled|cancelled: not needed"
    );
}

#[test]
fn task_unblock_and_task_fallback_write_the_rows_of_today() {
    let mut w = world();
    let reply = w.send(Kind::TaskUnblock, json!({ "task": "T-2", "from": "T-1" }));
    assert_eq!(lines(applied(&reply)), ["T-2 no longer waits for T-1."]);
    assert_eq!(
        w.one(&format!(
            "SELECT count(*) FROM task_deps WHERE task_id = '{}'",
            w.t2
        )),
        "0"
    );
    assert_eq!(w.one("SELECT status FROM tasks WHERE seq = 2"), "ready");

    let reply = w.send_as(
        "7f1c2a3e-0b9d-4c55-8e7f-1a2b3c4d5e6f",
        Kind::TaskFallback,
        json!({ "task": "T-1", "reason": "dev is busy" }),
    );
    let result = applied(&reply);
    assert_eq!(lines(result), ["T-1 can now be claimed by alt."]);
    assert_eq!(
        w.one("SELECT claimable_by, fallback_reason FROM tasks WHERE seq = 1"),
        "alt|dev is busy"
    );
    assert_eq!(
        w.one(&format!(
            "SELECT text FROM events WHERE task_id = '{}' AND kind = 'fallback'",
            w.t1
        )),
        "T-1 moves to fallback alt: dev is busy"
    );
}

#[test]
fn ledger_waive_records_the_waiver_of_today() {
    let mut w = world();
    w.code_sweep_round();
    let reply = w.send(
        Kind::LedgerWaive,
        json!({ "task": "T-1", "finding": "f1", "reason": "out of scope" }),
    );
    assert_eq!(
        lines(applied(&reply)),
        ["T-1 round 1: F1 waived. The next horch ledger finish of T-1 reads it."]
    );
    let waivers: Value = serde_json::from_slice(
        &std::fs::read(w.evidence.join(&w.t1).join("code-sweep-waivers.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        waivers,
        json!([{ "round": 1, "id": "F1", "reason": "out of scope" }])
    );
    assert_eq!(
        w.one(&format!(
            "SELECT text FROM events WHERE task_id = '{}' AND kind = 'verdict'",
            w.t1
        )),
        "T-1 code-sweep round 1: F1 waived: out of scope"
    );
    // A finding that the round does not name is refused, as today.
    let reply = w.send_as(
        "7f1c2a3e-0b9d-4c55-8e7f-1a2b3c4d5e6f",
        Kind::LedgerWaive,
        json!({ "task": "T-1", "finding": "F9", "reason": "x" }),
    );
    assert!(refused(&reply).contains("has no finding F9"), "{reply:?}");
}

#[test]
fn ledger_reopen_and_dismiss_write_the_rows_of_today() {
    let mut w = world();
    let first = w.finding();
    let reply = w.send(
        Kind::LedgerReopen,
        json!({
            "finding": first,
            "file": "tickets/c.md",
            "title": "c again",
            "primary": "dev",
            "fallback": "alt",
            "check": "cargo test",
        }),
    );
    let result = applied(&reply);
    assert_eq!(
        lines(result),
        [format!("T-3 queued for dev from finding F{first}.").as_str()]
    );
    assert_eq!(
        w.one("SELECT title, task_file, claimable_by, fallback_persona, land_check, spec_ref FROM tasks WHERE seq = 3"),
        format!("c again|tickets/c.md|dev|alt|cargo test|finding F{first}")
    );
    let t3 = w.one("SELECT task_id FROM tasks WHERE seq = 3");
    assert_eq!(
        w.one(&format!(
            "SELECT status, decided_by, decision_note FROM import_findings WHERE finding_id = {first}"
        )),
        format!("requeued|orchestrator|{t3}")
    );

    let second = w.finding();
    let reply = w.send_as(
        "7f1c2a3e-0b9d-4c55-8e7f-1a2b3c4d5e6f",
        Kind::LedgerDismiss,
        json!({ "finding": second, "reason": "not real", "cleanup": false }),
    );
    assert_eq!(
        lines(applied(&reply)),
        [format!("Finding F{second} dismissed.").as_str()]
    );
    assert_eq!(
        w.one(&format!(
            "SELECT status, decided_by, decision_note FROM import_findings WHERE finding_id = {second}"
        )),
        "dismissed|orchestrator|not real"
    );
}

#[test]
fn ledger_dismiss_cleanup_of_a_finding_with_no_worktree_is_refused() {
    let mut w = world();
    let f = w.finding();
    let reply = w.send(
        Kind::LedgerDismiss,
        json!({ "finding": f, "reason": "x", "cleanup": true }),
    );
    assert!(
        refused(&reply).contains("has no recorded worktree"),
        "{reply:?}"
    );
    assert_eq!(
        w.one(&format!(
            "SELECT status FROM import_findings WHERE finding_id = {f}"
        )),
        "open"
    );
}

/// Send `kind` with `args` 2 times with 1 `request_id`. The second reply
/// is the stored reply of the first, and `effect` counts 1 effect.
fn twice(w: &mut World, kind: Kind, args: Value, effect: &str, want: &str) {
    let first = w.send(kind, args.clone());
    let second = w.send(kind, args);
    assert_eq!(first, second, "{kind:?}");
    applied(&first);
    assert_eq!(w.one(effect), want, "{kind:?}");
    w.sql("DELETE FROM requests");
}

#[test]
fn each_kind_twice_with_1_request_id_has_1_effect() {
    let mut w = world();
    let requeued = |w: &World| {
        format!(
            "SELECT count(*) FROM events WHERE entity_id = '{}' AND to_state = 'ready'",
            w.t1
        )
    };

    w.sql("UPDATE tasks SET status = 'escalated', attempts = 3 WHERE seq = 1");
    let q = requeued(&w);
    twice(
        &mut w,
        Kind::TaskRequeue,
        json!({ "task": "T-1", "reason": "r" }),
        &q,
        "1",
    );

    let fb = format!(
        "SELECT count(*) FROM events WHERE task_id = '{}' AND kind = 'fallback'",
        w.t1
    );
    twice(
        &mut w,
        Kind::TaskFallback,
        json!({ "task": "T-1", "reason": "r" }),
        &fb,
        "1",
    );

    let unblocked = format!("SELECT count(*) FROM task_deps WHERE task_id = '{}'", w.t2);
    twice(
        &mut w,
        Kind::TaskUnblock,
        json!({ "task": "T-2", "from": "T-1" }),
        &unblocked,
        "0",
    );

    w.code_sweep_round();
    let waived = format!(
        "SELECT count(*) FROM events WHERE task_id = '{}' AND kind = 'verdict'",
        w.t1
    );
    twice(
        &mut w,
        Kind::LedgerWaive,
        json!({ "task": "T-1", "finding": "F1", "reason": "r" }),
        &waived,
        "1",
    );

    let f = w.finding();
    twice(
        &mut w,
        Kind::LedgerReopen,
        json!({ "finding": f, "file": "tickets/c.md", "title": "c", "primary": "dev" }),
        "SELECT count(*) FROM tasks WHERE title = 'c'",
        "1",
    );

    let f = w.finding();
    twice(
        &mut w,
        Kind::LedgerDismiss,
        json!({ "finding": f, "reason": "r", "cleanup": false }),
        &format!("SELECT status FROM import_findings WHERE finding_id = {f}"),
        "dismissed",
    );

    let cancelled = format!(
        "SELECT count(*) FROM events WHERE entity_id = '{}' AND to_state = 'cancelled'",
        w.t2
    );
    twice(
        &mut w,
        Kind::TaskCancel,
        json!({ "task": "T-2", "reason": "r" }),
        &cancelled,
        "1",
    );
}

/// The project root of a payload must be absolute: fleetd has no current
/// folder of the caller.
#[test]
fn relative_project_root_is_refused() {
    let mut w = world();
    let Value::Object(mut payload) = w.payload(json!({ "task": "T-2", "reason": "r" })) else {
        unreachable!();
    };
    payload.insert("project".into(), json!("project"));
    let req = Request {
        request_id: RequestId::parse(ID).unwrap(),
        kind: Kind::TaskCancel,
        payload,
        token: None,
    };
    let reply = handle(
        &mut w.conn,
        Generation(1),
        &req,
        None,
        &Decision::Authorized { auth: "a".into() },
        &w.handlers,
    )
    .unwrap();
    assert!(refused(&reply).contains("absolute"), "{reply:?}");
}
