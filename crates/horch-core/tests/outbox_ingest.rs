//! The ingest of the outbox files of an attempt: `progress.jsonl` from its
//! recorded offset, the handoff result, and the owner and mode checks of
//! each file (docs/tickets/fleetd-p5/29-ingest-outbox-files.md).
//!
//! `docs/specs/fleetd/design.md`, "Workspace layout and file protocol"
//! (protocol rules 1, 2 and 7), SR-6, RR-8 and FM-23;
//! `docs/tickets/fleetd-p5/MAP.md`, Q3 and Q4. Each test uses a temp ledger
//! and a temp fleet root. Part 1 of a pass reads the outbox files, and
//! part 2 ingests them in 1 transaction, as the pass does. The owner of the
//! ledger comes through the `owner` seam, because a test cannot make a file
//! of another user.
#![cfg(unix)]

use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;
use horch_core::fleetd::fleet_runtime::domain::ledger::Generation;
use horch_core::fleetd::task_execution::adapters::result_reader::AttemptRef;
use horch_core::fleetd::task_execution::application::outbox_ingest::{
    self, Limits, OutboxRead, Report,
};
use horch_core::fleetd::task_execution::domain::outbox::{handoff_result, LineKind, ProgressLine};
use horch_core::fleetd::task_execution::domain::result::{ResultFile, ResultStatus};
use horch_core::taskledger::model::{Actor, WorkerKind};
use horch_core::taskledger::runs::{self, NewRun, Run};
use horch_core::taskledger::tasks::{self, ClaimOutcome, NewTask};
use horch_core::taskledger::workers::{self, NewWorker};
use horch_core::taskledger::{schema, util};
use horch_platform::channel::User;
use rusqlite::{params, Connection};

const GUARD: Generation = Generation(1);

/// The fleet clock of the `fleetd` row in each test.
const CLOCK: i64 = 4242;

/// A temp ledger at generation 1 and fleet clock [`CLOCK`], 1 run and a
/// fleet root.
struct World {
    dir: PathBuf,
    _temp: tempfile::TempDir,
    conn: Connection,
    run: Run,
    limits: Limits,
    owner: User,
}

