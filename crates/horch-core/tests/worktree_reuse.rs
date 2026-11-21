//! The next attempt of a task works in the tree of the last attempt: the
//! bind of fleetd and a handoff give the tree to the new attempt, and an
//! `orphaned` tree is taken only after its rescue
//! (docs/specs/fleetd/design.md, "Worktrees: leased, reconciled, never
//! lost", "Reuse, don't redo"; FM-13, FM-14, FM-25; rows W2 and W3 of
//! preservation-04-behavior.md; docs/tickets/fleetd-p3/11-claim-reuses-the-tree.md).
//!
//! Each test makes its own temporary git repository and ledger. No test
//! starts a `horch` binary: a job runs in the test process (`TestJobs`).

#![cfg(unix)]

#[path = "common/test_jobs.rs"]
mod test_jobs;

use std::path::{Path, PathBuf};
use std::process::Command;

use horch_core::fleetd::fleet_runtime::application::jobs::{JobCtx, Queued};
use horch_core::fleetd::fleet_runtime::application::ledger::{transition, Cause};
use horch_core::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;
use horch_core::fleetd::fleet_runtime::domain::ledger::Entity;
use horch_core::fleetd::ideas::application::needs_you::NeedsYouList;
use horch_core::fleetd::task_execution::application::commands;
use horch_core::fleetd::task_execution::application::dispatch::Bound;
use horch_core::fleetd::task_execution::domain::task::Recorded;
use horch_core::fleetd::worktree_custody::adapters::sqlite::{
    self as trees, WorktreeRow, WorktreeState,
};
use horch_core::fleetd::worktree_custody::application::remove;
use horch_core::taskledger::finish;
use horch_core::taskledger::git::Git;
use horch_core::taskledger::model::Actor;
use horch_core::taskledger::runs::{self, NewRun, Run};
use horch_core::taskledger::supervisor;
use horch_core::taskledger::tasks::{self, NewTask};
use horch_core::taskledger::testing;
use horch_core::taskledger::workers::{self, NewWorker};
use horch_core::taskledger::Store;
use test_jobs::TestJobs;

const RUN_BRANCH: &str = "fleet/r1";

// ─── the world of a test ────────────────────────────────────────────────

struct World {
    tmp: tempfile::TempDir,
    root: PathBuf,
    store: Store,
    run: Run,
    git: Git,
    config: FleetConfig,
}

/// Run git in `dir` for the test itself, with no hook and no config of
/// the user, and give its trimmed output.
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

fn cause() -> Cause<'static> {
    Cause {
        actor: Actor::Horch,
        text: "test",
    }
}

/// The path of the tree of `bound`.
fn tree_path(bound: &Bound) -> Option<&str> {
    bound.worktree.as_ref().map(|t| t.path.as_str())
}

