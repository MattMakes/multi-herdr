//! The review steps after the check of a push attempt
//! (docs/tickets/fleetd-p4/20-review-steps-carry-over.md): the verifier runs
//! the steps of the run's verification profile that come before `land`,
//! with today's code of `taskledger::finish`, and a code kind moves to
//! `done` only after them (docs/specs/fleetd/design.md, "Done for each kind
//! of task", D6, RR-18; preservation-04 rows F1, F2, F5, F12;
//! preservation-05 row V-14).
//!
//! Each test uses a temp git repository, a temp ledger and a temp fleet
//! root. A pass is part 2 (the verifier, in 1 transaction) and part 3 (the
//! review steps, after the commit). A `task_check` job ends through its
//! row, as the job process ends it; the complexity-sweep job runs its
//! executor in the test process. No test reads the load, the limits or the
//! clock of this machine: the fleet clock is a fact of the test. The fake
//! analyser is a shell script, so the tests run on Unix only.
#![cfg(unix)]

#[path = "common/test_jobs.rs"]
mod test_jobs;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::Result;
use horch_core::fleetd::fleet_runtime::adapters::{job_result_file, jobs_store};
use horch_core::fleetd::fleet_runtime::application::jobs::{
    self, CheckRunner, JobCtx, SandboxProfiles,
};
use horch_core::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;
use horch_core::fleetd::fleet_runtime::domain::jobs::State;
use horch_core::fleetd::ideas::application::needs_you::NeedsYouList;
use horch_core::fleetd::task_execution::adapters::verdict_file;
use horch_core::fleetd::task_execution::application::retry::Policy;
use horch_core::fleetd::task_execution::application::review::{self, Review, Reviewed};
use horch_core::fleetd::task_execution::application::verifier::{self, Verifier};
use horch_core::fleetd::worker_profiles::adapters::launcher::Launcher;
use horch_core::fleetd::worker_profiles::application::ports::LaunchSpec;
use horch_core::taskledger::finish::{self, CodexSandbox, Tools};
use horch_core::taskledger::git::Git;
use horch_core::taskledger::model::{
    Actor, ReviewMode, StepName, VerifyProfile, WorkKind, WorkerKind,
};
use horch_core::taskledger::runs::{self, NewRun, Run};
use horch_core::taskledger::tasks::{self, NewTask, Task};
use horch_core::taskledger::workers::{self, NewWorker, Worker};
use horch_core::taskledger::{util, Store};
use rusqlite::{params, Connection};
use test_jobs::TestJobs;

use StepName::{CodeSweep, ComplexitySweep, Recheck};

const RUN_BRANCH: &str = "fleet/r1";

/// No finding.
const NONE: &str = "## Critical\n- none\n## Important\n- none\n## Minor\n- none\n";
/// 1 Important finding.
const IMPORTANT: &str =
    "## Critical\n- none\n## Important\n- lib.ts:1 off by one. Fix: <=\n## Minor\n- none\n";

/// The reviewer prompts of the code-sweep skill.
const REVIEWERS: &[&str] = &[
    "bug-hunter",
    "vulnerability-scanner",
    "structure-inspector",
    "efficiency-auditor",
    "clarity-checker",
    "coverage-analyst",
    "contract-guardian",
    "waste-detector",
    "reliability-auditor",
    "dependency-examiner",
    "power-of-ten-reviewer",
    "interface-auditor",
];

