//! Deterministic placement of every pane in a fleet workspace into one grid.
//!
//! # The shape
//!
//! ```text
//! tab 1                              tab 2..N (overflow, no orchestrator)
//! +-------+--------+--------+        +--------+--------+--------+
//! |       |   W    |   W    |        |   W    |   W    |   W    |
//! |   O   +--------+--------+        +--------+--------+--------+
//! |       |   W    |   W    |        |   W    |   W    |   W    |
//! +-------+--------+--------+        +--------+--------+--------+
//! ```
//!
//! The orchestrator is the leftmost full-height pane of tab 1 and never moves.
//! Tab 1 holds 4 workers in 2 columns, each overflow tab 6 in 3 columns, filled
//! column 1 top, column 1 bottom, column 2 top, column 2 bottom, and so on. That
//! order IS [`Slot`]'s ordering, so placement, reuse and reporting all read the
//! same list.
//!
//! # Why every pane is parked first
//!
//! herdr refuses to move a pane into the tab it already occupies: the reply is
//! `changed: false, reason: "same_tab"` (`socket-api.md:231`). Repositioning a
//! pane inside its own tab therefore means moving it out and back, so the plan
//! has two phases - park every worker in a scratch tab, then place them all from
//! there - and needs no same-tab case at all. It also means the plan does not
//! care how scrambled the tab was to begin with.
//!
//! Parking uses the same 2-row grid as placement rather than one chain of right
//! splits, because herdr enforces no minimum pane size: nine chained right
//! splits measured 46, 23, 11, 5, 2, 1, 0, 0, 0 cells wide without one error
//! (`ai_docs/reports/horch-tile-herdr-surface.md`). A chain would hand a live
//! agent a zero-column terminal on the way past.
//!
//! # Why a new column splits the row below it too
//!
//! A right split subdivides its own pane's rect and nothing else. Splitting
//! column 1's half-height top pane to the right therefore opens the new column in
//! the TOP ROW ONLY, and splitting that new pane DOWN produces two
//! quarter-height panes - the shape `ai_docs/reports/layout-survey.md` section 6
//! documents as unrecoverable. So column `c >= 2` is opened twice, once against
//! column `c-1`'s top pane and once against its bottom pane, which is the order
//! `crates/horch/src/cmd/recipes.rs` has always used by hand.

use crate::balance::{ResizeDir, ResizeOp};
use crate::herdr::{Direction, Layout, Rect};

/// Label of the tab workers are parked in while the grid is rebuilt. It exists
/// for at most one run; a run that dies leaves it, and the next run gathers the
/// panes in it like any others and places them.
pub const SCRATCH_LABEL: &str = "horch-tile-scratch";

/// Tab 1 gives one of its columns to the orchestrator, so it holds 2 worker
/// columns where an overflow tab holds 3.
pub const TAB1_COLUMNS: usize = 2;
pub const OVERFLOW_COLUMNS: usize = 3;
/// Two rows everywhere. A 3-row layout is deliberately out of scope.
pub const ROWS: usize = 2;

/// A divider within one cell of its target is left alone: the same tolerance
/// [`crate::balance`] uses, absorbing border rows and herdr's rounding of a
/// split ratio to four decimal places.
pub const TOLERANCE: i64 = 1;

/// Worker columns on a 1-based tab index.
pub fn columns(tab_index: usize) -> usize {
    if tab_index <= 1 {
        TAB1_COLUMNS
    } else {
        OVERFLOW_COLUMNS
    }
}

/// How many workers a 1-based tab index holds when full.
pub fn capacity(tab_index: usize) -> usize {
    columns(tab_index) * ROWS
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Row {
    Top,
    Bottom,
}

impl Row {
    pub fn as_str(self) -> &'static str {
        match self {
            Row::Top => "top",
            Row::Bottom => "bottom",
        }
    }
}

/// Where one worker belongs. Ordering is lexicographic (tab, column, row), which
/// is the fill order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Slot {
    /// 1-based, in tab-strip order. Tab 1 is the orchestrator's tab.
    pub tab_index: usize,
    /// 1-based, excluding the orchestrator's column.
    pub column: usize,
    pub row: Row,
}

/// The first `n` slots in fill order.
pub fn slots_for(n: usize) -> Vec<Slot> {
    let mut slots = Vec::with_capacity(n);
    let mut tab_index = 1;
    while slots.len() < n {
        for column in 1..=columns(tab_index) {
            for row in [Row::Top, Row::Bottom] {
                if slots.len() == n {
                    return slots;
                }
                slots.push(Slot {
                    tab_index,
                    column,
                    row,
                });
            }
        }
        tab_index += 1;
    }
    slots
}

/// How many tabs `n` workers need. Always at least one: the orchestrator's.
pub fn tab_count(n: usize) -> usize {
    slots_for(n).last().map(|s| s.tab_index).unwrap_or(1)
}

/// How many workers land on a 1-based tab index, given `n` in the workspace.
pub fn workers_on_tab(tab_index: usize, n: usize) -> usize {
    slots_for(n)
        .iter()
        .filter(|s| s.tab_index == tab_index)
        .count()
}

/// Worker columns a tab ends up with: every column has a top, so it is the
/// number of workers rounded up.
fn columns_used(workers: usize) -> usize {
    workers.div_ceil(ROWS)
}

