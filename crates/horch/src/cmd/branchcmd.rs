//! `horch branch abandon <branch>`: the operator ends a task branch that
//! its resolver or fix attempts did not repair
//! (`docs/specs/fleetd/design.md`, "Features, landing and integration").
//! Ticket 40 builds it and adds the role check.

use std::process::ExitCode;

use anyhow::Result;
use clap::Subcommand;
use horch_core::runtime::RuntimeContext;

#[derive(Subcommand)]
pub enum BranchCommand {
    /// Abandon a task branch. fleetd writes a rescue ref of the branch
    /// first.
    Abandon {
        /// The task branch.
        branch: String,
    },
}

/// `horch branch`.
pub fn run(_ctx: &RuntimeContext, command: BranchCommand) -> Result<ExitCode> {
    match command {
        BranchCommand::Abandon { .. } => Ok(super::not_built_yet("horch branch abandon")),
    }
}
