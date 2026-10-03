//! `horch balance` - make every worker column the same width.
//!
//! Equal columns are not decoration. `horch layout` decides whether the grid is
//! ragged by comparing the two rows' exact integer pane edges, so once a divider
//! drifts it reports phantom columns and advises a split that makes the grid
//! worse. Keeping the columns even keeps both rows on the same integers.
//!
//! The planning is pure and lives in [`horch_core::balance`]; the herdr calls
//! live in [`horch_core::workspace::arrange`]. This is the command.

use anyhow::Result;
use horch_core::balance;
use horch_core::herdr::Herdr;
use horch_core::runtime::RuntimeContext;
use horch_core::workspace::arrange;

/// `horch spawn` still reaches this through this module.
pub use horch_core::workspace::arrange::equalize_quietly;

use crate::output;

pub fn balance(
    ctx: &RuntimeContext,
    pane: Option<&str>,
    workspace: Option<&str>,
    dry_run: bool,
    settle_ms: u64,
) -> Result<()> {
    if settle_ms > 0 {
        std::thread::sleep(std::time::Duration::from_millis(settle_ms));
    }
    let herdr = Herdr::with_bin(&ctx.bins.harness.herdr);

    let layout = arrange::balance_target(ctx, &herdr, pane, workspace)?;
    let orch = arrange::orchestrator_of(ctx, &layout);

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

    let applied = arrange::equalize(ctx, &herdr, layout)?;
    output::print(&match applied {
        0 => "worker columns are already even; nothing to do\n".to_string(),
        1 => "evened out the worker columns (1 resize)\n".to_string(),
        n => format!("evened out the worker columns ({n} resizes)\n"),
    });
    Ok(())
}