/// A tab the plan refers to. A tab it creates has no id until the run reaches
/// the op that creates it, so the plan names it by creation order and the driver
/// substitutes the id herdr reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TabRef {
    Existing(String),
    Created(usize),
}

/// One herdr call. Every pane id is known when the plan is made; only tab ids
/// can be pending.
#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    /// `herdr pane move <pane> --new-tab --label <label> --no-focus`, creating
    /// the tab this plan calls `Created(creates)`. The moved pane is the only
    /// pane in it, which is why this and not `tab create`: `tab create` would add
    /// a shell pane to dispose of.
    NewTab {
        pane: String,
        label: String,
        creates: usize,
    },
    /// `herdr pane move <pane> --tab <tab> --split <split> --target-pane <target>
    /// [--ratio <ratio>] --no-focus`. `ratio` is the share `target` keeps.
    Move {
        pane: String,
        tab: TabRef,
        split: Direction,
        target: String,
        ratio: Option<f64>,
    },
    /// `herdr tab focus <tab>`, to put the operator back where they were.
    FocusTab { tab: TabRef },
}

/// A worker pane as the live workspace reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Worker {
    pub pane: String,
    pub tab_id: String,
    pub x: i64,
    pub y: i64,
}

/// What the workspace looks like now, and who is new.
#[derive(Debug, Clone)]
pub struct Fleet {
    /// Tab ids in tab-strip order.
    pub tabs: Vec<String>,
    pub orchestrator: String,
    pub orchestrator_tab: String,
    pub workers: Vec<Worker>,
    /// A pane that has just appeared and must be placed LAST however its
    /// rectangle sorts.
    ///
    /// `horch spawn` splits the orchestrator's own pane by default, so the new
    /// worker lands full-height immediately right of the orchestrator - the
    /// smallest `x` of any worker. Sorted by geometry it would take column 1 top
    /// and rotate every other worker one slot along on every single spawn.
    pub newcomer: Option<String>,
}

/// The whole rearrangement, split into the two phases so a report can name them.
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    pub park: Vec<Op>,
    pub place: Vec<Op>,
    /// Worker pane and where it lands, in fill order.
    pub placement: Vec<(String, Slot)>,
    /// How many tabs the plan creates: the scratch tab plus one per overflow tab.
    pub created_tabs: usize,
}

impl Plan {
    pub fn ops(&self) -> impl Iterator<Item = &Op> {
        self.park.iter().chain(self.place.iter())
    }

    pub fn is_empty(&self) -> bool {
        self.park.is_empty() && self.place.is_empty()
    }
}

/// Workers in fill order: reading order of where they sit now, so a re-run is
/// stable and neighbours stay neighbours, with any newcomer appended.
///
/// Columns are clustered within [`TOLERANCE`] before being compared, so a
/// divider that drifted one cell cannot swap a column's two panes on the next
/// run.
pub fn order_workers(fleet: &Fleet) -> Vec<&Worker> {
    let tab_index = |tab_id: &str| {
        fleet
            .tabs
            .iter()
            .position(|t| t == tab_id)
            .unwrap_or(usize::MAX)
    };

    // Column keys per tab: the x edges that exist there, deduped by tolerance.
    let column_key = |w: &Worker| -> usize {
        let mut xs: Vec<i64> = fleet
            .workers
            .iter()
            .filter(|o| o.tab_id == w.tab_id)
            .map(|o| o.x)
            .collect();
        xs.sort_unstable();
        let mut clusters: Vec<i64> = Vec::new();
        for x in xs {
            match clusters.last() {
                Some(last) if x - last <= TOLERANCE => {}
                _ => clusters.push(x),
            }
        }
        clusters
            .iter()
            .rposition(|c| w.x - *c >= 0 && w.x - *c <= TOLERANCE)
            .unwrap_or(0)
    };

    let mut ordered: Vec<&Worker> = fleet
        .workers
        .iter()
        .filter(|w| Some(w.pane.as_str()) != fleet.newcomer.as_deref())
        .collect();
    ordered.sort_by_key(|w| (tab_index(&w.tab_id), column_key(w), w.y, w.pane.clone()));

    if let Some(newcomer) = fleet.newcomer.as_deref() {
        if let Some(w) = fleet.workers.iter().find(|w| w.pane == newcomer) {
            ordered.push(w);
        }
    }
    ordered
}

/// The ratio that leaves equal columns when column `c` of `total` is opened
/// against column `c-1`.
///
/// When column `c` is opened, column `c-1`'s pane still spans everything from
/// column `c-1` to column `total`, so it must keep one of those
/// `total - c + 2` shares.
fn column_ratio(c: usize, total: usize) -> f64 {
    1.0 / (total.saturating_sub(c) + 2) as f64
}

/// The share of tab 1 the orchestrator keeps: one column of the `n` worker
/// columns plus itself.
pub fn orchestrator_share(worker_columns: usize) -> f64 {
    1.0 / (worker_columns + 1) as f64
}

