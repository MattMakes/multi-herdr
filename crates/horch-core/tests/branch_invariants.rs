//! I11 and I12: a fleet branch with no row is adopted as an orphan and
//! never deleted; a branch is deleted only when it is landed or rescued and
//! no `git stash` entry names it (docs/specs/fleetd/design.md, "No work is
//! lost" items 4 and 5; preservation-04-behavior.md row W6;
//! docs/tickets/fleetd-p4/44-orphan-branches-and-deletion-guard.md).
//!
//! Each test makes its own temporary git repository and ledger. The
//! `worktree_list` executor reads the fleet branches, the stash names and
//! the rescue refs; the pass part decides with no git call; the deletion is
//! a short effect after the commit. No test reads the load, the limits or
//! the clock of the machine: the config is the default and the fleet clock
//! is a number that the test gives.

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use horch_core::fleetd::fleet_runtime::adapters::jobs_store::JobRow;
use horch_core::fleetd::fleet_runtime::application::jobs::JobCtx;
use horch_core::fleetd::fleet_runtime::application::ledger::{transition, Cause};
use horch_core::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;
use horch_core::fleetd::fleet_runtime::domain::jobs::{Kind, State};
use horch_core::fleetd::fleet_runtime::domain::ledger::Entity;
use horch_core::fleetd::ideas::application::needs_you::{self, NeedsYouList};
use horch_core::fleetd::integration::adapters::git::CliGitGateway;
use horch_core::fleetd::integration::adapters::sqlite::{self as rows, BranchFact, NewBranch};
use horch_core::fleetd::integration::application::branches::{
    self, Deletion, Orphan, Ran, RepoListing, ORPHAN_KIND,
};
use horch_core::fleetd::worktree_custody::adapters::executors;
use horch_core::fleetd::worktree_custody::adapters::git::CliWorktreeGit;
use horch_core::fleetd::worktree_custody::adapters::rescue_refs_git as refs_git;
use horch_core::taskledger::model::Actor;
use horch_core::taskledger::runs::{self, NewRun};
use horch_core::taskledger::tasks::{self, NewTask};
use horch_core::taskledger::Store;
use rusqlite::Connection;

const F1_BRANCH: &str = "fleet/r1";
/// A run branch outside `fleet/`.
const F2_BRANCH: &str = "work";
/// The task branch with a row.
const TASK: &str = "fleet/r1-T1-a";
const CLOCK: FleetSeconds = FleetSeconds(1000);

// ─── the world of a test ────────────────────────────────────────────────

struct World {
    _dir: tempfile::TempDir,
    root: PathBuf,
    store: Store,
    config: FleetConfig,
    repo: PathBuf,
    /// The features of [`F1_BRANCH`] and [`F2_BRANCH`].
    f1: String,
    f2: String,
}

/// A temp ledger with 2 runs of 1 repository, their features on
/// [`F1_BRANCH`] and [`F2_BRANCH`], and the task branch [`TASK`] of the
/// first in `building`; a temp repository with 1 commit on `main`, and
/// each of these branches at that commit.
fn world() -> World {
    assert!(
        std::env::var_os("FLEET_ROOT").is_none(),
        "these tests use the fleet root of their temp ledger"
    );
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let mut store = Store::open(&root.join("ledger.sqlite3")).unwrap();
    let repo = root.join("repo");
    std::fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    std::fs::write(repo.join("a.txt"), "a\n").unwrap();
    git(&repo, &["add", "a.txt"]);
    git(&repo, &["commit", "-q", "-m", "first"]);
    for b in [F1_BRANCH, F2_BRANCH, TASK] {
        git(&repo, &["branch", b]);
    }
    // Adding the first task of a run makes its feature and the row of its
    // branch.
    let mut task_id = String::new();
    for (slug, branch) in [("r1", F1_BRANCH), ("w", F2_BRANCH)] {
        let new_run = NewRun {
            slug: slug.into(),
            run_branch: Some(branch.into()),
            ..Default::default()
        };
        let run = store
            .immediate(|tx| {
                let run = runs::create(tx, &repo, &new_run)?;
                // FR-12: a task needs an idea in flight of the feature of its run.
                horch_core::taskledger::schema::run_idea(
                    tx,
                    &run.run_id,
                    &horch_core::taskledger::util::now(),
                    true,
                )?;
                Ok(run)
            })
            .unwrap();
        let task = store
            .immediate(|tx| {
                tasks::add(
                    tx,
                    &run,
                    &NewTask::work("t1", "t1.md", "dev"),
                    Actor::Orchestrator,
                )
            })
            .unwrap();
        if slug == "r1" {
            task_id = task.task_id;
        }
    }
    let feature_of = |branch: &str| -> String {
        store
            .conn()
            .query_row("SELECT id FROM features WHERE branch = ?1", [branch], |r| {
                r.get(0)
            })
            .unwrap()
    };
    let f1 = feature_of(F1_BRANCH);
    let f2 = feature_of(F2_BRANCH);
    store
        .immediate(|tx| {
            let new = NewBranch {
                name: TASK,
                feature_id: &f1,
                task_id: Some(&task_id),
                owner: "worker",
                due_at: 0,
            };
            rows::insert_branch(tx, None, &new, cause("task branch"))?;
            Ok(())
        })
        .unwrap();
    World {
        _dir: dir,
        root,
        store,
        config: FleetConfig::default(),
        repo,
        f1,
        f2,
    }
}

