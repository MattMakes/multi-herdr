//! The git gateway of integration: compare-and-set ref moves, the by-content
//! check of I8, the candidate of a bundle, the fleet branch and stash reads,
//! and the git settings of fleetd (docs/specs/fleetd/design.md, "No work is
//! lost", I8, I9, I11, I12, "The bundle", "fleetd sets the git settings that
//! it needs"; docs/tickets/fleetd-p4/23-integration-git-gateway.md).
//!
//! Each test makes its own temporary git repository. No test touches this
//! repository.

use std::path::{Path, PathBuf};
use std::process::Command;

use horch_core::fleetd::integration::adapters::git::CliGitGateway;
use horch_core::fleetd::integration::application::ports::{
    BranchTip, Candidate, IntegrationGitGateway, Member, RefMove,
};
use horch_core::fleetd::worktree_custody::adapters::git::CliWorktreeGit;
use horch_marketplace::git::scrub_repo_env;

struct World {
    _dir: tempfile::TempDir,
    root: PathBuf,
    repo: PathBuf,
    /// The first commit of `main`.
    base: String,
}

fn world() -> World {
    let dir = tempfile::tempdir().unwrap();
    let root = horch_platform::path::canonicalize(dir.path()).unwrap();
    let repo = root.join("repo");
    std::fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    std::fs::write(repo.join("a.txt"), "a\n").unwrap();
    std::fs::write(repo.join("b.txt"), "b\n").unwrap();
    git(&repo, &["add", "a.txt", "b.txt"]);
    git(&repo, &["commit", "-q", "-m", "first"]);
    let base = git(&repo, &["rev-parse", "HEAD"]);
    World {
        _dir: dir,
        root,
        repo,
        base,
    }
}

impl World {
    /// The gateway of fleetd: it can write only the git folder of the
    /// repository.
    fn gateway(&self) -> CliGitGateway {
        CliGitGateway::new(
            CliWorktreeGit::new("git", self.root.join("fleet")).writing(&[self.repo.join(".git")]),
        )
    }

    fn tip(&self, rev: &str) -> String {
        git(&self.repo, &["rev-parse", rev])
    }

    /// Check out `branch` (made from `from` when it is new), write each
    /// file of `files`, and commit them. Returns the new tip.
    fn commit_on(&self, branch: &str, from: &str, files: &[(&str, &str)], msg: &str) -> String {
        if git_out(&self.repo, &["rev-parse", "--verify", "-q", branch])
            .status
            .success()
        {
            git(&self.repo, &["checkout", "-q", branch]);
        } else {
            git(&self.repo, &["checkout", "-q", "-b", branch, from]);
        }
        for (name, text) in files {
            std::fs::write(self.repo.join(name), text).unwrap();
            git(&self.repo, &["add", name]);
        }
        git(&self.repo, &["commit", "-q", "-m", msg]);
        let tip = self.tip("HEAD");
        git(&self.repo, &["checkout", "-q", "main"]);
        tip
    }

    /// The number of entries in the reflog of `name`: 1 for each move.
    fn moves(&self, name: &str) -> usize {
        git(&self.repo, &["reflog", "show", "--format=%H", name])
            .lines()
            .count()
    }

    fn is_ancestor(&self, a: &str, b: &str) -> bool {
        git_out(&self.repo, &["merge-base", "--is-ancestor", a, b])
            .status
            .success()
    }
}

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
    let mut cmd = Command::new("git");
    cmd.arg("-C")
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
        .env("GIT_CONFIG_NOSYSTEM", "1");
    scrub_repo_env(&mut cmd);
    cmd.output().unwrap()
}

const BRANCH: &str = "refs/heads/fleet/f";

// ─── I9: compare-and-set ref moves ─────────────────────────────────────

