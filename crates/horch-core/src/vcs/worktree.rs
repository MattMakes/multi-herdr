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
    a.canonicalize()
        .ok()
        .zip(b.canonicalize().ok())
        .map_or(a == b, |(a, b)| a == b)
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
        match existing {
            Some((_, on)) => self.reuse(spec, path, on.as_deref(), &branch),
            None => self.add_new(spec, path, &branch),
        }
    }

    /// Return the worktree already at `path` when it is on `branch` at or
    /// after the base.
    fn reuse(
        &self,
        spec: &WorktreeSpec,
        path: PathBuf,
        on: Option<&str>,
        branch: &str,
    ) -> anyhow::Result<PathBuf> {
        if on != Some(branch) {
            bail!(
                "{} is a worktree on {}, not on {branch}",
                path.display(),
                on.unwrap_or("a detached HEAD")
            );
        }
        let head = self.git.head(&path)?;
        self.ensure_contains(
            &path,
            &spec.base_sha,
            &head,
            format_args!("{} is at {head}, which", path.display()),
        )
        .map(|()| path)
    }

    fn add_new(&self, spec: &WorktreeSpec, path: PathBuf, branch: &str) -> anyhow::Result<PathBuf> {
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
            self.ensure_contains(
                &spec.repo,
                &spec.base_sha,
                &tip,
                format_args!("branch {branch} is at {tip}, which"),
            )?;
        }
        std::fs::create_dir_all(&spec.root)
            .with_context(|| format!("creating {}", spec.root.display()))?;
        self.git
            .worktree_add(&spec.repo, &path, branch, &spec.base_sha)?;
        Ok(path)
    }

    /// Bail with "`what` does not contain base …" unless `tip` grows
    /// from `base`.
    fn ensure_contains(
        &self,
        repo: &Path,
        base: &str,
        tip: &str,
        what: std::fmt::Arguments<'_>,
    ) -> anyhow::Result<()> {
        if !self.git.is_ancestor(repo, base, tip)? {
            bail!("{what} does not contain base {base}");
        }
        Ok(())
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
        let head_sha = self.commit_head(spec, &branch, &frozen_at)?;
        let repo = &spec.repo;
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

    /// Commit everything in the worktree as the dataset identity and return
    /// the head, which must be the branch tip and contain the base.
    fn commit_head(
        &self,
        spec: &WorktreeSpec,
        branch: &str,
        frozen_at: &str,
    ) -> anyhow::Result<String> {
        let path = spec.path();
        let id = GitIdentity {
            name: FREEZE_NAME.to_owned(),
            email: FREEZE_EMAIL.to_owned(),
            date: frozen_at.to_owned(),
        };
        let message = format!("candidate {} frozen", spec.label);
        self.git.commit_all(&path, &message, &id)?;
        let head_sha = self.git.head(&path)?;
        // The candidate controls its worktree, including its `.git` file, so
        // everything recorded is read from the main repository: the branch
        // tip there must be what the worktree reports.
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
        self.ensure_contains(
            &spec.repo,
            &spec.base_sha,
            &head_sha,
            format_args!("candidate {} head {head_sha}", spec.label),
        )
        .map(|()| head_sha)
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

    /// The label and the experiment prefix, and the base hash, are each
    /// refused by their own rule. The message names the field and the value,
    /// so a spec that fails for another reason does not pass here.
    #[test]
    fn unsafe_labels_are_refused() {
        for bad in ["", "-x", "a/b", "..", "a b", "a..b"] {
            let want = |what: &str| {
                format!("candidate {what} '{bad}' must be [A-Za-z0-9_-] and not start with '-'")
            };
            assert_eq!(spec(bad).check().unwrap_err().to_string(), want("label"));
            let mut s = spec("c1");
            s.exp8 = bad.into();
            assert_eq!(s.check().unwrap_err().to_string(), want("exp8"));
        }
        for bad in ["", "HEAD~1", "abc12g", "-abc"] {
            let mut s = spec("c1");
            s.base_sha = bad.into();
            assert_eq!(
                s.check().unwrap_err().to_string(),
                format!("base_sha '{bad}' is not a commit hash")
            );
        }
        // The edges are valid: letters of both cases, digits, '_' and '-'
        // inside a name; a hash of either hex case.
        let mut s = spec("Cand_1-b");
        s.exp8 = "A_9-z".into();
        s.base_sha = "0123456789abcdefABCDEF".into();
        assert!(s.check().is_ok());
    }
}
