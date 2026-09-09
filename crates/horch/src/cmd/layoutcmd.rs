//! `horch layout` - the port of `bin/horch-layout`.
//!
//! Reports the fleet's worker grid and the next split that keeps it 2 rows tall by
//! N columns wide.

use std::collections::HashMap;

use anyhow::Result;
use horch_core::herdr::Herdr;
use horch_core::layout;
use horch_core::mailbox::Mailbox;

use crate::output;

pub fn layout(pane: Option<&str>, workspace: Option<&str>) -> Result<()> {
    let herdr = Herdr::new();

    // Resolve a pane to ask about. HERDR_PANE_ID is an internal id (p_2), so
    // round-trip it through `pane get` for the public one.
    let target: Option<String> = match (pane, workspace) {
        (Some(p), _) => Some(p.to_string()),
        (None, Some(ws)) => herdr.pane_list(ws)?.first().map(|p| p.pane_id.clone()),
        (None, None) => match std::env::var("HERDR_PANE_ID") {
            Ok(internal) if !internal.is_empty() => Some(herdr.pane_get(&internal)?.pane_id),
            _ => None,
        },
    };

    let layout_data = herdr.pane_layout(target.as_deref())?;

    // The fleet records pane ids by role; use them for labels when present.
    let mailbox = Mailbox::new(&layout_data.workspace_id);
    let roles: HashMap<String, String> = mailbox.panes_to_roles().into_iter().collect();
    let orchestrator = roles
        .iter()
        .find(|(_, role)| role.as_str() == "orchestrator")
        .map(|(pane_id, _)| pane_id.clone());

    let analysis = layout::analyze(&layout_data, orchestrator.as_deref());
    output::print(&layout::render(&analysis, &roles));
    Ok(())
}
