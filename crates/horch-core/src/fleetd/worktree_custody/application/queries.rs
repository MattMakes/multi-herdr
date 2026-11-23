use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension};
use serde::Serialize;

use crate::fleetd::fleet_runtime::adapters::jobs_store;
use crate::fleetd::fleet_runtime::domain::jobs::Kind;
use crate::fleetd::worktree_custody::adapters::sqlite::{self as store, WorktreeRow};
use crate::taskledger::{runs, tasks, util, workers};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WorktreeView {
    pub worktree_id: String,
    pub path: String,
    pub state: String,
    pub status: String,
    pub task: Option<String>,
    pub run: Option<String>,
    pub worker: Option<String>,
    pub owner_attempt: Option<String>,
    pub age_s: Option<i64>,
    pub bytes_on_disk: Option<i64>,
    pub dirty: bool,
    pub rescue_ref: Option<String>,
    pub jobs: Vec<TreeJobView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TreeJobView {
    pub id: i64,
    pub kind: &'static str,
    pub state: &'static str,
}

const TREE_KINDS: [Kind; 3] = [Kind::WorktreeAdd, Kind::Rescue, Kind::WorktreeRemove];

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct DiskTotal {
    pub count: usize,
    pub bytes: u64,
    pub unmeasured: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RescueSource {
    pub row: WorktreeRow,
    pub rescue_ref: String,
    pub seq: i64,
    pub n: u64,
}

pub fn worktree_rows(conn: &Connection) -> Result<Vec<WorktreeView>> {
    let now = util::now();
    let mut stmt = conn.prepare(
        "SELECT worktree_id, created_at FROM worktrees
          WHERE state <> 'removed' ORDER BY created_at, worktree_id",
    )?;
    let ids = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut views = Vec::with_capacity(ids.len());
    for (id, created_at) in ids {
        let row = store::get(conn, &id)?.with_context(|| format!("no worktree {id}"))?;
        views.push(view(conn, row, util::seconds_between(&created_at, &now))?);
    }
    Ok(views)
}

fn view(conn: &Connection, row: WorktreeRow, age_s: Option<i64>) -> Result<WorktreeView> {
    let task = match row.task_id.as_deref() {
        Some(id) => Some(tasks::get(conn, id)?),
        None => None,
    };
    let run_id = task
        .as_ref()
        .map(|t| t.run_id.clone())
        .or_else(|| row.mode.as_ref().map(|m| m.run_id.clone()));
    let run = match run_id {
        Some(id) => Some(runs::get(conn, &id)?.slug),
        None => None,
    };
    let worker = match row.worker_id.as_deref() {
        Some(id) => Some(
            workers::find(conn, id)?
                .and_then(|w| w.role)
                .unwrap_or_else(|| id.to_string()),
        ),
        None => None,
    };
    let mut jobs = Vec::new();
    for kind in TREE_KINDS {
        if let Some(j) = jobs_store::active(conn, kind, &row.worktree_id)? {
            jobs.push(TreeJobView {
                id: j.id,
                kind: kind.as_str(),
                state: j.state.as_str(),
            });
        }
    }
    Ok(WorktreeView {
        jobs,
        worktree_id: row.worktree_id,
        path: row.path,
        state: row.state.as_str().to_string(),
        status: row.status.as_str().to_string(),
        task: task.map(|t| format!("T{}", t.seq)),
        run,
        worker,
        owner_attempt: row.owner_attempt,
        age_s,
        bytes_on_disk: row.bytes_on_disk,
        dirty: row.dirty,
        rescue_ref: row.rescue_ref,
    })
}

pub fn total_bytes(conn: &Connection) -> Result<DiskTotal> {
    let (count, bytes, unmeasured): (i64, i64, i64) = conn.query_row(
        "SELECT COUNT(*), COALESCE(SUM(MAX(bytes_on_disk, 0)), 0),
                COALESCE(SUM(bytes_on_disk IS NULL), 0)
           FROM worktrees WHERE state <> 'removed'",
        [],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    Ok(DiskTotal {
        count: usize::try_from(count).unwrap_or(0),
        bytes: u64::try_from(bytes).unwrap_or(0),
        unmeasured: usize::try_from(unmeasured).unwrap_or(0),
    })
}

pub fn rescue_source(
    conn: &Connection,
    run_id: Option<&str>,
    name: &str,
) -> Result<Option<RescueSource>> {
    let row = match (task_seq(name), run_id) {
        (Some(_), None) => None,
        (Some(seq), Some(run_id)) => match tasks::of_run(conn, run_id)?
            .into_iter()
            .find(|t| t.seq == seq)
        {
            Some(task) => rescued_tree_of_task(conn, &task.task_id)?,
            None => None,
        },
        (None, _) => store::get(conn, name)?,
    };
    let Some(row) = row else { return Ok(None) };
    let Some(rescue_ref) = row.rescue_ref.clone().filter(|r| !r.is_empty()) else {
        return Ok(None);
    };
    let Some(n) = rescue_ref.rsplit('/').next().and_then(|n| n.parse().ok()) else {
        anyhow::bail!("the rescue ref {rescue_ref} has no number at its end");
    };
    let Some(seq) = row_seq(conn, &row)? else {
        anyhow::bail!(
            "the worktree {} has no task, so its rescue ref {rescue_ref} has no branch name rescue/T<seq>-<n>; \
             check it out by hand",
            row.worktree_id
        );
    };
    Ok(Some(RescueSource {
        row,
        rescue_ref,
        seq,
        n,
    }))
}

fn rescued_tree_of_task(conn: &Connection, task_id: &str) -> Result<Option<WorktreeRow>> {
    let id: Option<String> = conn
        .query_row(
            "SELECT worktree_id FROM worktrees
              WHERE task_id = ?1 AND COALESCE(rescue_ref, '') <> ''
              ORDER BY created_at DESC, rowid DESC LIMIT 1",
            [task_id],
            |r| r.get(0),
        )
        .optional()?;
    match id {
        Some(id) => store::get(conn, &id),
        None => Ok(None),
    }
}

fn task_seq(name: &str) -> Option<i64> {
    let digits = name.strip_prefix('T')?;
    let digits = digits.strip_prefix('-').unwrap_or(digits);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

fn row_seq(conn: &Connection, row: &WorktreeRow) -> Result<Option<i64>> {
    match row.task_id.as_deref() {
        Some(task) => Ok(Some(tasks::get(conn, task)?.seq)),
        None => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_task_name_has_its_seq() {
        assert_eq!(task_seq("T7"), Some(7));
        assert_eq!(task_seq("T-12"), Some(12));
        assert_eq!(task_seq("T"), None);
        assert_eq!(task_seq("Tx1"), None);
        assert_eq!(task_seq("old"), None);
    }
}