#[test]
fn update_ref_moves_only_from_the_expected_old_tip() {
    let w = world();
    let gw = w.gateway();
    let next = w.commit_on("fleet/f", &w.base, &[("a.txt", "f\n")], "f");
    git(&w.repo, &["update-ref", BRANCH, &w.base]);

    let moved = gw
        .update_ref(&w.repo, BRANCH, &next, Some(&w.base))
        .unwrap();

    assert_eq!(moved, RefMove::Moved);
    assert_eq!(gw.tip(&w.repo, BRANCH).unwrap(), Some(next.clone()));

    // A new ref is made only when it does not exist.
    let candidate = "refs/horch/bundle/1";
    assert_eq!(
        gw.update_ref(&w.repo, candidate, &next, None).unwrap(),
        RefMove::Moved
    );
    assert_eq!(
        gw.update_ref(&w.repo, candidate, &w.base, None).unwrap(),
        RefMove::Mismatch {
            actual: Some(next.clone())
        }
    );
    assert_eq!(w.tip(candidate), next);

    // A delete is a compare-and-set too.
    assert_eq!(
        gw.delete_ref(&w.repo, candidate, &next).unwrap(),
        RefMove::Moved
    );
    assert_eq!(gw.tip(&w.repo, candidate).unwrap(), None);
}

#[test]
fn update_ref_with_a_moved_tip_moves_nothing_and_returns_the_tip() {
    let w = world();
    let gw = w.gateway();
    let ours = w.commit_on("fleet/f", &w.base, &[("a.txt", "ours\n")], "ours");
    // Someone else moves the branch after fleetd saw it at the base.
    git(&w.repo, &["update-ref", BRANCH, &w.base]);
    let theirs = w.commit_on("fleet/f", &w.base, &[("b.txt", "theirs\n")], "theirs");
    let before = w.moves(BRANCH);

    let moved = gw
        .update_ref(&w.repo, BRANCH, &ours, Some(&w.base))
        .unwrap();

    assert_eq!(
        moved,
        RefMove::Mismatch {
            actual: Some(theirs.clone())
        }
    );
    assert_eq!(w.tip(BRANCH), theirs);
    assert_eq!(w.moves(BRANCH), before, "no move is recorded");

    // A delete on a moved tip deletes nothing.
    assert_eq!(
        gw.delete_ref(&w.repo, BRANCH, &w.base).unwrap(),
        RefMove::Mismatch {
            actual: Some(theirs.clone())
        }
    );
    assert_eq!(w.tip(BRANCH), theirs);

    // A ref that is gone is a mismatch with no tip.
    assert_eq!(
        gw.update_ref(&w.repo, "refs/heads/fleet/gone", &ours, Some(&w.base))
            .unwrap(),
        RefMove::Mismatch { actual: None }
    );
}

#[test]
fn update_ref_refuses_a_short_name_and_a_revision_that_is_not_an_id() {
    let w = world();
    let gw = w.gateway();

    assert!(gw.update_ref(&w.repo, "fleet/f", &w.base, None).is_err());
    assert!(gw.update_ref(&w.repo, "--stdin", &w.base, None).is_err());
    assert!(gw.update_ref(&w.repo, BRANCH, "main", None).is_err());
    assert!(gw
        .update_ref(&w.repo, BRANCH, &w.base, Some("HEAD~1"))
        .is_err());
    assert_eq!(gw.tip(&w.repo, BRANCH).unwrap(), None);
}

// ─── I8: the by-content check ──────────────────────────────────────────

/// A branch that changes `a.txt` and adds `c.txt`, a main that changes
/// `a.txt` too, and a squash merge of the branch into main with the
/// conflict on `a.txt` resolved to `resolved`. Returns the branch tip.
fn squash_with_conflict(w: &World, resolved: &str) -> String {
    let branch = w.commit_on(
        "fleet/t",
        &w.base,
        &[("a.txt", "branch\n"), ("c.txt", "c\n")],
        "task",
    );
    w.commit_on("main", "main", &[("a.txt", "main\n")], "main moves");
    let merge = git_out(&w.repo, &["merge", "--squash", "fleet/t"]);
    assert!(!merge.status.success(), "the squash merge conflicts");
    std::fs::write(w.repo.join("a.txt"), resolved).unwrap();
    git(&w.repo, &["add", "a.txt"]);
    git(&w.repo, &["commit", "-q", "-m", "squash fleet/t"]);
    branch
}

#[test]
fn i8_squash_merge_with_a_conflict_resolution_is_seen_as_landed() {
    let w = world();
    let branch = squash_with_conflict(&w, "branch\n");
    let target = w.tip("main");
    assert!(!w.is_ancestor(&branch, &target), "a squash keeps no commit");

    assert!(w
        .gateway()
        .changes_in(&w.repo, &branch, &w.base, &target)
        .unwrap());
}

