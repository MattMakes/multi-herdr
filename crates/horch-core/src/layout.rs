//! Fleet grid analysis: report the worker grid and the next split that keeps it
//! 2 rows tall by N columns wide.
//!
//! Compares the top row's column boundaries against the bottom row's. The
//! orchestrator pane is excluded from the grid: it is the full-height pane at the
//! far left, or whatever the mailbox registered as `orchestrator`.
//!
//! Three states drive the suggestion:
//!   * `unpaired` - a worker owns a full-height column, so split DOWN to pair it.
//!   * `ragged`   - one row has a pane spanning a boundary the other row has, so
//!                  split RIGHT from that pane to subdivide it.
//!   * `complete` - every column is 2 tall, so split RIGHT from the top-rightmost
//!                  pane to start a new column, then RIGHT from the
//!                  bottom-rightmost to finish it (adding a column always takes
//!                  two splits).

use std::collections::HashMap;

use crate::herdr::{Direction, Layout, Rect};

/// A pane reduced to the geometry the grid analysis needs.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Slot {
    id: String,
    x: i64,
    y: i64,
    w: i64,
    h: i64,
}

impl Slot {
    /// True when this pane covers the whole column slice `[x0, x1]`.
    fn covers(&self, x0: i64, x1: i64) -> bool {
        self.x <= x0 && self.x + self.w >= x1
    }
}

