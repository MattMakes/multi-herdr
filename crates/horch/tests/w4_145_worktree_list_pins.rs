//! `horch worktree list`: the lines it prints for worktrees with no claim.
//!
//! The command prints to stdout, so the test runs the real binary against a
//! temp git repo with 2 changed worktrees under `.worktrees/`: `kept` is a
//! branch with 1 commit ahead of `main`, and `loose` is a detached checkout
//! with 1 untracked file.

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

struct World {
    _tmp: tempfile::TempDir,
    root: PathBuf,
    repo: PathBuf,
}

fn text(o: &Output) -> String {
    format!(
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

/// `git <args>` in `dir`, with no config of the operator's.
fn git(root: &Path, dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
        .args(["-c", "commit.gpgsign=false"])
        .args(args)
        .current_dir(dir)
        .env("HOME", root.join("home"))
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .expect("running git");
    assert!(out.status.success(), "git {args:?}: {}", text(&out));
}

impl World {
    fn new() -> World {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_path_buf();
        let repo = root.join("repo");
        std::fs::create_dir_all(root.join("home")).unwrap();
        std::fs::create_dir_all(&repo).unwrap();
        git(&root, &repo, &["init", "--quiet", "-b", "main"]);
        std::fs::write(repo.join("a.txt"), "a\n").unwrap();
        git(&root, &repo, &["add", "a.txt"]);
        git(&root, &repo, &["commit", "--quiet", "-m", "first"]);

        let kept = repo.join(".worktrees/kept");
        git(
            &root,
            &repo,
            &[
                "worktree",
                "add",
                "--quiet",
                "-b",
                "kept",
                kept.to_str().unwrap(),
                "main",
            ],
        );
        std::fs::write(kept.join("k.txt"), "k\n").unwrap();
        git(&root, &kept, &["add", "k.txt"]);
        git(&root, &kept, &["commit", "--quiet", "-m", "kept work"]);

        let loose = repo.join(".worktrees/loose");
        git(
            &root,
            &repo,
            &[
                "worktree",
                "add",
                "--quiet",
                "--detach",
                loose.to_str().unwrap(),
                "main",
            ],
        );
        std::fs::write(loose.join("x.txt"), "x\n").unwrap();
        World {
            _tmp: tmp,
            root,
            repo,
        }
    }

    fn list(&self) -> Output {
        self.horch(&["worktree", "list"])
    }

    fn horch(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_horch"))
            .args(args)
            .current_dir(&self.repo)
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("HOME", self.root.join("home"))
            .env("HORCH_STATE_DIR", self.root.join("state"))
            .env("HORCH_PROJECT_DIR", &self.repo)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap()
    }
}

/// Each worktree with no claim is 1 line: its path, its branch (`detached`
/// for none), the commits it is ahead of the branch it was scanned from, and
/// the lines of `git status --porcelain`. The section is headed `unregistered:`.
#[test]
fn list_prints_each_unregistered_worktree_as_one_line() {
    let w = World::new();
    let out = w.list();
    assert!(out.status.success(), "{}", text(&out));
    assert!(out.stderr.is_empty(), "{}", text(&out));
    let stdout = String::from_utf8(out.stdout).unwrap();
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 3, "{stdout}");
    assert_eq!(lines[0], "unregistered:");
    assert!(
        lines[1].ends_with(".worktrees/kept branch=kept ahead=1 dirty=0"),
        "{stdout}"
    );
    assert!(
        lines[2].ends_with(".worktrees/loose branch=detached ahead=0 dirty=1"),
        "{stdout}"
    );
}

/// With nothing to report the section is 1 line.
#[test]
fn list_says_none_when_every_worktree_is_clean() {
    let w = World::new();
    std::fs::remove_file(w.repo.join(".worktrees/loose/x.txt")).unwrap();
    git(
        &w.root,
        &w.repo.join(".worktrees/kept"),
        &["reset", "--quiet", "--hard", "main"],
    );
    let out = w.list();
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "unregistered: none\n");
}

/// A claim made by `worktree new` is a line of its own before the section of
/// the unregistered worktrees: its name, role, state (`open` until it lands),
/// branch, base, the commits it is ahead, the lines of `git status`, and
/// whether its branch is in the base.
#[test]
fn list_prints_a_claim_with_its_state() {
    let w = World::new();
    let made = w.horch(&["worktree", "new", "mine"]);
    assert!(made.status.success(), "{}", text(&made));
    let out = w.list();
    assert!(out.status.success(), "{}", text(&out));
    let stdout = String::from_utf8(out.stdout).unwrap();
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 4, "{stdout}");
    assert_eq!(
        lines[0],
        "mine orchestrator open wt/orchestrator-mine main ahead=0 dirty=0 contained=true"
    );
    assert_eq!(lines[1], "unregistered:");
}