#[test]
fn i8_branch_with_a_change_missing_in_the_target_is_not_landed() {
    let w = world();
    // Q14: the resolution kept main's text, so the change of the branch
    // to a.txt is not in the target.
    let branch = squash_with_conflict(&w, "main\n");
    let target = w.tip("main");
    let gw = w.gateway();

    assert!(!gw.changes_in(&w.repo, &branch, &w.base, &target).unwrap());

    // A file that the branch deletes and the target keeps is a change that
    // is missing too.
    git(&w.repo, &["checkout", "-q", "-b", "fleet/d", &w.base]);
    git(&w.repo, &["rm", "-q", "b.txt"]);
    git(&w.repo, &["commit", "-q", "-m", "delete b"]);
    let deleting = w.tip("HEAD");
    git(&w.repo, &["checkout", "-q", "main"]);

    assert!(!gw.changes_in(&w.repo, &deleting, &w.base, &target).unwrap());
    assert!(!gw.changes_in(&w.repo, &deleting, &w.base, &w.base).unwrap());
}

#[test]
fn i8_merged_branch_is_landed_after_the_target_changes_its_file() {
    let w = world();
    let branch = w.commit_on("fleet/t", &w.base, &[("a.txt", "branch\n")], "task");
    git(
        &w.repo,
        &["merge", "-q", "--no-ff", "-m", "merge", "fleet/t"],
    );
    let target = w.commit_on("main", "main", &[("a.txt", "later\n")], "later");

    assert!(w
        .gateway()
        .changes_in(&w.repo, &branch, &w.base, &target)
        .unwrap());
}

// ─── Q15: the candidate of a bundle ────────────────────────────────────

fn member(name: &str, tip: &str) -> Member {
    Member {
        name: name.to_string(),
        tip: tip.to_string(),
    }
}

#[test]
fn candidate_of_3_members_keeps_each_commit() {
    let w = world();
    let mut members = Vec::new();
    let mut commits = Vec::new();
    for n in 1..=3 {
        let name = format!("fleet/t{n}");
        let file = format!("t{n}.txt");
        let first = w.commit_on(&name, &w.base, &[(&file, "1\n")], "first");
        let second = w.commit_on(&name, &w.base, &[(&file, "2\n")], "second");
        commits.extend([first, second.clone()]);
        members.push(member(&name, &second));
    }
    let head = w.tip("HEAD");
    let refs = git(&w.repo, &["for-each-ref"]);

    let built = w
        .gateway()
        .build_candidate(&w.repo, &w.base, &members)
        .unwrap();

    let Candidate::Built { tip } = built else {
        panic!("a clean candidate: {built:?}");
    };
    for c in &commits {
        assert!(w.is_ancestor(c, &tip), "commit {c} is kept");
    }
    // 1 merge commit for each member, in queue order.
    let chain = git(&w.repo, &["rev-list", "--first-parent", &tip]);
    let chain: Vec<&str> = chain.lines().collect();
    assert_eq!(chain.len(), 4);
    assert_eq!(chain[3], w.base);
    for (n, m) in members.iter().enumerate() {
        let merge = chain[2 - n];
        let parents = git(&w.repo, &["rev-list", "--parents", "-n", "1", merge]);
        let parents: Vec<&str> = parents.split(' ').skip(1).collect();
        assert_eq!(parents, [chain[3 - n], m.tip.as_str()], "merge {n}");
    }
    for n in 1..=3 {
        let blob = format!("{tip}:t{n}.txt");
        assert_eq!(git(&w.repo, &["cat-file", "blob", &blob]), "2");
    }
    // No checkout, no worktree, no ref.
    assert_eq!(w.tip("HEAD"), head);
    assert_eq!(git(&w.repo, &["status", "--porcelain"]), "");
    assert_eq!(git(&w.repo, &["worktree", "list"]).lines().count(), 1);
    assert_eq!(git(&w.repo, &["for-each-ref"]), refs);
}

