pub mod add;
pub mod amend;
pub mod approve;
pub mod board;
pub mod cancel;
pub mod note;
pub mod park;
pub mod revise;
pub mod show;

use std::process::ExitCode;

use anyhow::Result;
use clap::Subcommand;
use horch_core::fleetd::fleet_runtime::adapters::channel_client::{self, Sent};
use horch_core::fleetd::fleet_runtime::adapters::peer_process::{
    fleet_process, recorded_processes, Fact, PlatformFacts,
};
use horch_core::fleetd::fleet_runtime::adapters::request_wire::Outcome;
use horch_core::fleetd::fleet_runtime::domain::requests::Kind;
use horch_core::runtime::RuntimeContext;
use horch_core::taskledger;
use serde_json::{Map, Value};

#[derive(Subcommand)]
pub enum IdeaCommand {
    /// Capture an idea from text, from a file, or as a ticket file that
    /// skips the interview (`--direct`). No AI runs to capture it.
    Add(add::AddArgs),
    /// Add text to an idea. An amend after approval moves the idea back
    /// to awaiting approval.
    Amend(amend::AmendArgs),
    /// Print 1 idea: its state, what it waits on and its folder.
    Show(show::ShowArgs),
    /// Record 1 question and answer, or dismiss 1 finding, in the idea
    /// folder. The sub-orchestrator of the idea runs it.
    Note(note::NoteArgs),
    /// Approve the assumptions of an idea that fleetd printed.
    Approve(approve::ApproveArgs),
    /// Send an idea back with what to change.
    Revise(revise::ReviseArgs),
    /// Park an idea: its running tasks go on, and no new task starts.
    Park(park::ParkArgs),
    /// Cancel an idea. Each task of it that is not terminal is cancelled.
    Cancel(cancel::CancelArgs),
}

pub fn run(ctx: &RuntimeContext, command: IdeaCommand) -> Result<ExitCode> {
    match command {
        IdeaCommand::Add(args) => add::run(ctx, args),
        IdeaCommand::Amend(args) => amend::run(ctx, args),
        IdeaCommand::Show(args) => show::run(ctx, args),
        IdeaCommand::Note(args) => note::run(ctx, args),
        IdeaCommand::Approve(args) => approve::run(ctx, args),
        IdeaCommand::Revise(args) => revise::run(ctx, args),
        IdeaCommand::Park(args) => park::run(ctx, args),
        IdeaCommand::Cancel(args) => cancel::run(ctx, args),
    }
}

fn send(
    ctx: &RuntimeContext,
    command: &str,
    kind: Kind,
    payload: Map<String, Value>,
    print: impl FnOnce(&Value),
) -> Result<ExitCode> {
    match channel_client::send(ctx, kind, payload)? {
        Sent::Replied(reply) => match reply.outcome {
            Outcome::Applied(result) => {
                print(&result);
                Ok(ExitCode::SUCCESS)
            }
            Outcome::Refused(reason) => {
                eprintln!("{command}: refused: {reason}");
                Ok(ExitCode::from(1))
            }
        },
        Sent::Queued { request_id, path } => {
            println!(
                "request {request_id}: waits for fleetd in {}",
                path.display()
            );
            Ok(ExitCode::SUCCESS)
        }
    }
}

fn text<'a>(result: &'a Value, key: &str) -> &'a str {
    result.get(key).and_then(Value::as_str).unwrap_or("-")
}

pub(crate) fn refuse_inside_fleet_attempt(ctx: &RuntimeContext, command: &str) -> Option<ExitCode> {
    let reason = match attempt_fact(ctx) {
        Fact::No => return None,
        Fact::Yes(attempts) => format!(
            "this process is a fleet process of attempt {}",
            attempts.join(", ")
        ),
        Fact::Unknown => {
            "horch cannot prove that this process runs outside a fleet attempt, because a process fact is unknown".to_string()
        }
    };
    eprintln!(
        "{command}: refused inside a fleet attempt: {reason}. Run it yourself from a terminal outside the fleet."
    );
    Some(ExitCode::from(1))
}

fn attempt_fact(ctx: &RuntimeContext) -> Fact {
    let Ok(store) = taskledger::open_existing_read_only(ctx) else {
        return Fact::Unknown;
    };
    let Some(store) = store else {
        return Fact::No;
    };
    match recorded_processes(store.conn()) {
        Ok(Some((recorded, _))) => fleet_process(std::process::id(), &recorded, &PlatformFacts),
        Ok(None) | Err(_) => Fact::Unknown,
    }
}
