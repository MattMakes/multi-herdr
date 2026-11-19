//! A feature is `ready` only when each of its ideas is `integrating`,
//! `cancelled` or `parked`, and the full gate passed on its tip over the
//! fetched primary tip. The idea state replaces the Phase 4 stand-in "each
//! task of the run is `done` or `cancelled`" (docs/specs/fleetd/design.md,
//! "The feature state machine", REQUIRED item 5;
//! docs/tickets/fleetd-p6/34-feature-ready.md).
//!
//! Each test has its own temporary ledger and git repository. No test reads
//! the load, the limits or the clock of the machine: the fleet clock is a
//! number that the test gives, and the config is the default. The full gate
//! job does not run here: the test writes its verdict in `gate_runs`, as
//! the gate executor writes it. The idea dispatcher and the roll-up do not
//! run here: the test writes the state of each idea.

use std::path::{Path, PathBuf};
use std::process::Command;

use horch_core::fleetd::fleet_runtime::application::jobs::JobCtx;
use horch_core::fleetd::fleet_runtime::application::ledger::Cause;
use horch_core::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;
use horch_core::fleetd::ideas::adapters::ideas_store::{self, NewIdea};
use horch_core::fleetd::ideas::application::needs_you::NeedsYouList;
use horch_core::fleetd::integration::adapters::git::CliGitGateway;
use horch_core::fleetd::integration::adapters::sqlite::{self as rows, NewFeature};
use horch_core::fleetd::integration::application::features::{self, Features};
use horch_core::fleetd::integration::application::landing;
use horch_core::fleetd::worktree_custody::adapters::git::CliWorktreeGit;
use horch_core::taskledger::model::Actor;
use horch_core::taskledger::runs::{self, NewRun, Run};
use horch_core::taskledger::tasks::{self, NewTask};
use horch_core::taskledger::{schema, util};
use rusqlite::{params, Connection};

const FEATURE: &str = "F-0001";
const RUN_BRANCH: &str = "fleet/r1";
/// The idea that the first task of the run gets (`schema::run_idea`).
const FIRST_IDEA: &str = "I-0001";

struct World {
    _temp: tempfile::TempDir,
    repo: PathBuf,
    conn: Connection,
    run: Run,
    config: FleetConfig,
    git: CliGitGateway,
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

/// A repository with `main` and the run branch at the same commit, a temp
/// ledger with the fleet clock at 0, the run and its feature `F-0001` in
/// `open`, and the fetched tip of `main`.
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
    // FR-12: a task needs an idea in flight of the feature of its run.
    horch_core::taskledger::schema::run_idea(
        &conn,
        &run.run_id,
        &horch_core::taskledger::util::now(),
        true,
    )
    .unwrap();
    let new = NewFeature {
        id: FEATURE,
        name: Some("r1"),
        branch: Some(RUN_BRANCH),
        primary_branch: "main",
    };
    let tx = conn.transaction().unwrap();
    rows::insert_feature(&tx, None, &new, cause("feature accepted")).unwrap();
    tx.commit().unwrap();
    let git = CliGitGateway::new(
        CliWorktreeGit::new("git", dir.join("fleet")).writing(&[repo.join(".git")]),
    );
    let w = World {
        _temp: temp,
        repo,
        conn,
        run,
        config: FleetConfig::default(),
        git,
    };
    w.fetched_main();
    w
}

impl World {
    /// 1 pass of the feature state machine.
    fn follow(&self) {
        let ctx = JobCtx {
            guard: None,
            config: &self.config,
            needs_you: &NeedsYouList,
        };
        let f = Features {
            jobs: &ctx,
            git: &self.git,
        };
        let tx = self.conn.unchecked_transaction().unwrap();
        features::follow(&f, &tx, FleetSeconds(0)).unwrap();
        tx.commit().unwrap();
    }

    fn state(&self) -> String {
        rows::feature(&self.conn, FEATURE).unwrap().unwrap().state
    }

    fn tip(&self, rev: &str) -> String {
        git(&self.repo, &["rev-parse", rev])
    }

    /// 1 commit on `branch` that writes `text` to `file`. Gives its tip.
    fn commit_on(&self, branch: &str, file: &str, text: &str) -> String {
        git(&self.repo, &["checkout", "--quiet", branch]);
        std::fs::write(self.repo.join(file), text).unwrap();
        git(&self.repo, &["add", file]);
        git(&self.repo, &["commit", "--quiet", "-m", file]);
        let tip = self.tip("HEAD");
        git(&self.repo, &["checkout", "--quiet", "main"]);
        tip
    }

    /// The fetched tip of `main` is the local tip of `main`, as a passed
    /// fetch writes it.
    fn fetched_main(&self) {
        let tip = self.tip("main");
        let tx = self.conn.unchecked_transaction().unwrap();
        rows::put_remote_tip(&tx, None, &self.run.project_root, "main", &tip).unwrap();
        tx.commit().unwrap();
    }

    /// A new idea `id` of the feature.
    fn idea(&self, id: &str) {
        let new = NewIdea {
            id,
            feature_id: FEATURE,
            priority: 0,
        };
        let tx = self.conn.unchecked_transaction().unwrap();
        ideas_store::insert_idea(&tx, None, &new, cause("idea added")).unwrap();
        tx.commit().unwrap();
    }

    /// The idea `id` is in `state` (the idea state machine is not under
    /// test here).
    fn idea_state(&self, id: &str, state: &str) {
        self.conn
            .execute(
                "UPDATE ideas SET state = ?2 WHERE id = ?1",
                params![id, state],
            )
            .unwrap();
    }

