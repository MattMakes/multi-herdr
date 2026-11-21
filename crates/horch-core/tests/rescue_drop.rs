//! `horch rescue drop <ref>`: you delete 1 rescue ref, and horch verifies it
//! (docs/specs/fleetd/design.md, "No work is lost", I10, FM-40;
//! docs/tickets/fleetd-p4/43-horch-rescue-drop.md).
//!
//! Each test makes its own temporary git repository and ledger. The drop
//! plans the ref from the facts of its rescue refs, records 1 event before
//! the effect, then deletes the ref with `git update-ref -d <ref> <tip>` and
//! reads it back. The role check of the caller is in the command
//! (`crates/horch/src/cmd/rescuecmd.rs`).

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::Command;

use horch_core::fleetd::integration::adapters::git::CliGitGateway;
use horch_core::fleetd::worktree_custody::adapters::git::CliWorktreeGit;
use horch_core::fleetd::worktree_custody::adapters::rescue_refs_git as refs_git;
use horch_core::fleetd::worktree_custody::application::rescue_refs::{self, Deleted, DropPlan};
use horch_core::fleetd::worktree_custody::domain::rescue_class::Class;
use horch_core::taskledger::Store;

const BRANCH: &str = "fleet/r1-T-1";

// ─── the world of a test ────────────────────────────────────────────────

struct World {
    _dir: tempfile::TempDir,
    root: PathBuf,
    store: Store,
    repo: PathBuf,
    /// The first commit of `main`.
    base: String,
}

/// A temp ledger, and a temp repository with 1 commit on `main` and the
/// branch [`BRANCH`] at that commit.
fn world() -> World {
    assert!(
        std::env::var_os("FLEET_ROOT").is_none(),
        "these tests use the fleet root of their temp ledger"
    );
    let dir = tempfile::tempdir().unwrap();
    let root = horch_platform::path::canonicalize(dir.path()).unwrap();
    let store = Store::open(&root.join("ledger.sqlite3")).unwrap();
    let repo = root.join("repo");
    std::fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    std::fs::write(repo.join("a.txt"), "a\n").unwrap();
    git(&repo, &["add", "a.txt"]);
    git(&repo, &["commit", "-q", "-m", "first"]);
    let base = git(&repo, &["rev-parse", "HEAD"]);
    git(&repo, &["branch", BRANCH]);
    World {
        _dir: dir,
        root,
        store,
        repo,
        base,
    }
}

