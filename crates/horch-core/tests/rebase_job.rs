//! The `rebase` job: when the fetched primary tip moves and the feature
//! does not contain it, fleetd writes a rescue ref of the feature tip, then
//! queues 1 rebase job. The job rebases the feature branch on the primary
//! tip in the gate tree of the feature and moves the branch by
//! compare-and-set. No rebase is queued while a bundle of the feature is in
//! `gating` or `passed` (docs/specs/fleetd/design.md, "Between features",
//! "The kinds of job", I9, I10, FM-37; docs/tickets/fleetd-p4/41-rebase-job.md).
//!
//! Each test makes its own ledger and git repository in a temporary folder.
//! No test touches this repository, and no test reads the clock, the load or
//! the limits of the machine: the fleet clock is a number of the test.
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::Duration;

use horch_core::fleetd::fleet_runtime::adapters::jobs_store::{self, JobRow};
use horch_core::fleetd::fleet_runtime::application::jobs::{
    self, outcome, result_data, CheckRunner, JobCtx, JobRunner, SandboxProfiles,
};
use horch_core::fleetd::fleet_runtime::application::ledger::Cause;
use horch_core::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;
use horch_core::fleetd::fleet_runtime::domain::jobs::{Kind, State};
use horch_core::fleetd::ideas::application::needs_you::NeedsYouList;
use horch_core::fleetd::integration::adapters::exec_rebase::{self, GitRebaseRefs};
use horch_core::fleetd::integration::adapters::git::CliGitGateway;
use horch_core::fleetd::integration::adapters::sqlite::{
    self as rows, BranchFact, NewBranch, NewFeature,
};
use horch_core::fleetd::integration::application::bundles::{self, Bundles};
use horch_core::fleetd::integration::application::landing;
use horch_core::fleetd::integration::application::rebase::{self, Effect, Rebased, Rebases};
use horch_core::fleetd::integration::domain::task_branch::LandingState;
use horch_core::fleetd::worker_profiles::adapters::launcher::Launcher;
use horch_core::fleetd::worker_profiles::application::ports::LaunchSpec;
use horch_core::fleetd::worktree_custody::adapters::git::CliWorktreeGit;
use horch_core::fleetd::worktree_custody::application::create::{self, Observed};
use horch_core::fleetd::worktree_custody::application::gate_tree::{
    self, GateTree, GateTreeRequest,
};
use horch_core::taskledger::model::Actor;
use horch_core::taskledger::runs::{self, NewRun, Run};
use horch_core::taskledger::schema;
use horch_core::taskledger::tasks::{self, NewTask};
use horch_core::taskledger::workers::Worker;
use rusqlite::{params, Connection};

const FEATURE: &str = "F-1";
const FEATURE_BRANCH: &str = "fleet/r1";

/// The folder of the rescue refs of [`FEATURE_BRANCH`].
const RESCUES: &str = "refs/horch/rescue/branch/fleet-r1/";

// ─── the world of a test ────────────────────────────────────────────────

struct World {
    _dir: tempfile::TempDir,
    conn: Connection,
    config: FleetConfig,
    run: Run,
    root: PathBuf,
    /// The repository, as its canonical path.
    repo: PathBuf,
    tasks: u32,
}

fn cause(text: &str) -> Cause<'_> {
    Cause {
        actor: Actor::Horch,
        text,
    }
}

/// A ledger of the current schema with the run `r1`, the feature
/// [`FEATURE`] on [`FEATURE_BRANCH`] and its branch row, the fetched tip of
/// `main`, and the gate tree of the feature. In git, `main` has 1 commit,
/// and the feature branch has 1 more commit of `f.txt`.
fn world() -> World {
    world_with(|_| {})
}

