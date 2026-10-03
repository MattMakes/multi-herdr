//! `multi-herdr-dataset`: the separate, expensive entrypoint (OD2, CMP-01).
//!
//! The binary in `src/bin/multi-herdr-dataset.rs` parses [`cli::Cli`],
//! builds the [`RuntimeContext`] and calls [`dispatch`]. Each command lives
//! in its own module here.

use std::collections::BTreeMap;

use anyhow::Result;
use horch_core::measure::paths::DatasetPaths;
use horch_core::runtime::RuntimeContext;

pub mod cli;
pub mod preflight;
pub mod run;

use cli::Command;

/// The exit codes of `multi-herdr-dataset` (B2).
pub mod exit {
    pub const SUCCESS: u8 = 0;
    /// Any error that is not one of the codes below.
    pub const FAILURE: u8 = 1;
    /// A hidden command that a later build fills.
    pub const NOT_IMPLEMENTED: u8 = 2;
    /// The budget or the quota refused the run.
    pub const BUDGET_REFUSED: u8 = 3;
    /// Preflight failed: no worktree was created and no model was called.
    pub const PREFLIGHT_FAILED: u8 = 4;
    /// The round needs the operator.
    pub const NEEDS_INTERVENTION: u8 = 5;
    /// The round was rejected.
    pub const REJECTED: u8 = 6;
}

/// Run `command`. `env` holds the process variables that the environment
/// snapshot may keep (SEC-02); the binary collects them once.
pub fn dispatch(
    ctx: &mut RuntimeContext,
    env: &BTreeMap<String, String>,
    command: Command,
) -> Result<u8> {
    match command {
        Command::Run(args) => run::run(ctx, env, &args),
        Command::Status { .. } => not_implemented("status"),
        Command::Export { .. } => not_implemented("export"),
        Command::Readiness { .. } => not_implemented("readiness"),
        Command::Outcome { .. } => not_implemented("outcome"),
        Command::Rebuild { .. } => not_implemented("rebuild"),
        Command::JudgeJob(_) => not_implemented("judge-job"),
        Command::Promote(_) => not_implemented("promote"),
        Command::Rollback(_) => not_implemented("rollback"),
        Command::Cleanup(_) => not_implemented("cleanup"),
        Command::Watch(_) => not_implemented("watch"),
    }
}

fn not_implemented(name: &str) -> Result<u8> {
    eprintln!("multi-herdr-dataset: {name} is not implemented in this build");
    Ok(exit::NOT_IMPLEMENTED)
}

/// The dataset of the context's project (OD3).
pub fn dataset_paths(ctx: &RuntimeContext) -> Result<DatasetPaths> {
    Ok(DatasetPaths::new(
        &ctx.paths.state_root,
        &ctx.paths.project()?,
    ))
}
