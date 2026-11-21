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
    /// at 0; a failed task gets its 1 replan. A done task whose branch
    /// waits for a resolver or fix attempt gets its repair_attempts at 0
    /// and 1 more attempt. A cancelled task is terminal.
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

pub fn run(ctx: &RuntimeContext, command: TaskCommand) -> Result<ExitCode> {
    match command {
        TaskCommand::Requeue { task, reason, run } => send_requeue(ctx, &task, &reason, &run),
        TaskCommand::Cancel { task, reason, run } => send_cancel(ctx, &task, &reason, &run),
    }
}

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

#[cfg(test)]
mod tests {
    use clap::{CommandFactory, Parser};

    use super::TaskCommand;

    #[derive(Parser)]
    struct Cli {
        #[command(subcommand)]
        command: TaskCommand,
    }

    #[test]
    fn requeue_help_names_the_repair_attempts_of_a_done_task() {
        let mut cmd = Cli::command();
        let help = cmd
            .find_subcommand_mut("requeue")
            .expect("the requeue subcommand")
            .render_long_help()
            .to_string();
        assert!(help.contains("A done task whose branch"), "{help}");
        assert!(help.contains("repair_attempts at 0"), "{help}");
    }
}
