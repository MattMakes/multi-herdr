//! The limit of the resolver and fix attempts of a task branch: a failed
//! attempt (a failed check, a stall, a crash, no result, a lease expiry)
//! does not move the `done` task. It adds 1 to `repair_attempts` of the
//! branch, and the next attempt starts after the backoff. At `max_attempts`
//! of the task, 1 needs-you item opens. `horch task requeue` of the `done`
//! task sets `repair_attempts` to 0 and starts 1 more attempt
//! (docs/specs/fleetd/design.md, "Landing a task branch on its feature
//! branch", "Leases, not claims", FM-38, RR-6;
//! docs/tickets/fleetd-p4/34-repair-attempt-failures.md).
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
use horch_core::fleetd::ideas::application::needs_you::{self, NeedsYouList};
use horch_core::fleetd::integration::adapters::sqlite::{self as rows, NewFeature};
use horch_core::fleetd::integration::application::landing;
use horch_core::fleetd::integration::domain::task_branch::LandingState;
use horch_core::fleetd::task_execution::adapters::result_reader::{AttemptRef, RESULT_JSON};
use horch_core::fleetd::task_execution::application::cancel;
use horch_core::fleetd::task_execution::application::dispatch::{bind, DispatchCtx};
use horch_core::fleetd::task_execution::application::lease::{self, Observed};
use horch_core::fleetd::task_execution::application::repair::{
    self, RepairKind, Report, Results, Started,
};
use horch_core::fleetd::task_execution::domain::lease::Limits;
use horch_core::fleetd::task_execution::domain::repair::REPAIR_LIMIT_KIND;
use horch_core::fleetd::task_execution::domain::task::Seen;
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

    /// The `max_attempts` of the run of the task.
    fn max_attempts(&self, n: i64) {
        self.store
            .conn()
            .execute(
                "UPDATE runs SET max_attempts = ?1 WHERE run_id = ?2",
                params![n, self.run.run_id],
            )
            .unwrap();
    }

    fn repair_attempts(&self) -> i64 {
        rows::branch(self.store.conn(), &self.branch)
            .unwrap()
            .unwrap()
            .repair_attempts
    }

    /// The attempt `attempt` ends `outcome` with no result: a crash.
    fn crash(&self, attempt: &str) {
        self.store
            .conn()
            .execute(
                "UPDATE attempts SET state = 'ended', outcome = 'crashed', ended_at = ?2
                 WHERE id = ?1",
                params![attempt, util::now()],
            )
            .unwrap();
    }

    /// The open needs-you items of the repair limit, as (key, entity id).
    fn limit_items(&self) -> Vec<(String, String)> {
        needs_you::open_items(self.store.conn())
            .unwrap()
            .into_iter()
            .filter(|i| i.kind == REPAIR_LIMIT_KIND)
            .map(|i| (i.key, i.entity_id))
            .collect()
    }

    /// `horch task requeue` of the task, as fleetd applies its request.
    fn requeue(&mut self) -> anyhow::Result<String> {
        let config = self.config.clone();
        let task = self.task.clone();
        self.store.immediate(|tx| {
            let jobs = JobCtx {
                guard: None,
                config: &config,
                needs_you: &NeedsYouList,
            };
            let t = tasks::get(tx, &task)?;
            Ok(cancel::requeue(tx, &jobs, &t, "the operator fixed the base")?.status)
        })
    }
}

/// The only repair attempt that `started` holds.
fn only(started: &[Started]) -> &Started {
    assert_eq!(started.len(), 1, "1 repair attempt: {started:?}");
    &started[0]
}

// ─── the tests ──────────────────────────────────────────────────────────