/// [`world`], and `setup` runs in the new repository before the gate tree
/// is made.
fn world_with(setup: impl FnOnce(&Path)) -> World {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let mut conn = Connection::open(root.join("ledger.sqlite3")).unwrap();
    conn.busy_timeout(Duration::from_secs(10)).unwrap();
    schema::migrate_to(&mut conn, schema::VERSION).unwrap();
    std::fs::create_dir_all(root.join("fleet")).unwrap();
    let repo = root.join("repo");
    std::fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    std::fs::write(repo.join("a.txt"), "a\n").unwrap();
    git(&repo, &["add", "a.txt"]);
    git(&repo, &["commit", "-q", "-m", "first"]);
    git(&repo, &["checkout", "-q", "-b", FEATURE_BRANCH]);
    std::fs::write(repo.join("f.txt"), "f\n").unwrap();
    git(&repo, &["add", "f.txt"]);
    git(&repo, &["commit", "-q", "-m", "feature"]);
    git(&repo, &["checkout", "-q", "main"]);
    // The worktree_add job writes only the tree folder: its parent exists.
    std::fs::create_dir(repo.join(".worktrees")).unwrap();
    setup(&repo);
    let tx = conn.transaction().unwrap();
    let new_run = NewRun {
        slug: "r1".into(),
        base_branch: Some("main".into()),
        run_branch: Some(FEATURE_BRANCH.into()),
        ..Default::default()
    };
    let run = runs::create(&tx, &repo, &new_run).unwrap();
    let feature = NewFeature {
        id: FEATURE,
        name: Some("r1"),
        branch: Some(FEATURE_BRANCH),
        primary_branch: "main",
    };
    rows::insert_feature(&tx, None, &feature, cause("the run starts")).unwrap();
    // FR-12: a task needs an idea in flight of the feature of its run.
    horch_core::taskledger::schema::run_idea(
        &tx,
        &run.run_id,
        &horch_core::taskledger::util::now(),
        true,
    )
    .unwrap();
    let branch = NewBranch {
        name: FEATURE_BRANCH,
        feature_id: FEATURE,
        task_id: None,
        owner: "fleetd",
        due_at: 0,
    };
    rows::insert_branch(&tx, None, &branch, cause("feature branch made")).unwrap();
    tx.commit().unwrap();
    let mut w = World {
        _dir: dir,
        conn,
        config: FleetConfig::default(),
        run,
        root,
        repo,
        tasks: 0,
    };
    let feature_tip = w.tip(FEATURE_BRANCH);
    w.fact(BranchFact::SeenTip(&feature_tip));
    let main = w.tip("main");
    w.fetched(&main);
    w.gate_tree();
    w
}

/// Run git in `dir` for the test itself, with no config of the user, and
/// give its output.
fn git_out(dir: &Path, args: &[&str]) -> Output {
    Command::new("git")
        .arg("-C")
        .arg(dir)
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@example.invalid",
            "-c",
            "commit.gpgSign=false",
        ])
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap()
}

