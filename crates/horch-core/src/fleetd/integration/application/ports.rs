use std::path::Path;

use anyhow::Result;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefMove {
    Moved,
    Mismatch { actual: Option<String> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    pub name: String,
    pub tip: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Candidate {
    Built { tip: String },
    Conflict { member: String, files: Vec<String> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchTip {
    pub name: String,
    pub tip: String,
}

pub trait IntegrationGitGateway {
    fn tip(&self, repo: &Path, name: &str) -> Result<Option<String>>;

    fn update_ref(
        &self,
        repo: &Path,
        name: &str,
        new: &str,
        expected_old: Option<&str>,
    ) -> Result<RefMove>;

    fn delete_ref(&self, repo: &Path, name: &str, expected_old: &str) -> Result<RefMove>;

    fn contains(&self, repo: &Path, tip: &str, ancestor: &str) -> Result<bool>;

    fn changes_in(
        &self,
        repo: &Path,
        branch_tip: &str,
        base: &str,
        target_tip: &str,
    ) -> Result<bool>;

    fn is_ancestor(&self, repo: &Path, a: &str, b: &str) -> Result<bool>;

    fn build_candidate(&self, repo: &Path, base_tip: &str, members: &[Member])
        -> Result<Candidate>;

    fn fleet_branches(&self, repo: &Path) -> Result<Vec<BranchTip>>;

    fn task_branches(&self, repo: &Path, run_branch: &str) -> Result<Vec<BranchTip>>;

    fn stash_branches(&self, repo: &Path) -> Result<Vec<String>>;

    fn fleet_toml(&self, repo: &Path, tip: &str) -> Result<Option<String>>;

    fn ensure_settings(&self, dir: &Path) -> Result<()>;
}
