//! The ingest of `result.json` and of the question files of a push attempt
//! (docs/tickets/fleetd-p4/13-ingest-result-and-questions.md).
//!
//! `docs/specs/fleetd/design.md`, "Workspace layout and file protocol"
//! (protocol rule 7 and its size table), "Ledger" (`ingested`, I1), "Order
//! inside 1 pass", SR-6, FM-6 and FM-23. Each test uses a temp ledger and a
//! temp fleet root. Part 1 of a pass reads the outbox files, and part 2
//! ingests them in 1 transaction, as the pass does.
#![cfg(unix)]

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use horch_core::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;
use horch_core::fleetd::fleet_runtime::domain::ledger::Generation;
use horch_core::fleetd::task_execution::adapters::result_reader::AttemptRef;
use horch_core::fleetd::task_execution::application::ingest::{self, Report};
use horch_core::fleetd::task_execution::application::lease;
use horch_core::fleetd::task_execution::domain::lease::Limits;
use horch_core::fleetd::task_execution::domain::result::{ResultFile, ResultStatus};
use horch_core::taskledger::model::{Actor, WorkerKind, WorktreeMode};
use horch_core::taskledger::runs::{self, NewRun, Run};
use horch_core::taskledger::tasks::{self, NewTask};
use horch_core::taskledger::testing::bind_live;
use horch_core::taskledger::workers::NewWorker;
use horch_core::taskledger::{schema, util};
use rusqlite::{params, Connection};

const GUARD: Generation = Generation(1);

/// A temp ledger at generation 1 and fleet clock 0, 1 run and a fleet root.
struct World {
    dir: PathBuf,
    _temp: tempfile::TempDir,
    conn: Connection,
    run: Run,
    max_bytes: u64,
}

fn world() -> World {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().to_path_buf();
    let mut conn = Connection::open(dir.join("ledger.sqlite3")).unwrap();
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
        &dir,
        &NewRun {
            slug: "r1".into(),
            ..Default::default()
        },
    )
    .unwrap();
    // FR-12: a task needs an idea in flight of the feature of its run.
    horch_core::taskledger::schema::run_idea(
        &conn,
        &run.run_id,
        &horch_core::taskledger::util::now(),
        true,
    )
    .unwrap();
    std::fs::create_dir_all(dir.join("fleet")).unwrap();
    World {
        dir,
        _temp: temp,
        conn,
        run,
        max_bytes: FleetConfig::default().result_max_bytes,
    }
}

impl World {
    fn root(&self) -> PathBuf {
        self.dir.join("fleet")
    }

    /// A task bound to a worker at fleet clock 0, with its 1 live push
    /// attempt `A1` and its lease.
    fn running_task(&self) -> AttemptRef {
        let n: i64 = self
            .conn
            .query_row("SELECT count(*) FROM tasks", [], |r| r.get(0))
            .unwrap();
        let mut new = NewTask::work(&format!("t{n}"), &format!("tickets/t{n}.md"), "dev");
        new.worktree_mode = WorktreeMode::Shared;
        let task = tasks::add(&self.conn, &self.run, &new, Actor::Orchestrator).unwrap();
        let worker = NewWorker {
            run_id: self.run.run_id.clone(),
            kind: Some(WorkerKind::Worker),
            persona: "dev".into(),
            ..Default::default()
        };
        let bound = bind_live(&self.conn, &task.task_id, &worker, None).unwrap();
        let attempt_id = bound.attempt_id;
        assert_eq!(attempt_id, format!("{}/A1", task.task_id));
        AttemptRef {
            task_id: task.task_id,
            attempt_id,
        }
    }

    fn outbox(&self, a: &AttemptRef) -> PathBuf {
        let dir = self
            .root()
            .join("tasks")
            .join(&a.task_id)
            .join("attempts")
            .join("A1")
            .join("outbox");
        std::fs::create_dir_all(&dir).unwrap();
        set_mode(&dir, 0o700);
        dir
    }

    /// Part 1 and part 2 of 1 pass for `attempts`.
    fn pass(&mut self, attempts: &[AttemptRef]) -> Report {
        let reads = ingest::read(&self.conn, &self.root(), attempts, self.max_bytes).unwrap();
        let tx = self.conn.transaction().unwrap();
        let report = ingest::ingest(&tx, Some(GUARD), &reads).unwrap();
        tx.commit().unwrap();
        report
    }

    fn status(&self, a: &AttemptRef) -> String {
        tasks::get(&self.conn, &a.task_id).unwrap().status
    }

    fn burned(&self, a: &AttemptRef) -> i64 {
        tasks::get(&self.conn, &a.task_id).unwrap().attempts
    }

    /// The `ingested` rows: path, sha256, outcome and reason.
    fn ingested(&self) -> Vec<(String, Option<String>, String, Option<String>)> {
        let mut stmt = self
            .conn
            .prepare("SELECT path, sha256, outcome, reason FROM ingested ORDER BY path")
            .unwrap();
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    }