/// One column slice of the grid, and what occupies each of its two rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cell {
    pub x0: i64,
    pub x1: i64,
    pub top: Option<String>,
    pub bottom: Option<String>,
    /// How many panes are stacked over this slice. 3+ means this is not a 2-row
    /// grid. Counting panes rather than comparing heights keeps this correct when
    /// a column was split with an uneven ratio.
    pub depth: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GridState {
    /// A column has 3+ panes stacked in it: not a 2-row grid at all.
    Offgrid,
    /// No worker panes yet.
    Empty,
    /// A worker owns a full-height column.
    Unpaired,
    RaggedBottom,
    RaggedTop,
    /// Every column is 2 tall. This is the shape you want.
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

    /// The one-line explanation printed under the grid summary.
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

/// The split to make next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NextSplit {
    pub direction: Direction,
    pub from: String,
    pub why: &'static str,
    /// Adding a whole column takes two splits; this is the second one.
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

/// Who the orchestrator is on the tab being analysed.
///
/// A fleet spreads over several tabs and only one of them holds the
/// orchestrator. On the others every pane is a worker, and a lone full-height
/// pane there is a worker column - so those tabs must be analysed with
/// [`Orchestrator::Absent`] rather than with the positional guess, which would
/// silently drop a worker from the grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Orchestrator<'a> {
    /// The pane id the mailbox registered for the role.
    Pane(&'a str),
    /// Guess: the leftmost full-height pane.
    Infer,
    /// This tab has none. Every pane is a worker.
    Absent,
}

/// Analyse a tab layout into a worker grid.
///
/// `orchestrator` is the pane id the mailbox registered for the orchestrator role.
/// When absent, the leftmost full-height pane is assumed to be it.
pub fn analyze(layout: &Layout, orchestrator: Option<&str>) -> Analysis {
    analyze_with(
        layout,
        match orchestrator.filter(|s| !s.is_empty()) {
            Some(id) => Orchestrator::Pane(id),
            None => Orchestrator::Infer,
        },
    )
}

/// Analyse a tab layout, saying explicitly whether the orchestrator is on it.
pub fn analyze_with(layout: &Layout, orchestrator: Orchestrator) -> Analysis {
    let area = layout.area;
    let height = area.height;
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

    // A column is "full" when it spans the tab height, "half" when it is one of
    // two stacked rows. The 1-cell tolerance absorbs border rows.
    let is_full = |s: &Slot| s.h >= height - 1;

    let orch_id: Option<String> = match orchestrator {
        Orchestrator::Pane(id) if !id.is_empty() => Some(id.to_string()),
        Orchestrator::Pane(_) | Orchestrator::Infer => {
            let mut full: Vec<&Slot> = all.iter().filter(|s| is_full(s)).collect();
            full.sort_by_key(|s| s.x);
            full.first().map(|s| s.id.clone())
        }
        Orchestrator::Absent => None,
    };

    let workers: Vec<&Slot> = all
        .iter()
        .filter(|s| Some(s.id.as_str()) != orch_id.as_deref())
        .collect();
    let full: Vec<&Slot> = workers.iter().copied().filter(|s| is_full(s)).collect();
    let half: Vec<&Slot> = workers.iter().copied().filter(|s| !is_full(s)).collect();
    let top: Vec<&Slot> = half.iter().copied().filter(|s| s.y == area.y).collect();
    let bottom: Vec<&Slot> = half.iter().copied().filter(|s| s.y > area.y).collect();

    // Column boundaries: every distinct pane edge across both rows.
    let mut edges: Vec<i64> = top
        .iter()
        .chain(bottom.iter())
        .flat_map(|s| [s.x, s.x + s.w])
        .collect();
    edges.sort_unstable();
    edges.dedup();

    let cells: Vec<Cell> = edges
        .windows(2)
        .map(|w| {
            let (x0, x1) = (w[0], w[1]);
            Cell {
                x0,
                x1,
                top: top.iter().find(|s| s.covers(x0, x1)).map(|s| s.id.clone()),
                bottom: bottom
                    .iter()
                    .find(|s| s.covers(x0, x1))
                    .map(|s| s.id.clone()),
                depth: half.iter().filter(|s| s.covers(x0, x1)).count(),
            }
        })
        .collect();

    let offgrid = cells.iter().any(|c| c.depth > 2);

    // A row is ragged where one of its panes swallows a boundary the other row
    // has. That pane is the one to split RIGHT.
    let ragged_in = |row: &[&Slot]| -> Option<String> {
        let mut spanning: Vec<&&Slot> = row
            .iter()
            .filter(|p| edges.iter().any(|e| *e > p.x && *e < p.x + p.w))
            .collect();
        spanning.sort_by_key(|p| p.x);
        spanning.first().map(|p| p.id.clone())
    };
    let top_ragged = ragged_in(&top);
    let bottom_ragged = ragged_in(&bottom);

    let rightmost =
        |row: &[&Slot]| -> Option<String> { row.iter().max_by_key(|s| s.x).map(|s| s.id.clone()) };

    let state = if offgrid {
        GridState::Offgrid
    } else if workers.is_empty() {
        GridState::Empty
    } else if !full.is_empty() {
        GridState::Unpaired
    } else if bottom_ragged.is_some() {
        GridState::RaggedBottom
    } else if top_ragged.is_some() {
        GridState::RaggedTop
    } else if top.len() == bottom.len() {
        GridState::Complete
    } else {
        GridState::Ragged
    };

    let next = match state {
        GridState::Unpaired => rightmost(&full).map(|from| NextSplit {
            direction: Direction::Down,
            from,
            why: "worker owns a full-height column; DOWN pairs it into a 2-tall column",
            followup: None,
        }),
        GridState::RaggedBottom => bottom_ragged.map(|from| NextSplit {
            direction: Direction::Right,
            from,
            why: "bottom pane spans a boundary the top row already has; RIGHT subdivides it",
            followup: None,
        }),
        GridState::RaggedTop => top_ragged.map(|from| NextSplit {
            direction: Direction::Right,
            from,
            why: "top pane spans a boundary the bottom row already has; RIGHT subdivides it",
            followup: None,
        }),
        GridState::Complete => rightmost(&top).map(|from| NextSplit {
            direction: Direction::Right,
            from,
            why: "grid is 2xN; RIGHT from the top-rightmost pane opens column N+1",
            followup: rightmost(&bottom),
        }),
        GridState::Empty => orch_id.clone().map(|from| NextSplit {
            direction: Direction::Right,
            from,
            why: "no workers yet; RIGHT off the orchestrator makes the first one",
            followup: None,
        }),
        GridState::Offgrid | GridState::Ragged => None,
    };

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

/// Render every tab of a workspace, one block per tab in tab-strip order.
///
/// The same per-tab renderer as [`render`]; what is new is that a fleet is no
/// longer assumed to fit in one tab.
///
/// `notes` is one line per tab, appended to its header. The states this renderer
/// prints describe one tab of a 2xN grid on its own, so an overflow tab whose last
/// bottom slot is still free reads as `ragged-bottom` when it is in fact exactly
/// right; the note is where `horch tile`'s verdict on the tab goes.
pub fn render_all(
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

/// Render an analysis the way `horch-layout` printed it.
pub fn render(analysis: &Analysis, roles: &HashMap<String, String>) -> String {
    let label = |id: Option<&str>| -> String {
        match id {
            None => "-".to_string(),
            Some(id) => match roles.get(id) {
                Some(role) => format!("{role}({id})"),
                None => id.to_string(),
            },
        }
    };

    let a = analysis;
    let mut out = String::new();
    out.push_str(&format!(
        "workspace {}  tab {}  area {}x{}\n",
        a.workspace_id, a.tab_id, a.area.width, a.area.height
    ));
    match &a.orchestrator {
        Some(id) => out.push_str(&format!(
            "orchestrator: {} (excluded from the grid)\n",
            label(Some(id))
        )),
        None => out.push_str("orchestrator: none detected\n"),
    }
    out.push('\n');

    if !a.cells.is_empty() {
        let mut hdr = " ".repeat(10);
        let mut top_line = "  top     ".to_string();
        let mut bot_line = "  bottom  ".to_string();
        let mut spans = false;

        for cell in &a.cells {
            let mut tl = label(cell.top.as_deref());
            let mut bl = label(cell.bottom.as_deref());
            // Mark a cell whose pane also covers the neighbouring column.
            let shared = |id: &Option<String>, pick: fn(&Cell) -> &Option<String>| -> bool {
                id.is_some() && a.cells.iter().filter(|c| pick(c) == id).count() > 1
            };
            if shared(&cell.top, |c| &c.top) {
                tl.push('*');
                spans = true;
            }
            if shared(&cell.bottom, |c| &c.bottom) {
                bl.push('*');
                spans = true;
            }
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

        out.push_str(&format!("{hdr}\n{top_line}\n{bot_line}\n"));
        if spans {
            out.push_str("  (* pane spans more than one column)\n");
        }
        out.push('\n');
    }

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

    let Some(next) = &a.next else {
        out.push_str("no suggestion for this shape.\n");
        return out;
    };

    let verb = match next.direction {
        Direction::Down => "VERTICAL",
        Direction::Right => "HORIZONTAL",
    };
    out.push_str(&format!(
        "next split: {verb} ({}) from {}\n",
        next.direction.as_str(),
        label(Some(&next.from))
    ));
    out.push_str(&format!("  why: {}\n\n", next.why));
    out.push_str(&format!(
        "  horch spawn <teammate> --from-pane {} --direction {}\n",
        next.from,
        next.direction.as_str()
    ));
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
    use crate::herdr::LayoutPane;

    /// Build a layout from `(id, x, y, w, h)` tuples in a 200x50 tab.
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

    /// Orchestrator alone: no workers yet, so the first split goes right off it.
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

    /// One full-height worker column wants a DOWN split to pair it.
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

    /// The canonical 2x2: two stacked rows over two equal columns.
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

    /// Top row subdivided further than the bottom: the bottom pane swallowing
    /// the extra boundary is the one to split.
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

    /// Three panes stacked in one column is not a 2-row grid; refuse to advise.
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

    /// With no registered orchestrator, the leftmost full-height pane is assumed
    /// to be it - and is excluded from the grid.
    #[test]
    fn orchestrator_is_inferred_as_the_leftmost_full_height_pane() {
        let l = layout(&[("right-full", 100, 0, 100, 50), ("orch", 0, 0, 100, 50)]);
        let a = analyze(&l, None);
        assert_eq!(a.orchestrator.as_deref(), Some("orch"));
        assert_eq!(a.state, GridState::Unpaired);
        assert_eq!(a.next.unwrap().from, "right-full");
    }

    /// A registered orchestrator wins over the leftmost-full-height guess, even
    /// when it is not leftmost.
    #[test]
    fn registered_orchestrator_overrides_the_positional_guess() {
        let l = layout(&[("orch", 100, 0, 100, 50), ("worker", 0, 0, 100, 50)]);
        let a = analyze(&l, Some("orch"));
        assert_eq!(a.orchestrator.as_deref(), Some("orch"));
        assert_eq!(a.next.unwrap().from, "worker");
    }

    /// Uneven ratios must not be mistaken for a third row: depth counts panes.
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
        // `wide` covers both columns, so it is starred and the legend appears.
        assert!(out.contains("opus-1(wide)*"), "{out}");
        assert!(out.contains("(* pane spans more than one column)"));
        // Unregistered panes fall back to the bare id.
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

    /// An overflow tab holds no orchestrator, so a lone full-height pane there is
    /// a worker column. Inferring an orchestrator would drop it from the grid and
    /// report an empty tab.
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

    /// `analyze` keeps its old contract: a registered id wins, and no id means
    /// guess.
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

    /// One block per tab, in order, each one the block a single tab printed
    /// before.
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

        // A note per tab rides on the header rather than changing the block.
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
}
