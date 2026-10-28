//! The git calls the task ledger makes: run branch, worktrees, land, cleanup.
//!
//! Every call names its directory with `-C`; nothing depends on the current
//! directory. No call here ever forces: no `--force`, no `-D`, no `reset`.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};

/// A git binary.
#[derive(Debug, Clone)]
pub struct Git {
    bin: PathBuf,
}

/// What 1 git call printed.
#[derive(Debug, Clone)]
pub struct Out {
    pub ok: bool,
    pub stdout: String,
    pub stderr: String,
}

impl Git {
    pub fn new(bin: impl Into<PathBuf>) -> Git {
        Git { bin: bin.into() }
    }

    /// Run git in `dir`; never fails on a non-zero exit.
    pub fn try_run(&self, dir: &Path, args: &[&str]) -> Result<Out> {
        debug_assert!(
            !args
                .iter()
                .any(|a| *a == "--force" || *a == "-D" || *a == "-f"),
            "the task ledger never forces git"
        );
        let out = Command::new(&self.bin)
            .arg("-C")
            .arg(dir)
            .args(args)
            .env_remove("ANTHROPIC_API_KEY")
            .output()
            .with_context(|| format!("running {} {}", self.bin.display(), args.join(" ")))?;
        Ok(Out {
            ok: out.status.success(),
            stdout: String::from_utf8_lossy(&out.stdout).trim_end().to_string(),
            stderr: String::from_utf8_lossy(&out.stderr).trim_end().to_string(),
        })
    }

    /// Run git in `dir`; a non-zero exit is an error with git's message.
    pub fn run(&self, dir: &Path, args: &[&str]) -> Result<String> {
        let out = self.try_run(dir, args)?;
        if !out.ok {
            bail!(
                "git {} failed in {}: {}",
                args.join(" "),
                dir.display(),
                if out.stderr.is_empty() {
                    &out.stdout
                } else {
                    &out.stderr
                }
            );
        }
        Ok(out.stdout)
    }

    /// The main checkout of the repository `dir` is in (also from inside a
    /// linked worktree), or `None` outside a repository.
    pub fn main_root(&self, dir: &Path) -> Option<PathBuf> {
        let common = self
            .try_run(
                dir,
                &["rev-parse", "--path-format=absolute", "--git-common-dir"],
            )
            .ok()
            .filter(|o| o.ok)?
            .stdout;
        let common = PathBuf::from(common.trim());
        let root = if common.file_name().is_some_and(|n| n == ".git") {
            common.parent()?.to_path_buf()
        } else {
            common
        };
        Some(root.canonicalize().unwrap_or(root))
    }

    /// The branch checked out in `dir`, if any.
    pub fn current_branch(&self, dir: &Path) -> Option<String> {
        self.try_run(dir, &["symbolic-ref", "--quiet", "--short", "HEAD"])
            .ok()
            .filter(|o| o.ok && !o.stdout.is_empty())
            .map(|o| o.stdout)
    }

    /// Whether `name` is a local branch.
    pub fn branch_exists(&self, dir: &Path, name: &str) -> bool {
        self.try_run(
            dir,
            &[
                "show-ref",
                "--verify",
                "--quiet",
                &format!("refs/heads/{name}"),
            ],
        )
        .map(|o| o.ok)
        .unwrap_or(false)
    }

    /// Whether `remote` is configured.
    pub fn has_remote(&self, dir: &Path, remote: &str) -> bool {
        self.try_run(dir, &["remote", "get-url", remote])
            .map(|o| o.ok)
            .unwrap_or(false)
    }

    /// The commit `rev` names.
    pub fn rev(&self, dir: &Path, rev: &str) -> Result<String> {
        self.run(
            dir,
            &["rev-parse", "--verify", &format!("{rev}^{{commit}}")],
        )
    }

    /// The tree `HEAD` holds in `dir`: what a finish step judges.
    pub fn tree(&self, dir: &Path) -> Result<String> {
        self.run(dir, &["rev-parse", "--verify", "HEAD^{tree}"])
    }

    /// Whether `ancestor` is reachable from `of`.
    pub fn is_ancestor(&self, dir: &Path, ancestor: &str, of: &str) -> bool {
        self.try_run(dir, &["merge-base", "--is-ancestor", ancestor, of])
            .map(|o| o.ok)
            .unwrap_or(false)
    }

    /// The files `git status --porcelain` lists in `dir`: modified, staged and
    /// untracked (ignored files are not listed).
    pub fn dirty(&self, dir: &Path) -> Result<Vec<String>> {
        let out = self.run(dir, &["status", "--porcelain", "--untracked-files=all"])?;
        Ok(out
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| l.to_string())
            .collect())
    }

    /// The stash entries whose message names `branch`.
    pub fn stashes_on(&self, dir: &Path, branch: &str) -> Result<Vec<String>> {
        let out = self.run(dir, &["stash", "list"])?;
        let needle_on = format!("On {branch}:");
        let needle_wip = format!("WIP on {branch}:");
        Ok(out
            .lines()
            .filter(|l| l.contains(&needle_on) || l.contains(&needle_wip))
            .map(|l| l.to_string())
            .collect())
    }

    /// The commits on `branch` that `base` does not contain.
    pub fn commits_missing_from(
        &self,
        dir: &Path,
        branch: &str,
        base: &str,
    ) -> Result<Vec<String>> {
        let out = self.run(dir, &["rev-list", &format!("{base}..{branch}")])?;
        Ok(out.lines().map(|l| l.to_string()).collect())
    }
}

/// The size of a directory tree in bytes, following no links.
pub fn dir_bytes(path: &Path) -> u64 {
    fn walk(p: &Path) -> u64 {
        let Ok(meta) = std::fs::symlink_metadata(p) else {
            return 0;
        };
        if meta.is_dir() {
            std::fs::read_dir(p)
                .map(|rd| rd.flatten().map(|e| walk(&e.path())).sum())
                .unwrap_or(0)
        } else {
            meta.len()
        }
    }
    walk(path)
}
