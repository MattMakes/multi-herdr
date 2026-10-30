//! A temp repository and ledger for the worktree triage tests (#308).

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Command;

use horch_core::taskledger::git::Git;
use horch_core::taskledger::Store;

/// Run the host git in `dir`; a failure fails the test.
pub fn git(dir: &Path, args: &[&str]) -> String {
    let o = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("LC_ALL", "C")
        .output()
        .unwrap();
    assert!(
        o.status.success(),
        "git {}: {}",
        args.join(" "),
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

/// Write `file` in `dir`, commit it, and return the new HEAD.
pub fn commit(dir: &Path, file: &str, body: &str) -> String {
    write(dir, file, body);
    git(dir, &["add", file]);
    git(dir, &["commit", "--quiet", "-m", file]);
    git(dir, &["rev-parse", "HEAD"])
}

/// Write `file` in `dir`, making its folders.
pub fn write(dir: &Path, file: &str, body: &str) {
    let path = dir.join(file);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, body).unwrap();
}

/// A repository on `main` with 1 commit (`README.md`, `a.txt` and a
/// `.gitignore` of `target/`), and an empty ledger next to it.
pub struct World {
    pub tmp: tempfile::TempDir,
    pub top: PathBuf,
    pub root: PathBuf,
    pub store: Store,
    pub git: Git,
}

impl World {
    pub fn new() -> World {
        let tmp = tempfile::tempdir().unwrap();
        let top = tmp.path().canonicalize().unwrap();
        let root = top.join("repo");
        std::fs::create_dir_all(&root).unwrap();
        git(&root, &["init", "--quiet", "-b", "main"]);
        for (k, v) in [
            ("user.name", "t"),
            ("user.email", "t@example.com"),
            ("commit.gpgsign", "false"),
        ] {
            git(&root, &["config", k, v]);
        }
        write(&root, ".gitignore", "target/\n");
        write(&root, "a.txt", "1\n");
        commit(&root, "README.md", "base\n");
        git(&root, &["add", ".gitignore", "a.txt"]);
        git(&root, &["commit", "--quiet", "--amend", "--no-edit"]);
        let store = Store::open(&top.join("ledger.sqlite3")).unwrap();
        World {
            tmp,
            top,
            root,
            store,
            git: Git::new("git"),
        }
    }

    /// A linked worktree `.worktrees/<name>` on a new branch `branch` from `from`.
    pub fn worktree(&self, name: &str, branch: &str, from: &str) -> PathBuf {
        let path = self.root.join(".worktrees").join(name);
        git(
            &self.root,
            &[
                "worktree",
                "add",
                "--quiet",
                "-b",
                branch,
                &path.to_string_lossy(),
                from,
            ],
        );
        path
    }

    pub fn rev(&self, rev: &str) -> String {
        git(&self.root, &["rev-parse", rev])
    }

    /// Whether `refname` exists.
    pub fn has_ref(&self, refname: &str) -> bool {
        Command::new("git")
            .arg("-C")
            .arg(&self.root)
            .args(["show-ref", "--verify", "--quiet", refname])
            .status()
            .unwrap()
            .success()
    }
}
