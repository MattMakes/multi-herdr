//! A merge with ideas in flight: fleetd records the feature as `merged` and
//! moves each idea in flight to a new feature, with a rescue ref and a
//! rebase of each task branch (docs/specs/fleetd/design.md, "Features" (A
//! merge with ideas in flight); I7, I10, I13; FM-41;
//! docs/tickets/fleetd-p6/35-merged-with-ideas.md).
//!
//! Each test has its own temporary ledger and git repository. No test reads
//! the load, the limits or the clock of the machine: the fleet clock is a
//! number that the test gives, and the config is the default. The rebase
//! job does not run here: the test reads its row. The task, idea and
//! landing state machines do not run here: the test writes their states.

use std::path::{Path, PathBuf};
use std::process::Command;

use horch_core::fleetd::fleet_runtime::application::jobs::JobCtx;
use horch_core::fleetd::fleet_runtime::application::ledger::Cause;
use horch_core::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;
use horch_core::fleetd::ideas::adapters::ideas_store::{self, NewIdea};
use horch_core::fleetd::ideas::application::needs_you::NeedsYouList;
use horch_core::fleetd::integration::adapters::git::CliGitGateway;
use horch_core::fleetd::integration::adapters::sqlite::{self as rows, NewBundle, NewFeature};
use horch_core::fleetd::integration::application::bundles::{self, Bundles};
use horch_core::fleetd::integration::application::landing::{self, GitBranchRescues};
use horch_core::fleetd::integration::application::merged_with_ideas::{
    self as mwi, MergedWithIdeas, Moved,
};
use horch_core::fleetd::integration::application::new_features as nf;
use horch_core::fleetd::worktree_custody::adapters::git::CliWorktreeGit;
use horch_core::taskledger::model::Actor;
use horch_core::taskledger::runs::{self, NewRun, Run};
use horch_core::taskledger::tasks::{self, NewTask};
use horch_core::taskledger::{util, Store};
use rusqlite::{params, Connection};

const OLD: &str = "F-0001";
const RUN_BRANCH: &str = "fleet/r1";
/// The idea that the first task of the run gets (`schema::run_idea`).
const SETTLED: &str = "I-0001";
/// The idea in flight when you merge the pull request.
const MOVED: &str = "I-0002";
/// The new feature: the internal run of the old feature has rowid 1.
const NEW: &str = "F-0002";
const NEW_BRANCH: &str = "fleet/r1-2";

struct World {
    _temp: tempfile::TempDir,
    repo: PathBuf,
    fleet: PathBuf,
    store: Store,
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

fn worktree_git(fleet: &Path, repo: &Path) -> CliWorktreeGit {
    CliWorktreeGit::new("git", fleet).writing(&[repo.join(".git")])
}

/// A repository with `main` and the run branch at the same commit, a temp
/// ledger with the fleet clock at 0, the run and its feature `F-0001` in
/// `open`, and the fetched tip of `main`.
fn world() -> World {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().canonicalize().unwrap();
    let repo = dir.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "--quiet", "--initial-branch=main"]);
    std::fs::write(repo.join("a.txt"), "a\n").unwrap();
    git(&repo, &["add", "a.txt"]);
    git(&repo, &["commit", "--quiet", "-m", "base"]);
    git(&repo, &["branch", RUN_BRANCH]);
    let db = dir.join("ledger.sqlite3");
    let store = Store::open(&db).unwrap();
    let mut conn = Connection::open(&db).unwrap();
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
        id: OLD,
        name: Some("r1"),
        branch: Some(RUN_BRANCH),
        primary_branch: "main",
    };
    let tx = conn.transaction().unwrap();
    rows::insert_feature(&tx, None, &new, cause("feature accepted")).unwrap();
    tx.commit().unwrap();
    let fleet = dir.join("fleet");
    let git = CliGitGateway::new(worktree_git(&fleet, &repo));
    let w = World {
        _temp: temp,
        repo,
        fleet,
        store,
        conn,
        run,
        config: FleetConfig::default(),
        git,
    };
    w.fetched_main();
    w
}

/// The world when you merge the pull request of `F-0001`: the idea
/// `I-0001` is `integrating` with its task branch `landed`, and the idea
/// `I-0002` is `in_progress` with the task branch of `two` in `building`.
/// Gives (task, branch) of `two`.
fn merged_with_an_idea_in_flight() -> (World, String, String) {
    let w = world();
    let (one, one_branch) = w.task("one", SETTLED);
    w.idea_state(SETTLED, "integrating");
    w.commit_on(RUN_BRANCH, "one.txt", "one\n");
    w.settle(&one, "done", &one_branch, "landed");
    w.idea(MOVED, "in_progress");
    let (two, two_branch) = w.task("two", MOVED);
    w.merge();
    (w, two, two_branch)
}

