//! The landing state machine of a task branch: the branch follows its task,
//! each state has a time limit on the fleet clock, a cancelled task's branch
//! gets a rescue ref and then `abandoned`, and no branch goes from
//! `building` to `landed` (docs/specs/fleetd/design.md, "Landing a task branch on its feature
//! branch", "A cancelled task does not block its idea", "What a release
//! does to a run in flight", row Phase 4;
//! docs/tickets/fleetd-p4/25-landing-state-machine.md).
//!
//! Each test has its own temporary ledger, fleet root and git repository.
//! No test reads the load, the limits or the clock of the machine: the fleet
//! clock is a number that the test gives, and the config is the default.

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::Result;
use horch_core::fleetd::fleet_runtime::application::jobs::JobCtx;
use horch_core::fleetd::fleet_runtime::application::ledger::Cause;
use horch_core::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;
use horch_core::fleetd::fleet_runtime::domain::ledger::{Entity, EXITS};
use horch_core::fleetd::ideas::application::needs_you::{open_items, NeedsYouList};
use horch_core::fleetd::integration::adapters::sqlite::{self as rows, NewFeature};
use horch_core::fleetd::integration::application::landing::{
    self, BranchRescues, Effect, GitBranchRescues, Landing, OVERDUE_KIND,
};
use horch_core::fleetd::worktree_custody::adapters::git::CliWorktreeGit;
use horch_core::taskledger::model::Actor;
use horch_core::taskledger::runs::{self, NewRun, Run};
use horch_core::taskledger::tasks::{self, NewTask};
use horch_core::taskledger::workers::{self, NewWorker};
use horch_core::taskledger::{schema, util};
use rusqlite::{params, Connection};

const FEATURE: &str = "F-0001";
const RUN_BRANCH: &str = "fleet/r1";
const BUDGET: usize = 50;
const RESCUE_PREFIX: &str = "refs/horch/rescue/branch/";

struct World {
    _temp: tempfile::TempDir,
    dir: PathBuf,
    repo: PathBuf,
    conn: Connection,
    run: Run,
    config: FleetConfig,
}

fn cause(text: &str) -> Cause<'_> {
    Cause {
        actor: Actor::Horch,
        text,
    }
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .current_dir(dir)
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn world() -> World {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().to_path_buf();
    let repo = dir.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "--quiet", "--initial-branch=main"]);
    std::fs::write(repo.join("a.txt"), "a\n").unwrap();
    git(&repo, &["add", "a.txt"]);
    git(&repo, &["commit", "--quiet", "-m", "base"]);
    git(&repo, &["branch", RUN_BRANCH]);
    let mut conn = Connection::open(dir.join("ledger.sqlite3")).unwrap();
    schema::migrate_to(&mut conn, schema::VERSION).unwrap();
    conn.execute(
        "INSERT INTO fleetd (id, generation, pid, pid_start, started_at, fleet_clock_s, clock_ticked_at)
         VALUES (1, 1, ?1, 0, ?2, 0, ?2)",
        params![std::process::id(), util::now()],
    )
    .unwrap();
    let new_run = NewRun {
        slug: "r1".into(),
        base_branch: Some("main".into()),
        run_branch: Some(RUN_BRANCH.into()),
        ..Default::default()
    };
    let run = runs::create(&conn, &repo, &new_run).unwrap();
    let new = NewFeature {
        id: FEATURE,
        name: Some("r1"),
        branch: Some(RUN_BRANCH),
        primary_branch: "main",
    };
    let tx = conn.transaction().unwrap();
    rows::insert_feature(&tx, None, &new, cause("feature accepted")).unwrap();
    // FR-12: a task needs an idea in flight of the feature of its run.
    horch_core::taskledger::schema::run_idea(
        &tx,
        &run.run_id,
        &horch_core::taskledger::util::now(),
        true,
    )
    .unwrap();
    tx.commit().unwrap();
    World {
        _temp: temp,
        dir,
        repo,
        conn,
        run,
        config: FleetConfig::default(),
    }
}

/// A rescue port that counts its calls and records nothing in git.
#[derive(Default)]
struct CountingRescues {
    calls: RefCell<Vec<String>>,
}

