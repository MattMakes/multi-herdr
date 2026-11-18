//! The executor of each worktree job kind.
//!
//! `run_job` of fleet runtime calls [`run`] for the kinds `worktree_add`,
//! `worktree_list`, `rescue` and `worktree_remove` (MAP.md of fleetd-p3,
//! Q22). It runs in the job process, outside each pass and each
//! transaction.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use rusqlite::Connection;

use crate::fleetd::fleet_runtime::adapters::jobs_store::JobRow;
use crate::fleetd::fleet_runtime::application::jobs::Ran;
use crate::fleetd::fleet_runtime::domain::jobs::{self, Kind};
use crate::fleetd::worktree_custody::adapters::git::{self, CliWorktreeGit};
use crate::fleetd::worktree_custody::adapters::rescue_git;
use crate::fleetd::worktree_custody::adapters::sqlite::{self as store, WorktreeState};
use crate::runtime::bins::{git_bin, BinOverrides};
use crate::runtime::context::ProcessEnv;
use crate::runtime::paths::nonempty_path;

/// Run the job `row` of a worktree kind with the git binary of this process.
/// `worktree_remove` has no executor yet.
pub fn run(conn: &Connection, row: &JobRow) -> Result<Ran> {
    let git = CliWorktreeGit::new(git_bin(&BinOverrides::from_env(&ProcessEnv)));
    match row.kind {
        // The subject is the worktree id.
        Kind::WorktreeAdd => match store::get(conn, &row.subject)? {
            Some(tree) => Ok(git::worktree_add(&git, &tree)),
            None => Ok(git::failed(format!(
                "no worktree {} in the ledger",
                row.subject
            ))),
        },
        // The subject is the repository.
        Kind::WorktreeList => {
            let live: Vec<WorktreeState> = WorktreeState::ALL
                .into_iter()
                .filter(|s| *s != WorktreeState::Removed)
                .collect();
            let rows: Vec<_> = store::in_states(conn, &live)?
                .into_iter()
                .filter(|r| r.repo_root == row.subject)
                .collect();
            Ok(git::worktree_list(&git, Path::new(&row.subject), &rows))
        }
        // The subject is the worktree id.
        Kind::Rescue => match store::get(conn, &row.subject)? {
            Some(tree) => Ok(rescue_git::rescue(&git, &fleet_root(conn)?, &tree)),
            None => Ok(git::failed(format!(
                "no worktree {} in the ledger",
                row.subject
            ))),
        },
        kind => Ok(jobs::no_executor(kind).into()),
    }
}

/// The fleet root of the ledger of `conn`: `$FLEET_ROOT`, else the folder
/// `fleet` beside the ledger file, the rule of `Paths` (MAP.md of
/// fleetd-p1, Q4). `horch job run` opens the ledger of its `Paths`, so both
/// give 1 folder.
fn fleet_root(conn: &Connection) -> Result<PathBuf> {
    if let Some(root) = nonempty_path(&ProcessEnv, "FLEET_ROOT") {
        return Ok(root);
    }
    let db = conn
        .path()
        .filter(|p| !p.is_empty())
        .context("the ledger has no file, so it has no fleet root")?;
    Ok(Path::new(db)
        .parent()
        .unwrap_or(Path::new(""))
        .join("fleet"))
}
