//! CMS-01..CMS-06 (docs/specs/comment-sweep.md): the comment-sweep finish
//! step queues 1 task for the `comment-remover` teammate, waits for it, and
//! passes when it ended.

use std::path::{Path, PathBuf};
use std::process::Command;

use horch_core::taskledger::finish::{self, CodexSandbox, FinishReport, Step, Tools};
use horch_core::taskledger::git::Git;
use horch_core::taskledger::model::{Actor, StepName, VerifyProfile, WorktreeMode};
use horch_core::taskledger::runs::{self, NewRun, Run};
use horch_core::taskledger::tasks::{self, NewTask};
use horch_core::taskledger::workers::{self, NewWorker};
use horch_core::taskledger::Store;

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

fn commit(dir: &Path, file: &str) -> String {
    std::fs::write(dir.join(file), "work\n").unwrap();
    git(dir, &["add", file]);
    git(dir, &["commit", "--quiet", "-m", file]);
    git(dir, &["rev-parse", "HEAD"])
}

/// A repository on its run branch, with 1 claimed shared-checkout task
/// that ran the shared claim, then 1 commit of `file` on the run branch.
struct World {
    _tmp: tempfile::TempDir,
    root: PathBuf,
    store: Store,
    tools: Tools,
    worker: String,
}

impl World {
    fn new(steps: &[StepName], file: &str) -> World {
        World::with_check(steps, file, None)
    }

    fn with_check(steps: &[StepName], file: &str, check: Option<&str>) -> World {
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
        commit(&root, "README.md");
        git(&root, &["switch", "--quiet", "-c", RUN_BRANCH]);
        let mut store = Store::open(&top.join("ledger.sqlite3")).unwrap();
        let run: Run = store
            .immediate(|tx| {
                runs::create(
                    tx,
                    &root,
                    &NewRun {
                        slug: "r1".into(),
                        base_branch: Some("main".into()),
                        run_branch: Some(RUN_BRANCH.into()),
                        finish_steps: steps.to_vec(),
                        ..Default::default()
                    },
                )
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
        };
        let mut t = NewTask::work("feature", "t.md", "opus");
        t.worktree_mode = WorktreeMode::Shared;
        t.land_check = check.map(String::from);
        let worker = store
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
        let task_id = workers::get(store.conn(), &worker)
            .unwrap()
            .task_id
            .unwrap();
        let task = tasks::get(store.conn(), &task_id).unwrap();
        finish::shared_claim(&tools, &run, &task).unwrap();
        commit(&root, file);
        World {
            _tmp: tmp,
            root,
            store,
            tools,
            worker,
        }
    }

    fn finish(&mut self) -> FinishReport {
        finish::finish(&mut self.store, &self.tools, &self.worker).unwrap()
    }
}

fn step<'a>(r: &'a FinishReport, name: &str) -> &'a Step {
    r.steps.iter().find(|s| s.name == name).unwrap()
}

impl World {
    /// The comment-sweep task of the worker's task: (task id, title, persona, ticket path, report path).
    fn sweeps(&self) -> Vec<(String, String, String, String, String)> {
        let mut stmt = self
            .store
            .conn()
            .prepare(
                "SELECT task_id, title, primary_persona, task_file, spec_ref FROM tasks
                 WHERE kind = 'review' ORDER BY seq",
            )
            .unwrap();
        let rows = stmt
            .query_map([], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
            })
            .unwrap();
        rows.map(|r| r.unwrap()).collect()
    }

    fn end_sweep(&self, task_id: &str, status: &str) {
        self.store
            .conn()
            .execute(
                "UPDATE tasks SET status = ?2 WHERE task_id = ?1",
                [task_id, status],
            )
            .unwrap();
    }
}

