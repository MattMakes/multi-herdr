//! The handlers of the run and plan request kinds write the rows of the
//! commands of today (docs/tickets/fleetd-p5/17-handlers-run.md; RR-5,
//! RR-8).
//!
//! Each test sends requests through `requests::handle` to a temp ledger at
//! generation 1, with a temp project folder. The test plays the command: it
//! gathers the facts that the command gathers (the sha256 of a file, the
//! import facts) and puts them in the payload.

use std::path::PathBuf;
use std::time::Duration;

use horch_core::fleetd::fleet_runtime::adapters::request_wire::{Outcome, Reply, Request};
use horch_core::fleetd::fleet_runtime::application::handlers_run::{
    operator_only, register, KINDS,
};
use horch_core::fleetd::fleet_runtime::application::ledger::Cause;
use horch_core::fleetd::fleet_runtime::application::requests::{handle, Handlers, NOT_HANDLED};
use horch_core::fleetd::fleet_runtime::domain::authorize::Decision;
use horch_core::fleetd::fleet_runtime::domain::ledger::Generation;
use horch_core::fleetd::fleet_runtime::domain::requests::{Class, Kind, RequestId};
use horch_core::fleetd::worktree_custody::adapters::sqlite::{self as trees, NewWorktree};
use horch_core::taskledger::git::Git;
use horch_core::taskledger::model::{Actor, WorktreeStatus};
use horch_core::taskledger::store::Store;
use horch_core::taskledger::{finish, import, runs, schema, tasks, util};
use rusqlite::{params, Connection};
use serde_json::{json, Value};

const TICKET: &str = "# 01: A\n\n<!-- horch-ticket: 1 -->\n**Persona:** primary `dev` · **Parallel:** yes\n**Kind:** feature\n**Check:** `true`\n**Own:** `src/a.rs`\n\n## Goal\n\nDo A.\n";
const MAP: &str =
    "# Map\n\n| # | Title | Blocked by | Wave |\n|---|---|---|---|\n| 01 | [A](a.md) | | 1 |\n";

/// A temp ledger at generation 1, a temp project folder, and the handlers
/// of this ticket.
struct World {
    temp: tempfile::TempDir,
    conn: Connection,
    handlers: Handlers,
    next_id: u64,
}

fn world() -> World {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join("proj/tickets")).unwrap();
    let mut conn = Connection::open(temp.path().join("ledger.sqlite3")).unwrap();
    conn.busy_timeout(Duration::from_secs(10)).unwrap();
    schema::migrate_to(&mut conn, schema::VERSION).unwrap();
    conn.execute(
        "INSERT INTO fleetd (id, generation, pid, pid_start, started_at, fleet_clock_s, clock_ticked_at)
         VALUES (1, 1, ?1, 0, ?2, 0, ?2)",
        params![std::process::id(), util::now()],
    )
    .unwrap();
    let mut handlers = Handlers::new();
    register(&mut handlers);
    World {
        temp,
        conn,
        handlers,
        next_id: 0,
    }
}

impl World {
    fn root(&self) -> PathBuf {
        self.temp.path().join("proj")
    }