impl World {
    /// A repository with 1 commit on `main` and the run branch, a ledger,
    /// a run of the repository, and 1 task.
    fn new() -> World {
        assert!(
            std::env::var_os("FLEET_ROOT").is_none(),
            "these tests use the fleet root of their temp ledger"
        );
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
                let t = NewTask::work("feature", "t.md", "opus");
                tasks::add(tx, &run, &t, Actor::Orchestrator)?;
                Ok(run)
            })
            .unwrap();
        World {
            tmp,
            root,
            store,
            run,
            git: Git::new("git"),
            config: FleetConfig::default(),
        }
    }

    /// fleetd binds the task to a new started worker of the run and its
    /// live attempt (FR-5). A refused bind writes nothing.
    fn bind(&mut self) -> anyhow::Result<Bound> {
        let n: i64 = self
            .store
            .conn()
            .query_row("SELECT count(*) FROM workers", [], |r| r.get(0))?;
        let task: String = self
            .store
            .conn()
            .query_row("SELECT task_id FROM tasks", [], |r| r.get(0))?;
        let new = NewWorker {
            run_id: self.run.run_id.clone(),
            persona: "opus".into(),
            role: Some(format!("opus-{}", n + 1)),
            ..Default::default()
        };
        let agent = Recorded {
            pid: 4243,
            pid_start: Some(2),
        };
        let bound = testing::bind_live(self.store.conn(), &task, &new, Some(agent))?;
        testing::record_supervisor(self.store.conn(), &bound.worker_id, 4242, Some(1))?;
        Ok(bound)
    }

    /// fleetd binds the task to a new worker, which makes its tree; gives
    /// the worker id and the tree.
    fn first_attempt(&mut self) -> (String, WorktreeRow) {
        let a = self.bind().unwrap().worker_id;
        let wt =
            finish::worktree_new_with(&mut self.store, &self.git, &a, "feat", &TestJobs).unwrap();
        let tree = self.row(&wt.worktree_id);
        assert_eq!(tree.state, WorktreeState::Active);
        (a, tree)
    }

    /// The agent of `worker_id` exits with code 1: the supervisor reaps
    /// the worker, and its task goes back to the queue.
    fn exit(&self, worker_id: &str) {
        horch_core::taskledger::Store::open(self.store.path())
            .and_then(|mut s| {
                s.immediate(|tx| {
                    let ended = supervisor::settle(tx, worker_id, Some(1))?;
                    // The pass of fleetd ends the attempt of the exited agent.
                    commands::end_own_attempt(tx, worker_id, "crashed")?;
                    Ok(ended)
                })
            })
            .unwrap();
        assert_eq!(self.task_status(), "ready");
    }

    /// The `live` attempt of `worker_id`.
    fn attempt(&self, worker_id: &str) -> String {
        self.store
            .conn()
            .query_row(
                "SELECT id FROM attempts WHERE worker_id = ?1 AND state = 'live'",
                [worker_id],
                |r| r.get(0),
            )
            .unwrap()
    }

    fn task_status(&self) -> String {
        self.store
            .conn()
            .query_row("SELECT status FROM tasks", [], |r| r.get(0))
            .unwrap()
    }

    fn row(&self, id: &str) -> WorktreeRow {
        trees::get(self.store.conn(), id).unwrap().unwrap()
    }

    /// Move the row `id` to `state` through the door, as the reconciler
    /// and the rescue job move it.
    fn move_tree(&mut self, id: &str, state: &str) {
        self.store
            .immediate(|tx| {
                transition(
                    tx,
                    None,
                    Entity::Worktree,
                    id,
                    state,
                    cause(),
                    &NeedsYouList,
                )?;
                Ok(())
            })
            .unwrap();
    }

    /// The tree `tree` as a passed rescue leaves it: `orphaned`, then
    /// `rescued` with a rescue ref at its HEAD. Gives the ref.
    fn rescued(&mut self, tree: &WorktreeRow) -> String {
        let id = tree.worktree_id.as_str();
        self.move_tree(id, "orphaned");
        let rescue_ref = format!("refs/horch/rescue/{id}/1");
        let head = git(Path::new(&tree.path), &["rev-parse", "HEAD"]);
        git(&self.root, &["update-ref", &rescue_ref, &head]);
        self.store
            .immediate(|tx| trees::set_rescue_ref(tx, id, &rescue_ref))
            .unwrap();
        self.move_tree(id, "rescued");
        rescue_ref
    }

    /// Queue the `worktree_remove` run of `tree` with `queue_remove`.
    fn queue_remove(&mut self, tree: &WorktreeRow) -> Result<Queued, String> {
        let row = self.row(&tree.worktree_id);
        let config = self.config.clone();
        self.store
            .immediate(|tx| {
                let ctx = JobCtx {
                    guard: None,
                    config: &config,
                    needs_you: &NeedsYouList,
                };
                remove::queue_remove(&ctx, tx, &row, "test", false, FleetSeconds(0))
            })
            .unwrap()
    }

    /// Set the state of the job `id`, as a pass moves it.
    fn set_job_state(&self, id: i64, state: &str) {
        self.store
            .conn()
            .execute(
                "UPDATE jobs SET state = ?2 WHERE id = ?1",
                rusqlite::params![id, state],
            )
            .unwrap();
    }

    fn handoff_file(&self) -> String {
        let path = self.tmp.path().join("handoff.md");
        std::fs::write(&path, "next step\n").unwrap();
        path.display().to_string()
    }
}

