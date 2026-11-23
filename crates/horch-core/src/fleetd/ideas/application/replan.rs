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

const READ: &str = "read";

const VERDICT_MODE: u32 = 0o600;

pub struct Replan<'a> {
    pub plan: PlanCommit<'a>,
    pub jobs: JobCtx<'a>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Replanned {
    Waiting,
    Retried,
    Replaced { tasks: Vec<(String, String)> },
    Refused { problems: Vec<Problem> },
    Escalated,
}

pub fn replan_file(key: &str) -> String {
    format!("replan-{key}.yaml")
}

pub fn verdict_file(key: &str) -> String {
    format!("replan-{key}.verdict.json")
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReplanKeys {
    replace: Option<Vec<serde_yaml::Value>>,
    retry: Option<String>,
}

enum Decision {
    Retry(String),
    Replace(Vec<serde_yaml::Value>),
}

struct Failed {
    task: Task,
    key: String,
    idea: IdeaRow,
    question: Option<(Question, String)>,
}

struct Combined {
    plan: Plan,
    kept: usize,
    tickets: Vec<Option<String>>,
    live: BTreeMap<String, String>,
}

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

fn file_word(key: &str) -> bool {
    !key.is_empty()
        && !key.starts_with('.')
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

fn spec_key(spec_json: &str) -> Option<String> {
    let spec: serde_json::Value = serde_json::from_str(spec_json).ok()?;
    spec["key"].as_str().map(str::to_string)
}

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

fn refuse(dir: &Path, key: &str, problems: Vec<Problem>) -> Result<Replanned> {
    write_verdict(dir, key, false, &problems, &[])?;
    Ok(Replanned::Refused { problems })
}

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