#[test]
fn candidate_with_a_conflict_names_the_member_and_its_files() {
    let w = world();
    let one = w.commit_on("fleet/t1", &w.base, &[("x.txt", "one\n")], "one");
    let two = w.commit_on(
        "fleet/t2",
        &w.base,
        &[("x.txt", "two\n"), ("a.txt", "two\n")],
        "two",
    );
    let three = w.commit_on("fleet/t3", &w.base, &[("y.txt", "y\n")], "three");
    let members = [
        member("fleet/t1", &one),
        member("fleet/t2", &two),
        member("fleet/t3", &three),
    ];

    let built = w
        .gateway()
        .build_candidate(&w.repo, &w.base, &members)
        .unwrap();

    assert_eq!(
        built,
        Candidate::Conflict {
            member: "fleet/t2".to_string(),
            files: vec!["x.txt".to_string()],
        }
    );
}

// ─── the reads ─────────────────────────────────────────────────────────

#[test]
fn stash_list_names_its_branch() {
    let w = world();
    w.commit_on("fleet/s", &w.base, &[("s.txt", "s\n")], "s");
    git(&w.repo, &["checkout", "-q", "fleet/s"]);
    std::fs::write(w.repo.join("s.txt"), "stashed\n").unwrap();
    git(&w.repo, &["stash", "push", "-q", "-m", "keep: this"]);
    git(&w.repo, &["checkout", "-q", "main"]);
    std::fs::write(w.repo.join("a.txt"), "wip\n").unwrap();
    git(&w.repo, &["stash", "push", "-q"]);
    let gw = w.gateway();

    assert_eq!(
        gw.stash_branches(&w.repo).unwrap(),
        ["fleet/s".to_string(), "main".to_string()]
    );

    let empty = world();
    assert!(empty
        .gateway()
        .stash_branches(&empty.repo)
        .unwrap()
        .is_empty());
}

#[test]
fn fleet_branches_and_the_fleet_toml_of_a_tip_are_read() {
    let w = world();
    let f = w.commit_on("fleet/f", &w.base, &[("fleet.toml", "[gates]\n")], "f");
    let g = w.commit_on("fleet/run/g", &w.base, &[("g.txt", "g\n")], "g");
    w.commit_on("other", &w.base, &[("o.txt", "o\n")], "o");
    let gw = w.gateway();

    assert_eq!(
        gw.fleet_branches(&w.repo).unwrap(),
        [
            BranchTip {
                name: "fleet/f".to_string(),
                tip: f.clone()
            },
            BranchTip {
                name: "fleet/run/g".to_string(),
                tip: g.clone()
            },
        ]
    );
    assert_eq!(
        gw.fleet_toml(&w.repo, &f).unwrap(),
        Some("[gates]\n".to_string())
    );
    assert_eq!(gw.fleet_toml(&w.repo, &g).unwrap(), None);
}

// ─── the settings of fleetd ────────────────────────────────────────────

#[test]
fn fleet_settings_are_set_in_the_repository() {
    let w = world();
    git(&w.repo, &["config", "merge.conflictStyle", "merge"]);
    let tree = w.root.join("tree");
    git(
        &w.repo,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "fleet/w",
            tree.to_str().unwrap(),
        ],
    );
    let gw = w.gateway();

    gw.ensure_settings(&w.repo).unwrap();
    gw.ensure_settings(&tree).unwrap();

    for dir in [&w.repo, &tree] {
        assert_eq!(git(dir, &["config", "rerere.enabled"]), "true");
        assert_eq!(git(dir, &["config", "merge.conflictStyle"]), "zdiff3");
    }
}

// ─── the crash test of a ref move ──────────────────────────────────────

