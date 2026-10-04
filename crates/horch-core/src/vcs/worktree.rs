//! Candidate worktrees (phase B3): one git worktree per candidate, all from
//! one base commit, on a branch the round owns.
//!
//! Every step is idempotent, so a coordinator that crashed between two steps
//! can run the same step again: `create` returns the worktree it made before,
//! `freeze` with nothing new to commit returns the same head, and `remove` of
//! a removed worktree is a no-op.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context};
use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};

use crate::ids::ExecutionId;
use crate::measure::digest::Digest;
use crate::measure::NumstatLine;
use crate::vcs::git::{GitClient, GitIdentity};

/// The identity of every freeze commit. With the date fixed by the caller,
/// the same edits on the same base give the same commit hash (CMP-08).
pub(crate) const FREEZE_NAME: &str = "multi-herdr-dataset";
pub(crate) const FREEZE_EMAIL: &str = "dataset@multi-herdr.invalid";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeSpec {
    pub repo: PathBuf,
    pub root: PathBuf,
    pub exp8: String,
    pub round_index: u32,
    pub label: String,
    pub base_sha: String,
}

impl WorktreeSpec {
    /// `mh/exp/<exp8>/r<idx>/<label>`.
    pub fn branch(&self) -> String {
        format!("mh/exp/{}/r{}/{}", self.exp8, self.round_index, self.label)
    }

    /// `<root>/<label>`.
    pub fn path(&self) -> PathBuf {
        self.root.join(&self.label)
    }

