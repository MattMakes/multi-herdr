//! Rescue refs of branches and the 2 classes of rescue refs
//! (docs/specs/fleetd/design.md, "No work is lost", I10, FM-40, RR-16;
//! docs/tickets/fleetd-p4/24-rescue-ref-classes.md).
//!
//! Each test makes its own temporary git repository and ledger. The
//! `worktree_list` executor reads the facts of each rescue ref, and the pass
//! part `collect` decides the class, the expiry and the budget item with no
//! git call. The deletion is a short effect after the commit. No test reads
//! the real clock for a decision: each test gives `now`.

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::Command;

use chrono::{DateTime, Duration, Utc};
use horch_core::fleetd::fleet_runtime::adapters::jobs_store::JobRow;
use horch_core::fleetd::fleet_runtime::application::jobs::JobCtx;
use horch_core::fleetd::fleet_runtime::application::ledger::{transition, Cause};
use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;
use horch_core::fleetd::fleet_runtime::domain::jobs::{Kind, State};
use horch_core::fleetd::fleet_runtime::domain::ledger::Entity;
use horch_core::fleetd::ideas::application::needs_you::{self, NeedsYouList};
use horch_core::fleetd::integration::adapters::git::CliGitGateway;
use horch_core::fleetd::integration::adapters::sqlite::{self as rows, NewBranch, NewFeature};
use horch_core::fleetd::worktree_custody::adapters::executors;
use horch_core::fleetd::worktree_custody::adapters::git::CliWorktreeGit;
use horch_core::fleetd::worktree_custody::adapters::rescue_refs_git::{self as refs_git, RefFacts};
use horch_core::fleetd::worktree_custody::application::rescue_refs::{
    self, Collection, Deleted, RepoFacts,
};
use horch_core::fleetd::worktree_custody::domain::rescue_class::{Class, NEEDS_YOU_KEY};
use horch_core::taskledger::model::Actor;
use horch_core::taskledger::runs::{self, NewRun};
use horch_core::taskledger::tasks::{self, NewTask};
use horch_core::taskledger::Store;
use rusqlite::Connection;

const FEATURE: &str = "F-1";
const FEATURE_BRANCH: &str = "fleet/r1";
const TASK_BRANCH: &str = "fleet/r1-T-1";

// ─── the world of a test ────────────────────────────────────────────────

struct World {
    _dir: tempfile::TempDir,
    root: PathBuf,
    store: Store,
    config: FleetConfig,
    repo: PathBuf,
    /// The first commit of `main`.
    base: String,
}