#[test]
fn failed_resolver_attempt_starts_the_next_after_the_backoff() {
    let mut w = World::new();
    w.max_attempts(3);
    w.conflicted(&["a.txt"]);
    let first = only(&w.start(10)).bound.attempt_id.clone();

    // The result of the resolver fails its check at fleet clock 12.
    w.result_of(&first, "a.txt");
    w.observe(11);
    w.end_check("failed");
    let r = w.observe(12);
    assert_eq!(r.failed, vec![w.branch.clone()]);
    assert_eq!(w.repair_attempts(), 1);
    w.observe(13);
    assert_eq!(w.repair_attempts(), 1, "1 failure counts 1 time");
    assert_eq!(
        w.task_status(),
        "done",
        "the failure does not move the task"
    );
    assert_eq!(w.landing_state(), "conflicted");

    // The process of the attempt exits. The backoff of 30 s holds the next
    // attempt until fleet clock 42.
    w.crash(&first);
    w.observe(14);
    assert_eq!(w.repair_attempts(), 1, "the attempt failed 1 time");
    assert!(w.start(41).is_empty(), "the backoff holds the next attempt");
    let second = only(&w.start(42)).clone();
    assert_eq!(second.kind, RepairKind::Resolver);
    assert_ne!(second.bound.attempt_id, first);
    assert_eq!(w.task_status(), "done");

    // A crash with no result is a failure too: 2 min of backoff.
    w.crash(&second.bound.attempt_id);
    let r = w.observe(50);
    assert_eq!(r.failed, vec![w.branch.clone()]);
    assert_eq!(w.repair_attempts(), 2);
    assert!(w.start(169).is_empty());
    assert_eq!(w.start(170).len(), 1);
    assert!(w.limit_items().is_empty());
}

#[test]
fn lease_expiry_of_a_fix_attempt_does_not_move_the_done_task() {
    let mut w = World::new();
    w.max_attempts(3);
    w.gate_failed("test x ... FAILED\n");
    let first = only(&w.start(10)).bound.attempt_id.clone();

    let expiry = 10 + LIMITS.lease_s + 1;
    w.renew_or_expire(expiry, &first, Seen::Dead);
    assert_eq!(w.task_status(), "done", "the expiry does not move the task");
    assert_eq!(w.landing_state(), "gate_failed");

    let r = w.observe(expiry);
    assert_eq!(r.failed, vec![w.branch.clone()]);
    assert_eq!(w.repair_attempts(), 1);
    assert!(w.start(expiry + 29).is_empty());
    let next = only(&w.start(expiry + 30)).clone();
    assert_eq!(next.kind, RepairKind::Fix);
    assert_eq!(w.task_status(), "done");
}

#[test]
fn at_max_attempts_1_needs_you_item_opens() {
    let mut w = World::new();
    w.max_attempts(2);
    w.conflicted(&["a.txt"]);
    let first = only(&w.start(10)).bound.attempt_id.clone();
    w.crash(&first);
    w.observe(11);
    let second = only(&w.start(41)).bound.attempt_id.clone();
    w.crash(&second);
    w.observe(50);
    assert_eq!(w.repair_attempts(), 2);

    // At max_attempts no attempt starts, and 1 item opens.
    assert!(w.start(170).is_empty());
    assert_eq!(
        w.limit_items(),
        vec![(
            format!("{REPAIR_LIMIT_KIND}:{}", w.branch),
            w.branch.clone()
        )]
    );
    w.observe(10_000);
    assert!(w.start(10_000).is_empty(), "RR-6: the retries end");
    assert_eq!(w.limit_items().len(), 1, "1 item, not 1 for each pass");
    assert_eq!(w.repair_attempts(), 2);
    assert_eq!(w.task_status(), "done");
    assert_eq!(w.landing_state(), "conflicted");
}

#[test]
fn requeue_of_a_done_task_resets_repair_attempts_and_starts_1_more() {
    // A done task with no branch to repair is not held: the requeue refuses.
    let mut w = World::new();
    let e = w.requeue().unwrap_err().to_string();
    assert!(e.contains("no branch"), "{e}");

    let mut w = World::new();
    w.max_attempts(1);
    w.conflicted(&["a.txt"]);
    let first = only(&w.start(10)).bound.attempt_id.clone();
    w.crash(&first);
    w.observe(11);
    assert!(w.start(100).is_empty());
    assert_eq!(w.limit_items().len(), 1);

    assert_eq!(w.requeue().unwrap(), "done", "the task stays done");
    assert_eq!(w.repair_attempts(), 0);

    // The next pass starts 1 more attempt with no backoff, and the item
    // resolves.
    let next = only(&w.start(101)).bound.attempt_id.clone();
    assert_ne!(next, first);
    assert!(w.limit_items().is_empty());

    // It fails too: the limit holds again.
    w.crash(&next);
    w.observe(102);
    assert_eq!(w.repair_attempts(), 1);
    assert!(w.start(10_000).is_empty());
    assert_eq!(w.limit_items().len(), 1);
}
