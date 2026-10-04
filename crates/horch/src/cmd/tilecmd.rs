//! `horch tile` - rearrange every pane in the workspace into the fleet grid.
//!
//! The herdr call sequence lives in [`horch_core::workspace::arrange`]; this is
//! the command: it waits, picks the workspace and prints the report.

use std::time::Duration;

use anyhow::Result;
use horch_core::runtime::RuntimeContext;
use horch_core::workspace::arrange;
use horch_core::workspace::herdr::Herdr;

use crate::output;

/// `horch tile`.
pub fn tile(
    ctx: &RuntimeContext,
    pane: Option<&str>,
    workspace: Option<&str>,
    plan_only: bool,
    settle_ms: u64,
) -> Result<()> {
    if settle_ms > 0 {
        std::thread::sleep(Duration::from_millis(settle_ms));
    }
    let herdr = Herdr::with_bin(&ctx.bins.harness.herdr);
    let workspace_id = arrange::workspace_of(ctx, &herdr, pane, workspace)?;
    let out = arrange::run(ctx, &herdr, &workspace_id, None, plan_only)?;
    output::print(&out);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tiling skips the telemetry workspace by its label, so core's copy of
    /// the label must match the one `horch telemetry` creates.
    #[test]
    fn the_telemetry_label_matches_the_one_tiling_skips() {
        assert_eq!(
            arrange::TELEMETRY_WORKSPACE_LABEL,
            crate::cmd::telemetry::WORKSPACE_LABEL
        );
    }
}
