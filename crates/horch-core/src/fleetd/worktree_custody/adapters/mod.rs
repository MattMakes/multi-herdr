//! The adapters of worktree custody: SQLite, git and the job executors.

pub mod executors;
pub mod git;
pub mod memory;
pub mod remove_git;
pub mod rescue_git;
pub mod run_now;
pub mod sqlite;
