//! TRF-18: a shared-checkout task diffs from its claim commit, so
//! code-sweep and complexity-sweep see its work. Its commits are on the run
//! branch already, so `run_branch...HEAD` is empty.

use std::path::{Path, PathBuf};
use std::process::Command;

use horch_core::taskledger::finish::{self, CodexSandbox, FinishReport, Step, Tools};
use horch_core::taskledger::git::Git;
use horch_core::taskledger::model::{Actor, StepName, WorktreeMode};
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
    top: PathBuf,
    root: PathBuf,
    store: Store,
    tools: Tools,
    worker: String,
    start: String,
}

impl World {
    fn new(steps: &[StepName], file: &str) -> World {
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
        let start = git(&root, &["rev-parse", "HEAD"]);
        commit(&root, file);
        World {
            _tmp: tmp,
            top,
            root,
            store,
            tools,
            worker,
            start,
        }
    }

    fn finish(&mut self) -> FinishReport {
        finish::finish(&mut self.store, &self.tools, &self.worker).unwrap()
    }
}

fn step<'a>(r: &'a FinishReport, name: &str) -> &'a Step {
    r.steps.iter().find(|s| s.name == name).unwrap()
}

#[test]
fn trf_18_complexity_sweep_reads_the_work_of_a_shared_task() {
    let mut w = World::new(&[StepName::ComplexitySweep], "lib.ts");
    let args = w.top.join("args");
    let cli = w.top.join("cli.js");
    std::fs::write(
        &cli,
        format!(
            "printf '%s\\n' \"$@\" > {}\necho '{{\"files\":[],\"violations\":[]}}'\n",
            args.display()
        ),
    )
    .unwrap();
    w.tools.complexity_cli = Some(cli);
    let r = w.finish();
    assert!(r.complete, "{r:?}");
    let s = step(&r, "complexity-sweep");
    assert_eq!(s.status, "passed", "{s:?}");
    assert_eq!(s.note.as_deref(), Some("1 files analysed"));
    assert!(
        std::fs::read_to_string(&args)
            .unwrap()
            .ends_with("lib.ts\n"),
        "the analyser reads the file the task committed"
    );
}

#[test]
fn trf_18_code_sweep_reviews_the_work_of_a_shared_task() {
    let mut w = World::new(&[StepName::CodeSweep], "feature.txt");
    let r = w.finish();
    let s = step(&r, "code-sweep");
    assert_ne!(s.status, "not_applicable", "{s:?}");
    assert_eq!(
        s.verdict.as_deref(),
        Some("no skill"),
        "the step sees a change and looks for its skill"
    );
}

#[test]
fn trf_18_the_diff_base_is_the_claim_commit() {
    let w = World::new(&[StepName::CodeSweep], "feature.txt");
    let worker = workers::get(w.store.conn(), &w.worker).unwrap();
    let task = tasks::get(w.store.conn(), worker.task_id.as_deref().unwrap()).unwrap();
    let run = runs::get(w.store.conn(), &task.run_id).unwrap();
    let cx = |tools: &Tools| -> String {
        finish::Ctx {
            tools,
            run: &run,
            task: &task,
            worker: &worker,
            worktree: None,
            dir: w.root.clone(),
            root: w.root.clone(),
            evidence_dir: w.top.join("ev"),
            seq: 1,
            default_check: None,
        }
        .diff_base()
        .unwrap()
    };
    assert_eq!(cx(&w.tools), w.start);
    // No claim commit recorded: the run branch, as before.
    let mut other = w.tools.clone();
    other.evidence_root = w.top.join("elsewhere");
    assert_eq!(cx(&other), RUN_BRANCH);
}
