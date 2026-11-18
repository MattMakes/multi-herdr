//! `horch idea add <text> | -f <file> | --direct <ticket file>`: capture an
//! idea (`docs/specs/fleetd/design.md`, "Ideas", "Features"). Ticket 23 of
//! fleetd-p6 fills it.

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Result;
use clap::{ArgGroup, Args};
use horch_core::runtime::RuntimeContext;

#[derive(Args)]
#[command(group(
    ArgGroup::new("source")
        .required(true)
        .args(["text", "file", "direct"])
))]
pub struct AddArgs {
    /// The idea, in your words.
    pub text: Option<String>,
    /// Read the idea from this file.
    #[arg(short = 'f', long)]
    pub file: Option<PathBuf>,
    /// A ready ticket file: the idea skips the interview and the spec.
    #[arg(long, value_name = "TICKET_FILE")]
    pub direct: Option<PathBuf>,
    /// Add the idea to this open feature, for example `F-3`.
    #[arg(long, value_name = "FEATURE")]
    pub to: Option<String>,
    /// The priority of the idea. A higher number goes first.
    #[arg(long, value_name = "N")]
    pub priority: Option<i64>,
    /// Print the result as JSON.
    #[arg(long)]
    pub json: bool,
}

/// `horch idea add`.
pub fn run(_ctx: &RuntimeContext, _args: AddArgs) -> Result<ExitCode> {
    super::not_built_yet("horch idea add", 23)
}
