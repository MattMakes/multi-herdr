//! Equalise the worker grid's column widths.
//!
//! # Why this exists
//!
//! [`crate::layout::analyze`] decides whether the grid is ragged by unioning the
//! *exact integer* edges of every half-height pane and asking whether any pane
//! strictly contains an edge the other row produced. It never reads the split
//! tree. So identical column boundaries in both rows are not a nicety - they are
//! the precondition the analysis is built on.
//!
//! Fresh splits satisfy that precondition but produce a halving cascade: three
//! columns come out 1/2, 1/4, 1/4. Once any single divider drifts, the two rows
//! stop agreeing on integers, `analyze` invents phantom columns, and it reports
//! `ragged` for a grid whose rows both hold N panes. Acting on that advice adds a
//! third pane to a column, which drives the grid to `offgrid`, where the advisor
//! has no suggestion at all. Equalising is what keeps both rows on the same
//! integers, so it repairs the advisor rather than threatening it.
//!
//! # Model
//!
//! A row is a right-leaning binary chain, not a flat N-way container: divider `k`
//! is owned by a split spanning `[pos[k-1], x1]`. Moving it rescales everything
//! to its right proportionally. Both facts are reproduced by [`plan`], so a
//! planned op sequence matches what herdr actually does.
//!
//! # Convergence under interleaving
//!
//! The target is absolute, not relative: column `k` of an `n`-column row belongs
//! at `x0 + round(k * w / n)`, derived only from the row's current extent and
//! pane count. Two passes running concurrently therefore aim at the *same*
//! schedule, so a nudge from one pass is never undone by the other - it is at
//! worst redundant. That makes [`plan`] idempotent (see `plan_is_idempotent`) and
//! interleaved passes converge on the next pass instead of oscillating, which is
//! why the driver needs no locking. See `interleaved_passes_converge`.

use crate::herdr::Layout;

/// A column boundary within one cell of its target is left alone. Absorbs both
/// border rows and herdr's rounding of a split ratio to four decimal places.
const TOLERANCE: i64 = 1;

/// Which way a divider moves. `herdr pane resize` also accepts up and down; the
/// worker grid only ever moves vertical dividers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResizeDir {
    Left,
    Right,
}

impl ResizeDir {
    pub fn as_str(self) -> &'static str {
        match self {
            ResizeDir::Left => "left",
            ResizeDir::Right => "right",
        }
    }
}

/// One `herdr pane resize` call.
///
/// `amount` is a delta expressed as a fraction of the split container that owns
/// the divider, which is what herdr's `--amount` means.
#[derive(Debug, Clone, PartialEq)]
pub struct ResizeOp {
    pub pane: String,
    pub direction: ResizeDir,
    pub amount: f64,
    /// Where the divider sits now, and where this op aims it. Reported by
    /// `horch balance --dry-run`.
    pub from_x: i64,
    pub to_x: i64,
}

/// One row of the worker grid, left to right.
struct Row {
    /// `(pane_id, x, width)`.
    panes: Vec<(String, i64, i64)>,
}