/// A temp ledger with 1 run, the feature [`FEATURE`] on [`FEATURE_BRANCH`]
/// and its task branch [`TASK_BRANCH`]; a temp repository with 1 commit on
/// `main`, and both branches at that commit.
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
    let base = git(&repo, &["rev-parse", "HEAD"]);
    git(&repo, &["branch", FEATURE_BRANCH]);
    git(&repo, &["branch", TASK_BRANCH]);

    let new_run = NewRun {
        slug: "r1".into(),
        ..Default::default()
    };
    let run = store
        .immediate(|tx| runs::create(tx, &root, &new_run))
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
    store
        .immediate(|tx| {
            let feature = NewFeature {
                id: FEATURE,
                name: Some("r1"),
                branch: Some(FEATURE_BRANCH),
                primary_branch: "main",
            };
            rows::insert_feature(tx, None, &feature, cause("feature"))?;
            for (name, task_id) in [
                (FEATURE_BRANCH, None),
                (TASK_BRANCH, Some(task.task_id.as_str())),
            ] {
                let new = NewBranch {
                    name,
                    feature_id: FEATURE,
                    task_id,
                    owner: "fleetd",
                    due_at: 0,
                };
                rows::insert_branch(tx, None, &new, cause("branch"))?;
            }
            Ok(())
        })
        .unwrap();
    World {
        _dir: dir,
        root,
        store,
        config: FleetConfig::default(),
        repo,
        base,
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

fn git_out(dir: &Path, args: &[&str]) -> std::process::Output {
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

    /// The git of fleetd: it can write only the git folder of the
    /// repository.
    fn writer(&self) -> CliWorktreeGit {
        CliWorktreeGit::new("git", self.root.join("fleet")).writing(&[self.repo.join(".git")])
    }

    /// The gateway of the deletion: it can write the git folder.
    fn gateway(&self) -> CliGitGateway {
        CliGitGateway::new(self.writer())
    }

    /// A gateway that can only read, so each ref write fails.
    fn read_only_gateway(&self) -> CliGitGateway {
        CliGitGateway::new(
            CliWorktreeGit::new("git", self.root.join("fleet"))
                .reading(std::slice::from_ref(&self.repo)),
        )
    }

    /// The tip of `rev`, or `None` when it does not exist.
    fn tip(&self, rev: &str) -> Option<String> {
        let out = git_out(&self.repo, &["rev-parse", "--verify", "-q", rev]);
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    /// Commit `files` on `branch` with no checkout, and give the new tip.
    fn commit_on(&self, branch: &str, files: &[(&str, &[u8])], msg: &str) -> String {
        let parent = self.tip(branch).unwrap();
        let index = self.root.join("test-index");
        let _ = std::fs::remove_file(&index);
        let with_index = |args: &[&str]| {
            let out = Command::new("git")
                .arg("-C")
                .arg(&self.repo)
                .args(["-c", "user.name=t", "-c", "user.email=t@t"])
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
        with_index(&["read-tree", &parent]);
        for (name, bytes) in files {
            let blob_file = self.root.join("test-blob");
            std::fs::write(&blob_file, bytes).unwrap();
            let blob = git(
                &self.repo,
                &["hash-object", "-w", blob_file.to_str().unwrap()],
            );
            let info = format!("100644,{blob},{name}");
            with_index(&["update-index", "--add", "--cacheinfo", &info]);
        }
        let tree = with_index(&["write-tree"]);
        let commit = git(
            &self.repo,
            &["commit-tree", &tree, "-p", &parent, "-m", msg],
        );
        git(
            &self.repo,
            &["update-ref", &format!("refs/heads/{branch}"), &commit],
        );
        commit
    }

    /// Force `branch` to `tip`, as a forced update does.
    fn force(&self, branch: &str, tip: &str) {
        git(
            &self.repo,
            &["update-ref", &format!("refs/heads/{branch}"), tip],
        );
    }

    /// Write the rescue ref of the tip of `branch`, as fleetd does before it
    /// moves the branch, and record it on the row.
    fn rescue(&mut self, branch: &str) -> String {
        let tip = self.tip(branch).unwrap();
        let name = refs_git::write_branch_rescue(&self.writer(), &self.repo, branch, &tip).unwrap();
        let ctx_config = self.config.clone();
        self.store
            .immediate(|tx| {
                let ctx = JobCtx {
                    guard: None,
                    config: &ctx_config,
                    needs_you: &NeedsYouList,
                };
                rescue_refs::record_branch_rescue(&ctx, tx, branch, &name)
            })
            .unwrap();
        name
    }

    /// The facts of the rescue refs, as the `worktree_list` job of the
    /// repository reads them.
    fn facts(&self) -> Vec<RefFacts> {
        let row = JobRow {
            id: 1,
            kind: Kind::WorktreeList,
            subject: self.repo.display().to_string(),
            repo: Some(self.repo.display().to_string()),
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
        refs_git::facts_of(ran.data.as_ref().unwrap()).expect("the result names the rescue refs")
    }

    /// 1 pass part: read the facts, then decide in 1 transaction that
    /// commits.
    fn collect(&mut self, now: DateTime<Utc>) -> Collection {
        let lists = [RepoFacts {
            repo: self.repo.display().to_string(),
            refs: Some(self.facts()),
        }];
        let config = self.config.clone();
        self.store
            .immediate(|tx| {
                let ctx = JobCtx {
                    guard: None,
                    config: &config,
                    needs_you: &NeedsYouList,
                };
                rescue_refs::collect(&ctx, tx, &lists, now)
            })
            .unwrap()
    }

    /// The effect after the commit: each deletion of `c`.
    fn delete_all(&self, c: &Collection) -> Vec<Deleted> {
        c.deletions
            .iter()
            .map(|d| rescue_refs::delete(&self.gateway(), d).unwrap())
            .collect()
    }

    /// Move the feature to `to`, and give a time that is not before the
    /// stamp of the move.
    fn end_feature(&mut self, to: &str) -> DateTime<Utc> {
        self.store
            .immediate(|tx| {
                transition(
                    tx,
                    None,
                    Entity::Feature,
                    FEATURE,
                    to,
                    cause("test"),
                    &NeedsYouList,
                )?;
                Ok(())
            })
            .unwrap();
        Utc::now()
    }

    fn class_of(&self, name: &str) -> Class {
        let f = self.facts();
        let r = f
            .iter()
            .find(|r| r.name == name)
            .expect("the ref is listed");
        rescue_refs::class(r)
    }

    fn open_budget_items(&self) -> usize {
        needs_you::open_items(self.conn())
            .unwrap()
            .into_iter()
            .filter(|i| i.key == NEEDS_YOU_KEY)
            .count()
    }
}

fn days(n: i64) -> Duration {
    Duration::days(n)
}

/// Bytes that git cannot compress: `n` bytes of a simple generator.
fn noise(n: usize) -> Vec<u8> {
    let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
    (0..n)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            (x >> 24) as u8
        })
        .collect()
}

// ─── the writer ─────────────────────────────────────────────────────────

#[test]
fn branch_rescue_ref_is_create_only_and_read_back() {
    let mut w = world();
    let tip = w.commit_on(TASK_BRANCH, &[("c.txt", b"c\n")], "c");
    // A ref that another writer made first keeps its tip.
    let first = "refs/horch/rescue/branch/fleet-r1-T-1/1";
    git(&w.repo, &["update-ref", first, &w.base, ""]);

    let name = w.rescue(TASK_BRANCH);

    assert_eq!(name, "refs/horch/rescue/branch/fleet-r1-T-1/2");
    assert_eq!(w.tip(&name).as_deref(), Some(tip.as_str()));
    assert_eq!(w.tip(first).as_deref(), Some(w.base.as_str()));
    let row = rows::branch(w.conn(), TASK_BRANCH).unwrap().unwrap();
    assert_eq!(row.rescue_ref.as_deref(), Some(name.as_str()));

    // A tip that is not a commit writes nothing.
    let bad = refs_git::write_branch_rescue(&w.writer(), &w.repo, TASK_BRANCH, &"0".repeat(40));
    assert!(bad.is_err());
    assert_eq!(w.tip("refs/horch/rescue/branch/fleet-r1-T-1/3"), None);
    // A branch name that cannot name a ref folder writes nothing.
    let bad = refs_git::write_branch_rescue(&w.writer(), &w.repo, "a b", &tip);
    assert!(bad.is_err());
}

#[test]
fn second_rescue_of_1_branch_does_not_overwrite_the_first() {
    let mut w = world();
    let tip1 = w.commit_on(TASK_BRANCH, &[("c.txt", b"c\n")], "c");
    let first = w.rescue(TASK_BRANCH);
    let tip2 = w.commit_on(TASK_BRANCH, &[("d.txt", b"d\n")], "d");
    let second = w.rescue(TASK_BRANCH);

    assert_ne!(first, second);
    assert_eq!(w.tip(&first).as_deref(), Some(tip1.as_str()));
    assert_eq!(w.tip(&second).as_deref(), Some(tip2.as_str()));
    // A rescue of the same tip again names the newest ref and writes no
    // new one.
    assert_eq!(w.rescue(TASK_BRANCH), second);
    let all = git(
        &w.repo,
        &["for-each-ref", "--format=%(refname)", "refs/horch/rescue/"],
    );
    assert_eq!(all.lines().count(), 2, "{all}");
}

// ─── the class ──────────────────────────────────────────────────────────

#[test]
fn ref_with_work_in_no_landed_branch_is_unique() {
    let mut w = world();
    w.commit_on(TASK_BRANCH, &[("c.txt", b"c\n")], "c");
    let name = w.rescue(TASK_BRANCH);
    // A forced update drops the commit from the branch.
    w.force(TASK_BRANCH, &w.base.clone());

    assert_eq!(w.class_of(&name), Class::Unique);
}

#[test]
fn ref_whose_work_landed_is_superseded() {
    let mut w = world();
    w.commit_on(TASK_BRANCH, &[("c.txt", b"c\n")], "c");
    let name = w.rescue(TASK_BRANCH);
    w.force(TASK_BRANCH, &w.base.clone());
    assert_eq!(w.class_of(&name), Class::Unique);

    // A squash merge puts the same content on the feature branch.
    w.commit_on(FEATURE_BRANCH, &[("c.txt", b"c\n")], "squash");
    assert_eq!(w.class_of(&name), Class::Superseded);
}

#[test]
fn ref_whose_work_is_in_a_newer_tip_of_its_branch_is_superseded() {
    let mut w = world();
    w.commit_on(TASK_BRANCH, &[("c.txt", b"c\n")], "c");
    let name = w.rescue(TASK_BRANCH);
    // A rebase onto a new feature tip: a new commit with the same content.
    w.commit_on(FEATURE_BRANCH, &[("b.txt", b"b\n")], "b");
    w.force(TASK_BRANCH, &w.tip(FEATURE_BRANCH).unwrap());
    w.commit_on(TASK_BRANCH, &[("c.txt", b"c\n")], "c again");

    assert_eq!(w.class_of(&name), Class::Superseded);
}

// ─── the expiry ─────────────────────────────────────────────────────────

/// A superseded ref of the task branch: its work is on the feature branch.
fn superseded_ref(w: &mut World) -> String {
    w.commit_on(TASK_BRANCH, &[("c.txt", b"c\n")], "c");
    let name = w.rescue(TASK_BRANCH);
    w.force(TASK_BRANCH, &w.base.clone());
    w.commit_on(FEATURE_BRANCH, &[("c.txt", b"c\n")], "squash");
    assert_eq!(w.class_of(&name), Class::Superseded);
    name
}

#[test]
fn superseded_ref_expires_30_days_after_its_feature_ended() {
    let mut w = world();
    let name = superseded_ref(&mut w);
    let tip = w.tip(&name).unwrap();

    // An open feature: no expiry, also after a long time.
    let c = w.collect(Utc::now() + days(365));
    assert!(c.deletions.is_empty(), "{c:?}");
    assert_eq!((c.superseded.count, c.unique.count), (1, 0));

    let ended = w.end_feature("merged");
    let c = w.collect(ended + days(29));
    assert!(c.deletions.is_empty(), "{c:?}");
    assert!(w.tip(&name).is_some());

    let c = w.collect(ended + days(30));
    assert_eq!(c.deletions.len(), 1, "{c:?}");
    assert_eq!(c.deletions[0].name, name);
    assert_eq!(c.deletions[0].tip, tip);
    assert_eq!(w.delete_all(&c), vec![Deleted::Deleted]);
    assert_eq!(w.tip(&name), None);

    // The next pass sees the ref gone and plans nothing.
    let c = w.collect(ended + days(31));
    assert!(c.deletions.is_empty(), "{c:?}");
}

#[test]
fn unique_ref_is_never_deleted_by_the_expiry() {
    let mut w = world();
    w.commit_on(TASK_BRANCH, &[("c.txt", b"c\n")], "c");
    let name = w.rescue(TASK_BRANCH);
    w.force(TASK_BRANCH, &w.base.clone());
    let ended = w.end_feature("abandoned");

    let c = w.collect(ended + days(3650));

    assert!(c.deletions.is_empty(), "{c:?}");
    assert_eq!(c.unique.count, 1);
    assert!(c.unique.bytes > 0);
    assert!(w.tip(&name).is_some());
}

// ─── the budget ─────────────────────────────────────────────────────────

#[test]
fn over_the_unique_budget_opens_1_item_and_deletes_nothing() {
    let mut w = world();
    w.config.rescue_unique_budget_mb = 1;
    let big = noise(2 * 1024 * 1024);
    w.commit_on(TASK_BRANCH, &[("big.bin", &big)], "big");
    let name = w.rescue(TASK_BRANCH);
    w.force(TASK_BRANCH, &w.base.clone());
    let ended = w.end_feature("abandoned");

    let c = w.collect(ended + days(400));
    let again = w.collect(ended + days(401));

    assert!(c.over_budget && again.over_budget);
    assert!(c.unique.bytes > 1024 * 1024, "{c:?}");
    assert!(c.deletions.is_empty() && again.deletions.is_empty());
    assert_eq!(w.open_budget_items(), 1);
    assert!(w.tip(&name).is_some());

    // Under a larger budget, the item ends.
    w.config.rescue_unique_budget_mb = 100;
    let c = w.collect(ended + days(402));
    assert!(!c.over_budget);
    assert_eq!(w.open_budget_items(), 0);
}

// ─── the deletion and its faults ────────────────────────────────────────

#[test]
fn failed_deletion_is_done_again_on_the_next_pass() {
    let mut w = world();
    let name = superseded_ref(&mut w);
    let ended = w.end_feature("merged");

    let c = w.collect(ended + days(30));
    assert_eq!(c.deletions.len(), 1);
    // The effect fails: git cannot write the git folder.
    assert!(rescue_refs::delete(&w.read_only_gateway(), &c.deletions[0]).is_err());
    assert!(w.tip(&name).is_some());

    let c = w.collect(ended + days(30));
    assert_eq!(c.deletions.len(), 1);
    assert_eq!(w.delete_all(&c), vec![Deleted::Deleted]);
    assert_eq!(w.tip(&name), None);
    // A second effect of the same plan finds the ref gone.
    assert_eq!(w.delete_all(&c), vec![Deleted::Gone]);
}

#[test]
fn crash_before_the_commit_or_before_the_effect_loses_no_tip() {
    let mut w = world();
    w.config.rescue_unique_budget_mb = 1;
    let name = superseded_ref(&mut w);
    let big = noise(2 * 1024 * 1024);
    let tip = w.commit_on(TASK_BRANCH, &[("big.bin", &big)], "big");
    let unique = w.rescue(TASK_BRANCH);
    w.force(TASK_BRANCH, &w.base.clone());
    let ended = w.end_feature("merged");
    let now = ended + days(30);

    // Fault point 1: the pass dies before its commit. Nothing is written,
    // and no effect runs.
    let lists = [RepoFacts {
        repo: w.repo.display().to_string(),
        refs: Some(w.facts()),
    }];
    {
        let tx = w.store.conn().unchecked_transaction().unwrap();
        let ctx = w.ctx();
        let c = rescue_refs::collect(&ctx, &tx, &lists, now).unwrap();
        assert_eq!(c.deletions.len(), 1);
        drop(tx);
    }
    assert_eq!(w.open_budget_items(), 0);
    assert!(w.tip(&name).is_some());

    // Fault point 2: the pass commits and dies before the effect.
    let c = w.collect(now);
    assert_eq!(c.deletions.len(), 1);
    assert_eq!(w.open_budget_items(), 1);
    assert!(w.tip(&name).is_some());

    // The next pass completes the effect 1 time.
    let c = w.collect(now);
    assert_eq!(c.deletions.len(), 1);
    assert_eq!(w.delete_all(&c), vec![Deleted::Deleted]);
    assert_eq!(w.tip(&name), None);
    assert_eq!(w.open_budget_items(), 1);
    // No tip and no unique work is lost.
    assert_eq!(w.tip(&unique).as_deref(), Some(tip.as_str()));
    let c = w.collect(now + days(1));
    assert!(c.deletions.is_empty(), "{c:?}");
}

#[test]
fn crash_after_the_ref_write_and_before_the_row_writes_no_second_ref() {
    let mut w = world();
    let tip = w.commit_on(TASK_BRANCH, &[("c.txt", b"c\n")], "c");
    // The writer dies after the ref write, before the row records it.
    let name = refs_git::write_branch_rescue(&w.writer(), &w.repo, TASK_BRANCH, &tip).unwrap();
    let row = rows::branch(w.conn(), TASK_BRANCH).unwrap().unwrap();
    assert_eq!(row.rescue_ref, None);

    // The next rescue of the same tip names the same ref, and the row
    // records it.
    assert_eq!(w.rescue(TASK_BRANCH), name);
    let row = rows::branch(w.conn(), TASK_BRANCH).unwrap().unwrap();
    assert_eq!(row.rescue_ref.as_deref(), Some(name.as_str()));
    assert_eq!(w.tip(&name).as_deref(), Some(tip.as_str()));
}
