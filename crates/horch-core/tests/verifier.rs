//! The verifier: a task in `verifying` gets its check as a `task_check`
//! job, `verdict-<n>.json` is written, and only the verifier makes the
//! value that moves a task to `done` (docs/specs/fleetd/design.md,
//! "Architecture and roles": Verifier; "The task state machine":
//! `verifying`; "The kinds of job": `task_check`; RR-3, Phase 4 part; FM-9,
//! part test; docs/tickets/fleetd-p4/19-verifier.md).
//!
//! Each test uses a temp ledger and a temp fleet root. The tip of a task
//! branch is a fact that the test gives, as part 1 of a pass gives it. A
//! check job ends through its row, as the job process ends it. No test
//! reads the load, the limits or the clock of this machine.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use horch_core::fleetd::fleet_runtime::adapters::jobs_store;
use horch_core::fleetd::fleet_runtime::application::jobs::{self, JobCtx};
use horch_core::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;
use horch_core::fleetd::fleet_runtime::domain::jobs::{Kind, State};
use horch_core::fleetd::ideas::application::needs_you::NeedsYouList;
use horch_core::fleetd::task_execution::adapters::verdict_file;
use horch_core::fleetd::task_execution::application::retry::Policy;
use horch_core::fleetd::task_execution::application::verifier::{
    self, check_hash, Report, Verifier,
};
use horch_core::fleetd::task_execution::domain::verification::{Decision, Next};
use horch_core::taskledger::finish;
use horch_core::taskledger::model::{Actor, WorkKind, WorkerKind};
use horch_core::taskledger::runs::{self, NewRun, Run};
use horch_core::taskledger::tasks::{self, NewTask, Task};
use horch_core::taskledger::workers::{self, NewWorker};
use horch_core::taskledger::{schema, util};
use rusqlite::{params, Connection, TransactionBehavior};

/// The tip of the task branch in each test.
const TIP: &str = "1111111111111111111111111111111111111111";
/// A newer tip: the worker made new commits.
const NEW_TIP: &str = "2222222222222222222222222222222222222222";

struct World {
    _dir: tempfile::TempDir,
    conn: Connection,
    config: FleetConfig,
    fleet_root: PathBuf,
    run: Run,
    tips: BTreeMap<String, String>,
}

