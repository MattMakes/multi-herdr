//! The replan of a failed task: the sub-orchestrator replaces the task or
//! sends it back 1 time, else it is escalated.
//!
//! `replan_task` is the use case `ReplanTask` (docs/specs/fleetd/design.md,
//! "The task state machine", `failed`; RR-6, FM-21;
//! docs/tickets/fleetd-p6/31-replan.md). The sub-orchestrator of the
//! feature owns a `failed` task. It writes `replan-<task key>.yaml` in the
//! idea folder (MAP.md, Q4), with exactly 1 of 2 keys:
//!
//! - `retry: <reason>`: the task goes back to `ready` with its attempt
//!   count at 0, through the requeue of task execution;
//! - `replace:` with new task entries in the form of plan.yaml: fleetd
//!   validates the plan of the idea after the replace with the plan
//!   validator, then, in 1 transaction, the old task follows A cancel and
//!   the new tasks go in through `replace_planned_task` (MAP.md, Q20).
//!
//! Each adds 1 to `replans`. A task that fails after its 1 replan, and a
//! task that a replace added, are escalated by rung 6. A `failed` task past
//! its replan limit on the fleet clock is escalated here, and its replan
//! file is not applied. Each escalation opens 1 needs-you item.
//!
//! fleetd writes `replan-<task key>.verdict.json` after each decision on a
//! replan file, so the sub-orchestrator sees each problem of a refused
//! replan and writes its file again within the limit.
//!
//! A task in `blocked_on_user` whose question has a `replan` answer
//! (ticket 32, [`answers::replan_question`]) has its replan too, with the
//! exits of `blocked_on_user` only: `retry` answers the question with the
//! text of the answer, so the task goes to `ready`; `replace` cancels the
//! task (A cancel) and answers its question. The time limit of the
//! question bounds the wait for the file, so the replan limit does not
//! apply.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{bail, Context, Result};
use horch_platform::channel::User;
use rusqlite::{params, Connection};
use serde::Deserialize;
use serde_json::json;

use crate::digest::sha256_bytes;
use crate::fleetd::fleet_runtime::application::jobs::JobCtx;
use crate::fleetd::fleet_runtime::application::ledger::Cause;
use crate::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use crate::fleetd::fleet_runtime::domain::ledger::Generation;
use crate::fleetd::ideas::adapters::idea_folder_files::IdeaFolder;
use crate::fleetd::ideas::adapters::ideas_store::{self, IdeaRow};
use crate::fleetd::ideas::adapters::plan_yaml;
use crate::fleetd::ideas::application::answers;
use crate::fleetd::ideas::application::plan_commit::{
    plan_facts, planned_task, read_file, read_ticket, PlanCommit, PLAN_YAML,
};
use crate::fleetd::ideas::application::questions;
use crate::fleetd::ideas::domain::plan::Plan;
use crate::fleetd::ideas::domain::plan_validator::{PlanValidator, Problem};
use crate::fleetd::ideas::domain::questions::Question;
use crate::fleetd::task_execution::application::cancel;
use crate::fleetd::task_execution::application::planned_tasks::{
    self, replace_planned_task, Replaced,
};
use crate::fleetd::task_execution::domain::retry::replan_overdue;
use crate::fsx;
use crate::taskledger::map::ticket_problems;
use crate::taskledger::model::{Actor, TaskStatus};
use crate::taskledger::tasks::{self, Task};
use crate::taskledger::{runs, Store};

/// The rule name of a problem of reading the replan file.
const READ: &str = "read";

/// The mode of a verdict file.
const VERDICT_MODE: u32 = 0o600;

/// What the replan needs besides the ledger: the facts of the plan
/// validator, and the requeue of a retry.
pub struct Replan<'a> {
    pub plan: PlanCommit<'a>,
    pub jobs: JobCtx<'a>,
}

/// The result of 1 replan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Replanned {
    /// The task waits for its replan file, within its replan limit.
    Waiting,
    /// The task is `ready` again with its attempt count at 0.
    Retried,
    /// The old task is `cancelled`, and the new tasks are in the ledger:
    /// (key, task id) of each, in the order of the file.
    Replaced { tasks: Vec<(String, String)> },
    /// The replan file is refused, with each problem. The ledger is as
    /// before.
    Refused { problems: Vec<Problem> },
    /// The task is past its replan limit: `escalated`, with 1 needs-you
    /// item.
    Escalated,
}

/// The replan file of the task `key`, in its idea folder.
pub fn replan_file(key: &str) -> String {
    format!("replan-{key}.yaml")
}

