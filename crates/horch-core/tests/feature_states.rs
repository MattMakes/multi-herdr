//! The feature state machine of the 1 feature of a run: `open`, `ready`
//! after the full gate on its tip over the fetched primary tip, back to
//! `open` when a tip moves, and `merged` by content, decided 1 time
//! (docs/specs/fleetd/design.md, "The feature state machine", "Between
//! features" (The full gate; Merged), I8; docs/tickets/fleetd-p4/MAP.md,
//! Q5; docs/tickets/fleetd-p4/36-feature-state-machine.md). The idea
//! state of docs/tickets/fleetd-p6/34-feature-ready.md replaces the stand-in
//! of Q20: the test sets the idea of the run to `integrating`.
//!
//! Each test has its own temporary ledger and git repository. No test reads
//! the load, the limits or the clock of the machine: the fleet clock is a
//! number that the test gives, and the config is the default. The full gate
//! job does not run here: the test writes its verdict in `gate_runs`, as
//! the gate executor writes it.

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::Result;
use horch_core::fleetd::fleet_runtime::adapters::jobs_store;
use horch_core::fleetd::fleet_runtime::application::jobs::JobCtx;
use horch_core::fleetd::fleet_runtime::application::ledger::Cause;
use horch_core::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;
use horch_core::fleetd::ideas::application::needs_you::NeedsYouList;
use horch_core::fleetd::integration::adapters::git::CliGitGateway;
use horch_core::fleetd::integration::adapters::sqlite::{self as rows, NewFeature};
use horch_core::fleetd::integration::application::features::{self, Features};
use horch_core::fleetd::integration::application::landing;
use horch_core::fleetd::integration::application::ports::{
    BranchTip, Candidate, IntegrationGitGateway, Member, RefMove,
};
use horch_core::fleetd::worktree_custody::adapters::git::CliWorktreeGit;
use horch_core::fleetd::worktree_custody::application::create::{self, Observed};
use horch_core::fleetd::worktree_custody::application::gate_tree::{
    self, GateTree, GateTreeRequest,
};
use horch_core::taskledger::model::Actor;
use horch_core::taskledger::runs::{self, NewRun, Run};
use horch_core::taskledger::tasks::{self, NewTask};
use horch_core::taskledger::{schema, util};
use rusqlite::{params, Connection};

const FEATURE: &str = "F-0001";
const RUN_BRANCH: &str = "fleet/r1";

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
/// `open`.
/// [`bare_world`] with the gate tree of [`FEATURE`] made, so a full gate
/// can run.
fn world() -> World {
    let w = bare_world();
    w.gate_tree();
    w
}

