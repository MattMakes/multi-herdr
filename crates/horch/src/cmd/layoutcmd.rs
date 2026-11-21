//! `horch layout` - the port of `bin/horch-layout`.
//!
//! Reports the fleet's worker grid, one block per tab. A fleet outgrows one tab
//! at the fifth worker, so a report about a single tab would describe part of the
//! grid and call it the grid. The report is built in
//! [`horch_core::workspace::arrange::report`].

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
