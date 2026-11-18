//! The port of the integration git gateway (ticket 23).
//!
//! `docs/specs/fleetd/design.md`, "No work is lost" (I8, I9, I11, I12),
//! "The bundle" and "fleetd sets the git settings that it needs": each git
//! call of integration goes through [`IntegrationGitGateway`]. Each call is
//! short and local: no working tree, no network. `fetch` and `push` are
//! jobs through `crate::vcs::network` (F11), not calls of this port.

use std::path::Path;

use anyhow::Result;

/// The outcome of a compare-and-set on a ref (I9).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefMove {
    /// The ref was at the expected tip, and git moved it.
    Moved,
    /// The ref was not at the expected tip, and nothing moved. `actual` is
    /// the tip that git has, or `None` when the ref does not exist.
    Mismatch { actual: Option<String> },
}

/// 1 member of a bundle: a task branch and the tip that fleetd saw.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    /// The branch name, for example `fleet/run/T-1-x`.
    pub name: String,
    pub tip: String,
}

/// The result of the build of a candidate (Q15).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Candidate {
    /// The candidate commit: 1 merge commit for each member, in queue order.
    /// No ref names it yet.
    Built { tip: String },
    /// The merge of `member` conflicts on `files`. Nothing names a commit.
    Conflict { member: String, files: Vec<String> },
}

/// A fleet branch and its tip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchTip {
    /// The branch name without `refs/heads/`, for example `fleet/run/x`.
    pub name: String,
    pub tip: String,
}

/// Each git call of integration. A ref is a full name (`refs/...`), and a
/// tip is a full object id.
pub trait IntegrationGitGateway {
    /// The tip of the ref `name`, or `None` when it does not exist.
    fn tip(&self, repo: &Path, name: &str) -> Result<Option<String>>;

    /// Move the ref `name` to `new` only when it is at `expected_old`
    /// (`None`: only when it does not exist): `git update-ref <ref> <new>
    /// <expected old>` (I9).
    fn update_ref(
        &self,
        repo: &Path,
        name: &str,
        new: &str,
        expected_old: Option<&str>,
    ) -> Result<RefMove>;

    /// Delete the ref `name` only when it is at `expected_old`.
    fn delete_ref(&self, repo: &Path, name: &str, expected_old: &str) -> Result<RefMove>;

    /// Whether the changes of the branch at `branch_tip` are in
    /// `target_tip` (I8): `branch_tip` is an ancestor of `target_tip`, or
    /// each file that the branch changed from its merge base with `base`
    /// has the same content in `target_tip`. A deleted file must be absent.
    /// So a squash merge is seen as landed (Q14).
    fn changes_in(
        &self,
        repo: &Path,
        branch_tip: &str,
        base: &str,
        target_tip: &str,
    ) -> Result<bool>;

    /// The candidate of a bundle on `base_tip` (Q15), with no checkout and
    /// no worktree.
    fn build_candidate(&self, repo: &Path, base_tip: &str, members: &[Member])
        -> Result<Candidate>;

    /// Each branch below `refs/heads/fleet/`, by name (I11).
    fn fleet_branches(&self, repo: &Path) -> Result<Vec<BranchTip>>;

    /// Each branch whose name starts with `<run_branch>-T`, by name: the
    /// task branches of a run branch outside `fleet/` (I11; Q4 of Phase 3).
    fn task_branches(&self, repo: &Path, run_branch: &str) -> Result<Vec<BranchTip>>;

    /// The branches that a `git stash list` entry names, sorted, each 1
    /// time (I12, preservation row W6).
    fn stash_branches(&self, repo: &Path) -> Result<Vec<String>>;

    /// The text of `fleet.toml` at the commit `tip`, or `None` when the
    /// commit has no such file.
    fn fleet_toml(&self, repo: &Path, tip: &str) -> Result<Option<String>>;

    /// Set `rerere.enabled=true` and `merge.conflictStyle=zdiff3` in the
    /// repository or worktree at `dir`, when they are not set so.
    fn ensure_settings(&self, dir: &Path) -> Result<()>;
}