fn cause(text: &str) -> Cause<'_> {
    Cause {
        actor: Actor::Horch,
        text,
    }
}

/// Run git in `dir` for the test itself, with no config of the user, and
/// give its trimmed output. A failed call fails the test.
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
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .output()
        .unwrap()
}

impl World {
    fn conn(&self) -> &Connection {
        self.store.conn()
    }

    fn ctx(&self) -> JobCtx<'_> {
        JobCtx {
            guard: None,
            config: &self.config,
            needs_you: &NeedsYouList,
        }
    }

    /// A git that can write the git folder of the repository.
    fn writer(&self) -> CliWorktreeGit {
        CliWorktreeGit::new("git", self.root.join("fleet")).writing(&[self.repo.join(".git")])
    }

    /// The gateway of the deletion effect.
    fn gateway(&self) -> CliGitGateway {
        CliGitGateway::new(self.writer())
    }

    /// The tip of `rev`, or `None` when it does not exist.
    fn tip(&self, rev: &str) -> Option<String> {
        let out = git_out(&self.repo, &["rev-parse", "--verify", "-q", rev]);
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    /// Commit 1 new file on `branch` with no checkout, and give the new tip.
    fn commit_on(&self, branch: &str, file: &str) -> String {
        let parent = self.tip(branch).unwrap();
        let blob = {
            let out = Command::new("git")
                .arg("-C")
                .arg(&self.repo)
                .args(["hash-object", "-w", "--stdin"])
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .spawn()
                .and_then(|mut c| {
                    use std::io::Write;
                    c.stdin.take().unwrap().write_all(file.as_bytes())?;
                    c.wait_with_output()
                })
                .unwrap();
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        };
        let base_tree = git(&self.repo, &["rev-parse", &format!("{parent}^{{tree}}")]);
        let index = self.root.join("test-index");
        let _ = std::fs::remove_file(&index);
        let with_index = |args: &[&str]| {
            let out = Command::new("git")
                .arg("-C")
                .arg(&self.repo)
                .args(args)
                .env("GIT_INDEX_FILE", &index)
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        };
        with_index(&["read-tree", &base_tree]);
        with_index(&[
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("100644,{blob},{file}"),
        ]);
        let tree = with_index(&["write-tree"]);
        let tip = git(
            &self.repo,
            &["commit-tree", &tree, "-p", &parent, "-m", file],
        );
        git(
            &self.repo,
            &["update-ref", &format!("refs/heads/{branch}"), &tip, &parent],
        );
        tip
    }

    /// Make a stash entry that names `branch`, and leave `main` checked out.
    fn stash_on(&self, branch: &str) {
        git(&self.repo, &["checkout", "-q", branch]);
        std::fs::write(self.repo.join("a.txt"), "stashed work\n").unwrap();
        git(&self.repo, &["stash", "push", "-q", "-m", "keep"]);
        git(&self.repo, &["checkout", "-q", "main"]);
    }

    /// Drop each stash entry.
    fn clear_stash(&self) {
        git(&self.repo, &["stash", "clear"]);
    }

    /// Move the task branch `name` from `building` to `to` through the 1
    /// door. A branch lands only from a bundle: `landed` goes through
    /// `verifying`, `ready` and `in_bundle` first.
    fn move_branch(&mut self, name: &str, to: &str) {
        let path: &[&str] = match to {
            "landed" => &["verifying", "ready", "in_bundle", "landed"],
            _ => &[to],
        };
        self.store
            .immediate(|tx| {
                for state in path {
                    transition(
                        tx,
                        None,
                        Entity::TaskBranch,
                        name,
                        state,
                        cause("test"),
                        &NeedsYouList,
                    )?;
                }
                Ok(())
            })
            .unwrap();
    }

    /// Write the rescue ref of `branch` at its tip, and record it in the
    /// row. Gives the name of the ref.
    fn rescue(&mut self, branch: &str) -> String {
        let tip = self.tip(branch).unwrap();
        let name = refs_git::write_branch_rescue(&self.writer(), &self.repo, branch, &tip).unwrap();
        self.store
            .immediate(|tx| {
                rows::set_branch_fact(
                    tx,
                    None,
                    branch,
                    BranchFact::RescueRef(&name),
                    cause("rescue"),
                )
            })
            .unwrap();
        name
    }

    /// 1 run of the `worktree_list` job of the repository, through its
    /// executor: the facts that a pass reads.
    fn list(&self) -> Vec<RepoListing> {
        let subject = self.repo.display().to_string();
        let row = JobRow {
            id: 1,
            kind: Kind::WorktreeList,
            subject: subject.clone(),
            repo: Some(subject.clone()),
            input_key: "0".into(),
            run: 1,
            abandoned_runs: 0,
            not_before: None,
            state: State::Running,
            pid: None,
            pid_start: None,
            time_limit_s: 60,
            deadline: None,
            result_json: None,
            created_at: String::new(),
            started_at: None,
            ended_at: None,
        };
        let ran = executors::run(self.conn(), &row).unwrap();
        assert!(ran.outcome.passed, "{}", ran.outcome.text);
        let listing = RepoListing::of(subject, ran.data.as_ref().unwrap());
        assert!(
            listing.listing.is_some(),
            "the result names the fleet branches"
        );
        vec![listing]
    }

    /// The pass part in `tx`: adopt the orphans, then decide the deletions.
    fn decide(&self, tx: &Connection, lists: &[RepoListing]) -> (Vec<Orphan>, Vec<Deletion>) {
        let ctx = self.ctx();
        let orphans = branches::adopt_orphans(&ctx, tx, lists, CLOCK).unwrap();
        let deletions = branches::deletions(tx, lists).unwrap();
        (orphans, deletions)
    }

    /// 1 pass part that commits.
    fn pass(&self, lists: &[RepoListing]) -> (Vec<Orphan>, Vec<Deletion>) {
        let tx = self.conn().unchecked_transaction().unwrap();
        let out = self.decide(&tx, lists);
        tx.commit().unwrap();
        out
    }

    /// Run each deletion after the commit.
    fn delete_all(&self, deletions: &[Deletion]) -> Vec<Ran> {
        let gw = self.gateway();
        deletions
            .iter()
            .map(|d| branches::run(&gw, d).unwrap())
            .collect()
    }

    fn open_orphan_items(&self) -> Vec<String> {
        needs_you::open_keys(self.conn(), ORPHAN_KIND).unwrap()
    }

    fn row(&self, name: &str) -> Option<rows::BranchRow> {
        rows::branch(self.conn(), name).unwrap()
    }
}

