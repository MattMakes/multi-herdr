//! The structural check of a judgment kind
//! (docs/tickets/fleetd-p4/21-structural-check.md): after the check of a
//! push attempt passed, the verifier checks the outputs of a task of a
//! judgment kind at the tip of its branch. Its outputs are the files of its
//! `Own` that its commits changed (Q12 of the MAP). A task whose check
//! passes waits in `verifying` for its judge task (tests/judge_pair.rs;
//! docs/specs/fleetd/design.md, FR-4, "Done for each kind of task", FM-9).
//!
//! Each test uses a temp git repository, a temp ledger and a temp fleet
//! root. A pass is part 2 (the verifier, in 1 transaction) and part 3 (the
//! structural check and the review steps, after the commit). A
//! `task_check` job ends through its row, as the job process ends it. No
//! test reads the load, the limits or the clock of this machine: the fleet
//! clock is a fact of the test.
#![cfg(unix)]

#[path = "common/test_jobs.rs"]
mod test_jobs;

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

use horch_core::fleetd::fleet_runtime::application::jobs::JobCtx;
use horch_core::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;
use horch_core::fleetd::fleet_runtime::domain::jobs::State;
use horch_core::fleetd::ideas::application::needs_you::NeedsYouList;
use horch_core::fleetd::task_execution::adapters::verdict_file;
use horch_core::fleetd::task_execution::application::retry::Policy;
use horch_core::fleetd::task_execution::application::review::{Review, Reviewed};
use horch_core::fleetd::task_execution::application::verifier::{self, Structural, Verifier};
use horch_core::fleetd::task_execution::domain::verification::{Decision, Verdict};
use horch_core::taskledger::finish::{self, CodexSandbox, Tools};
use horch_core::taskledger::git::Git;
use horch_core::taskledger::model::{Actor, VerifyProfile, WorkKind};
use horch_core::taskledger::runs::{self, NewRun, Run};
use horch_core::taskledger::tasks::{self, NewTask, Task};
use horch_core::taskledger::workers::{self, NewWorker};
use horch_core::taskledger::{util, Store};
use rusqlite::{params, Connection};
use test_jobs::TestJobs;

const RUN_BRANCH: &str = "fleet/r1";

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

/// Write `file` in `dir` and commit it.
fn commit(dir: &Path, file: &str, body: &str) {
    let path = dir.join(file);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, body).unwrap();
    git(dir, &["add", file]);
    git(dir, &["commit", "--quiet", "-m", file]);
}

/// Remove `file` in `dir` and commit it.
fn remove(dir: &Path, file: &str) {
    git(dir, &["rm", "--quiet", file]);
    git(dir, &["commit", "--quiet", "-m", file]);
}

fn policy(_: &Task) -> Policy {
    Policy::default()
}

/// What 1 pass did with 1 task.
#[derive(Debug, Default)]
struct Pass {
    structural: Vec<(String, Structural)>,
    reviewed: Vec<(String, Reviewed)>,
}

struct World {
    _tmp: tempfile::TempDir,
    store: Store,
    run: Run,
    tools: Tools,
    config: FleetConfig,
}

