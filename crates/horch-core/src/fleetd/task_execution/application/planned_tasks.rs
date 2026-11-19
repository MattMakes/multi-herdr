//! The tasks of a committed or replanned plan, with the idea of each task
//! and the requirements that it covers.
//!
//! `add_planned_tasks` is the port `AddPlannedTasks` of task execution: the
//! ideas context does not write the `tasks` table, so the plan commit calls
//! this function inside the transaction that it opens
//! (docs/tickets/fleetd-p6/14-plan-commit.md). Each task goes in the
//! internal run of the feature of its idea (MAP.md of fleetd-p6, Q5). An
//! `after:` key is a `task_deps` row. A `judges:` key is the judge link of
//! ticket 40: the judge task is a task of kind `work` whose
//! `parent_task_id` is the judged task (see `judged_task`), and not a
//! `task_deps` row. Each new row gets its event through `created()` of the
//! 1 door.
//!
//! `replace_planned_task` is the replace path of the port, for the replan
//! of a `failed` task (docs/tickets/fleetd-p6/31-replan.md; MAP.md, Q20):
//! the old task follows A cancel, the new tasks go in, and each task that
//! waited on the old task waits on the new task with its key.

use std::collections::BTreeMap;

use anyhow::{bail, Context, Result};
use rusqlite::{params, Connection, OptionalExtension};

use crate::fleetd::fleet_runtime::adapters::sqlite::check_writable;
use crate::fleetd::fleet_runtime::application::ledger::{created, Cause};
use crate::fleetd::fleet_runtime::domain::ledger::{Entity, Generation};
use crate::fleetd::task_execution::application::cancel::{self, Cancel};
use crate::fleetd::task_execution::domain::task::judged_task;
use crate::taskledger::model::{FileMode, Origin, TaskKind, TaskStatus, WorkKind, WorktreeMode};
use crate::taskledger::runs::Run;
use crate::taskledger::tasks::{self, ticket_sha, Task};
use crate::taskledger::util;

/// 1 task of a valid plan, as the plan commit gives it.
#[derive(Debug, Clone)]
pub struct PlannedTask {
    /// The key of the task in plan.yaml, unique in the plan.
    pub key: String,
    pub title: String,
    /// The file that the worker reads: the ticket, else plan.yaml.
    pub task_file: String,
    /// The ticket text at commit time; its sha256 is stored with it.
    pub ticket_snapshot: Option<String>,
    /// The `**Kind:**` of today for the kinds that it knows; the kind of
    /// the plan is in `spec_json`.
    pub work_kind: Option<WorkKind>,
    pub profile: String,
    /// The `run:` commands of the acceptance check, or `None`.
    pub land_check: Option<String>,
    /// The task as committed.
    pub spec_json: String,
    /// The keys of the tasks that must be done and landed first.
    pub after: Vec<String>,
    /// For a judge task: the key of the task it judges.
    pub judges: Option<String>,
    /// The paths that the task owns: its Own rows in `task_files`.
    pub own: Vec<String>,
}

/// 1 task of a committed plan, as the ledger holds it.
#[derive(Debug, Clone)]
pub struct CommittedTask {
    pub task_id: String,
    pub idea_id: String,
    pub status: String,
    /// For a judge task: the id of the task it judges (`judged_task`).
    pub judged: Option<String>,
    /// The task as committed (`PlannedTask::spec_json`).
    pub spec_json: String,
}

const COMMITTED: &str = "SELECT task_id, idea_id, status, kind, parent_task_id, spec_json
    FROM tasks WHERE idea_id IS NOT NULL AND spec_json IS NOT NULL";

fn committed(r: &rusqlite::Row<'_>) -> rusqlite::Result<CommittedTask> {
    let kind: String = r.get(3)?;
    let parent: Option<String> = r.get(4)?;
    Ok(CommittedTask {
        task_id: r.get(0)?,
        idea_id: r.get(1)?,
        status: r.get(2)?,
        judged: judged_task(&kind, parent.as_deref()).map(str::to_string),
        spec_json: r.get(5)?,
    })
}

/// The task `task_id` when it is a task of a committed plan.
pub fn committed_task(conn: &Connection, task_id: &str) -> Result<Option<CommittedTask>> {
    let sql = format!("{COMMITTED} AND task_id = ?1");
    Ok(conn.query_row(&sql, [task_id], committed).optional()?)
}