    fn events_of(&self, a: &AttemptRef) -> i64 {
        self.conn
            .query_row(
                "SELECT count(*) FROM events WHERE task_id = ?1",
                [&a.task_id],
                |r| r.get(0),
            )
            .unwrap()
    }

    fn set_clock(&self, s: i64) {
        self.conn
            .execute("UPDATE fleetd SET fleet_clock_s = ?1 WHERE id = 1", [s])
            .unwrap();
    }
}

/// The text of a valid `result.json` of task `task`, attempt `attempt`.
fn result_json(task: &str, attempt: &str, summary: &str) -> String {
    ResultFile {
        task: task.into(),
        attempt: attempt.into(),
        status: ResultStatus::Completed,
        summary: summary.into(),
        commits: vec!["3f9c2ab".into()],
        files_changed: vec!["src/lib.rs".into()],
        acceptance_self_check: Vec::new(),
        notes_for_next: None,
        handoff: None,
    }
    .to_json()
}

fn question_json(task: &str, attempt: &str) -> String {
    serde_json::json!({
        "task": task,
        "attempt": attempt,
        "question_id": "Q-1",
        "text": "Which table owns the cursor?",
        "at": "2026-11-18T10:00:00Z",
    })
    .to_string()
}

/// Write `text` to `path` with mode 0600, as the outbox writer does: the
/// protected file reader refuses a wider mode (ticket 29 of fleetd-p5).
fn write_0600(path: impl AsRef<std::path::Path>, text: impl AsRef<[u8]>) {
    std::fs::write(&path, text).unwrap();
    set_mode(path.as_ref(), 0o600);
}

fn set_mode(path: &std::path::Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
}

fn write_result(w: &World, a: &AttemptRef) -> PathBuf {
    let path = w.outbox(a).join("result.json");
    write_0600(&path, result_json(&a.task_id, "A1", "done"));
    path
}

fn assert_rejected(w: &World, path: &std::path::Path, check: &str) {
    let rows = w.ingested();
    assert_eq!(rows.len(), 1, "{rows:?}");
    let (p, _, outcome, reason) = &rows[0];
    assert_eq!(p, &path.display().to_string());
    assert_eq!(outcome, "rejected");
    let reason = reason.as_deref().unwrap_or_default();
    assert!(
        reason.starts_with(&format!("{check}:")),
        "the reason names the check {check}: {reason}"
    );
}

#[test]
fn fm_result_file_alone_leaves_task_verifying() {
    let mut w = world();
    let a = w.running_task();
    let path = write_result(&w, &a);

    let report = w.pass(std::slice::from_ref(&a));
    assert_eq!(report.to_verifying, vec![a.task_id.clone()]);
    assert_eq!(w.status(&a), "verifying");
    for _ in 0..3 {
        w.pass(std::slice::from_ref(&a));
    }
    assert_eq!(
        w.status(&a),
        "verifying",
        "only the verifier moves a task to done"
    );

    let bytes = std::fs::read(&path).unwrap();
    let rows = w.ingested();
    assert_eq!(
        rows,
        vec![(
            path.display().to_string(),
            Some(horch_core::digest::sha256_bytes(&bytes).hex()),
            "accepted".to_string(),
            None
        )]
    );
}

#[test]
fn symlinked_result_is_rejected_and_not_read_again() {
    let mut w = world();
    let a = w.running_task();
    let real = w.dir.join("elsewhere.json");
    write_0600(&real, result_json(&a.task_id, "A1", "done"));
    let path = w.outbox(&a).join("result.json");
    std::os::unix::fs::symlink(&real, &path).unwrap();

    let report = w.pass(std::slice::from_ref(&a));
    assert_eq!(report.rejected, vec![path.clone()]);
    assert_rejected(&w, &path, "no_link");
    assert_eq!(w.status(&a), "running");

    // A valid file at the same path later is not read: the path is done.
    std::fs::remove_file(&path).unwrap();
    write_0600(&path, result_json(&a.task_id, "A1", "done"));
    let report = w.pass(std::slice::from_ref(&a));
    assert_eq!(report, Report::default());
    assert_rejected(&w, &path, "no_link");
    assert_eq!(w.status(&a), "running");
}

#[test]
fn oversized_result_is_rejected() {
    let mut w = world();
    let a = w.running_task();
    let path = w.outbox(&a).join("result.json");
    let summary = "x".repeat(usize::try_from(w.max_bytes).unwrap());
    write_0600(&path, result_json(&a.task_id, "A1", &summary));

    w.pass(std::slice::from_ref(&a));
    assert_rejected(&w, &path, "size");
    assert_eq!(w.status(&a), "running");
}