#[test]
fn cms_01_the_step_queues_1_sweep_for_the_comment_remover_and_the_author_waits() {
    let mut w = World::new(&[StepName::CommentSweep], "lib.rs");
    let r = w.finish();
    assert!(!r.complete, "{r:?}");
    let s = step(&r, "comment-sweep");
    assert_eq!(s.status, "running", "{s:?}");
    let sweeps = w.sweeps();
    assert_eq!(sweeps.len(), 1, "{sweeps:?}");
    let (id, title, persona, ticket, report) = &sweeps[0];
    assert_eq!(persona, "comment-remover");
    assert_eq!(title, "Comment sweep T-1");
    assert_eq!(r.queued_reviews, vec![id.clone()]);
    let text = std::fs::read_to_string(ticket).unwrap();
    assert!(
        text.contains(&format!("checked out: {}", w.root.display())),
        "{text}"
    );
    assert!(text.contains("The changed code files: lib.rs"), "{text}");
    assert!(text.contains(report.as_str()), "{text}");
    let waits = workers::waiting(w.store.conn(), &w.worker).unwrap();
    assert_eq!(
        waits.len(),
        1,
        "the author waits, so the watchdog leaves it"
    );
    // A second finish queues nothing more.
    let r = w.finish();
    assert_eq!(step(&r, "comment-sweep").status, "running");
    assert_eq!(w.sweeps().len(), 1);
}

#[test]
fn cms_02_a_done_sweep_passes_and_its_commit_runs_the_check_again() {
    let mut w = World::with_check(
        &[StepName::Check, StepName::CommentSweep],
        "lib.rs",
        Some("true"),
    );
    let r = w.finish();
    assert_eq!(step(&r, "check").status, "passed", "{r:?}");
    assert_eq!(step(&r, "check").rounds, 1);
    let (id, _, _, _, report) = w.sweeps().remove(0);
    // The sweep commits in the author's work folder.
    commit(&w.root, "lib2.rs");
    std::fs::write(&report, "removed 3, kept 0\n").unwrap();
    w.end_sweep(&id, "done");
    let r = w.finish();
    assert!(r.complete, "{r:?}");
    assert_eq!(step(&r, "check").rounds, 2, "the tree moved");
    let s = step(&r, "comment-sweep");
    assert_eq!(
        (s.status.as_str(), s.verdict.as_deref()),
        ("passed", Some("swept"))
    );
    assert_eq!(s.evidence_path.as_deref(), Some(report.as_str()));
    assert!(workers::waiting(w.store.conn(), &w.worker)
        .unwrap()
        .is_empty());
}

#[test]
fn cms_03_a_change_with_no_code_file_is_not_applicable() {
    let mut w = World::new(&[StepName::CommentSweep], "notes.md");
    let r = w.finish();
    assert!(r.complete, "{r:?}");
    assert_eq!(step(&r, "comment-sweep").status, "not_applicable");
    assert!(w.sweeps().is_empty());
}

#[test]
fn cms_04_a_cancelled_sweep_passes_as_skipped() {
    let mut w = World::new(&[StepName::CommentSweep], "lib.rs");
    w.finish();
    let (id, ..) = w.sweeps().remove(0);
    w.end_sweep(&id, "cancelled");
    let r = w.finish();
    assert!(r.complete, "{r:?}");
    assert_eq!(
        step(&r, "comment-sweep").verdict.as_deref(),
        Some("skipped")
    );
}

#[test]
fn cms_05_a_later_commit_gets_1_more_sweep_from_the_swept_tree_then_no_more() {
    let mut w = World::new(&[StepName::CommentSweep], "lib.rs");
    w.finish();
    let (first, ..) = w.sweeps().remove(0);
    w.end_sweep(&first, "done");
    assert!(w.finish().complete);
    // The author commits a fix after the sweep.
    commit(&w.root, "fix.rs");
    let r = w.finish();
    assert_eq!(step(&r, "comment-sweep").status, "running", "{r:?}");
    let sweeps = w.sweeps();
    assert_eq!(sweeps.len(), 2);
    let text = std::fs::read_to_string(&sweeps[1].3).unwrap();
    assert!(
        text.contains("The changed code files: fix.rs\n"),
        "sweep 2 reads only the change since sweep 1: {text}"
    );
    w.end_sweep(&sweeps[1].0, "done");
    assert!(w.finish().complete);
    commit(&w.root, "fix2.rs");
    let r = w.finish();
    assert!(r.complete, "{r:?}");
    assert!(step(&r, "comment-sweep")
        .note
        .as_deref()
        .unwrap()
        .contains("sweeps of this task are spent"));
    assert_eq!(w.sweeps().len(), 2);
}

#[test]
fn cms_06_light_and_minimal_sweep_comments_right_after_the_check() {
    for p in [
        VerifyProfile::Minimal,
        VerifyProfile::Light,
        VerifyProfile::Full,
    ] {
        let steps = p.steps();
        assert_eq!(
            &steps[..2],
            &[StepName::Check, StepName::CommentSweep],
            "{p}"
        );
    }
}
