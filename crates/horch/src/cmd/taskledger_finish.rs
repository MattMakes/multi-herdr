//! `horch ledger finish` (ticket 27 of fleetd-p5): the worker writes
//! `result.json` with `status: completed` to the outbox of its own attempt.
//! fleetd runs the finish steps (Phase 4) and reports the result to the
//! orchestrator, as `horch ledger done` does. The command opens the ledger
//! only to read.

use std::process::ExitCode;

use anyhow::Result;
use horch_core::runtime::RuntimeContext;

/// The summary of a `result.json` that `horch ledger finish` writes.
const FINISH_SUMMARY: &str =
    "The worker ran horch ledger finish: the work is complete and committed.";

/// `horch ledger finish`. Its flags of Phase 4 have no effect.
pub fn finish_cmd(ctx: &RuntimeContext) -> Result<ExitCode> {
    super::taskledger::write_completed(ctx, FINISH_SUMMARY, None)
}
