//! The ports of worktree custody.
//!
//! [`WorktreeGitGateway`] is the driven port of the git calls of the
//! worktree jobs (`ddd/08-code/worktree-custody/design.md`, section 4). Its
//! production adapter, `CliWorktreeGit`, holds the only `git worktree add`
//! and `git worktree list` calls of custody (FR-11). A test uses a fake.

use std::path::Path;

use anyhow::Result;
use serde::{Deserialize, Serialize};

/// 1 entry of `git worktree list --porcelain`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListEntry {
    /// The folder of the tree, as git prints it.
    pub path: String,
    /// The HEAD commit; `None` for a bare repository.
    pub head: Option<String>,
    /// The full ref of the branch (`refs/heads/<name>`); `None` when the
    /// HEAD is detached.
    pub branch: Option<String>,
    pub detached: bool,
    pub bare: bool,
    /// The reason of the lock, `""` when the lock has no reason; `None`
    /// when the tree is not locked.
    pub locked: Option<String>,
    /// Why git can prune the entry; `None` when it cannot.
    pub prunable: Option<String>,
}

/// The state of 1 work tree as git gives it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeState {
    /// The HEAD commit.
    pub head: String,
    /// `git status --porcelain --untracked-files=all` lists a file.
    pub dirty: bool,
}

/// The git calls of the worktree jobs. Each call names its folder. A call
/// that git cannot answer is an error, never a default fact.
pub trait WorktreeGitGateway {
    /// The entries of `git worktree list --porcelain` of the repository
    /// `repo`, with its `prunable` and `locked` entries.
    fn list(&self, repo: &Path) -> Result<Vec<ListEntry>>;

    /// The state of the tree at `path`, or `None` when `path` is not the top
    /// folder of a work tree.
    fn tree(&self, path: &Path) -> Result<Option<TreeState>>;

    /// Whether the branch `branch` exists in the repository `repo`.
    fn branch_exists(&self, repo: &Path, branch: &str) -> Result<bool>;

    /// `git worktree prune` in the repository `repo`.
    fn prune(&self, repo: &Path) -> Result<()>;

    /// `git worktree add -b <branch> <path> <base>` in the repository
    /// `repo`.
    fn add(&self, repo: &Path, path: &Path, branch: &str, base: &str) -> Result<()>;
}
