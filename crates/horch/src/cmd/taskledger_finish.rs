use std::process::ExitCode;

use anyhow::Result;
use horch_core::runtime::RuntimeContext;

const FINISH_SUMMARY: &str =
    "The worker ran horch ledger finish: the work is complete and committed.";

pub fn finish_cmd(ctx: &RuntimeContext) -> Result<ExitCode> {
    super::taskledger::write_completed(ctx, FINISH_SUMMARY, None)
}
