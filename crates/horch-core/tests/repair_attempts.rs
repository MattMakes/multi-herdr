//! Resolver and fix attempts of a task branch: fleetd starts 1 attempt of
//! the `done` task of a `conflicted` or `gate_failed` branch, in its
//! worktree, with the conflict or the gate output. The attempt holds the
//! lease of its task, and its passed result moves the branch to `ready`
//! (docs/specs/fleetd/design.md, "Landing a task branch on its feature
//! branch", "Leases, not claims", I1, FM-38;
//! docs/tickets/fleetd-p4/33-resolver-and-fix-attempts.md).
//!
//! Each test makes its own temporary git repository, ledger and fleet
//! root. No test starts a `horch` binary, and no test reads the load, the
//! limits or the clock of this machine: the time is the fleet clock that
//! the test gives, and the config is the default.

#![cfg(unix)]

#[path = "common/test_jobs.rs"]
mod test_jobs;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use horch_core::fleetd::fleet_runtime::adapters::jobs_store;
use horch_core::fleetd::fleet_runtime::application::jobs::JobCtx;
use horch_core::fleetd::fleet_runtime::application::ledger::Cause;
use horch_core::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;
use horch_core::fleetd::fleet_runtime::domain::jobs::Kind;
use horch_core::fleetd::ideas::application::needs_you::NeedsYouList;
use horch_core::fleetd::integration::adapters::sqlite::{self as rows, NewFeature};
use horch_core::fleetd::integration::application::landing;
use horch_core::fleetd::integration::domain::task_branch::LandingState;
use horch_core::fleetd::task_execution::adapters::leases_store;
use horch_core::fleetd::task_execution::adapters::result_reader::{AttemptRef, RESULT_JSON};
use horch_core::fleetd::task_execution::application::dispatch::{
    bind, write_missing_task_files, write_task_files, DispatchCtx,
};
use horch_core::fleetd::task_execution::application::lease::{self, Observed};
use horch_core::fleetd::task_execution::application::repair::{
    self, RepairKind, Report, Results, Started,
};
use horch_core::fleetd::task_execution::domain::lease::Limits;
use horch_core::fleetd::task_execution::domain::task::Seen;
use horch_core::fleetd::worktree_custody::adapters::sqlite as trees;
use horch_core::fleetd::worktree_custody::application::create;
use horch_core::taskledger::git::Git;
use horch_core::taskledger::model::Actor;
use horch_core::taskledger::runs::{self, NewRun, Run};
use horch_core::taskledger::tasks::{self, NewTask};
use horch_core::taskledger::workers::NewWorker;
use horch_core::taskledger::{util, Store};
use rusqlite::{params, Connection};
use test_jobs::TestJobs;

const FEATURE: &str = "F-0001";
const RUN_BRANCH: &str = "fleet/r1";
const SNAPSHOT: &str = "# A ticket\n\nDo the work.\n";
const CHECK: &str = "cargo test -p demo";
const PERSONA: &str = "opus";

/// The numbers of a worker in these tests.
const LIMITS: Limits = Limits {
    lease_s: 120,
    stall_after_s: 300,
    heartbeat_max_age_s: 30,
};

// ─── the world of a test ────────────────────────────────────────────────

struct World {
    _tmp: tempfile::TempDir,
    root: PathBuf,
    fleet_root: PathBuf,
    ledger: PathBuf,
    store: Store,
    run: Run,
    config: FleetConfig,
    task: String,
    branch: String,
    tree: PathBuf,
}