/// A temp ledger of the current schema, a fleet root and 1 run with no
/// default check.
fn world() -> World {
    let dir = tempfile::tempdir().unwrap();
    let mut conn = Connection::open(dir.path().join("ledger.sqlite3")).unwrap();
    schema::migrate_to(&mut conn, schema::VERSION).unwrap();
    let top = horch_platform::path::canonicalize(dir.path()).unwrap();
    let fleet_root = top.join("fleet");
    let project = top.join("repo");
    std::fs::create_dir_all(&fleet_root).unwrap();
    std::fs::create_dir_all(&project).unwrap();
    let run = runs::create(
        &conn,
        &project,
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
    World {
        _dir: dir,
        conn,
        config: FleetConfig::default(),
        fleet_root,
        run,
        tips: BTreeMap::new(),
    }
}

/// The policy of rung 6 of each task: the design defaults.
fn policy(_: &Task) -> Policy {
    Policy::default()
}

impl World {
    /// A task in `verifying` of the shared checkout, with the check
    /// `check`, the kind `kind` and `**Check:** none` when `check_none`.
    /// Its 1 attempt is a push attempt, and its branch tip is [`TIP`].
    fn verifying(&mut self, check: Option<&str>, kind: WorkKind, check_none: bool) -> String {
        let n = tasks::of_run(&self.conn, &self.run.run_id).unwrap().len();
        let title = format!("t{n}");
        let new = NewTask {
            land_check: check.map(Into::into),
            work_kind: Some(kind),
            check_none,
            ..NewTask::work(&title, &format!("{title}.md"), "opus-dev")
        };
        let task = tasks::add(&self.conn, &self.run, &new, Actor::Orchestrator)
            .unwrap()
            .task_id;
        self.attempt(&task, "push");
        self.tips.insert(task.clone(), TIP.into());
        task
    }

    /// A new attempt of `task` that began by `via`, with its own worker.
    /// The task is `verifying`: its worker wrote a valid `result.json`.
    fn attempt(&self, task: &str, via: &str) -> String {
        let worker = workers::insert(
            &self.conn,
            &NewWorker {
                run_id: self.run.run_id.clone(),
                kind: Some(WorkerKind::Worker),
                persona: "opus-dev".into(),
                target_task_id: Some(task.into()),
                ..Default::default()
            },
        )
        .unwrap();
        let n: i64 = self
            .conn
            .query_row(
                "SELECT attempts + 1 FROM tasks WHERE task_id = ?1",
                [task],
                |r| r.get(0),
            )
            .unwrap();
        let attempt = format!("{task}-a{n}");
        self.conn
            .execute(
                "UPDATE tasks SET status = 'verifying', attempts = ?2, current_worker_id = ?3
                 WHERE task_id = ?1",
                params![task, n, worker],
            )
            .unwrap();
        self.conn
            .execute(
                "INSERT INTO attempts (id, task_id, worker_id, state, started_at, via)
                 VALUES (?1, ?2, ?3, 'live', ?4, ?5)",
                params![attempt, task, worker, util::now(), via],
            )
            .unwrap();
        attempt
    }

    /// 1 verifier part of a pass, in its own committed transaction.
    fn verify(&mut self) -> Report {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        let report = run_verifier(&self.config, &self.tips, &tx);
        tx.commit().unwrap();
        report
    }

    /// Part 3 of a pass: the verdict files of the committed verdicts.
    fn write_files(&mut self) -> Vec<PathBuf> {
        verifier::write_verdicts(&mut self.conn, &self.fleet_root).unwrap()
    }

    /// The `task_check` rows of `task`, oldest first.
    fn check_jobs(&self, task: &str) -> Vec<jobs_store::JobRow> {
        let mut stmt = self
            .conn
            .prepare("SELECT id FROM jobs WHERE kind = 'task_check' AND subject = ?1 ORDER BY id")
            .unwrap();
        let ids: Vec<i64> = stmt
            .query_map([task], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        ids.into_iter()
            .map(|id| jobs_store::get(&self.conn, id).unwrap().unwrap())
            .collect()
    }

    /// The job process of the `queued` run of `task` ends `passed` or
    /// `failed`, with a verdict of its check.
    fn end_check(&self, task: &str, passed: bool) {
        let row = self
            .check_jobs(task)
            .into_iter()
            .find(|r| r.state == State::Queued)
            .expect("a queued task_check run");
        let state = if passed { "passed" } else { "failed" };
        let result =
            format!(r#"{{"passed":{passed},"verdict":true,"text":"`check` in repo: exit"}}"#);
        self.conn
            .execute(
                "UPDATE jobs SET state = ?2, result_json = ?3, ended_at = ?4 WHERE id = ?1",
                params![row.id, state, result, util::now()],
            )
            .unwrap();
    }

    fn status(&self, task: &str) -> String {
        tasks::get(&self.conn, task).unwrap().status
    }

    fn verdict_dir(&self, task: &str) -> PathBuf {
        self.fleet_root.join("tasks").join(task)
    }
}

/// 1 verifier part of a pass in `tx`, which the caller ends, with the
/// config `config` and the tips `tips`.
fn run_verifier(config: &FleetConfig, tips: &BTreeMap<String, String>, tx: &Connection) -> Report {
    let ctx = JobCtx {
        guard: None,
        config,
        needs_you: &NeedsYouList,
    };
    let v = Verifier {
        jobs: &ctx,
        clock: FleetSeconds(100),
        tips,
        policy: &policy,
    };
    verifier::verify(&v, tx).unwrap()
}

/// The names of the verdict files of a task folder, sorted.
fn verdict_files(dir: &Path) -> Vec<String> {
    let Ok(read) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = read
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("verdict"))
        .collect();
    names.sort();
    names
}

/// The input key of the check `check` at `tip`.
fn key(tip: &str, check: &str) -> String {
    format!("{tip}:{}", check_hash(Some(check)))
}

#[test]
fn verifying_task_queues_1_task_check_job() {
    let mut w = world();
    let task = w.verifying(Some("cargo test"), WorkKind::Feature, false);

    let report = w.verify();
    assert_eq!(report.queued, vec![task.clone()]);
    assert!(report.passed.is_empty() && report.rejected.is_empty());
    let rows = w.check_jobs(&task);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].kind, Kind::TaskCheck);
    assert_eq!(rows[0].input_key, key(TIP, "cargo test"));
    assert_eq!(rows[0].state, State::Queued);

    // A second pass finds the run queued and adds no row.
    let again = w.verify();
    assert_eq!(again.queued, vec![task.clone()]);
    assert_eq!(w.check_jobs(&task).len(), 1);
    assert_eq!(w.status(&task), "verifying");
}

#[test]
fn task_check_runs_as_a_job_and_the_pass_does_not_wait() {
    let mut w = world();
    let task = w.verifying(Some("sleep 600"), WorkKind::Feature, false);

    let started = Instant::now();
    let report = w.verify();
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "the pass waited"
    );
    assert_eq!(report.queued, vec![task.clone()]);
    // The check is a job row that part 3 starts after the commit, with the
    // time limit of today's check (F4).
    let due = jobs::due_to_start(&w.conn, FleetSeconds(100)).unwrap();
    assert_eq!(due.len(), 1);
    assert_eq!(due[0].subject, task);
    assert_eq!(due[0].time_limit_s, finish::land::CHECK_TIMEOUT.as_secs());
    // No verdict before the job ends.
    assert!(verdict_file::recorded(&w.conn, &task).unwrap().is_empty());
    assert_eq!(w.status(&task), "verifying");
}