/// The verdict of fleetd on the replan file of the task `key`.
pub fn verdict_file(key: &str) -> String {
    format!("replan-{key}.verdict.json")
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReplanKeys {
    replace: Option<Vec<serde_yaml::Value>>,
    retry: Option<String>,
}

/// The decision of a replan file.
enum Decision {
    Retry(String),
    Replace(Vec<serde_yaml::Value>),
}

/// The `failed` task of a replan, with its place in the plan of its idea.
struct Failed {
    task: Task,
    key: String,
    idea: IdeaRow,
    /// For a task in `blocked_on_user`: its question with a `replan`
    /// answer, and the text of that answer.
    question: Option<(Question, String)>,
}

/// The plan of the idea after a replace, with what the commit needs.
struct Combined {
    plan: Plan,
    /// The number of tasks of `plan` before the new tasks.
    kept: usize,
    /// The text of the ticket of each new task.
    tickets: Vec<Option<String>>,
    /// The tasks of the idea that stay: key -> task id.
    live: BTreeMap<String, String>,
}

/// The replan of the `failed` task `task_id` at fleet clock `clock`, or of
/// the task `task_id` in `blocked_on_user` whose question has a `replan`
/// answer. A task in another state is an error that writes nothing.
pub fn replan_task(
    store: &mut Store,
    ctx: &Replan<'_>,
    task_id: &str,
    clock: FleetSeconds,
) -> Result<Replanned> {
    let failed = failed_task(store.conn(), task_id)?;
    if failed.question.is_none() && replan_overdue(failed.task.due_at, clock.0) {
        store.immediate(|tx| {
            let now = tasks::get(tx, task_id)?;
            if now.status != TaskStatus::Failed.as_str() {
                bail!("{} left failed before its escalation", now.short());
            }
            tasks::escalate(tx, task_id)
        })?;
        return Ok(Replanned::Escalated);
    }
    if !file_word(&failed.key) {
        bail!(
            "the key `{}` of {} is not a file name: it has no replan file",
            failed.key,
            failed.task.short()
        );
    }
    let owner = ctx.plan.owner;
    let folder = IdeaFolder::new(ctx.plan.ideas, &failed.idea.id, owner.clone())
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let dir = folder.dir();
    let name = replan_file(&failed.key);
    match std::fs::symlink_metadata(dir.join(&name)) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Replanned::Waiting),
        _ => {}
    }
    let decision = match read_decision(dir, owner, &name) {
        Ok(d) => d,
        Err(p) => return refuse(dir, &failed.key, vec![p]),
    };
    match decision {
        Decision::Retry(reason) => {
            let reason = format!("replan: {reason}");
            store.immediate(|tx| {
                let now = tasks::get(tx, task_id)?;
                if now.status != failed.task.status || now.replans != failed.task.replans {
                    bail!("{} changed before its replan", now.short());
                }
                match &failed.question {
                    Some((q, answer)) => retry_blocked(tx, ctx.plan.guard, &now, q, answer),
                    None => cancel::requeue(tx, &ctx.jobs, &now, &reason).map(drop),
                }
            })?;
            write_verdict(dir, &failed.key, true, &[], &[])?;
            Ok(Replanned::Retried)
        }
        Decision::Replace(entries) => {
            let combined = match combine(store.conn(), dir, owner, &failed, entries) {
                Ok(c) => c,
                Err(p) => return refuse(dir, &failed.key, vec![p]),
            };
            let facts = match plan_facts(store.conn(), &folder, &ctx.plan, &failed.idea.id) {
                Ok(f) => f,
                Err(p) => return refuse(dir, &failed.key, vec![p]),
            };
            let problems = PlanValidator::validate(&combined.plan, &facts);
            if !problems.is_empty() {
                return refuse(dir, &failed.key, problems);
            }
            let ids = store.immediate(|tx| commit(tx, ctx, &failed, &combined, dir, &name))?;
            write_verdict(dir, &failed.key, true, &[], &ids)?;
            Ok(Replanned::Replaced { tasks: ids })
        }
    }
}

