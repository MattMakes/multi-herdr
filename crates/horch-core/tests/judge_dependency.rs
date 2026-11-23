#![cfg(unix)]

#[path = "common/test_jobs.rs"]
mod test_jobs;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use horch_core::fleetd::fleet_runtime::application::jobs::JobCtx;
use horch_core::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;
use horch_core::fleetd::ideas::application::needs_you::NeedsYouList;
use horch_core::fleetd::task_execution::application::dispatch::{bind, Bound, DispatchCtx};
use horch_core::fleetd::task_execution::domain::task::{
    judged_task, promotable_deps, Dep, DepKind,
};
use horch_core::fleetd::worktree_custody::adapters::sqlite as trees;
use horch_core::fleetd::worktree_custody::application::create;
use horch_core::taskledger::git::Git;
use horch_core::taskledger::model::{Actor, TaskKind};
use horch_core::taskledger::runs::{self, NewRun, Run};
use horch_core::taskledger::tasks::{self, NewTask};
use horch_core::taskledger::watchdog;
use horch_core::taskledger::workers::NewWorker;
use horch_core::taskledger::Store;
use rusqlite::Connection;
use test_jobs::TestJobs;

const RUN_BRANCH: &str = "fleet/r1";

fn statuses(rows: &[(&str, &str)]) -> BTreeMap<String, String> {
    rows.iter()
        .map(|(id, s)| (id.to_string(), s.to_string()))
        .collect()
}

fn dep(task: &str, on: &str, kind: DepKind) -> Dep {
    Dep {
        task: task.into(),
        on: on.into(),
        kind,
    }
}

fn passed(ids: &[&str]) -> BTreeSet<String> {
    ids.iter().map(|s| s.to_string()).collect()
}

fn judge_ready(w_status: &str, structural_passed: &[&str]) -> Vec<String> {
    let s = statuses(&[("W", w_status), ("J", "pending")]);
    let deps = [dep("J", "W", DepKind::Judges)];
    let landed = BTreeMap::from([("W".to_string(), false)]);
    promotable_deps(&s, &deps, &landed, &passed(structural_passed))
}

#[test]
fn judge_task_waits_until_its_judged_task_passed_its_structural_check() {
    for status in ["pending", "ready", "leased", "running", "stalled"] {
        assert!(
            !judge_ready(status, &[]).contains(&"J".to_string()),
            "judged task {status}: the judge task stays pending"
        );
    }
    assert!(judge_ready("verifying", &[]).is_empty());
    assert_eq!(judge_ready("verifying", &["W"]), vec!["J"]);
    assert!(judge_ready("verifying", &["X"]).is_empty());
    for status in ["ready", "running", "failed", "escalated", "cancelled"] {
        assert!(
            judge_ready(status, &["W"]).is_empty(),
            "judged task {status}: the judge task stays pending"
        );
    }
    let s = statuses(&[("J", "pending")]);
    let deps = [dep("J", "W", DepKind::Judges)];
    assert!(promotable_deps(&s, &deps, &BTreeMap::new(), &passed(&["W"])).is_empty());
}

#[test]
fn after_dependency_still_needs_done_and_landed() {
    let deps = [dep("B", "A", DepKind::After)];
    let ready = |a: &str, landed: Option<bool>, checked: &[&str]| {
        let s = statuses(&[("A", a), ("B", "pending")]);
        let landed: BTreeMap<String, bool> =
            landed.map(|l| ("A".to_string(), l)).into_iter().collect();
        promotable_deps(&s, &deps, &landed, &passed(checked))
    };
    assert_eq!(ready("done", Some(true), &[]), vec!["B"]);
    assert!(ready("done", Some(false), &[]).is_empty());
    assert_eq!(ready("done", None, &[]), vec!["B"]);
    assert!(ready("verifying", Some(false), &["A"]).is_empty());
    assert!(ready("verifying", None, &["A"]).is_empty());
    let s = statuses(&[("A", "done"), ("W", "verifying"), ("J", "pending")]);
    let both = [
        dep("J", "A", DepKind::After),
        dep("J", "W", DepKind::Judges),
    ];
    let a_landed = BTreeMap::from([("A".to_string(), false), ("W".to_string(), false)]);
    assert!(promotable_deps(&s, &both, &a_landed, &passed(&["W"])).is_empty());
    let all_landed = BTreeMap::from([("A".to_string(), true), ("W".to_string(), false)]);
    assert_eq!(
        promotable_deps(&s, &both, &all_landed, &passed(&["W"])),
        vec!["J"]
    );
}

#[test]
fn a_judge_task_is_a_work_task_with_a_parent() {
    assert_eq!(judged_task("work", Some("T-1")), Some("T-1"));
    assert_eq!(judged_task("work", None), None);
    assert_eq!(judged_task("review", Some("T-1")), None);
}

struct World {
    _tmp: tempfile::TempDir,
    root: PathBuf,
    ledger: PathBuf,
    store: Store,
    run: Run,
    git: Git,
    config: FleetConfig,
}

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

fn task(title: &str) -> NewTask {
    let mut t = NewTask::work(title, &format!("docs/{title}.md"), "opus");
    t.ticket_snapshot = Some("# A ticket\n\nDo the work.\n".into());
    t
}