/// [`git_out`], trimmed. A failed call fails the test.
fn git(dir: &Path, args: &[&str]) -> String {
    let out = git_out(dir, args);
    assert!(
        out.status.success(),
        "git {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// The sandbox profiles of a test: no task check runs in these tests.
struct NoProfiles;

impl SandboxProfiles for NoProfiles {
    fn check_launch(
        &self,
        _worker: &Worker,
        _dir: &Path,
        _check: &str,
    ) -> anyhow::Result<Option<LaunchSpec>> {
        Ok(None)
    }
}

impl World {
    fn tip(&self, rev: &str) -> String {
        git(&self.repo, &["rev-parse", rev])
    }

    /// Commit `file` with `text` on `branch`, and give the new tip.
    fn commit_on(&self, branch: &str, file: &str, text: &str) -> String {
        let exists = git_out(&self.repo, &["rev-parse", "--verify", "-q", branch])
            .status
            .success();
        if exists {
            git(&self.repo, &["checkout", "-q", branch]);
        } else {
            git(
                &self.repo,
                &["checkout", "-q", "-b", branch, FEATURE_BRANCH],
            );
        }
        std::fs::write(self.repo.join(file), text).unwrap();
        git(&self.repo, &["add", file]);
        git(
            &self.repo,
            &["commit", "-q", "-m", &format!("{branch} {file}")],
        );
        let tip = self.tip(branch);
        git(&self.repo, &["checkout", "-q", "main"]);
        tip
    }

    /// A new commit of `file` on `main`, which a fetch then sees: the
    /// fetched primary tip moves. Gives the new tip.
    fn move_primary(&self, file: &str, text: &str) -> String {
        let tip = self.commit_on("main", file, text);
        self.fetched(&tip);
        tip
    }

    /// The fetch saw `tip` as the tip of `main`.
    fn fetched(&self, tip: &str) {
        let repo = self.repo.to_string_lossy();
        let tx = self.conn.unchecked_transaction().unwrap();
        rows::put_remote_tip(&tx, None, &repo, "main", tip).unwrap();
        tx.commit().unwrap();
    }

    fn fact(&mut self, fact: BranchFact<'_>) {
        let tx = self.conn.transaction().unwrap();
        rows::set_branch_fact(&tx, None, FEATURE_BRANCH, fact, cause("test")).unwrap();
        tx.commit().unwrap();
    }

    /// Each rescue ref of the feature branch, as (name, tip).
    fn rescues(&self) -> Vec<(String, String)> {
        let out = git(
            &self.repo,
            &["for-each-ref", "--format=%(refname) %(objectname)", RESCUES],
        );
        out.lines()
            .filter_map(|l| l.split_once(' '))
            .map(|(n, t)| (n.to_string(), t.to_string()))
            .collect()
    }

    fn fleet_git(&self) -> CliWorktreeGit {
        CliWorktreeGit::new("git", self.root.join("fleet"))
            .reading(std::slice::from_ref(&self.repo))
            .writing(&[self.repo.join(".git")])
    }

    /// The gate tree of [`FEATURE`], made by its `worktree_add` job and
    /// observed, as a pass observes it.
    fn gate_tree(&mut self) {
        let repo = self.repo.clone();
        let tip = self.tip(FEATURE_BRANCH);
        let run_id = self.run.run_id.clone();
        let req = GateTreeRequest {
            feature_id: FEATURE,
            run_id: &run_id,
            repo: &repo,
            base: FEATURE_BRANCH,
            tip: &tip,
        };
        let config = self.config.clone();
        let tx = self.conn.transaction().unwrap();
        let reserved = gate_tree::reserve(&ctx(&config), &tx, &req, FleetSeconds(0));
        tx.commit().unwrap();
        let job = match reserved {
            Ok(GateTree::Reserved { job, .. }) => job,
            other => panic!("expected a reserved gate tree, got {other:?}"),
        };
        let ended = self.run_job(job);
        assert_eq!(ended.state, State::Passed, "{:?}", outcome(&ended));
        let tx = self.conn.transaction().unwrap();
        let seen = create::observe(&ctx(&config), &tx, &ended).unwrap();
        tx.commit().unwrap();
        assert!(matches!(seen, Observed::Created(_)), "{seen:?}");
    }

    fn gate_path(&self) -> PathBuf {
        gate_tree::path(&self.repo, FEATURE)
    }

    /// 1 pass of the rebase rule at the fleet clock `clock`, committed.
    /// Gives its effects.
    fn pass(&mut self, clock: i64) -> Vec<Effect> {
        let config = self.config.clone();
        let gw = CliGitGateway::new(self.fleet_git());
        let refs = GitRebaseRefs::new(self.fleet_git());
        let tx = self.conn.transaction().unwrap();
        let ctx = ctx(&config);
        let r = Rebases {
            jobs: &ctx,
            git: &gw,
            refs: &refs,
        };
        let effects = rebase::follow(&r, &tx, FleetSeconds(clock)).unwrap();
        tx.commit().unwrap();
        effects
    }

    /// Run each effect of a pass after its commit, and observe each result
    /// in 1 more transaction. `commit` false: the observation is lost, as
    /// in a crash after the effect.
    fn effects(&mut self, effects: &[Effect], commit: bool) {
        let refs = GitRebaseRefs::new(self.fleet_git());
        let config = self.config.clone();
        for effect in effects {
            let done = rebase::run(effect, &refs).unwrap();
            let tx = self.conn.transaction().unwrap();
            rebase::observe(&ctx(&config), &tx, &done).unwrap();
            if commit {
                tx.commit().unwrap();
            }
        }
    }

    /// Passes until the rule queues the rebase: the rescue pass, its
    /// effect, then the queue pass. Gives the queued run.
    fn queue_rebase(&mut self, clock: i64) -> JobRow {
        let effects = self.pass(clock);
        self.effects(&effects, true);
        assert!(
            self.pass(clock).is_empty(),
            "the second pass writes no rescue"
        );
        self.active_rebase().expect("a rebase run is queued")
    }

    fn active_rebase(&self) -> Option<JobRow> {
        jobs_store::active(&self.conn, Kind::Rebase, FEATURE_BRANCH).unwrap()
    }

    fn rebase_runs(&self) -> i64 {
        self.conn
            .query_row("SELECT COUNT(*) FROM jobs WHERE kind = 'rebase'", [], |r| {
                r.get(0)
            })
            .unwrap()
    }

    fn job(&self, id: i64) -> JobRow {
        jobs_store::get(&self.conn, id).unwrap().unwrap()
    }

    /// Run the job `id` as `horch job run` runs it, and end it from its
    /// result file, as a pass ends it. Gives the ended row.
    fn run_job(&mut self, id: i64) -> JobRow {
        let spec = LaunchSpec {
            program: "true".into(),
            args: Vec::new(),
            cwd: None,
            env_rule: Default::default(),
            detach: false,
            output: None,
        };
        let job = self.job(id);
        let config = self.config.clone();
        let ctx = JobCtx {
            guard: None,
            config: &config,
            needs_you: &NeedsYouList,
        };
        let mut runner = JobRunner::new(&Launcher);
        assert!(runner
            .start(&ctx, &mut self.conn, &job, &spec, FleetSeconds(0))
            .unwrap());
        let pid = self.job(id).pid.unwrap();
        let checks = CheckRunner {
            host: &Launcher,
            profiles: &NoProfiles,
            sweep: None,
        };
        let written = jobs::run_job(&ctx, &mut self.conn, id, pid, &checks).unwrap();
        assert!(written.is_some(), "run_job wrote the result of job {id}");
        jobs::end_from_files(&ctx, &mut self.conn).unwrap();
        self.job(id)
    }

    /// The process of the run `id` was lost: the pass ends it `abandoned`.
    fn lose(&self, id: i64) {
        self.conn
            .execute(
                "UPDATE jobs SET state = 'abandoned', abandoned_runs = abandoned_runs + 1
                 WHERE id = ?1",
                [id],
            )
            .unwrap();
    }

    fn set_bundle_state(&self, id: i64, state: &str) {
        self.conn
            .execute(
                "UPDATE bundles SET state = ?2 WHERE id = ?1",
                params![id, state],
            )
            .unwrap();
    }

    fn bundle_state(&self, id: i64) -> String {
        rows::bundle(&self.conn, id).unwrap().unwrap().state
    }

    /// A task, its git branch `name` with 1 commit of `file`, and its task
    /// branch row moved to `ready`, as the landing moves it.
    fn ready_branch(&mut self, name: &str, file: &str) {
        self.commit_on(name, file, &format!("{name}\n"));
        self.tasks += 1;
        let n = self.tasks;
        let new = NewTask::work(&format!("t{n}"), &format!("t{n}.md"), "dev");
        let run = self.run.clone();
        let config = self.config.clone();
        let tx = self.conn.transaction().unwrap();
        let task = tasks::add(&tx, &run, &new, Actor::Orchestrator)
            .unwrap()
            .task_id;
        let row = NewBranch {
            name,
            feature_id: FEATURE,
            task_id: Some(&task),
            owner: "worker",
            due_at: 0,
        };
        rows::insert_branch(&tx, None, &row, cause("task bound")).unwrap();
        let ctx = ctx(&config);
        for to in [LandingState::Verifying, LandingState::Ready] {
            landing::enter(&ctx, &tx, name, to, FleetSeconds(0), "its task is done").unwrap();
        }
        tx.commit().unwrap();
    }

    /// 1 pass of bundles at `clock`, committed.
    fn bundles_pass(&mut self, clock: i64) {
        let config = self.config.clone();
        let gw = CliGitGateway::new(self.fleet_git());
        let tx = self.conn.transaction().unwrap();
        let ctx = ctx(&config);
        let b = Bundles {
            jobs: &ctx,
            git: &gw,
        };
        bundles::advance(&b, &tx, FleetSeconds(clock)).unwrap();
        tx.commit().unwrap();
    }

    /// The 1 bundle of the feature.
    fn the_bundle(&self) -> i64 {
        self.conn
            .query_row("SELECT id FROM bundles", [], |r| r.get(0))
            .unwrap()
    }

    fn landing_state(&self, name: &str) -> Option<String> {
        rows::branch(&self.conn, name)
            .unwrap()
            .unwrap()
            .landing_state
    }

    fn seen_tip(&self) -> Option<String> {
        rows::branch(&self.conn, FEATURE_BRANCH)
            .unwrap()
            .unwrap()
            .seen_tip
    }
}

fn ctx(config: &FleetConfig) -> JobCtx<'_> {
    JobCtx {
        guard: None,
        config,
        needs_you: &NeedsYouList,
    }
}

fn rebased(row: &JobRow) -> Option<Rebased> {
    serde_json::from_value(result_data(row)?).ok()
}

fn is_ancestor(repo: &Path, a: &str, b: &str) -> bool {
    git_out(repo, &["merge-base", "--is-ancestor", a, b])
        .status
        .success()
}

// ─── the tests ──────────────────────────────────────────────────────────

#[test]
fn moved_primary_tip_queues_1_rebase_after_a_rescue_ref() {
    let mut w = world();
    let old = w.tip(FEATURE_BRANCH);
    // The feature contains the fetched primary tip: nothing to do.
    assert!(w.pass(1).is_empty());
    assert_eq!(w.rebase_runs(), 0);

    let primary = w.move_primary("p.txt", "p\n");
    let effects = w.pass(2);
    assert_eq!(effects.len(), 1, "{effects:?}");
    assert_eq!(w.rebase_runs(), 0, "no rebase before the rescue ref");
    assert!(w.rescues().is_empty(), "the pass writes no ref itself");

    w.effects(&effects, true);
    assert_eq!(w.rescues().len(), 1);
    assert_eq!(w.rescues()[0].1, old, "the rescue ref names the old tip");

    assert!(w.pass(3).is_empty());
    let job = w.active_rebase().expect("1 rebase run");
    assert_eq!(job.input_key, rebase::rebase_key(&old, &primary));
    assert_eq!(job.repo.as_deref(), Some(w.repo.to_str().unwrap()));
    assert!(w.pass(4).is_empty());
    assert_eq!(w.rebase_runs(), 1, "a later pass queues no second run");
    assert_eq!(w.rescues().len(), 1);
    assert_eq!(w.tip(FEATURE_BRANCH), old, "the pass moves no branch");
}

#[test]
fn no_rebase_while_a_bundle_is_gating_or_passed() {
    let mut w = world();
    w.ready_branch("fleet/t1", "t1.txt");
    w.bundles_pass(1);
    let bundle = w.the_bundle();
    w.move_primary("p.txt", "p\n");
    for state in ["gating", "passed"] {
        w.set_bundle_state(bundle, state);
        assert!(w.pass(2).is_empty(), "no rescue while a bundle is {state}");
        assert_eq!(w.rebase_runs(), 0, "no rebase while a bundle is {state}");
    }
    w.set_bundle_state(bundle, "dropped");
    let effects = w.pass(3);
    assert_eq!(effects.len(), 1, "the rule goes on after the bundle ends");
    w.effects(&effects, true);
    w.pass(4);
    assert_eq!(w.rebase_runs(), 1);
}

#[test]
fn clean_rebase_moves_the_ref_by_compare_and_set_and_drops_a_building_bundle() {
    let mut w = world();
    let old = w.tip(FEATURE_BRANCH);
    w.ready_branch("fleet/t1", "t1.txt");
    w.bundles_pass(1);
    let bundle = w.the_bundle();
    assert_eq!(w.bundle_state(bundle), "building");
    let primary = w.move_primary("p.txt", "p\n");

    let job = w.queue_rebase(2);
    let ended = w.run_job(job.id);
    assert_eq!(ended.state, State::Passed, "{:?}", outcome(&ended));
    let new = w.tip(FEATURE_BRANCH);
    assert_ne!(new, old);
    assert_eq!(rebased(&ended), Some(Rebased::Moved { tip: new.clone() }));
    assert!(
        is_ancestor(&w.repo, &primary, &new),
        "the branch is on the primary tip"
    );
    assert_eq!(
        w.tip(&format!("{new}~1")),
        primary,
        "1 commit on the primary tip"
    );
    assert_eq!(w.rescues()[0].1, old, "the old tip stays in its rescue ref");

    // The next pass records the new tip and queues the push.
    assert!(w.pass(3).is_empty());
    assert_eq!(w.seen_tip().as_deref(), Some(new.as_str()));
    let push = jobs_store::active(&w.conn, Kind::Push, FEATURE).unwrap();
    assert!(
        push.is_some_and(|p| p.input_key.starts_with(&new)),
        "a push is queued"
    );
    assert_eq!(w.rebase_runs(), 1, "the rebased feature needs no rebase");

    // The moved tip drops the building bundle, and its member is ready.
    w.bundles_pass(4);
    assert_eq!(w.bundle_state(bundle), "dropped");
    assert_eq!(w.landing_state("fleet/t1").as_deref(), Some("ready"));
}

#[test]
fn a_tip_that_moved_after_the_queue_is_not_overwritten() {
    let mut w = world();
    w.move_primary("p.txt", "p\n");
    let job = w.queue_rebase(2);
    // A push of the operator moves the branch before the job runs.
    let theirs = w.commit_on(FEATURE_BRANCH, "o.txt", "o\n");
    let ended = w.run_job(job.id);
    assert_eq!(ended.state, State::Failed, "{:?}", outcome(&ended));
    assert_eq!(
        w.tip(FEATURE_BRANCH),
        theirs,
        "the compare-and-set moves nothing"
    );
    // The tips moved, so the rule queues a run for the new tips.
    let effects = w.pass(3);
    w.effects(&effects, true);
    w.pass(4);
    let next = w.active_rebase().expect("a run for the new tip");
    assert!(next.input_key.starts_with(&theirs), "{}", next.input_key);
}

#[test]
fn crash_before_and_after_the_rescue_loses_no_tip() {
    let mut w = world();
    let old = w.tip(FEATURE_BRANCH);
    w.move_primary("p.txt", "p\n");

    // A crash after the rescue ref and before the commit of its row.
    let effects = w.pass(2);
    w.effects(&effects, false);
    assert_eq!(w.rescues().len(), 1);
    assert_eq!(w.rebase_runs(), 0);
    // The next pass writes the rescue again: the same ref, then 1 run.
    let effects = w.pass(3);
    assert_eq!(effects.len(), 1);
    w.effects(&effects, true);
    assert_eq!(
        w.rescues().len(),
        1,
        "the repeated rescue writes no new ref"
    );
    w.pass(4);
    let first = w.active_rebase().expect("1 rebase run");

    // A crash after the commit of the run and before its effect.
    w.lose(first.id);
    assert_eq!(w.tip(FEATURE_BRANCH), old);
    w.pass(10_000);
    let second = w.active_rebase().expect("a new run of the same key");
    assert_eq!(second.input_key, first.input_key);
    assert_eq!(second.run, 2);

    // A crash after the compare-and-set and before the result.
    let lost = exec_rebase::run(&w.conn, &w.job(second.id)).unwrap();
    assert!(lost.outcome.passed, "{:?}", lost.outcome);
    let new = w.tip(FEATURE_BRANCH);
    assert_ne!(new, old);
    w.lose(second.id);
    w.pass(20_000);
    let third = w.active_rebase().expect("a new run of the same key");
    let ended = w.run_job(third.id);
    assert_eq!(ended.state, State::Passed, "{:?}", outcome(&ended));
    assert_eq!(rebased(&ended), Some(Rebased::Moved { tip: new.clone() }));
    assert_eq!(w.tip(FEATURE_BRANCH), new, "the branch is rebased 1 time");
    assert_eq!(w.rescues(), vec![(format!("{RESCUES}1"), old)]);
    w.pass(20_001);
    assert_eq!(w.seen_tip().as_deref(), Some(new.as_str()));
    assert_eq!(w.rebase_runs(), 3);
}

#[test]
fn conflict_leaves_the_branch_at_its_old_tip() {
    let mut w = world();
    let old = w.commit_on(FEATURE_BRANCH, "a.txt", "feature\n");
    w.fact(BranchFact::SeenTip(&old));
    w.move_primary("a.txt", "primary\n");
    let job = w.queue_rebase(2);
    let ended = w.run_job(job.id);
    assert_eq!(ended.state, State::Failed, "{:?}", outcome(&ended));
    assert!(outcome(&ended).unwrap().verdict, "a conflict is a result");
    assert_eq!(
        rebased(&ended),
        Some(Rebased::Conflict {
            files: vec!["a.txt".to_string()]
        })
    );
    assert_eq!(w.tip(FEATURE_BRANCH), old);
    let tree = w.gate_path();
    let state = git(&tree, &["status", "--porcelain"]);
    assert!(state.is_empty(), "the gate tree is clean: {state}");
    assert!(
        !git_out(&tree, &["rev-parse", "--verify", "-q", "REBASE_HEAD"])
            .status
            .success(),
        "no rebase is in progress in the gate tree"
    );
    // The conflict is a result: the same tips queue no new run.
    assert!(w.pass(3).is_empty());
    assert_eq!(w.rebase_runs(), 1);
}

#[test]
fn rebase_runs_no_hook_or_program_of_the_project() {
    let mut w = world_with(|repo| {
        let hooks = repo.join(".git").join("hooks");
        for hook in ["pre-rebase", "post-rewrite", "post-checkout"] {
            let marker = repo.join(".git").join(format!("{hook}.ran"));
            let path = hooks.join(hook);
            let script = format!("#!/bin/sh\ntouch '{}'\n", marker.display());
            std::fs::write(&path, script).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
    });
    // The gate tree was made with hooks off too.
    let git_dir = w.repo.join(".git");
    let marker = |hook: &str| git_dir.join(format!("{hook}.ran"));
    w.move_primary("p.txt", "p\n");
    for hook in ["pre-rebase", "post-rewrite", "post-checkout"] {
        let _ = std::fs::remove_file(marker(hook));
    }
    let job = w.queue_rebase(2);
    let ended = w.run_job(job.id);
    assert_eq!(ended.state, State::Passed, "{:?}", outcome(&ended));
    for hook in ["pre-rebase", "post-rewrite", "post-checkout"] {
        assert!(!marker(hook).exists(), "the {hook} hook ran");
    }

    // A merge driver in the repository config stops the job.
    let old = w.commit_on(FEATURE_BRANCH, "g.txt", "g\n");
    w.fact(BranchFact::SeenTip(&old));
    let primary = w.move_primary("q.txt", "q\n");
    let driver = w.repo.join(".git").join("driver.ran");
    git(
        &w.repo,
        &[
            "config",
            "merge.x.driver",
            &format!("touch '{}'", driver.display()),
        ],
    );
    let rescues = w.rescues();
    let key = rebase::rebase_key(&old, &primary);
    let new = jobs_store::NewRun {
        kind: Kind::Rebase,
        subject: FEATURE_BRANCH,
        repo: Some(w.repo.to_str().unwrap()),
        input_key: &key,
        run: 1,
        abandoned_runs: 0,
        not_before: None,
        time_limit_s: 600,
    };
    let id = jobs_store::insert(&w.conn, &new).unwrap();
    let ended = w.run_job(id);
    assert_eq!(ended.state, State::Failed, "{:?}", outcome(&ended));
    let text = outcome(&ended).unwrap().text;
    assert!(text.contains("merge.x.driver"), "{text}");
    assert_eq!(w.tip(FEATURE_BRANCH), old, "the job changes nothing");
    assert_eq!(w.rescues(), rescues, "the job writes no ref");
    assert!(!driver.exists());
}
