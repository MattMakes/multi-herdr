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

#[derive(Debug, Clone)]
pub struct PlannedTask {
    pub key: String,
    pub title: String,
    pub task_file: String,
    pub ticket_snapshot: Option<String>,
    pub work_kind: Option<WorkKind>,
    pub profile: String,
    pub land_check: Option<String>,
    pub spec_json: String,
    pub after: Vec<String>,
    pub judges: Option<String>,
    pub own: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct CommittedTask {
    pub task_id: String,
    pub idea_id: String,
    pub status: String,
    pub judged: Option<String>,
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

pub fn committed_task(conn: &Connection, task_id: &str) -> Result<Option<CommittedTask>> {
    let sql = format!("{COMMITTED} AND task_id = ?1");
    Ok(conn.query_row(&sql, [task_id], committed).optional()?)
}

pub fn committed_tasks(conn: &Connection, idea_id: &str) -> Result<Vec<CommittedTask>> {
    let sql = format!("{COMMITTED} AND idea_id = ?1 ORDER BY rowid");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([idea_id], committed)?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

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

pub struct Replaced<'a> {
    pub old: &'a Task,
    pub key: &'a str,
    pub reason: &'a str,
    pub live: &'a BTreeMap<String, String>,
}

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

pub fn use_replan(tx: &Connection, guard: Option<Generation>, task_id: &str) -> Result<()> {
    check_writable(tx, guard)?;
    tx.execute(
        "UPDATE tasks SET replans = replans + 1 WHERE task_id = ?1",
        params![task_id],
    )?;
    Ok(())
}

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
    tasks::add_files(tx, &id, &task.own, FileMode::Own)?;
    for dep in deps {
        tx.execute(
            "INSERT INTO task_deps (task_id, blocked_by) VALUES (?1, ?2)",
            params![id, dep],
        )?;
    }
    Ok(id)
}