/// The `failed` task `task_id`, or the task in `blocked_on_user` with a
/// `replan` answer, with its key in the plan and its idea.
fn failed_task(conn: &Connection, task_id: &str) -> Result<Failed> {
    let task = tasks::get(conn, task_id)?;
    let question = if task.status == TaskStatus::BlockedOnUser.as_str() {
        answers::replan_question(conn, task_id)?
    } else {
        None
    };
    if task.status != TaskStatus::Failed.as_str() && question.is_none() {
        bail!(
            "{} is {}, not failed and with no replan answer: it has no replan",
            task.short(),
            task.status
        );
    }
    let committed = planned_tasks::committed_task(conn, task_id)?;
    let Some((idea_id, key)) = committed.and_then(|c| Some((c.idea_id, spec_key(&c.spec_json)?)))
    else {
        bail!(
            "{} is not a task of a committed plan: it has no replan file",
            task.short()
        );
    };
    let idea = ideas_store::idea(conn, &idea_id)?
        .with_context(|| format!("the ledger has no idea {idea_id}"))?;
    Ok(Failed {
        task,
        key,
        idea,
        question,
    })
}

/// The retry of the task `task` in `blocked_on_user`: its replan is used,
/// and its question `q` is answered with the text `answer` of the
/// sub-orchestrator, so the task goes to `ready` and its worker resumes.
fn retry_blocked(
    tx: &Connection,
    guard: Option<Generation>,
    task: &Task,
    q: &Question,
    answer: &str,
) -> Result<()> {
    planned_tasks::use_replan(tx, guard, &task.task_id)?;
    questions::answered(tx, q, Some(answer))
}

/// Read the replan file `name` with the checks of SR-6, and its decision.
fn read_decision(dir: &Path, owner: &User, name: &str) -> Result<Decision, Problem> {
    let text =
        read_ticket(dir, owner, name).map_err(|r| Problem::plan(READ, format!("{name}: {r}")))?;
    let keys: ReplanKeys =
        serde_yaml::from_str(&text).map_err(|e| Problem::plan(READ, format!("{name}: {e}")))?;
    match (keys.replace, keys.retry) {
        (Some(entries), None) if !entries.is_empty() => Ok(Decision::Replace(entries)),
        (None, Some(reason)) if !reason.trim().is_empty() => {
            Ok(Decision::Retry(reason.trim().to_string()))
        }
        _ => Err(Problem::plan(
            READ,
            format!("{name} has exactly 1 of `replace:` with tasks and `retry:` with a reason"),
        )),
    }
}

/// The plan of the idea after the replace: the header of the committed
/// plan.yaml, each task of the idea that stays, then `entries`. A task that
/// stays is not `cancelled`, is not the failed task, and is not a judge
/// task of it (a judge follows the cancel of the task it judges).
fn combine(
    conn: &Connection,
    dir: &Path,
    owner: &User,
    failed: &Failed,
    entries: Vec<serde_yaml::Value>,
) -> Result<Combined, Problem> {
    let read = |m: String| Problem::plan(READ, m);
    let bytes = read_file(dir, owner, PLAN_YAML).map_err(|r| read(format!("{PLAN_YAML}: {r}")))?;
    if failed.idea.plan_sha256.as_deref() != Some(sha256_bytes(&bytes).hex().as_str()) {
        return Err(read(format!(
            "{PLAN_YAML} is not the plan that fleetd committed"
        )));
    }
    let mut doc: serde_yaml::Value =
        serde_yaml::from_slice(&bytes).map_err(|e| read(format!("{PLAN_YAML}: {e}")))?;
    let (kept, live) = kept_tasks(conn, failed).map_err(|e| read(format!("{e:#}")))?;
    let count = kept.len();
    let mut all = kept;
    all.extend(entries);
    let serde_yaml::Value::Mapping(map) = &mut doc else {
        return Err(read(format!("{PLAN_YAML} is not a mapping")));
    };
    map.insert("tasks".into(), serde_yaml::Value::Sequence(all));
    let text = serde_yaml::to_string(&doc).map_err(|e| read(format!("{e}")))?;
    let mut plan = plan_yaml::parse(&text).map_err(|e| read(format!("the replan: {e:#}")))?;
    let tickets = plan.tasks[count..]
        .iter_mut()
        .map(|task| {
            let ticket = task.ticket.as_mut()?;
            match read_ticket(dir, owner, &ticket.path) {
                Ok(text) => {
                    ticket.problems = ticket_problems(&text)
                        .into_iter()
                        .map(|(line, p)| format!("line {line}: {p}"))
                        .collect();
                    Some(text)
                }
                Err(reason) => {
                    ticket.problems = vec![reason];
                    None
                }
            }
        })
        .collect();
    Ok(Combined {
        plan,
        kept: count,
        tickets,
        live,
    })
}

