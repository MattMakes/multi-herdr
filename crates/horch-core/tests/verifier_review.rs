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

const NONE: &str = "## Critical\n- none\n## Important\n- none\n## Minor\n- none\n";
const IMPORTANT: &str =
    "## Critical\n- none\n## Important\n- lib.ts:1 off by one. Fix: <=\n## Minor\n- none\n";

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

fn commit(dir: &Path, file: &str, body: &str) -> String {
    std::fs::write(dir.join(file), body).unwrap();
    git(dir, &["add", file]);
    git(dir, &["commit", "--quiet", "-m", file]);
    git(dir, &["rev-parse", "HEAD"])
}

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
    fn new(profile: VerifyProfile) -> World {
        let tmp = tempfile::tempdir().unwrap();
        let top = horch_platform::path::canonicalize(tmp.path()).unwrap();
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
        horch_platform::path::canonicalize(self.tmp.path()).unwrap()
    }

    fn analyser(&self, body: &str) {
        std::fs::write(self.top().join("cli.js"), format!("{body}\n")).unwrap();
    }

    fn conn(&self) -> &Connection {
        self.store.conn()
    }

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

    fn used(&self, task: &str) -> i64 {
        self.number(
            "SELECT attempts - attempt_floor FROM tasks WHERE task_id = ?1",
            task,
        )
    }

    fn fix_rounds(&self, task: &str) -> i64 {
        self.number("SELECT fix_rounds FROM tasks WHERE task_id = ?1", task)
    }

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
    assert!(w.pass().is_empty());
    w.end_check(&t, true);
    assert_eq!(w.pass(), [(t.clone(), Reviewed::Waiting(ComplexitySweep))]);
    assert_eq!(w.status(&t), "verifying");
    assert_eq!(step_of(&w.steps(&t), ComplexitySweep), "running");
    w.run_complexity(&t);
    let job = w.jobs_of(&t).pop().unwrap();
    assert_eq!(job.state, State::Passed);
    assert!(review::is_complexity_key(&job.input_key));
    assert_eq!(w.pass(), [(t.clone(), Reviewed::Done)]);
    assert_eq!(w.status(&t), "done");
    let steps = w.steps(&t);
    assert_eq!(step_of(&steps, ComplexitySweep), "passed");
    assert!(
        w.reviews(&t).is_empty(),
        "the light profile has no code-sweep"
    );
    assert_eq!(w.jobs_of(&t).len(), 2);
}

#[test]
fn passed_task_gives_its_attempt_the_outcome_verified() {
    let mut w = World::new(VerifyProfile::Light);
    let t = w.task(ReviewMode::Light);
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
    assert_eq!(w.pass(), [(t.clone(), Reviewed::Waiting(CodeSweep))]);
    assert_eq!(w.reviews(&t).len(), 2, "light: 1 round of 2 reviewers");
    assert_eq!(w.pass(), [(t.clone(), Reviewed::Waiting(CodeSweep))]);
    assert_eq!(w.reviews(&t).len(), 2);
    let rounds = w.number(
        "SELECT rounds FROM task_steps WHERE task_id = ?1 AND name = 'code-sweep'",
        &t,
    );
    assert_eq!(rounds, 1);
    w.reviews_in(&t, |_| NONE);
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
    assert_eq!(w.status(&t), "ready");
    assert_eq!(w.fix_rounds(&t), 1);
    w.next_attempt(&t);
    assert_eq!(w.review_until_settled(&t), Reviewed::FixAttempt(CodeSweep));
    assert_eq!(w.fix_rounds(&t), 2);
    w.next_attempt(&t);
    assert_eq!(w.review_until_settled(&t), Reviewed::Rejected(CodeSweep));
    assert_eq!(w.fix_rounds(&t), 2);
    assert_ne!(w.status(&t), "done");
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
    let reports = reports(&w.tools.evidence_root.join(&t));
    assert_eq!(reports.len(), 2, "{reports:?}");
    for report in &reports {
        let text = std::fs::read_to_string(report).unwrap();
        assert!(text.contains("F1"), "{text}");
        assert!(text.contains("off by one"), "{text}");
        assert!(!text.contains("F2"), "{text}");
    }
}

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
    w.next_attempt(&t);
    assert_eq!(w.used(&t), 1);
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
    assert_eq!(step_of(&steps, CodeSweep), "not_applicable");
    for name in [StepName::Land, StepName::Pr, StepName::Cleanup] {
        assert_eq!(step_of(&steps, name), "pending", "{steps:?}");
    }
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
    let r = finish::finish(&mut w.store, &w.tools, &worker).unwrap();
    assert!(r.complete, "{r:?}");
    let names: Vec<&str> = r.steps.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "check",
            "comment-sweep",
            "complexity-sweep",
            "land",
            "pr",
            "cleanup"
        ]
    );
    assert_eq!(r.steps[0].status, "not_applicable");
    assert_eq!(r.steps[1].status, "not_applicable");
    assert_eq!(r.steps[2].status, "not_applicable");
    assert_eq!(r.steps[3].status, "passed");
    assert_eq!(w.status(&task), "verifying");
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
    assert!(w.pass().is_empty());
    assert_eq!(w.reviews(&t).len(), 2);
    assert_eq!(git(&w.worktree(&t), &["rev-parse", "HEAD"]), tip);
    assert!(w.worktree(&t).join("lib.ts").exists());
}