/// Each task of a committed plan of the idea `idea_id`, oldest first.
pub fn committed_tasks(conn: &Connection, idea_id: &str) -> Result<Vec<CommittedTask>> {
    let sql = format!("{COMMITTED} AND idea_id = ?1 ORDER BY rowid");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([idea_id], committed)?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// Insert each task of `tasks` for the idea `idea_id` in `run`, in the
/// caller's transaction `tx`: its row, its `task_deps` rows, its judge link
/// and its event. A task with no dependency is `ready`; the others are
/// `pending`. Returns (key, task id) of each task, in the order of `tasks`.
/// A key that names no task of `tasks`, or a cycle, inserts nothing that
/// the caller keeps: it gives an error, and the caller rolls back.
pub fn add_planned_tasks(
    tx: &Connection,
    guard: Option<Generation>,
    run: &Run,
    idea_id: &str,
    tasks: &[PlannedTask],
    cause: Cause<'_>,
) -> Result<Vec<(String, String)>> {
    add_in(tx, guard, run, idea_id, tasks, &BTreeMap::new(), cause)
}

/// The `failed` task that a replan replaces.
pub struct Replaced<'a> {
    /// The task as the replan read it.
    pub old: &'a Task,
    /// Its key in the plan.
    pub key: &'a str,
    /// The reason of its cancel.
    pub reason: &'a str,
    /// The other tasks of the idea that are not `cancelled`: key -> task id.
    pub live: &'a BTreeMap<String, String>,
}

/// The replace path: the replan of the `failed` task `replaced.old` (or of
/// the task in `blocked_on_user` with a `replan` answer) of the
/// idea `idea_id` by `tasks`, in the caller's transaction `tx`. The old
/// task follows A cancel, and its `replans` goes up by 1. Each task of
/// `tasks` goes in as `add_planned_tasks` adds it, with the replan of the
/// old task used (its `replans` + 1), so a replan never starts a second
/// one. An `after:` or `judges:` key that names no task of `tasks` names a
/// task of `replaced.live`. Each task that is not `cancelled` and waits on
/// the old task waits on the task of `tasks` with its key instead. A task
/// that changed after the replan read it gives an error. Returns (key,
/// task id) of each task, in the order of `tasks`.
pub fn replace_planned_task(
    tx: &Connection,
    guard: Option<Generation>,
    run: &Run,
    idea_id: &str,
    replaced: &Replaced<'_>,
    tasks: &[PlannedTask],
    cause: Cause<'_>,
) -> Result<Vec<(String, String)>> {
    check_writable(tx, guard)?;
    let Replaced {
        old,
        key: old_key,
        reason,
        live,
    } = *replaced;
    let now = tasks::get(tx, &old.task_id)?;
    // A task in `blocked_on_user` has a replan when its question has a
    // `replan` answer (ticket 32 of fleetd-p6); it leaves to `cancelled`.
    let replannable = [
        TaskStatus::Failed.as_str(),
        TaskStatus::BlockedOnUser.as_str(),
    ];
    if now.status != old.status
        || !replannable.contains(&now.status.as_str())
        || now.replans != old.replans
    {
        bail!(
            "{} is {} with {} replans: it changed before its replan",
            now.short(),
            now.status,
            now.replans
        );
    }
    match cancel::request(tx, &now, reason)? {
        Cancel::Cancelled { .. } => {}
        Cancel::Stopping { .. } => bail!("{} has a live attempt", now.short()),
    }
    let replans = now.replans + 1;
    tx.execute(
        "UPDATE tasks SET replans = ?2 WHERE task_id = ?1",
        params![now.task_id, replans],
    )?;
    let ids = add_in(tx, guard, run, idea_id, tasks, live, cause)?;
    for (_, id) in &ids {
        tx.execute(
            "UPDATE tasks SET replans = ?2 WHERE task_id = ?1",
            params![id, replans],
        )?;
    }
    let successor = ids.iter().find(|(key, _)| key == old_key).map(|(_, id)| id);
    let waiting: Vec<String> = {
        let mut stmt = tx.prepare(
            "SELECT d.task_id FROM task_deps d JOIN tasks t ON t.task_id = d.task_id
             WHERE d.blocked_by = ?1 AND t.status <> ?2 ORDER BY d.task_id",
        )?;
        let rows = stmt.query_map(params![now.task_id, TaskStatus::Cancelled.as_str()], |r| {
            r.get(0)
        })?;
        rows.collect::<rusqlite::Result<_>>()?
    };
    for task_id in waiting {
        let Some(successor) = successor else {
            bail!("task {task_id} waits on {old_key}, and the replan has no task {old_key}");
        };
        tx.execute(
            "UPDATE task_deps SET blocked_by = ?3 WHERE task_id = ?1 AND blocked_by = ?2",
            params![task_id, now.task_id, successor],
        )?;
    }
    Ok(ids)
}

/// The retry of a replan of the task `task_id` in `blocked_on_user` uses
/// its 1 replan: `replans` goes up by 1, in the caller's transaction `tx`.
/// The answer of its question then moves it to `ready` (ticket 32 of
/// fleetd-p6).
pub fn use_replan(tx: &Connection, guard: Option<Generation>, task_id: &str) -> Result<()> {
    check_writable(tx, guard)?;
    tx.execute(
        "UPDATE tasks SET replans = replans + 1 WHERE task_id = ?1",
        params![task_id],
    )?;
    Ok(())
}

