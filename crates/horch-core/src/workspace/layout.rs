use std::collections::HashMap;

use crate::workspace::model::{Direction, Layout, Rect};

#[derive(Debug, Clone, PartialEq, Eq)]
struct Slot {
    id: String,
    x: i64,
    y: i64,
    w: i64,
    h: i64,
}

impl Slot {
    fn covers(&self, x0: i64, x1: i64) -> bool {
        self.x <= x0 && self.x + self.w >= x1
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cell {
    pub x0: i64,
    pub x1: i64,
    pub top: Option<String>,
    pub bottom: Option<String>,
    pub depth: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GridState {
    Offgrid,
    Empty,
    Unpaired,
    RaggedBottom,
    RaggedTop,
    Complete,
    Ragged,
}

impl GridState {
    pub fn as_str(self) -> &'static str {
        match self {
            GridState::Offgrid => "offgrid",
            GridState::Empty => "empty",
            GridState::Unpaired => "unpaired",
            GridState::RaggedBottom => "ragged-bottom",
            GridState::RaggedTop => "ragged-top",
            GridState::Complete => "complete",
            GridState::Ragged => "ragged",
        }
    }

    fn explanation(self) -> Option<&'static str> {
        match self {
            GridState::Complete => Some("every column is 2 tall - this is the shape you want."),
            GridState::Unpaired => Some("a worker column is 1 tall."),
            GridState::RaggedBottom | GridState::RaggedTop | GridState::Ragged => {
                Some("one row is subdivided further than the other.")
            }
            GridState::Offgrid => Some(
                "a column has 3+ panes stacked in it, so this tab is not a 2-row \
                 grid. Close the extra pane(s) or start a fresh workspace.",
            ),
            GridState::Empty => Some("no worker panes yet."),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NextSplit {
    pub direction: Direction,
    pub from: String,
    pub why: &'static str,
    pub followup: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Analysis {
    pub workspace_id: String,
    pub tab_id: String,
    pub area: Rect,
    pub orchestrator: Option<String>,
    pub columns: usize,
    pub rows_top: usize,
    pub rows_bottom: usize,
    pub state: GridState,
    pub cells: Vec<Cell>,
    pub next: Option<NextSplit>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Orchestrator<'a> {
    Pane(&'a str),
    Infer,
    Absent,
}

#[cfg(test)]
pub(crate) fn analyze(layout: &Layout, orchestrator: Option<&str>) -> Analysis {
    analyze_with(
        layout,
        match orchestrator.filter(|s| !s.is_empty()) {
            Some(id) => Orchestrator::Pane(id),
            None => Orchestrator::Infer,
        },
    )
}

pub(crate) fn analyze_with(layout: &Layout, orchestrator: Orchestrator) -> Analysis {
    let area = layout.area;
    let all: Vec<Slot> = layout
        .panes
        .iter()
        .map(|p| Slot {
            id: p.pane_id.clone(),
            x: p.rect.x,
            y: p.rect.y,
            w: p.rect.width,
            h: p.rect.height,
        })
        .collect();

    let is_full = |s: &Slot| s.h >= area.height - 1;

    let orch_id = pick_orchestrator(&all, orchestrator, area.height);

    let workers: Vec<&Slot> = all
        .iter()
        .filter(|s| Some(s.id.as_str()) != orch_id.as_deref())
        .collect();
    let full: Vec<&Slot> = workers.iter().copied().filter(|s| is_full(s)).collect();
    let half: Vec<&Slot> = workers.iter().copied().filter(|s| !is_full(s)).collect();
    let top: Vec<&Slot> = half.iter().copied().filter(|s| s.y == area.y).collect();
    let bottom: Vec<&Slot> = half.iter().copied().filter(|s| s.y > area.y).collect();

    let edges = column_edges(&top, &bottom);
    let cells = cells(&edges, &top, &bottom, &half);
    let top_ragged = ragged_in(&edges, &top);
    let bottom_ragged = ragged_in(&edges, &bottom);
    let state = grid_state(&Shape {
        offgrid: cells.iter().any(|c| c.depth > 2),
        workers: workers.len(),
        full: full.len(),
        top_ragged: top_ragged.is_some(),
        bottom_ragged: bottom_ragged.is_some(),
        top: top.len(),
        bottom: bottom.len(),
    });
    let next = next_split(
        state,
        &Rows {
            full: &full,
            top: &top,
            bottom: &bottom,
            top_ragged,
            bottom_ragged,
            orchestrator: orch_id.clone(),
        },
    );

    Analysis {
        workspace_id: layout.workspace_id.clone(),
        tab_id: layout.tab_id.clone(),
        area,
        orchestrator: orch_id,
        columns: cells.len() + full.len(),
        rows_top: top.len(),
        rows_bottom: bottom.len(),
        state,
        cells,
        next,
    }
}

fn pick_orchestrator(all: &[Slot], orchestrator: Orchestrator, height: i64) -> Option<String> {
    match orchestrator {
        Orchestrator::Pane(id) if !id.is_empty() => Some(id.to_string()),
        Orchestrator::Pane(_) | Orchestrator::Infer => all
            .iter()
            .filter(|s| s.h >= height - 1)
            .min_by_key(|s| s.x)
            .map(|s| s.id.clone()),
        Orchestrator::Absent => None,
    }
}

fn column_edges(top: &[&Slot], bottom: &[&Slot]) -> Vec<i64> {
    let mut edges: Vec<i64> = top
        .iter()
        .chain(bottom.iter())
        .flat_map(|s| [s.x, s.x + s.w])
        .collect();
    edges.sort_unstable();
    edges.dedup();
    edges
}

fn cells(edges: &[i64], top: &[&Slot], bottom: &[&Slot], half: &[&Slot]) -> Vec<Cell> {
    let occupant =
        |row: &[&Slot], x0, x1| row.iter().find(|s| s.covers(x0, x1)).map(|s| s.id.clone());
    edges
        .windows(2)
        .map(|w| {
            let (x0, x1) = (w[0], w[1]);
            Cell {
                x0,
                x1,
                top: occupant(top, x0, x1),
                bottom: occupant(bottom, x0, x1),
                depth: half.iter().filter(|s| s.covers(x0, x1)).count(),
            }
        })
        .collect()
}

fn ragged_in(edges: &[i64], row: &[&Slot]) -> Option<String> {
    row.iter()
        .filter(|p| edges.iter().any(|e| *e > p.x && *e < p.x + p.w))
        .min_by_key(|p| p.x)
        .map(|p| p.id.clone())
}

fn rightmost(row: &[&Slot]) -> Option<String> {
    row.iter().max_by_key(|s| s.x).map(|s| s.id.clone())
}

struct Shape {
    offgrid: bool,
    workers: usize,
    full: usize,
    top_ragged: bool,
    bottom_ragged: bool,
    top: usize,
    bottom: usize,
}

fn grid_state(shape: &Shape) -> GridState {
    if shape.offgrid {
        GridState::Offgrid
    } else if shape.workers == 0 {
        GridState::Empty
    } else if shape.full > 0 {
        GridState::Unpaired
    } else if shape.bottom_ragged {
        GridState::RaggedBottom
    } else if shape.top_ragged {
        GridState::RaggedTop
    } else if shape.top == shape.bottom {
        GridState::Complete
    } else {
        GridState::Ragged
    }
}

struct Rows<'a> {
    full: &'a [&'a Slot],
    top: &'a [&'a Slot],
    bottom: &'a [&'a Slot],
    top_ragged: Option<String>,
    bottom_ragged: Option<String>,
    orchestrator: Option<String>,
}

fn next_split(state: GridState, rows: &Rows) -> Option<NextSplit> {
    let (from, direction, why, followup) = match state {
        GridState::Unpaired => (
            rightmost(rows.full),
            Direction::Down,
            "worker owns a full-height column; DOWN pairs it into a 2-tall column",
            None,
        ),
        GridState::RaggedBottom => (
            rows.bottom_ragged.clone(),
            Direction::Right,
            "bottom pane spans a boundary the top row already has; RIGHT subdivides it",
            None,
        ),
        GridState::RaggedTop => (
            rows.top_ragged.clone(),
            Direction::Right,
            "top pane spans a boundary the bottom row already has; RIGHT subdivides it",
            None,
        ),
        GridState::Complete => (
            rightmost(rows.top),
            Direction::Right,
            "grid is 2xN; RIGHT from the top-rightmost pane opens column N+1",
            rightmost(rows.bottom),
        ),
        GridState::Empty => (
            rows.orchestrator.clone(),
            Direction::Right,
            "no workers yet; RIGHT off the orchestrator makes the first one",
            None,
        ),
        GridState::Offgrid | GridState::Ragged => return None,
    };
    from.map(|from| NextSplit {
        direction,
        from,
        why,
        followup,
    })
}

pub(crate) fn render_all(
    analyses: &[Analysis],
    notes: &[String],
    roles: &HashMap<String, String>,
) -> String {
    if analyses.is_empty() {
        return "no tabs in this workspace\n".to_string();
    }
    let total = analyses.len();
    analyses
        .iter()
        .enumerate()
        .map(|(i, a)| {
            let note = match notes.get(i) {
                Some(n) if !n.is_empty() => format!("  {n}"),
                _ => String::new(),
            };
            format!(
                "=== tab {} of {total}{note} ===\n{}",
                i + 1,
                render(a, roles)
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn render(analysis: &Analysis, roles: &HashMap<String, String>) -> String {
    let a = analysis;
    let mut out = format!(
        "workspace {}  tab {}  area {}x{}\n",
        a.workspace_id, a.tab_id, a.area.width, a.area.height
    );
    match &a.orchestrator {
        Some(id) => out.push_str(&format!(
            "orchestrator: {} (excluded from the grid)\n",
            role_label(roles, Some(id))
        )),
        None => out.push_str("orchestrator: none detected\n"),
    }
    out.push('\n');
    out.push_str(&render_cells(a, roles));
    out.push_str(&format!(
        "grid: {} column(s), top row {}, bottom row {}  ->  {}\n",
        a.columns,
        a.rows_top,
        a.rows_bottom,
        a.state.as_str()
    ));
    if let Some(explanation) = a.state.explanation() {
        out.push_str(&format!("        {explanation}\n"));
    }
    out.push('\n');
    out.push_str(&render_next(a.next.as_ref(), roles));
    out
}

fn role_label(roles: &HashMap<String, String>, id: Option<&str>) -> String {
    match id {
        None => "-".to_string(),
        Some(id) => match roles.get(id) {
            Some(role) => format!("{role}({id})"),
            None => id.to_string(),
        },
    }
}

fn marked(label: String, shared: bool) -> String {
    if shared {
        label + "*"
    } else {
        label
    }
}

fn spans_columns(cells: &[Cell], id: &Option<String>, pick: fn(&Cell) -> &Option<String>) -> bool {
    id.is_some() && cells.iter().filter(|c| pick(c) == id).count() > 1
}

fn render_cells(a: &Analysis, roles: &HashMap<String, String>) -> String {
    if a.cells.is_empty() {
        return String::new();
    }
    let mut hdr = " ".repeat(10);
    let mut top_line = "  top     ".to_string();
    let mut bot_line = "  bottom  ".to_string();
    let mut spans = false;

    for cell in &a.cells {
        let top_shared = spans_columns(&a.cells, &cell.top, |c| &c.top);
        let bottom_shared = spans_columns(&a.cells, &cell.bottom, |c| &c.bottom);
        spans |= top_shared || bottom_shared;
        let tl = marked(role_label(roles, cell.top.as_deref()), top_shared);
        let bl = marked(role_label(roles, cell.bottom.as_deref()), bottom_shared);
        let head = format!("{}-{}", cell.x0, cell.x1);
        let w = tl
            .chars()
            .count()
            .max(bl.chars().count())
            .max(head.chars().count());
        hdr.push_str(&format!("{head:<w$}  "));
        top_line.push_str(&format!("{tl:<w$}  "));
        bot_line.push_str(&format!("{bl:<w$}  "));
    }

    let legend = if spans {
        "  (* pane spans more than one column)\n"
    } else {
        ""
    };
    format!("{hdr}\n{top_line}\n{bot_line}\n{legend}\n")
}

fn render_next(next: Option<&NextSplit>, roles: &HashMap<String, String>) -> String {
    let Some(next) = next else {
        return "no suggestion for this shape.\n".to_string();
    };
    let verb = match next.direction {
        Direction::Down => "VERTICAL",
        Direction::Right => "HORIZONTAL",
    };
    let mut out = format!(
        "next split: {verb} ({}) from {}\n  why: {}\n\n  horch spawn <teammate> --from-pane {} --direction {}\n",
        next.direction.as_str(),
        role_label(roles, Some(&next.from)),
        next.why,
        next.from,
        next.direction.as_str()
    );
    if let Some(followup) = &next.followup {
        out.push_str("\n  then, to finish the new column (2 splits per column):\n");
        out.push_str(&format!(
            "  horch spawn <teammate> --from-pane {followup} --direction right\n"
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::model::LayoutPane;

    fn layout(panes: &[(&str, i64, i64, i64, i64)]) -> Layout {
        Layout {
            workspace_id: "w1".into(),
            tab_id: "t1".into(),
            area: Rect {
                x: 0,
                y: 0,
                width: 200,
                height: 50,
            },
            panes: panes
                .iter()
                .map(|(id, x, y, w, h)| LayoutPane {
                    pane_id: (*id).into(),
                    rect: Rect {
                        x: *x,
                        y: *y,
                        width: *w,
                        height: *h,
                    },
                })
                .collect(),
            focused_pane_id: None,
            zoomed: false,
        }
    }

    #[test]
    fn empty_grid_suggests_splitting_off_the_orchestrator() {
        let l = layout(&[("orch", 0, 0, 200, 50)]);
        let a = analyze(&l, Some("orch"));
        assert_eq!(a.state, GridState::Empty);
        assert_eq!(a.columns, 0);
        let next = a.next.unwrap();
        assert_eq!(next.direction, Direction::Right);
        assert_eq!(next.from, "orch");
        assert!(next.followup.is_none());
    }

    #[test]
    fn a_full_height_worker_is_unpaired() {
        let l = layout(&[("orch", 0, 0, 100, 50), ("w1", 100, 0, 100, 50)]);
        let a = analyze(&l, Some("orch"));
        assert_eq!(a.state, GridState::Unpaired);
        assert_eq!(a.columns, 1);
        let next = a.next.unwrap();
        assert_eq!(next.direction, Direction::Down);
        assert_eq!(next.from, "w1");
    }

    #[test]
    fn a_2x2_grid_is_complete_and_needs_two_splits_for_a_new_column() {
        let l = layout(&[
            ("orch", 0, 0, 60, 50),
            ("tl", 60, 0, 70, 25),
            ("bl", 60, 25, 70, 25),
            ("tr", 130, 0, 70, 25),
            ("br", 130, 25, 70, 25),
        ]);
        let a = analyze(&l, Some("orch"));
        assert_eq!(a.state, GridState::Complete);
        assert_eq!(a.columns, 2);
        assert_eq!((a.rows_top, a.rows_bottom), (2, 2));

        let next = a.next.unwrap();
        assert_eq!(next.direction, Direction::Right);
        assert_eq!(next.from, "tr", "must split off the top-rightmost pane");
        assert_eq!(next.followup.as_deref(), Some("br"));
    }

    #[test]
    fn bottom_row_spanning_an_extra_boundary_is_ragged_bottom() {
        let l = layout(&[
            ("orch", 0, 0, 60, 50),
            ("tl", 60, 0, 70, 25),
            ("tr", 130, 0, 70, 25),
            ("wide", 60, 25, 140, 25),
        ]);
        let a = analyze(&l, Some("orch"));
        assert_eq!(a.state, GridState::RaggedBottom);
        let next = a.next.unwrap();
        assert_eq!(next.direction, Direction::Right);
        assert_eq!(next.from, "wide");
    }

    #[test]
    fn top_row_spanning_an_extra_boundary_is_ragged_top() {
        let l = layout(&[
            ("orch", 0, 0, 60, 50),
            ("wide", 60, 0, 140, 25),
            ("bl", 60, 25, 70, 25),
            ("br", 130, 25, 70, 25),
        ]);
        let a = analyze(&l, Some("orch"));
        assert_eq!(a.state, GridState::RaggedTop);
        assert_eq!(a.next.unwrap().from, "wide");
    }

    #[test]
    fn three_stacked_panes_are_offgrid_with_no_suggestion() {
        let l = layout(&[
            ("orch", 0, 0, 60, 50),
            ("a", 60, 0, 140, 17),
            ("b", 60, 17, 140, 17),
            ("c", 60, 34, 140, 16),
        ]);
        let a = analyze(&l, Some("orch"));
        assert_eq!(a.state, GridState::Offgrid);
        assert!(a.next.is_none());
        assert!(render(&a, &HashMap::new()).contains("no suggestion for this shape."));
    }

    #[test]
    fn orchestrator_is_inferred_as_the_leftmost_full_height_pane() {
        let l = layout(&[("right-full", 100, 0, 100, 50), ("orch", 0, 0, 100, 50)]);
        let a = analyze(&l, None);
        assert_eq!(a.orchestrator.as_deref(), Some("orch"));
        assert_eq!(a.state, GridState::Unpaired);
        assert_eq!(a.next.unwrap().from, "right-full");
    }

    #[test]
    fn registered_orchestrator_overrides_the_positional_guess() {
        let l = layout(&[("orch", 100, 0, 100, 50), ("worker", 0, 0, 100, 50)]);
        let a = analyze(&l, Some("orch"));
        assert_eq!(a.orchestrator.as_deref(), Some("orch"));
        assert_eq!(a.next.unwrap().from, "worker");
    }

    #[test]
    fn uneven_split_ratios_still_read_as_two_rows() {
        let l = layout(&[
            ("orch", 0, 0, 60, 50),
            ("tall", 60, 0, 140, 40),
            ("short", 60, 40, 140, 10),
        ]);
        let a = analyze(&l, Some("orch"));
        assert_eq!(a.state, GridState::Complete);
        assert_eq!(a.cells.len(), 1);
        assert_eq!(a.cells[0].depth, 2);
    }

    #[test]
    fn render_labels_panes_with_their_roles_and_marks_spanning_cells() {
        let l = layout(&[
            ("orch", 0, 0, 60, 50),
            ("tl", 60, 0, 70, 25),
            ("tr", 130, 0, 70, 25),
            ("wide", 60, 25, 140, 25),
        ]);
        let a = analyze(&l, Some("orch"));
        let roles = HashMap::from([
            ("orch".to_string(), "orchestrator".to_string()),
            ("tl".to_string(), "sonnet-1".to_string()),
            ("wide".to_string(), "opus-1".to_string()),
        ]);
        let out = render(&a, &roles);

        assert!(out.contains("orchestrator: orchestrator(orch) (excluded from the grid)"));
        assert!(out.contains("sonnet-1(tl)"));
        assert!(out.contains("opus-1(wide)*"), "{out}");
        assert!(out.contains("(* pane spans more than one column)"));
        assert!(out.contains("tr"));
        assert!(out.contains("horch spawn <teammate> --from-pane wide --direction right"));
    }

    #[test]
    fn render_shows_both_splits_for_a_complete_grid() {
        let l = layout(&[
            ("orch", 0, 0, 60, 50),
            ("tl", 60, 0, 70, 25),
            ("bl", 60, 25, 70, 25),
            ("tr", 130, 0, 70, 25),
            ("br", 130, 25, 70, 25),
        ]);
        let out = render(&analyze(&l, Some("orch")), &HashMap::new());
        assert!(out.contains("next split: HORIZONTAL (right) from tr"));
        assert!(out.contains("then, to finish the new column (2 splits per column):"));
        assert!(out.contains("--from-pane br --direction right"));
    }

    #[test]
    fn a_worker_tab_keeps_its_lone_full_height_pane_as_a_worker() {
        let l = layout(&[("w", 0, 0, 200, 50)]);
        let inferred = analyze_with(&l, Orchestrator::Infer);
        assert_eq!(inferred.orchestrator.as_deref(), Some("w"));
        assert_eq!(inferred.state, GridState::Empty);

        let a = analyze_with(&l, Orchestrator::Absent);
        assert_eq!(a.orchestrator, None);
        assert_eq!(a.state, GridState::Unpaired, "it is a worker column");
        assert_eq!(a.columns, 1);
    }

    #[test]
    fn analyze_still_means_infer_when_no_orchestrator_is_registered() {
        let l = layout(&[
            ("orch", 0, 0, 50, 50),
            ("a", 50, 0, 150, 25),
            ("b", 50, 25, 150, 25),
        ]);
        assert_eq!(
            analyze(&l, None).orchestrator.as_deref(),
            analyze_with(&l, Orchestrator::Infer)
                .orchestrator
                .as_deref()
        );
        assert_eq!(analyze(&l, Some("a")).orchestrator.as_deref(), Some("a"));
        assert_eq!(
            analyze(&l, Some("")).orchestrator.as_deref(),
            Some("orch"),
            "an empty registration falls back to the guess"
        );
    }

    #[test]
    fn render_all_prints_one_block_per_tab_in_order() {
        let roles = HashMap::new();
        let tab1 = analyze(
            &layout(&[("orch", 0, 0, 50, 50), ("a", 50, 0, 150, 50)]),
            Some("orch"),
        );
        let tab2 = analyze_with(
            &layout(&[
                ("b", 0, 0, 100, 25),
                ("c", 100, 0, 100, 25),
                ("d", 0, 25, 200, 25),
            ]),
            Orchestrator::Absent,
        );
        let out = render_all(&[tab1.clone(), tab2.clone()], &[], &roles);
        assert!(out.starts_with("=== tab 1 of 2 ===\n"), "{out}");
        assert!(out.contains("=== tab 2 of 2 ===\n"), "{out}");
        assert!(
            out.contains(&render(&tab1, &roles)),
            "the same renderer per tab"
        );
        assert!(
            out.contains(&render(&tab2, &roles)),
            "the same renderer per tab"
        );
        assert_eq!(render_all(&[], &[], &roles), "no tabs in this workspace\n");

        let noted = render_all(
            &[tab1, tab2],
            &["matches the fleet grid".to_string(), String::new()],
            &roles,
        );
        assert!(
            noted.starts_with("=== tab 1 of 2  matches the fleet grid ===\n"),
            "{noted}"
        );
        assert!(
            noted.contains("=== tab 2 of 2 ===\n"),
            "an empty note adds nothing"
        );
    }

    #[test]
    fn render_names_a_down_split_vertical() {
        let l = layout(&[("orch", 0, 0, 100, 50), ("w1", 100, 0, 100, 50)]);
        let out = render(&analyze(&l, Some("orch")), &HashMap::new());
        assert!(out.contains("next split: VERTICAL (down) from w1"), "{out}");
    }

    #[test]
    fn render_prints_the_whole_report_of_a_ragged_bottom_tab() {
        let l = layout(&[
            ("orch", 0, 0, 60, 50),
            ("tl", 60, 0, 70, 25),
            ("tr", 130, 0, 70, 25),
            ("wide", 60, 25, 140, 25),
        ]);
        let roles = HashMap::from([("tl".to_string(), "sonnet-1".to_string())]);
        assert_eq!(
            render(&analyze(&l, Some("orch")), &roles),
            "workspace w1  tab t1  area 200x50\n\
             orchestrator: orch (excluded from the grid)\n\
             \n\
             \x20         60-130        130-200  \n\
             \x20 top     sonnet-1(tl)  tr       \n\
             \x20 bottom  wide*         wide*    \n\
             \x20 (* pane spans more than one column)\n\
             \n\
             grid: 2 column(s), top row 2, bottom row 1  ->  ragged-bottom\n\
             \x20       one row is subdivided further than the other.\n\
             \n\
             next split: HORIZONTAL (right) from wide\n\
             \x20 why: bottom pane spans a boundary the top row already has; RIGHT subdivides it\n\
             \n\
             \x20 horch spawn <teammate> --from-pane wide --direction right\n"
        );
    }

    #[test]
    fn rows_of_unequal_length_with_no_spanning_pane_are_ragged_without_advice() {
        let l = layout(&[
            ("orch", 0, 0, 60, 50),
            ("t1", 60, 0, 70, 25),
            ("t2", 130, 0, 70, 25),
            ("b1", 60, 25, 70, 25),
        ]);
        let a = analyze(&l, Some("orch"));
        assert_eq!(a.state, GridState::Ragged);
        assert!(a.next.is_none());
        let out = render(&a, &HashMap::new());
        assert!(out.contains("->  ragged\n"), "{out}");
        assert!(out.ends_with("no suggestion for this shape.\n"), "{out}");
    }

    #[test]
    fn a_tab_with_no_orchestrator_and_no_panes_is_empty_without_advice() {
        let a = analyze_with(&layout(&[]), Orchestrator::Absent);
        assert_eq!(a.state, GridState::Empty);
        assert!(a.orchestrator.is_none());
        assert!(a.next.is_none());
        let out = render(&a, &HashMap::new());
        assert!(out.contains("orchestrator: none detected\n"), "{out}");
        assert!(out.contains("        no worker panes yet.\n"), "{out}");
        assert!(
            !out.contains("  top     "),
            "no cell table without cells: {out}"
        );
    }

    #[test]
    fn the_offgrid_explanation_asks_to_close_the_extra_panes() {
        let l = layout(&[
            ("orch", 0, 0, 60, 50),
            ("a", 60, 0, 140, 17),
            ("b", 60, 17, 140, 17),
            ("c", 60, 34, 140, 16),
        ]);
        let out = render(&analyze(&l, Some("orch")), &HashMap::new());
        assert!(
            out.contains(
                "        a column has 3+ panes stacked in it, so this tab is not a 2-row grid. \
                 Close the extra pane(s) or start a fresh workspace.\n"
            ),
            "{out}"
        );
    }
}