/// Each file of `tree` with its bytes, but `.git`, in path order.
fn files(tree: &Path) -> Vec<(String, Vec<u8>)> {
    fn collect(root: &Path, dir: &Path, out: &mut Vec<(String, Vec<u8>)>) {
        for e in std::fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            if p.file_name().is_some_and(|n| n == ".git") {
                continue;
            }
            if p.is_dir() {
                collect(root, &p, out);
            } else {
                let rel = p.strip_prefix(root).unwrap().display().to_string();
                out.push((rel, std::fs::read(&p).unwrap()));
            }
        }
    }
    let mut out = Vec::new();
    collect(tree, tree, &mut out);
    out.sort();
    out
}

// ─── FM-25 and FM-13: the next attempt reuses the tree ──────────────────

#[test]
fn bind_of_a_retried_task_makes_its_tree_active_with_the_new_attempt() {
    let mut w = World::new();
    let (a, tree) = w.first_attempt();
    w.exit(&a);

    let c = w.bind().unwrap();
    let b = c.worker_id.clone();

    let row = w.row(&tree.worktree_id);
    assert_eq!(row.state, WorktreeState::Active);
    assert_eq!(row.owner_attempt, Some(w.attempt(&b)));
    assert_eq!(row.worker_id.as_deref(), Some(b.as_str()));
    assert_eq!(tree_path(&c), Some(tree.path.as_str()));
    assert_eq!(c.worktree.as_ref().and_then(|t| t.add_job), None);
}

#[test]
fn bind_of_a_task_with_a_rescued_tree_takes_the_tree() {
    let mut w = World::new();
    let (a, tree) = w.first_attempt();
    w.exit(&a);
    let rescue_ref = w.rescued(&tree);

    let c = w.bind().unwrap();
    let b = c.worker_id.clone();

    let row = w.row(&tree.worktree_id);
    assert_eq!(row.state, WorktreeState::Active);
    assert_eq!(row.owner_attempt, Some(w.attempt(&b)));
    assert_eq!(tree_path(&c), Some(tree.path.as_str()));
    assert_eq!(row.rescue_ref.as_deref(), Some(rescue_ref.as_str()));
}

#[test]
fn bind_of_a_task_with_an_orphaned_tree_is_refused_before_the_rescue() {
    let mut w = World::new();
    let (a, tree) = w.first_attempt();
    std::fs::write(Path::new(&tree.path).join("work.txt"), "unsaved\n").unwrap();
    w.exit(&a);
    w.move_tree(&tree.worktree_id, "orphaned");
    let before = w.row(&tree.worktree_id);

    let e = w.bind().unwrap_err();

    // The bind takes neither the task nor the tree: the rescue comes first.
    assert!(
        format!("{e:#}").contains("is orphaned: its rescue comes first"),
        "{e:#}"
    );
    assert_eq!(w.task_status(), "ready", "the refused bind wrote nothing");
    assert_eq!(w.row(&tree.worktree_id), before);
    assert_eq!(
        std::fs::read_to_string(Path::new(&tree.path).join("work.txt")).unwrap(),
        "unsaved\n"
    );
}

// ─── FM-14: a worktree has 1 owner ──────────────────────────────────────

#[test]
fn handoff_moves_the_owner_of_the_tree() {
    let mut w = World::new();
    let (a, tree) = w.first_attempt();
    let file = w.handoff_file();

    w.store
        .immediate(|tx| {
            tasks::handoff(tx, &a, &file)?;
            // The pass of fleetd ends the attempt that handed off.
            commands::end_own_attempt(tx, &a, "handed_off")
        })
        .unwrap();

    // The design row `active`: retried -> back to `created`, with no
    // owner, so the next attempt continues in the same tree.
    let row = w.row(&tree.worktree_id);
    assert_eq!(row.state, WorktreeState::Created);
    assert_eq!(row.owner_attempt, None);
    assert_eq!(row.worker_id, None);

    let c = w.bind().unwrap();
    let b = c.worker_id.clone();
    let row = w.row(&tree.worktree_id);
    assert_eq!(row.state, WorktreeState::Active);
    assert_eq!(row.owner_attempt, Some(w.attempt(&b)));
    assert_eq!(row.worker_id.as_deref(), Some(b.as_str()));
    assert_eq!(tree_path(&c), Some(tree.path.as_str()));
    let old = workers::get(w.store.conn(), &a).unwrap();
    assert_eq!(old.handoff_path.as_deref(), Some(file.as_str()));
}