/// A key that names 1 file of the idea folder: letters, digits, `.`, `_`
/// and `-`, and no `.` first. A plan key can hold any text.
fn file_word(key: &str) -> bool {
    !key.is_empty()
        && !key.starts_with('.')
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

/// The key of the task as committed in `spec_json`.
fn spec_key(spec_json: &str) -> Option<String> {
    let spec: serde_json::Value = serde_json::from_str(spec_json).ok()?;
    spec["key"].as_str().map(str::to_string)
}

/// The task of plan.yaml form of each task of the idea that stays, oldest
/// first, and key -> task id of each.
fn kept_tasks(
    conn: &Connection,
    failed: &Failed,
) -> Result<(Vec<serde_yaml::Value>, BTreeMap<String, String>)> {
    let old = failed.task.task_id.as_str();
    let mut specs = Vec::new();
    let mut live = BTreeMap::new();
    for t in planned_tasks::committed_tasks(conn, &failed.idea.id)? {
        let stays = t.status != TaskStatus::Cancelled.as_str()
            && t.task_id != old
            && t.judged.as_deref() != Some(old);
        if !stays {
            continue;
        }
        let spec: serde_json::Value = serde_json::from_str(&t.spec_json)
            .with_context(|| format!("the spec_json of task {}", t.task_id))?;
        let key = spec["key"]
            .as_str()
            .with_context(|| format!("the spec_json of task {} has no key", t.task_id))?;
        live.insert(key.to_string(), t.task_id.clone());
        specs.push(serde_yaml::to_value(&spec)?);
    }
    Ok((specs, live))
}

/// Commit the replace in `tx`: the old task follows A cancel, the new
/// tasks go in, and each new task gets its `task_covers` rows.
fn commit(
    tx: &Connection,
    ctx: &Replan<'_>,
    failed: &Failed,
    combined: &Combined,
    dir: &Path,
    name: &str,
) -> Result<Vec<(String, String)>> {
    let text = format!("{} is replaced by its replan", failed.task.short());
    let cause = Cause {
        actor: Actor::Horch,
        text: &text,
    };
    let run = runs::of_feature(tx, &failed.idea.feature_id)?.with_context(|| {
        format!(
            "the feature {} of {} has no internal run",
            failed.idea.feature_id, failed.idea.id
        )
    })?;
    let new = &combined.plan.tasks[combined.kept..];
    let planned: Vec<planned_tasks::PlannedTask> = new
        .iter()
        .zip(&combined.tickets)
        .map(|(task, ticket)| {
            let mut p = planned_task(task, ticket.as_deref(), dir);
            if task.ticket.is_none() {
                p.task_file = dir.join(name).to_string_lossy().into_owned();
            }
            p
        })
        .collect();
    let replaced = Replaced {
        old: &failed.task,
        key: &failed.key,
        reason: &text,
        live: &combined.live,
    };
    let ids = replace_planned_task(
        tx,
        ctx.plan.guard,
        &run,
        &failed.idea.id,
        &replaced,
        &planned,
        cause,
    )?;
    for (task, (_, id)) in new.iter().zip(&ids) {
        for req in &task.covers {
            tx.execute(
                "INSERT INTO task_covers (task_id, idea_id, req_id) VALUES (?1, ?2, ?3)",
                params![id, failed.idea.id, req],
            )?;
        }
    }
    if let Some((q, answer)) = &failed.question {
        questions::answered(tx, q, Some(answer))?;
    }
    Ok(ids)
}

/// Write the verdict of a refused replan file. The ledger is as before.
fn refuse(dir: &Path, key: &str, problems: Vec<Problem>) -> Result<Replanned> {
    write_verdict(dir, key, false, &problems, &[])?;
    Ok(Replanned::Refused { problems })
}

/// Write `replan-<key>.verdict.json` in the idea folder `dir`, atomically.
fn write_verdict(
    dir: &Path,
    key: &str,
    valid: bool,
    problems: &[Problem],
    tasks: &[(String, String)],
) -> Result<()> {
    let verdict = json!({
        "task": key,
        "valid": valid,
        "problems": problems
            .iter()
            .map(|p| json!({ "rule": p.rule, "task": p.task, "message": p.message }))
            .collect::<Vec<_>>(),
        "tasks": tasks
            .iter()
            .map(|(key, id)| json!({ "key": key, "task": id }))
            .collect::<Vec<_>>(),
    });
    let mut text = serde_json::to_string_pretty(&verdict)?;
    text.push('\n');
    let name = verdict_file(key);
    fsx::write_atomic(&dir.join(&name), text.as_bytes(), VERDICT_MODE)
        .with_context(|| format!("writing {name} of {}", dir.display()))
}
