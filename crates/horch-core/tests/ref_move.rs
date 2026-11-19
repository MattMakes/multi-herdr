//! The ref move: a `passed` bundle moves the feature ref by compare-and-set
//! from its base tip to its candidate tip; the next pass observes the move
//! before the bundle and its members are `landed`; a failed compare-and-set
//! drops the bundle (docs/specs/fleetd/design.md, I8, I9, FM-37, F9, "The
//! bundle state machine", "A moved tip drops a bundle";
//! docs/tickets/fleetd-p4/31-ref-move-by-compare-and-set.md).
//!
//! Each test has its own temporary ledger, fleet root and git repository.
//! No test reads the load, the limits or the clock of the machine: the fleet
//! clock is a number that the test gives, and the config is the default.
//! The bundle starts in `passed`, as the bundle use cases leave it after a
//! pass of its fast gate.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use horch_core::fleetd::fleet_runtime::application::jobs::JobCtx;
use horch_core::fleetd::fleet_runtime::application::ledger::{transition, Cause};
use horch_core::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;
use horch_core::fleetd::fleet_runtime::domain::ledger::Entity;
use horch_core::fleetd::ideas::application::needs_you::{open_keys, NeedsYouList};
use horch_core::fleetd::integration::adapters::git::CliGitGateway;
use horch_core::fleetd::integration::adapters::sqlite::{
    self as rows, BranchFact, BundleFact, BundleRow, NewBranch, NewBundle, NewFeature,
};
use horch_core::fleetd::integration::application::landing;
use horch_core::fleetd::integration::application::ports::{
    Candidate, IntegrationGitGateway, Member,
};
use horch_core::fleetd::integration::application::ref_move::{
    self, overdue_key, Move, Ran, RefMoves, OVERDUE_KIND,
};
use horch_core::fleetd::integration::domain::task_branch::LandingState;
use horch_core::fleetd::worktree_custody::adapters::git::CliWorktreeGit;
use horch_core::taskledger::model::Actor;
use horch_core::taskledger::runs::{self, NewRun, Run};
use horch_core::taskledger::tasks::{self, NewTask};
use horch_core::taskledger::{schema, util};
use rusqlite::{params, Connection};

const FEATURE: &str = "F-0001";
const FEATURE_BRANCH: &str = "fleet/r1";
const FEATURE_REF: &str = "refs/heads/fleet/r1";
const A: &str = "fleet/r1-a";
const B: &str = "fleet/r1-b";

// ─── the world of a test ────────────────────────────────────────────────

struct World {
    _temp: tempfile::TempDir,
    root: PathBuf,
    repo: PathBuf,
    conn: Connection,
    run: Run,
    config: FleetConfig,
    /// The task id of each task branch, by branch name.
    tasks: Vec<(String, String)>,
}

fn cause(text: &str) -> Cause<'_> {
    Cause {
        actor: Actor::Horch,
        text,
    }
}

