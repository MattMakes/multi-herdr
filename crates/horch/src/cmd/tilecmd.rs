use std::time::Duration;

use anyhow::Result;
use horch_core::runtime::RuntimeContext;
use horch_core::workspace::{arrange, PaneHost};

use crate::output;

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
    let herdr = match workspace.filter(|s| !s.is_empty()) {
        Some(ws) => PaneHost::for_workspace(ctx, ws),
        None => PaneHost::of(ctx),
    };
    if !herdr.caps().geometry {
        output::print("the Herdr app arranges its own panes; nothing to tile\n");
        return Ok(());
    }
    let workspace_id = arrange::workspace_of(ctx, &herdr, pane, workspace)?;
    let herdr = PaneHost::for_workspace(ctx, &workspace_id);
    let out = arrange::run(ctx, &herdr, &workspace_id, None, plan_only)?;
    output::print(&out);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_telemetry_label_matches_the_one_tiling_skips() {
        assert_eq!(
            arrange::TELEMETRY_WORKSPACE_LABEL,
            crate::cmd::telemetry::WORKSPACE_LABEL
        );
    }
}
