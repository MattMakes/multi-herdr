//! `horch idea` and `horch ideas`: the operator captures, approves and
//! steers ideas, and the sub-orchestrator records notes
//! (`docs/specs/fleetd/design.md`, "Ideas"). 1 file for each subcommand.
//! Each write command sends its request to fleetd with the channel client.

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
    fleet_process, launching_attempt, recorded_processes, Fact, PlatformFacts,
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

/// `horch idea`.
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

/// Send 1 operator request of `kind` with the channel client, or write a
/// request file when fleetd is down. An applied request gives its result
/// to `print`, and exits 0. A refused request prints its reason on stderr,
/// and exits 1. A queued request prints its id and its file, and exits 0.
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

/// The text of `key` in the result of a request, else `-`.
fn text<'a>(result: &'a Value, key: &str) -> &'a str {
    result.get(key).and_then(Value::as_str).unwrap_or("-")
}

/// The first guard of `horch idea approve`, `horch idea add --direct` and
/// `horch feature abandon` (SR-5, FR-10): `Some(exit 1)` when this process
/// is a fleet process: the rule [`fleet_process`] of the channel server,
/// never an environment variable. A fact that is unknown refuses too: a
/// guard whose fact is unknown does not hold. While an attempt launches
/// with no recorded process ([`launching_attempt`]), a process that is not
/// proven inside an attempt is refused too, and told to try again. This
/// guard only gives an early message. The checks of fleetd at the server stay the wall.
pub(crate) fn refuse_inside_fleet_attempt(ctx: &RuntimeContext, command: &str) -> Option<ExitCode> {
    let reason = match attempt_fact(ctx) {
        AttemptFact::Launching(attempt) => format!(
            "attempt {attempt} launches and fleetd has not recorded its process yet, so horch cannot prove that this process is not a fleet process of it. Try again in some seconds"
        ),
        AttemptFact::Known(Fact::No) => return None,
        AttemptFact::Known(Fact::Yes(attempts)) => format!(
            "this process is a fleet process of attempt {}",
            attempts.join(", ")
        ),
        AttemptFact::Known(Fact::Unknown) => {
            "horch cannot prove that this process runs outside a fleet attempt, because a process fact is unknown".to_string()
        }
    };
    eprintln!(
        "{command}: refused inside a fleet attempt: {reason}. Run it yourself from a terminal outside the fleet."
    );
    Some(ExitCode::from(1))
}

enum AttemptFact {
    Known(Fact),
    Launching(String),
}

fn attempt_fact(ctx: &RuntimeContext) -> AttemptFact {
    let Ok(store) = taskledger::open_existing_read_only(ctx) else {
        return AttemptFact::Known(Fact::Unknown);
    };
    let Some(store) = store else {
        return AttemptFact::Known(Fact::No);
    };
    let Ok(Some((recorded, rows))) = recorded_processes(store.conn()) else {
        return AttemptFact::Known(Fact::Unknown);
    };
    match fleet_process(std::process::id(), &recorded, &PlatformFacts) {
        Fact::No => launching_attempt(&rows).map_or(AttemptFact::Known(Fact::No), |a| {
            AttemptFact::Launching(a.to_string())
        }),
        fact => AttemptFact::Known(fact),
    }
}