fn git_out(dir: &Path, args: &[&str]) -> Output {
    Command::new("git")
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
        .env("GIT_AUTHOR_DATE", "1700000000 +0000")
        .env("GIT_COMMITTER_DATE", "1700000000 +0000")
        .output()
        .unwrap()
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = git_out(dir, args);
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn world() -> World {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let repo = root.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    std::fs::write(repo.join("a.txt"), "a\n").unwrap();
    git(&repo, &["add", "a.txt"]);
    git(&repo, &["commit", "-q", "-m", "base"]);
    git(&repo, &["branch", FEATURE_BRANCH]);
    let base = git(&repo, &["rev-parse", FEATURE_BRANCH]);
    let mut conn = Connection::open(root.join("ledger.sqlite3")).unwrap();
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
        run_branch: Some(FEATURE_BRANCH.into()),
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
    let tx = conn.transaction().unwrap();
    let new = NewFeature {
        id: FEATURE,
        name: Some("r1"),
        branch: Some(FEATURE_BRANCH),
        primary_branch: "main",
    };
    rows::insert_feature(&tx, None, &new, cause("feature accepted")).unwrap();
    let row = NewBranch {
        name: FEATURE_BRANCH,
        feature_id: FEATURE,
        task_id: None,
        owner: "fleetd",
        due_at: 0,
    };
    rows::insert_branch(&tx, None, &row, cause("feature branch")).unwrap();
    rows::set_branch_fact(
        &tx,
        None,
        FEATURE_BRANCH,
        BranchFact::SeenTip(&base),
        cause("fleetd saw it"),
    )
    .unwrap();
    tx.commit().unwrap();
    World {
        _temp: temp,
        root,
        repo,
        conn,
        run,
        config: FleetConfig::default(),
        tasks: Vec::new(),
    }
}

impl World {
    fn tip(&self, rev: &str) -> String {
        git(&self.repo, &["rev-parse", rev])
    }

    /// Commit `file` on `branch`, made from the feature branch when it is
    /// new. Gives the new tip.
    fn commit_on(&self, branch: &str, file: &str) -> String {
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
        std::fs::write(self.repo.join(file), format!("{branch} {file}\n")).unwrap();
        git(&self.repo, &["add", file]);
        git(&self.repo, &["commit", "-q", "-m", file]);
        git(&self.repo, &["checkout", "-q", "main"]);
        self.tip(branch)
    }

    fn gateway(&self) -> CliGitGateway {
        CliGitGateway::new(
            CliWorktreeGit::new("git", self.root.join("fleet")).writing(&[self.repo.join(".git")]),
        )
    }

    fn task_of(&self, branch: &str) -> String {
        let (_, task) = self.tasks.iter().find(|(b, _)| b == branch).unwrap();
        task.clone()
    }

    /// A task, its git branch `name` with 1 commit of `file`, and its task
    /// branch row moved to `ready`, as the landing moves it.
    fn ready_branch(&mut self, name: &str, file: &str) {
        self.commit_on(name, file);
        let n = self.tasks.len() + 1;
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
        self.tasks.push((name.to_string(), task));
    }

    /// 1 bundle of `members` on the feature tip, built and gated, in
    /// `passed` at the fleet clock `clock`, as the bundle use cases leave
    /// it. Gives its id.
    fn passed_bundle(&mut self, members: &[&str], clock: i64) -> i64 {
        let base = self.tip(FEATURE_BRANCH);
        let list: Vec<Member> = members
            .iter()
            .map(|m| Member {
                name: m.to_string(),
                tip: self.tip(m),
            })
            .collect();
        let candidate = match self
            .gateway()
            .build_candidate(&self.repo, &base, &list)
            .unwrap()
        {
            Candidate::Built { tip } => tip,
            other => panic!("the candidate does not build: {other:?}"),
        };
        let config = self.config.clone();
        let ctx = ctx(&config);
        let tx = self.conn.transaction().unwrap();
        let new = NewBundle {
            feature_id: FEATURE,
            base_tip: &base,
            parent_id: None,
            members,
            due_at: 0,
        };
        let id = rows::insert_bundle(&tx, None, &new, cause("ready branches")).unwrap();
        for m in members {
            landing::enter(
                &ctx,
                &tx,
                m,
                LandingState::InBundle,
                FleetSeconds(clock),
                "a member",
            )
            .unwrap();
        }
        let name = format!("refs/horch/bundle/{id}");
        for fact in [
            BundleFact::CandidateRef(Some(&name)),
            BundleFact::CandidateTip(&candidate),
        ] {
            rows::set_bundle_fact(&tx, None, id, fact, cause("built")).unwrap();
        }
        for to in ["gating", "passed"] {
            transition(
                &tx,
                None,
                Entity::Bundle,
                &id.to_string(),
                to,
                cause("the build and the fast gate passed"),
                &NeedsYouList,
            )
            .unwrap();
        }
        let due = clock + config.bundle_passed_limit_s as i64;
        rows::set_bundle_fact(&tx, None, id, BundleFact::DueAt(due), cause("passed")).unwrap();
        tx.commit().unwrap();
        id
    }

    /// 1 pass of the ref move at the fleet clock `clock`, committed. Gives
    /// the moves to run after the commit.
    fn pass(&mut self, clock: i64) -> Vec<Move> {
        let config = self.config.clone();
        let ctx = ctx(&config);
        let gateway = self.gateway();
        let r = RefMoves {
            jobs: &ctx,
            git: &gateway,
        };
        let tx = self.conn.transaction().unwrap();
        let moves = ref_move::advance(&r, &tx, FleetSeconds(clock)).unwrap();
        tx.commit().unwrap();
        moves
    }

    /// 1 pass whose process stops before the commit: its transaction rolls
    /// back. Gives the moves that it would run.
    fn pass_that_crashes_before_the_commit(&mut self, clock: i64) -> Vec<Move> {
        let config = self.config.clone();
        let ctx = ctx(&config);
        let gateway = self.gateway();
        let r = RefMoves {
            jobs: &ctx,
            git: &gateway,
        };
        let tx = self.conn.transaction().unwrap();
        let moves = ref_move::advance(&r, &tx, FleetSeconds(clock)).unwrap();
        drop(tx);
        moves
    }

    /// Run `mv` after the commit of its pass.
    fn run(&self, mv: &Move) -> Ran {
        ref_move::run(&self.conn, &self.gateway(), mv).unwrap()
    }

    fn bundle(&self, id: i64) -> BundleRow {
        rows::bundle(&self.conn, id).unwrap().unwrap()
    }

    fn landing_state(&self, branch: &str) -> String {
        rows::branch(&self.conn, branch)
            .unwrap()
            .unwrap()
            .landing_state
            .unwrap()
    }

    fn seen_tip(&self) -> Option<String> {
        rows::branch(&self.conn, FEATURE_BRANCH)
            .unwrap()
            .unwrap()
            .seen_tip
    }

    fn cancel(&self, branch: &str) {
        let task = tasks::get(&self.conn, &self.task_of(branch)).unwrap();
        tasks::cancel(&self.conn, &task, "the operator cancels it").unwrap();
    }

    /// The count of the transition events of the row `id` of `entity` into
    /// `to`.
    fn entered(&self, entity: &str, id: &str, to: &str) -> i64 {
        self.conn
            .query_row(
                "SELECT count(*) FROM events WHERE entity = ?1 AND entity_id = ?2
                 AND kind = 'transition' AND to_state = ?3",
                params![entity, id, to],
                |r| r.get(0),
            )
            .unwrap()
    }

    fn events(&self) -> i64 {
        self.conn
            .query_row("SELECT count(*) FROM events", [], |r| r.get(0))
            .unwrap()
    }
}

fn ctx(config: &FleetConfig) -> JobCtx<'_> {
    JobCtx {
        guard: None,
        config,
        needs_you: &NeedsYouList,
    }
}

