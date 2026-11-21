use std::path::Path;

use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListEntry {
    pub path: String,
    pub head: Option<String>,
    pub branch: Option<String>,
    pub detached: bool,
    pub bare: bool,
    pub locked: Option<String>,
    pub prunable: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeState {
    pub head: String,
    pub dirty: bool,
}

pub trait WorktreeGitGateway {
    fn list(&self, repo: &Path) -> Result<Vec<ListEntry>>;

    fn tree(&self, path: &Path) -> Result<Option<TreeState>>;

    fn branch_exists(&self, repo: &Path, branch: &str) -> Result<bool>;

    fn branch_tip(&self, repo: &Path, branch: &str) -> Result<Option<String>>;

    fn prune(&self, repo: &Path) -> Result<()>;

    fn add(&self, repo: &Path, path: &Path, branch: &str, base: &str) -> Result<()>;

    fn attach(&self, repo: &Path, path: &Path, branch: &str) -> Result<()>;
}