/// Emit the moves that build a 2-row grid of `panes` in `tab`.
///
/// `first` is how the first pane gets there - right of the orchestrator on tab 1,
/// or into a tab of its own everywhere else - and every later pane splits a pane
/// this function has already placed.
fn grid_ops(panes: &[&str], tab: TabRef, total_columns: usize, first: FirstPane) -> Vec<Op> {
    let mut ops = Vec::new();
    // Panes already placed, by column, so a later op can name its target.
    let mut tops: Vec<&str> = Vec::new();
    let mut bottoms: Vec<&str> = Vec::new();

    for (i, pane) in panes.iter().enumerate() {
        let column = i / ROWS + 1;
        let row = if i % ROWS == 0 { Row::Top } else { Row::Bottom };
        match (column, row) {
            (1, Row::Top) => match &first {
                FirstPane::RightOf {
                    target,
                    ratio,
                    tab: tab1,
                } => ops.push(Op::Move {
                    pane: (*pane).to_string(),
                    tab: tab1.clone(),
                    split: Direction::Right,
                    target: target.clone(),
                    ratio: *ratio,
                }),
                FirstPane::OwnTab { label, creates } => ops.push(Op::NewTab {
                    pane: (*pane).to_string(),
                    label: label.clone(),
                    creates: *creates,
                }),
            },
            // Column 1's pane is full height, so DOWN pairs it without touching
            // anything else.
            (1, Row::Bottom) => ops.push(Op::Move {
                pane: (*pane).to_string(),
                tab: tab.clone(),
                split: Direction::Down,
                target: tops[0].to_string(),
                ratio: Some(1.0 / ROWS as f64),
            }),
            // A right split subdivides only the pane it targets, so a new column
            // is opened once per row: against column c-1's top, then against its
            // bottom. Never DOWN from the new top pane, which would quarter it.
            (c, Row::Top) => ops.push(Op::Move {
                pane: (*pane).to_string(),
                tab: tab.clone(),
                split: Direction::Right,
                target: tops[c - 2].to_string(),
                ratio: Some(column_ratio(c, total_columns)),
            }),
            (c, Row::Bottom) => ops.push(Op::Move {
                pane: (*pane).to_string(),
                tab: tab.clone(),
                split: Direction::Right,
                target: bottoms[c - 2].to_string(),
                ratio: Some(column_ratio(c, total_columns)),
            }),
        }
        match row {
            Row::Top => tops.push(pane),
            Row::Bottom => bottoms.push(pane),
        }
    }
    ops
}

/// How the first pane of a grid arrives.
enum FirstPane {
    /// Right of a pane that is already in the tab: the orchestrator.
    RightOf {
        tab: TabRef,
        target: String,
        ratio: Option<f64>,
    },
    /// In a tab created around it.
    OwnTab { label: String, creates: usize },
}

/// Plan the whole rearrangement. Pure: it never calls herdr.
pub fn plan(fleet: &Fleet) -> Plan {
    let ordered = order_workers(fleet);
    let n = ordered.len();
    if n == 0 {
        return Plan {
            park: Vec::new(),
            place: Vec::new(),
            placement: Vec::new(),
            created_tabs: 0,
        };
    }
    let panes: Vec<&str> = ordered.iter().map(|w| w.pane.as_str()).collect();

    // Park: one scratch tab, the same 2-row grid, equal columns so no live pane
    // is squeezed to nothing on the way through.
    let park = grid_ops(
        &panes,
        TabRef::Created(0),
        columns_used(n),
        FirstPane::OwnTab {
            label: SCRATCH_LABEL.to_string(),
            creates: 0,
        },
    );

    // Place: tab by tab, in fill order.
    let slots = slots_for(n);
    let mut place = Vec::new();
    let mut created_tabs = 1; // the scratch tab
    for tab_index in 1..=tab_count(n) {
        let on_tab: Vec<&str> = panes
            .iter()
            .zip(&slots)
            .filter(|(_, s)| s.tab_index == tab_index)
            .map(|(p, _)| *p)
            .collect();
        if on_tab.is_empty() {
            continue;
        }
        let used = columns_used(on_tab.len());
        let (tab, first) = if tab_index == 1 {
            (
                TabRef::Existing(fleet.orchestrator_tab.clone()),
                FirstPane::RightOf {
                    tab: TabRef::Existing(fleet.orchestrator_tab.clone()),
                    target: fleet.orchestrator.clone(),
                    ratio: Some(orchestrator_share(used)),
                },
            )
        } else {
            let creates = created_tabs;
            created_tabs += 1;
            (
                TabRef::Created(creates),
                FirstPane::OwnTab {
                    label: format!("workers {tab_index}"),
                    creates,
                },
            )
        };
        place.extend(grid_ops(&on_tab, tab, used, first));
    }

    Plan {
        park,
        place,
        placement: panes
            .iter()
            .zip(slots)
            .map(|(p, s)| ((*p).to_string(), s))
            .collect(),
        created_tabs,
    }
}

/// One tab's live geometry, reduced to what the checks and the balance need.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabShape {
    pub tab_id: String,
    pub area: Rect,
    pub panes: Vec<(String, Rect)>,
    pub zoomed: bool,
}

impl TabShape {
    pub fn from_layout(layout: &Layout) -> Self {
        Self {
            tab_id: layout.tab_id.clone(),
            area: layout.area,
            panes: layout
                .panes
                .iter()
                .map(|p| (p.pane_id.clone(), p.rect))
                .collect(),
            zoomed: layout.zoomed,
        }
    }
}