/// Split the tab into its two worker rows, or `None` when this is not a settled
/// 2-row grid.
///
/// Rows are grouped by `y`, never by `x`, so this stays correct precisely when
/// the columns have drifted - which is the case that needs repairing.
fn worker_rows(layout: &Layout, orchestrator: Option<&str>) -> Option<Vec<Row>> {
    let height = layout.area.height;
    let is_full = |h: i64| h >= height - 1;

    // The orchestrator is whatever the mailbox registered, else the leftmost
    // full-height pane. Same rule as `layout::analyze`, so both agree on which
    // panes form the grid.
    let orch: Option<String> = match orchestrator.filter(|s| !s.is_empty()) {
        Some(id) => Some(id.to_string()),
        None => layout
            .panes
            .iter()
            .filter(|p| is_full(p.rect.height))
            .min_by_key(|p| p.rect.x)
            .map(|p| p.pane_id.clone()),
    };

    let workers: Vec<&crate::herdr::LayoutPane> = layout
        .panes
        .iter()
        .filter(|p| Some(&p.pane_id) != orch.as_ref())
        .collect();

    if workers.is_empty() {
        return None;
    }
    // A full-height worker owns an unpaired column; the grid is mid-build and the
    // advisor's `unpaired` handling should run before anything is equalised.
    if workers.iter().any(|p| is_full(p.rect.height)) {
        return None;
    }

    let mut ys: Vec<i64> = workers.iter().map(|p| p.rect.y).collect();
    ys.sort_unstable();
    ys.dedup();
    if ys.len() != 2 {
        return None;
    }

    let mut rows = Vec::with_capacity(2);
    for y in ys {
        let mut panes: Vec<(String, i64, i64)> = workers
            .iter()
            .filter(|p| p.rect.y == y)
            .map(|p| (p.pane_id.clone(), p.rect.x, p.rect.width))
            .collect();
        panes.sort_by_key(|(_, x, _)| *x);
        rows.push(Row { panes });
    }

    let n = rows[0].panes.len();
    // Unequal row lengths mean the grid is genuinely ragged - a column is half
    // built or half drained. Equalising one row alone would move it off the other
    // row's integers and manufacture exactly the phantom raggedness this module
    // exists to prevent. Leave it for the advisor's ragged handling.
    if n < 2 || rows[1].panes.len() != n {
        return None;
    }

    let extent = |r: &Row| {
        let first = &r.panes[0];
        let last = &r.panes[n - 1];
        (first.1, last.1 + last.2)
    };
    if extent(&rows[0]) != extent(&rows[1]) {
        return None;
    }

    Some(rows)
}

/// Whether this tab is a settled 2-row worker grid, and so something [`plan`] is
/// willing to touch at all.
///
/// False while a column is half built or half drained. An empty plan means one of
/// two quite different things, and callers reporting to a human want to tell them
/// apart.
pub fn balanceable(layout: &Layout, orchestrator: Option<&str>) -> bool {
    worker_rows(layout, orchestrator).is_some()
}