#[test]
fn owner_does_not_move_while_the_old_attempt_is_live() {
    let mut w = World::new();
    let (a, tree) = w.first_attempt();
    let owner = w.attempt(&a);
    // The task is in the queue again while its worker and its attempt are
    // still live: no handoff, no reap, no lease. The worker row lets go
    // of the task, so the unique index of `workers.task_id` does not
    // refuse the bind first. The bind refuses the open attempt (I1).
    w.store
        .immediate(|tx| {
            tx.execute(
                "UPDATE tasks SET status = 'ready', current_worker_id = NULL",
                [],
            )?;
            tx.execute(
                "UPDATE workers SET task_id = NULL WHERE worker_id = ?1",
                [&a],
            )?;
            tx.execute("DELETE FROM leases", [])?;
            Ok(())
        })
        .unwrap();

    let e = w.bind().unwrap_err();

    assert!(
        format!("{e:#}").contains(&format!("the attempt {owner} that is not ended")),
        "{e:#}"
    );
    let row = w.row(&tree.worktree_id);
    assert_eq!(row.state, WorktreeState::Active);
    assert_eq!(row.owner_attempt, Some(owner));
    assert_eq!(row.worker_id.as_deref(), Some(a.as_str()));
    assert_eq!(w.task_status(), "ready", "the refused bind wrote nothing");
}

// ─── W3: a reap leaves the tree with its files ──────────────────────────

#[test]
fn reap_leaves_the_tree_and_each_file() {
    let mut w = World::new();
    let (a, tree) = w.first_attempt();
    let path = Path::new(&tree.path);
    std::fs::write(path.join("done.txt"), "committed\n").unwrap();
    git(path, &["add", "done.txt"]);
    git(path, &["commit", "--quiet", "-m", "work"]);
    std::fs::write(path.join("README.md"), "changed\n").unwrap();
    std::fs::create_dir_all(path.join("new")).unwrap();
    std::fs::write(path.join("new/u.txt"), "untracked\n").unwrap();
    let head = git(path, &["rev-parse", "HEAD"]);
    let before = files(path);

    w.exit(&a);

    let row = w.row(&tree.worktree_id);
    assert_eq!(row.status.as_str(), "open");
    assert_ne!(row.state, WorktreeState::Removed);
    assert_eq!(files(path), before);
    assert_eq!(git(path, &["rev-parse", "HEAD"]), head);

    let c = w.bind().unwrap();
    let b = c.worker_id.clone();
    assert_eq!(tree_path(&c), Some(tree.path.as_str()));
    assert_eq!(w.row(&tree.worktree_id).owner_attempt, Some(w.attempt(&b)));
    assert_eq!(files(path), before);
}

// ─── the bind and the worktree_remove job never both pass ───────────────

#[test]
fn bind_refuses_a_tree_while_its_worktree_remove_job_is_queued_running_or_passed() {
    let mut w = World::new();
    let (a, tree) = w.first_attempt();
    w.exit(&a);
    w.rescued(&tree);
    let job = match w.queue_remove(&tree) {
        Ok(Queued::New(id)) => id,
        other => panic!("expected a new run, got {other:?}"),
    };

    for state in ["queued", "running", "passed"] {
        w.set_job_state(job, state);
        let e = w.bind().unwrap_err();
        assert!(
            format!("{e:#}").contains(&format!("worktree_remove job {job}")),
            "{state}: {e:#}"
        );
        assert_eq!(w.row(&tree.worktree_id).state, WorktreeState::Rescued);
        assert_eq!(
            w.task_status(),
            "ready",
            "{state}: the refused bind wrote nothing"
        );
    }

    // A failed removal keeps the tree: the next bind takes it.
    w.set_job_state(job, "failed");
    let c = w.bind().unwrap();
    assert_eq!(tree_path(&c), Some(tree.path.as_str()));
    assert_eq!(w.row(&tree.worktree_id).state, WorktreeState::Active);
}

#[test]
fn worktree_remove_is_refused_after_the_bind_took_the_tree() {
    let mut w = World::new();
    let (a, tree) = w.first_attempt();
    w.exit(&a);
    w.rescued(&tree);

    w.bind().unwrap();

    let refused = w.queue_remove(&tree).unwrap_err();
    assert!(refused.contains("active"), "{refused}");
    assert_eq!(w.row(&tree.worktree_id).state, WorktreeState::Active);
    assert!(Path::new(&tree.path).is_dir());
}
