//! `horch ideas [--json]`: the board of every idea, its state and what it
//! waits on (`docs/specs/fleetd/design.md`, "Ideas"). Ticket 24 of
//! fleetd-p6 fills it.

use std::process::ExitCode;

use anyhow::Result;
use clap::Args;
use horch_core::runtime::RuntimeContext;

#[derive(Args)]
pub struct BoardArgs {
    /// Print the board as JSON.
    #[arg(long)]
    pub json: bool,
}

/// `horch ideas`.
pub fn run(_ctx: &RuntimeContext, _args: BoardArgs) -> Result<ExitCode> {
    super::not_built_yet("horch ideas", 24)
}
