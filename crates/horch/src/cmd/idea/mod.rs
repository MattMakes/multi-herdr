//! `horch idea` and `horch ideas`: the operator captures, approves and
//! steers ideas, and the sub-orchestrator records notes
//! (`docs/specs/fleetd/design.md`, "Ideas"). 1 file for each subcommand;
//! each body is a stub until the ticket of fleetd-p6 that it names fills it.

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
use horch_core::runtime::RuntimeContext;

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

/// The exit of a subcommand whose ticket has not filled it yet:
/// "not built yet: ticket NN of fleetd-p6" on stderr, and exit 1.
fn not_built_yet(command: &str, ticket: u8) -> Result<ExitCode> {
    eprintln!("{command}: not built yet: ticket {ticket:02} of fleetd-p6");
    Ok(ExitCode::from(1))
}
