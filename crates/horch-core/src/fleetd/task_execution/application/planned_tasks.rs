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

use std::collections::BTreeMap;

use anyhow::{bail, Context, Result};
use rusqlite::{params, Connection};

use crate::fleetd::fleet_runtime::adapters::sqlite::check_writable;
use crate::fleetd::fleet_runtime::application::ledger::{created, Cause};
use crate::fleetd::fleet_runtime::domain::ledger::{Entity, Generation};
use crate::taskledger::model::{Origin, TaskKind, TaskStatus, WorkKind, WorktreeMode};
use crate::taskledger::runs::Run;
use crate::taskledger::tasks::ticket_sha;
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
            let named = named_ids(task, &index, &ids)?;
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
/// `None` while 1 of them is not inserted yet.
fn named_ids(
    task: &PlannedTask,
    index: &BTreeMap<&str, usize>,
    ids: &[Option<String>],
) -> Result<Option<(Vec<String>, Option<String>)>> {
    let id_of = |key: &str| -> Result<Option<String>> {
        let i = index.get(key).with_context(|| {
            format!(
                "task {} names {key}, which is not a task of the plan",
                task.key
            )
        })?;
        Ok(ids[*i].clone())
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
    for dep in deps {
        tx.execute(
            "INSERT INTO task_deps (task_id, blocked_by) VALUES (?1, ?2)",
            params![id, dep],
        )?;
    }
    Ok(id)
}