    fn write(&self, rel: &str, text: &str) -> PathBuf {
        let path = self.root().join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, text).unwrap();
        path
    }

    fn request(&mut self, kind: Kind, payload: Value) -> Request {
        self.next_id += 1;
        let Value::Object(payload) = payload else {
            panic!("a payload is an object");
        };
        Request {
            request_id: RequestId::parse(&format!("6f1c2a3e-0b9d-4c55-8e7f-{:012x}", self.next_id))
                .unwrap(),
            kind,
            payload,
            token: Some("t".into()),
        }
    }

    fn send_request(&mut self, req: &Request) -> Reply {
        handle(
            &mut self.conn,
            Generation(1),
            req,
            Some("me"),
            &Decision::Authorized {
                auth: "token; request file".into(),
            },
            &self.handlers,
        )
        .unwrap()
    }

    /// Send 1 new request; its outcome.
    fn send(&mut self, kind: Kind, payload: Value) -> Outcome {
        let req = self.request(kind, payload);
        self.send_request(&req).outcome
    }

    /// Send 1 new request that must apply; its result.
    fn applied(&mut self, kind: Kind, payload: Value) -> Value {
        match self.send(kind, payload) {
            Outcome::Applied(v) => v,
            Outcome::Refused(r) => panic!("{} refused: {r}", kind.as_str()),
        }
    }

    /// Send 1 new request that must be refused; its reason.
    fn refused(&mut self, kind: Kind, payload: Value) -> String {
        match self.send(kind, payload) {
            Outcome::Refused(r) => r,
            Outcome::Applied(v) => panic!("{} applied: {v}", kind.as_str()),
        }
    }

    fn count(&self, sql: &str) -> i64 {
        self.conn.query_row(sql, [], |r| r.get(0)).unwrap()
    }

    /// A new run `slug` with no workspace; its id.
    fn run(&mut self, slug: &str) -> String {
        let payload = json!({
            "project_root": self.root(),
            "slug": slug,
            "run_branch": format!("fleet/{slug}"),
            "base_branch": "main",
            "finish_steps": ["check", "land"],
        });
        let v = self.applied(Kind::RunNew, payload);
        v["run"]["run_id"].as_str().unwrap().to_string()
    }

    /// The ledger row of the gate folder of `run_id`, as the custody job of
    /// `run gate` leaves it.
    fn gate_tree(&self, run_id: &str) {
        let run = runs::get(&self.conn, run_id).unwrap();
        let tx = self.conn.unchecked_transaction().unwrap();
        trees::insert_reserved(
            &tx,
            None,
            &NewWorktree {
                worktree_id: util::new_id(),
                task_id: None,
                worker_id: None,
                repo_root: run.project_root.clone(),
                path: runs::gate_dir(&run).to_string_lossy().into_owned(),
                branch: String::new(),
                base: "main".into(),
                base_commit: None,
                status: WorktreeStatus::Open,
                mode: None,
            },
            Cause {
                actor: Actor::Orchestrator,
                text: "test",
            },
        )
        .unwrap();
        tx.commit().unwrap();
    }

    fn gate(&mut self, run_id: &str, tip: &str, exit: i64) -> Value {
        self.applied(
            Kind::RunGate,
            json!({
                "run_id": run_id,
                "tip_sha": tip,
                "command": "cargo test",
                "exit_code": exit,
                "log_path": "/tmp/gate.log",
                "started_at": util::now(),
                "ended_at": util::now(),
            }),
        )
    }

    fn add_payload(&self, run_id: &str) -> Value {
        json!({
            "run_id": run_id,
            "project_root": self.root(),
            "task_file": "tickets/a.md",
            "ticket_sha256": tasks::ticket_sha(TICKET),
            "primary": "dev",
            "priority": 100,
        })
    }

    fn map_payload(&self, run_id: &str) -> Value {
        json!({
            "run_id": run_id,
            "project_root": self.root(),
            "map_path": self.root().join("tickets/MAP.md"),
            "map_sha256": tasks::ticket_sha(MAP),
            "tickets": { "tickets/a.md": tasks::ticket_sha(TICKET) },
        })
    }

    fn solo_payload(&self) -> Value {
        json!({
            "project_root": self.root(),
            "workspace_id": "w1",
            "title": "Fix the typo",
            "persona": "dev",
            "check": "none",
            "solo": { "run_branch": "fleet/solo-w1", "base_branch": "main" },
            "seat": { "pane_id": "p1" },
        })
    }

    /// An old session ledger and 1 worktree claim under `state/`, and the
    /// import facts that the command gathers.
    fn old_state(&self) -> (PathBuf, Value) {
        let state = self.temp.path().join("state");
        std::fs::create_dir_all(state.join("worktrees")).unwrap();
        let records = json!([
            {"record_id": "a", "session_id": null, "agent": "claude", "tier": "opus",
             "model": "opus", "role": "opus-1", "status": "done", "task": "t1",
             "history": [], "created_at": "2026-01-01T00:00:00Z",
             "updated_at": "2026-01-01T00:00:00Z", "project": "/repo"},
            {"record_id": "b", "session_id": "s-b", "agent": "codex", "tier": "codex-sol",
             "model": "gpt", "role": "codex-sol-1", "status": "working", "task": "t2",
             "history": [], "created_at": "2026-01-02T00:00:00Z",
             "updated_at": "2026-01-02T00:00:00Z"}
        ]);
        std::fs::write(state.join("-repo.json"), records.to_string()).unwrap();
        let claim = json!([{
            "id": "c1", "record_id": "a",
            "path": self.root().join(".worktrees/gone").to_string_lossy(),
            "branch": "fleet/gone", "base": "main", "state": "open"
        }]);
        std::fs::write(state.join("worktrees/claims.json"), claim.to_string()).unwrap();
        let facts = import::gather(&self.conn, &Git::new("git"), &state).unwrap();
        (
            state.clone(),
            json!({ "state_root": state, "facts": facts }),
        )
    }
}