/// The full sequence of resizes that makes every worker column the same width.
///
/// Pure: it plans against the geometry it is given, simulating herdr's
/// proportional rescale so later ops in the sequence are planned against the
/// positions the earlier ones will produce. Empty when the grid is already even,
/// or when it is not a settled 2-row grid.
pub fn plan(layout: &Layout, orchestrator: Option<&str>) -> Vec<ResizeOp> {
    let Some(rows) = worker_rows(layout, orchestrator) else {
        return Vec::new();
    };

    let mut ops = Vec::new();
    for row in &rows {
        let n = row.panes.len();
        let x0 = row.panes[0].1;
        let x1 = row.panes[n - 1].1 + row.panes[n - 1].2;
        let w = x1 - x0;
        if w <= 0 {
            continue;
        }

        // `pos[k]` is the left edge of column k, so `pos[k]` for k >= 1 is
        // divider k. Updated as we go to mirror what herdr will have done.
        let mut pos: Vec<i64> = row.panes.iter().map(|(_, x, _)| *x).collect();

        for k in 1..n {
            let target = x0 + ((k as f64) * (w as f64) / (n as f64)).round() as i64;
            let current = pos[k];
            let delta = target - current;
            if delta.abs() <= TOLERANCE {
                continue;
            }

            // Right-leaning chain: divider k is owned by a split spanning
            // [pos[k-1], x1], and `--amount` is a fraction of that span.
            let container = x1 - pos[k - 1];
            if container <= 0 {
                continue;
            }

            let (pane, direction) = if delta > 0 {
                (row.panes[k - 1].0.clone(), ResizeDir::Right)
            } else {
                (row.panes[k].0.clone(), ResizeDir::Left)
            };
            ops.push(ResizeOp {
                pane,
                direction,
                amount: delta.abs() as f64 / container as f64,
                from_x: current,
                to_x: target,
            });

            // Moving divider k rescales the subtree to its right proportionally.
            let before = x1 - current;
            let after = x1 - target;
            for p in pos.iter_mut().skip(k + 1) {
                *p = target + ((*p - current) as f64 * after as f64 / before as f64).round() as i64;
            }
            pos[k] = target;
        }
    }
    ops
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::herdr::{LayoutPane, Rect};

    /// Build a tab: a full-height orchestrator on the left, then two worker rows
    /// described by their column boundaries.
    fn grid(area_w: i64, orch_w: i64, top: &[i64], bottom: &[i64]) -> Layout {
        let h = 56;
        let mut panes = vec![LayoutPane {
            pane_id: "orch".into(),
            rect: Rect {
                x: 0,
                y: 1,
                width: orch_w,
                height: h,
            },
        }];
        for (row, (edges, y)) in [(top, 1), (bottom, 29)].iter().enumerate() {
            for (i, pair) in edges.windows(2).enumerate() {
                panes.push(LayoutPane {
                    pane_id: format!("r{row}c{i}"),
                    rect: Rect {
                        x: pair[0],
                        y: *y,
                        width: pair[1] - pair[0],
                        height: 28,
                    },
                });
            }
        }
        Layout {
            workspace_id: "w1".into(),
            tab_id: "w1:t1".into(),
            area: Rect {
                x: 0,
                y: 1,
                width: area_w,
                height: h,
            },
            panes,
            focused_pane_id: None,
            zoomed: false,
        }
    }

    /// Apply a planned op the way herdr does, so tests can check convergence
    /// rather than just the shape of the first op.
    ///
    /// Models the right-leaning chain: the divider sits at the far edge of a leaf
    /// column whose container runs from the previous divider to the row's right
    /// edge, and moving it rescales the subtree to its right proportionally.
    fn apply(layout: &Layout, op: &ResizeOp) -> Layout {
        let mut out = layout.clone();
        let (row_y, divider) = {
            let p = out.panes.iter().find(|p| p.pane_id == op.pane).unwrap();
            let d = match op.direction {
                ResizeDir::Left => p.rect.x,
                ResizeDir::Right => p.rect.x + p.rect.width,
            };
            (p.rect.y, d)
        };
        let row: Vec<&LayoutPane> = out.panes.iter().filter(|p| p.rect.y == row_y).collect();
        let x1 = row.iter().map(|p| p.rect.x + p.rect.width).max().unwrap();
        // The container's left edge is the divider before this one.
        let left = row
            .iter()
            .map(|p| p.rect.x)
            .filter(|x| *x < divider)
            .max()
            .unwrap();
        let shift = (op.amount * (x1 - left) as f64).round() as i64;
        let target = match op.direction {
            ResizeDir::Left => divider - shift,
            ResizeDir::Right => divider + shift,
        };

        let rescale = |v: i64| {
            if v < divider {
                v
            } else if v == divider {
                target
            } else {
                target
                    + ((v - divider) as f64 * (x1 - target) as f64 / (x1 - divider) as f64).round()
                        as i64
            }
        };
        for p in out.panes.iter_mut().filter(|p| p.rect.y == row_y) {
            let (l, r) = (rescale(p.rect.x), rescale(p.rect.x + p.rect.width));
            p.rect.x = l;
            p.rect.width = r - l;
        }
        out
    }

    /// The two properties that matter: both rows on identical integers (what
    /// `layout::analyze` needs), and every column within a cell of even.
    fn assert_balanced(layout: &Layout) {
        let top = edges(layout, 1);
        assert_eq!(
            top,
            edges(layout, 29),
            "both rows must land on identical integers"
        );
        let n = (top.len() - 1) as i64;
        let want = (top[top.len() - 1] - top[0]) / n;
        for pair in top.windows(2) {
            let got = pair[1] - pair[0];
            assert!(
                (got - want).abs() <= TOLERANCE,
                "column width {got} is not within {TOLERANCE} of {want}"
            );
        }
    }

    fn edges(layout: &Layout, y: i64) -> Vec<i64> {
        let mut v: Vec<i64> = layout
            .panes
            .iter()
            .filter(|p| p.rect.y == y && p.rect.height < 50)
            .flat_map(|p| [p.rect.x, p.rect.x + p.rect.width])
            .collect();
        v.sort_unstable();
        v.dedup();
        v
    }

    fn run(mut layout: Layout) -> Layout {
        for _ in 0..16 {
            let ops = plan(&layout, Some("orch"));
            let Some(op) = ops.first() else { break };
            layout = apply(&layout, op);
        }
        layout
    }

    /// The halving cascade a fresh 2x3 grid comes out of `pane split` with.
    fn fresh_2x3() -> Layout {
        grid(306, 153, &[179, 256, 294, 332], &[179, 256, 294, 332])
    }

    #[test]
    fn fresh_three_column_grid_needs_one_op_per_row() {
        let ops = plan(&fresh_2x3(), Some("orch"));
        assert_eq!(ops.len(), 2, "1/2,1/4,1/4 -> thirds is one divider per row");
        // Matches the move verified against herdr 0.7.4: divider 1 from 256 to
        // 230, addressed as the second column's left edge.
        assert_eq!(ops[0].direction, ResizeDir::Left);
        assert_eq!(ops[0].pane, "r0c1");
        assert_eq!((ops[0].from_x, ops[0].to_x), (256, 230));
        // 26 cells of a 153-cell container.
        assert!((ops[0].amount - 26.0 / 153.0).abs() < 1e-9);
    }

    #[test]
    fn balancing_lands_both_rows_on_identical_integers() {
        let out = run(fresh_2x3());
        assert_eq!(edges(&out, 1), vec![179, 230, 281, 332]);
        assert_balanced(&out);
    }

    #[test]
    fn repairs_a_drifted_row() {
        // One row equalised, the other still a halving cascade: the exact state
        // that makes `analyze` report 5 phantom columns and `ragged-bottom`.
        let drifted = grid(306, 153, &[179, 230, 281, 332], &[179, 256, 294, 332]);
        assert_balanced(&run(drifted));
    }

    #[test]
    fn four_columns_converge() {
        // 1/2, 1/4, 1/8, 1/8 - the cascade four fresh splits produce.
        let fresh = grid(
            400,
            200,
            &[200, 300, 350, 375, 400],
            &[200, 300, 350, 375, 400],
        );
        assert_balanced(&run(fresh));
    }

    #[test]
    fn plan_is_idempotent() {
        let out = run(fresh_2x3());
        assert!(
            plan(&out, Some("orch")).is_empty(),
            "an even grid plans no work, so re-running is free"
        );
    }

    /// Two workers finishing at once means two balance passes can interleave.
    /// Because the target schedule is absolute, a nudge from one pass is at worst
    /// redundant to the other, so the grid still converges - no locking needed.
    #[test]
    fn interleaved_passes_converge() {
        let start = fresh_2x3();
        // Pass A applies its first op; pass B planned against the ORIGINAL
        // geometry and applies a now-stale op on top.
        let stale = plan(&start, Some("orch")).remove(0);
        let after_a = apply(&start, &stale);
        let interleaved = apply(&after_a, &stale);
        // The stale op overshot, but a fresh pass still lands the grid evenly.
        let out = run(interleaved);
        assert_balanced(&out);
        assert!(plan(&out, Some("orch")).is_empty());
    }

    #[test]
    fn already_even_grid_plans_nothing() {
        let even = grid(306, 153, &[179, 230, 281, 332], &[179, 230, 281, 332]);
        assert!(plan(&even, Some("orch")).is_empty());
    }

    #[test]
    fn ragged_grid_is_left_alone() {
        // A column half drained: bottom has 2 panes where top has 3. Equalising
        // the bottom row alone would move it off the top row's integers.
        let ragged = grid(306, 153, &[179, 256, 294, 332], &[179, 256, 332]);
        assert!(plan(&ragged, Some("orch")).is_empty());
    }

    #[test]
    fn unpaired_full_height_worker_is_left_alone() {
        let mut l = fresh_2x3();
        l.panes.push(LayoutPane {
            pane_id: "solo".into(),
            rect: Rect {
                x: 332,
                y: 1,
                width: 40,
                height: 56,
            },
        });
        assert!(plan(&l, Some("orch")).is_empty());
    }

    #[test]
    fn single_column_grid_plans_nothing() {
        let one = grid(306, 153, &[179, 332], &[179, 332]);
        assert!(plan(&one, Some("orch")).is_empty());
    }

    #[test]
    fn orchestrator_column_is_never_moved() {
        let ops = plan(&fresh_2x3(), Some("orch"));
        assert!(
            ops.iter().all(|o| o.pane != "orch"),
            "the orchestrator/worker boundary is not ours to move"
        );
        assert!(ops.iter().all(|o| o.from_x > 179 && o.to_x > 179));
    }
}
