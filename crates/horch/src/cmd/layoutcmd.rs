//! `horch layout` - the port of `bin/horch-layout`.
//!
//! Reports the fleet's worker grid, one block per tab. A fleet outgrows one tab
//! at the fifth worker, so a report about a single tab would describe part of the
//! grid and call it the grid.

use std::collections::HashMap;

use anyhow::Result;
use horch_core::herdr::{Herdr, Layout};
use horch_core::layout::{self, Orchestrator};
use horch_core::mailbox::Mailbox;
use horch_core::tile;

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

    let mut layouts = Vec::new();
    for tab in &tabs {
        if let Some(probe) = panes
            .iter()
            .find(|p| p.tab_id.as_deref() == Some(tab.tab_id.as_str()))
        {
            layouts.push(herdr.pane_layout(Some(&probe.pane_id))?);
        }
    }

    // One orchestrator for the whole workspace: whatever the mailbox registered,
    // else the leftmost full-height pane of the first tab, which is the guess
    // `horch layout` has always made. A workspace with neither is reported with
    // every pane as a worker rather than refused.
    let orchestrator = roles
        .iter()
        .find(|(_, role)| role.as_str() == "orchestrator")
        .map(|(pane_id, _)| pane_id.clone())
        .filter(|id| layouts.iter().any(|l| holds(l, id)))
        .or_else(|| {
            layouts
                .first()
                .and_then(|l| layout::analyze_with(l, Orchestrator::Infer).orchestrator)
        });

    let workers = panes.len() - usize::from(orchestrator.is_some());
    let mut analyses = Vec::new();
    let mut notes = Vec::new();
    for (i, data) in layouts.iter().enumerate() {
        let on_this_tab = orchestrator.as_deref().filter(|id| holds(data, id));
        let who = match on_this_tab {
            Some(id) => Orchestrator::Pane(id),
            None => Orchestrator::Absent,
        };
        notes.push(verdict(
            data,
            on_this_tab,
            tile::workers_on_tab(i + 1, workers),
        ));
        analyses.push(layout::analyze_with(data, who));
    }
    Ok(layout::render_all(&analyses, &notes, &roles))
}

fn holds(layout: &Layout, pane_id: &str) -> bool {
    layout.panes.iter().any(|p| p.pane_id == pane_id)
}

/// One line saying whether this tab is the shape `horch tile` builds.
///
/// Worth stating plainly, because the per-tab state below it judges a tab as if it
/// were the whole grid: an overflow tab whose last bottom slot is free reads as
/// `ragged-bottom` when it is exactly what the tiler meant to build.
fn verdict(layout: &Layout, orchestrator: Option<&str>, expected: usize) -> String {
    let shape = tile::TabShape::from_layout(layout);
    let panes = shape.panes.len() - usize::from(orchestrator.is_some());
    // Tab 1 shares its width with the orchestrator, so it holds 4 workers where an
    // overflow tab holds 6.
    let slots = tile::capacity(if orchestrator.is_some() { 1 } else { 2 });
    if tile::tab_is_canonical(&shape, orchestrator, expected) {
        format!("matches the fleet grid, {panes}/{slots} slots filled")
    } else {
        format!("NOT the fleet grid; run `horch tile` ({panes} worker pane(s))")
    }
}

pub fn layout(pane: Option<&str>, workspace: Option<&str>) -> Result<()> {
    let herdr = Herdr::new();
    let workspace_id = crate::cmd::tilecmd::workspace_of(&herdr, pane, workspace)?;
    output::print(&report(&herdr, &workspace_id)?);
    Ok(())
}
