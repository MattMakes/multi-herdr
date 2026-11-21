use crate::workspace::model::{Layout, LayoutPane};

const TOLERANCE: i64 = 1;

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

#[derive(Debug, Clone, PartialEq)]
pub struct ResizeOp {
    pub pane: String,
    pub direction: ResizeDir,
    pub amount: f64,
    pub from_x: i64,
    pub to_x: i64,
}

struct Row {
    panes: Vec<(String, i64, i64)>,
}

impl Row {
    fn extent(&self) -> (i64, i64) {
        let first = &self.panes[0];
        let last = &self.panes[self.panes.len() - 1];
        (first.1, last.1 + last.2)
    }
}

fn worker_rows(layout: &Layout, orchestrator: Option<&str>) -> Option<Vec<Row>> {
    let rows = two_rows(&grid_workers(layout, orchestrator)?)?;
    rows_match(&rows).then_some(rows)
}

fn grid_orchestrator(layout: &Layout, orchestrator: Option<&str>) -> Option<String> {
    match orchestrator.filter(|s| !s.is_empty()) {
        Some(id) => Some(id.to_string()),
        None => layout
            .panes
            .iter()
            .filter(|p| is_full(layout, p.rect.height))
            .min_by_key(|p| p.rect.x)
            .map(|p| p.pane_id.clone()),
    }
}

fn is_full(layout: &Layout, height: i64) -> bool {
    height >= layout.area.height - 1
}

fn grid_workers<'l>(layout: &'l Layout, orchestrator: Option<&str>) -> Option<Vec<&'l LayoutPane>> {
    let orch = grid_orchestrator(layout, orchestrator);
    let workers: Vec<&LayoutPane> = layout
        .panes
        .iter()
        .filter(|p| Some(&p.pane_id) != orch.as_ref())
        .collect();
    Some(workers).filter(|w| !w.is_empty() && !w.iter().any(|p| is_full(layout, p.rect.height)))
}

fn two_rows(workers: &[&LayoutPane]) -> Option<Vec<Row>> {
    let mut ys: Vec<i64> = workers.iter().map(|p| p.rect.y).collect();
    ys.sort_unstable();
    ys.dedup();
    (ys.len() == 2).then(|| {
        ys.into_iter()
            .map(|y| {
                let mut panes: Vec<(String, i64, i64)> = workers
                    .iter()
                    .filter(|p| p.rect.y == y)
                    .map(|p| (p.pane_id.clone(), p.rect.x, p.rect.width))
                    .collect();
                panes.sort_by_key(|(_, x, _)| *x);
                Row { panes }
            })
            .collect()
    })
}

fn rows_match(rows: &[Row]) -> bool {
    let n = rows[0].panes.len();
    n >= 2 && rows[1].panes.len() == n && rows[0].extent() == rows[1].extent()
}

pub fn balanceable(layout: &Layout, orchestrator: Option<&str>) -> bool {
    worker_rows(layout, orchestrator).is_some()
}

pub fn plan(layout: &Layout, orchestrator: Option<&str>) -> Vec<ResizeOp> {
    worker_rows(layout, orchestrator)
        .map(|rows| rows.iter().flat_map(row_ops).collect())
        .unwrap_or_default()
}

fn row_ops(row: &Row) -> Vec<ResizeOp> {
    let n = row.panes.len();
    let (x0, x1) = row.extent();
    let mut ops = Vec::new();
    if x1 - x0 <= 0 {
        return ops;
    }

    let mut pos: Vec<i64> = row.panes.iter().map(|(_, x, _)| *x).collect();

    for k in 1..n {
        if let Some(op) = divider_op(row, &pos, k, (x0, x1)) {
            let before = x1 - op.from_x;
            let after = x1 - op.to_x;
            for p in pos.iter_mut().skip(k + 1) {
                *p = op.to_x
                    + ((*p - op.from_x) as f64 * after as f64 / before as f64).round() as i64;
            }
            pos[k] = op.to_x;
            ops.push(op);
        }
    }
    ops
}

