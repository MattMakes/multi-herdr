use anyhow::Result;
use horch_core::runtime::RuntimeContext;
use horch_core::workspace::{arrange, PaneHost};

use crate::output;

pub fn layout(ctx: &RuntimeContext, pane: Option<&str>, workspace: Option<&str>) -> Result<()> {
    let herdr = match workspace.filter(|s| !s.is_empty()) {
        Some(ws) => PaneHost::for_workspace(ctx, ws),
        None => PaneHost::of(ctx),
    };
    let workspace_id = arrange::workspace_of(ctx, &herdr, pane, workspace)?;
    let herdr = PaneHost::for_workspace(ctx, &workspace_id);
    output::print(&arrange::report(ctx, &herdr, &workspace_id)?);
    Ok(())
}