fn names(deletions: &[Deletion]) -> Vec<&str> {
    deletions.iter().map(|d| d.branch.as_str()).collect()
}

// ─── I11 ────────────────────────────────────────────────────────────────

#[test]
fn orphan_fleet_branch_is_adopted_and_reported_and_not_deleted() {
    let w = world();
    let lost = "fleet/r1-T7-lost";
    let outside = "work-T3-x";
    let stray = "fleet/stray";
    for b in [lost, outside, stray, "other"] {
        git(&w.repo, &["branch", b]);
    }
    let lost_tip = w.commit_on(lost, "lost.txt");

    let lists = w.list();
    let (orphans, deletions) = w.pass(&lists);

    let mut adopted: Vec<(&str, Option<&str>)> = orphans
        .iter()
        .map(|o| (o.branch.as_str(), o.feature_id.as_deref()))
        .collect();
    adopted.sort();
    assert_eq!(
        adopted,
        vec![
            (lost, Some(w.f1.as_str())),
            (stray, None),
            (outside, Some(w.f2.as_str()))
        ],
        "each fleet branch with no row is adopted or reported; main and other are not fleet branches"
    );
    let row = w.row(lost).expect("the orphan has a row");
    assert!(row.orphan);
    assert_eq!(row.feature_id, w.f1);
    assert_eq!(row.task_id, None);
    assert_eq!(row.landing_state, None);
    assert!(w.row(outside).unwrap().orphan);
    assert_eq!(w.row(outside).unwrap().feature_id, w.f2);
    assert!(w.row(stray).is_none(), "a branch of no feature gets no row");
    assert!(w.row("other").is_none());
    assert_eq!(w.open_orphan_items().len(), 3, "each orphan is reported");
    assert!(deletions.is_empty(), "{deletions:?}");
    assert!(w.delete_all(&deletions).is_empty());

    // The next pass adopts nothing again, and deletes no orphan.
    let lists = w.list();
    let (orphans, deletions) = w.pass(&lists);
    let again: Vec<&str> = orphans.iter().map(|o| o.branch.as_str()).collect();
    assert_eq!(
        again,
        vec![stray],
        "only the branch with no row is reported again"
    );
    assert!(deletions.is_empty(), "{deletions:?}");
    assert_eq!(w.open_orphan_items().len(), 3);
    for b in [lost, outside, stray] {
        assert!(w.tip(b).is_some(), "{b} is not deleted");
    }
    assert_eq!(w.tip(lost).as_deref(), Some(lost_tip.as_str()));
}

