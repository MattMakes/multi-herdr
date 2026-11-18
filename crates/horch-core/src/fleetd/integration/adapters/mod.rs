//! The adapters layer of integration: SQLite, git, the project file and the
//! job executors.

pub mod exec_bundle;
pub mod exec_fetch;
pub mod exec_gate;
pub mod exec_push;
pub mod exec_rebase;
pub mod executors;
pub mod git;
pub mod project_file;
pub mod sqlite;