fn near(a: i64, b: i64) -> bool {
    (a - b).abs() <= TOLERANCE
}

/// True when this tab already holds `workers` in the canonical shape.
///
/// Geometry only, and widths are deliberately not compared: uneven columns are
/// what the balance step is for, and a tab that needs only a resize must not be
/// torn down and rebuilt. `orchestrator` is `Some` on tab 1 and `None` on an
/// overflow tab, where a lone full-height pane is a worker column and not an
/// orchestrator.
pub fn tab_is_canonical(shape: &TabShape, orchestrator: Option<&str>, workers: usize) -> bool {
    let area = shape.area;
    let mut x0 = area.x;
    let x1 = area.x + area.width;

    let mut rest: Vec<&(String, Rect)> = shape.panes.iter().collect();
    if let Some(orch) = orchestrator {
        let Some(pos) = rest.iter().position(|(id, _)| id == orch) else {
            return false;
        };
        let (_, rect) = rest.remove(pos);
        // Leftmost and full height, and nothing to its left.
        if !near(rect.height, area.height) || !near(rect.x, area.x) {
            return false;
        }
        if rest
            .iter()
            .any(|(_, r)| r.x < rect.x + rect.width - TOLERANCE)
        {
            return false;
        }
        x0 = rect.x + rect.width;
    }
    if rest.len() != workers {
        return false;
    }
    if workers == 0 {
        return true;
    }
    if workers == 1 {
        let (_, r) = rest[0];
        return near(r.height, area.height) && near(r.x, x0) && near(r.x + r.width, x1);
    }

    let used = columns_used(workers);
    let mut tops: Vec<Rect> = rest
        .iter()
        .map(|(_, r)| *r)
        .filter(|r| near(r.y, area.y))
        .collect();
    let mut bottoms: Vec<Rect> = rest
        .iter()
        .map(|(_, r)| *r)
        .filter(|r| !near(r.y, area.y))
        .collect();
    if tops.len() != used || bottoms.len() != workers - used {
        return false;
    }
    tops.sort_by_key(|r| r.x);
    bottoms.sort_by_key(|r| r.x);

    // Both rows tile the worker area exactly once, left to right.
    let tiles = |row: &[Rect]| -> bool {
        row.first().is_some_and(|r| near(r.x, x0))
            && row.last().is_some_and(|r| near(r.x + r.width, x1))
            && row
                .windows(2)
                .all(|w| near(w[0].x + w[0].width, w[1].x) && w[0].y == w[1].y)
    };
    if !tiles(&tops) || !tiles(&bottoms) {
        return false;
    }
    // Two rows, no third: the bottom row starts where the top row ends and they
    // fill the height between them.
    if !near(bottoms[0].y, area.y + tops[0].height)
        || !near(tops[0].height + bottoms[0].height, area.height)
    {
        return false;
    }
    // Every column is filled top before bottom, so the bottom panes line up with
    // the leftmost columns and only the last of them may span the rest.
    bottoms
        .iter()
        .enumerate()
        .all(|(i, b)| near(b.x, tops[i].x))
}

/// True when the whole workspace is already the canonical grid for `workers`
/// panes, so nothing needs moving and only the balance has to run.
pub fn is_canonical(tabs: &[TabShape], orchestrator: &str, workers: usize) -> bool {
    if tabs.len() != tab_count(workers) {
        return false;
    }
    tabs.iter().enumerate().all(|(i, shape)| {
        let index = i + 1;
        let orch = if index == 1 { Some(orchestrator) } else { None };
        tab_is_canonical(shape, orch, workers_on_tab(index, workers))
    })
}

