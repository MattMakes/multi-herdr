//! `horch layout` - the port of `bin/horch-layout`.
//!
//! Reports the fleet's worker grid, one block per tab. A fleet outgrows one tab
//! at the fifth worker, so a report about a single tab would describe part of the
//! grid and call it the grid.

use std::collections::HashMap;

use anyhow::Result;
use horch_core::herdr::Herdr;
use horch_core::layout::{self, Orchestrator};
use horch_core::mailbox::Mailbox;

use crate::output;

/// The whole workspace's grid, one block per tab in tab-strip order.
///
/// The orchestrator is only sought on the tab that actually holds it. On the
/// others every pane is a worker, and the positional guess - leftmost full-height
/// pane - would quietly drop a worker column from the report.
pub fn report(herdr: &Herdr, workspace_id: &str) -> Result<String> {
    let tabs = herdr.tab_list(workspace_id)?;
    let panes = herdr.pane_list(workspace_id)?;

    let roles: HashMap<String, String> = Mailbox::new(workspace_id)
        .panes_to_roles()
        .into_iter()
        .collect();
    let registered = roles
        .iter()
        .find(|(_, role)| role.as_str() == "orchestrator")
        .map(|(pane_id, _)| pane_id.clone());

    let mut analyses = Vec::new();
    for (i, tab) in tabs.iter().enumerate() {
        let Some(probe) = panes
            .iter()
            .find(|p| p.tab_id.as_deref() == Some(tab.tab_id.as_str()))
        else {
            continue;
        };
        let layout_data = herdr.pane_layout(Some(&probe.pane_id))?;
        let holds_registered = registered
            .as_deref()
            .is_some_and(|orch| layout_data.panes.iter().any(|p| p.pane_id == orch));
        let who = match (&registered, holds_registered, i) {
            (Some(orch), true, _) => Orchestrator::Pane(orch),
            // Nothing registered: the first tab is the orchestrator's by the same
            // rule `horch layout` has always used, and no other tab has one.
            (None, _, 0) => Orchestrator::Infer,
            _ => Orchestrator::Absent,
        };
        analyses.push(layout::analyze_with(&layout_data, who));
    }
    Ok(layout::render_all(&analyses, &roles))
}

pub fn layout(pane: Option<&str>, workspace: Option<&str>) -> Result<()> {
    let herdr = Herdr::new();
    let workspace_id = crate::cmd::tilecmd::workspace_of(&herdr, pane, workspace)?;
    output::print(&report(&herdr, &workspace_id)?);
    Ok(())
}