/// The rows of the ledger that a ref move needs: the tip that fleetd last
/// saw, and the intended move, committed before its effect.
#[derive(Debug, Clone, Default)]
struct Rows {
    seen_tip: String,
    intent: Option<(String, String)>,
    failed: usize,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Fault {
    None,
    /// The process stops before the commit of the intended row.
    BeforeCommit,
    /// The process stops after the commit and before the effect.
    BeforeEffect,
    /// The process stops after the effect and before its outcome is
    /// recorded.
    BeforeOutcome,
}

/// 1 pass: it ends an intended move of a row first, else it decides to
/// move the branch to `next`. Returns `false` when the fault stops it.
fn pass(
    gw: &dyn IntegrationGitGateway,
    repo: &Path,
    rows: &mut Rows,
    next: &str,
    fault: Fault,
) -> bool {
    if rows.intent.is_none() {
        if rows.seen_tip == next {
            return true;
        }
        if fault == Fault::BeforeCommit {
            return false;
        }
        rows.intent = Some((next.to_string(), rows.seen_tip.clone()));
    }
    if fault == Fault::BeforeEffect {
        return false;
    }
    let (new, old) = rows.intent.clone().unwrap();
    let moved = gw.update_ref(repo, BRANCH, &new, Some(&old)).unwrap();
    if fault == Fault::BeforeOutcome {
        return false;
    }
    rows.intent = None;
    match moved {
        RefMove::Moved => rows.seen_tip = new,
        RefMove::Mismatch { actual } if actual.as_deref() == Some(new.as_str()) => {
            rows.seen_tip = new;
        }
        RefMove::Mismatch { actual } => {
            rows.failed += 1;
            rows.seen_tip = actual.unwrap_or_default();
        }
    }
    true
}

#[test]
fn crash_at_each_fault_point_of_a_ref_move_moves_the_ref_once_and_loses_no_tip() {
    for fault in [
        Fault::BeforeCommit,
        Fault::BeforeEffect,
        Fault::BeforeOutcome,
    ] {
        let w = world();
        let gw = w.gateway();
        let next = w.commit_on("fleet/f", &w.base, &[("a.txt", "f\n")], "f");
        git(&w.repo, &["update-ref", BRANCH, &w.base]);
        let before = w.moves(BRANCH);
        let mut rows = Rows {
            seen_tip: w.base.clone(),
            ..Rows::default()
        };

        assert!(!pass(&gw, &w.repo, &mut rows, &next, fault), "{fault:?}");
        assert!(pass(&gw, &w.repo, &mut rows, &next, Fault::None));
        assert!(pass(&gw, &w.repo, &mut rows, &next, Fault::None));

        assert_eq!(w.tip(BRANCH), next, "{fault:?}");
        assert_eq!(w.moves(BRANCH), before + 1, "{fault:?}: 1 move");
        assert_eq!(rows.seen_tip, next);
        assert!(rows.intent.is_none());
        assert_eq!(rows.failed, 0);
        assert!(w.is_ancestor(&w.base, &next));
    }
}

#[test]
fn crash_before_the_effect_on_a_moved_tip_fails_the_move_once_and_keeps_the_tip() {
    let w = world();
    let gw = w.gateway();
    let next = w.commit_on("fleet/f", &w.base, &[("a.txt", "f\n")], "f");
    git(&w.repo, &["update-ref", BRANCH, &w.base]);
    let mut rows = Rows {
        seen_tip: w.base.clone(),
        ..Rows::default()
    };
    assert!(!pass(&gw, &w.repo, &mut rows, &next, Fault::BeforeEffect));
    // While fleetd is down, someone moves the branch.
    let theirs = w.commit_on("fleet/f", &w.base, &[("b.txt", "theirs\n")], "theirs");
    let before = w.moves(BRANCH);

    assert!(pass(&gw, &w.repo, &mut rows, &next, Fault::None));

    assert_eq!(rows.failed, 1);
    assert_eq!(rows.seen_tip, theirs);
    assert!(rows.intent.is_none());
    assert_eq!(w.tip(BRANCH), theirs);
    assert_eq!(w.moves(BRANCH), before);
}

// ─── the fleetd git rule (Q26) ─────────────────────────────────────────

#[test]
fn candidate_build_runs_no_merge_driver_of_the_project() {
    let w = world();
    let one = w.commit_on("fleet/t1", &w.base, &[("x.txt", "one\n")], "one");
    let two = w.commit_on("fleet/t2", &w.base, &[("x.txt", "two\n")], "two");
    let marker = w.root.join("driver-ran");
    let driver = format!("touch {} && false", marker.display());
    git(&w.repo, &["config", "merge.evil.driver", &driver]);
    std::fs::write(w.repo.join(".git/info/attributes"), "* merge=evil\n").unwrap();
    let members = [member("fleet/t1", &one), member("fleet/t2", &two)];

    let built = w.gateway().build_candidate(&w.repo, &w.base, &members);

    let error = format!("{:#}", built.unwrap_err());
    assert!(error.contains("merge.evil.driver"), "{error}");
    assert!(!marker.exists(), "the merge driver of the project ran");
}