/// A world whose feature has no gate tree yet.
fn bare_world() -> World {
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
    let git = CliGitGateway::new(
        CliWorktreeGit::new("git", dir.join("fleet")).writing(&[repo.join(".git")]),
    );
    World {
        _temp: temp,
        repo,
        conn,
        run,
        config: FleetConfig::default(),
        git,
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

    /// 1 pass of the feature state machine through `gateway`.
    fn follow_with(&self, gateway: &dyn IntegrationGitGateway) {
        let ctx = self.jobs();
        let f = Features {
            jobs: &ctx,
            git: gateway,
        };
        let tx = self.conn.unchecked_transaction().unwrap();
        features::follow(&f, &tx, FleetSeconds(0)).unwrap();
        tx.commit().unwrap();
    }

    fn follow(&self) {
        self.follow_with(&self.git);
    }

    /// The repository as the run names it: fleetd reads this path.
    fn project_root(&self) -> PathBuf {
        PathBuf::from(&self.run.project_root)
    }

    /// The gate tree of [`FEATURE`]: its reserved row, then its made
    /// folder.
    fn gate_tree(&self) {
        let job = self.reserve_gate_tree();
        self.make_gate_tree(job);
    }

    /// The `worktree_add` run of the reserved gate tree of [`FEATURE`].
    fn reserve_gate_tree(&self) -> i64 {
        let tip = self.tip(RUN_BRANCH);
        let repo = self.project_root();
        let req = GateTreeRequest {
            feature_id: FEATURE,
            run_id: &self.run.run_id,
            repo: &repo,
            base: RUN_BRANCH,
            tip: &tip,
        };
        let tx = self.conn.unchecked_transaction().unwrap();
        let reserved = gate_tree::reserve(&self.jobs(), &tx, &req, FleetSeconds(0)).unwrap();
        tx.commit().unwrap();
        match reserved {
            GateTree::Reserved { job, .. } => job,
            other => panic!("expected a reserved gate tree, got {other:?}"),
        }
    }

    /// The `worktree_add` run `job` of the gate tree makes its folder and
    /// passes, and a pass observes it.
    fn make_gate_tree(&self, job: i64) {
        let path = gate_tree::path(&self.project_root(), FEATURE);
        let tip = self.tip(RUN_BRANCH);
        git(
            &self.repo,
            &[
                "worktree",
                "add",
                "--quiet",
                "--detach",
                &path.to_string_lossy(),
                &tip,
            ],
        );
        self.conn
            .execute(
                "UPDATE jobs SET state = 'passed', result_json = ?2 WHERE id = ?1",
                params![job, r#"{"passed":true,"verdict":true,"text":"made"}"#],
            )
            .unwrap();
        let ended = jobs_store::get(&self.conn, job).unwrap().unwrap();
        let tx = self.conn.unchecked_transaction().unwrap();
        let seen = create::observe(&self.jobs(), &tx, &ended).unwrap();
        tx.commit().unwrap();
        assert!(matches!(seen, Observed::Created(_)), "{seen:?}");
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

    /// The fetched tip of `main`, as a passed fetch writes it.
    fn fetched_main(&self, tip: &str) {
        let tx = self.conn.unchecked_transaction().unwrap();
        rows::put_remote_tip(&tx, None, &self.run.project_root, "main", tip).unwrap();
        tx.commit().unwrap();
    }

    /// A new task `title` with its branch row in `building`, as the bind of
    /// its first push attempt writes it. Gives (task, branch).
    fn task(&self, title: &str) -> (String, String) {
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

    /// The idea of the run is `integrating`, as the roll-up writes it when
    /// each task is settled (the idea state machine is not under test here).
    fn integrating(&self) {
        self.conn
            .execute(
                "UPDATE ideas SET state = 'integrating' WHERE feature_id = ?1",
                [FEATURE],
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

    fn feature_events(&self) -> i64 {
        self.conn
            .query_row(
                "SELECT count(*) FROM events WHERE entity = 'feature' AND entity_id = ?1",
                [FEATURE],
                |r| r.get(0),
            )
            .unwrap()
    }

    /// The feature with 1 landed task, 1 cancelled task and a passed full
    /// gate on its tip over the fetched `main`: `ready`.
    fn ready(&self) {
        let (t1, b1) = self.task("one");
        let (t2, b2) = self.task("two");
        self.commit_on(RUN_BRANCH, "b.txt", "b\n");
        self.fetched_main(&self.tip("main"));
        self.settle(&t1, "done", &b1, "landed");
        self.settle(&t2, "cancelled", &b2, "abandoned");
        self.integrating();
        self.follow();
        self.pass_full_gate();
        self.follow();
        assert_eq!(self.state(), "ready");
    }

    /// A squash merge of the feature branch on `main`, then 1 more commit
    /// on `main` that the feature did not change. Gives the tip of `main`.
    fn squash_merge(&self) -> String {
        git(&self.repo, &["merge", "--quiet", "--squash", RUN_BRANCH]);
        git(&self.repo, &["commit", "--quiet", "-m", "squash"]);
        self.commit_on("main", "c.txt", "c\n")
    }
}

/// A gateway that counts its calls and then calls git.
struct Counting<'a> {
    inner: &'a CliGitGateway,
    calls: Cell<usize>,
}

impl Counting<'_> {
    fn call(&self) {
        self.calls.set(self.calls.get() + 1);
    }
}

impl IntegrationGitGateway for Counting<'_> {
    fn tip(&self, repo: &Path, name: &str) -> Result<Option<String>> {
        self.call();
        self.inner.tip(repo, name)
    }
    fn update_ref(
        &self,
        repo: &Path,
        name: &str,
        new: &str,
        expected_old: Option<&str>,
    ) -> Result<RefMove> {
        self.call();
        self.inner.update_ref(repo, name, new, expected_old)
    }
    fn delete_ref(&self, repo: &Path, name: &str, expected_old: &str) -> Result<RefMove> {
        self.call();
        self.inner.delete_ref(repo, name, expected_old)
    }
    fn changes_in(&self, repo: &Path, tip: &str, base: &str, target: &str) -> Result<bool> {
        self.call();
        self.inner.changes_in(repo, tip, base, target)
    }
    fn build_candidate(&self, repo: &Path, base: &str, members: &[Member]) -> Result<Candidate> {
        self.call();
        self.inner.build_candidate(repo, base, members)
    }
    fn fleet_branches(&self, repo: &Path) -> Result<Vec<BranchTip>> {
        self.call();
        self.inner.fleet_branches(repo)
    }
    fn task_branches(&self, repo: &Path, run_branch: &str) -> Result<Vec<BranchTip>> {
        self.call();
        self.inner.task_branches(repo, run_branch)
    }

    fn stash_branches(&self, repo: &Path) -> Result<Vec<String>> {
        self.call();
        self.inner.stash_branches(repo)
    }
    fn fleet_toml(&self, repo: &Path, tip: &str) -> Result<Option<String>> {
        self.call();
        self.inner.fleet_toml(repo, tip)
    }
    fn ensure_settings(&self, dir: &Path) -> Result<()> {
        self.call();
        self.inner.ensure_settings(dir)
    }
}

#[test]
fn feature_is_ready_after_its_branches_landed_and_the_full_gate_passed() {
    let w = world();
    w.fetched_main(&w.tip("main"));
    // A feature with no landed task branch has no changes: its tip is in
    // `main`, but it is not merged, and no gate runs.
    w.follow();
    assert_eq!(w.state(), "open");
    assert!(w.full_gates().is_empty());

    let (t1, b1) = w.task("one");
    let (t2, b2) = w.task("two");
    let tip = w.commit_on(RUN_BRANCH, "b.txt", "b\n");
    let main = w.tip("main");
    w.settle(&t1, "done", &b1, "landed");
    // 1 task is still running: no gate.
    w.follow();
    assert_eq!(w.state(), "open");
    assert!(w.full_gates().is_empty());

    // The task is cancelled but its branch is not abandoned yet: no gate.
    w.settle(&t2, "cancelled", &b2, "building");
    w.follow();
    assert!(w.full_gates().is_empty());

    // Each task is done or cancelled, each branch landed or abandoned, so
    // the idea of the run is `integrating`: 1 full gate on the feature tip
    // over the fetched primary tip.
    w.settle(&t2, "cancelled", &b2, "abandoned");
    w.integrating();
    w.follow();
    assert_eq!(w.state(), "open");
    assert_eq!(w.full_gates(), vec![(tip.clone(), main.clone(), None)]);
    let job: (String, String) = w
        .conn
        .query_row(
            "SELECT kind, subject FROM jobs ORDER BY id DESC LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(job, ("full_gate".to_string(), FEATURE.to_string()));

    // The gate runs once: a second pass queues no second run.
    w.follow();
    assert_eq!(w.full_gates().len(), 1);
    assert_eq!(w.state(), "open");

    w.pass_full_gate();
    w.follow();
    assert_eq!(w.state(), "ready");
    w.follow();
    assert_eq!(w.state(), "ready");
}

/// Q18: a full gate runs only in the gate tree of its feature. A feature
/// that never rebased has no gate tree: when its full gate is due, the pass
/// reserves the tree and queues its `worktree_add` run, and no full gate.
/// The pass after the tree is made queues 1 full gate.
#[test]
fn full_gate_waits_for_the_gate_tree_of_the_feature() {
    let w = bare_world();
    w.fetched_main(&w.tip("main"));
    let (t1, b1) = w.task("one");
    let tip = w.commit_on(RUN_BRANCH, "b.txt", "b\n");
    let main = w.tip("main");
    w.settle(&t1, "done", &b1, "landed");
    w.integrating();

    w.follow();
    assert!(w.full_gates().is_empty(), "no gate tree, no gate");
    let tree = gate_tree::of_feature(&w.conn, &w.project_root(), FEATURE)
        .unwrap()
        .expect("the reserved row of the gate tree");
    let adds: Vec<(i64, String)> = {
        let mut stmt = w
            .conn
            .prepare("SELECT id, subject FROM jobs WHERE kind = 'worktree_add'")
            .unwrap();
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        rows.collect::<rusqlite::Result<_>>().unwrap()
    };
    assert_eq!(adds.len(), 1, "{adds:?}");
    assert_eq!(adds[0].1, tree.worktree_id);

    w.make_gate_tree(adds[0].0);
    w.follow();
    assert_eq!(w.full_gates(), vec![(tip, main, None)]);
}

#[test]
fn moved_primary_tip_makes_the_full_gate_stale_and_the_feature_open() {
    let w = world();
    w.ready();
    let tip = w.tip(RUN_BRANCH);
    let moved = w.commit_on("main", "c.txt", "c\n");
    // The local primary branch is not the fetched tip: nothing changes.
    w.follow();
    assert_eq!(w.state(), "ready");

    w.fetched_main(&moved);
    w.follow();
    assert_eq!(w.state(), "open");
    // The pass on the old tips does not make the feature ready again: the
    // gate runs again on the new tips.
    w.follow();
    assert_eq!(w.state(), "open");
    let gates = w.full_gates();
    assert_eq!(gates.len(), 2);
    assert_eq!(gates[1], (tip, moved, None));
    w.pass_full_gate();
    w.follow();
    assert_eq!(w.state(), "ready");
}

#[test]
fn moved_feature_tip_makes_the_feature_open() {
    let w = world();
    w.ready();
    w.commit_on(RUN_BRANCH, "d.txt", "d\n");
    w.follow();
    assert_eq!(w.state(), "open");
}

#[test]
fn squash_merged_feature_is_merged() {
    let w = world();
    w.ready();
    // The squash is on the local `main`, not yet fetched: not merged.
    let merged = w.squash_merge();
    w.follow();
    assert_eq!(w.state(), "ready");

    w.fetched_main(&merged);
    w.follow();
    assert_eq!(w.state(), "merged");
}

#[test]
fn merged_is_decided_once() {
    let w = world();
    w.ready();
    let base = w.tip("main");
    let merged = w.squash_merge();
    w.fetched_main(&merged);
    w.follow();
    assert_eq!(w.state(), "merged");
    let events = w.feature_events();

    // The fetched tip moves to a commit without the changes of the
    // feature: fleetd does not read git for the feature again.
    w.fetched_main(&base);
    let counting = Counting {
        inner: &w.git,
        calls: Cell::new(0),
    };
    w.follow_with(&counting);
    assert_eq!(w.state(), "merged");
    assert_eq!(counting.calls.get(), 0);
    assert_eq!(w.feature_events(), events);
}

#[test]
fn remote_none_reads_the_local_primary_branch() {
    let w = world();
    let (t1, b1) = w.task("one");
    w.commit_on(RUN_BRANCH, "b.txt", "b\n");
    let tip = w.commit_on(RUN_BRANCH, "fleet.toml", "remote = \"none\"\n");
    w.settle(&t1, "done", &b1, "landed");
    w.integrating();
    // A fetched tip that the project does not use.
    let side = w.commit_on("main", "c.txt", "c\n");
    git(&w.repo, &["reset", "--quiet", "--hard", "HEAD~1"]);
    w.fetched_main(&side);
    let main = w.tip("main");

    w.follow();
    assert_eq!(w.full_gates(), vec![(tip, main, None)]);
    w.pass_full_gate();
    w.follow();
    assert_eq!(w.state(), "ready");

    // A fetched tip with the changes of the feature does not make it
    // merged with `remote = none`: the local primary branch does.
    git(&w.repo, &["branch", "elsewhere"]);
    git(&w.repo, &["checkout", "--quiet", "elsewhere"]);
    git(&w.repo, &["merge", "--quiet", "--squash", RUN_BRANCH]);
    git(&w.repo, &["commit", "--quiet", "-m", "squash"]);
    let squashed = w.tip("HEAD");
    git(&w.repo, &["checkout", "--quiet", "main"]);
    w.fetched_main(&squashed);
    w.follow();
    assert_eq!(w.state(), "ready");

    git(&w.repo, &["merge", "--quiet", "--ff-only", "elsewhere"]);
    w.follow();
    assert_eq!(w.state(), "merged");
}