// ─── I12 ────────────────────────────────────────────────────────────────

#[test]
fn stash_entry_blocks_the_deletion_of_its_branch() {
    let mut w = world();
    w.commit_on(TASK, "t.txt");
    w.move_branch(TASK, "landed");
    w.stash_on(TASK);

    let (_, deletions) = w.pass(&w.list());
    assert!(
        deletions.is_empty(),
        "a stash entry names the branch: {deletions:?}"
    );
    assert!(w.tip(TASK).is_some());

    // The stash is dropped: the branch is deleted.
    w.clear_stash();
    let (_, deletions) = w.pass(&w.list());
    assert_eq!(names(&deletions), vec![TASK]);

    // A stash entry made after the listing still blocks the effect.
    w.stash_on(TASK);
    assert_eq!(w.delete_all(&deletions), vec![Ran::Stashed]);
    assert!(w.tip(TASK).is_some());
}

#[test]
fn unlanded_unrescued_branch_is_never_deleted() {
    let mut w = world();
    w.commit_on(TASK, "t.txt");
    // building, with work.
    let (_, deletions) = w.pass(&w.list());
    assert!(deletions.is_empty(), "{deletions:?}");

    // A rescue ref at an older tip does not save the newer work.
    w.rescue(TASK);
    let newer = w.commit_on(TASK, "u.txt");
    w.move_branch(TASK, "abandoned");
    let (_, deletions) = w.pass(&w.list());
    assert!(deletions.is_empty(), "{deletions:?}");

    // The feature is merged: the branch holds work that is not saved.
    w.store
        .immediate(|tx| {
            transition(
                tx,
                None,
                Entity::Feature,
                &w.f1,
                "merged",
                cause("test"),
                &NeedsYouList,
            )?;
            Ok(())
        })
        .unwrap();
    let (_, deletions) = w.pass(&w.list());
    assert!(deletions.is_empty(), "{deletions:?}");
    assert_eq!(w.tip(TASK).as_deref(), Some(newer.as_str()));
    // No feature branch is deleted.
    assert!(w.tip(F1_BRANCH).is_some());
}