    /// A new task `title` of the idea `idea`, with its branch row in
    /// `building`. Gives (task, branch).
    fn task(&self, title: &str, idea: &str) -> (String, String) {
        let new = NewTask::work(title, &format!("tickets/{title}.md"), "dev");
        let task = tasks::add(&self.conn, &self.run, &new, Actor::Orchestrator)
            .unwrap()
            .task_id;
        self.conn
            .execute(
                "UPDATE tasks SET idea_id = ?2 WHERE task_id = ?1",
                params![task, idea],
            )
            .unwrap();
        let branch = format!("{RUN_BRANCH}-{title}");
        let ctx = JobCtx {
            guard: None,
            config: &self.config,
            needs_you: &NeedsYouList,
        };
        let tx = self.conn.unchecked_transaction().unwrap();
        landing::bind_branch(&ctx, &tx, &task, &branch, FleetSeconds(0)).unwrap();
        tx.commit().unwrap();
        (task, branch)
    }

    /// The task `task` is `status`, and its branch is `landing` (the task
    /// and landing state machines are not under test here).
    fn settle(&self, task: &str, status: &str, branch: &str, landing: &str) {
        self.conn
            .execute(
                "UPDATE tasks SET status = ?2 WHERE task_id = ?1",
                params![task, status],
            )
            .unwrap();
        self.conn
            .execute(
                "UPDATE branches SET landing_state = ?2 WHERE name = ?1",
                params![branch, landing],
            )
            .unwrap();
    }

    /// The full gate runs of the feature, as (tip, base tip, result).
    fn full_gates(&self) -> Vec<(String, String, Option<String>)> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT tip, base_tip, result FROM gate_runs
                 WHERE feature_id = ?1 AND gate = 'full' ORDER BY id",
            )
            .unwrap();
        let rows = stmt
            .query_map([FEATURE], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap();
        rows.collect::<rusqlite::Result<_>>().unwrap()
    }

    /// The full gate job of the open gate run passed, as the gate executor
    /// records it.
    fn pass_full_gate(&self) {
        let (id, job): (i64, i64) = self
            .conn
            .query_row(
                "SELECT id, job_id FROM gate_runs
                 WHERE feature_id = ?1 AND gate = 'full' AND result IS NULL
                 ORDER BY id DESC LIMIT 1",
                [FEATURE],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        let tx = self.conn.unchecked_transaction().unwrap();
        rows::set_gate_result(&tx, None, id, job, "pass", cause("the gate ran")).unwrap();
        // The job ends (the job state machine is not under test here).
        tx.execute("UPDATE jobs SET state = 'passed' WHERE id = ?1", [job])
            .unwrap();
        tx.commit().unwrap();
    }
}

#[test]
fn feature_with_an_idea_in_progress_is_not_ready() {
    let w = world();
    let (task, branch) = w.task("one", FIRST_IDEA);
    w.commit_on(RUN_BRANCH, "b.txt", "b\n");
    // Each task is done and each branch is landed, but the idea is still
    // `in_progress`: the feature waits, and no full gate runs.
    w.settle(&task, "done", &branch, "landed");
    w.idea_state(FIRST_IDEA, "in_progress");
    w.follow();
    w.follow();
    assert_eq!(w.state(), "open");
    assert!(w.full_gates().is_empty());

    // A second idea that has no task yet holds the feature too.
    w.idea_state(FIRST_IDEA, "integrating");
    w.idea("I-late");
    w.follow();
    assert_eq!(w.state(), "open");
    assert!(w.full_gates().is_empty());
}

#[test]
fn feature_with_ideas_integrating_cancelled_or_parked_is_ready_after_the_full_gate() {
    let w = world();
    w.idea("I-cancelled");
    w.idea("I-parked");
    let (t1, b1) = w.task("one", FIRST_IDEA);
    let (t2, b2) = w.task("two", "I-cancelled");
    // The task of a parked idea goes on, and it does not hold the feature.
    w.task("three", "I-parked");
    let tip = w.commit_on(RUN_BRANCH, "b.txt", "b\n");
    w.settle(&t1, "done", &b1, "landed");
    w.settle(&t2, "cancelled", &b2, "abandoned");
    w.idea_state(FIRST_IDEA, "integrating");
    w.idea_state("I-cancelled", "cancelled");
    w.idea_state("I-parked", "parked");

    // 1 full gate on the feature tip over the fetched primary tip. The
    // feature stays `open` until it passes.
    w.follow();
    assert_eq!(w.state(), "open");
    assert_eq!(w.full_gates(), vec![(tip, w.tip("main"), None)]);
    w.pass_full_gate();
    w.follow();
    assert_eq!(w.state(), "ready");

    // A new idea joins with `--to`: the feature is `open` again.
    w.idea("I-joined");
    w.follow();
    assert_eq!(w.state(), "open");
}

#[test]
fn open_feature_merged_by_content_is_merged() {
    let w = world();
    let (t1, b1) = w.task("one", FIRST_IDEA);
    w.idea("I-flight");
    w.task("two", "I-flight");
    w.commit_on(RUN_BRANCH, "b.txt", "b\n");
    w.settle(&t1, "done", &b1, "landed");
    w.idea_state(FIRST_IDEA, "integrating");
    w.idea_state("I-flight", "in_progress");
    w.follow();
    assert_eq!(w.state(), "open");

    // You merge the pull request while an idea is in flight: git shows the
    // changes of the feature in `main` by content, so the feature is
    // `merged` (ticket 35 moves the idea in flight).
    git(&w.repo, &["merge", "--quiet", "--squash", RUN_BRANCH]);
    git(&w.repo, &["commit", "--quiet", "-m", "squash"]);
    w.fetched_main();
    w.follow();
    assert_eq!(w.state(), "merged");
    assert!(w.full_gates().is_empty());
}
