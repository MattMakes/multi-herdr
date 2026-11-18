//! `horch task requeue` and `horch task cancel`: the commands of the
//! operator on 1 task (`docs/specs/fleetd/design.md`, "The task state
//! machine" and "A cancel").
//!
//! Each command sends 1 request to fleetd (`task_requeue`, `task_cancel`)
//! and writes no row itself (RR-5). [`send_requeue`] and [`send_cancel`]
//! are the send of `horch ledger requeue` and `horch ledger cancel` too.
//! fleetd applies the request: with a live attempt, the pass of fleetd
//! stops its processes and cancels the task when they are gone.

use std::process::ExitCode;

use anyhow::Result;
use clap::Subcommand;
use horch_core::fleetd::fleet_runtime::domain::requests::Kind;
use horch_core::runtime::RuntimeContext;
use serde_json::{json, Value};

use super::operator_request;
use super::taskledger::{project_root, RunArg};

#[derive(Subcommand)]
pub enum TaskCommand {
    /// Queue a held task again. An escalated task gets its attempt count
    /// at 0; a failed task gets its 1 replan. A cancelled task is terminal.
    Requeue {
        task: String,
        reason: String,
        #[command(flatten)]
        run: RunArg,
    },
    /// Cancel a task. A live attempt is stopped first by its recorded PIDs
    /// and pane; the task is cancelled when its process is gone.
    Cancel {
        task: String,
        reason: String,
        #[command(flatten)]
        run: RunArg,
    },
}

/// `horch task`.
pub fn run(ctx: &RuntimeContext, command: TaskCommand) -> Result<ExitCode> {
    match command {
        TaskCommand::Requeue { task, reason, run } => send_requeue(ctx, &task, &reason, &run),
        TaskCommand::Cancel { task, reason, run } => send_cancel(ctx, &task, &reason, &run),
    }
}

/// `horch task requeue` and `horch ledger requeue`: 1 `task_requeue`
/// request.
pub(super) fn send_requeue(
    ctx: &RuntimeContext,
    task: &str,
    reason: &str,
    run: &RunArg,
) -> Result<ExitCode> {
    let payload = task_payload(ctx, task, reason, run)?;
    operator_request::run(ctx, Kind::TaskRequeue, payload, |r| {
        operator_request::print_lines(r);
        Ok(())
    })
}

/// `horch task cancel` and `horch ledger cancel`: 1 `task_cancel` request.
pub(super) fn send_cancel(
    ctx: &RuntimeContext,
    task: &str,
    reason: &str,
    run: &RunArg,
) -> Result<ExitCode> {
    let payload = task_payload(ctx, task, reason, run)?;
    operator_request::run(ctx, Kind::TaskCancel, payload, |r| {
        operator_request::print_lines(r);
        Ok(())
    })
}

/// The payload of a request on 1 task: the project, the run that `--run`
/// or `HORCH_RUN_ID` names, the workspace, the task and the reason. fleetd
/// resolves the run as the command of today does.
fn task_payload(ctx: &RuntimeContext, task: &str, reason: &str, run: &RunArg) -> Result<Value> {
    let named = run
        .run
        .clone()
        .or_else(|| std::env::var("HORCH_RUN_ID").ok().filter(|v| !v.is_empty()));
    Ok(json!({
        "project": project_root(ctx)?,
        "run": named,
        "workspace": ctx.herdr.workspace.as_ref().map(|w| w.as_str().to_string()),
        "task": task,
        "reason": reason,
    }))
}