impl BranchRescues for CountingRescues {
    fn rescue(&self, _repo: &Path, branch: &str) -> Result<Option<String>> {
        self.calls.borrow_mut().push(branch.to_string());
        Ok(Some(format!("{RESCUE_PREFIX}{branch}/1")))
    }
}

impl World {
    fn jobs(&self) -> JobCtx<'_> {
        JobCtx {
            guard: None,
            config: &self.config,
            needs_you: &NeedsYouList,
        }
    }

    /// A new task `title` with its branch row in `building`, as the bind of
    /// its first push attempt writes it. Gives (task, branch).
    fn task(&mut self, title: &str) -> (String, String) {
        let new = NewTask::work(title, &format!("tickets/{title}.md"), "dev");
        let task = tasks::add(&self.conn, &self.run, &new, Actor::Orchestrator)
            .unwrap()
            .task_id;
        let branch = format!("{RUN_BRANCH}-{title}");
        let ctx = self.jobs();
        let tx = self.conn.unchecked_transaction().unwrap();
        landing::bind_branch(&ctx, &tx, &task, &branch, FleetSeconds(0)).unwrap();
        tx.commit().unwrap();
        (task, branch)
    }

    /// 1 attempt of `task` that began by `via` (`claim` or `push`), with its
    /// worker. Gives the worker id.
    fn attempt(&self, task: &str, via: &str) -> String {
        let worker = workers::insert(
            &self.conn,
            &NewWorker {
                run_id: self.run.run_id.clone(),
                persona: "dev".into(),
                ..Default::default()
            },
        )
        .unwrap();
        let n: i64 = self
            .conn
            .query_row(
                "SELECT count(*) + 1 FROM attempts WHERE task_id = ?1",
                [task],
                |r| r.get(0),
            )
            .unwrap();
        self.conn
            .execute(
                "INSERT INTO attempts (id, task_id, worker_id, state, via, started_at)
                 VALUES (?1, ?2, ?3, 'live', ?4, ?5)",
                params![format!("{task}-A{n}"), task, worker, via, util::now()],
            )
            .unwrap();
        worker
    }

    /// The task `task` is in `status` (the task's own state machine is not
    /// under test here).
    fn set_task(&self, task: &str, status: &str) {
        self.conn
            .execute(
                "UPDATE tasks SET status = ?2 WHERE task_id = ?1",
                params![task, status],
            )
            .unwrap();
    }

    /// The branch row moves to `to` through the 1 door, as the use case of
    /// another ticket moves it (the bundle, the bisect).
    fn put(&self, branch: &str, to: &str, clock: i64) {
        let ctx = self.jobs();
        let tx = self.conn.unchecked_transaction().unwrap();
        let to = landing::state_named(to).unwrap();
        landing::enter(&ctx, &tx, branch, to, FleetSeconds(clock), "test move").unwrap();
        tx.commit().unwrap();
    }

    /// 1 pass of `follow` at `clock`.
    fn follow(&mut self, clock: i64) -> Vec<Effect> {
        let config = self.config.clone();
        let ctx = JobCtx {
            guard: None,
            config: &config,
            needs_you: &NeedsYouList,
        };
        let l = Landing { jobs: &ctx };
        let tx = self.conn.transaction().unwrap();
        let effects = landing::follow(&l, &tx, FleetSeconds(clock), BUDGET).unwrap();
        tx.commit().unwrap();
        effects
    }

    /// Run each effect with `port` and record what it did.
    fn settle(&mut self, effects: &[Effect], port: &dyn BranchRescues, clock: i64) {
        for e in effects {
            let done = landing::run(e, port).unwrap();
            let config = self.config.clone();
            let ctx = JobCtx {
                guard: None,
                config: &config,
                needs_you: &NeedsYouList,
            };
            let tx = self.conn.transaction().unwrap();
            landing::observe(&ctx, &tx, FleetSeconds(clock), &done).unwrap();
            tx.commit().unwrap();
        }
    }

    fn git_port(&self) -> CliWorktreeGit {
        CliWorktreeGit::new("git", self.dir.join("fleet")).writing(&[self.repo.join(".git")])
    }

    fn state(&self, branch: &str) -> String {
        rows::branch(&self.conn, branch)
            .unwrap()
            .unwrap()
            .landing_state
            .unwrap()
    }

    fn row(&self, branch: &str) -> rows::BranchRow {
        rows::branch(&self.conn, branch).unwrap().unwrap()
    }

    fn rescue_refs(&self) -> BTreeSet<String> {
        git(
            &self.repo,
            &["for-each-ref", "--format=%(refname)", RESCUE_PREFIX],
        )
        .lines()
        .map(str::to_string)
        .collect()
    }

    /// The open needs-you items of the landing time limits.
    fn overdue_items(&self) -> Vec<String> {
        open_items(&self.conn)
            .unwrap()
            .into_iter()
            .filter(|i| i.kind == OVERDUE_KIND)
            .map(|i| i.key)
            .collect()
    }

    /// A git branch `branch` with 1 commit of its own. Gives its tip.
    fn git_branch(&self, branch: &str) -> String {
        git(&self.repo, &["branch", branch, RUN_BRANCH]);
        git(&self.repo, &["checkout", "--quiet", branch]);
        std::fs::write(self.repo.join("work.txt"), branch).unwrap();
        git(&self.repo, &["add", "work.txt"]);
        git(&self.repo, &["commit", "--quiet", "-m", "work"]);
        git(&self.repo, &["checkout", "--quiet", "main"]);
        git(&self.repo, &["rev-parse", branch])
    }
}