fn world() -> World {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().to_path_buf();
    let mut conn = Connection::open(dir.join("ledger.sqlite3")).unwrap();
    conn.busy_timeout(Duration::from_secs(10)).unwrap();
    schema::migrate_to(&mut conn, schema::VERSION).unwrap();
    conn.execute(
        "INSERT INTO fleetd (id, generation, pid, pid_start, started_at, fleet_clock_s, clock_ticked_at)
         VALUES (1, 1, ?1, 0, ?2, ?3, ?2)",
        params![std::process::id(), util::now(), CLOCK],
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
    fs::create_dir_all(dir.join("fleet")).unwrap();
    World {
        dir,
        _temp: temp,
        conn,
        run,
        limits: Limits::of(&FleetConfig::default()),
        owner: User::current().unwrap(),
    }
}

impl World {
    fn root(&self) -> PathBuf {
        self.dir.join("fleet")
    }

    /// A task that a worker claimed, with its 1 live push attempt `A1`.
    fn running_task(&self) -> AttemptRef {
        let n: i64 = self
            .conn
            .query_row("SELECT count(*) FROM tasks", [], |r| r.get(0))
            .unwrap();
        let new = NewTask::work(&format!("t{n}"), &format!("tickets/t{n}.md"), "dev");
        let task = tasks::add(&self.conn, &self.run, &new, Actor::Orchestrator).unwrap();
        let worker = workers::insert(
            &self.conn,
            &NewWorker {
                run_id: self.run.run_id.clone(),
                kind: Some(WorkerKind::Worker),
                persona: "dev".into(),
                ..Default::default()
            },
        )
        .unwrap();
        self.conn
            .execute(
                "UPDATE workers SET status = 'spawning' WHERE worker_id = ?1",
                [&worker],
            )
            .unwrap();
        match tasks::claim(&self.conn, &worker).unwrap() {
            ClaimOutcome::Claimed(_) => {}
            other => panic!("no claim: {other:?}"),
        }
        let attempt_id = format!("{}/A1", task.task_id);
        let push = self
            .conn
            .execute(
                "UPDATE attempts SET via = 'push' WHERE id = ?1",
                [&attempt_id],
            )
            .unwrap();
        assert_eq!(push, 1, "the claim writes the attempt {attempt_id}");
        AttemptRef {
            task_id: task.task_id,
            attempt_id,
        }
    }

    /// The task folder of `a`.
    fn task_dir(&self, a: &AttemptRef) -> PathBuf {
        self.root().join("tasks").join(&a.task_id)
    }

    /// The outbox of `a`, made with mode 0700 as a worker command makes it.
    fn outbox(&self, a: &AttemptRef) -> PathBuf {
        let dir = self.task_dir(a).join("attempts").join("A1").join("outbox");
        fs::create_dir_all(&dir).unwrap();
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
        dir
    }

    fn progress_path(&self, a: &AttemptRef) -> PathBuf {
        self.outbox(a).join("progress.jsonl")
    }

    /// Part 1 of 1 pass for `attempts`.
    fn read(&self, attempts: &[AttemptRef]) -> Vec<OutboxRead> {
        outbox_ingest::read(
            &self.conn,
            &self.root(),
            attempts,
            &self.owner,
            &self.limits,
        )
        .unwrap()
    }

    /// Part 2 of 1 pass, in its own transaction.
    fn ingest(&mut self, reads: &[OutboxRead]) -> Report {
        let tx = self.conn.transaction().unwrap();
        let report = outbox_ingest::ingest(&tx, Some(GUARD), reads).unwrap();
        tx.commit().unwrap();
        report
    }

    /// Part 1 and part 2 of 1 pass for `attempts`.
    fn pass(&mut self, attempts: &[AttemptRef]) -> Report {
        let reads = self.read(attempts);
        self.ingest(&reads)
    }

    fn status(&self, a: &AttemptRef) -> String {
        tasks::get(&self.conn, &a.task_id).unwrap().status
    }

    /// The `ingested` row of `path`: outcome, reason and log offset.
    fn ingested(&self, path: &Path) -> Option<(String, Option<String>, Option<i64>)> {
        self.conn
            .query_row(
                "SELECT outcome, reason, log_offset FROM ingested WHERE path = ?1",
                [path.display().to_string()],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .ok()
    }

    fn ingested_count(&self) -> i64 {
        self.conn
            .query_row("SELECT count(*) FROM ingested", [], |r| r.get(0))
            .unwrap()
    }

    /// The texts of the events of `kind` of the task of `a`, in order.
    fn events(&self, a: &AttemptRef, kind: &str) -> Vec<String> {
        let mut stmt = self
            .conn
            .prepare("SELECT text FROM events WHERE task_id = ?1 AND kind = ?2 ORDER BY rowid")
            .unwrap();
        stmt.query_map(params![a.task_id, kind], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    }

    /// The state and outcome of the attempt `a`.
    fn attempt(&self, a: &AttemptRef) -> (String, Option<String>, Option<i64>) {
        self.conn
            .query_row(
                "SELECT state, outcome, active_at FROM attempts WHERE id = ?1",
                [&a.attempt_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap()
    }

    /// The worker of `a`: its end reason and its handoff path.
    fn worker(&self, a: &AttemptRef) -> (Option<String>, Option<String>) {
        self.conn
            .query_row(
                "SELECT w.end_reason, w.handoff_path FROM workers w
                 JOIN attempts a ON a.worker_id = w.worker_id WHERE a.id = ?1",
                [&a.attempt_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap()
    }
}

/// Write `text` to `path` with mode 0600, as the outbox writer does.
fn write_0600(path: &Path, text: &str) {
    fs::write(path, text).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
}

/// Append `text` to `path`, made with mode 0600 when it is missing.
fn append(path: &Path, text: &str) {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .unwrap();
    file.write_all(text.as_bytes()).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
}

/// 1 line of `progress.jsonl` of task `task` and attempt `attempt`.
fn line(task: &str, attempt: &str, text: &str) -> String {
    ProgressLine {
        task: task.into(),
        attempt: attempt.into(),
        at: "2026-11-18T12:00:00Z".into(),
        kind: LineKind::Progress,
        text: text.into(),
    }
    .to_line(4096)
    .unwrap()
}

fn result_json(a: &AttemptRef) -> String {
    ResultFile {
        task: a.task_id.clone(),
        attempt: "A1".into(),
        status: ResultStatus::Completed,
        summary: "done".into(),
        commits: vec!["3f9c2ab".into()],
        files_changed: vec!["src/lib.rs".into()],
        acceptance_self_check: Vec::new(),
        notes_for_next: None,
        handoff: None,
    }
    .to_json()
}

fn write_result(w: &World, a: &AttemptRef) -> PathBuf {
    let path = w.outbox(a).join("result.json");
    write_0600(&path, &result_json(a));
    path
}

/// The reason of the rejected row of `path`.
fn rejected_reason(w: &World, path: &Path) -> String {
    let (outcome, reason, _) = w.ingested(path).expect("an ingested row");
    assert_eq!(outcome, "rejected", "{reason:?}");
    reason.unwrap_or_default()
}

#[test]
fn progress_lines_are_ingested_once_from_the_recorded_offset() {
    let mut w = world();
    let a = w.running_task();
    let path = w.progress_path(&a);
    let first = format!(
        "{}{}",
        line(&a.task_id, "A1", "one"),
        line(&a.task_id, "A1", "two")
    );
    append(&path, &first);

    let report = w.pass(std::slice::from_ref(&a));
    assert_eq!(report.progress_lines, 2);
    assert_eq!(w.events(&a, "progress"), vec!["one", "two"]);
    let (outcome, _, offset) = w.ingested(&path).unwrap();
    assert_eq!(outcome, "accepted");
    assert_eq!(offset, Some(first.len() as i64));
    // The row of `horch ledger progress`: the worker progress and the
    // activity of the attempt on the fleet clock.
    assert_eq!(w.attempt(&a).2, Some(CLOCK));

    let third = line(&a.task_id, "A1", "three");
    append(&path, &third);
    let report = w.pass(std::slice::from_ref(&a));
    assert_eq!(report.progress_lines, 1);
    assert_eq!(w.events(&a, "progress"), vec!["one", "two", "three"]);
    assert_eq!(
        w.ingested(&path).unwrap().2,
        Some((first.len() + third.len()) as i64)
    );

    let report = w.pass(std::slice::from_ref(&a));
    assert_eq!(report, Report::default());
    assert_eq!(w.events(&a, "progress").len(), 3);
}

#[test]
fn half_written_last_line_waits_for_the_next_pass() {
    let mut w = world();
    let a = w.running_task();
    let path = w.progress_path(&a);
    let whole = line(&a.task_id, "A1", "whole");
    let half = line(&a.task_id, "A1", "half");
    let (head, tail) = half.split_at(half.len() / 2);
    append(&path, &format!("{whole}{head}"));

    w.pass(std::slice::from_ref(&a));
    assert_eq!(w.events(&a, "progress"), vec!["whole"]);
    assert_eq!(w.ingested(&path).unwrap().2, Some(whole.len() as i64));

    append(&path, tail);
    w.pass(std::slice::from_ref(&a));
    assert_eq!(w.events(&a, "progress"), vec!["whole", "half"]);
    assert_eq!(
        w.ingested(&path).unwrap().2,
        Some((whole.len() + half.len()) as i64)
    );
}

#[test]
fn progress_over_its_limit_is_rejected_and_fleetd_stays_healthy() {
    let mut w = world();
    w.limits.progress_max_bytes = 256;
    let a = w.running_task();
    let path = w.progress_path(&a);
    let mut text = String::new();
    while text.len() <= 256 {
        text.push_str(&line(&a.task_id, "A1", "filler"));
    }
    append(&path, &text);
    let result = write_result(&w, &a);

    let report = w.pass(std::slice::from_ref(&a));
    assert_eq!(report.rejected, vec![path.clone()]);
    let reason = rejected_reason(&w, &path);
    assert!(reason.contains("limit of 256 bytes"), "{reason}");
    assert!(w.events(&a, "progress").is_empty());
    // The other files of the outbox are still ingested.
    assert_eq!(report.to_verifying, vec![a.task_id.clone()]);
    assert_eq!(w.ingested(&result).unwrap().0, "accepted");

    // The rejected path is not read again, even when it is valid now.
    fs::remove_file(&path).unwrap();
    append(&path, &line(&a.task_id, "A1", "late"));
    let report = w.pass(std::slice::from_ref(&a));
    assert_eq!(report, Report::default());
    assert!(w.events(&a, "progress").is_empty());
}

#[test]
fn line_with_the_attempt_id_of_another_attempt_is_skipped() {
    let mut w = world();
    let a = w.running_task();
    let path = w.progress_path(&a);
    let text = format!(
        "{}{}{}",
        line(&a.task_id, "A2", "foreign"),
        "not json\n",
        line(&a.task_id, "A1", "own"),
    );
    append(&path, &text);

    let report = w.pass(std::slice::from_ref(&a));
    assert_eq!(report.progress_lines, 1);
    assert_eq!(report.skipped_lines, 2);
    assert_eq!(w.events(&a, "progress"), vec!["own"]);
    let skipped = w.events(&a, "note");
    assert_eq!(
        skipped.len(),
        2,
        "1 event for each skipped line: {skipped:?}"
    );
    assert!(skipped[0].contains("line 1"), "{skipped:?}");
    assert!(skipped[0].contains("\"A2\""), "{skipped:?}");
    assert!(skipped[1].contains("line 2"), "{skipped:?}");
    let (outcome, _, offset) = w.ingested(&path).unwrap();
    assert_eq!(outcome, "accepted", "a bad line does not reject the file");
    assert_eq!(offset, Some(text.len() as i64));
}

#[test]
fn pres_04_m5_foreign_owned_outbox_is_refused() {
    let mut w = world();
    let User::Uid(uid) = w.owner else {
        unreachable!("a unix test")
    };
    // The ledger owner is another user than the owner of the outbox.
    w.owner = User::Uid(uid + 1);
    let a = w.running_task();
    let result = write_result(&w, &a);
    let progress = w.progress_path(&a);
    append(&progress, &line(&a.task_id, "A1", "one"));

    let report = w.pass(std::slice::from_ref(&a));
    assert_eq!(report.rejected.len(), 2, "{report:?}");
    for path in [&result, &progress] {
        let reason = rejected_reason(&w, path);
        assert!(reason.contains("is not the ledger owner"), "{reason}");
    }
    assert_eq!(w.status(&a), "running");
    assert!(w.events(&a, "progress").is_empty());
}

#[test]
fn outbox_with_mode_0755_is_refused() {
    let mut w = world();
    let a = w.running_task();
    let result = write_result(&w, &a);
    let progress = w.progress_path(&a);
    append(&progress, &line(&a.task_id, "A1", "one"));
    let outbox = result.parent().unwrap();
    fs::set_permissions(outbox, fs::Permissions::from_mode(0o755)).unwrap();

    let reads = w.read(std::slice::from_ref(&a));
    w.ingest(&reads);
    for path in [&result, &progress] {
        let reason = rejected_reason(&w, path);
        assert!(reason.contains("mode 0755"), "{reason}");
    }
    assert_eq!(w.status(&a), "running");
}

#[test]
fn symlinked_result_is_rejected() {
    let mut w = world();
    let a = w.running_task();
    let real = w.dir.join("elsewhere.json");
    write_0600(&real, &result_json(&a));
    let path = w.outbox(&a).join("result.json");
    std::os::unix::fs::symlink(&real, &path).unwrap();

    let report = w.pass(std::slice::from_ref(&a));
    assert_eq!(report.rejected, vec![path.clone()]);
    let reason = rejected_reason(&w, &path);
    assert!(reason.starts_with("no_link:"), "{reason}");
    assert_eq!(w.status(&a), "running");

    // Not read again.
    assert_eq!(w.pass(std::slice::from_ref(&a)), Report::default());
}

/// Write the handoff `result.json` of `a` that names `handoff`.
fn write_handoff(w: &World, a: &AttemptRef, handoff: &Path) -> PathBuf {
    let path = w.outbox(a).join("result.json");
    let file = handoff_result(
        &a.task_id,
        "A1",
        "context limit",
        &handoff.display().to_string(),
    );
    write_0600(&path, &file.to_json());
    path
}

#[test]
fn handoff_result_ends_the_attempt_handed_off_and_burns_no_attempt() {
    let mut w = world();
    let a = w.running_task();
    let before = tasks::get(&w.conn, &a.task_id).unwrap();
    let handoff = w.task_dir(&a).join("handoff.md");
    fs::create_dir_all(w.task_dir(&a)).unwrap();
    fs::write(&handoff, "next steps").unwrap();
    let result = write_handoff(&w, &a, &handoff);

    let report = w.pass(std::slice::from_ref(&a));
    assert_eq!(report.handed_off, vec![a.task_id.clone()]);
    assert!(report.to_verifying.is_empty(), "a handoff is not a result");
    assert_eq!(w.ingested(&result).unwrap().0, "accepted");
    let (state, outcome, _) = w.attempt(&a);
    assert_eq!(state, "ended");
    assert_eq!(outcome.as_deref(), Some("handed_off"));
    let real = fs::canonicalize(&handoff).unwrap().display().to_string();
    assert_eq!(
        w.worker(&a),
        (Some("handed_off".to_string()), Some(real.clone()))
    );
    assert_eq!(w.status(&a), "ready");
    let after = tasks::get(&w.conn, &a.task_id).unwrap();
    assert_eq!(
        after.attempts - after.attempt_floor,
        before.attempts - before.attempt_floor - 1,
        "the attempt is given back: the next claim uses the same number"
    );
    let transitions = w.events(&a, "transition");
    assert!(
        transitions.iter().any(|t| t.contains("handoff")),
        "the event says handoff: {transitions:?}"
    );
    // The next attempt names the handoff file.
    assert_eq!(
        outbox_ingest::handoff_of(&w.conn, &a.task_id).unwrap(),
        Some(real)
    );

    // A second pass changes nothing.
    assert_eq!(w.pass(std::slice::from_ref(&a)), Report::default());
}

#[test]
fn handoff_path_outside_the_attempt_is_not_used() {
    let mut w = world();
    let outside = w.dir.join("outside.md");
    fs::write(&outside, "steal me").unwrap();
    // A link inside the task folder whose real path is outside it.
    let a = w.running_task();
    let b = w.running_task();
    fs::create_dir_all(w.task_dir(&b)).unwrap();
    let link = w.task_dir(&b).join("handoff.md");
    std::os::unix::fs::symlink(&outside, &link).unwrap();
    write_handoff(&w, &a, &outside);
    write_handoff(&w, &b, &link);

    let report = w.pass(&[a.clone(), b.clone()]);
    assert_eq!(
        report.handed_off,
        vec![a.task_id.clone(), b.task_id.clone()]
    );
    for t in [&a, &b] {
        assert_eq!(w.attempt(t).1.as_deref(), Some("handed_off"));
        assert_eq!(
            w.worker(t),
            (Some("handed_off".to_string()), None),
            "the handoff path is not kept"
        );
        let transitions = w.events(t, "transition");
        assert!(
            transitions.iter().any(|e| e.contains("is not used")),
            "the event says why: {transitions:?}"
        );
        assert_eq!(w.status(t), "ready");
        assert_eq!(
            outbox_ingest::handoff_of(&w.conn, &t.task_id).unwrap(),
            None
        );
    }
}

#[test]
fn idempotent_progress_line() {
    let mut w = world();
    let a = w.running_task();
    append(&w.progress_path(&a), &line(&a.task_id, "A1", "once"));

    let reads = w.read(std::slice::from_ref(&a));
    let first = w.ingest(&reads);
    let second = w.ingest(&reads);
    assert_eq!(first.progress_lines, 1);
    assert_eq!(second, Report::default());
    assert_eq!(w.events(&a, "progress"), vec!["once"]);
}

#[test]
fn idempotent_result_file() {
    let mut w = world();
    let a = w.running_task();
    write_result(&w, &a);

    let reads = w.read(std::slice::from_ref(&a));
    let first = w.ingest(&reads);
    let transitions = w.events(&a, "transition").len();
    let second = w.ingest(&reads);
    assert_eq!(first.to_verifying, vec![a.task_id.clone()]);
    assert_eq!(second, Report::default());
    assert_eq!(w.ingested_count(), 1);
    assert_eq!(w.events(&a, "transition").len(), transitions);
}

#[test]
fn idempotent_question_file() {
    let mut w = world();
    let a = w.running_task();
    let questions = w.outbox(&a).join("questions");
    fs::create_dir(&questions).unwrap();
    fs::set_permissions(&questions, fs::Permissions::from_mode(0o700)).unwrap();
    let question = serde_json::json!({
        "task": a.task_id,
        "attempt": "A1",
        "text": "Which table owns the cursor?",
        "at": "2026-11-18T10:00:00Z",
    })
    .to_string();
    write_0600(&questions.join("1.json"), &question);

    let reads = w.read(std::slice::from_ref(&a));
    let first = w.ingest(&reads);
    let transitions = w.events(&a, "transition").len();
    let second = w.ingest(&reads);
    assert_eq!(first.to_blocked, vec![a.task_id.clone()]);
    assert_eq!(second, Report::default());
    assert_eq!(w.ingested_count(), 1);
    assert_eq!(w.events(&a, "transition").len(), transitions);
    assert_eq!(w.status(&a), "blocked_on_user");
}