impl World {
    fn ctx(&self) -> JobCtx<'_> {
        JobCtx {
            guard: None,
            config: &self.config,
            needs_you: &NeedsYouList,
        }
    }

    /// 1 pass of `MoveIdeasToNewFeature`, committed.
    fn move_ideas(&self) -> Vec<Moved> {
        let ctx = self.ctx();
        let m = MergedWithIdeas {
            jobs: &ctx,
            git: &self.git,
        };
        let tx = self.conn.unchecked_transaction().unwrap();
        let moved = mwi::move_ideas(&m, &tx, FleetSeconds(0)).unwrap();
        tx.commit().unwrap();
        moved
    }

    /// The effects after the commit: the ref of each new feature branch,
    /// then the rescue and the rebase of each moved task branch.
    fn effects(&mut self) -> Vec<String> {
        for r in nf::pending_refs(&self.conn, &self.git).unwrap() {
            nf::create_ref(&self.git, &r).unwrap();
        }
        self.complete()
    }

    /// The rescue and the rebase of each moved task branch.
    fn complete(&mut self) -> Vec<String> {
        let wgit = worktree_git(&self.fleet, &self.repo);
        let rescues = GitBranchRescues { git: &wgit };
        let ctx = JobCtx {
            guard: None,
            config: &self.config,
            needs_you: &NeedsYouList,
        };
        mwi::complete_moves(&ctx, &mut self.store, &self.git, &rescues, FleetSeconds(0)).unwrap()
    }

    fn feature_state(&self, id: &str) -> String {
        rows::feature(&self.conn, id).unwrap().unwrap().state
    }

    fn feature_count(&self) -> i64 {
        self.conn
            .query_row("SELECT count(*) FROM features", [], |r| r.get(0))
            .unwrap()
    }

    fn feature_of_idea(&self, idea: &str) -> String {
        ideas_store::idea(&self.conn, idea)
            .unwrap()
            .unwrap()
            .feature_id
    }

    fn feature_of_branch(&self, branch: &str) -> String {
        rows::branch(&self.conn, branch)
            .unwrap()
            .unwrap()
            .feature_id
    }

    fn landing_state(&self, branch: &str) -> String {
        rows::branch(&self.conn, branch)
            .unwrap()
            .unwrap()
            .landing_state
            .unwrap()
    }

    fn run_of_task(&self, task: &str) -> String {
        tasks::get(&self.conn, task).unwrap().run_id
    }

    fn tip(&self, rev: &str) -> String {
        git(&self.repo, &["rev-parse", rev])
    }

    fn has_ref(&self, name: &str) -> bool {
        !git(&self.repo, &["for-each-ref", "--format=%(refname)", name]).is_empty()
    }

    /// Each rescue ref, as (ref, tip).
    fn rescue_refs(&self) -> Vec<(String, String)> {
        let out = git(
            &self.repo,
            &[
                "for-each-ref",
                "--format=%(refname) %(objectname)",
                "refs/horch/rescue/",
            ],
        );
        out.lines()
            .map(|l| {
                let (name, tip) = l.split_once(' ').unwrap();
                (name.to_string(), tip.to_string())
            })
            .collect()
    }

    /// Each `rebase` job, as (subject, input key).
    fn rebase_jobs(&self) -> Vec<(String, String)> {
        let mut stmt = self
            .conn
            .prepare("SELECT subject, input_key FROM jobs WHERE kind = 'rebase' ORDER BY id")
            .unwrap();
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        rows.collect::<rusqlite::Result<_>>().unwrap()
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

    /// You merge the pull request of the run branch with a squash merge,
    /// and a fetch sees it.
    fn merge(&self) {
        git(&self.repo, &["merge", "--quiet", "--squash", RUN_BRANCH]);
        git(&self.repo, &["commit", "--quiet", "-m", "squash"]);
        self.fetched_main();
    }

    /// A new idea `id` of `F-0001` in `state`.
    fn idea(&self, id: &str, state: &str) {
        let new = NewIdea {
            id,
            feature_id: OLD,
            priority: 0,
        };
        let tx = self.conn.unchecked_transaction().unwrap();
        ideas_store::insert_idea(&tx, None, &new, cause("idea added")).unwrap();
        tx.commit().unwrap();
        self.idea_state(id, state);
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

    /// A new task `title` of the idea `idea`, with its branch from the run
    /// branch and 1 commit in git, and its branch row in `building`. Gives
    /// (task, branch).
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
        git(&self.repo, &["branch", &branch, RUN_BRANCH]);
        self.commit_on(&branch, &format!("{title}-work.txt"), "work\n");
        let ctx = self.ctx();
        let tx = self.conn.unchecked_transaction().unwrap();
        assert!(landing::bind_branch(&ctx, &tx, &task, &branch, FleetSeconds(0)).unwrap());
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

    /// An open worker question of `task` on `F-0001`, owned by its
    /// sub-orchestrator.
    fn question(&self, id: &str, task: &str, state: &str) {
        self.conn
            .execute(
                "INSERT INTO questions (id, task_id, feature_id, owner, state, due_at, asked_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, 0, ?6)",
                params![
                    id,
                    task,
                    OLD,
                    format!("idea-lead:{OLD}"),
                    state,
                    util::now()
                ],
            )
            .unwrap();
    }

    /// The (feature, owner) of the question `id`.
    fn question_owner(&self, id: &str) -> (String, String) {
        self.conn
            .query_row(
                "SELECT feature_id, owner FROM questions WHERE id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap()
    }
}

#[test]
fn open_feature_merged_by_content_moves_ideas_in_flight_to_a_new_feature() {
    let (mut w, two, two_branch) = merged_with_an_idea_in_flight();
    w.question("q-open", &two, "open");
    let main_tip = w.tip("main");

    let moved = w.move_ideas();

    assert_eq!(moved.len(), 1);
    let m = &moved[0];
    assert_eq!((m.feature.as_str(), m.new_feature.as_str()), (OLD, NEW));
    assert_eq!(m.ideas, vec![MOVED.to_string()]);
    assert_eq!(m.branch_ref.name, format!("refs/heads/{NEW_BRANCH}"));
    assert_eq!(
        m.branch_ref.tip, main_tip,
        "the new branch starts from the fetched primary tip"
    );
    assert_eq!(w.feature_state(OLD), "merged");
    let new = rows::feature(&w.conn, NEW).unwrap().unwrap();
    assert_eq!(new.state, "open");
    assert_eq!(new.branch.as_deref(), Some(NEW_BRANCH));
    // The row of the new branch exists before the branch (I11).
    let row = rows::branch(&w.conn, NEW_BRANCH).unwrap().unwrap();
    assert_eq!(row.seen_tip.as_deref(), Some(main_tip.as_str()));
    assert!(!w.has_ref(&format!("refs/heads/{NEW_BRANCH}")));
    // The idea in flight moves; the settled idea stays.
    assert_eq!(w.feature_of_idea(MOVED), NEW);
    assert_eq!(w.feature_of_idea(SETTLED), OLD);
    // Its task belongs to the internal run of the new feature, so it starts
    // and lands on the new feature branch.
    let new_run = runs::feature_run(&w.conn, NEW).unwrap().unwrap();
    assert_eq!(new_run.run_branch.as_deref(), Some(NEW_BRANCH));
    assert_eq!(w.run_of_task(&two), new_run.run_id);
    // Its open question has the new owner (I13).
    assert_eq!(
        w.question_owner("q-open"),
        (NEW.to_string(), format!("idea-lead:{NEW}"))
    );

    w.effects();
    assert_eq!(w.tip(NEW_BRANCH), main_tip);
    assert_eq!(w.feature_of_branch(&two_branch), NEW);
}

#[test]
fn moved_idea_keeps_its_sub_orchestrator_and_pane() {
    let (w, _, _) = merged_with_an_idea_in_flight();
    w.conn
        .execute(
            "INSERT INTO workers (worker_id, run_id, kind, persona, host, status)
             VALUES ('lead', ?1, 'worker', 'idea-lead', 'h', 'working')",
            [&w.run.run_id],
        )
        .unwrap();
    w.conn
        .execute(
            "INSERT INTO attempts (id, task_id, idea_id, worker_id, mux_handle, state, started_at)
             VALUES ('I-0002/supervising/1', NULL, ?1, 'lead', 'pane-7', 'live', ?2)",
            params![MOVED, util::now()],
        )
        .unwrap();
    w.conn
        .execute("UPDATE ideas SET pane = 'pane-7' WHERE id = ?1", [MOVED])
        .unwrap();
    let attempt = |w: &World| -> (Option<String>, String, Option<String>, String) {
        w.conn
            .query_row(
                "SELECT idea_id, worker_id, mux_handle, state FROM attempts
                 WHERE id = 'I-0002/supervising/1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap()
    };
    let before = attempt(&w);

    w.move_ideas();

    assert_eq!(w.feature_of_idea(MOVED), NEW);
    assert_eq!(attempt(&w), before, "the attempt stays with its idea");
    let idea = ideas_store::idea(&w.conn, MOVED).unwrap().unwrap();
    assert_eq!(idea.pane.as_deref(), Some("pane-7"));
    assert_eq!(idea.state, "in_progress");
}

#[test]
fn each_task_branch_of_a_moved_idea_gets_a_rescue_ref_before_its_rebase() {
    let (mut w, _, two_branch) = merged_with_an_idea_in_flight();
    // A landed and an abandoned branch of the moved idea are not rebased.
    let (gone, gone_branch) = w.task("gone", MOVED);
    w.settle(&gone, "cancelled", &gone_branch, "abandoned");
    let (old, old_branch) = w.task("old", MOVED);
    w.settle(&old, "done", &old_branch, "landed");
    let two_tip = w.tip(&two_branch);
    let main_tip = w.tip("main");

    w.move_ideas();
    for r in nf::pending_refs(&w.conn, &w.git).unwrap() {
        nf::create_ref(&w.git, &r).unwrap();
    }

    let pending = mwi::pending_moves(&w.conn).unwrap();
    assert_eq!(
        pending
            .iter()
            .map(|p| p.branch.as_str())
            .collect::<Vec<_>>(),
        vec![two_branch.as_str()]
    );
    assert_eq!(pending[0].feature, NEW);
    // Step 1: the rescue ref of the tip, and no job yet.
    let wgit = worktree_git(&w.fleet, &w.repo);
    let rescues = GitBranchRescues { git: &wgit };
    let rescued = mwi::rescue(&w.git, &rescues, &pending[0]).unwrap().unwrap();
    let name = rescued.rescue_ref.clone().unwrap();
    assert_eq!(w.rescue_refs(), vec![(name.clone(), two_tip.clone())]);
    assert_eq!(rescued.tip.as_deref(), Some(two_tip.as_str()));
    assert!(w.rebase_jobs().is_empty());
    // Step 2: the move and the rebase job onto the new feature branch.
    let ctx = w.ctx();
    let tx = w.conn.unchecked_transaction().unwrap();
    assert!(mwi::record_move(&ctx, &tx, &rescued, FleetSeconds(0)).unwrap());
    tx.commit().unwrap();
    assert_eq!(
        w.rebase_jobs(),
        vec![(
            two_branch.clone(),
            mwi::rebase_input_key(&two_tip, &main_tip)
        )]
    );
    let row = rows::branch(&w.conn, &two_branch).unwrap().unwrap();
    assert_eq!(row.feature_id, NEW);
    assert_eq!(row.rescue_ref.as_deref(), Some(name.as_str()));
    assert_eq!(w.feature_of_branch(&gone_branch), OLD);
    assert_eq!(w.feature_of_branch(&old_branch), OLD);
    assert!(mwi::pending_moves(&w.conn).unwrap().is_empty());
    assert!(w.complete().is_empty());
}

#[test]
fn crash_between_commit_and_rebase_loses_no_tip() {
    let (mut w, _, two_branch) = merged_with_an_idea_in_flight();
    let two_tip = w.tip(&two_branch);
    let main_tip = w.tip("main");

    // The transaction commits, then fleetd stops before any effect.
    w.move_ideas();
    let wgit = worktree_git(&w.fleet, &w.repo);
    let rescues = GitBranchRescues { git: &wgit };
    let pending = mwi::pending_moves(&w.conn).unwrap();
    assert_eq!(pending.len(), 1);
    // The new feature branch is not there yet: the rescue waits for it and
    // writes nothing.
    assert!(mwi::rescue(&w.git, &rescues, &pending[0])
        .unwrap()
        .is_none());
    assert!(w.rescue_refs().is_empty());
    let refs = nf::pending_refs(&w.conn, &w.git).unwrap();
    assert_eq!(refs.len(), 1);
    nf::create_ref(&w.git, &refs[0]).unwrap();
    // The rescue ref is written, then fleetd stops before the move.
    mwi::rescue(&w.git, &rescues, &pending[0]).unwrap().unwrap();
    assert_eq!(w.rescue_refs().len(), 1);

    // The next start completes each effect 1 time.
    assert_eq!(w.complete(), vec![two_branch.clone()]);
    assert!(w.complete().is_empty());

    let refs = w.rescue_refs();
    assert_eq!(refs.len(), 1, "the same tip gets the same rescue ref");
    assert_eq!(refs[0].1, two_tip);
    assert_eq!(w.tip(&two_branch), two_tip);
    assert_eq!(
        w.rebase_jobs(),
        vec![(
            two_branch.clone(),
            mwi::rebase_input_key(&two_tip, &main_tip)
        )]
    );
    assert_eq!(w.feature_of_branch(&two_branch), NEW);
}

#[test]
fn no_branch_lands_on_a_merged_feature() {
    let w = world();
    let (one, one_branch) = w.task("one", SETTLED);
    w.idea_state(SETTLED, "integrating");
    w.commit_on(RUN_BRANCH, "one.txt", "one\n");
    w.settle(&one, "done", &one_branch, "landed");
    w.idea(MOVED, "in_progress");
    let (three, three_branch) = w.task("three", MOVED);
    let (four, four_branch) = w.task("four", MOVED);
    w.settle(&three, "done", &three_branch, "in_bundle");
    w.settle(&four, "done", &four_branch, "ready");
    // A bundle of the old feature passed its gate, and its ref move waits.
    let old_tip = w.tip(RUN_BRANCH);
    let tx = w.conn.unchecked_transaction().unwrap();
    let bundle = rows::insert_bundle(
        &tx,
        None,
        &NewBundle {
            feature_id: OLD,
            base_tip: &old_tip,
            parent_id: None,
            members: &[three_branch.as_str()],
            due_at: 100,
        },
        cause("a bundle"),
    )
    .unwrap();
    tx.execute(
        "UPDATE bundles SET state = 'passed' WHERE id = ?1",
        [bundle],
    )
    .unwrap();
    tx.commit().unwrap();
    w.merge();
    let mut w = w;

    w.move_ideas();

    let state: String = w
        .conn
        .query_row("SELECT state FROM bundles WHERE id = ?1", [bundle], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(
        state, "dropped",
        "the bundle of the merged feature never moves its ref"
    );
    assert_eq!(w.landing_state(&three_branch), "ready");
    // A pass of the bundles builds nothing on the merged feature.
    let ctx = w.ctx();
    let b = Bundles {
        jobs: &ctx,
        git: &w.git,
    };
    let tx = w.conn.unchecked_transaction().unwrap();
    bundles::advance(&b, &tx, FleetSeconds(0)).unwrap();
    tx.commit().unwrap();
    let open: i64 = w
        .conn
        .query_row(
            "SELECT count(*) FROM bundles WHERE feature_id = ?1 AND state <> 'dropped'",
            [OLD],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(open, 0);
    assert_eq!(w.tip(RUN_BRANCH), old_tip);

    w.effects();
    for branch in [&three_branch, &four_branch] {
        assert_eq!(w.feature_of_branch(branch), NEW);
        assert_eq!(w.landing_state(branch), "ready");
    }
    assert_eq!(w.rebase_jobs().len(), 2);
    assert_eq!(w.tip(RUN_BRANCH), old_tip);
}

#[test]
fn idempotent_pull_request_merged() {
    let w = world();
    let (one, one_branch) = w.task("one", SETTLED);
    w.idea_state(SETTLED, "integrating");
    w.commit_on(RUN_BRANCH, "one.txt", "one\n");
    w.settle(&one, "done", &one_branch, "landed");
    w.idea(MOVED, "in_progress");
    w.task("two", MOVED);
    // No merge yet: nothing moves.
    assert!(w.move_ideas().is_empty());
    assert_eq!(w.feature_state(OLD), "open");
    w.merge();
    let mut w = w;

    // 2 passes see the same merge.
    assert_eq!(w.move_ideas().len(), 1);
    assert!(w.move_ideas().is_empty());
    assert_eq!(w.effects().len(), 1);
    assert!(w.effects().is_empty());

    assert_eq!(w.feature_count(), 2);
    assert_eq!(w.feature_state(OLD), "merged");
    assert_eq!(w.feature_state(NEW), "open");
    assert_eq!(w.feature_of_idea(MOVED), NEW);
    assert_eq!(w.rebase_jobs().len(), 1);
    assert_eq!(w.rescue_refs().len(), 1);
}