/// The next resize that moves this tab toward equal columns, or `None` when every
/// divider is within [`TOLERANCE`].
///
/// One op at a time on purpose. [`crate::balance::plan`] simulates herdr's
/// proportional rescale so it can emit a whole sequence against predicted
/// geometry; a driver that re-reads the layout herdr hands back after each
/// resize needs no prediction, and cannot accumulate rounding. What is reused
/// from that module is the part that is not obvious: a row is a right-leaning
/// chain, so divider `k` belongs to a split spanning `[pos[k-1], x1]` and
/// `--amount` is a fraction of THAT span, and a divider is moved by asking the
/// pane on the side it should grow into to grow.
///
/// Targets come from the grid, not from the pane edges: column boundaries are
/// `x0 + round(k * w / columns)` whatever each row happens to hold, so a row
/// with a spanning bottom pane still lines up with the row above it.
pub fn balance_op(shape: &TabShape, orchestrator: Option<&str>) -> Option<ResizeOp> {
    let area = shape.area;
    let x1 = area.x + area.width;
    let mut rest: Vec<&(String, Rect)> = shape.panes.iter().collect();
    let mut x0 = area.x;

    if let Some(orch) = orchestrator {
        let pos = rest.iter().position(|(id, _)| id == orch)?;
        let (_, orch_rect) = *rest.remove(pos);
        x0 = orch_rect.x + orch_rect.width;
        let used = columns_used(rest.len());
        if used > 0 {
            let target = area.x + (area.width as f64 * orchestrator_share(used)).round() as i64;
            if (target - x0).abs() > TOLERANCE {
                // The orchestrator divider is the root split, so its container is
                // the whole tab.
                let leftmost = rest.iter().min_by_key(|(_, r)| r.x)?;
                let (pane, direction) = if target > x0 {
                    (orch.to_string(), ResizeDir::Right)
                } else {
                    (leftmost.0.clone(), ResizeDir::Left)
                };
                return Some(ResizeOp {
                    pane,
                    direction,
                    amount: (target - x0).abs() as f64 / area.width as f64,
                    from_x: x0,
                    to_x: target,
                });
            }
        }
    }

    let workers = rest.len();
    if workers < 2 {
        return None;
    }
    let used = columns_used(workers);
    let width = x1 - x0;
    if width <= 0 {
        return None;
    }

    for row_top in [true, false] {
        let mut row: Vec<&(String, Rect)> = rest
            .iter()
            .copied()
            .filter(|(_, r)| near(r.y, area.y) == row_top)
            .collect();
        row.sort_by_key(|(_, r)| r.x);
        for k in 1..row.len() {
            let target = x0 + ((k as f64) * (width as f64) / (used as f64)).round() as i64;
            let current = row[k].1.x;
            if (target - current).abs() <= TOLERANCE {
                continue;
            }
            let container = x1 - row[k - 1].1.x;
            if container <= 0 {
                continue;
            }
            let (pane, direction) = if target > current {
                (row[k - 1].0.clone(), ResizeDir::Right)
            } else {
                (row[k].0.clone(), ResizeDir::Left)
            };
            return Some(ResizeOp {
                pane,
                direction,
                amount: (target - current).abs() as f64 / container as f64,
                from_x: current,
                to_x: target,
            });
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn worker(pane: &str, tab: &str, x: i64, y: i64) -> Worker {
        Worker {
            pane: pane.to_string(),
            tab_id: tab.to_string(),
            x,
            y,
        }
    }

    /// `n` workers all sitting in tab 1, left to right, so ordering is the
    /// identity and the ops are the only thing under test.
    fn fleet(n: usize) -> Fleet {
        Fleet {
            tabs: vec!["t1".to_string()],
            orchestrator: "orch".to_string(),
            orchestrator_tab: "t1".to_string(),
            workers: (1..=n)
                .map(|i| worker(&format!("p{i}"), "t1", i as i64 * 10, 0))
                .collect(),
            newcomer: None,
        }
    }

    fn slot(tab_index: usize, column: usize, row: Row) -> Slot {
        Slot {
            tab_index,
            column,
            row,
        }
    }

    #[test]
    fn slots_fill_a_column_before_opening_the_next_and_a_tab_before_opening_the_next() {
        assert_eq!(slots_for(0), vec![]);
        assert_eq!(slots_for(1), vec![slot(1, 1, Row::Top)]);
        assert_eq!(
            slots_for(5),
            vec![
                slot(1, 1, Row::Top),
                slot(1, 1, Row::Bottom),
                slot(1, 2, Row::Top),
                slot(1, 2, Row::Bottom),
                slot(2, 1, Row::Top),
            ]
        );
        assert_eq!(slots_for(11).last().copied(), Some(slot(3, 1, Row::Top)));
        assert_eq!(
            slots_for(16).last().copied(),
            Some(slot(3, 3, Row::Bottom)),
            "4 on tab 1 plus 6 per overflow tab means 16 fills three tabs exactly"
        );
    }

    #[test]
    fn capacities_are_4_then_6_and_tabs_follow_from_them() {
        assert_eq!((capacity(1), capacity(2), capacity(9)), (4, 6, 6));
        assert_eq!(
            (0..=17).map(tab_count).collect::<Vec<_>>(),
            [1, 1, 1, 1, 1, 2, 2, 2, 2, 2, 2, 3, 3, 3, 3, 3, 3, 4]
        );
        assert_eq!(workers_on_tab(1, 11), 4);
        assert_eq!(workers_on_tab(2, 11), 6);
        assert_eq!(workers_on_tab(3, 11), 1);
    }

    #[test]
    fn no_workers_plans_nothing() {
        let plan = plan(&fleet(0));
        assert!(plan.is_empty());
        assert_eq!(plan.created_tabs, 0);
    }

    /// One worker: out to the scratch tab, back beside the orchestrator, which
    /// keeps half the tab because there is one worker column.
    #[test]
    fn one_worker_parks_once_and_lands_right_of_the_orchestrator() {
        let plan = plan(&fleet(1));
        assert_eq!(
            plan.park,
            vec![Op::NewTab {
                pane: "p1".into(),
                label: SCRATCH_LABEL.into(),
                creates: 0
            }]
        );
        assert_eq!(
            plan.place,
            vec![Op::Move {
                pane: "p1".into(),
                tab: TabRef::Existing("t1".into()),
                split: Direction::Right,
                target: "orch".into(),
                ratio: Some(0.5)
            }]
        );
        assert_eq!(plan.created_tabs, 1);
    }

    /// The shape verified by hand against herdr 0.8.2: the fourth worker opens
    /// column 2 in the BOTTOM row, against column 1's bottom pane. A DOWN split
    /// from column 2's top pane instead would halve a half, which is the
    /// quarter-height bug in ai_docs/reports/layout-survey.md section 6.
    #[test]
    fn a_full_tab_1_opens_column_2_in_both_rows_and_never_splits_a_half_pane_down() {
        let plan = plan(&fleet(4));
        assert_eq!(
            plan.place,
            vec![
                Op::Move {
                    pane: "p1".into(),
                    tab: TabRef::Existing("t1".into()),
                    split: Direction::Right,
                    target: "orch".into(),
                    ratio: Some(1.0 / 3.0)
                },
                Op::Move {
                    pane: "p2".into(),
                    tab: TabRef::Existing("t1".into()),
                    split: Direction::Down,
                    target: "p1".into(),
                    ratio: Some(0.5)
                },
                Op::Move {
                    pane: "p3".into(),
                    tab: TabRef::Existing("t1".into()),
                    split: Direction::Right,
                    target: "p1".into(),
                    ratio: Some(0.5)
                },
                Op::Move {
                    pane: "p4".into(),
                    tab: TabRef::Existing("t1".into()),
                    split: Direction::Right,
                    target: "p2".into(),
                    ratio: Some(0.5)
                },
            ]
        );
    }

    #[test]
    fn no_op_ever_splits_the_orchestrator_downward() {
        for n in [1, 4, 5, 10, 11, 16] {
            let p = plan(&fleet(n));
            for op in p.ops() {
                if let Op::Move { split, target, .. } = op {
                    assert!(
                        !(*target == "orch" && *split == Direction::Down),
                        "{n} workers: the orchestrator must never be split DOWN"
                    );
                }
            }
        }
    }

    /// The fifth worker opens tab 2 and takes its first slot; the orchestrator
    /// goes back to a third of tab 1 because tab 1 still has 2 worker columns.
    #[test]
    fn the_fifth_worker_opens_an_overflow_tab() {
        let plan = plan(&fleet(5));
        assert_eq!(plan.created_tabs, 2, "the scratch tab and tab 2");
        assert_eq!(
            plan.placement.last().unwrap(),
            &("p5".to_string(), slot(2, 1, Row::Top))
        );
        assert_eq!(
            plan.place.last().unwrap(),
            &Op::NewTab {
                pane: "p5".into(),
                label: "workers 2".into(),
                creates: 1
            },
            "the first worker of an overflow tab takes a tab of its own"
        );
    }

    /// Three workers on an overflow tab are column 1 top, column 1 bottom,
    /// column 2 top - so its bottom row has one pane spanning two columns.
    #[test]
    fn an_overflow_tab_with_three_workers_leaves_the_last_bottom_slot_free() {
        let plan = plan(&fleet(7));
        let on_tab2: Vec<&(String, Slot)> = plan
            .placement
            .iter()
            .filter(|(_, s)| s.tab_index == 2)
            .collect();
        assert_eq!(
            on_tab2
                .iter()
                .map(|(p, s)| (p.as_str(), s.column, s.row))
                .collect::<Vec<_>>(),
            [
                ("p5", 1, Row::Top),
                ("p6", 1, Row::Bottom),
                ("p7", 2, Row::Top)
            ]
        );
        // Column 2 opens against column 1's TOP pane only, because column 1's
        // bottom slot is the one still free.
        assert_eq!(
            plan.place.last().unwrap(),
            &Op::Move {
                pane: "p7".into(),
                tab: TabRef::Created(1),
                split: Direction::Right,
                target: "p5".into(),
                ratio: Some(0.5)
            }
        );
    }

    /// Equal thirds on a 3-column tab need the second column to leave the first
    /// one a third, then the third column to halve what is left.
    #[test]
    fn an_overflow_tab_opens_its_three_columns_with_equal_width_ratios() {
        let plan = plan(&fleet(10));
        let ratios: Vec<Option<f64>> = plan
            .place
            .iter()
            .filter_map(|op| match op {
                Op::Move { ratio, split, .. } if *split == Direction::Right => Some(*ratio),
                _ => None,
            })
            .collect();
        // Tab 1: column 2 in both rows (1/2 each) after the orchestrator's 1/3.
        // Tab 2: column 2 in both rows (1/3), then column 3 in both rows (1/2).
        assert_eq!(
            ratios,
            vec![
                Some(1.0 / 3.0),
                Some(0.5),
                Some(0.5),
                Some(1.0 / 3.0),
                Some(1.0 / 3.0),
                Some(0.5),
                Some(0.5)
            ]
        );
    }

    #[test]
    fn overflow_tabs_never_exceed_three_columns_and_tab_1_never_two() {
        for n in [1, 4, 5, 10, 11, 16, 23] {
            for (pane, slot) in plan(&fleet(n)).placement {
                let limit = if slot.tab_index == 1 { 2 } else { 3 };
                assert!(
                    slot.column <= limit,
                    "{n} workers: {pane} landed in column {} of tab {}",
                    slot.column,
                    slot.tab_index
                );
            }
        }
    }

    /// Every worker is parked and placed exactly once, whatever the fleet size.
    #[test]
    fn every_worker_is_parked_once_and_placed_once() {
        for n in [1, 4, 5, 10, 11, 16] {
            let plan = plan(&fleet(n));
            let moved = |ops: &[Op]| -> Vec<String> {
                ops.iter()
                    .map(|op| match op {
                        Op::NewTab { pane, .. } => pane.clone(),
                        Op::Move { pane, .. } => pane.clone(),
                        Op::FocusTab { .. } => String::new(),
                    })
                    .collect()
            };
            let expected: Vec<String> = (1..=n).map(|i| format!("p{i}")).collect();
            assert_eq!(moved(&plan.park), expected, "{n} workers, park");
            assert_eq!(moved(&plan.place), expected, "{n} workers, place");
            assert_eq!(plan.placement.len(), n);
        }
    }

    /// The park grid is 2 rows of equal columns too. Halving one pane again and
    /// again would drive the later panes to zero width, and herdr allows that
    /// without complaint.
    #[test]
    fn parking_builds_equal_columns_rather_than_a_chain_of_halves() {
        let plan = plan(&fleet(16));
        let right_ratios: Vec<f64> = plan
            .park
            .iter()
            .filter_map(|op| match op {
                Op::Move {
                    split: Direction::Right,
                    ratio: Some(r),
                    ..
                } => Some(*r),
                _ => None,
            })
            .collect();
        // 16 panes in 2 rows is 8 columns; column c keeps 1/(8-c+2).
        let expected: Vec<f64> = (2..=8)
            .flat_map(|c| [1.0 / (8 - c + 2) as f64; 2])
            .collect();
        assert_eq!(right_ratios, expected);
        assert!(right_ratios.iter().all(|r| *r >= 1.0 / 8.0));
    }

    #[test]
    fn ordering_is_reading_order_and_stable_across_runs() {
        let mut f = fleet(0);
        f.tabs = vec!["t1".into(), "t2".into()];
        f.workers = vec![
            worker("late", "t2", 0, 0),
            worker("b", "t1", 10, 30),
            worker("a", "t1", 10, 0),
            worker("d", "t1", 60, 30),
            worker("c", "t1", 61, 0),
        ];
        let order: Vec<&str> = order_workers(&f).iter().map(|w| w.pane.as_str()).collect();
        assert_eq!(
            order,
            ["a", "b", "c", "d", "late"],
            "tab order, then column, then top before bottom"
        );
        // c and d sit one cell apart: a drifted divider must not swap the column.
        assert_eq!(order_workers(&f), order_workers(&f));
    }

    /// `horch spawn` splits the orchestrator's own pane, so the newcomer appears
    /// at the smallest x of any worker. Ordered by geometry it would take column
    /// 1 top and shift every other worker along on every spawn.
    #[test]
    fn a_newcomer_is_placed_last_however_its_rectangle_sorts() {
        let mut f = fleet(0);
        f.workers = vec![
            worker("fresh", "t1", 5, 0),
            worker("a", "t1", 40, 0),
            worker("b", "t1", 40, 30),
        ];
        f.newcomer = Some("fresh".into());
        let p = plan(&f);
        assert_eq!(
            p.placement,
            vec![
                ("a".to_string(), slot(1, 1, Row::Top)),
                ("b".to_string(), slot(1, 1, Row::Bottom)),
                ("fresh".to_string(), slot(1, 2, Row::Top)),
            ]
        );
    }

    /// Rects measured from herdr 0.8.2 in a 184x53 area with the orchestrator on
    /// a third: the shape the tiler builds must read back as canonical, or every
    /// spawn would rebuild a grid that was already right.
    fn tab1_2x2() -> TabShape {
        TabShape {
            tab_id: "t1".into(),
            area: Rect {
                x: 26,
                y: 1,
                width: 184,
                height: 53,
            },
            panes: vec![
                ("orch".into(), rect(26, 1, 61, 53)),
                ("p1".into(), rect(87, 1, 62, 27)),
                ("p3".into(), rect(149, 1, 61, 27)),
                ("p2".into(), rect(87, 28, 62, 26)),
                ("p4".into(), rect(149, 28, 61, 26)),
            ],
            zoomed: false,
        }
    }

    fn rect(x: i64, y: i64, width: i64, height: i64) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn the_grid_the_tiler_builds_reads_back_as_canonical() {
        assert!(tab_is_canonical(&tab1_2x2(), Some("orch"), 4));
        assert!(is_canonical(&[tab1_2x2()], "orch", 4));
    }

    /// Uneven columns are the balance step's job, not a reason to move panes.
    #[test]
    fn drifted_columns_are_still_canonical() {
        let mut shape = tab1_2x2();
        shape.panes[1].1.width = 70;
        shape.panes[2].1 = rect(157, 1, 53, 27);
        shape.panes[3].1.width = 70;
        shape.panes[4].1 = rect(157, 28, 53, 26);
        assert!(tab_is_canonical(&shape, Some("orch"), 4));
        assert!(
            balance_op(&shape, Some("orch")).is_some(),
            "but it balances"
        );
    }

    /// What `horch spawn` leaves behind: a full-height worker between the
    /// orchestrator and a finished column. Column 1 must be filled before column
    /// 2 exists, so this is not canonical and the fast path must not take it.
    #[test]
    fn a_fresh_spawn_off_the_orchestrator_is_not_canonical() {
        let shape = TabShape {
            tab_id: "t1".into(),
            area: rect(26, 1, 184, 53),
            panes: vec![
                ("orch".into(), rect(26, 1, 61, 53)),
                ("fresh".into(), rect(87, 1, 61, 53)),
                ("p1".into(), rect(148, 1, 62, 27)),
                ("p2".into(), rect(148, 28, 62, 26)),
            ],
            zoomed: false,
        };
        assert!(!tab_is_canonical(&shape, Some("orch"), 3));
    }

    /// The quarter-height shape from the survey: column 2 holds three panes.
    #[test]
    fn the_quarter_height_shape_is_not_canonical() {
        let shape = TabShape {
            tab_id: "t1".into(),
            area: rect(0, 0, 200, 52),
            panes: vec![
                ("orch".into(), rect(0, 0, 50, 52)),
                ("p2".into(), rect(50, 0, 75, 26)),
                ("p4".into(), rect(125, 0, 75, 13)),
                ("p5".into(), rect(125, 13, 75, 13)),
                ("p3".into(), rect(50, 26, 150, 26)),
            ],
            zoomed: false,
        };
        assert!(!tab_is_canonical(&shape, Some("orch"), 4));
    }

    /// Three workers on an overflow tab: two tops and one bottom spanning both
    /// columns, and no orchestrator to exclude. The old `worker_rows` would read
    /// the spanning pane as ragged and refuse; this is the canonical shape.
    #[test]
    fn an_overflow_tab_with_a_spanning_bottom_pane_is_canonical() {
        let shape = TabShape {
            tab_id: "t2".into(),
            area: rect(0, 0, 200, 52),
            panes: vec![
                ("p5".into(), rect(0, 0, 100, 26)),
                ("p7".into(), rect(100, 0, 100, 26)),
                ("p6".into(), rect(0, 26, 200, 26)),
            ],
            zoomed: false,
        };
        assert!(tab_is_canonical(&shape, None, 3));
        // A lone full-height pane on an overflow tab is a worker, not an
        // orchestrator: one worker there is canonical.
        let single = TabShape {
            tab_id: "t2".into(),
            area: rect(0, 0, 200, 52),
            panes: vec![("p5".into(), rect(0, 0, 200, 52))],
            zoomed: false,
        };
        assert!(tab_is_canonical(&single, None, 1));
    }

    #[test]
    fn a_wrong_number_of_tabs_is_not_canonical() {
        assert!(!is_canonical(&[tab1_2x2()], "orch", 5), "tab 2 is missing");
        assert!(
            !is_canonical(&[tab1_2x2(), tab1_2x2()], "orch", 4),
            "one tab too many"
        );
    }

    #[test]
    fn an_even_grid_balances_to_nothing() {
        assert_eq!(balance_op(&tab1_2x2(), Some("orch")), None);
    }

    /// The orchestrator's own divider is the one the old balance would never
    /// touch. Here it is a fifth of the tab and has to grow to a third.
    #[test]
    fn a_narrow_orchestrator_column_is_widened_to_its_share() {
        let mut shape = tab1_2x2();
        shape.panes[0].1.width = 37;
        for i in 1..=4 {
            shape.panes[i].1.x -= 24;
        }
        shape.panes[1].1.width += 24;
        shape.panes[3].1.width += 24;
        let op = balance_op(&shape, Some("orch")).expect("the orchestrator is too narrow");
        assert_eq!(op.pane, "orch");
        assert_eq!(op.direction, ResizeDir::Right);
        assert_eq!((op.from_x, op.to_x), (63, 87));
        assert!((op.amount - 24.0 / 184.0).abs() < 1e-9);
    }

    /// A row divider is a fraction of the split that owns it, which spans from
    /// the previous divider to the right edge - not of the whole row.
    #[test]
    fn a_row_divider_moves_by_a_fraction_of_the_split_that_owns_it() {
        let mut shape = tab1_2x2();
        // Top row: column 1 swallowed 20 cells of column 2.
        shape.panes[1].1.width = 82;
        shape.panes[2].1 = rect(169, 1, 41, 27);
        let op = balance_op(&shape, Some("orch")).expect("the top row is uneven");
        assert_eq!((op.pane.as_str(), op.direction), ("p3", ResizeDir::Left));
        assert_eq!((op.from_x, op.to_x), (169, 149));
        // The container is [87, 210], so 20 cells is 20/123.
        assert!((op.amount - 20.0 / 123.0).abs() < 1e-9, "{}", op.amount);
    }

    /// An overflow tab has no orchestrator column to exclude, and its columns are
    /// measured against the whole tab.
    #[test]
    fn an_overflow_tabs_columns_are_measured_against_the_whole_tab() {
        let shape = TabShape {
            tab_id: "t2".into(),
            area: rect(0, 0, 300, 52),
            panes: vec![
                ("p5".into(), rect(0, 0, 100, 26)),
                ("p7".into(), rect(100, 0, 200, 26)),
                ("p6".into(), rect(0, 26, 140, 26)),
                ("p8".into(), rect(140, 26, 160, 26)),
            ],
            zoomed: false,
        };
        // 4 workers is 2 columns, so both rows want their divider at 150.
        let op = balance_op(&shape, None).expect("the top row divider is at 100");
        assert_eq!((op.pane.as_str(), op.direction), ("p5", ResizeDir::Right));
        assert_eq!((op.from_x, op.to_x), (100, 150));
    }
}