/// A world with the ready task branches `a` and `b` in 1 `passed` bundle at
/// the fleet clock 10. Gives the world, the bundle id and the base tip.
fn passed_ab() -> (World, i64, String) {
    let mut w = world();
    w.ready_branch(A, "a1.txt");
    w.ready_branch(B, "b1.txt");
    let base = w.tip(FEATURE_BRANCH);
    let id = w.passed_bundle(&[A, B], 10);
    (w, id, base)
}

// ─── the tests of the ticket ────────────────────────────────────────────

#[test]
fn passed_bundle_moves_the_feature_ref_from_the_base_tip() {
    let (mut w, id, base) = passed_ab();
    let candidate = w.bundle(id).candidate_tip.unwrap();

    // Part 2 of the pass decides the move and moves nothing itself.
    let moves = w.pass(20);
    assert_eq!(moves.len(), 1, "moves: {moves:?}");
    let mv = &moves[0];
    assert_eq!(mv.bundle, id);
    assert_eq!(mv.repo, w.repo);
    assert_eq!(mv.name, FEATURE_REF);
    assert_eq!(mv.from, base, "the compare-and-set expects the base tip");
    assert_eq!(mv.to, candidate);
    assert_eq!(w.tip(FEATURE_BRANCH), base);
    assert_eq!(
        w.seen_tip().as_deref(),
        Some(base.as_str()),
        "seen_tip is the expected old tip"
    );

    // After the commit, the move is 1 compare-and-set from the base tip.
    assert_eq!(w.run(mv), Ran::Moved);
    assert_eq!(w.tip(FEATURE_BRANCH), candidate);
    let landed = w.gateway();
    for m in [A, B] {
        assert!(landed
            .changes_in(&w.repo, &w.tip(m), &base, &w.tip(FEATURE_BRANCH))
            .unwrap());
    }

    // The move is safe to repeat: the ref is no longer at the base tip.
    assert_eq!(
        w.run(mv),
        Ran::Mismatch {
            actual: Some(candidate.clone())
        }
    );
    assert_eq!(w.tip(FEATURE_BRANCH), candidate);
}

