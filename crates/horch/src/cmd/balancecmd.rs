//! `horch balance` - make every worker column the same width.
//!
//! Equal columns are not decoration. `horch layout` decides whether the grid is
//! ragged by comparing the two rows' exact integer pane edges, so once a divider
//! drifts it reports phantom columns and advises a split that makes the grid
//! worse. Keeping the columns even keeps both rows on the same integers.
//!
//! The planning is pure and lives in [`horch_core::balance`]; this is the part
//! that talks to herdr.

use anyhow::Result;
use horch_core::balance;
use horch_core::herdr::{Herdr, Layout};
use horch_core::mailbox::Mailbox;

use crate::output;

/// Two rows of at most a dozen columns need one op per divider; the rest is slack
/// for a divider that lands a cell off and wants a second nudge.
const MAX_OPS: usize = 24;

/// Whichever pane the mailbox registered as the orchestrator. `None` lets the
/// planner fall back to the leftmost full-height pane, matching `horch layout`.
fn orchestrator_of(layout: &Layout) -> Option<String> {
    Mailbox::new(&layout.workspace_id)
        .panes_to_roles()
        .into_iter()
        .find(|(_, role)| role == "orchestrator")
        .map(|(pane_id, _)| pane_id)
}

/// Equalise the worker columns, returning how many resizes were sent.
///
/// Re-plans against the layout herdr returns after every resize rather than
/// predicting the next position, so accumulated rounding cannot walk the grid
/// off target. Two things stop the loop: herdr reporting `changed: false`, which
/// is how the minimum pane width announces itself, and a divider that is asked
/// to move from the same place twice, which means the previous op did not land.
pub fn equalize(herdr: &Herdr, mut layout: Layout) -> Result<usize> {
    let orch = orchestrator_of(&layout);
    let mut applied = 0;
    let mut last: Option<(String, i64)> = None;

    for _ in 0..MAX_OPS {
        let Some(op) = balance::plan(&layout, orch.as_deref()).into_iter().next() else {
            break;
        };
        if last.as_ref() == Some(&(op.pane.clone(), op.from_x)) {
            break;
        }
        last = Some((op.pane.clone(), op.from_x));

        let out = herdr.pane_resize(&op.pane, op.direction.as_str(), op.amount)?;
        applied += 1;
        if !out.changed {
            break;
        }
        layout = out.layout;
    }
    Ok(applied)
}

/// Best-effort equalise for callers whose real job is something else.
///
/// A grid that will not even out must never fail a spawn or a worker's shutdown,
/// so this reports the problem and returns.
pub fn equalize_quietly(herdr: &Herdr, layout: Layout) {
    if let Err(e) = equalize(herdr, layout) {
        eprintln!("horch: could not even out the worker columns ({e:#}); carrying on");
    }
}

pub fn balance(
    pane: Option<&str>,
    workspace: Option<&str>,
    dry_run: bool,
    settle_ms: u64,
) -> Result<()> {
    if settle_ms > 0 {
        std::thread::sleep(std::time::Duration::from_millis(settle_ms));
    }
    let herdr = Herdr::new();

    // Same resolution order as `horch layout`: an explicit pane, else any pane in
    // the named workspace, else this pane. HERDR_PANE_ID is an internal id, so it
    // round-trips through `pane get` for the public one.
    let target: Option<String> = match (pane, workspace) {
        (Some(p), _) => Some(p.to_string()),
        (None, Some(ws)) => herdr.pane_list(ws)?.first().map(|p| p.pane_id.clone()),
        (None, None) => match std::env::var("HERDR_PANE_ID") {
            Ok(internal) if !internal.is_empty() => Some(herdr.pane_get(&internal)?.pane_id),
            _ => None,
        },
    };

    let layout = herdr.pane_layout(target.as_deref())?;
    let orch = orchestrator_of(&layout);

    // Distinguish "level already" from "not a shape worth touching": a column that
    // is half built or half drained leaves the rows with different column counts,
    // and evening one row alone would move it off the other row's integers.
    if !balance::balanceable(&layout, orch.as_deref()) {
        output::print(
            "not a settled 2-row grid (a column is half built or half drained); \
             leaving the columns alone\n",
        );
        return Ok(());
    }

    if dry_run {
        let ops = balance::plan(&layout, orch.as_deref());
        if ops.is_empty() {
            output::print("worker columns are already even; nothing to do\n");
            return Ok(());
        }
        let mut out = format!(
            "{} resize(s) would even out the worker columns:\n",
            ops.len()
        );
        for op in &ops {
            out.push_str(&format!(
                "  {} {} {:.4}   (divider {} -> {})\n",
                op.pane,
                op.direction.as_str(),
                op.amount,
                op.from_x,
                op.to_x
            ));
        }
        output::print(&out);
        return Ok(());
    }

    let applied = equalize(&herdr, layout)?;
    output::print(&match applied {
        0 => "worker columns are already even; nothing to do\n".to_string(),
        1 => "evened out the worker columns (1 resize)\n".to_string(),
        n => format!("evened out the worker columns ({n} resizes)\n"),
    });
    Ok(())
}