#[test]
fn branch_follows_its_task_to_verifying_and_ready() {
    let mut w = world();
    let (task, branch) = w.task("a");
    w.attempt(&task, "push");
    assert_eq!(w.row(&branch).owner, "worker");

    w.set_task(&task, "running");
    assert!(w.follow(10).is_empty());
    assert_eq!(w.state(&branch), "building");

    w.set_task(&task, "verifying");
    w.follow(20);
    assert_eq!(w.state(&branch), "verifying");
    assert_eq!(w.row(&branch).owner, "fleetd");

    w.set_task(&task, "done");
    w.follow(30);
    let row = w.row(&branch);
    assert_eq!(row.landing_state.as_deref(), Some("ready"));
    assert_eq!(row.owner, "fleetd");
    assert_eq!(row.due_at, 30 + 1800, "ready is due 30 min later");
}

#[test]
fn branch_of_a_task_done_between_2_passes_takes_each_step() {
    let mut w = world();
    let (task, branch) = w.task("a");
    w.attempt(&task, "push");
    w.set_task(&task, "done");
    w.follow(5);
    assert_eq!(w.state(&branch), "ready");
    let moves: i64 = w
        .conn
        .query_row(
            "SELECT count(*) FROM events WHERE entity = 'task_branch' AND entity_id = ?1
             AND kind = 'transition' AND from_state IS NOT NULL",
            [&branch],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(moves, 2, "building -> verifying -> ready, 1 event each");
}

#[test]
fn failed_check_with_a_new_attempt_moves_verifying_back_to_building() {
    let mut w = world();
    let (task, branch) = w.task("a");
    w.attempt(&task, "push");
    w.set_task(&task, "verifying");
    w.follow(10);
    assert_eq!(w.state(&branch), "verifying");

    // The check failed: rung 6 sends the task back, and no attempt runs.
    w.set_task(&task, "ready");
    w.follow(20);
    assert_eq!(w.state(&branch), "verifying");

    // The next attempt is bound.
    w.attempt(&task, "push");
    w.set_task(&task, "leased");
    w.follow(30);
    assert_eq!(w.state(&branch), "building");
    assert_eq!(w.row(&branch).owner, "worker");
}

#[test]
fn cancelled_task_branch_gets_a_rescue_ref_then_abandoned() {
    let mut w = world();
    let (task, branch) = w.task("a");
    w.attempt(&task, "push");
    let tip = w.git_branch(&branch);
    w.set_task(&task, "running");
    w.follow(10);

    w.set_task(&task, "cancelled");
    let effects = w.follow(20);
    assert_eq!(
        effects,
        vec![Effect::Rescue {
            branch: branch.clone(),
            repo: PathBuf::from(&w.run.project_root),
        }]
    );
    assert_eq!(w.state(&branch), "building", "the rescue ref comes first");
    let git_port = w.git_port();
    let port = GitBranchRescues { git: &git_port };
    w.settle(&effects, &port, 21);

    let row = w.row(&branch);
    assert_eq!(row.landing_state.as_deref(), Some("abandoned"));
    let rescue = row.rescue_ref.expect("the row names its rescue ref");
    assert_eq!(git(&w.repo, &["rev-parse", &rescue]), tip);
    assert!(w.follow(30).is_empty(), "abandoned is terminal");
}

#[test]
fn cancelled_task_with_no_git_branch_is_abandoned_with_no_rescue_ref() {
    let mut w = world();
    let (task, branch) = w.task("a");
    w.attempt(&task, "push");
    w.set_task(&task, "cancelled");
    let effects = w.follow(20);
    let git_port = w.git_port();
    let port = GitBranchRescues { git: &git_port };
    w.settle(&effects, &port, 21);
    let row = w.row(&branch);
    assert_eq!(row.landing_state.as_deref(), Some("abandoned"));
    assert_eq!(row.rescue_ref, None);
    assert!(w.rescue_refs().is_empty());
}

#[test]
fn rescue_survives_a_crash_before_and_after_the_commit() {
    let mut w = world();
    let (task, branch) = w.task("a");
    w.attempt(&task, "push");
    let tip = w.git_branch(&branch);
    w.set_task(&task, "cancelled");
    let git_port = w.git_port();
    let port = GitBranchRescues { git: &git_port };

    // Fault point 1: the pass dies before its commit. Nothing is written.
    {
        let config = w.config.clone();
        let ctx = JobCtx {
            guard: None,
            config: &config,
            needs_you: &NeedsYouList,
        };
        let l = Landing { jobs: &ctx };
        let tx = w.conn.transaction().unwrap();
        let lost = landing::follow(&l, &tx, FleetSeconds(10), BUDGET).unwrap();
        assert_eq!(lost.len(), 1);
        drop(tx);
    }
    assert_eq!(w.state(&branch), "building");
    assert!(w.rescue_refs().is_empty());

    // Fault point 2: the pass commits, and dies before the effect.
    let lost = w.follow(20);
    assert_eq!(lost.len(), 1);
    assert!(w.rescue_refs().is_empty());

    // Fault point 3: the effect runs, and fleetd dies before it records it.
    let effects = w.follow(30);
    assert_eq!(effects, lost, "the next pass plans the same effect");
    for e in &effects {
        landing::run(e, &port).unwrap();
    }
    assert_eq!(w.state(&branch), "building");

    // The next pass completes the effect 1 time.
    let effects = w.follow(40);
    w.settle(&effects, &port, 41);
    let refs = w.rescue_refs();
    assert_eq!(refs.len(), 1, "1 rescue ref: {refs:?}");
    let rescue = refs.into_iter().next().unwrap();
    assert_eq!(git(&w.repo, &["rev-parse", &rescue]), tip);
    assert_eq!(git(&w.repo, &["rev-parse", &branch]), tip, "the tip stays");
    let row = w.row(&branch);
    assert_eq!(row.landing_state.as_deref(), Some("abandoned"));
    assert_eq!(row.rescue_ref.as_deref(), Some(rescue.as_str()));
    assert!(w.follow(50).is_empty());
}

#[test]
fn cancel_after_a_branch_left_building_lands_nothing() {
    // F9: a task cancelled at any point before the ref move lands nothing.
    let mut w = world();
    let (task, branch) = w.task("a");
    w.attempt(&task, "push");
    w.set_task(&task, "verifying");
    w.follow(10);
    assert_eq!(w.state(&branch), "verifying");
    w.set_task(&task, "cancelled");
    let effects = w.follow(20);
    let port = CountingRescues::default();
    w.settle(&effects, &port, 21);
    assert_eq!(w.state(&branch), "abandoned");
    assert_eq!(*port.calls.borrow(), vec![branch]);
}

#[test]
fn ready_branch_past_30_min_opens_1_needs_you_item() {
    let mut w = world();
    let (task, branch) = w.task("a");
    w.attempt(&task, "push");
    w.set_task(&task, "done");
    w.follow(100);
    assert_eq!(w.state(&branch), "ready");

    w.follow(100 + 1799);
    assert!(w.overdue_items().is_empty(), "not yet due");

    w.follow(100 + 1800);
    w.follow(100 + 1900);
    let key = format!("{OVERDUE_KIND}:{branch}");
    assert_eq!(
        w.overdue_items(),
        vec![key.clone()],
        "1 item, keyed by the branch"
    );
    let opened: i64 = w
        .conn
        .query_row(
            "SELECT count(*) FROM events WHERE entity = 'needs_you' AND entity_id = ?1",
            [&key],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(opened, 1, "a second pass opens nothing more");

    // The branch leaves `ready`: the item ends.
    w.put(&branch, "in_bundle", 2000);
    w.follow(2001);
    assert!(w.overdue_items().is_empty());
}

#[test]
fn each_state_with_a_limit_is_due_by_its_key() {
    let mut w = world();
    w.config.landing_ready_limit_s = 7;
    w.config.landing_repair_limit_s = 11;
    w.config.landing_job_margin_s = 13;
    w.config.job_limit_s.bundle_build = 17;
    w.config.job_limit_s.fast_gate = 19;
    let (task, branch) = w.task("a");
    w.attempt(&task, "push");
    w.set_task(&task, "done");
    w.follow(1000);
    assert_eq!(w.row(&branch).due_at, 1000 + 7);
    w.put(&branch, "in_bundle", 2000);
    assert_eq!(w.row(&branch).due_at, 2000 + 17 + 19 + 13);
    w.put(&branch, "conflicted", 3000);
    assert_eq!(w.row(&branch).due_at, 3000 + 11);
    w.follow(3000 + 11);
    assert_eq!(w.overdue_items().len(), 1);
    w.put(&branch, "ready", 4000);
    w.put(&branch, "in_bundle", 4001);
    w.put(&branch, "gate_failed", 4002);
    assert_eq!(w.row(&branch).due_at, 4002 + 11);
    w.follow(4003);
    assert!(w.overdue_items().is_empty(), "gate_failed is not due yet");
}

/// T-871 removed the interim edge `building -> landed` of the old claim
/// path. The door refuses it, and the branch of an attempt of an old
/// ledger (`via = claim`) lands only through a bundle, as a push branch.
#[test]
fn no_branch_goes_from_building_to_landed() {
    assert!(!EXITS.contains(&(Entity::TaskBranch, "building", "landed")));
    let mut w = world();
    let (task, branch) = w.task("a");
    w.attempt(&task, "claim");
    w.set_task(&task, "done");
    w.follow(10);
    assert_eq!(w.state(&branch), "ready");

    let (other, other_branch) = w.task("b");
    w.attempt(&other, "push");
    let config = w.config.clone();
    let ctx = JobCtx {
        guard: None,
        config: &config,
        needs_you: &NeedsYouList,
    };
    let tx = w.conn.transaction().unwrap();
    let landed = landing::state_named("landed").unwrap();
    let refused = landing::enter(&ctx, &tx, &other_branch, landed, FleetSeconds(20), "test");
    assert!(refused.is_err(), "{refused:?}");
    drop(tx);
    let landed: i64 = w
        .conn
        .query_row(
            "SELECT count(*) FROM events WHERE entity = 'task_branch'
             AND kind = 'transition' AND to_state = 'landed'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(landed, 0, "no branch went to landed");
}

#[test]
fn first_push_bind_writes_the_branch_row_once() {
    let mut w = world();
    let (task, branch) = w.task("a");
    let row = w.row(&branch);
    assert_eq!(row.feature_id, FEATURE);
    assert_eq!(row.task_id.as_deref(), Some(task.as_str()));
    assert_eq!(row.landing_state.as_deref(), Some("building"));
    // A second bind of the task keeps the row.
    w.set_task(&task, "verifying");
    w.attempt(&task, "push");
    w.follow(5);
    let ctx = w.jobs();
    let tx = w.conn.unchecked_transaction().unwrap();
    landing::bind_branch(&ctx, &tx, &task, &branch, FleetSeconds(9)).unwrap();
    tx.commit().unwrap();
    assert_eq!(w.state(&branch), "verifying");
}
