//! `horch result`: the worker writes `outbox/result.json` of its attempt
//! (`docs/specs/fleetd/design.md`, "Workspace layout and file protocol").
//! `--check` is the test of the Stop hooks. Ticket 12 builds it.

use std::process::ExitCode;

use anyhow::Result;
use clap::{Args, ValueEnum};
use horch_core::runtime::RuntimeContext;

/// The closed list of statuses of `result.json`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum ResultStatus {
    Completed,
    Blocked,
    Failed,
}

#[derive(Args)]
pub struct ResultArgs {
    /// What the attempt did, in 1 paragraph.
    #[arg(long)]
    pub summary: Option<String>,
    /// The claim of the worker. Only the verifier moves a task to done.
    #[arg(long, value_enum)]
    pub status: Option<ResultStatus>,
    /// A commit of the attempt. Give it 1 time for each commit.
    #[arg(long, value_name = "SHA")]
    pub commit: Vec<String>,
    /// A file that the attempt changed. Give it 1 time for each file.
    #[arg(long, value_name = "PATH")]
    pub file: Vec<String>,
    /// Notes for the next attempt.
    #[arg(long)]
    pub notes: Option<String>,
    /// Write nothing. Exit 2 while both `result.json` and a question file
    /// are missing: the test of the Stop hooks.
    #[arg(long)]
    pub check: bool,
}

/// `horch result`.
pub fn run(_ctx: &RuntimeContext, _args: ResultArgs) -> Result<ExitCode> {
    Ok(super::not_built_yet("horch result"))
}