/// Run git in `dir` for the test itself, with no config of the user, and
/// give its trimmed output. A failed call fails the test.
fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
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
        .unwrap();
    assert!(
        out.status.success(),
        "git {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

impl World {
    /// The git of the drop: it can write only the git folder of the
    /// repository.
    fn writer(&self) -> CliWorktreeGit {
        CliWorktreeGit::new("git", self.root.join("fleet")).writing(&[self.repo.join(".git")])
    }

    fn gateway(&self) -> CliGitGateway {
        CliGitGateway::new(self.writer())
    }

    fn repo_str(&self) -> String {
        self.repo.display().to_string()
    }

    /// The tip of `rev`, or `None` when it does not exist.
    fn tip(&self, rev: &str) -> Option<String> {
        let out = Command::new("git")
            .arg("-C")
            .arg(&self.repo)
            .args(["rev-parse", "--verify", "-q", rev])
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    /// Commit the file `name` on [`BRANCH`], write the rescue ref of that
    /// tip, and force the branch back to `main`: the ref is then the only
    /// holder of the work, so its class is `unique`. Gives the ref and its
    /// tip.
    fn unique_ref(&self, name: &str) -> (String, String) {
        git(&self.repo, &["checkout", "-q", BRANCH]);
        std::fs::write(self.repo.join(name), format!("{name}\n")).unwrap();
        git(&self.repo, &["add", name]);
        git(&self.repo, &["commit", "-q", "-m", name]);
        let tip = git(&self.repo, &["rev-parse", "HEAD"]);
        let rescue =
            refs_git::write_branch_rescue(&self.writer(), &self.repo, BRANCH, &tip).unwrap();
        git(&self.repo, &["checkout", "-q", "main"]);
        git(
            &self.repo,
            &["update-ref", &format!("refs/heads/{BRANCH}"), &self.base],
        );
        (rescue, tip)
    }

    /// The plan of the drop of `name`, from the facts of the rescue refs
    /// as the command reads them.
    fn plan(&self, name: &str) -> anyhow::Result<DropPlan> {
        let refs = refs_git::read_facts(&self.writer(), &self.repo, &|_| None).unwrap();
        rescue_refs::plan_drop(&self.repo_str(), name, &refs)
    }

    /// 1 whole drop of `name`, as the command runs it: plan, commit the
    /// event, then the effect.
    fn drop_ref(&mut self, name: &str) -> Deleted {
        let plan = self.plan(name).unwrap();
        self.store
            .immediate(|tx| rescue_refs::record_drop(tx, &plan))
            .unwrap();
        rescue_refs::drop_ref(&self.gateway(), &plan.deletion).unwrap()
    }

    /// The events about the ref `name`: actor, kind and data.
    fn events(&self, name: &str) -> Vec<(String, String, serde_json::Value)> {
        let mut stmt = self
            .store
            .conn()
            .prepare(
                "SELECT actor, kind, data_json FROM events
                 WHERE entity = 'worktree' AND entity_id = ?1 ORDER BY rowid",
            )
            .unwrap();
        stmt.query_map([name], |r| {
            let data: String = r.get(2)?;
            Ok((r.get(0)?, r.get(1)?, serde_json::from_str(&data).unwrap()))
        })
        .unwrap()
        .map(Result::unwrap)
        .collect()
    }
}

// ─── the drop ───────────────────────────────────────────────────────────

#[test]
fn drop_deletes_the_ref_and_verifies_it() {
    let mut w = world();
    let (name, tip) = w.unique_ref("b.txt");
    let (other, other_tip) = w.unique_ref("c.txt");

    let plan = w.plan(&name).unwrap();
    assert_eq!(plan.deletion.name, name);
    assert_eq!(plan.deletion.tip, tip);
    assert_eq!(plan.deletion.repo, w.repo_str());
    assert_eq!(plan.class, Class::Unique);
    assert!(plan.bytes.unwrap_or(0) > 0, "{plan:?}");

    assert_eq!(w.drop_ref(&name), Deleted::Deleted);
    assert_eq!(w.tip(&name), None);
    let events = w.events(&name);
    assert_eq!(events.len(), 1, "{events:?}");
    let (actor, kind, data) = &events[0];
    assert_eq!(actor, "operator");
    assert_eq!(kind, "note");
    assert_eq!(data["ref"], name.as_str());
    assert_eq!(data["tip"], tip.as_str());
    assert_eq!(data["class"], "unique");

    // Only the named ref goes: the other unique ref, its tip and the
    // branches stay.
    assert_eq!(w.tip(&other).as_deref(), Some(other_tip.as_str()));
    assert_eq!(w.tip("main").as_deref(), Some(w.base.as_str()));
    assert!(w.events(&other).is_empty());
}

#[test]
fn ref_outside_the_rescue_namespace_is_refused() {
    let w = world();
    let (name, tip) = w.unique_ref("b.txt");
    for outside in [
        "refs/heads/main".to_string(),
        format!("refs/heads/{BRANCH}"),
        "refs/horch/trash/x/1".into(),
        "refs/horch/rescue".into(),
        "refs/horch/rescue/../../heads/main/1".into(),
        "main".into(),
    ] {
        let err = w.plan(&outside).unwrap_err();
        assert!(
            format!("{err:#}").contains("refs/horch/rescue/"),
            "{outside}: {err:#}"
        );
    }
    // A name in the namespace that git does not have is refused too.
    let missing = name.replace("/1", "/9");
    assert!(w.plan(&missing).is_err());

    // Nothing changed.
    assert_eq!(w.tip(&name).as_deref(), Some(tip.as_str()));
    assert_eq!(w.tip("main").as_deref(), Some(w.base.as_str()));
    assert!(w.events(&name).is_empty());
}

#[test]
fn ref_that_moved_after_the_plan_is_not_deleted() {
    let w = world();
    let (name, _) = w.unique_ref("b.txt");
    let plan = w.plan(&name).unwrap();
    git(&w.repo, &["update-ref", &name, &w.base]);
    assert!(rescue_refs::drop_ref(&w.gateway(), &plan.deletion).is_err());
    assert_eq!(w.tip(&name).as_deref(), Some(w.base.as_str()));
}

// ─── the faults ─────────────────────────────────────────────────────────

#[test]
fn crash_before_the_commit_or_before_the_effect_loses_no_tip() {
    let mut w = world();
    let (first, _) = w.unique_ref("b.txt");
    let (second, _) = w.unique_ref("c.txt");
    let (kept, kept_tip) = w.unique_ref("d.txt");

    // Fault point 1: the drop dies before the commit of its event. Nothing
    // is written, and no effect runs.
    let plan = w.plan(&first).unwrap();
    {
        let tx = w.store.conn().unchecked_transaction().unwrap();
        assert!(rescue_refs::record_drop(&tx, &plan).unwrap());
        drop(tx);
    }
    assert!(w.events(&first).is_empty());
    assert!(w.tip(&first).is_some());
    // The next drop completes the effect 1 time.
    assert_eq!(w.drop_ref(&first), Deleted::Deleted);
    assert_eq!(w.tip(&first), None);
    assert_eq!(w.events(&first).len(), 1);

    // Fault point 2: the drop commits its event and dies before the effect.
    let plan = w.plan(&second).unwrap();
    assert!(w
        .store
        .immediate(|tx| rescue_refs::record_drop(tx, &plan))
        .unwrap());
    assert_eq!(w.events(&second).len(), 1);
    assert!(w.tip(&second).is_some());
    // The next drop writes no second event and completes the effect 1 time.
    let plan = w.plan(&second).unwrap();
    assert!(!w
        .store
        .immediate(|tx| rescue_refs::record_drop(tx, &plan))
        .unwrap());
    assert_eq!(
        rescue_refs::drop_ref(&w.gateway(), &plan.deletion).unwrap(),
        Deleted::Deleted
    );
    assert_eq!(w.tip(&second), None);
    assert_eq!(w.events(&second).len(), 1);
    // A second effect of the same plan finds the ref gone.
    assert_eq!(
        rescue_refs::drop_ref(&w.gateway(), &plan.deletion).unwrap(),
        Deleted::Gone
    );

    // No other tip and no other work is lost.
    assert_eq!(w.tip(&kept).as_deref(), Some(kept_tip.as_str()));
    assert_eq!(w.tip("main").as_deref(), Some(w.base.as_str()));
    assert_eq!(w.tip(BRANCH).as_deref(), Some(w.base.as_str()));
}