#[test]
fn passed_check_of_a_code_kind_starts_the_review_steps() {
    let mut w = world();
    let code = w.verifying(Some("cargo test"), WorkKind::Feature, false);
    let docs = w.verifying(Some("true"), WorkKind::Docs, false);
    w.verify();
    w.end_check(&code, true);
    w.end_check(&docs, true);

    let report = w.verify();
    assert_eq!(report.passed.len(), 2, "{report:?}");
    let of = |task: &str| {
        report
            .passed
            .iter()
            .find(|p| p.verified.task_id() == task)
            .unwrap()
    };
    assert_eq!(of(&code).next, Next::Review);
    assert_eq!(of(&docs).next, Next::Structural);
    assert_eq!(of(&code).verified.tip(), TIP);
    // The check alone does not move a task to done.
    assert_eq!(w.status(&code), "verifying");

    let recorded = verdict_file::recorded(&w.conn, &code).unwrap();
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].decision, Decision::Passed);
    assert_eq!(recorded[0].tip, TIP);
    let files = w.write_files();
    assert_eq!(files.len(), 2);
    assert_eq!(verdict_files(&w.verdict_dir(&code)), ["verdict-1.json"]);
    let text = std::fs::read_to_string(w.verdict_dir(&code).join("verdict-1.json")).unwrap();
    assert!(text.contains("\"decision\":\"passed\""), "{text}");

    // The value alone does not move a code kind to done: its review steps
    // pass first (ticket 20, tests/verifier_review.rs).
    let verified = report
        .passed
        .into_iter()
        .find(|p| p.verified.task_id() == code)
        .unwrap()
        .verified;
    let tx = w.conn.transaction().unwrap();
    assert!(!verifier::done(&tx, None, verified, TIP).unwrap());
    tx.commit().unwrap();
    assert_eq!(w.status(&code), "verifying");
}

#[test]
fn failed_check_ends_the_attempt_rejected_and_burns_1_attempt() {
    let mut w = world();
    let task = w.verifying(Some("cargo test"), WorkKind::Feature, false);
    let attempt = format!("{task}-a1");
    w.verify();
    w.end_check(&task, false);

    let report = w.verify();
    assert_eq!(report.rejected, vec![task.clone()]);
    assert!(report.passed.is_empty());
    let (state, outcome): (String, String) = w
        .conn
        .query_row(
            "SELECT state, outcome FROM attempts WHERE id = ?1",
            [&attempt],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!((state.as_str(), outcome.as_str()), ("ended", "rejected"));
    // Rung 6: 1 attempt of the limit is used, and the task waits in ready.
    let t = tasks::get(&w.conn, &task).unwrap();
    assert_eq!(t.status, "ready");
    assert_eq!(t.attempts - t.attempt_floor, 1);
    let recorded = verdict_file::recorded(&w.conn, &task).unwrap();
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].decision, Decision::Failed);

    // A later pass does not reject it again.
    let again = w.verify();
    assert!(again.rejected.is_empty());
    assert_eq!(verdict_file::recorded(&w.conn, &task).unwrap().len(), 1);
}

#[test]
fn same_tip_and_check_reuse_the_passed_verdict() {
    let mut w = world();
    let task = w.verifying(Some("cargo test"), WorkKind::Feature, false);
    w.verify();
    w.end_check(&task, true);
    assert_eq!(w.verify().passed.len(), 1);
    // The same attempt in the next pass: no new verdict and no new job.
    assert_eq!(w.verify().passed.len(), 1);
    assert_eq!(verdict_file::recorded(&w.conn, &task).unwrap().len(), 1);

    // A new attempt ends with the same tip and the same check: the passed
    // run is used again, and no new run is queued.
    w.conn
        .execute(
            "UPDATE attempts SET state = 'ended' WHERE task_id = ?1",
            [&task],
        )
        .unwrap();
    w.attempt(&task, "push");
    let report = w.verify();
    assert_eq!(report.passed.len(), 1, "{report:?}");
    assert!(report.queued.is_empty());
    assert_eq!(w.check_jobs(&task).len(), 1);
    let recorded = verdict_file::recorded(&w.conn, &task).unwrap();
    assert_eq!(recorded.len(), 2);
    assert_eq!(recorded[1].n, 2);
    assert_eq!(recorded[1].job, recorded[0].job);
}

