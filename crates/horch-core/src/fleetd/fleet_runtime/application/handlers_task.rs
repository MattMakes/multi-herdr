use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use rusqlite::Connection;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Value};

use super::jobs::{JobCtx, Queued};
use super::requests::Handlers;
use crate::fleetd::fleet_runtime::adapters::clock_store;
use crate::fleetd::fleet_runtime::domain::config::FleetConfig;
use crate::fleetd::fleet_runtime::domain::requests::Kind;
use crate::fleetd::ideas::application::needs_you::NeedsYouList;
use crate::fleetd::task_execution::application::cancel::{self, Cancel};
use crate::fleetd::worktree_custody::adapters::sqlite as custody;
use crate::fleetd::worktree_custody::application::remove;
use crate::taskledger::finish::codesweep;
use crate::taskledger::import;
use crate::taskledger::model::FindingStatus;
use crate::taskledger::runs::{self, Run};
use crate::taskledger::tasks::{self, NewTask};

pub struct TaskHandlers {
    pub config: Arc<FleetConfig>,
    pub evidence_root: PathBuf,
}

type TaskHandler = fn(&TaskHandlers, &Connection, &Value) -> Result<Value>;

pub fn register(handlers: &mut Handlers, ctx: TaskHandlers) {
    let ctx = Arc::new(ctx);
    let entries: [(Kind, TaskHandler); 6] = [
        (Kind::TaskRequeue, requeue),
        (Kind::TaskCancel, cancel),
        (Kind::TaskFallback, fallback),
        (Kind::LedgerWaive, waive),
        (Kind::LedgerReopen, reopen),
        (Kind::LedgerDismiss, dismiss),
    ];
    for (kind, handler) in entries {
        let ctx = Arc::clone(&ctx);
        handlers.insert(kind, move |tx, payload| handler(&ctx, tx, payload));
    }
}

#[derive(Deserialize)]
struct Target {
    project: PathBuf,
    #[serde(default)]
    run: Option<String>,
    #[serde(default)]
    workspace: Option<String>,
}

#[derive(Deserialize)]
struct TaskReason {
    task: String,
    reason: String,
}

#[derive(Deserialize)]
struct WaiveArgs {
    task: String,
    finding: String,
    reason: String,
}

#[derive(Deserialize)]
struct ReopenArgs {
    finding: i64,
    file: String,
    title: String,
    primary: String,
    #[serde(default)]
    fallback: Option<String>,
    #[serde(default)]
    check: Option<String>,
}

#[derive(Deserialize)]
struct DismissArgs {
    finding: i64,
    reason: String,
    #[serde(default)]
    cleanup: bool,
}

fn parse<T: DeserializeOwned>(payload: &Value) -> Result<T> {
    T::deserialize(payload).context("the payload of the request is not valid")
}

fn run_of(tx: &Connection, payload: &Value) -> Result<(Run, Option<String>)> {
    let t: Target = parse(payload)?;
    if !t.project.is_absolute() {
        bail!("the project root {} is not absolute", t.project.display());
    }
    runs::resolve(tx, &t.project, t.run.as_deref(), t.workspace.as_deref())
}

fn result(mut values: Value, lines: Vec<String>, note: Option<String>) -> Value {
    values["lines"] = json!(lines);
    if let Some(note) = note {
        values["note"] = json!(note);
    }
    values
}

fn job_ctx(ctx: &TaskHandlers) -> JobCtx<'_> {
    JobCtx {
        guard: None,
        config: &ctx.config,
        needs_you: &NeedsYouList,
    }
}

fn requeue(ctx: &TaskHandlers, tx: &Connection, payload: &Value) -> Result<Value> {
    let a: TaskReason = parse(payload)?;
    let (run, note) = run_of(tx, payload)?;
    let t = tasks::resolve(tx, &run.run_id, &a.task)?;
    let t = cancel::requeue(tx, &job_ctx(ctx), &t, &a.reason)?;
    let line = format!("{} is queued again.", t.short());
    Ok(result(json!({ "task": t.short() }), vec![line], note))
}