#[test]
fn result_with_the_attempt_id_of_another_attempt_is_rejected() {
    let mut w = world();
    let a = w.running_task();
    let path = w.outbox(&a).join("result.json");
    write_0600(&path, result_json(&a.task_id, "A2", "done"));

    w.pass(std::slice::from_ref(&a));
    assert_rejected(&w, &path, "attempt_id");
    assert_eq!(w.status(&a), "running");
}

#[test]
fn result_outside_its_outbox_is_rejected() {
    let mut w = world();
    let a = w.running_task();
    // The outbox is a link to a folder of the worker, so the real path of
    // the file is outside the outbox of its attempt.
    let outbox = w.outbox(&a);
    std::fs::remove_dir(&outbox).unwrap();
    let other = w.dir.join("other-outbox");
    std::fs::create_dir_all(&other).unwrap();
    std::fs::write(
        other.join("result.json"),
        result_json(&a.task_id, "A1", "done"),
    )
    .unwrap();
    std::os::unix::fs::symlink(&other, &outbox).unwrap();
    let path = outbox.join("result.json");

    w.pass(std::slice::from_ref(&a));
    assert_rejected(&w, &path, "real_path");
    assert_eq!(w.status(&a), "running");
}

#[test]
fn same_result_is_ingested_once() {
    let mut w = world();
    let a = w.running_task();
    write_result(&w, &a);

    let first = w.pass(std::slice::from_ref(&a));
    assert_eq!(first.to_verifying, vec![a.task_id.clone()]);
    let events = w.events_of(&a);
    let second = w.pass(std::slice::from_ref(&a));
    assert_eq!(second, Report::default());
    assert_eq!(w.ingested().len(), 1);
    assert_eq!(w.events_of(&a), events, "the second pass writes nothing");
}

#[test]
fn result_wins_over_a_lease_expiry_in_the_same_pass() {
    let mut w = world();
    let with_result = w.running_task();
    let without = w.running_task();
    write_result(&w, &with_result);
    let limits = |_: &str| Limits {
        lease_s: 120,
        stall_after_s: 300,
        heartbeat_max_age_s: 30,
    };

    // 1 pass far past both leases: ingest first, then the leases.
    let clock = FleetSeconds(10_000);
    w.set_clock(clock.0);
    let attempts = [with_result.clone(), without.clone()];
    let reads = ingest::read(&w.conn, &w.root(), &attempts, w.max_bytes).unwrap();
    let tx = w.conn.transaction().unwrap();
    ingest::ingest(&tx, Some(GUARD), &reads).unwrap();
    let expired =
        lease::renew_or_expire_leases(&tx, Some(GUARD), clock, &BTreeMap::new(), &limits).unwrap();
    tx.commit().unwrap();

    let expired: Vec<&str> = expired.iter().map(|e| e.task_id.as_str()).collect();
    assert_eq!(expired, vec![without.task_id.as_str()]);
    assert_eq!(w.status(&with_result), "verifying");
    assert_eq!(w.status(&without), "stalled");

    // A result of a stalled task moves it to verifying too.
    write_result(&w, &without);
    let report = w.pass(std::slice::from_ref(&without));
    assert_eq!(report.to_verifying, vec![without.task_id.clone()]);
    assert_eq!(w.status(&without), "verifying");
}

#[test]
fn question_file_moves_the_task_to_blocked_on_user_and_burns_no_attempt() {
    let mut w = world();
    let a = w.running_task();
    let burned = w.burned(&a);
    let questions = w.outbox(&a).join("questions");
    std::fs::create_dir_all(&questions).unwrap();
    set_mode(&questions, 0o700);
    write_0600(questions.join("1.json"), question_json(&a.task_id, "A1"));

    let report = w.pass(std::slice::from_ref(&a));
    assert_eq!(report.to_blocked, vec![a.task_id.clone()]);
    assert_eq!(w.status(&a), "blocked_on_user");
    assert_eq!(w.burned(&a), burned, "a question burns no attempt");
    let (state, outcome): (String, Option<String>) = w
        .conn
        .query_row(
            "SELECT state, outcome FROM attempts WHERE id = ?1",
            [&a.attempt_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(
        (state.as_str(), outcome.as_deref()),
        ("ended", Some("asked"))
    );
    let rows = w.ingested();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].2, "accepted");
}

#[test]
fn tmp_file_is_not_read() {
    let mut w = world();
    let a = w.running_task();
    let outbox = w.outbox(&a);
    std::fs::write(
        outbox.join("result.json.tmp"),
        result_json(&a.task_id, "A1", "done"),
    )
    .unwrap();
    std::fs::create_dir_all(outbox.join("questions")).unwrap();
    std::fs::write(
        outbox.join("questions").join("1.json.tmp"),
        question_json(&a.task_id, "A1"),
    )
    .unwrap();

    let report = w.pass(std::slice::from_ref(&a));
    assert_eq!(report, Report::default());
    assert!(w.ingested().is_empty());
    assert_eq!(w.status(&a), "running");
}
