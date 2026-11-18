//! `horch rescue drop <ref>`: the operator deletes a `unique` rescue ref
//! (`docs/specs/fleetd/design.md`, "Features, landing and integration").
//! Ticket 43 builds it and adds the role check.

use std::process::ExitCode;

use anyhow::Result;
use clap::Subcommand;
use horch_core::runtime::RuntimeContext;

#[derive(Subcommand)]
pub enum RescueCommand {
    /// Delete a rescue ref. No collection deletes a `unique` ref: only
    /// this command or `horch worktree restore` ends it.
    Drop {
        /// The rescue ref.
        #[arg(value_name = "REF")]
        rescue_ref: String,
    },
}

/// `horch rescue`.
pub fn run(_ctx: &RuntimeContext, command: RescueCommand) -> Result<ExitCode> {
    match command {
        RescueCommand::Drop { .. } => Ok(super::not_built_yet("horch rescue drop")),
    }
}
