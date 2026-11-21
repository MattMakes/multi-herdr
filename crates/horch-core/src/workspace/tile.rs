use std::collections::HashMap;

use crate::workspace::balance::{ResizeDir, ResizeOp};
use crate::workspace::model::{Direction, FocusDir, Layout, Rect};

pub(crate) const SCRATCH_LABEL: &str = "horch-tile-scratch";

pub(crate) const TAB1_COLUMNS: usize = 2;
pub(crate) const OVERFLOW_COLUMNS: usize = 3;
pub(crate) const ROWS: usize = 2;

pub(crate) const TOLERANCE: i64 = 1;

pub fn columns(tab_index: usize) -> usize {
    if tab_index <= 1 {
        TAB1_COLUMNS
    } else {
        OVERFLOW_COLUMNS
    }
}

pub(crate) fn capacity(tab_index: usize) -> usize {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Slot {
    pub tab_index: usize,
    pub column: usize,
    pub row: Row,
}

pub(crate) fn slots_for(n: usize) -> Vec<Slot> {
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

pub(crate) fn tab_count(n: usize) -> usize {
    slots_for(n).last().map(|s| s.tab_index).unwrap_or(1)
}

pub(crate) fn workers_on_tab(tab_index: usize, n: usize) -> usize {
    slots_for(n)
        .iter()
        .filter(|s| s.tab_index == tab_index)
        .count()
}

fn columns_used(workers: usize) -> usize {
    workers.div_ceil(ROWS)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TabRef {
    Existing(String),
    Created(usize),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    NewTab {
        pane: String,
        label: String,
        creates: usize,
    },
    Move {
        pane: String,
        tab: TabRef,
        split: Direction,
        target: String,
        ratio: Option<f64>,
    },
    FocusTab {
        tab: TabRef,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Worker {
    pub pane: String,
    pub tab_id: String,
    pub x: i64,
    pub y: i64,
}

#[derive(Debug, Clone)]
pub struct Fleet {
    pub tabs: Vec<String>,
    pub orchestrator: String,
    pub orchestrator_tab: String,
    pub workers: Vec<Worker>,
    pub newcomer: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    pub park: Vec<Op>,
    pub place: Vec<Op>,
    pub placement: Vec<(String, Slot)>,
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

pub(crate) fn order_workers(fleet: &Fleet) -> Vec<&Worker> {
    let tab_index = |tab_id: &str| {
        fleet
            .tabs
            .iter()
            .position(|t| t == tab_id)
            .unwrap_or(usize::MAX)
    };

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

fn column_ratio(c: usize, total: usize) -> f64 {
    1.0 / (total.saturating_sub(c) + 2) as f64
}

pub fn orchestrator_share(worker_columns: usize) -> f64 {
    1.0 / (worker_columns + 1) as f64
}

fn grid_ops(panes: &[&str], tab: TabRef, total_columns: usize, first: FirstPane) -> Vec<Op> {
    let mut ops = Vec::new();
    let mut tops: Vec<&str> = Vec::new();
    let mut bottoms: Vec<&str> = Vec::new();

    for (i, pane) in panes.iter().enumerate() {
        let column = i / ROWS + 1;
        let row = if i % ROWS == 0 { Row::Top } else { Row::Bottom };
        ops.push(match (column, row) {
            (1, Row::Top) => first_op(pane, &first),
            _ => grid_op(pane, &tab, (column, row), (&tops, &bottoms), total_columns),
        });
        match row {
            Row::Top => tops.push(pane),
            Row::Bottom => bottoms.push(pane),
        }
    }
    ops
}

fn first_op(pane: &str, first: &FirstPane) -> Op {
    match first {
        FirstPane::RightOf { target, ratio, tab } => Op::Move {
            pane: pane.to_string(),
            tab: tab.clone(),
            split: Direction::Right,
            target: target.clone(),
            ratio: *ratio,
        },
        FirstPane::OwnTab { label, creates } => Op::NewTab {
            pane: pane.to_string(),
            label: label.clone(),
            creates: *creates,
        },
    }
}

fn grid_op(
    pane: &str,
    tab: &TabRef,
    (column, row): (usize, Row),
    (tops, bottoms): (&[&str], &[&str]),
    total_columns: usize,
) -> Op {
    let (split, target, ratio) = match (column, row) {
        (1, _) => (Direction::Down, tops[0], 1.0 / ROWS as f64),
        (c, Row::Top) => (
            Direction::Right,
            tops[c - 2],
            column_ratio(c, total_columns),
        ),
        (c, Row::Bottom) => (
            Direction::Right,
            bottoms[c - 2],
            column_ratio(c, total_columns),
        ),
    };
    Op::Move {
        pane: pane.to_string(),
        tab: tab.clone(),
        split,
        target: target.to_string(),
        ratio: Some(ratio),
    }
}

enum FirstPane {
    RightOf {
        tab: TabRef,
        target: String,
        ratio: Option<f64>,
    },
    OwnTab {
        label: String,
        creates: usize,
    },
}

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

    let park = grid_ops(
        &panes,
        TabRef::Created(0),
        columns_used(n),
        FirstPane::OwnTab {
            label: SCRATCH_LABEL.to_string(),
            creates: 0,
        },
    );

    let slots = slots_for(n);
    let mut place = Vec::new();
    let mut created_tabs = 1;
    let tabs = (1..=tab_count(n)).map(|tab_index| {
        let on_tab: Vec<&str> = panes
            .iter()
            .zip(&slots)
            .filter(|(_, s)| s.tab_index == tab_index)
            .map(|(p, _)| *p)
            .collect();
        (tab_index, on_tab)
    });
    for (tab_index, on_tab) in tabs.filter(|(_, on_tab)| !on_tab.is_empty()) {
        let used = columns_used(on_tab.len());
        let (tab, first) = tab_start(fleet, tab_index, used, &mut created_tabs);
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

fn tab_start(
    fleet: &Fleet,
    tab_index: usize,
    used: usize,
    created_tabs: &mut usize,
) -> (TabRef, FirstPane) {
    if tab_index == 1 {
        return (
            TabRef::Existing(fleet.orchestrator_tab.clone()),
            FirstPane::RightOf {
                tab: TabRef::Existing(fleet.orchestrator_tab.clone()),
                target: fleet.orchestrator.clone(),
                ratio: Some(orchestrator_share(used)),
            },
        );
    }
    let creates = *created_tabs;
    *created_tabs += 1;
    (
        TabRef::Created(creates),
        FirstPane::OwnTab {
            label: format!("workers {tab_index}"),
            creates,
        },
    )
}

impl TabRef {
    pub fn describe(&self) -> String {
        match self {
            TabRef::Existing(id) => id.clone(),
            TabRef::Created(0) => "<scratch tab>".to_string(),
            TabRef::Created(i) => format!("<new tab {}>", i + 1),
        }
    }
}

pub fn command_line(op: &Op) -> String {
    match op {
        Op::NewTab { pane, label, .. } => {
            format!("herdr pane move {pane} --new-tab --label \"{label}\" --no-focus")
        }
        Op::Move {
            pane,
            tab,
            split,
            target,
            ratio,
        } => {
            let ratio = match ratio {
                Some(r) => format!(" --ratio {r:.4}"),
                None => String::new(),
            };
            format!(
                "herdr pane move {pane} --tab {} --split {} --target-pane {target}{ratio} --no-focus",
                tab.describe(),
                split.as_str()
            )
        }
        Op::FocusTab { tab } => format!("herdr tab focus {}", tab.describe()),
    }
}

pub(crate) fn render_target(
    plan: &Plan,
    orchestrator: &str,
    roles: &HashMap<String, String>,
) -> String {
    let n = plan.placement.len();
    if n == 0 {
        return format!(
            "after: tab 1 holds {} alone; no workers to place\n",
            target_label(roles, orchestrator)
        );
    }
    (1..=tab_count(n))
        .map(|tab_index| {
            let on_tab: Vec<&(String, Slot)> = plan
                .placement
                .iter()
                .filter(|(_, s)| s.tab_index == tab_index)
                .collect();
            render_tab(tab_index, &on_tab, orchestrator, roles)
        })
        .collect()
}

fn target_label(roles: &HashMap<String, String>, id: &str) -> String {
    match roles.get(id) {
        Some(role) => format!("{role}({id})"),
        None => id.to_string(),
    }
}

fn render_tab(
    tab_index: usize,
    on_tab: &[&(String, Slot)],
    orchestrator: &str,
    roles: &HashMap<String, String>,
) -> String {
    if on_tab.is_empty() {
        return String::new();
    }
    let used = columns_used(on_tab.len());
    let at = |column: usize, row: Row| -> String {
        on_tab
            .iter()
            .find(|(_, s)| s.column == column && s.row == row)
            .map_or_else(|| "-".to_string(), |(p, _)| target_label(roles, p))
    };
    let header = if tab_index == 1 {
        format!(
            "after: tab 1  orchestrator {} (1/{} of the tab)  {} worker(s), {} column(s)\n",
            target_label(roles, orchestrator),
            used + 1,
            on_tab.len(),
            used
        )
    } else {
        format!(
            "       tab {tab_index}  {} worker(s), {used} column(s)\n",
            on_tab.len()
        )
    };
    let cells: Vec<(String, String, String)> = (1..=used)
        .map(|c| (format!("c{c}"), at(c, Row::Top), at(c, Row::Bottom)))
        .collect();
    header + &grid_lines(&cells)
}

fn grid_lines(cells: &[(String, String, String)]) -> String {
    let widths: Vec<usize> = cells
        .iter()
        .map(|(h, t, b)| {
            h.chars()
                .count()
                .max(t.chars().count())
                .max(b.chars().count())
        })
        .collect();
    let mut out = String::new();
    for (prefix, pick) in [("         ", 0usize), ("  top    ", 1), ("  bottom ", 2)] {
        out.push_str(prefix);
        for (cell, w) in cells.iter().zip(&widths) {
            let text = [&cell.0, &cell.1, &cell.2][pick];
            out.push_str(&format!("{text:<w$}  "));
        }
        out.push('\n');
    }
    out
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FocusState {
    pub tab: String,
    pub pane: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FocusTarget {
    pub tab: String,
    pub pane: Option<String>,
    pub kept: bool,
    pub reason: &'static str,
}

pub(crate) fn focus_target(
    before: &FocusState,
    after: &[TabShape],
    orchestrator: &str,
) -> Option<FocusTarget> {
    let holder = |pane: &str| {
        after
            .iter()
            .find(|t| t.panes.iter().any(|(id, _)| id == pane))
            .map(|t| t.tab_id.clone())
    };
    let orchestrator_tab = holder(orchestrator);

    let (tab, tab_kept, reason) = if after.iter().any(|t| t.tab_id == before.tab) {
        (before.tab.clone(), true, "")
    } else if let Some(tab) = holder(&before.pane) {
        (
            tab,
            false,
            "the viewed tab closed, so the view follows the pane",
        )
    } else {
        let tab = orchestrator_tab
            .clone()
            .or_else(|| after.first().map(|t| t.tab_id.clone()))?;
        (
            tab,
            false,
            "the viewed tab and the focused pane are both gone",
        )
    };

    let on_tab = |pane: &str| {
        after
            .iter()
            .find(|t| t.tab_id == tab)
            .is_some_and(|t| t.panes.iter().any(|(id, _)| id == pane))
    };
    if on_tab(&before.pane) {
        return Some(FocusTarget {
            tab,
            pane: Some(before.pane.clone()),
            kept: tab_kept,
            reason,
        });
    }
    let orchestrator_here = orchestrator_tab.as_deref() == Some(tab.as_str());
    let reason = if !reason.is_empty() {
        reason
    } else if holder(&before.pane).is_some() {
        "the focused pane moved to another tab"
    } else {
        "the focused pane is gone"
    };
    Some(FocusTarget {
        pane: orchestrator_here.then(|| orchestrator.to_string()),
        tab,
        kept: false,
        reason,
    })
}

pub(crate) fn step_toward(from: &Rect, to: &Rect) -> Option<FocusDir> {
    let overlaps_x = spans(from.x, from.x + from.width, to.x, to.x + to.width);
    let overlaps_y = spans(from.y, from.y + from.height, to.y, to.y + to.height);
    let dx = (to.x * 2 + to.width) - (from.x * 2 + from.width);
    let dy = (to.y * 2 + to.height) - (from.y * 2 + from.height);
    let horizontal = toward(dx, FocusDir::Right, FocusDir::Left);
    let vertical = toward(dy, FocusDir::Down, FocusDir::Up);
    match (overlaps_x, overlaps_y) {
        (true, true) => None,
        (false, true) => horizontal,
        (true, false) => vertical,
        (false, false) => {
            if wider_axis(dx, dy) {
                horizontal
            } else {
                vertical
            }
        }
    }
}

fn spans(a0: i64, a1: i64, b0: i64, b1: i64) -> bool {
    a0 < b1 && b0 < a1
}

fn toward(d: i64, positive: FocusDir, negative: FocusDir) -> Option<FocusDir> {
    match d.signum() {
        0 => None,
        1 => Some(positive),
        _ => Some(negative),
    }
}

fn wider_axis(dx: i64, dy: i64) -> bool {
    dx.abs() >= dy.abs()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TabShape {
    pub tab_id: String,
    pub area: Rect,
    pub panes: Vec<(String, Rect)>,
    pub zoomed: bool,
}

impl TabShape {
    pub(crate) fn from_layout(layout: &Layout) -> Self {
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

pub(crate) fn tab_is_canonical(
    shape: &TabShape,
    orchestrator: Option<&str>,
    workers: usize,
) -> bool {
    let area = shape.area;
    let mut rest: Vec<&(String, Rect)> = shape.panes.iter().collect();
    let x0 = match orchestrator {
        Some(orch) => orchestrator_edge(&mut rest, orch, area),
        None => Some(area.x),
    };
    let Some(x0) = x0 else {
        return false;
    };
    let x1 = area.x + area.width;
    rest.len() == workers
        && match workers {
            0 => true,
            1 => single_fills(rest[0], area, x0, x1),
            _ => grid_fills(&rest, area, x0, x1, workers),
        }
}

fn orchestrator_edge(rest: &mut Vec<&(String, Rect)>, orch: &str, area: Rect) -> Option<i64> {
    let pos = rest.iter().position(|(id, _)| id == orch)?;
    let (_, rect) = *rest.remove(pos);
    let leftmost_and_full = near(rect.height, area.height)
        && near(rect.x, area.x)
        && !rest
            .iter()
            .any(|(_, r)| r.x < rect.x + rect.width - TOLERANCE);
    leftmost_and_full.then_some(rect.x + rect.width)
}

fn single_fills((_, r): &(String, Rect), area: Rect, x0: i64, x1: i64) -> bool {
    near(r.height, area.height) && near(r.x, x0) && near(r.x + r.width, x1)
}

fn grid_fills(rest: &[&(String, Rect)], area: Rect, x0: i64, x1: i64, workers: usize) -> bool {
    let used = columns_used(workers);
    let (tops, bottoms) = split_rows(rest, area);
    tops.len() == used
        && bottoms.len() == workers - used
        && row_tiles(&tops, x0, x1)
        && row_tiles(&bottoms, x0, x1)
        && rows_stack(&tops, &bottoms, area)
        && bottoms.iter().zip(&tops).all(|(b, t)| near(b.x, t.x))
}

fn split_rows(rest: &[&(String, Rect)], area: Rect) -> (Vec<Rect>, Vec<Rect>) {
    let row = |top: bool| {
        let mut row: Vec<Rect> = rest
            .iter()
            .map(|(_, r)| *r)
            .filter(|r| near(r.y, area.y) == top)
            .collect();
        row.sort_by_key(|r| r.x);
        row
    };
    (row(true), row(false))
}

fn row_tiles(row: &[Rect], x0: i64, x1: i64) -> bool {
    row.first().is_some_and(|r| near(r.x, x0))
        && row.last().is_some_and(|r| near(r.x + r.width, x1))
        && row
            .windows(2)
            .all(|w| near(w[0].x + w[0].width, w[1].x) && w[0].y == w[1].y)
}

fn rows_stack(tops: &[Rect], bottoms: &[Rect], area: Rect) -> bool {
    near(bottoms[0].y, area.y + tops[0].height)
        && near(tops[0].height + bottoms[0].height, area.height)
}

pub(crate) fn is_canonical(tabs: &[TabShape], orchestrator: &str, workers: usize) -> bool {
    if tabs.len() != tab_count(workers) {
        return false;
    }
    tabs.iter().enumerate().all(|(i, shape)| {
        let index = i + 1;
        let orch = if index == 1 { Some(orchestrator) } else { None };
        tab_is_canonical(shape, orch, workers_on_tab(index, workers))
    })
}

pub(crate) fn balance_op(shape: &TabShape, orchestrator: Option<&str>) -> Option<ResizeOp> {
    let area = shape.area;
    let mut rest: Vec<&(String, Rect)> = shape.panes.iter().collect();
    let (x0, divider) = match orchestrator {
        Some(orch) => orchestrator_op(&mut rest, orch, area)?,
        None => (area.x, None),
    };
    divider.or_else(|| worker_op(&rest, area, x0))
}

fn orchestrator_op(
    rest: &mut Vec<&(String, Rect)>,
    orch: &str,
    area: Rect,
) -> Option<(i64, Option<ResizeOp>)> {
    let pos = rest.iter().position(|(id, _)| id == orch)?;
    let (_, orch_rect) = *rest.remove(pos);
    let x0 = orch_rect.x + orch_rect.width;
    let used = columns_used(rest.len());
    let target = area.x + (area.width as f64 * orchestrator_share(used)).round() as i64;
    if used == 0 || (target - x0).abs() <= TOLERANCE {
        return Some((x0, None));
    }
    let leftmost = rest.iter().min_by_key(|(_, r)| r.x)?;
    let op = resize_toward((orch, &leftmost.0), x0, target, area.width);
    Some((x0, Some(op)))
}

fn resize_toward(grow: (&str, &str), current: i64, target: i64, container: i64) -> ResizeOp {
    let (pane, direction) = if target > current {
        (grow.0, ResizeDir::Right)
    } else {
        (grow.1, ResizeDir::Left)
    };
    ResizeOp {
        pane: pane.to_string(),
        direction,
        amount: (target - current).abs() as f64 / container as f64,
        from_x: current,
        to_x: target,
    }
}

fn worker_op(rest: &[&(String, Rect)], area: Rect, x0: i64) -> Option<ResizeOp> {
    let x1 = area.x + area.width;
    let width = x1 - x0;
    if rest.len() < 2 || width <= 0 {
        return None;
    }
    let used = columns_used(rest.len());
    [true, false].into_iter().find_map(|row_top| {
        let mut row: Vec<&(String, Rect)> = rest
            .iter()
            .copied()
            .filter(|(_, r)| near(r.y, area.y) == row_top)
            .collect();
        row.sort_by_key(|(_, r)| r.x);
        (1..row.len()).find_map(|k| {
            let target = x0 + ((k as f64) * (width as f64) / (used as f64)).round() as i64;
            let current = row[k].1.x;
            let container = x1 - row[k - 1].1.x;
            ((target - current).abs() > TOLERANCE && container > 0)
                .then(|| resize_toward((&row[k - 1].0, &row[k].0), current, target, container))
        })
    })
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
        let mut rerun = fleet(0);
        rerun.tabs = f.tabs.clone();
        rerun.workers = vec![
            worker("w", "t1", 60, 30),
            worker("v", "t2", 0, 0),
            worker("y", "t1", 11, 30),
            worker("x", "t1", 61, 0),
            worker("z", "t1", 10, 0),
        ];
        let again: Vec<&str> = order_workers(&rerun)
            .iter()
            .map(|w| w.pane.as_str())
            .collect();
        assert_eq!(again, ["z", "y", "x", "w", "v"]);
    }

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

    fn overflow_tab(id: &str, panes: &[&str]) -> TabShape {
        let slots = [
            rect(26, 1, 61, 27),
            rect(87, 1, 62, 27),
            rect(149, 1, 61, 27),
            rect(26, 28, 61, 26),
            rect(87, 28, 62, 26),
            rect(149, 28, 61, 26),
        ];
        TabShape {
            tab_id: id.into(),
            area: rect(26, 1, 184, 53),
            panes: panes
                .iter()
                .zip(slots)
                .map(|(id, r)| ((*id).to_string(), r))
                .collect(),
            zoomed: false,
        }
    }

    #[test]
    fn focus_target_keeps_a_tab_and_pane_that_both_survived() {
        let after = [tab1_2x2()];
        let before = FocusState {
            tab: "t1".into(),
            pane: "p2".into(),
        };
        let target = focus_target(&before, &after, "orch").unwrap();
        assert_eq!(target.tab, "t1");
        assert_eq!(target.pane.as_deref(), Some("p2"));
        assert!(target.kept);
        assert_eq!(target.reason, "");
    }

    #[test]
    fn focus_target_follows_the_pane_when_its_tab_was_rebuilt() {
        let after = [tab1_2x2(), overflow_tab("t8", &["p5", "p6", "p7"])];
        let before = FocusState {
            tab: "t6".into(),
            pane: "p5".into(),
        };
        let target = focus_target(&before, &after, "orch").unwrap();
        assert_eq!(target.tab, "t8", "the pane's new tab, not the old id");
        assert_eq!(target.pane.as_deref(), Some("p5"));
        assert!(!target.kept);
        assert!(
            target.reason.contains("follows the pane"),
            "{}",
            target.reason
        );
    }

    #[test]
    fn focus_target_falls_back_to_the_orchestrator_when_both_are_gone() {
        let after = [tab1_2x2()];
        let before = FocusState {
            tab: "t6".into(),
            pane: "gone".into(),
        };
        let target = focus_target(&before, &after, "orch").unwrap();
        assert_eq!(target.tab, "t1");
        assert_eq!(target.pane.as_deref(), Some("orch"));
        assert!(!target.kept);
    }

    #[test]
    fn focus_target_leaves_herdrs_choice_on_an_overflow_tab() {
        let after = [tab1_2x2(), overflow_tab("t8", &["p5", "p6"])];
        let on_tab1 = focus_target(
            &FocusState {
                tab: "t1".into(),
                pane: "gone".into(),
            },
            &after,
            "orch",
        )
        .unwrap();
        assert_eq!(on_tab1.tab, "t1");
        assert_eq!(on_tab1.pane.as_deref(), Some("orch"));

        let on_overflow = focus_target(
            &FocusState {
                tab: "t8".into(),
                pane: "gone".into(),
            },
            &after,
            "orch",
        )
        .unwrap();
        assert_eq!(on_overflow.tab, "t8");
        assert_eq!(on_overflow.pane, None, "herdr's choice stands");
        assert!(!on_overflow.kept);
    }

    #[test]
    fn focus_target_says_whether_the_pane_moved_or_closed() {
        let after = [tab1_2x2(), overflow_tab("t8", &["p5", "p6"])];
        let moved = focus_target(
            &FocusState {
                tab: "t1".into(),
                pane: "p5".into(),
            },
            &after,
            "orch",
        )
        .unwrap();
        assert_eq!(moved.tab, "t1", "the viewed tab wins");
        assert_eq!(moved.pane.as_deref(), Some("orch"));
        assert_eq!(moved.reason, "the focused pane moved to another tab");

        let closed = focus_target(
            &FocusState {
                tab: "t1".into(),
                pane: "gone".into(),
            },
            &after,
            "orch",
        )
        .unwrap();
        assert_eq!(closed.reason, "the focused pane is gone");
    }

    #[test]
    fn focus_target_of_an_empty_workspace_is_none() {
        let before = FocusState {
            tab: "t1".into(),
            pane: "p2".into(),
        };
        assert!(focus_target(&before, &[], "orch").is_none());
    }

    #[test]
    fn step_toward_picks_the_axis_with_the_larger_gap() {
        let here = rect(87, 28, 62, 26);
        assert_eq!(
            step_toward(&here, &rect(149, 28, 61, 26)),
            Some(FocusDir::Right)
        );
        assert_eq!(
            step_toward(&here, &rect(26, 1, 61, 53)),
            Some(FocusDir::Left)
        );
        assert_eq!(step_toward(&here, &rect(87, 1, 62, 27)), Some(FocusDir::Up));
        assert_eq!(
            step_toward(&rect(87, 1, 62, 27), &here),
            Some(FocusDir::Down)
        );
        assert_eq!(step_toward(&here, &here), None, "the same pane");
    }

    #[test]
    fn step_toward_goes_down_to_a_pane_that_spans_the_columns_above_it() {
        let spanning = rect(87, 28, 123, 26);
        assert_eq!(
            step_toward(&rect(149, 1, 61, 27), &spanning),
            Some(FocusDir::Down),
            "from the top-right pane"
        );
        assert_eq!(
            step_toward(&rect(87, 1, 62, 27), &spanning),
            Some(FocusDir::Down),
            "from the top-left pane"
        );
        assert_eq!(
            step_toward(&rect(26, 1, 61, 53), &spanning),
            Some(FocusDir::Right)
        );
    }

    #[test]
    fn step_toward_crosses_the_columns_before_the_rows() {
        let from = rect(26, 1, 61, 53);
        let to = rect(149, 28, 61, 26);
        assert_eq!(step_toward(&from, &to), Some(FocusDir::Right));
        assert_eq!(
            step_toward(&rect(149, 1, 61, 27), &to),
            Some(FocusDir::Down)
        );
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
    fn a_plan_prints_as_the_herdr_commands_it_stands_for() {
        let plan = plan(&fleet(5));
        let lines: Vec<String> = plan.ops().map(command_line).collect();
        assert_eq!(
            lines[0],
            "herdr pane move p1 --new-tab --label \"horch-tile-scratch\" --no-focus"
        );
        assert!(
            lines.iter().any(|l| l
                == "herdr pane move p1 --tab t1 --split right --target-pane orch --ratio 0.3333 --no-focus"),
            "{lines:#?}"
        );
        assert_eq!(
            lines.last().unwrap(),
            "herdr pane move p5 --new-tab --label \"workers 2\" --no-focus"
        );
        assert!(
            lines.iter().any(|l| l.contains("--tab <scratch tab>")),
            "a tab the plan creates is named by its place in the plan: {lines:#?}"
        );
    }

    #[test]
    fn the_target_grid_names_every_slot_and_marks_the_free_ones() {
        let mut roles = HashMap::new();
        roles.insert("orch".to_string(), "orchestrator".to_string());
        roles.insert("p1".to_string(), "sonnet-1".to_string());
        let out = render_target(&plan(&fleet(7)), "orch", &roles);
        assert!(out.contains("orchestrator(orch) (1/3 of the tab)"), "{out}");
        assert!(out.contains("sonnet-1(p1)"), "{out}");
        assert!(out.contains("tab 2  3 worker(s), 2 column(s)"), "{out}");
        let bottom = out.lines().rfind(|l| l.starts_with("  bottom")).unwrap();
        assert!(bottom.contains("p6"), "{bottom}");
        assert!(bottom.trim_end().ends_with('-'), "{bottom}");
    }

    #[test]
    fn an_empty_fleet_renders_as_the_orchestrator_alone() {
        let out = render_target(&plan(&fleet(0)), "orch", &HashMap::new());
        assert_eq!(out, "after: tab 1 holds orch alone; no workers to place\n");
    }

    #[test]
    fn an_even_grid_balances_to_nothing() {
        assert_eq!(balance_op(&tab1_2x2(), Some("orch")), None);
    }

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

    #[test]
    fn a_row_divider_moves_by_a_fraction_of_the_split_that_owns_it() {
        let mut shape = tab1_2x2();
        shape.panes[1].1.width = 82;
        shape.panes[2].1 = rect(169, 1, 41, 27);
        let op = balance_op(&shape, Some("orch")).expect("the top row is uneven");
        assert_eq!((op.pane.as_str(), op.direction), ("p3", ResizeDir::Left));
        assert_eq!((op.from_x, op.to_x), (169, 149));
        assert!((op.amount - 20.0 / 123.0).abs() < 1e-9, "{}", op.amount);
    }

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
        let op = balance_op(&shape, None).expect("the top row divider is at 100");
        assert_eq!((op.pane.as_str(), op.direction), ("p5", ResizeDir::Right));
        assert_eq!((op.from_x, op.to_x), (100, 150));
    }

    #[test]
    fn every_broken_grid_rule_is_not_canonical() {
        type Break = Box<dyn Fn(&mut TabShape)>;
        let broken: Vec<(&str, Break)> = vec![
            (
                "orchestrator pane missing",
                Box::new(|s| s.panes[0].0 = "gone".into()),
            ),
            (
                "orchestrator not full height",
                Box::new(|s| s.panes[0].1.height = 40),
            ),
            (
                "orchestrator not leftmost",
                Box::new(|s| s.panes[0].1.x = 30),
            ),
            (
                "worker left of the orchestrator's edge",
                Box::new(|s| s.panes[1].1.x = 80),
            ),
            (
                "3 panes on top",
                Box::new(|s| s.panes[3].1 = rect(87, 1, 62, 27)),
            ),
            ("gap in the top row", Box::new(|s| s.panes[2].1.x = 160)),
            ("top row ends short", Box::new(|s| s.panes[2].1.width = 50)),
            (
                "gap above the bottom row",
                Box::new(|s| {
                    s.panes[3].1.y = 40;
                    s.panes[4].1.y = 40;
                }),
            ),
            (
                "rows do not fill the height",
                Box::new(|s| {
                    s.panes[3].1.height = 20;
                    s.panes[4].1.height = 20;
                }),
            ),
            (
                "bottom columns off the top columns",
                Box::new(|s| {
                    s.panes[3].1.width = 43;
                    s.panes[4].1 = rect(130, 28, 80, 26);
                }),
            ),
        ];
        assert!(tab_is_canonical(&tab1_2x2(), Some("orch"), 4));
        for (what, change) in broken {
            let mut shape = tab1_2x2();
            change(&mut shape);
            assert!(!tab_is_canonical(&shape, Some("orch"), 4), "{what}");
        }
    }

    #[test]
    fn the_worker_count_must_match_the_panes_on_the_tab() {
        assert!(!tab_is_canonical(&tab1_2x2(), Some("orch"), 3));
        let alone = TabShape {
            tab_id: "t1".into(),
            area: rect(26, 1, 184, 53),
            panes: vec![("orch".into(), rect(26, 1, 184, 53))],
            zoomed: false,
        };
        assert!(tab_is_canonical(&alone, Some("orch"), 0));
        assert!(!tab_is_canonical(&alone, Some("orch"), 1));
    }

    #[test]
    fn a_single_worker_must_fill_the_worker_area() {
        let one = |r: Rect| TabShape {
            tab_id: "t2".into(),
            area: rect(0, 0, 200, 52),
            panes: vec![("p5".into(), r)],
            zoomed: false,
        };
        assert!(tab_is_canonical(&one(rect(0, 0, 200, 52)), None, 1));
        assert!(
            !tab_is_canonical(&one(rect(0, 0, 200, 26)), None, 1),
            "short"
        );
        assert!(
            !tab_is_canonical(&one(rect(10, 0, 190, 52)), None, 1),
            "left gap"
        );
        assert!(
            !tab_is_canonical(&one(rect(0, 0, 150, 52)), None, 1),
            "right gap"
        );
    }

    #[test]
    fn the_orchestrator_divider_moves_left_when_the_column_is_too_wide() {
        let mut shape = tab1_2x2();
        shape.panes[0].1.width = 85;
        shape.panes[1].1 = rect(111, 1, 38, 27);
        shape.panes[3].1 = rect(111, 28, 38, 26);
        let op = balance_op(&shape, Some("orch")).expect("too wide");
        assert_eq!((op.pane.as_str(), op.direction), ("p1", ResizeDir::Left));
        assert_eq!((op.from_x, op.to_x), (111, 87));
        assert!((op.amount - 24.0 / 184.0).abs() < 1e-9);
    }

    #[test]
    fn nothing_balances_without_the_orchestrator_pane_or_two_workers_or_width() {
        assert_eq!(balance_op(&tab1_2x2(), Some("gone")), None);
        let mut one_worker = tab1_2x2();
        one_worker.panes.truncate(2);
        one_worker.panes[0].1.width = 92;
        one_worker.panes[1].1 = rect(118, 1, 92, 53);
        assert_eq!(balance_op(&one_worker, Some("orch")), None);
        let mut flat = overflow_tab("t2", &["a", "b", "c", "d"]);
        flat.area.width = 0;
        assert_eq!(balance_op(&flat, None), None);
    }

    #[test]
    fn the_bottom_row_is_balanced_after_the_top_row() {
        let mut shape = tab1_2x2();
        shape.panes[3].1.width = 42;
        shape.panes[4].1 = rect(129, 28, 81, 26);
        let op = balance_op(&shape, Some("orch")).expect("the bottom row is uneven");
        assert_eq!((op.pane.as_str(), op.direction), ("p2", ResizeDir::Right));
        assert_eq!((op.from_x, op.to_x), (129, 149));
        assert!((op.amount - 20.0 / 123.0).abs() < 1e-9, "{}", op.amount);
    }

    #[test]
    fn a_diagonal_step_closes_the_wider_gap_and_ties_go_across() {
        let from = rect(0, 0, 10, 10);
        assert_eq!(
            step_toward(&from, &rect(20, 100, 10, 10)),
            Some(FocusDir::Down)
        );
        assert_eq!(
            step_toward(&rect(20, 100, 10, 10), &from),
            Some(FocusDir::Up)
        );
        assert_eq!(
            step_toward(&from, &rect(100, 20, 10, 10)),
            Some(FocusDir::Right)
        );
        assert_eq!(
            step_toward(&rect(100, 20, 10, 10), &from),
            Some(FocusDir::Left)
        );
        assert_eq!(
            step_toward(&from, &rect(20, 20, 10, 10)),
            Some(FocusDir::Right)
        );
    }
}