impl World {
    fn new() -> World {
        let tmp = tempfile::tempdir().unwrap();
        let top = horch_platform::path::canonicalize(tmp.path()).unwrap();
        let root = top.join("repo");
        std::fs::create_dir_all(&root).unwrap();
        git(&root, &["init", "--quiet", "-b", "main"]);
        std::fs::write(root.join("README.md"), "base\n").unwrap();
        std::fs::write(root.join(".gitignore"), ".worktrees/\n").unwrap();
        git(&root, &["add", "README.md", ".gitignore"]);
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
                horch_core::taskledger::schema::run_idea(
                    tx,
                    &run.run_id,
                    &horch_core::taskledger::util::now(),
                    true,
                )?;
                Ok(run)
            })
            .unwrap();
        World {
            _tmp: tmp,
            root,
            ledger,
            store,
            run,
            git: Git::new("git"),
            config: FleetConfig::default(),
        }
    }

    fn add(&mut self, t: NewTask) -> String {
        let run = self.run.clone();
        self.store
            .immediate(|tx| Ok(tasks::add(tx, &run, &t, Actor::Orchestrator)?.task_id))
            .unwrap()
    }

    fn bind(&mut self, task_id: &str) -> anyhow::Result<Bound> {
        let ctx = DispatchCtx {
            jobs: JobCtx {
                guard: None,
                config: &self.config,
                needs_you: &NeedsYouList,
            },
            clock: FleetSeconds(0),
            git: &self.git,
        };
        self.store
            .immediate(|tx| bind(tx, &ctx, task_id, "opus", &NewWorker::default()))
    }

    fn add_tree(&self, id: &str) {
        let mut conn = Connection::open(&self.ledger).unwrap();
        let ctx = JobCtx {
            guard: None,
            config: &self.config,
            needs_you: &NeedsYouList,
        };
        create::finish_reserved(&mut conn, &ctx, &TestJobs, id).unwrap();
    }

    fn set_status(&self, task_id: &str, status: &str) {
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
fn judge_worktree_starts_from_the_judged_branch_tip() {
    let mut w = World::new();
    let judged = w.add(task("retry-doc"));
    let bound = w.bind(&judged).unwrap();
    let tree = bound.worktree.unwrap();
    w.add_tree(&tree.worktree_id);
    let path = Path::new(&tree.path);
    std::fs::write(path.join("retry.md"), "# Retry\n").unwrap();
    git(path, &["add", "retry.md"]);
    git(path, &["commit", "--quiet", "-m", "the result"]);
    let judged_tip = git(&w.root, &["rev-parse", &tree.branch]);
    let run_tip = git(&w.root, &["rev-parse", RUN_BRANCH]);
    assert_ne!(judged_tip, run_tip);
    w.set_status(&judged, "verifying");

    let mut judge = task("retry-doc-judge");
    judge.parent_task_id = Some(judged.clone());
    let judge = w.add(judge);
    let bound = w.bind(&judge).unwrap();
    let judge_tree = bound.worktree.unwrap();

    let row = trees::of_task(w.store.conn(), &judge).unwrap().unwrap();
    assert_eq!(row.base, tree.branch, "the base is the judged branch");
    assert_eq!(
        row.base_commit.as_deref(),
        Some(judged_tip.as_str()),
        "the worktree_add job gets the tip of the judged branch"
    );
    assert_ne!(
        judge_tree.branch, tree.branch,
        "the judge has its own branch"
    );

    w.add_tree(&judge_tree.worktree_id);
    let judge_path = Path::new(&judge_tree.path);
    assert_eq!(git(judge_path, &["rev-parse", "HEAD"]), judged_tip);
    assert!(
        judge_path.join("retry.md").exists(),
        "the judge reads the result"
    );
}

#[test]
fn a_work_task_with_no_parent_still_starts_from_the_run_branch() {
    let mut w = World::new();
    let id = w.add(task("plain"));
    w.bind(&id).unwrap();
    let row = trees::of_task(w.store.conn(), &id).unwrap().unwrap();
    assert_eq!(row.base, RUN_BRANCH);
    assert_eq!(
        row.base_commit.as_deref(),
        Some(git(&w.root, &["rev-parse", RUN_BRANCH]).as_str())
    );
}

#[test]
fn judge_of_a_task_with_no_tree_is_not_bound() {
    let mut w = World::new();
    let judged = w.add(task("never-bound"));
    w.set_status(&judged, "verifying");
    let mut judge = task("its-judge");
    judge.parent_task_id = Some(judged.clone());
    let judge = w.add(judge);
    let err = w.bind(&judge).unwrap_err();
    assert!(
        format!("{err:#}").contains("which has no worktree"),
        "the refusal names the missing tree: {err:#}"
    );
    assert_eq!(tasks::get(w.store.conn(), &judge).unwrap().status, "ready");
}

#[test]
fn review_code_does_not_read_a_judge_task() {
    let mut w = World::new();
    let judged = w.add(task("retry-doc"));
    let mut judge = task("retry-doc-judge");
    judge.parent_task_id = Some(judged.clone());
    let judge = w.add(judge);
    assert_eq!(
        tasks::get(w.store.conn(), &judge).unwrap().kind,
        TaskKind::Work.as_str()
    );
    assert_eq!(
        watchdog::review_round(w.store.conn(), &judged).unwrap(),
        "",
        "a judge task is not a review of its judged task"
    );
    let mut review = task("review");
    review.kind = TaskKind::Review;
    review.parent_task_id = Some(judged.clone());
    let review = w.add(review);
    assert_eq!(
        watchdog::review_round(w.store.conn(), &judged).unwrap(),
        review
    );
}