#[test]
fn new_commits_make_the_verdict_stale() {
    let mut w = world();
    let task = w.verifying(Some("cargo test"), WorkKind::Feature, false);
    w.verify();
    w.end_check(&task, true);
    let old = w.verify().passed.into_iter().next().unwrap().verified;

    // The worker made new commits: the verdict of the old tip stands no more.
    w.tips.insert(task.clone(), NEW_TIP.into());
    let report = w.verify();
    assert!(report.passed.is_empty(), "{report:?}");
    assert_eq!(report.queued, vec![task.clone()]);
    let rows = w.check_jobs(&task);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[1].input_key, key(NEW_TIP, "cargo test"));

    // The old value moves nothing.
    let tx = w.conn.transaction().unwrap();
    assert!(!verifier::done(&tx, None, old, NEW_TIP).unwrap());
    tx.commit().unwrap();
    assert_eq!(w.status(&task), "verifying");
}

#[test]
fn no_check_and_no_default_fails_with_no_check() {
    let mut w = world();
    let task = w.verifying(None, WorkKind::Feature, false);

    let report = w.verify();
    assert_eq!(report.rejected, vec![task.clone()]);
    assert!(w.check_jobs(&task).is_empty());
    let recorded = verdict_file::recorded(&w.conn, &task).unwrap();
    assert_eq!(recorded[0].decision, Decision::Failed);
    assert!(recorded[0].reason.contains("no check"), "{recorded:?}");
    assert_eq!(w.status(&task), "ready");
}

#[test]
fn check_none_is_not_applicable_and_queues_no_job() {
    let mut w = world();
    let task = w.verifying(None, WorkKind::Feature, true);

    let report = w.verify();
    assert_eq!(report.passed.len(), 1);
    assert_eq!(
        report.passed[0].verified.decision(),
        Decision::NotApplicable
    );
    assert!(w.check_jobs(&task).is_empty());
    assert_eq!(w.status(&task), "verifying");
}

#[test]
fn claim_attempt_is_not_verified_by_fleetd() {
    let mut w = world();
    let task = w.verifying(Some("cargo test"), WorkKind::Feature, false);
    w.conn
        .execute(
            "UPDATE attempts SET via = 'claim' WHERE task_id = ?1",
            [&task],
        )
        .unwrap();

    let report = w.verify();
    assert!(report.queued.is_empty() && report.passed.is_empty() && report.rejected.is_empty());
    assert!(w.check_jobs(&task).is_empty());
    assert!(verdict_file::recorded(&w.conn, &task).unwrap().is_empty());
    assert_eq!(w.status(&task), "verifying");
}

/// A fault before the commit of the verdict row: the next pass decides
/// again, records 1 verdict, burns 1 attempt and writes 1 file.
#[test]
fn crash_before_the_commit_of_the_verdict_row_decides_again_1_time() {
    let mut w = world();
    let task = w.verifying(Some("cargo test"), WorkKind::Feature, false);
    w.verify();
    w.end_check(&task, false);

    {
        let tx = w
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        assert_eq!(
            run_verifier(&w.config, &w.tips, &tx).rejected,
            vec![task.clone()]
        );
        // The fault: the transaction rolls back.
    }
    assert_eq!(w.status(&task), "verifying");
    assert!(verdict_file::recorded(&w.conn, &task).unwrap().is_empty());

    assert_eq!(w.verify().rejected, vec![task.clone()]);
    w.write_files();
    w.verify();
    w.write_files();
    let t = tasks::get(&w.conn, &task).unwrap();
    assert_eq!(t.status, "ready");
    assert_eq!(t.attempts - t.attempt_floor, 1);
    assert_eq!(verdict_file::recorded(&w.conn, &task).unwrap().len(), 1);
    assert_eq!(verdict_files(&w.verdict_dir(&task)), ["verdict-1.json"]);
}

/// A fault after the commit of the verdict row and before its file: the
/// next pass writes the file 1 time, and a later pass changes nothing.
#[test]
fn crash_after_the_commit_and_before_the_file_writes_it_1_time() {
    let mut w = world();
    let task = w.verifying(Some("cargo test"), WorkKind::Feature, false);
    w.verify();
    w.end_check(&task, true);
    assert_eq!(w.verify().passed.len(), 1);
    // The fault: part 3 did not run.
    assert!(verdict_files(&w.verdict_dir(&task)).is_empty());

    w.verify();
    let written = w.write_files();
    assert_eq!(written, [w.verdict_dir(&task).join("verdict-1.json")]);
    let bytes = std::fs::read(&written[0]).unwrap();
    assert!(w.write_files().is_empty());
    assert_eq!(std::fs::read(&written[0]).unwrap(), bytes);
    assert_eq!(verdict_files(&w.verdict_dir(&task)), ["verdict-1.json"]);
    assert_eq!(w.status(&task), "verifying");
    assert_eq!(w.check_jobs(&task).len(), 1);
}