/// The rows of a table that the tests compare.
fn rows(conn: &Connection, sql: &str) -> Vec<Vec<Option<String>>> {
    let mut stmt = conn.prepare(sql).unwrap();
    let n = stmt.column_count();
    stmt.query_map([], |r| {
        (0..n)
            .map(|i| r.get::<_, rusqlite::types::Value>(i))
            .map(|v| {
                v.map(|v| match v {
                    rusqlite::types::Value::Null => None,
                    rusqlite::types::Value::Integer(i) => Some(i.to_string()),
                    rusqlite::types::Value::Text(t) => Some(t),
                    other => Some(format!("{other:?}")),
                })
            })
            .collect()
    })
    .unwrap()
    .collect::<rusqlite::Result<_>>()
    .unwrap()
}

#[test]
fn run_new_writes_the_run_row_of_today() {
    let mut w = world();
    let v = w.applied(
        Kind::RunNew,
        json!({
            "project_root": w.root(),
            "slug": "r1",
            "workspace_id": "w1",
            "run_branch": "fleet/r1",
            "base_branch": "main",
            "remote": "origin",
            "finish_steps": ["check", "land"],
            "thresholds": { "max_attempts": 5 },
            "check": "cargo test",
            "seat": { "pane_id": "p1", "harness": ["claude", "opus"] },
        }),
    );
    let run_id = v["run"]["run_id"].as_str().unwrap();
    let run = runs::get(&w.conn, run_id).unwrap();
    assert_eq!(
        (
            run.slug.as_str(),
            run.status.as_str(),
            run.workspace_id.as_deref(),
            run.run_branch.as_deref(),
            run.base_branch.as_deref(),
            run.remote.as_deref(),
            run.max_attempts,
        ),
        (
            "r1",
            "active",
            Some("w1"),
            Some("fleet/r1"),
            Some("main"),
            Some("origin"),
            5
        )
    );
    assert_eq!(run.finish_steps, ["check", "land"]);
    assert_eq!(v["run"]["slug"], "r1");
    assert_eq!(
        finish::default_check(&w.conn, run_id).unwrap().as_deref(),
        Some("cargo test")
    );
    let orch = run.orchestrator_worker_id.clone().unwrap();
    let worker: (String, String, String, Option<String>, Option<String>, bool) = w
        .conn
        .query_row(
            "SELECT kind, status, pane_id, harness, model, started_at IS NOT NULL
             FROM workers WHERE worker_id = ?1",
            [&orch],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(
        worker,
        (
            "orchestrator".into(),
            "working".into(),
            "p1".into(),
            Some("claude".into()),
            Some("opus".into()),
            true
        )
    );

    // TRF-34: a 2nd run of the workspace is refused, and writes no row.
    let reason = w.refused(
        Kind::RunNew,
        json!({
            "project_root": w.root(), "slug": "r2", "workspace_id": "w1",
            "finish_steps": [],
        }),
    );
    assert!(reason.contains("r1"), "{reason}");
    assert_eq!(w.count("SELECT count(*) FROM runs"), 1);

    // An orchestrator with no record takes the harness of the workspace.
    let v = w.applied(
        Kind::RunNew,
        json!({ "project_root": w.root(), "slug": "r3", "finish_steps": [] }),
    );
    let run = runs::get(&w.conn, v["run"]["run_id"].as_str().unwrap()).unwrap();
    assert_eq!(run.workspace_id, None);
}

#[test]
fn run_close_and_run_gate_write_the_rows_of_today() {
    let mut w = world();
    let run_id = w.run("r1");
    w.gate_tree(&run_id);

    let v = w.gate(&run_id, "abc123", 1);
    assert_eq!(v["passed"], false);
    assert_eq!(v["gate"]["tip_sha"], "abc123");
    let v = w.gate(&run_id, "abc123", 0);
    assert_eq!(v["passed"], true);
    assert_eq!(
        rows(
            &w.conn,
            "SELECT tip_sha, command, exit_code, log_path FROM run_gates ORDER BY gate_id"
        ),
        [
            ["abc123", "cargo test", "1", "/tmp/gate.log"].map(|s| Some(s.to_string())),
            ["abc123", "cargo test", "0", "/tmp/gate.log"].map(|s| Some(s.to_string())),
        ]
    );
    assert_eq!(
        runs::last_gate(&w.conn, &run_id, "abc123")
            .unwrap()
            .unwrap()
            .exit_code,
        Some(0)
    );

    // A settled run needs the C5 facts for a close as done.
    let reason = w.refused(Kind::RunClose, json!({ "run_id": run_id }));
    assert!(reason.contains("the tip and the PR"), "{reason}");
    // C5: the tip of the run branch has no passing gate.
    let reason = w.refused(
        Kind::RunClose,
        json!({
            "run_id": run_id,
            "guards": { "tip": "def456", "pr": { "state": "not_applicable", "why": "no remote" } },
        }),
    );
    assert!(reason.contains("def456"), "{reason}");
    assert_eq!(runs::get(&w.conn, &run_id).unwrap().status, "active");

    let v = w.applied(
        Kind::RunClose,
        json!({
            "run_id": run_id,
            "guards": { "tip": "abc123", "pr": { "state": "not_applicable", "why": "no remote" } },
        }),
    );
    assert_eq!(v["closed"]["status"], "done");
    assert_eq!(v["run"]["status"], "done");
    assert!(runs::get(&w.conn, &run_id).unwrap().ended_at.is_some());

    // --abandon cancels each open task and closes the run as abandoned.
    let run2 = w.run("r2");
    w.write("tickets/a.md", TICKET);
    let payload = w.add_payload(&run2);
    w.applied(Kind::TaskAdd, payload);
    let v = w.applied(
        Kind::RunClose,
        json!({ "run_id": run2, "abandon": "wrong plan" }),
    );
    assert_eq!(v["closed"]["status"], "abandoned");
    assert_eq!(v["closed"]["cancelled"][0][0], "T-1");
    assert_eq!(
        rows(
            &w.conn,
            &format!("SELECT status FROM tasks WHERE run_id = '{run2}'")
        ),
        [[Some("cancelled".to_string())]]
    );
}

#[test]
fn task_add_queues_1_task_with_its_ticket_snapshot() {
    let mut w = world();
    let run_id = w.run("r1");
    w.write("tickets/a.md", TICKET);
    let payload = w.add_payload(&run_id);
    let v = w.applied(Kind::TaskAdd, payload);
    assert_eq!(v["task"]["short"], "T-1");
    assert_eq!(v["task"]["claimable_by"], "dev");
    assert_eq!(v["task"]["title"], "A");
    let t = tasks::of_run(&w.conn, &run_id).unwrap().remove(0);
    assert_eq!(t.task_file.as_deref(), Some("tickets/a.md"));
    assert_eq!(t.land_check.as_deref(), Some("true"));
    assert_eq!(t.status, "ready");
    // `horch ledger add` keeps no snapshot today; the facts come from the
    // ticket that the sha256 names.
    assert_eq!((t.ticket_snapshot, t.ticket_sha), (None, None));
    assert_eq!(
        (t.work_kind.as_deref(), t.review_mode.as_deref()),
        (Some("feature"), Some("none"))
    );

    // A ticket that changed after the command read it is refused.
    w.write("tickets/a.md", "# 01: Changed\n");
    let payload = w.add_payload(&run_id);
    let reason = w.refused(Kind::TaskAdd, payload);
    assert!(
        reason.contains("changed after the request was sent"),
        "{reason}"
    );
    assert_eq!(w.count("SELECT count(*) FROM tasks"), 1);
}

#[test]
fn map_add_queues_each_row_of_the_map() {
    let mut w = world();
    let run_id = w.run("r1");
    w.write("tickets/a.md", TICKET);
    w.write("tickets/MAP.md", MAP);
    let payload = w.map_payload(&run_id);
    let v = w.applied(Kind::MapAdd, payload);
    assert_eq!(v["tasks"][0]["short"], "T-1");
    assert_eq!(v["tasks"][0]["claimable_by"], "dev");
    let t = tasks::of_run(&w.conn, &run_id).unwrap().remove(0);
    assert_eq!(t.task_file.as_deref(), Some("tickets/a.md"));
    assert_eq!(t.ticket_snapshot.as_deref(), Some(TICKET));
    assert_eq!(t.ticket_sha, Some(tasks::ticket_sha(TICKET)));

    // TRF-13: the same map again is refused, unless `again`.
    let payload = w.map_payload(&run_id);
    let reason = w.refused(Kind::MapAdd, payload);
    assert!(
        reason.contains("tickets/a.md is already T-1 (queued) in this run."),
        "{reason}"
    );
    let mut payload = w.map_payload(&run_id);
    payload["again"] = json!(true);
    w.applied(Kind::MapAdd, payload);
    assert_eq!(w.count("SELECT count(*) FROM tasks"), 2);
}

#[test]
fn map_add_with_a_changed_ticket_sha_is_refused() {
    let mut w = world();
    let run_id = w.run("r1");
    w.write("tickets/a.md", TICKET);
    w.write("tickets/MAP.md", MAP);
    let payload = w.map_payload(&run_id);
    w.write("tickets/a.md", &TICKET.replace("Do A.", "Do B."));
    let reason = w.refused(Kind::MapAdd, payload);
    assert!(
        reason.contains("the ticket tickets/a.md changed after the request was sent"),
        "{reason}"
    );
    // A changed map is refused too.
    let mut payload = w.map_payload(&run_id);
    payload["map_sha256"] = json!(tasks::ticket_sha("another map"));
    let reason = w.refused(Kind::MapAdd, payload);
    assert!(
        reason.contains("changed after the request was sent"),
        "{reason}"
    );
    // A request that names another set of tickets is refused.
    let mut payload = w.map_payload(&run_id);
    payload["tickets"]["tickets/b.md"] = json!(tasks::ticket_sha("b"));
    let reason = w.refused(Kind::MapAdd, payload);
    assert!(
        reason.contains("are not the tickets of the request"),
        "{reason}"
    );
    assert_eq!(w.count("SELECT count(*) FROM tasks"), 0);
}

#[test]
fn task_solo_creates_the_solo_run_when_absent() {
    let mut w = world();
    let payload = w.solo_payload();
    let v = w.applied(Kind::TaskSolo, payload);
    assert_eq!(v["created_run"], true);
    assert_eq!(v["run"]["slug"], runs::solo_slug("w1"));
    assert_eq!(v["task"]["short"], "T-1");
    assert_eq!(v["primary"], "dev");
    assert_eq!(v["no_check"], false);
    let run = runs::active_solo(&w.conn, &w.root(), "w1")
        .unwrap()
        .unwrap();
    assert_eq!(run.run_branch.as_deref(), Some("fleet/solo-w1"));
    assert!(runs::is_solo(&run));
    let t = tasks::of_run(&w.conn, &run.run_id).unwrap().remove(0);
    assert_eq!(t.task_file.as_deref(), Some(tasks::ONE_OFF_TICKET));
    assert_eq!(t.ticket_snapshot.as_deref(), Some("# Fix the typo\n"));
    assert!(t.check_none);

    // The 2nd one-off task goes to the same solo run.
    let mut payload = w.solo_payload();
    payload["title"] = json!("Fix the 2nd typo");
    payload.as_object_mut().unwrap().remove("check");
    let v = w.applied(Kind::TaskSolo, payload);
    assert_eq!(v["created_run"], false);
    assert_eq!(v["run"]["run_id"], run.run_id);
    assert_eq!(v["no_check"], true);
    assert_eq!(w.count("SELECT count(*) FROM runs"), 1);
}

#[test]
fn plan_workers_writes_the_plan_of_today() {
    let mut w = world();
    let run_id = w.run("r1");
    w.write("tickets/a.md", TICKET);
    let payload = w.add_payload(&run_id);
    w.applied(Kind::TaskAdd, payload);

    let v = w.applied(Kind::PlanWorkers, json!({ "run_id": run_id }));
    assert_eq!(v["adopted"], false);
    assert_eq!(
        v["plan"],
        serde_json::to_value(tasks::plan_workers(&w.conn, &run_id).unwrap()).unwrap()
    );
    assert_eq!(v["plan"][0]["persona"], "dev");
    assert_eq!(v["plan"][0]["spawn"], 1);

    // In the pane of the orchestrator, a run with no workspace adopts it.
    let v = w.applied(
        Kind::PlanWorkers,
        json!({ "run_id": run_id, "workspace_id": "w9" }),
    );
    assert_eq!(v["adopted"], true);
    assert_eq!(
        runs::get(&w.conn, &run_id).unwrap().workspace_id.as_deref(),
        Some("w9")
    );
    let v = w.applied(
        Kind::PlanWorkers,
        json!({ "run_id": run_id, "workspace_id": "w8" }),
    );
    assert_eq!(v["adopted"], false);
    assert_eq!(v["run"]["workspace_id"], "w9");
}

/// The tables that an import writes.
const IMPORT_TABLES: [&str; 5] = [
    "SELECT source_kind, source_key, mapped_to IS NOT NULL, map_error FROM import_raw ORDER BY raw_id",
    "SELECT record_id FROM session_records ORDER BY record_id",
    "SELECT persona, status, legacy_record_id FROM workers ORDER BY legacy_record_id",
    "SELECT title, status FROM tasks ORDER BY title",
    "SELECT kind, detail FROM import_findings ORDER BY finding_id",
];

#[test]
fn ledger_import_and_gc_write_the_rows_of_today() {
    let mut w = world();
    let (state, payload) = w.old_state();
    let v = w.applied(Kind::LedgerImport, payload);
    assert_eq!(v["report"]["sessions"], 2);
    assert_eq!(v["report"]["worktrees"], 1);

    // The command of today, on a twin ledger, writes the same rows.
    let twin = w.temp.path().join("twin.sqlite3");
    let mut store = Store::open(&twin).unwrap();
    let today = import::import(&mut store, &Git::new("git"), &state, false).unwrap();
    assert_eq!(serde_json::to_value(&today).unwrap(), v["report"]);
    for sql in IMPORT_TABLES {
        assert_eq!(rows(&w.conn, sql), rows(store.conn(), sql), "{sql}");
    }

    // An old file that changed after the facts were read is refused.
    let (_, payload) = w.old_state();
    std::fs::write(state.join("-repo.json"), "[]").unwrap();
    let reason = w.refused(Kind::LedgerImport, payload);
    assert!(
        reason.contains("changed after the import facts were read"),
        "{reason}"
    );

    // gc deletes git refs, which are no ledger rows: the request records
    // them and writes no other row.
    let events = w.count("SELECT count(*) FROM events");
    let v = w.applied(
        Kind::LedgerGc,
        json!({ "deleted": ["refs/horch/trash/a/20260101T000000Z"] }),
    );
    assert_eq!(v["deleted"][0], "refs/horch/trash/a/20260101T000000Z");
    assert_eq!(w.count("SELECT count(*) FROM events"), events);
}

/// The count of rows of each table that a handler of this file writes,
/// and the workspace of each run.
fn effects(conn: &Connection) -> (Vec<i64>, Option<String>) {
    let counts = [
        "runs",
        "workers",
        "tasks",
        "task_deps",
        "run_gates",
        "events",
        "import_raw",
        "session_records",
        "import_findings",
        "worktrees",
    ]
    .iter()
    .map(|t| {
        conn.query_row(&format!("SELECT count(*) FROM {t}"), [], |r| r.get(0))
            .unwrap()
    })
    .collect();
    let workspaces = conn
        .query_row("SELECT group_concat(workspace_id) FROM runs", [], |r| {
            r.get(0)
        })
        .unwrap();
    (counts, workspaces)
}

#[test]
fn each_kind_twice_with_1_request_id_has_1_effect() {
    for kind in KINDS {
        let mut w = world();
        let run_id = w.run("r1");
        w.write("tickets/a.md", TICKET);
        w.write("tickets/MAP.md", MAP);
        let payload = match kind {
            Kind::RunNew => json!({
                "project_root": w.root(), "slug": "r2", "finish_steps": [],
            }),
            Kind::RunClose => json!({ "run_id": run_id, "abandon": "wrong plan" }),
            Kind::RunGate => {
                w.gate_tree(&run_id);
                json!({
                    "run_id": run_id, "tip_sha": "abc", "command": "true",
                    "exit_code": 0, "started_at": util::now(),
                })
            }
            Kind::TaskAdd => w.add_payload(&run_id),
            Kind::MapAdd => w.map_payload(&run_id),
            Kind::TaskSolo => w.solo_payload(),
            Kind::PlanWorkers => json!({ "run_id": run_id, "workspace_id": "w9" }),
            Kind::LedgerImport => w.old_state().1,
            Kind::LedgerGc => json!({ "deleted": ["refs/horch/trash/a/x"] }),
            other => panic!("{} is not a kind of this file", other.as_str()),
        };
        let req = w.request(kind, payload);
        let before = effects(&w.conn);
        let first = w.send_request(&req);
        assert!(
            matches!(first.outcome, Outcome::Applied(_)),
            "{}: {first:?}",
            kind.as_str()
        );
        let once = effects(&w.conn);
        assert!(
            kind == Kind::LedgerGc || once != before,
            "{} wrote no row",
            kind.as_str()
        );
        let second = w.send_request(&req);
        assert_eq!(second, first, "{}", kind.as_str());
        assert_eq!(effects(&w.conn), once, "{} applied 2 times", kind.as_str());
        assert_eq!(
            w.count("SELECT count(*) FROM requests"),
            2,
            "{}",
            kind.as_str()
        );
    }
}

#[test]
fn each_kind_of_this_file_is_an_operator_kind_with_a_handler() {
    let mut w = world();
    for kind in KINDS {
        assert_eq!(kind.class(), Class::Operator, "{}", kind.as_str());
        let reason = w.refused(kind, json!({ "unknown": 1 }));
        assert_ne!(reason, NOT_HANDLED, "{}", kind.as_str());
        assert!(
            reason.contains("the payload does not fit its kind"),
            "{reason}"
        );
    }
}

#[test]
fn a_handler_with_client_facts_refuses_a_worker_kind() {
    let mut w = world();
    let mut handlers = Handlers::new();
    handlers.insert(
        Kind::Progress,
        operator_only(Kind::Progress, |_, _| Ok(json!({}))),
    );
    w.handlers = handlers;
    let reason = w.refused(Kind::Progress, json!({}));
    assert!(reason.contains("not an operator kind"), "{reason}");
}