    /// The label and the experiment prefix become a path component and a
    /// ref component, so both are held to a plain character set.
    fn check(&self) -> anyhow::Result<()> {
        for (what, v) in [("label", &self.label), ("exp8", &self.exp8)] {
            let plain = v
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_');
            if v.is_empty() || !plain || v.starts_with('-') {
                bail!("candidate {what} '{v}' must be [A-Za-z0-9_-] and not start with '-'");
            }
        }
        if self.base_sha.is_empty() || !self.base_sha.bytes().all(|c| c.is_ascii_hexdigit()) {
            bail!("base_sha '{}' is not a commit hash", self.base_sha);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FrozenCandidate {
    pub label: String,
    pub execution_id: ExecutionId,
    pub worktree: PathBuf,
    pub branch: String,
    pub base_sha: String,
    pub head_sha: String,
    pub numstat: Vec<NumstatLine>,
    /// sha256 of the whole `base..head` patch, uncapped.
    pub diff_digest: Digest,
    pub frozen_at: String,
}

pub struct WorktreeManager<'a, G: GitClient> {
    pub git: &'a G,
}

/// Whether `a` and `b` name the same directory. Git prints resolved paths
/// (`/private/var/...` on macOS), the caller may not.
fn same_dir(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

impl<'a, G: GitClient> WorktreeManager<'a, G> {
    /// Create the candidate's worktree, or return it when it is already
    /// there on its branch at or after `base_sha`. A path that holds anything
    /// else is an error, never overwritten.
    pub fn create(&self, spec: &WorktreeSpec) -> anyhow::Result<PathBuf> {
        spec.check()?;
        let branch = spec.branch();
        let path = spec.path();
        let existing = self
            .git
            .worktree_list(&spec.repo)?
            .into_iter()
            .find(|(p, _)| same_dir(p, &path));
        if let Some((_, on)) = existing {
            if on.as_deref() != Some(branch.as_str()) {
                bail!(
                    "{} is a worktree on {}, not on {branch}",
                    path.display(),
                    on.as_deref().unwrap_or("a detached HEAD")
                );
            }
            let head = self.git.head(&path)?;
            if !self.git.is_ancestor(&path, &spec.base_sha, &head)? {
                bail!(
                    "{} is at {head}, which does not contain base {}",
                    path.display(),
                    spec.base_sha
                );
            }
            return Ok(path);
        }
        if path.exists() {
            bail!(
                "{} exists and is not a worktree of {}",
                path.display(),
                spec.repo.display()
            );
        }
        // A branch kept by an earlier `remove` is checked out again, but only
        // when it still grows from the same base.
        if let Some(tip) = self
            .git
            .rev_parse(&spec.repo, &format!("refs/heads/{branch}"))?
        {
            if !self.git.is_ancestor(&spec.repo, &spec.base_sha, &tip)? {
                bail!(
                    "branch {branch} is at {tip}, which does not contain base {}",
                    spec.base_sha
                );
            }
        }
        std::fs::create_dir_all(&spec.root)
            .with_context(|| format!("creating {}", spec.root.display()))?;
        self.git
            .worktree_add(&spec.repo, &path, &branch, &spec.base_sha)?;
        Ok(path)
    }

    /// Commit everything in the worktree as the dataset identity at `at`
    /// and record what the candidate changed against its base.
    pub fn freeze(
        &self,
        spec: &WorktreeSpec,
        execution: &ExecutionId,
        at: DateTime<Utc>,
    ) -> anyhow::Result<FrozenCandidate> {
        spec.check()?;
        let branch = spec.branch();
        let path = spec.path();
        let on = self.git.current_branch(&path)?;
        if on.as_deref() != Some(branch.as_str()) {
            bail!(
                "{} is on {}, not on {branch}",
                path.display(),
                on.as_deref().unwrap_or("a detached HEAD")
            );
        }
        let frozen_at = at.to_rfc3339_opts(SecondsFormat::Secs, true);
        let id = GitIdentity {
            name: FREEZE_NAME.to_owned(),
            email: FREEZE_EMAIL.to_owned(),
            date: frozen_at.clone(),
        };
        let message = format!("candidate {} frozen", spec.label);
        self.git.commit_all(&path, &message, &id)?;
        // The candidate controls its worktree, including its `.git` file, so
        // everything recorded is read from the main repository: the branch
        // tip there must be what the worktree reports.
        let head_sha = self.git.head(&path)?;
        let tip = self
            .git
            .rev_parse(&spec.repo, &format!("refs/heads/{branch}"))?;
        if tip.as_deref() != Some(head_sha.as_str()) {
            bail!(
                "candidate {} worktree reports {head_sha}, but {branch} in {} is at {}",
                spec.label,
                spec.repo.display(),
                tip.as_deref().unwrap_or("nothing")
            );
        }
        let repo = &spec.repo;
        if !self.git.is_ancestor(repo, &spec.base_sha, &head_sha)? {
            bail!(
                "candidate {} head {head_sha} does not contain base {}",
                spec.label,
                spec.base_sha
            );
        }
        Ok(FrozenCandidate {
            label: spec.label.clone(),
            execution_id: execution.clone(),
            worktree: path.clone(),
            branch,
            base_sha: spec.base_sha.clone(),
            numstat: self.git.diff_numstat(repo, &spec.base_sha, &head_sha)?,
            diff_digest: self.git.diff_digest(repo, &spec.base_sha, &head_sha)?,
            head_sha,
            frozen_at,
        })
    }

    /// Remove the worktree with `--force`. The branch stays, so the frozen
    /// commit stays reachable.
    pub fn remove(&self, spec: &WorktreeSpec) -> anyhow::Result<()> {
        spec.check()?;
        let path = spec.path();
        let listed = self
            .git
            .worktree_list(&spec.repo)?
            .into_iter()
            .any(|(p, _)| same_dir(&p, &path));
        if listed {
            self.git.worktree_remove(&spec.repo, &path, true)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(label: &str) -> WorktreeSpec {
        WorktreeSpec {
            repo: "/r".into(),
            root: "/w".into(),
            exp8: "0a1b2c3d".into(),
            round_index: 2,
            label: label.into(),
            base_sha: "abc123".into(),
        }
    }

    #[test]
    fn branch_and_path_follow_the_layout() {
        let s = spec("c1");
        assert_eq!(s.branch(), "mh/exp/0a1b2c3d/r2/c1");
        assert_eq!(s.path(), PathBuf::from("/w/c1"));
        assert!(s.check().is_ok());
    }

    #[test]
    fn unsafe_labels_are_refused() {
        for bad in ["", "-x", "a/b", "..", "a b", "a..b"] {
            assert!(spec(bad).check().is_err(), "{bad:?}");
        }
        let mut s = spec("c1");
        s.base_sha = "HEAD~1".into();
        assert!(s.check().is_err());
    }
}