fn cancel(_: &TaskHandlers, tx: &Connection, payload: &Value) -> Result<Value> {
    let a: TaskReason = parse(payload)?;
    let (run, note) = run_of(tx, payload)?;
    let t = tasks::resolve(tx, &run.run_id, &a.task)?;
    let asked = cancel::request(tx, &t, &a.reason)?;
    let judges: Vec<String> = asked
        .judges()
        .iter()
        .map(|j| {
            format!(
                "{} is the judge task of {}: its cancel follows.",
                j.task().short(),
                asked.task().short()
            )
        })
        .collect();
    let (values, line) = match asked {
        Cancel::Cancelled { task, holder, .. } => (
            json!({ "task": task.short(), "status": "cancelled", "holder": holder }),
            format!("{} cancelled.", task.short()),
        ),
        Cancel::Stopping { task, mut held, .. } => {
            held.sort();
            held.dedup();
            (
                json!({ "task": task.short(), "status": "stopping", "held": held }),
                format!(
                    "{} is not cancelled yet: a process of its attempt still runs. The next pass of fleetd cancels it when the process is gone.",
                    task.short()
                ),
            )
        }
    };
    let mut lines = judges;
    lines.push(line);
    Ok(result(values, lines, note))
}

fn fallback(_: &TaskHandlers, tx: &Connection, payload: &Value) -> Result<Value> {
    let a: TaskReason = parse(payload)?;
    let (run, note) = run_of(tx, payload)?;
    let t = tasks::resolve(tx, &run.run_id, &a.task)?;
    let t = tasks::fallback(tx, &t, &a.reason)?;
    let by = t.claimable_by.as_deref().unwrap_or("?");
    let line = format!("{} can now be claimed by {by}.", t.short());
    Ok(result(
        json!({ "task": t.short(), "claimable_by": by }),
        vec![line],
        note,
    ))
}

fn waive(ctx: &TaskHandlers, tx: &Connection, payload: &Value) -> Result<Value> {
    let a: WaiveArgs = parse(payload)?;
    let (run, note) = run_of(tx, payload)?;
    let t = tasks::resolve(tx, &run.run_id, &a.task)?;
    let w = codesweep::waive(tx, &ctx.evidence_root, &t, &a.finding, &a.reason)?;
    let line = format!(
        "{} round {}: {} waived. The next horch ledger finish of {} reads it.",
        t.short(),
        w.round,
        w.id,
        t.short()
    );
    Ok(result(
        json!({ "task": t.short(), "round": w.round, "finding": w.id }),
        vec![line],
        note,
    ))
}

fn reopen(_: &TaskHandlers, tx: &Connection, payload: &Value) -> Result<Value> {
    let a: ReopenArgs = parse(payload)?;
    let (run, note) = run_of(tx, payload)?;
    let f = import::get_finding(tx, a.finding)?;
    let mut new = NewTask::work(&a.title, &a.file, &a.primary);
    new.fallback = a.fallback;
    new.land_check = a.check;
    new.spec_ref = Some(format!("finding F{}", a.finding));
    let task = import::reopen(tx, &run, &new, &f)?;
    let line = format!(
        "{} queued for {} from finding F{}.",
        task.short(),
        a.primary,
        a.finding
    );
    Ok(result(json!({ "task": task.short() }), vec![line], note))
}

fn dismiss(ctx: &TaskHandlers, tx: &Connection, payload: &Value) -> Result<Value> {
    let a: DismissArgs = parse(payload)?;
    let f = import::get_finding(tx, a.finding)?;
    let mut lines = Vec::new();
    if a.cleanup {
        let Some(wt) = &f.worktree_id else {
            bail!(
                "finding F{} has no recorded worktree; --cleanup removes only a recorded path",
                a.finding
            );
        };
        let cause = format!("finding F{} dismissed: {}", a.finding, a.reason);
        lines.push(queue_cleanup(ctx, tx, wt, &cause)?);
    }
    import::decide(
        tx,
        a.finding,
        FindingStatus::Dismissed,
        "orchestrator",
        &a.reason,
    )?;
    lines.push(format!("Finding F{} dismissed.", a.finding));
    Ok(result(json!({ "finding": a.finding }), lines, None))
}

fn queue_cleanup(ctx: &TaskHandlers, tx: &Connection, wt: &str, cause: &str) -> Result<String> {
    let Some(row) = custody::get(tx, wt)? else {
        bail!("worktree {wt} is not in the ledger ({cause})");
    };
    let clock = clock_store::read(tx)?;
    let queued = match remove::queue_remove(&job_ctx(ctx), tx, &row, cause, false, clock)? {
        Ok(queued) => queued,
        Err(reason) => bail!("{reason}"),
    };
    Ok(match queued {
        Queued::New(id) => format!(
            "worktree {wt}: the worktree_remove job {id} is queued; fleetd removes the tree."
        ),
        Queued::Active(job) | Queued::Reused(job) => format!(
            "worktree {wt}: the worktree_remove job {} is already {}.",
            job.id,
            job.state.as_str()
        ),
        Queued::Held(job) => format!(
            "worktree {wt}: the worktree_remove job {} is held at its limit of abandoned runs.",
            job.id
        ),
    })
}
