//! The executor of each worktree job kind.
//!
//! `run_job` of fleet runtime calls [`run`] for the kinds `worktree_add`,
//! `worktree_list`, `rescue` and `worktree_remove` (MAP.md of fleetd-p3,
//! Q22). It runs in the job process, outside each pass and each
//! transaction.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use horch_marketplace::git::SandboxHost;
use horch_platform::sandbox::sandboxed_hiding;
use rusqlite::Connection;

use crate::fleetd::fleet_runtime::adapters::jobs_store::JobRow;
use crate::fleetd::fleet_runtime::application::jobs::Ran;
use crate::fleetd::fleet_runtime::domain::jobs::{self, Kind};
use crate::fleetd::task_execution::adapters::sqlite::attempts_of_worker;
use crate::fleetd::worktree_custody::adapters::git::{self, CliWorktreeGit};
use crate::fleetd::worktree_custody::adapters::remove_git::{self, CliRemoval, Owner};
use crate::fleetd::worktree_custody::adapters::rescue_git;
use crate::fleetd::worktree_custody::adapters::sqlite::{
    self as store, WorktreeRow, WorktreeState,
};
use crate::fleetd::worktree_custody::domain::removal;
use crate::runtime::bins::{git_bin, BinOverrides};
use crate::runtime::context::ProcessEnv;
use crate::runtime::paths::nonempty_path;

/// Run the job `row` of a worktree kind with the git binary of this process.
/// Each git call runs in the sandbox, and can write only the paths of its
/// kind. A host with no sandbox runs no git: the run fails with a text that
/// starts with [`git::NO_SANDBOX`].
pub fn run(conn: &Connection, row: &JobRow) -> Result<Ran> {
    run_in(conn, row, sandboxed_hiding)
}

/// [`run`] with the sandbox host `host`. A test gives a fake answer.
#[doc(hidden)]
pub fn run_in(conn: &Connection, row: &JobRow, host: SandboxHost) -> Result<Ran> {
    let root = fleet_root(conn)?;
    let git = CliWorktreeGit::new(git_bin(&BinOverrides::from_env(&ProcessEnv)), &root)
        .with_sandbox_host(host);
    if let Some(ran) = git::no_sandbox(&git) {
        return Ok(ran);
    }
    match row.kind {
        // The subject is the worktree id.
        Kind::WorktreeAdd => match store::get(conn, &row.subject)? {
            Some(tree) => Ok(match git::tree_writer(git, &tree) {
                Ok(git) => git::worktree_add(&git, &tree),
                Err(e) => git::failed(format!("{e:#}")),
            }),
            None => Ok(git::failed(format!(
                "no worktree {} in the ledger",
                row.subject
            ))),
        },
        // The subject is the repository root. The job only reads.
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
        Kind::Rescue => match store::get(conn, &row.subject)? {
            Some(tree) => {
                let ran = rescue_git::rescue(&git, &root, &tree);
                rescue_git::drop_index_folder(&root, &tree);
                Ok(ran)
            }
            None => Ok(git::failed(format!(
                "no worktree {} in the ledger",
                row.subject
            ))),
        },
        Kind::WorktreeRemove => match store::get(conn, &row.subject)? {
            Some(tree) => {
                let path = Path::new(&tree.path);
                let quarantine = removal::quarantine_path(path, &tree.worktree_id);
                let git = match git::tree_writer(git, &tree) {
                    Ok(git) => git.writing(&[quarantine]),
                    Err(e) => return Ok(git::failed(format!("{e:#}"))),
                };
                let landed = landed_commit(conn, &tree.worktree_id)?;
                let owner = owner(conn, &tree)?;
                let gw = CliRemoval::new(git, root.clone());
                let ledger = |id: &str| store::get(conn, id);
                let ran = remove_git::remove(&gw, &tree, landed.as_deref(), &owner, &ledger);
                rescue_git::drop_index_folder(&root, &tree);
                Ok(ran)
            }
            None => Ok(git::failed(format!(
                "no worktree {} in the ledger",
                row.subject
            ))),
        },
        kind => Ok(jobs::no_executor(kind).into()),
    }
}

fn landed_commit(conn: &Connection, id: &str) -> Result<Option<String>> {
    Ok(conn.query_row(
        "SELECT landed_commit FROM worktrees WHERE worktree_id = ?1",
        [id],
        |r| r.get(0),
    )?)
}

/// The owner attempt of `tree` and its agent process, as the ledger
/// records it.
fn owner(conn: &Connection, tree: &WorktreeRow) -> Result<Owner> {
    let Some(attempt) = tree.owner_attempt.as_deref() else {
        return Ok(Owner::NoAttempt);
    };
    let unknown = |why: &str| Ok(Owner::Unknown(format!("the owner attempt {attempt} {why}")));
    let Some(worker) = tree.worker_id.as_deref() else {
        return unknown("has no worker in the row");
    };
    let found = attempts_of_worker(conn, worker)?
        .into_iter()
        .find(|a| a.id == attempt);
    match found.map(|a| a.process) {
        None => unknown("is not an attempt of the worker of the row"),
        Some(None) => unknown("has no recorded process"),
        Some(Some(p)) => Ok(Owner::Process {
            pid: p.pid,
            start: p.pid_start,
        }),
    }
}

/// The fleet root of the ledger of `conn`: `$FLEET_ROOT`, else the folder
/// `fleet` beside the ledger file, the rule of `Paths` (MAP.md of
/// fleetd-p1, Q4). `horch job run` opens the ledger of its `Paths`, so both
/// give 1 folder.
pub(crate) fn fleet_root(conn: &Connection) -> Result<PathBuf> {
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
