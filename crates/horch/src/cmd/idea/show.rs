//! `horch idea show <id> [--in-flight] [--json]`: 1 idea, and with
//! `--in-flight` the diffs of the features in flight
//! (`docs/specs/fleetd/design.md`, "Ideas"). Ticket 24 of fleetd-p6 fills it.

use std::process::ExitCode;

use anyhow::Result;
use clap::Args;
use horch_core::runtime::RuntimeContext;

#[derive(Args)]
pub struct ShowArgs {
    /// The idea id.
    pub id: String,
    /// Also print the diffs of the features in flight, to build on them.
    #[arg(long)]
    pub in_flight: bool,
    /// Print the result as JSON.
    #[arg(long)]
    pub json: bool,
}

/// `horch idea show`.
pub fn run(_ctx: &RuntimeContext, _args: ShowArgs) -> Result<ExitCode> {
    super::not_built_yet("horch idea show", 24)
}