/// Run git in `dir` for the test itself, with no hook and no config of the
/// user, and give its trimmed output.
fn git(dir: &Path, args: &[&str]) -> String {
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
    assert!(
        o.status.success(),
        "git {}: {}",
        args.join(" "),
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

fn cause(text: &str) -> Cause<'_> {
    Cause {
        actor: Actor::Horch,
        text,
    }
}

impl World {
    /// A repository with a run branch and its feature, and 1 task whose
    /// first attempt worked in its own worktree, committed 1 file and ended.
    /// The task is `done`, and its task branch is `in_bundle`.
    fn new() -> World {
        let tmp = tempfile::tempdir().unwrap();
        let top = tmp.path().canonicalize().unwrap();
        let root = top.join("repo");
        let fleet_root = top.join("fleet");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&fleet_root).unwrap();
        git(&root, &["init", "--quiet", "-b", "main"]);
        std::fs::write(root.join("a.txt"), "base\n").unwrap();
        std::fs::write(root.join(".gitignore"), ".worktrees/\n").unwrap();
        git(&root, &["add", "a.txt", ".gitignore"]);
        git(&root, &["commit", "--quiet", "-m", "base"]);
        git(&root, &["branch", RUN_BRANCH]);
        let ledger = top.join("ledger.sqlite3");
        let mut store = Store::open(&ledger).unwrap();
        let run = store
            .immediate(|tx| {
                let run = runs::create(
                    tx,
                    &root,
                    &NewRun {
                        slug: "r1".into(),
                        base_branch: Some("main".into()),
                        run_branch: Some(RUN_BRANCH.into()),
                        ..Default::default()
                    },
                )?;
                let feature = NewFeature {
                    id: FEATURE,
                    name: Some("r1"),
                    branch: Some(RUN_BRANCH),
                    primary_branch: "main",
                };
                rows::insert_feature(tx, None, &feature, cause("feature accepted"))?;
                Ok(run)
            })
            .unwrap();
        let mut w = World {
            _tmp: tmp,
            root,
            fleet_root,
            ledger,
            store,
            run,
            config: FleetConfig::default(),
            task: String::new(),
            branch: String::new(),
            tree: PathBuf::new(),
        };
        w.first_attempt_done();
        w
    }

    fn jobs(&self) -> JobCtx<'_> {
        JobCtx {
            guard: None,
            config: &self.config,
            needs_you: &NeedsYouList,
        }
    }

    /// The task, its first attempt, its tree with 1 commit, the attempt
    /// ended, the task `done` and its branch `in_bundle`.
    fn first_attempt_done(&mut self) {
        let mut t = NewTask::work("t1", "docs/t1.md", PERSONA);
        t.ticket_snapshot = Some(SNAPSHOT.into());
        t.land_check = Some(CHECK.into());
        let run = self.run.clone();
        let task = self
            .store
            .immediate(|tx| Ok(tasks::add(tx, &run, &t, Actor::Orchestrator)?.task_id))
            .unwrap();
        let config = self.config.clone();
        let ledger_git = Git::new("git");
        let ctx = DispatchCtx {
            jobs: JobCtx {
                guard: None,
                config: &config,
                needs_you: &NeedsYouList,
            },
            clock: FleetSeconds(0),
            git: &ledger_git,
        };
        let bound = self
            .store
            .immediate(|tx| bind(tx, &ctx, &task, PERSONA, &NewWorker::default()))
            .unwrap();
        let tree = bound.worktree.clone().unwrap();
        let mut conn = Connection::open(&self.ledger).unwrap();
        create::finish_reserved(&mut conn, &self.jobs(), &TestJobs, &tree.worktree_id).unwrap();
        let path = PathBuf::from(&tree.path);
        std::fs::write(path.join("b.txt"), "work\n").unwrap();
        git(&path, &["add", "b.txt"]);
        git(&path, &["commit", "--quiet", "-m", "work"]);
        let now = util::now();
        conn.execute(
            "UPDATE attempts SET state = 'ended', outcome = 'verified', ended_at = ?2
             WHERE id = ?1",
            params![bound.attempt_id, now],
        )
        .unwrap();
        conn.execute(
            "UPDATE workers SET status = 'done' WHERE worker_id = ?1",
            [&bound.worker_id],
        )
        .unwrap();
        conn.execute(
            "UPDATE tasks SET status = 'done' WHERE task_id = ?1",
            [&task],
        )
        .unwrap();
        conn.execute("DELETE FROM leases WHERE task_id = ?1", [&task])
            .unwrap();
        self.task = task;
        self.branch = tree.branch;
        self.tree = path;
        self.enter(&[
            LandingState::Verifying,
            LandingState::Ready,
            LandingState::InBundle,
        ]);
    }

    /// Move the task branch along `path` through the 1 door, as the use
    /// cases of other tickets move it.
    fn enter(&mut self, path: &[LandingState]) {
        let config = self.config.clone();
        let branch = self.branch.clone();
        self.store
            .immediate(|tx| {
                let ctx = JobCtx {
                    guard: None,
                    config: &config,
                    needs_you: &NeedsYouList,
                };
                for to in path {
                    landing::enter(&ctx, tx, &branch, *to, FleetSeconds(0), "test")?;
                }
                Ok(())
            })
            .unwrap();
    }

    /// A job row of `kind` that ended `state` with `result`.
    fn ended_job(&self, kind: &str, subject: &str, state: &str, result: &serde_json::Value) -> i64 {
        let conn = self.store.conn();
        conn.execute(
            "INSERT INTO jobs (kind, subject, repo, input_key, state, time_limit_s, result_json,
                created_at)
             VALUES (?1, ?2, ?3, 'k', ?4, 600, ?5, ?6)",
            params![
                kind,
                subject,
                self.root.display().to_string(),
                state,
                result.to_string(),
                util::now()
            ],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    /// A bundle of the task branch in `state`, with the job `job`.
    fn bundle(&self, state: &str, job: i64) -> i64 {
        let conn = self.store.conn();
        let base = git(&self.root, &["rev-parse", RUN_BRANCH]);
        conn.execute(
            "INSERT INTO bundles (feature_id, base_tip, state, job_id, due_at, at)
             VALUES (?1, ?2, ?3, ?4, 0, ?5)",
            params![FEATURE, base, state, job, util::now()],
        )
        .unwrap();
        let id = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO bundle_members (bundle_id, branch, position) VALUES (?1, ?2, 0)",
            params![id, self.branch],
        )
        .unwrap();
        id
    }

    /// The build of a bundle of the branch conflicted on `files`: the
    /// branch is `conflicted`.
    fn conflicted(&mut self, files: &[&str]) {
        let data = serde_json::json!({
            "passed": false,
            "verdict": true,
            "text": "the merge conflicts",
            "data": {"conflict": {"member": self.branch, "files": files}},
        });
        let job = self.ended_job("bundle_build", "1", "failed", &data);
        self.bundle("dropped", job);
        self.enter(&[LandingState::Conflicted]);
    }

    /// The fast gate of a bundle of the branch alone failed with the output
    /// `output`: the branch is `gate_failed`.
    fn gate_failed(&mut self, output: &str) {
        let data = serde_json::json!({"passed": false, "verdict": true, "text": "exit 101"});
        let job = self.ended_job("fast_gate", FEATURE, "failed", &data);
        let log = self
            .fleet_root
            .join("gate-output")
            .join(format!("{job}.log"));
        std::fs::create_dir_all(log.parent().unwrap()).unwrap();
        std::fs::write(&log, output).unwrap();
        let data = serde_json::json!({
            "passed": false,
            "verdict": true,
            "text": format!("`cargo test` on x in y: exit 101; output in {}", log.display()),
        });
        self.store
            .conn()
            .execute(
                "UPDATE jobs SET result_json = ?2 WHERE id = ?1",
                params![job, data.to_string()],
            )
            .unwrap();
        self.bundle("failed", job);
        self.enter(&[LandingState::GateFailed]);
    }

    /// 1 pass of the repair start at fleet clock `clock`, committed.
    fn start(&mut self, clock: i64) -> Vec<Started> {
        let config = self.config.clone();
        let git = Git::new("git");
        let ctx = DispatchCtx {
            jobs: JobCtx {
                guard: None,
                config: &config,
                needs_you: &NeedsYouList,
            },
            clock: FleetSeconds(clock),
            git: &git,
        };
        self.store
            .immediate(|tx| repair::start_due(tx, &ctx, 8))
            .unwrap()
    }

    /// 1 pass of the repair results at fleet clock `clock`, committed.
    fn observe(&mut self, clock: i64) -> Report {
        let config = self.config.clone();
        let tip = git(&self.root, &["rev-parse", &self.branch]);
        let tips = BTreeMap::from([(self.task.clone(), tip)]);
        let fleet_root = self.fleet_root.clone();
        self.store
            .immediate(|tx| {
                let jobs = JobCtx {
                    guard: None,
                    config: &config,
                    needs_you: &NeedsYouList,
                };
                let r = Results {
                    jobs: &jobs,
                    clock: FleetSeconds(clock),
                    tips: &tips,
                    fleet_root: &fleet_root,
                };
                repair::observe(&r, tx)
            })
            .unwrap()
    }

    fn task_status(&self) -> String {
        tasks::get(self.store.conn(), &self.task).unwrap().status
    }

    fn landing_state(&self) -> String {
        rows::branch(self.store.conn(), &self.branch)
            .unwrap()
            .unwrap()
            .landing_state
            .unwrap()
    }

    /// The attempts of the task, oldest first: (id, state, via).
    fn attempts(&self) -> Vec<(String, String, String)> {
        let conn = self.store.conn();
        let mut stmt = conn
            .prepare("SELECT id, state, via FROM attempts WHERE task_id = ?1 ORDER BY rowid")
            .unwrap();
        let rows = stmt
            .query_map([&self.task], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap();
        rows.collect::<rusqlite::Result<_>>().unwrap()
    }

    /// `START.md` of the attempt `attempt`, written as part 3 writes it.
    fn start_md(&self, attempt: &str) -> String {
        let files = write_task_files(self.store.conn(), &self.fleet_root, attempt, &[]).unwrap();
        std::fs::read_to_string(files.start_md).unwrap()
    }

    /// The repair attempt commits `file` in the tree and writes its
    /// `result.json`, which the ingest accepted.
    fn result_of(&self, attempt: &str, file: &str) {
        std::fs::write(self.tree.join(file), "fixed\n").unwrap();
        git(&self.tree, &["add", file]);
        git(&self.tree, &["commit", "--quiet", "-m", "repair"]);
        let a = AttemptRef {
            task_id: self.task.clone(),
            attempt_id: attempt.to_string(),
        };
        let path = a.outbox(&self.fleet_root).join(RESULT_JSON);
        self.store
            .conn()
            .execute(
                "INSERT INTO ingested (path, sha256, outcome, reason, at)
                 VALUES (?1, 'abc', 'accepted', NULL, ?2)",
                params![path.display().to_string(), util::now()],
            )
            .unwrap();
    }

    /// The `task_check` job of the task ends `state`.
    fn end_check(&self, state: &str) {
        let job = jobs_store::last(self.store.conn(), Kind::TaskCheck, &self.task)
            .unwrap()
            .expect("a task_check job of the repair result");
        let passed = state == "passed";
        let data = serde_json::json!({"passed": passed, "verdict": true, "text": "the check ran"});
        self.store
            .conn()
            .execute(
                "UPDATE jobs SET state = ?2, result_json = ?3 WHERE id = ?1",
                params![job.id, state, data.to_string()],
            )
            .unwrap();
    }

    fn lease_of_task(&self) -> Option<String> {
        leases_store::get(self.store.conn(), &self.task)
            .unwrap()
            .map(|l| l.attempt_id)
    }

    fn check_i1(&mut self, clock: i64) {
        self.store
            .immediate(|tx| lease::check_i1(tx, None, FleetSeconds(clock), &|_| LIMITS))
            .unwrap();
    }

    /// 1 renewal pass at `clock` in which the attempt `attempt` shows
    /// `process` and no other signal.
    fn renew_or_expire(&mut self, clock: i64, attempt: &str, process: Seen) {
        let facts = BTreeMap::from([(
            attempt.to_string(),
            Observed {
                process,
                heartbeat_age_s: None,
                active: false,
            },
        )]);
        self.store
            .immediate(|tx| {
                lease::renew_or_expire_leases(tx, None, FleetSeconds(clock), &facts, &|_| LIMITS)
            })
            .unwrap();
    }

    fn attempt_row(&self, attempt: &str) -> (String, Option<String>) {
        self.store
            .conn()
            .query_row(
                "SELECT state, outcome FROM attempts WHERE id = ?1",
                [attempt],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap()
    }
}

/// The only repair attempt that `started` holds.
fn only(started: &[Started]) -> &Started {
    assert_eq!(started.len(), 1, "1 repair attempt: {started:?}");
    &started[0]
}

// ─── the tests ──────────────────────────────────────────────────────────

#[test]
fn conflicted_branch_gets_1_resolver_attempt_and_the_task_stays_done() {
    let mut w = World::new();
    w.conflicted(&["a.txt"]);

    let started = w.start(10);
    let s = only(&started);
    assert_eq!(s.kind, RepairKind::Resolver);
    assert_eq!(s.branch, w.branch);
    assert_eq!(w.task_status(), "done", "the task stays done");
    assert_eq!(
        w.landing_state(),
        "conflicted",
        "the branch waits for its result"
    );

    let attempts = w.attempts();
    assert_eq!(
        attempts.len(),
        2,
        "the first attempt and the resolver: {attempts:?}"
    );
    let (id, state, via) = &attempts[1];
    assert_eq!(id, &s.bound.attempt_id);
    assert_eq!((state.as_str(), via.as_str()), ("launching", "push"));
    assert_eq!(
        repair::repair_kind(w.store.conn(), id).unwrap(),
        Some(RepairKind::Resolver)
    );
    assert_eq!(
        repair::repair_kind(w.store.conn(), &attempts[0].0).unwrap(),
        None,
        "the first attempt is a work attempt"
    );

    // The attempt works in the tree of the task.
    let tree = s.bound.worktree.as_ref().expect("the tree of the task");
    assert_eq!(PathBuf::from(&tree.path), w.tree);
    assert_eq!(tree.branch, w.branch);
    assert_eq!(tree.add_job, None, "the tree exists");
    let row = trees::of_task(w.store.conn(), &w.task).unwrap().unwrap();
    assert_eq!(row.owner_attempt.as_deref(), Some(id.as_str()));

    // A second pass starts no second attempt.
    assert!(w.start(11).is_empty());
    assert_eq!(w.attempts().len(), 2);
}

#[test]
fn resolver_start_md_names_the_conflict_files() {
    let mut w = World::new();
    w.conflicted(&["a.txt", "src/lib.rs"]);
    let started = w.start(10);
    let attempt = only(&started).bound.attempt_id.clone();

    let md = w.start_md(&attempt);
    assert!(md.starts_with(SNAPSHOT), "the ticket comes first:\n{md}");
    assert!(md.contains("## Resolver attempt"), "{md}");
    assert!(
        md.contains("`a.txt`") && md.contains("`src/lib.rs`"),
        "{md}"
    );
    assert!(md.contains("zdiff3"), "the conflict form: {md}");
    assert!(md.contains(&format!("git merge {RUN_BRANCH}")), "{md}");
    assert!(md.contains(CHECK), "the tests of the task side: {md}");
    assert!(
        md.contains("fast gate"),
        "the tests of the feature side: {md}"
    );
}

#[test]
fn gate_failed_branch_gets_1_fix_attempt_with_the_gate_output() {
    let mut w = World::new();
    w.gate_failed("running 3 tests\ntest parse::empty ... FAILED\nexit 101\n");

    let started = w.start(10);
    let s = only(&started);
    assert_eq!(s.kind, RepairKind::Fix);
    assert_eq!(w.task_status(), "done");
    assert_eq!(w.landing_state(), "gate_failed");
    assert!(w.start(11).is_empty(), "1 fix attempt");

    let md = w.start_md(&s.bound.attempt_id);
    assert!(md.contains("## Fix attempt"), "{md}");
    assert!(
        md.contains("test parse::empty ... FAILED"),
        "the gate output: {md}"
    );
    assert!(md.contains("gate-output"), "the path of the output: {md}");
}

#[test]
fn passed_resolver_result_moves_the_branch_to_ready() {
    let mut w = World::new();
    w.conflicted(&["a.txt"]);
    let attempt = only(&w.start(10)).bound.attempt_id.clone();

    // No result yet: nothing moves.
    let r = w.observe(11);
    assert!(r.queued.is_empty() && r.ready.is_empty(), "{r:?}");

    w.result_of(&attempt, "a.txt");
    let r = w.observe(12);
    assert_eq!(
        r.queued,
        vec![w.branch.clone()],
        "the check of the task runs"
    );
    assert_eq!(w.landing_state(), "conflicted");
    assert!(
        w.start(13).is_empty(),
        "no second attempt while its check runs"
    );

    w.end_check("passed");
    let r = w.observe(14);
    assert_eq!(r.ready, vec![w.branch.clone()]);
    assert_eq!(w.landing_state(), "ready");
    assert_eq!(w.task_status(), "done", "the branch moves, not the task");
}

#[test]
fn passed_fix_result_moves_the_branch_through_verifying_to_ready() {
    let mut w = World::new();
    w.gate_failed("test x ... FAILED\n");
    let attempt = only(&w.start(10)).bound.attempt_id.clone();
    w.result_of(&attempt, "b.txt");
    assert_eq!(w.observe(11).queued, vec![w.branch.clone()]);
    w.end_check("passed");
    assert_eq!(w.observe(12).ready, vec![w.branch.clone()]);
    assert_eq!(w.landing_state(), "ready");
    let path: Vec<String> = {
        let conn = w.store.conn();
        let mut stmt = conn
            .prepare(
                "SELECT to_state FROM events WHERE entity = 'task_branch' AND entity_id = ?1 AND to_state IS NOT NULL
                   AND kind <> 'note'
                 ORDER BY event_id",
            )
            .unwrap();
        let rows = stmt.query_map([&w.branch], |r| r.get(0)).unwrap();
        rows.collect::<rusqlite::Result<_>>().unwrap()
    };
    assert!(
        path.ends_with(&["gate_failed".into(), "verifying".into(), "ready".into()]),
        "{path:?}"
    );
}

#[test]
fn failed_repair_check_moves_nothing() {
    let mut w = World::new();
    w.conflicted(&["a.txt"]);
    let attempt = only(&w.start(10)).bound.attempt_id.clone();
    w.result_of(&attempt, "a.txt");
    w.observe(11);
    w.end_check("failed");
    let r = w.observe(12);
    assert_eq!(r.failed, vec![w.branch.clone()]);
    assert_eq!(w.landing_state(), "conflicted");
    assert_eq!(w.task_status(), "done");
}

#[test]
fn repair_attempt_has_a_lease() {
    let mut w = World::new();
    w.conflicted(&["a.txt"]);
    let attempt = only(&w.start(10)).bound.attempt_id.clone();
    assert_eq!(
        w.lease_of_task(),
        Some(attempt.clone()),
        "the bind takes the lease"
    );

    // I1 keeps the lease of the live repair attempt of a done task.
    w.check_i1(20);
    assert_eq!(w.lease_of_task(), Some(attempt.clone()));

    // I1 writes it again when it is missing.
    w.store
        .conn()
        .execute("DELETE FROM leases WHERE task_id = ?1", [&w.task])
        .unwrap();
    w.check_i1(30);
    assert_eq!(w.lease_of_task(), Some(attempt));
}

#[test]
fn done_task_with_no_live_attempt_holds_no_lease() {
    let mut w = World::new();
    w.conflicted(&["a.txt"]);
    let attempt = only(&w.start(10)).bound.attempt_id.clone();
    w.store
        .conn()
        .execute(
            "UPDATE attempts SET state = 'ended', outcome = 'crashed' WHERE id = ?1",
            [&attempt],
        )
        .unwrap();
    w.check_i1(20);
    assert_eq!(
        w.lease_of_task(),
        None,
        "I1 deletes the lease of an ended attempt"
    );

    // A done task whose branch needs no repair holds no lease either.
    let mut w = World::new();
    let first = w.attempts()[0].0.clone();
    w.store
        .conn()
        .execute(
            "INSERT INTO leases (task_id, attempt_id, expires_at, renewed_at, renew_basis)
             VALUES (?1, ?2, 100, ?3, 'claim')",
            params![w.task, first, util::now()],
        )
        .unwrap();
    w.check_i1(20);
    assert_eq!(w.lease_of_task(), None);
}

#[test]
fn expiry_of_a_repair_lease_ends_the_attempt_and_does_not_move_the_task() {
    let mut w = World::new();
    w.gate_failed("test x ... FAILED\n");
    let attempt = only(&w.start(10)).bound.attempt_id.clone();

    // Inside its lease, a pass with no signal keeps it.
    w.renew_or_expire(100, &attempt, Seen::Dead);
    assert_eq!(w.lease_of_task(), Some(attempt.clone()));
    assert_eq!(w.attempt_row(&attempt).0, "launching");

    // Past its lease, the attempt ends `stalled` and frees the lease; the
    // task stays done, and the branch stays gate_failed.
    w.renew_or_expire(10 + LIMITS.lease_s + 1, &attempt, Seen::Dead);
    assert_eq!(
        w.attempt_row(&attempt),
        ("ended".into(), Some("stalled".into()))
    );
    assert_eq!(w.lease_of_task(), None);
    assert_eq!(w.task_status(), "done");
    assert_eq!(w.landing_state(), "gate_failed");
}

/// Step 3 of the ticket: a crash before the commit of the bind leaves no
/// row, and the next pass binds 1 attempt. A crash after the commit and
/// before the task files and the launch leaves the attempt, and the next
/// pass writes its files 1 time and binds no second attempt.
#[test]
fn crash_before_and_after_the_commit_of_the_bind_loses_no_attempt() {
    let mut w = World::new();
    w.conflicted(&["a.txt"]);

    // Fault point 1: the bind runs, and fleetd dies before the commit.
    {
        let config = w.config.clone();
        let git = Git::new("git");
        let ctx = DispatchCtx {
            jobs: JobCtx {
                guard: None,
                config: &config,
                needs_you: &NeedsYouList,
            },
            clock: FleetSeconds(10),
            git: &git,
        };
        let mut conn = Connection::open(&w.ledger).unwrap();
        let tx = conn.transaction().unwrap();
        assert_eq!(repair::start_due(&tx, &ctx, 8).unwrap().len(), 1);
        drop(tx);
    }
    assert_eq!(w.attempts().len(), 1, "no row of the bind is left");
    assert_eq!(w.lease_of_task(), None);

    // Fault point 2: the bind commits, and fleetd dies before the files.
    let attempt = only(&w.start(11)).bound.attempt_id.clone();
    let written = write_missing_task_files(w.store.conn(), &w.fleet_root, &[]).unwrap();
    assert_eq!(
        written.len(),
        1,
        "the next pass writes the files: {written:?}"
    );
    let md = std::fs::read_to_string(written[0].1.as_ref().unwrap().start_md.clone()).unwrap();
    assert!(md.contains("## Resolver attempt"), "{md}");
    assert!(
        write_missing_task_files(w.store.conn(), &w.fleet_root, &[])
            .unwrap()
            .is_empty(),
        "1 time"
    );
    assert!(w.start(12).is_empty(), "no second attempt");
    assert_eq!(w.attempts().len(), 2);
    assert_eq!(w.lease_of_task(), Some(attempt));
}

#[test]
fn branch_of_a_task_that_is_not_done_gets_no_repair_attempt() {
    let mut w = World::new();
    w.conflicted(&["a.txt"]);
    w.store
        .conn()
        .execute(
            "UPDATE tasks SET status = 'cancelled' WHERE task_id = ?1",
            [&w.task],
        )
        .unwrap();
    assert!(w.start(10).is_empty());
}