#[test]
fn move_is_observed_before_landed() {
    let (mut w, id, _) = passed_ab();
    let candidate = w.bundle(id).candidate_tip.unwrap();
    let moves = w.pass(20);
    assert_eq!(w.run(&moves[0]), Ran::Moved);

    // The effect alone lands nothing: the rows wait for the observation.
    assert_eq!(w.bundle(id).state, "passed");
    for m in [A, B] {
        assert_eq!(w.landing_state(m), "in_bundle");
    }

    // The next pass reads the ref at the candidate tip.
    let moves = w.pass(30);
    assert!(moves.is_empty(), "moves: {moves:?}");
    assert_eq!(w.bundle(id).state, "landed");
    for m in [A, B] {
        assert_eq!(w.landing_state(m), "landed");
    }
    assert_eq!(w.seen_tip().as_deref(), Some(candidate.as_str()));

    // `landed` is terminal and not checked again: a later move of the
    // feature tip changes nothing of the bundle or its members.
    w.commit_on(FEATURE_BRANCH, "later.txt");
    let before = w.events();
    assert!(w.pass(40).is_empty());
    assert_eq!(w.bundle(id).state, "landed");
    for m in [A, B] {
        assert_eq!(w.landing_state(m), "landed");
    }
    assert_eq!(w.events(), before);
}

#[test]
fn passed_bundle_whose_compare_and_set_fails_is_dropped_and_its_members_are_ready() {
    let (mut w, id, base) = passed_ab();
    let candidate = w.bundle(id).candidate_tip.unwrap();
    let moves = w.pass(20);
    assert_eq!(moves.len(), 1);

    // Someone commits on the feature branch between the pass and the move.
    let theirs = w.commit_on(FEATURE_BRANCH, "theirs.txt");
    assert_eq!(
        w.run(&moves[0]),
        Ran::Mismatch {
            actual: Some(theirs.clone())
        }
    );
    assert_eq!(w.tip(FEATURE_BRANCH), theirs, "the lost race moves nothing");
    assert_ne!(w.tip(FEATURE_BRANCH), candidate);

    // The next pass drops the bundle and records the new tip.
    let moves = w.pass(30);
    assert!(moves.is_empty(), "moves: {moves:?}");
    assert_eq!(w.bundle(id).state, "dropped");
    for m in [A, B] {
        assert_eq!(w.landing_state(m), "ready");
    }
    assert_eq!(w.seen_tip().as_deref(), Some(theirs.as_str()));
    assert_ne!(theirs, base);

    // I9: a gate result on a stale tip moves no ref. The tip moves before
    // the pass of a second `passed` bundle: no move is decided at all.
    let id2 = w.passed_bundle(&[A, B], 40);
    let other = w.commit_on(FEATURE_BRANCH, "other.txt");
    assert!(w.pass(50).is_empty());
    assert_eq!(w.tip(FEATURE_BRANCH), other);
    assert_eq!(w.bundle(id2).state, "dropped");
    for m in [A, B] {
        assert_eq!(w.landing_state(m), "ready");
    }
    assert_eq!(w.seen_tip().as_deref(), Some(other.as_str()));
}

#[test]
fn cancelled_member_task_moves_no_ref() {
    // F9 after the commit: the task of a member is cancelled between the
    // pass and the move. The move checks once more and moves nothing.
    let (mut w, id, base) = passed_ab();
    let moves = w.pass(20);
    assert_eq!(moves.len(), 1);
    w.cancel(B);
    assert_eq!(w.run(&moves[0]), Ran::Cancelled { task: w.task_of(B) });
    assert_eq!(w.tip(FEATURE_BRANCH), base);

    // The next pass drops the bundle: no move, and the member of the live
    // task goes back to `ready`.
    let moves = w.pass(30);
    assert!(moves.is_empty(), "moves: {moves:?}");
    assert_eq!(w.tip(FEATURE_BRANCH), base);
    assert_eq!(w.bundle(id).state, "dropped");
    assert_eq!(w.landing_state(A), "ready");

    // A task cancelled before the pass: the pass decides no move.
    let mut w = world();
    w.ready_branch(A, "a1.txt");
    w.ready_branch(B, "b1.txt");
    let base = w.tip(FEATURE_BRANCH);
    let id = w.passed_bundle(&[A, B], 10);
    w.cancel(A);
    assert!(w.pass(20).is_empty());
    assert_eq!(w.tip(FEATURE_BRANCH), base);
    assert_eq!(w.bundle(id).state, "dropped");
    assert_eq!(w.landing_state(B), "ready");
}

