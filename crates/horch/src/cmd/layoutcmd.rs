//! `horch layout` - the port of `bin/horch-layout`.
//!
//! Reports the fleet's worker grid, one block per tab. A fleet outgrows one tab
//! at the fifth worker, so a report about a single tab would describe part of the
//! grid and call it the grid. The report is built in
//! [`horch_core::workspace::arrange::report`].

use anyhow::Result;
use horch_core::herdr::Herdr;
use horch_core::runtime::RuntimeContext;
use horch_core::workspace::arrange;

use crate::output;

pub fn layout(ctx: &RuntimeContext, pane: Option<&str>, workspace: Option<&str>) -> Result<()> {
    let herdr = Herdr::with_bin(&ctx.bins.harness.herdr);
    let workspace_id = arrange::workspace_of(ctx, &herdr, pane, workspace)?;
    output::print(&arrange::report(ctx, &herdr, &workspace_id)?);
    Ok(())
}
