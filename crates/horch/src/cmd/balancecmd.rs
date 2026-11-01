//! `horch balance` - make every worker column the same width.
//!
//! Equal columns are not decoration. `horch layout` decides whether the grid is
//! ragged by comparing the two rows' exact integer pane edges, so once a divider
//! drifts it reports phantom columns and advises a split that makes the grid
//! worse. Keeping the columns even keeps both rows on the same integers.
//!
//! The planning is pure and lives in [`horch_core::workspace::balance`]; the herdr calls
//! live in [`horch_core::workspace::arrange`]. This is the command.

use anyhow::Result;
use horch_core::runtime::RuntimeContext;
use horch_core::workspace::arrange;
use horch_core::workspace::balance::{self, ResizeOp};
use horch_core::workspace::herdr::Herdr;
use horch_core::workspace::model::Layout;

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
    output::print(&balance_text(
        ctx,
        &herdr,
        layout,
        orch.as_deref(),
        dry_run,
    )?);
    Ok(())
}

const NOT_SETTLED: &str = "not a settled 2-row grid (a column is half built or half drained); \
                           leaving the columns alone\n";
const ALREADY_EVEN: &str = "worker columns are already even; nothing to do\n";

/// What the command says. Distinguish "level already" from "not a shape worth
/// touching": a column that is half built or half drained leaves the rows with
/// different column counts, and evening one row alone would move it off the
/// other row's integers.
fn balance_text(
    ctx: &RuntimeContext,
    herdr: &Herdr,
    layout: Layout,
    orch: Option<&str>,
    dry_run: bool,
) -> Result<String> {
    if !balance::balanceable(&layout, orch) {
        Ok(NOT_SETTLED.to_string())
    } else if dry_run {
        Ok(dry_run_text(&balance::plan(&layout, orch)))
    } else {
        arrange::equalize(ctx, herdr, layout).map(applied_text)
    }
}

fn dry_run_text(ops: &[ResizeOp]) -> String {
    if ops.is_empty() {
        return ALREADY_EVEN.to_string();
    }
    let mut out = format!(
        "{} resize(s) would even out the worker columns:\n",
        ops.len()
    );
    for op in ops {
        out.push_str(&format!(
            "  {} {} {:.4}   (divider {} -> {})\n",
            op.pane,
            op.direction.as_str(),
            op.amount,
            op.from_x,
            op.to_x
        ));
    }
    out
}

fn applied_text(applied: usize) -> String {
    match applied {
        0 => ALREADY_EVEN.to_string(),
        1 => "evened out the worker columns (1 resize)\n".to_string(),
        n => format!("evened out the worker columns ({n} resizes)\n"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use horch_core::workspace::balance::ResizeDir;

    fn op(pane: &str, direction: ResizeDir, amount: f64, from_x: i64, to_x: i64) -> ResizeOp {
        ResizeOp {
            pane: pane.into(),
            direction,
            amount,
            from_x,
            to_x,
        }
    }

    #[test]
    fn a_dry_run_lists_each_resize_with_its_divider_move() {
        let ops = [
            op("p1", ResizeDir::Right, 0.25, 100, 150),
            op("p4", ResizeDir::Left, 1.0 / 3.0, 200, 150),
        ];
        assert_eq!(
            dry_run_text(&ops),
            "2 resize(s) would even out the worker columns:\n\
             \x20 p1 right 0.2500   (divider 100 -> 150)\n\
             \x20 p4 left 0.3333   (divider 200 -> 150)\n"
        );
    }

    #[test]
    fn a_dry_run_with_nothing_to_do_says_so() {
        assert_eq!(
            dry_run_text(&[]),
            "worker columns are already even; nothing to do\n"
        );
    }

    #[test]
    fn the_applied_text_counts_the_resizes() {
        assert_eq!(
            applied_text(0),
            "worker columns are already even; nothing to do\n"
        );
        assert_eq!(
            applied_text(1),
            "evened out the worker columns (1 resize)\n"
        );
        assert_eq!(
            applied_text(5),
            "evened out the worker columns (5 resizes)\n"
        );
    }
}