fn divider_op(row: &Row, pos: &[i64], k: usize, (x0, x1): (i64, i64)) -> Option<ResizeOp> {
    let n = row.panes.len();
    let w = x1 - x0;
    let target = x0 + ((k as f64) * (w as f64) / (n as f64)).round() as i64;
    let current = pos[k];
    let delta = target - current;
    let container = x1 - pos[k - 1];
    if delta.abs() <= TOLERANCE || container <= 0 {
        return None;
    }
    let (pane, direction) = if delta > 0 {
        (row.panes[k - 1].0.clone(), ResizeDir::Right)
    } else {
        (row.panes[k].0.clone(), ResizeDir::Left)
    };
    Some(ResizeOp {
        pane,
        direction,
        amount: delta.abs() as f64 / container as f64,
        from_x: current,
        to_x: target,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::model::{LayoutPane, Rect};

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

    fn fresh_2x3() -> Layout {
        grid(306, 153, &[179, 256, 294, 332], &[179, 256, 294, 332])
    }

    #[test]
    fn fresh_three_column_grid_needs_one_op_per_row() {
        let ops = plan(&fresh_2x3(), Some("orch"));
        assert_eq!(ops.len(), 2, "1/2,1/4,1/4 -> thirds is one divider per row");
        assert_eq!(ops[0].direction, ResizeDir::Left);
        assert_eq!(ops[0].pane, "r0c1");
        assert_eq!((ops[0].from_x, ops[0].to_x), (256, 230));
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
        let drifted = grid(306, 153, &[179, 230, 281, 332], &[179, 256, 294, 332]);
        assert_balanced(&run(drifted));
    }

    #[test]
    fn four_columns_converge() {
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

    #[test]
    fn interleaved_passes_converge() {
        let start = fresh_2x3();
        let stale = plan(&start, Some("orch")).remove(0);
        let after_a = apply(&start, &stale);
        let interleaved = apply(&after_a, &stale);
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

    #[test]
    fn the_orchestrator_is_guessed_when_none_is_named() {
        let named = plan(&fresh_2x3(), Some("orch"));
        assert!(!named.is_empty());
        assert_eq!(plan(&fresh_2x3(), None), named);
        assert_eq!(plan(&fresh_2x3(), Some("")), named);
    }

    #[test]
    fn only_a_settled_two_row_grid_is_balanceable() {
        assert!(balanceable(&fresh_2x3(), Some("orch")));
        let mut alone = fresh_2x3();
        alone.panes.truncate(1);
        assert!(!balanceable(&alone, Some("orch")));
        let mut three = fresh_2x3();
        three.panes.push(LayoutPane {
            pane_id: "third".into(),
            rect: Rect {
                x: 179,
                y: 15,
                width: 40,
                height: 10,
            },
        });
        assert!(!balanceable(&three, Some("orch")));
        let uneven = grid(306, 153, &[179, 256, 332], &[179, 256, 300]);
        assert!(!balanceable(&uneven, Some("orch")));
        assert!(plan(&uneven, Some("orch")).is_empty());
    }

    #[test]
    fn a_divider_right_of_its_target_moves_left_and_one_left_of_it_moves_right() {
        let right_of = grid(306, 153, &[179, 290, 332], &[179, 290, 332]);
        let ops = plan(&right_of, Some("orch"));
        assert_eq!(ops.len(), 2);
        assert!(ops.iter().all(|o| o.direction == ResizeDir::Left));
        assert!(ops.iter().all(|o| o.from_x == 290 && o.to_x == 256));
        let left_of = grid(306, 153, &[179, 200, 332], &[179, 200, 332]);
        let ops = plan(&left_of, Some("orch"));
        assert_eq!(ops.len(), 2);
        assert!(ops.iter().all(|o| o.direction == ResizeDir::Right));
        assert!(ops.iter().all(|o| o.from_x == 200 && o.to_x == 256));
    }

    #[test]
    fn a_divider_within_the_tolerance_is_left_alone() {
        let near = grid(306, 153, &[179, 257, 332], &[179, 255, 332]);
        assert!(plan(&near, Some("orch")).is_empty());
    }
}