#[test]
fn landed_branch_with_no_stash_is_deleted() {
    let mut w = world();
    w.commit_on(TASK, "t.txt");
    w.move_branch(TASK, "landed");

    let (_, deletions) = w.pass(&w.list());
    assert_eq!(names(&deletions), vec![TASK]);
    assert_eq!(w.delete_all(&deletions), vec![Ran::Deleted]);
    assert_eq!(w.tip(TASK), None);
    // The next listing has no such branch, so nothing is due.
    let (_, deletions) = w.pass(&w.list());
    assert!(deletions.is_empty(), "{deletions:?}");
    assert!(w.row(TASK).is_some(), "the row stays");
}

#[test]
fn abandoned_branch_with_a_rescue_ref_at_its_tip_is_deleted() {
    let mut w = world();
    let tip = w.commit_on(TASK, "t.txt");
    w.move_branch(TASK, "abandoned");
    let rescue = w.rescue(TASK);

    let (_, deletions) = w.pass(&w.list());
    assert_eq!(names(&deletions), vec![TASK]);
    assert_eq!(w.delete_all(&deletions), vec![Ran::Deleted]);
    assert_eq!(w.tip(TASK), None);
    assert_eq!(
        w.tip(&rescue).as_deref(),
        Some(tip.as_str()),
        "the work is kept"
    );
}

// ─── crash ──────────────────────────────────────────────────────────────

#[test]
fn crash_before_the_commit_or_before_the_effect_loses_no_tip() {
    let mut w = world();
    let tip = w.commit_on(TASK, "t.txt");
    w.move_branch(TASK, "landed");
    let orphan = "fleet/r1-T2-o";
    git(&w.repo, &["branch", orphan]);
    let lists = w.list();

    // Fault point 1: the pass dies before its commit. Nothing is written,
    // and no effect runs.
    {
        let tx = w.conn().unchecked_transaction().unwrap();
        let (orphans, deletions) = w.decide(&tx, &lists);
        assert_eq!(orphans.len(), 1);
        assert_eq!(names(&deletions), vec![TASK]);
        drop(tx);
    }
    assert!(w.row(orphan).is_none());
    assert!(w.open_orphan_items().is_empty());
    assert_eq!(w.tip(TASK).as_deref(), Some(tip.as_str()));

    // Fault point 2: the pass commits and dies before the effect.
    let (orphans, deletions) = w.pass(&lists);
    assert_eq!(orphans.len(), 1);
    assert_eq!(names(&deletions), vec![TASK]);
    assert!(w.row(orphan).unwrap().orphan);
    assert_eq!(w.tip(TASK).as_deref(), Some(tip.as_str()));

    // The next pass, on the same listing, completes the effect 1 time.
    let (orphans, deletions) = w.pass(&lists);
    assert!(orphans.is_empty(), "the orphan is adopted 1 time");
    assert_eq!(w.delete_all(&deletions), vec![Ran::Deleted]);
    assert_eq!(w.tip(TASK), None);
    // A pass on a stale listing gives the effect again: git has no such
    // branch, so nothing more happens.
    let (_, deletions) = w.pass(&lists);
    assert_eq!(
        w.delete_all(&deletions),
        vec![Ran::Mismatch { actual: None }]
    );
    assert!(w.tip(orphan).is_some(), "the orphan is never deleted");
}

#[test]
fn branch_that_moved_after_the_listing_is_not_deleted() {
    let mut w = world();
    w.commit_on(TASK, "t.txt");
    w.move_branch(TASK, "landed");
    let (_, deletions) = w.pass(&w.list());
    assert_eq!(names(&deletions), vec![TASK]);

    // New work arrives on the branch before the effect runs.
    let newer = w.commit_on(TASK, "late.txt");
    assert_eq!(
        w.delete_all(&deletions),
        vec![Ran::Mismatch {
            actual: Some(newer.clone())
        }]
    );
    assert_eq!(
        w.tip(TASK).as_deref(),
        Some(newer.as_str()),
        "no tip is lost"
    );
}