impl World {
    /// A git repository with the run branch, and 1 run with the steps of
    /// the profile `minimal`: the check, then land. No review step comes
    /// before land.
    fn new() -> World {
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
        let mut store = Store::open(&top.join("ledger.sqlite3")).unwrap();
        let run = store
            .immediate(|tx| {
                let run = runs::create(
                    tx,
                    &root,
                    &NewRun {
                        slug: "r1".into(),
                        base_branch: Some("main".into()),
                        run_branch: Some(RUN_BRANCH.into()),
                        finish_steps: VerifyProfile::Minimal.steps(),
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
        let tools = Tools {
            git: Git::new("git"),
            gh: top.join("gh"),
            node: "/bin/sh".into(),
            complexity_cli: None,
            code_sweep_dir: None,
            evidence_root: top.join("evidence"),
            shell: "/bin/sh".into(),
            reviewers_down: Vec::new(),
            codex: CodexSandbox::default(),
            jobs: Some(std::sync::Arc::new(TestJobs)),
        };
        World {
            _tmp: tmp,
            store,
            run,
            tools,
            config: FleetConfig::default(),
        }
    }

    fn conn(&self) -> &Connection {
        self.store.conn()
    }

    /// A task of the ticket kind `kind` with the check `true` and the
    /// `Own` list `own`, claimed by a worker, with its worktree. `work`
    /// makes its commits in the worktree. Its attempt is a push attempt in
    /// `verifying`.
    fn task(&mut self, kind: WorkKind, own: &[&str], work: impl FnOnce(&Path)) -> String {
        let mut t = NewTask::work("judged", "t.md", "opus");
        t.land_check = Some("true".into());
        t.work_kind = Some(kind);
        t.own = own.iter().map(|s| s.to_string()).collect();
        let run = self.run.clone();
        let worker = self
            .store
            .immediate(|tx| {
                tasks::add(tx, &run, &t, Actor::Orchestrator)?;
                let id = workers::insert(
                    tx,
                    &NewWorker {
                        run_id: run.run_id.clone(),
                        persona: "opus".into(),
                        role: Some("opus-1".into()),
                        ..Default::default()
                    },
                )?;
                workers::supervisor_started(tx, &id, 4242, Some(1))?;
                workers::agent_started(tx, &id, 4243, Some(2))?;
                tasks::claim(tx, &id)?;
                Ok(id)
            })
            .unwrap();
        let wt = finish::worktree_new_with(
            &mut self.store,
            &self.tools.git,
            &worker,
            "judged",
            &TestJobs,
        )
        .unwrap();
        work(Path::new(&wt.path));
        let task = wt.task_id.unwrap();
        let n: i64 = self
            .conn()
            .query_row(
                "SELECT attempts FROM tasks WHERE task_id = ?1",
                [&task],
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
        task
    }

    /// 1 pass: part 2, the verifier in 1 transaction; part 3, the
    /// structural check of each judgment kind and the review steps of each
    /// code kind, after the commit. The tips are the HEADs of the
    /// worktrees.
    fn pass(&mut self) -> Pass {
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
        Pass {
            structural: verifier::structural(&r, &mut self.store, &report.passed).unwrap(),
            reviewed: verifier::review(&r, &mut self.store, report.passed).unwrap(),
        }
    }

    /// The job process of the queued check of `task` ends `passed`.
    fn end_check(&self, task: &str) {
        let id: i64 = self
            .conn()
            .query_row(
                "SELECT id FROM jobs WHERE kind = 'task_check' AND subject = ?1 AND state = ?2",
                params![task, State::Queued.as_str()],
                |r| r.get(0),
            )
            .unwrap();
        let result = r#"{"passed":true,"verdict":true,"text":"`true` in wt: exit 0"}"#;
        self.conn()
            .execute(
                "UPDATE jobs SET state = 'passed', result_json = ?2, ended_at = ?3 WHERE id = ?1",
                params![id, result, util::now()],
            )
            .unwrap();
    }

    /// Pass 1 queues the check of `task`, its job passes, and pass 2 gives
    /// the result of part 3.
    fn verify(&mut self, task: &str) -> Pass {
        let first = self.pass();
        assert!(first.structural.is_empty() && first.reviewed.is_empty());
        self.end_check(task);
        self.pass()
    }

    fn status(&self, task: &str) -> String {
        tasks::get(self.conn(), task).unwrap().status
    }

    fn newest(&self, task: &str) -> Verdict {
        verdict_file::newest(self.conn(), task).unwrap().unwrap()
    }

    fn outcome(&self, task: &str) -> Option<String> {
        self.conn()
            .query_row(
                "SELECT outcome FROM attempts WHERE task_id = ?1 ORDER BY rowid DESC LIMIT 1",
                [task],
                |r| r.get(0),
            )
            .unwrap()
    }

    /// The structural check of `task` failed: its attempt ended
    /// `rejected`, the task is not `done`, and the reason of its newest
    /// verdict holds `why`.
    fn assert_rejected(&self, pass: &Pass, task: &str, why: &str) {
        let (id, got) = &pass.structural[0];
        assert_eq!(id, task);
        let Structural::Rejected(reason) = got else {
            panic!("not rejected: {got:?}");
        };
        assert!(reason.contains(why), "{reason}");
        let newest = self.newest(task);
        assert_eq!(newest.decision, Decision::Failed);
        assert!(newest.reason.contains(why), "{}", newest.reason);
        assert_eq!(self.outcome(task).as_deref(), Some("rejected"));
        assert_ne!(self.status(task), "done");
        assert_ne!(self.status(task), "verifying");
    }
}

#[test]
fn docs_task_with_its_file_and_sections_passes() {
    let mut w = World::new();
    let task = w.task(WorkKind::Docs, &["docs/sync.md", "data/"], |wt| {
        commit(
            wt,
            "docs/sync.md",
            "# Sync\n\n## Retries\nThe fetch runs again.\n",
        );
        commit(wt, "data/plan.json", "{\"steps\": [1, 2]}\n");
        commit(wt, "notes.md", "not of Own\n");
    });

    let pass = w.verify(&task);

    // The check passes, and the task waits in verifying for its judge
    // task (The judge pair, step 1; tests/judge_pair.rs).
    assert_eq!(
        pass.structural,
        vec![(task.clone(), Structural::WaitsForJudge)]
    );
    assert!(pass.reviewed.is_empty());
    assert_eq!(w.status(&task), "verifying");
    let newest = w.newest(&task);
    assert_eq!(newest.decision, Decision::Passed);
    assert_eq!(
        newest.reason,
        "structural: passed: data/plan.json, docs/sync.md"
    );
    // The next pass finds the check passed and records nothing more.
    let again = w.pass();
    assert_eq!(
        again.structural,
        vec![(task.clone(), Structural::WaitsForJudge)]
    );
    assert_eq!(verdict_file::recorded(w.conn(), &task).unwrap().len(), 2);
}

#[test]
fn missing_output_file_fails() {
    let mut w = World::new();
    let task = w.task(WorkKind::Report, &["docs/report.md"], |wt| {
        commit(wt, "docs/report.md", "# R\n\n## Findings\n- 1\n");
        remove(wt, "docs/report.md");
        commit(wt, "other.md", "x\n");
    });

    let pass = w.verify(&task);

    w.assert_rejected(&pass, &task, "docs/report.md: not at the tip");
}

#[test]
fn markdown_with_no_section_fails() {
    let mut w = World::new();
    let task = w.task(WorkKind::Docs, &["docs/a.md", "docs/b.md"], |wt| {
        commit(wt, "docs/a.md", "# Title\n\nOnly text, no section.\n");
        commit(wt, "docs/b.md", "# B\n\n## Empty\n\n## Full\ntext\n");
    });

    let pass = w.verify(&task);

    w.assert_rejected(&pass, &task, "docs/a.md: no `## ` heading");
    let reason = &w.newest(&task).reason;
    assert!(
        reason.contains("docs/b.md: the section `## Empty` is empty"),
        "{reason}"
    );
}

#[test]
fn invalid_json_output_fails() {
    let mut w = World::new();
    let task = w.task(WorkKind::Report, &["out/result.json"], |wt| {
        commit(wt, "out/result.json", "{\"open\": [1, 2\n");
    });

    let pass = w.verify(&task);

    w.assert_rejected(&pass, &task, "out/result.json: invalid JSON");
}

#[test]
fn judgment_task_that_changes_no_owned_file_fails() {
    let mut w = World::new();
    let task = w.task(WorkKind::Docs, &["docs/sync.md"], |wt| {
        commit(wt, "elsewhere.md", "# E\n\n## S\ntext\n");
    });

    let pass = w.verify(&task);

    w.assert_rejected(&pass, &task, "the commits change no file of Own");
}

#[test]
fn code_kind_gets_no_structural_check() {
    let mut w = World::new();
    // A code kind with no `## ` heading in its markdown file: the
    // structural check would fail it.
    let task = w.task(WorkKind::Feature, &["notes.md"], |wt| {
        commit(wt, "notes.md", "no heading\n");
    });

    let pass = w.verify(&task);

    assert!(pass.structural.is_empty(), "{pass:?}");
    assert_eq!(pass.reviewed, vec![(task.clone(), Reviewed::Done)]);
    assert_eq!(w.status(&task), "done");
    let recorded = verdict_file::recorded(w.conn(), &task).unwrap();
    assert_eq!(recorded.len(), 1);
    assert!(!recorded[0].reason.contains("structural"), "{recorded:?}");
}