/// Insert `tasks` as `add_planned_tasks` does; a key that names no task of
/// `tasks` names a task of `live`.
fn add_in(
    tx: &Connection,
    guard: Option<Generation>,
    run: &Run,
    idea_id: &str,
    tasks: &[PlannedTask],
    live: &BTreeMap<String, String>,
    cause: Cause<'_>,
) -> Result<Vec<(String, String)>> {
    check_writable(tx, guard)?;
    let index: BTreeMap<&str, usize> = tasks
        .iter()
        .enumerate()
        .map(|(i, t)| (t.key.as_str(), i))
        .collect();
    if index.len() != tasks.len() {
        bail!("2 planned tasks share a key");
    }
    let mut ids: Vec<Option<String>> = vec![None; tasks.len()];
    // A task goes in after each task that it names, so that each
    // `task_deps` row and each judge link points at an existing row.
    while ids.iter().any(Option::is_none) {
        let mut progress = false;
        for (i, task) in tasks.iter().enumerate() {
            if ids[i].is_some() {
                continue;
            }
            let named = named_ids(task, &index, &ids, live)?;
            let Some((deps, judged)) = named else {
                continue;
            };
            let id = insert(tx, run, idea_id, task, &deps, judged.as_deref())?;
            created(tx, guard, Entity::Task, &id, cause)?;
            ids[i] = Some(id);
            progress = true;
        }
        if !progress {
            bail!("the planned tasks have a dependency cycle");
        }
    }
    Ok(tasks
        .iter()
        .zip(ids)
        .map(|(t, id)| (t.key.clone(), id.expect("each task is inserted")))
        .collect())
}

/// The ids of the `after:` tasks and of the judged task of `task`, or
/// `None` while 1 of them is not inserted yet. A key of `tasks` comes
/// first; else the key names a task of `live`.
fn named_ids(
    task: &PlannedTask,
    index: &BTreeMap<&str, usize>,
    ids: &[Option<String>],
    live: &BTreeMap<String, String>,
) -> Result<Option<(Vec<String>, Option<String>)>> {
    let id_of = |key: &str| -> Result<Option<String>> {
        if let Some(i) = index.get(key) {
            return Ok(ids[*i].clone());
        }
        let id = live.get(key).with_context(|| {
            format!(
                "task {} names {key}, which is not a task of the plan",
                task.key
            )
        })?;
        Ok(Some(id.clone()))
    };
    let mut deps = Vec::new();
    for key in &task.after {
        match id_of(key)? {
            Some(id) => deps.push(id),
            None => return Ok(None),
        }
    }
    let judged = match task.judges.as_deref() {
        None => None,
        Some(key) => match id_of(key)? {
            Some(id) => Some(id),
            None => return Ok(None),
        },
    };
    Ok(Some((deps, judged)))
}

/// Insert the row of `task` and its `task_deps` rows. Returns its id.
fn insert(
    tx: &Connection,
    run: &Run,
    idea_id: &str,
    task: &PlannedTask,
    deps: &[String],
    judged: Option<&str>,
) -> Result<String> {
    let seq: i64 = tx.query_row(
        "SELECT COALESCE(MAX(seq), 0) + 1 FROM tasks WHERE run_id = ?1",
        [&run.run_id],
        |r| r.get(0),
    )?;
    let id = util::new_id();
    let now = util::now();
    let status = if deps.is_empty() && judged.is_none() {
        TaskStatus::Ready
    } else {
        TaskStatus::Pending
    };
    tx.execute(
        "INSERT INTO tasks (task_id, run_id, seq, title, task_file, primary_persona,
            claimable_by, worktree_mode, land_check, kind, parent_task_id, status, origin,
            created_at, updated_at, work_kind, ticket_snapshot, ticket_sha, idea_id, spec_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?13, ?14, ?15,
            ?16, ?17, ?18)",
        params![
            id,
            run.run_id,
            seq,
            task.title,
            task.task_file,
            task.profile,
            WorktreeMode::Worktree.as_str(),
            task.land_check,
            TaskKind::Work.as_str(),
            judged,
            status.as_str(),
            Origin::Native.as_str(),
            now,
            task.work_kind.map(|k| k.as_str()),
            task.ticket_snapshot,
            task.ticket_snapshot.as_deref().map(ticket_sha),
            idea_id,
            task.spec_json,
        ],
    )
    .with_context(|| format!("inserting the task {} of {idea_id}", task.key))?;
    for path in &task.own {
        tx.execute(
            "INSERT OR IGNORE INTO task_files (task_id, path, mode) VALUES (?1, ?2, ?3)",
            params![id, path.trim(), FileMode::Own.as_str()],
        )?;
    }
    for dep in deps {
        tx.execute(
            "INSERT INTO task_deps (task_id, blocked_by) VALUES (?1, ?2)",
            params![id, dep],
        )?;
    }
    Ok(id)
}