fn git(dir: &Path, args: &[&str]) -> String {
    let o = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
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

/// Write `file` in `dir`, commit it, and give the new HEAD.
fn commit(dir: &Path, file: &str, body: &str) -> String {
    std::fs::write(dir.join(file), body).unwrap();
    git(dir, &["add", file]);
    git(dir, &["commit", "--quiet", "-m", file]);
    git(dir, &["rev-parse", "HEAD"])
}

/// The policy of rung 6 of each task: the design defaults.
fn policy(_: &Task) -> Policy {
    Policy::default()
}

struct TestProfile;

impl SandboxProfiles for TestProfile {
    fn check_launch(&self, _: &Worker, dir: &Path, check: &str) -> Result<Option<LaunchSpec>> {
        Ok(Some(LaunchSpec {
            program: "/bin/sh".into(),
            args: vec!["-c".into(), check.into()],
            cwd: Some(dir.to_path_buf()),
            env_rule: Default::default(),
            detach: false,
            output: None,
        }))
    }
}

struct World {
    tmp: tempfile::TempDir,
    ledger: PathBuf,
    store: Store,
    run: Run,
    tools: Tools,
    config: FleetConfig,
}

impl World {
    /// A git repository with the run branch, and 1 run with the steps of
    /// `profile`. The analyser of the complexity-sweep finds nothing.
    fn new(profile: VerifyProfile) -> World {
        let tmp = tempfile::tempdir().unwrap();
        let top = tmp.path().canonicalize().unwrap();
        let root = top.join("repo");
        std::fs::create_dir_all(&root).unwrap();
        git(&root, &["init", "--quiet", "-b", "main"]);
        for (k, v) in [
            ("user.name", "t"),
            ("user.email", "t@example.com"),
            ("commit.gpgsign", "false"),
        ] {
            git(&root, &["config", k, v]);
        }
        commit(&root, "README.md", "base\n");
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
                        finish_steps: profile.steps(),
                        ..Default::default()
                    },
                )?;
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
        let sweep = top.join("code-sweep");
        std::fs::create_dir_all(sweep.join("reviewers")).unwrap();
        for r in REVIEWERS {
            std::fs::write(sweep.join("reviewers").join(format!("{r}.md")), "x\n").unwrap();
        }
        let tools = Tools {
            git: Git::new("git"),
            gh: top.join("gh"),
            // The analyser is `cli.js`: `node` runs it, and `/bin/sh` is node.
            node: "/bin/sh".into(),
            complexity_cli: Some(top.join("cli.js")),
            code_sweep_dir: Some(sweep),
            evidence_root: top.join("evidence"),
            shell: "/bin/sh".into(),
            reviewers_down: Vec::new(),
            codex: CodexSandbox::default(),
            jobs: Some(std::sync::Arc::new(TestJobs)),
        };
        let w = World {
            tmp,
            ledger,
            store,
            run,
            tools,
            config: FleetConfig::default(),
        };
        w.analyser(r#"echo '{"files":[],"violations":[]}'"#);
        w
    }

    fn top(&self) -> PathBuf {
        self.tmp.path().canonicalize().unwrap()
    }

    /// The script of the analyser of the complexity-sweep.
    fn analyser(&self, body: &str) {
        std::fs::write(self.top().join("cli.js"), format!("{body}\n")).unwrap();
    }

    fn conn(&self) -> &Connection {
        self.store.conn()
    }

    /// A task of the code kind `feature` with the check `true` and the
    /// review mode `mode`, claimed by a worker, with its worktree and 1
    /// commit of `lib.ts`. Its attempt is a push attempt in `verifying`.
    fn task(&mut self, mode: ReviewMode) -> String {
        let mut t = NewTask::work("feature", "t.md", "opus");
        t.land_check = Some("true".into());
        t.work_kind = Some(WorkKind::Feature);
        t.review_mode = Some(mode);
        let run = self.run.clone();
        let worker = self
            .store
            .immediate(|tx| {
                let task = tasks::add(tx, &run, &t, Actor::Orchestrator)?.task_id;
                let new = NewWorker {
                    run_id: run.run_id.clone(),
                    persona: "opus".into(),
                    role: Some("opus-1".into()),
                    ..Default::default()
                };
                let agent = horch_core::fleetd::task_execution::domain::task::Recorded {
                    pid: 4243,
                    pid_start: Some(2),
                };
                let id = horch_core::taskledger::testing::bind_live(tx, &task, &new, Some(agent))?.worker_id;
                tx.execute(
                    "UPDATE workers SET supervisor_pid = 4242, supervisor_started = 1 WHERE worker_id = ?1",
                    [&id],
                )?;
                Ok(id)
            })
            .unwrap();
        let wt =
            finish::worktree_new_with(&mut self.store, &self.tools.git, &worker, "feat", &TestJobs)
                .unwrap();
        commit(Path::new(&wt.path), "lib.ts", "export const a = 1;\n");
        let task = wt.task_id.unwrap();
        self.push_attempt(&task, &worker);
        task
    }

    /// The attempt of `worker` on `task` is a push attempt in `verifying`.
    fn push_attempt(&self, task: &str, worker: &str) {
        let n: i64 = self
            .conn()
            .query_row(
                "SELECT attempts FROM tasks WHERE task_id = ?1",
                [task],
                |r| r.get(0),
            )
            .unwrap();
        self.conn()
            .execute(
                "UPDATE tasks SET status = 'verifying', current_worker_id = ?2 WHERE task_id = ?1",
                params![task, worker],
            )
            .unwrap();
        self.conn()
            .execute(
                "INSERT INTO attempts (id, task_id, worker_id, state, started_at, via)
                 VALUES (?1, ?2, ?3, 'live', ?4, 'push')",
                params![format!("{task}-a{n}"), task, worker, util::now()],
            )
            .unwrap();
    }

    /// The next attempt of `task`, which the review requeued: a new worker,
    /// 1 more attempt, in the same worktree.
    fn next_attempt(&self, task: &str) -> String {
        assert_eq!(self.status(task), "ready");
        let worker = workers::insert(
            self.conn(),
            &NewWorker {
                run_id: self.run.run_id.clone(),
                kind: Some(WorkerKind::Worker),
                persona: "opus".into(),
                target_task_id: Some(task.into()),
                ..Default::default()
            },
        )
        .unwrap();
        self.conn()
            .execute(
                "UPDATE tasks SET attempts = attempts + 1 WHERE task_id = ?1",
                [task],
            )
            .unwrap();
        self.push_attempt(task, &worker);
        worker
    }

    fn worktree(&self, task: &str) -> PathBuf {
        PathBuf::from(
            finish::worktree_of(self.conn(), task)
                .unwrap()
                .unwrap()
                .path,
        )
    }

    /// 1 pass: part 2, the verifier in 1 transaction; part 3, the review
    /// steps after the commit. The tips are the HEADs of the worktrees.
    fn pass(&mut self) -> Vec<(String, Reviewed)> {
        let mut tips = BTreeMap::new();
        for t in tasks::of_run(self.conn(), &self.run.run_id).unwrap() {
            if let Some(wt) = finish::worktree_of(self.conn(), &t.task_id).unwrap() {
                tips.insert(t.task_id, git(Path::new(&wt.path), &["rev-parse", "HEAD"]));
            }
        }
        let ctx = JobCtx {
            guard: None,
            config: &self.config,
            needs_you: &NeedsYouList,
        };
        let v = Verifier {
            jobs: &ctx,
            clock: FleetSeconds(0),
            tips: &tips,
            policy: &policy,
        };
        let report = self.store.immediate(|tx| verifier::verify(&v, tx)).unwrap();
        let r = Review {
            jobs: &ctx,
            clock: FleetSeconds(0),
            tools: &self.tools,
            policy: &policy,
        };
        verifier::review(&r, &mut self.store, report.passed).unwrap()
    }

    /// The `task_check` rows of `task`, oldest first.
    fn jobs_of(&self, task: &str) -> Vec<jobs_store::JobRow> {
        let mut stmt = self
            .conn()
            .prepare("SELECT id FROM jobs WHERE kind = 'task_check' AND subject = ?1 ORDER BY id")
            .unwrap();
        let ids: Vec<i64> = stmt
            .query_map([task], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        ids.into_iter()
            .map(|id| jobs_store::get(self.conn(), id).unwrap().unwrap())
            .collect()
    }

    fn queued(&self, task: &str) -> jobs_store::JobRow {
        self.jobs_of(task)
            .into_iter()
            .find(|r| r.state == State::Queued)
            .expect("a queued task_check run")
    }

    /// The job process of the queued check or recheck of `task` ends
    /// `passed` or `failed`, with a verdict.
    fn end_check(&self, task: &str, passed: bool) {
        let row = self.queued(task);
        assert!(!review::is_complexity_key(&row.input_key));
        let state = if passed { "passed" } else { "failed" };
        let result = format!(r#"{{"passed":{passed},"verdict":true,"text":"`true` in wt: exit"}}"#);
        self.conn()
            .execute(
                "UPDATE jobs SET state = ?2, result_json = ?3, ended_at = ?4 WHERE id = ?1",
                params![row.id, state, result, util::now()],
            )
            .unwrap();
    }

    /// The job process of the queued complexity-sweep of `task`: its
    /// executor runs in this process as `horch job run` runs it, then its
    /// result file ends the row, as the pass ends it.
    fn run_complexity(&self, task: &str) {
        let row = self.queued(task);
        assert!(
            review::is_complexity_key(&row.input_key),
            "{}",
            row.input_key
        );
        let me = std::process::id();
        let conn = Connection::open(&self.ledger).unwrap();
        conn.execute(
            "UPDATE jobs SET state = 'running', pid = ?2, started_at = ?3 WHERE id = ?1",
            params![row.id, me, util::now()],
        )
        .unwrap();
        let ctx = JobCtx {
            guard: None,
            config: &self.config,
            needs_you: &NeedsYouList,
        };
        let checks = CheckRunner {
            host: &Launcher,
            profiles: &TestProfile,
            sweep: Some(&self.tools),
        };
        let o = jobs::run_job(&ctx, &conn, row.id, me, &checks)
            .unwrap()
            .expect("the job process wrote its result");
        let file = job_result_file::path(&self.top().join("fleet"), row.id, row.run);
        let result = std::fs::read_to_string(file).unwrap();
        let state = if o.passed { "passed" } else { "failed" };
        conn.execute(
            "UPDATE jobs SET state = ?2, result_json = ?3, ended_at = ?4 WHERE id = ?1",
            params![row.id, state, result, util::now()],
        )
        .unwrap();
    }

    fn status(&self, task: &str) -> String {
        tasks::get(self.conn(), task).unwrap().status
    }

    fn number(&self, sql: &str, task: &str) -> i64 {
        self.conn().query_row(sql, [task], |r| r.get(0)).unwrap()
    }

    /// The attempts used since the last operator requeue.
    fn used(&self, task: &str) -> i64 {
        self.number(
            "SELECT attempts - attempt_floor FROM tasks WHERE task_id = ?1",
            task,
        )
    }

    fn fix_rounds(&self, task: &str) -> i64 {
        self.number("SELECT fix_rounds FROM tasks WHERE task_id = ?1", task)
    }

    /// The review tasks of `task`: (id, title, findings file, status).
    fn reviews(&self, task: &str) -> Vec<(String, String, String, String)> {
        let mut stmt = self
            .conn()
            .prepare(
                "SELECT task_id, title, spec_ref, status FROM tasks
                 WHERE kind = 'review' AND parent_task_id = ?1 ORDER BY seq",
            )
            .unwrap();
        stmt.query_map([task], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
            .unwrap()
            .map(|r| r.unwrap())
            .collect()
    }

    /// Each open review of `task` is in, with the findings `body` of its
    /// reviewer.
    fn reviews_in(&self, task: &str, body: impl Fn(&str) -> &'static str) {
        for (id, title, file, status) in self.reviews(task) {
            if status == "done" || status == "cancelled" {
                continue;
            }
            std::fs::write(&file, body(&title)).unwrap();
            self.set_status(&id, "done");
        }
    }

    fn set_status(&self, task: &str, status: &str) {
        self.conn()
            .execute(
                "UPDATE tasks SET status = ?2 WHERE task_id = ?1",
                params![task, status],
            )
            .unwrap();
    }

    /// The step rows of the newest worker of `task`: (name, status).
    fn steps(&self, task: &str) -> Vec<(String, String)> {
        let worker = tasks::get(self.conn(), task).unwrap().current_worker_id;
        let worker = worker.unwrap_or_else(|| {
            self.conn()
                .query_row(
                    "SELECT worker_id FROM attempts WHERE task_id = ?1 ORDER BY rowid DESC LIMIT 1",
                    [task],
                    |r| r.get(0),
                )
                .unwrap()
        });
        finish::steps_of(self.conn(), task, &worker)
            .unwrap()
            .into_iter()
            .map(|s| (s.name, s.status))
            .collect()
    }

    /// Passes, the check and the job steps, until the review of `task`
    /// stops on something else than a job: gives that.
    fn review_until_settled(&mut self, task: &str) -> Reviewed {
        for _ in 0..10 {
            let out = self.pass();
            let got = out.into_iter().find(|(t, _)| t == task).map(|(_, r)| r);
            match got {
                None => self.end_check(task, true),
                Some(Reviewed::Waiting(ComplexitySweep)) => self.run_complexity(task),
                Some(Reviewed::Waiting(Recheck)) => self.end_check(task, true),
                Some(other) => return other,
            }
        }
        panic!("the review of {task} did not settle");
    }
}

fn step_of(steps: &[(String, String)], name: StepName) -> &str {
    &steps.iter().find(|(n, _)| n == name.as_str()).unwrap().1
}

#[test]
fn light_profile_runs_check_and_complexity_sweep_then_done() {
    let mut w = World::new(VerifyProfile::Light);
    let t = w.task(ReviewMode::Light);
    // Pass 1: the check job; the review waits for its verdict.
    assert!(w.pass().is_empty());
    w.end_check(&t, true);
    // Pass 2: the check passed; the complexity-sweep is a job, and the pass
    // does not wait for it.
    assert_eq!(w.pass(), [(t.clone(), Reviewed::Waiting(ComplexitySweep))]);
    assert_eq!(w.status(&t), "verifying");
    assert_eq!(step_of(&w.steps(&t), ComplexitySweep), "running");
    // The job runs today's complexity-sweep in the job process.
    w.run_complexity(&t);
    let job = w.jobs_of(&t).pop().unwrap();
    assert_eq!(job.state, State::Passed);
    assert!(review::is_complexity_key(&job.input_key));
    // Pass 3: each step passed: done.
    assert_eq!(w.pass(), [(t.clone(), Reviewed::Done)]);
    assert_eq!(w.status(&t), "done");
    let steps = w.steps(&t);
    assert_eq!(step_of(&steps, ComplexitySweep), "passed");
    assert!(
        w.reviews(&t).is_empty(),
        "the light profile has no code-sweep"
    );
    // 2 job runs only: the check and the complexity-sweep.
    assert_eq!(w.jobs_of(&t).len(), 2);
}

#[test]
fn passed_task_gives_its_attempt_the_outcome_verified() {
    let mut w = World::new(VerifyProfile::Light);
    let t = w.task(ReviewMode::Light);
    // The agent exits after its result: rung 5 ends the attempt of the
    // verifying task with `no_result` before the verifier decides.
    w.conn()
        .execute(
            "UPDATE attempts SET state = 'ended', outcome = 'no_result', ended_at = ?2
             WHERE task_id = ?1",
            params![t, util::now()],
        )
        .unwrap();
    assert!(w.pass().is_empty());
    w.end_check(&t, true);
    w.pass();
    w.run_complexity(&t);
    assert_eq!(w.pass(), [(t.clone(), Reviewed::Done)]);
    assert_eq!(w.status(&t), "done");
    let (state, outcome): (String, String) = w
        .conn()
        .query_row(
            "SELECT state, outcome FROM attempts WHERE task_id = ?1
             ORDER BY rowid DESC LIMIT 1",
            [&t],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!((state.as_str(), outcome.as_str()), ("ended", "verified"));
}

#[test]
fn full_profile_runs_code_sweep_rounds() {
    let mut w = World::new(VerifyProfile::Full);
    let t = w.task(ReviewMode::Light);
    assert!(w.pass().is_empty());
    w.end_check(&t, true);
    // The code-sweep queues its round of reviewer tasks and waits.
    assert_eq!(w.pass(), [(t.clone(), Reviewed::Waiting(CodeSweep))]);
    assert_eq!(w.reviews(&t).len(), 2, "light: 1 round of 2 reviewers");
    // While a review is open, the step does not run again.
    assert_eq!(w.pass(), [(t.clone(), Reviewed::Waiting(CodeSweep))]);
    assert_eq!(w.reviews(&t).len(), 2);
    let rounds = w.number(
        "SELECT rounds FROM task_steps WHERE task_id = ?1 AND name = 'code-sweep'",
        &t,
    );
    assert_eq!(rounds, 1);
    w.reviews_in(&t, |_| NONE);
    // Then the complexity-sweep job, then the recheck job, in the run order.
    assert_eq!(w.pass(), [(t.clone(), Reviewed::Waiting(ComplexitySweep))]);
    assert_eq!(step_of(&w.steps(&t), CodeSweep), "passed");
    w.run_complexity(&t);
    assert_eq!(w.pass(), [(t.clone(), Reviewed::Waiting(Recheck))]);
    w.end_check(&t, true);
    assert_eq!(w.pass(), [(t.clone(), Reviewed::Done)]);
    assert_eq!(w.status(&t), "done");
    let steps = w.steps(&t);
    for name in [CodeSweep, ComplexitySweep, Recheck] {
        assert_eq!(step_of(&steps, name), "passed", "{steps:?}");
    }
}

#[test]
fn code_kind_needs_review_pass_with_at_most_2_requeues() {
    let mut w = World::new(VerifyProfile::Full);
    let t = w.task(ReviewMode::Light);
    assert!(w.pass().is_empty());
    w.end_check(&t, true);
    assert_eq!(w.pass(), [(t.clone(), Reviewed::Waiting(CodeSweep))]);
    // V-14: a cancelled reviewer task is queued again at most 2 times.
    let bug_hunter = |w: &World| {
        w.reviews(&t)
            .into_iter()
            .filter(|r| r.1.ends_with("bug-hunter"))
            .collect::<Vec<_>>()
    };
    w.reviews_in(&t, |title| {
        if title.ends_with("bug-hunter") {
            ""
        } else {
            NONE
        }
    });
    for _ in 0..3 {
        let open = bug_hunter(&w).pop().unwrap();
        w.set_status(&open.0, "cancelled");
        w.pass();
    }
    assert_eq!(bug_hunter(&w).len(), 3, "the first review and 2 requeues");
    // The review did not pass: a fix attempt, and not done.
    assert_eq!(w.status(&t), "ready");
    assert_eq!(w.fix_rounds(&t), 1);
    // RR-18 and V-14: at most 2 review requeues; the next failure burns 1
    // attempt by rung 6.
    w.next_attempt(&t);
    assert_eq!(w.review_until_settled(&t), Reviewed::FixAttempt(CodeSweep));
    assert_eq!(w.fix_rounds(&t), 2);
    w.next_attempt(&t);
    assert_eq!(w.review_until_settled(&t), Reviewed::Rejected(CodeSweep));
    assert_eq!(w.fix_rounds(&t), 2);
    assert_ne!(w.status(&t), "done");
    // 3 attempts ran; only the last burned 1.
    assert_eq!(
        w.number("SELECT attempts FROM tasks WHERE task_id = ?1", &t),
        3
    );
    assert_eq!(w.used(&t), 1);
}

#[test]
fn findings_keep_their_ids_across_rounds() {
    let mut w = World::new(VerifyProfile::Full);
    let t = w.task(ReviewMode::Full);
    assert_eq!(w.review_until_settled(&t), Reviewed::Waiting(CodeSweep));
    let first = w.reviews(&t).len();
    w.reviews_in(&t, |title| {
        if title.ends_with("bug-hunter") {
            IMPORTANT
        } else {
            NONE
        }
    });
    assert_eq!(w.pass(), [(t.clone(), Reviewed::FixAttempt(CodeSweep))]);
    let reason = verdict_file::newest(w.conn(), &t).unwrap().unwrap().reason;
    assert!(reason.starts_with("code-sweep: WARN: "), "{reason}");
    assert!(reason.contains("(F1)"), "{reason}");
    // The fix attempt commits a change; round 2 reviews it.
    w.next_attempt(&t);
    commit(&w.worktree(&t), "lib.ts", "export const a = 2;\n");
    assert_eq!(w.review_until_settled(&t), Reviewed::Waiting(CodeSweep));
    assert!(w.reviews(&t).len() > first, "round 2 queued its reviews");
    w.reviews_in(&t, |title| {
        if title.ends_with("bug-hunter") {
            IMPORTANT
        } else {
            NONE
        }
    });
    assert_eq!(w.pass(), [(t.clone(), Reviewed::FixAttempt(CodeSweep))]);
    // The same finding has the same id in both rounds.
    let reports = reports(&w.tools.evidence_root.join(&t));
    assert_eq!(reports.len(), 2, "{reports:?}");
    for report in &reports {
        let text = std::fs::read_to_string(report).unwrap();
        assert!(text.contains("F1"), "{text}");
        assert!(text.contains("off by one"), "{text}");
        assert!(!text.contains("F2"), "{text}");
    }
}

/// The code-sweep reports of the attempts under `dir`, by path.
fn reports(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            out.extend(reports(&p));
        } else if p.to_string_lossy().ends_with("-code-sweep-report.md") {
            out.push(p);
        }
    }
    out.sort();
    out
}

#[test]
fn review_requeue_keeps_the_attempt_count() {
    let mut w = World::new(VerifyProfile::Full);
    let t = w.task(ReviewMode::Light);
    assert_eq!(w.review_until_settled(&t), Reviewed::Waiting(CodeSweep));
    assert_eq!(w.used(&t), 1);
    w.reviews_in(&t, |title| {
        if title.ends_with("bug-hunter") {
            IMPORTANT
        } else {
            NONE
        }
    });
    assert_eq!(w.pass(), [(t.clone(), Reviewed::FixAttempt(CodeSweep))]);
    // The attempt ended `rejected`, its worker ended, and the task is ready
    // for a fix attempt with 1 fix round.
    let outcome: String = w
        .conn()
        .query_row(
            "SELECT outcome FROM attempts WHERE task_id = ?1 ORDER BY rowid DESC LIMIT 1",
            [&t],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(outcome, "rejected");
    assert_eq!(w.status(&t), "ready");
    assert_eq!(w.fix_rounds(&t), 1);
    // The fix attempt runs with the same attempt count.
    w.next_attempt(&t);
    assert_eq!(w.used(&t), 1);
    // Its START.md names the review that sent it back.
    let section = review::start_section(w.conn(), &t).unwrap().unwrap();
    assert!(
        section.starts_with("## The review of the last attempt"),
        "{section}"
    );
    assert!(section.contains("code-sweep: WARN: "), "{section}");
}

#[test]
fn complexity_findings_reject_and_reach_the_next_start_md() {
    let mut w = World::new(VerifyProfile::Light);
    w.analyser(
        r#"echo '{"files":[{"filePath":"lib.ts","functions":[{"line":1}]}],"violations":[{"functionName":"f","filePath":"lib.ts","line":1,"metric":"cyclomaticComplexity","value":12,"threshold":10}]}'"#,
    );
    let t = w.task(ReviewMode::Light);
    assert_eq!(
        w.review_until_settled(&t),
        Reviewed::Rejected(ComplexitySweep)
    );
    // Q11: the outcome is `rejected`, and the list is in the verdict and
    // in START.md of the next attempt.
    assert_eq!(w.fix_rounds(&t), 0);
    let verdict = verdict_file::newest(w.conn(), &t).unwrap().unwrap();
    assert!(
        verdict
            .reason
            .contains("lib.ts:1 f cyclomaticComplexity=12 (max 10)"),
        "{}",
        verdict.reason
    );
    let section = review::start_section(w.conn(), &t).unwrap().unwrap();
    assert!(
        section.contains("lib.ts:1 f cyclomaticComplexity=12"),
        "{section}"
    );
}

#[test]
fn answered_important_finding_passes() {
    let mut w = World::new(VerifyProfile::Full);
    let t = w.task(ReviewMode::Light);
    assert_eq!(w.review_until_settled(&t), Reviewed::Waiting(CodeSweep));
    w.reviews_in(&t, |title| {
        if title.ends_with("bug-hunter") {
            IMPORTANT
        } else {
            NONE
        }
    });
    assert_eq!(w.pass(), [(t.clone(), Reviewed::FixAttempt(CodeSweep))]);
    // The fix attempt answers the finding in the answers file of the round
    // and changes no code.
    let reason = verdict_file::newest(w.conn(), &t).unwrap().unwrap().reason;
    let answers = reason
        .split_whitespace()
        .find(|p| p.ends_with("answers.md"))
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("the verdict names the answers file: {reason}"));
    w.next_attempt(&t);
    std::fs::write(&answers, "F1: the caller clamps it\n").unwrap();
    assert_eq!(w.review_until_settled(&t), Reviewed::Done);
    assert_eq!(w.status(&t), "done");
    assert_eq!(w.fix_rounds(&t), 1);
}

#[test]
fn land_pr_and_cleanup_are_not_verifier_steps() {
    let mut w = World::new(VerifyProfile::Full);
    let t = w.task(ReviewMode::NoReview);
    let run_branch = git(&w.worktree(&t), &["rev-parse", RUN_BRANCH]);
    assert_eq!(w.review_until_settled(&t), Reviewed::Done);
    assert_eq!(w.status(&t), "done");
    let steps = w.steps(&t);
    // Review: none makes the code-sweep not applicable, as today.
    assert_eq!(step_of(&steps, CodeSweep), "not_applicable");
    for name in [StepName::Land, StepName::Pr, StepName::Cleanup] {
        assert_eq!(step_of(&steps, name), "pending", "{steps:?}");
    }
    // Nothing landed: the run branch did not move.
    assert_eq!(git(&w.worktree(&t), &["rev-parse", RUN_BRANCH]), run_branch);
}

#[test]
fn claim_attempt_finish_is_unchanged() {
    let mut w = World::new(VerifyProfile::Light);
    let mut t = NewTask::work("feature", "t.md", "opus");
    t.check_none = true;
    t.work_kind = Some(WorkKind::Feature);
    let run = w.run.clone();
    let worker = w
        .store
        .immediate(|tx| {
            let task = tasks::add(tx, &run, &t, Actor::Orchestrator)?.task_id;
            let new = NewWorker {
                run_id: run.run_id.clone(),
                persona: "opus".into(),
                role: Some("opus-1".into()),
                ..Default::default()
            };
            let agent = horch_core::fleetd::task_execution::domain::task::Recorded {
                pid: 4243,
                pid_start: Some(2),
            };
            let id = horch_core::taskledger::testing::bind_live(tx, &task, &new, Some(agent))?.worker_id;
            tx.execute(
                "UPDATE workers SET supervisor_pid = 4242, supervisor_started = 1 WHERE worker_id = ?1",
                [&id],
            )?;
            Ok(id)
        })
        .unwrap();
    let wt =
        finish::worktree_new_with(&mut w.store, &w.tools.git, &worker, "feat", &TestJobs).unwrap();
    commit(Path::new(&wt.path), "notes.txt", "x\n");
    let task = wt.task_id.unwrap();
    // The worker's finish runs each step of the run in its own process,
    // land included, as today.
    let r = finish::finish(&mut w.store, &w.tools, &worker).unwrap();
    assert!(r.complete, "{r:?}");
    let names: Vec<&str> = r.steps.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(
        names,
        ["check", "complexity-sweep", "land", "pr", "cleanup"]
    );
    assert_eq!(r.steps[0].status, "not_applicable");
    assert_eq!(r.steps[1].status, "not_applicable");
    assert_eq!(r.steps[2].status, "passed");
    assert_eq!(w.status(&task), "verifying");
    // The verifier of fleetd does not touch a claim attempt.
    assert!(w.pass().is_empty());
    assert!(w.jobs_of(&task).is_empty());
}

#[test]
fn a_crash_before_or_after_the_step_row_runs_each_effect_once() {
    let mut w = World::new(VerifyProfile::Full);
    let t = w.task(ReviewMode::Light);
    let tip = git(&w.worktree(&t), &["rev-parse", "HEAD"]);
    assert!(w.pass().is_empty());
    w.end_check(&t, true);
    // A crash before the commit: the row of the running code-sweep is
    // rolled back. The next pass runs the step 1 time.
    {
        let mut conn = Connection::open(&w.ledger).unwrap();
        let tx = conn.transaction().unwrap();
        tx.execute(
            "UPDATE task_steps SET status = 'running' WHERE task_id = ?1",
            [&t],
        )
        .unwrap();
        drop(tx);
    }
    assert_eq!(w.pass(), [(t.clone(), Reviewed::Waiting(CodeSweep))]);
    assert_eq!(w.reviews(&t).len(), 2);
    // A crash after the commit and before the effect: the complexity-sweep
    // row is running and its job is queued, but no job process ran. The
    // next passes queue no second run, and the result is recorded 1 time.
    w.reviews_in(&t, |_| NONE);
    assert_eq!(w.pass(), [(t.clone(), Reviewed::Waiting(ComplexitySweep))]);
    assert_eq!(w.pass(), [(t.clone(), Reviewed::Waiting(ComplexitySweep))]);
    let complexity = |w: &World| {
        w.jobs_of(&t)
            .into_iter()
            .filter(|r| review::is_complexity_key(&r.input_key))
            .count()
    };
    assert_eq!(complexity(&w), 1);
    w.run_complexity(&t);
    assert_eq!(w.pass(), [(t.clone(), Reviewed::Waiting(Recheck))]);
    assert_eq!(w.pass(), [(t.clone(), Reviewed::Waiting(Recheck))]);
    assert_eq!(complexity(&w), 1);
    let passed_events = w.number(
        "SELECT count(*) FROM events WHERE task_id = ?1 AND kind = 'step_passed'
           AND text LIKE '%complexity-sweep%'",
        &t,
    );
    assert_eq!(passed_events, 1);
    w.end_check(&t, true);
    assert_eq!(w.pass(), [(t.clone(), Reviewed::Done)]);
    // A pass after done changes nothing, and the code-sweep ran 1 round.
    assert!(w.pass().is_empty());
    assert_eq!(w.reviews(&t).len(), 2);
    // No tip and no work is lost.
    assert_eq!(git(&w.worktree(&t), &["rev-parse", "HEAD"]), tip);
    assert!(w.worktree(&t).join("lib.ts").exists());
}
