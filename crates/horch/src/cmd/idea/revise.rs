//! `horch idea revise <id> <text>`: send an idea back with what to change
//! (`docs/specs/fleetd/design.md`, "Quick approval stays with you").
//! Ticket 23 of fleetd-p6 fills it.

use std::process::ExitCode;

use anyhow::Result;
use clap::Args;
use horch_core::runtime::RuntimeContext;

#[derive(Args)]
pub struct ReviseArgs {
    /// The idea id.
    pub id: String,
    /// What to change.
    pub text: String,
}

/// `horch idea revise`.
pub fn run(_ctx: &RuntimeContext, _args: ReviseArgs) -> Result<ExitCode> {
    super::not_built_yet("horch idea revise", 23)
}