#[test]
fn crash_after_the_move_before_the_observation_lands_once() {
    let (mut w, id, _) = passed_ab();
    let candidate = w.bundle(id).candidate_tip.unwrap();
    let moves = w.pass(20);
    assert_eq!(w.run(&moves[0]), Ran::Moved);

    // fleetd stops after the move and before the commit of the
    // observation: the rows stay as they were.
    assert!(w.pass_that_crashes_before_the_commit(30).is_empty());
    assert_eq!(w.bundle(id).state, "passed");
    for m in [A, B] {
        assert_eq!(w.landing_state(m), "in_bundle");
    }

    // The next pass reads the ref again and lands the bundle 1 time.
    assert!(w.pass(40).is_empty());
    assert!(w.pass(50).is_empty());
    assert_eq!(w.tip(FEATURE_BRANCH), candidate);
    assert_eq!(w.bundle(id).state, "landed");
    assert_eq!(w.entered("bundle", &id.to_string(), "landed"), 1);
    for m in [A, B] {
        assert_eq!(w.landing_state(m), "landed");
        assert_eq!(w.entered("task_branch", m, "landed"), 1);
    }
}

#[test]
fn crash_before_the_commit_or_before_the_move_moves_the_ref_1_time() {
    // A fault before the commit of the pass: no move runs, and the rows
    // stay. The next pass decides the same move.
    let (mut w, id, base) = passed_ab();
    let candidate = w.bundle(id).candidate_tip.unwrap();
    let lost = w.pass_that_crashes_before_the_commit(20);
    assert_eq!(lost.len(), 1);
    assert_eq!(w.tip(FEATURE_BRANCH), base);
    assert_eq!(w.bundle(id).state, "passed");

    // A fault after the commit and before the move: the next pass decides
    // the same move again, from the same base tip.
    let first = w.pass(30);
    assert_eq!(first, lost);
    assert_eq!(w.tip(FEATURE_BRANCH), base);
    let again = w.pass(40);
    assert_eq!(again, first);

    // The move runs 1 time, and the bundle lands 1 time.
    assert_eq!(w.run(&again[0]), Ran::Moved);
    assert!(w.pass(50).is_empty());
    assert_eq!(w.tip(FEATURE_BRANCH), candidate);
    assert_eq!(w.bundle(id).state, "landed");
    assert_eq!(w.entered("bundle", &id.to_string(), "landed"), 1);
    for m in [A, B] {
        assert_eq!(w.entered("task_branch", m, "landed"), 1);
    }
}

#[test]
fn member_whose_changes_are_not_in_the_feature_by_content_is_not_landed() {
    // I8: a member is `landed` only when its changes are in the feature
    // branch by content. A member that has a new commit after the build is
    // not, and it goes back to `ready`.
    let (mut w, id, _) = passed_ab();
    let moves = w.pass(20);
    w.commit_on(B, "b2.txt");
    assert_eq!(w.run(&moves[0]), Ran::Moved);
    assert!(w.pass(30).is_empty());
    assert_eq!(w.bundle(id).state, "landed");
    assert_eq!(w.landing_state(A), "landed");
    assert_eq!(w.landing_state(B), "ready");
}

#[test]
fn passed_bundle_past_its_limit_has_1_needs_you_item() {
    let (mut w, id, _) = passed_ab();
    let limit = w.config.bundle_passed_limit_s as i64;
    let key = overdue_key(id);
    let items = |w: &World| open_keys(&w.conn, OVERDUE_KIND).unwrap();

    // The move is decided but never runs, so the bundle stays `passed`.
    w.pass(10 + limit - 1);
    assert!(items(&w).is_empty());
    w.pass(10 + limit);
    assert_eq!(items(&w), std::slice::from_ref(&key));
    w.pass(10 + limit + 60);
    assert_eq!(
        items(&w),
        std::slice::from_ref(&key),
        "1 item, not 1 for each pass"
    );

    // The bundle leaves `passed`: the item ends.
    let moves = w.pass(10 + limit + 70);
    assert_eq!(w.run(&moves[0]), Ran::Moved);
    w.pass(10 + limit + 80);
    assert_eq!(w.bundle(id).state, "landed");
    assert!(items(&w).is_empty());
}
