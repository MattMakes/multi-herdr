//! The screen's view state, its keys, and the frame (section 12.4).
//!
//! [`draw`] is a pure function of a snapshot and a [`ViewState`]; [`on_key`]
//! and [`on_wheel`] are pure functions of a state and an input. [`render`]
//! draws 1 frame into a `ratatui` `TestBackend` and returns its text, which
//! is what the goldens compare and what `horch telemetry render` prints.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use horch_core::clock;
use horch_core::routing::quota;
use horch_core::telemetry::collect::{LiveRow, Snapshot};
use horch_core::telemetry::store::{RollupRow, GROUPS};
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::{Frame, Terminal};

use super::{
    clip, human, money, pad, pct, pool_table, project_name, short_model, short_phase,
    unpriced_line, VIEW_WINDOWS,
};

/// The live panes table.
pub const LIVE: usize = 0;
/// The rollup table (not in the `live` window).
pub const ROLLUP: usize = 1;

/// The rows the mouse wheel scrolls per step.
const WHEEL_ROWS: usize = 3;

const FOOTER: &str = "Tab focus  ↑↓ jk move  PgUp PgDn  Home End  g group  w window  p project  r refresh  ? help  q quit";

const HELP: &[&str] = &[
    "Tab  Shift-Tab    focus the next or previous table",
    "↓ j  ↑ k          move the selection 1 row",
    "PgDn  PgUp        move 1 page",
    "Home  End         the first or the last row",
    "mouse wheel       scroll the table under the pointer",
    "g                 cycle the grouping:",
    "                  teammate, phase, agent, project, plan, kind",
    "w                 cycle the window",
    "p                 cycle the project filter",
    "Esc               close this help, else clear the project filter",
    "r                 refresh now",
    "?                 show or hide this help",
    "q  Ctrl-C         quit",
    "",
    "windows:",
    "live    the live panes only ($* counts the last 5h's unpriced)",
    "5h      the live panes, and the rollup of the last 5 hours",
    "today   the live panes, and the rollup since 00:00 UTC",
    "7d      the live panes, and the rollup of the last 7 days",
];

/// The scroll and selection of 1 table.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TableState {
    /// The selected row.
    pub sel: usize,
    /// The first row on the screen.
    pub offset: usize,
    /// The selected row's key, so that the selection follows its row when
    /// the rows change. `None` after a move: `sel` is then the truth.
    pub key: Option<String>,
}

impl TableState {
    /// Select row `sel` (clamped) and scroll so that it shows.
    fn select(&mut self, sel: usize, n: usize, page: usize) {
        self.sel = sel.min(n.saturating_sub(1));
        self.key = None;
        self.fit(n, page);
    }

    /// Scroll so that the selection shows, and no page shows past the end.
    fn fit(&mut self, n: usize, page: usize) {
        let page = page.max(1);
        if self.sel < self.offset {
            self.offset = self.sel;
        }
        if self.sel >= self.offset + page {
            self.offset = self.sel + 1 - page;
        }
        self.offset = self.offset.min(n.saturating_sub(page));
    }

    /// After new rows: keep the selection on its key, else clamp it.
    pub fn sync(&mut self, keys: &[String], page: usize) {
        if let Some(i) = self
            .key
            .as_ref()
            .and_then(|k| keys.iter().position(|x| x == k))
        {
            self.sel = i;
        }
        self.sel = self.sel.min(keys.len().saturating_sub(1));
        self.fit(keys.len(), page);
        self.key = keys.get(self.sel).cloned();
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ViewState {
    /// Index into [`GROUPS`].
    pub group: usize,
    /// Index into [`VIEW_WINDOWS`].
    pub window: usize,
    /// Only this project's rows, when set.
    pub project: Option<String>,
    /// One line of status (an error from the last tick).
    pub status: Option<String>,
    /// `collector` or `viewer`, on a live screen.
    pub mode: Option<&'static str>,
    /// The table that the keys move: [`LIVE`] or [`ROLLUP`].
    pub focus: usize,
    /// The scroll and selection of [`LIVE`] and [`ROLLUP`].
    pub tables: [TableState; 2],
    /// The help popup shows.
    pub help: bool,
    /// Draw colors (not when `NO_COLOR` is set).
    pub color: bool,
}

impl ViewState {
    pub fn group_name(&self) -> &'static str {
        GROUPS[self.group % GROUPS.len()]
    }

    pub fn window_name(&self) -> &'static str {
        VIEW_WINDOWS[self.window % VIEW_WINDOWS.len()]
    }

    pub fn with(group: &str, window: &str) -> Result<ViewState> {
        let g = GROUPS
            .iter()
            .position(|x| *x == group)
            .with_context(|| format!("--group takes one of: {}", GROUPS.join(", ")))?;
        let w = VIEW_WINDOWS
            .iter()
            .position(|x| *x == window)
            .with_context(|| format!("--window takes one of: {}", VIEW_WINDOWS.join(", ")))?;
        Ok(ViewState {
            group: g,
            window: w,
            ..ViewState::default()
        })
    }

    /// The window shows a rollup table.
    pub fn has_rollup(&self) -> bool {
        self.window_name() != "live"
    }

    /// After new rows: each table keeps its selected key, or clamps.
    pub fn sync(&mut self, rows: &Rows) {
        for t in [LIVE, ROLLUP] {
            self.tables[t].sync(&rows.keys[t], rows.page[t]);
        }
        if !self.has_rollup() {
            self.focus = LIVE;
        }
    }
}

/// What the screen loop does after an input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    None,
    Redraw,
    Refresh,
    SetProject(Option<String>),
    Quit,
}

/// What [`on_key`] needs to know of the frame: each table's row keys and
/// page height, and the projects `p` cycles through.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Rows {
    pub keys: [Vec<String>; 2],
    pub page: [usize; 2],
    pub projects: Vec<String>,
}

impl Rows {
    pub fn of(snap: &Snapshot, view: &ViewState, area: Rect) -> Rows {
        let a = areas(snap, view, area);
        let live = live_rows(snap, view);
        let rollup = rollup_rows(snap, view);
        Rows {
            keys: [
                live.iter().map(|r| live_key(r)).collect(),
                rollup.iter().map(|r| r.key.clone()).collect(),
            ],
            page: [page(a.live), a.rollup.map(page).unwrap_or(1)],
            projects: projects(snap),
        }
    }

    fn count(&self, t: usize) -> usize {
        self.keys[t].len()
    }
}

/// The key that a live row keeps its selection by: the pane's record.
fn live_key(r: &LiveRow) -> String {
    if r.record_id.is_empty() {
        format!("{}\t{}", r.project.as_deref().unwrap_or("-"), r.role)
    } else {
        r.record_id.clone()
    }
}

/// The rows a table area shows: less its title line and its column header.
fn page(r: Rect) -> usize {
    (r.height as usize).saturating_sub(2).max(1)
}

/// A key on the screen. Pure.
pub fn on_key(mut s: ViewState, key: KeyEvent, rows: &Rows) -> (ViewState, Action) {
    if key.kind != KeyEventKind::Press {
        return (s, Action::None);
    }
    let before = s.clone();
    let t = s.focus;
    let (n, pg) = (rows.count(t), rows.page[t]);
    let action = match key.code {
        KeyCode::Char('q') => Action::Quit,
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => Action::Quit,
        KeyCode::Char('?') => {
            s.help = !s.help;
            Action::Redraw
        }
        KeyCode::Esc if s.help => {
            s.help = false;
            Action::Redraw
        }
        KeyCode::Esc if s.project.is_some() => {
            s.project = None;
            Action::SetProject(None)
        }
        KeyCode::Esc => Action::None,
        KeyCode::Tab | KeyCode::BackTab => {
            // 2 tables at most: next and previous are the same one.
            if s.has_rollup() {
                s.focus = 1 - s.focus.min(1);
            }
            Action::Redraw
        }
        KeyCode::Down | KeyCode::Char('j') => {
            s.tables[t].select(s.tables[t].sel + 1, n, pg);
            Action::Redraw
        }
        KeyCode::Up | KeyCode::Char('k') => {
            s.tables[t].select(s.tables[t].sel.saturating_sub(1), n, pg);
            Action::Redraw
        }
        KeyCode::PageDown => {
            s.tables[t].offset = (s.tables[t].offset + pg).min(n.saturating_sub(pg));
            s.tables[t].select(s.tables[t].sel + pg, n, pg);
            Action::Redraw
        }
        KeyCode::PageUp => {
            s.tables[t].offset = s.tables[t].offset.saturating_sub(pg);
            s.tables[t].select(s.tables[t].sel.saturating_sub(pg), n, pg);
            Action::Redraw
        }
        KeyCode::Home => {
            s.tables[t].select(0, n, pg);
            Action::Redraw
        }
        KeyCode::End => {
            s.tables[t].select(n.saturating_sub(1), n, pg);
            Action::Redraw
        }
        KeyCode::Char('g') => {
            s.group = (s.group + 1) % GROUPS.len();
            s.tables[ROLLUP] = TableState::default();
            Action::Redraw
        }
        KeyCode::Char('w') => {
            s.window = (s.window + 1) % VIEW_WINDOWS.len();
            s.tables[ROLLUP] = TableState::default();
            if !s.has_rollup() {
                s.focus = LIVE;
            }
            Action::Redraw
        }
        KeyCode::Char('p') => {
            s.project = next_project(&rows.projects, s.project.take());
            Action::SetProject(s.project.clone())
        }
        KeyCode::Char('r') => Action::Refresh,
        _ => Action::None,
    };
    if action == Action::Redraw && s == before {
        return (s, Action::None);
    }
    (s, action)
}

/// A mouse wheel step over table `t`: scroll it, and keep the selection on
/// the screen. Pure.
pub fn on_wheel(mut s: ViewState, t: usize, down: bool, rows: &Rows) -> (ViewState, Action) {
    let before = s.clone();
    let (n, pg) = (rows.count(t), rows.page[t].max(1));
    let table = &mut s.tables[t];
    table.offset = if down {
        (table.offset + WHEEL_ROWS).min(n.saturating_sub(pg))
    } else {
        table.offset.saturating_sub(WHEEL_ROWS)
    };
    let sel = table
        .sel
        .clamp(table.offset, (table.offset + pg - 1).max(table.offset));
    if sel != table.sel {
        table.sel = sel.min(n.saturating_sub(1));
        table.key = None;
    }
    let action = if s == before {
        Action::None
    } else {
        Action::Redraw
    };
    (s, action)
}

/// The table under the screen cell `(col, row)`, if any.
pub fn table_at(
    snap: &Snapshot,
    view: &ViewState,
    area: Rect,
    col: u16,
    row: u16,
) -> Option<usize> {
    let a = areas(snap, view, area);
    let pos = ratatui::layout::Position::new(col, row);
    if a.live.contains(pos) {
        Some(LIVE)
    } else if a.rollup.is_some_and(|r| r.contains(pos)) {
        Some(ROLLUP)
    } else {
        None
    }
}

/// Every project the snapshot names, sorted.
fn projects(snap: &Snapshot) -> Vec<String> {
    let mut projects: Vec<String> = snap.live.iter().filter_map(|l| l.project.clone()).collect();
    for rollup in snap.rollups.values() {
        projects.extend(
            rollup
                .by_project
                .iter()
                .map(|r| r.key.clone())
                .filter(|k| k != "-"),
        );
    }
    projects.sort();
    projects.dedup();
    projects
}

/// The projects the `p` key cycles through: none (all), then each one.
fn next_project(projects: &[String], current: Option<String>) -> Option<String> {
    match current {
        None => projects.first().cloned(),
        Some(c) => projects.iter().skip_while(|p| **p != c).nth(1).cloned(),
    }
}

// ─── the frame ──────────────────────────────────────────────────────────────

fn live_rows<'a>(snap: &'a Snapshot, view: &ViewState) -> Vec<&'a LiveRow> {
    snap.live
        .iter()
        .filter(|r| view.project.is_none() || r.project == view.project)
        .collect()
}

fn rollup_rows<'a>(snap: &'a Snapshot, view: &ViewState) -> &'a [RollupRow] {
    if !view.has_rollup() {
        return &[];
    }
    snap.rollups
        .get(view.window_name())
        .and_then(|r| r.group(view.group_name()))
        .unwrap_or(&[])
}

/// Where each part of the frame goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Areas {
    header: Rect,
    pools: Rect,
    live: Rect,
    rollup: Option<Rect>,
    facts: Rect,
    footer: Rect,
}

fn areas(snap: &Snapshot, view: &ViewState, area: Rect) -> Areas {
    let row = |y: u16, h: u16| Rect::new(area.x, area.y + y, area.width, h);
    let h = area.height;
    // The pool table has as many lines at any time.
    let pools_want = pool_lines(snap, area.width, snapshot_time(snap)).len() as u16;
    let facts_want = facts(snap, view, area.width).len() as u16;
    // The header and the footer take 1 line each; then the pools, then the
    // facts give way to the tables' minimum of 3 lines each.
    let tables_min = if view.has_rollup() { 6 } else { 3 };
    let mut rest = h.saturating_sub(2);
    let pools_h = pools_want.min(rest.saturating_sub(tables_min));
    rest -= pools_h;
    let facts_h = facts_want.min(rest.saturating_sub(tables_min));
    rest -= facts_h;
    let header = row(0, h.min(1));
    let pools = row(1, pools_h);
    let tables_y = 1 + pools_h;
    let (live, rollup) = if view.has_rollup() {
        let live_want = live_rows(snap, view).len() as u16 + 2;
        let rollup_want = rollup_rows(snap, view).len() as u16 + 2;
        let live_h = if live_want + rollup_want <= rest {
            live_want
        } else {
            live_want.min((rest / 2).max(3))
        }
        .min(rest);
        (
            row(tables_y, live_h),
            Some(row(tables_y + live_h, rest - live_h)),
        )
    } else {
        (row(tables_y, rest), None)
    };
    Areas {
        header,
        pools,
        live,
        rollup,
        facts: row(tables_y + rest, facts_h),
        footer: row(h.saturating_sub(1), h.min(1)),
    }
}

fn pool_lines(snap: &Snapshot, width: u16, now: DateTime<Utc>) -> Vec<String> {
    // No probe reads the Google pool yet, so the screen shows it only when
    // a reading exists; `horch quota` always lists it.
    pool_table(&snap.pools, now, width as usize, &[quota::POOL_GOOGLE])
}

/// The time the snapshot names. On disk that is its last content change,
/// not its last tick: the live screen draws at `clock::now()` instead.
pub fn snapshot_time(snap: &Snapshot) -> DateTime<Utc> {
    clock::parse(&snap.generated_at).unwrap_or_else(clock::now)
}

/// The lines under the tables: UNREAD, insights, unpriced, status.
fn facts(snap: &Snapshot, view: &ViewState, width: u16) -> Vec<String> {
    let width = width as usize;
    let mut out = Vec::new();
    if !snap.unread.is_empty() {
        let items: Vec<String> = snap
            .unread
            .iter()
            .map(|u| format!("{}: {}", u.role, u.reason))
            .collect();
        out.push(clip(&format!("UNREAD  {}", items.join("; ")), width));
    }
    let i = &snap.insights;
    let mut facts = Vec::new();
    if let Some(s) = i.orchestrator_share {
        facts.push(format!("orchestrators {} of spend", pct(s)));
    }
    if let Some(h) = i.cache_hit {
        facts.push(format!("cache hit {}", pct(h)));
    }
    if let Some(s) = i.idle_spend_share {
        facts.push(format!("{} of spend while idle", pct(s)));
    }
    if let Some(p) = &i.top_plan {
        facts.push(format!("top plan {p}"));
    }
    if !facts.is_empty() {
        out.push(clip(&facts.join(" · "), width));
    }
    let window = view.window_name();
    // The live view counts the last 5 hours' unpriced events.
    let priced_window = if window == "live" { "5h" } else { window };
    if let Some(line) = snap
        .rollups
        .get(priced_window)
        .and_then(|r| unpriced_line(&r.unpriced, width))
    {
        out.push(line);
    }
    if let Some(s) = &view.status {
        out.push(clip(&format!("! {s}"), width));
    }
    out
}

fn header(snap: &Snapshot, view: &ViewState, width: usize, now: DateTime<Utc>) -> String {
    let live = live_rows(snap, view);
    let fleets = live
        .iter()
        .filter(|r| r.kind == horch_core::execution::legacy::KIND_ORCHESTRATOR)
        .count();
    let left = format!(
        "horch telemetry · {fleets} fleet{} · {} live pane{} · {}",
        if fleets == 1 { "" } else { "s" },
        live.len(),
        if live.len() == 1 { "" } else { "s" },
        now.format("%H:%MZ")
    );
    let mut right = String::new();
    if let Some(m) = view.mode {
        right.push_str(&format!("[{m}]  "));
    }
    right.push_str(&format!(
        "group: {}  window: {}  project: {}",
        view.group_name(),
        view.window_name(),
        view.project
            .as_deref()
            .map(|p| project_name(Some(p)))
            .unwrap_or_else(|| "all".into())
    ));
    // The left part gives way first: the view's group and window must show.
    let left = clip(&left, width.saturating_sub(right.chars().count() + 2));
    let gap = width
        .saturating_sub(left.chars().count() + right.chars().count())
        .max(2);
    clip(&format!("{left}{}{right}", " ".repeat(gap)), width)
}

fn live_header(width: usize) -> String {
    clip(
        &format!(
            "{}{}{}{}{}{}{}{}{}{}",
            pad("LIVE", 14),
            pad("", 17),
            pad("model", 11),
            pad("phase", 7),
            pad("plan", 21),
            pad("fresh", 8),
            pad("c.read", 9),
            pad("out", 7),
            pad("$", 8),
            "tok/min"
        ),
        width,
    )
}

fn live_line(r: &LiveRow, project: &str, width: usize) -> String {
    let mut role = r.role.clone();
    if let Some(via) = &r.via {
        role = format!("{role}>{via}");
    }
    let mut flags = String::new();
    if r.idle {
        flags.push_str(" idle");
    }
    if matches!(
        r.pool_state.as_str(),
        "tight" | "exhausted" | "broken" | "cooling"
    ) {
        flags.push_str(&format!(" {}:{}", r.pool, r.pool_state));
    }
    clip(
        &format!(
            "{}{}{}{}{}{}{}{}{}{}{}",
            pad(project, 14),
            pad(&role, 17),
            pad(&short_model(&r.model), 11),
            pad(short_phase(r.phase.as_deref()), 7),
            pad(r.plan.as_deref().unwrap_or("-"), 21),
            pad(&human(r.tokens.fresh()), 8),
            pad(&human(r.tokens.cache_read), 9),
            pad(&human(r.tokens.output), 7),
            pad(&money(r.cost_usd, r.unpriced_events), 8),
            human(r.rate_tokens_per_min.round() as u64),
            flags
        ),
        width,
    )
}

fn rollup_header(title: &str, width: usize) -> String {
    clip(
        &format!(
            "{}{}{}{}{}{}",
            pad(&title.to_uppercase(), 31),
            pad("tokens", 9),
            pad("$", 9),
            pad("share", 8),
            pad("cache hit", 12),
            "$/DONE"
        ),
        width,
    )
}

fn rollup_line(r: &RollupRow, total: f64, width: usize) -> String {
    clip(
        format!(
            "{}{}{}{}{}{}",
            pad(&r.key, 31),
            pad(&human(r.tokens.total()), 9),
            pad(&money(r.cost_usd, r.unpriced_events), 9),
            pad(
                &if total > 0.0 {
                    pct(r.cost_usd / total)
                } else {
                    "-".into()
                },
                8
            ),
            pad(&r.cache_hit.map(pct).unwrap_or_else(|| "-".into()), 12),
            r.cost_per_done
                .map(|c| format!("{c:.2}"))
                .unwrap_or_else(|| "-".into())
        )
        .trim_end(),
        width,
    )
}

/// `rows A-B of N` for a table scrolled to `offset` with `shown` rows.
fn range(offset: usize, shown: usize, n: usize) -> String {
    if n == 0 {
        "rows 0 of 0".into()
    } else {
        format!("rows {}-{} of {n}", offset + 1, offset + shown)
    }
}

/// 1 scrollable table: a title line, a column header, and the rows from
/// `table.offset`. `lines` holds each row's text and its own style.
fn draw_table(
    buf: &mut Buffer,
    area: Rect,
    title: &str,
    head: String,
    lines: Vec<(String, Style)>,
    table: &TableState,
    focused: bool,
) {
    if area.height == 0 {
        return;
    }
    let pg = page(area);
    let n = lines.len();
    let offset = table.offset.min(n.saturating_sub(pg));
    let shown = n.saturating_sub(offset).min(pg);
    let mut title_style = Style::default();
    if focused {
        title_style = title_style.add_modifier(Modifier::BOLD);
    }
    let block = Block::new().borders(Borders::TOP).title(Span::styled(
        format!(" {title}  {} ", range(offset, shown, n)),
        title_style,
    ));
    let inner = block.inner(area);
    ratatui::widgets::Widget::render(block, area, buf);
    let mut text = vec![Line::from(head)];
    for (i, (line, style)) in lines.into_iter().enumerate().skip(offset).take(shown) {
        let mut style = style;
        if focused && i == table.sel {
            style = style.add_modifier(Modifier::REVERSED);
        }
        text.push(Line::styled(line, style));
    }
    ratatui::widgets::Widget::render(Paragraph::new(text), inner, buf);
}

/// A pool line, with its state colored: ok green, tight yellow, exhausted
/// red. The text is the same with or without color.
fn pool_line(line: String, color: bool) -> Line<'static> {
    // Every pool row form puts the state at characters 43..55.
    const STATE: (usize, usize) = (43, 55);
    let state: String = line.chars().skip(STATE.0).take(STATE.1 - STATE.0).collect();
    let fg = match state
        .trim()
        .trim_end_matches('*')
        .to_ascii_lowercase()
        .as_str()
    {
        "ok" => Some(Color::Green),
        "tight" => Some(Color::Yellow),
        "exhausted" => Some(Color::Red),
        _ => None,
    };
    match fg {
        Some(fg) if color => {
            let head: String = line.chars().take(STATE.0).collect();
            let tail: String = line.chars().skip(STATE.1).collect();
            Line::from(vec![
                Span::raw(head),
                Span::styled(state, Style::default().fg(fg)),
                Span::raw(tail),
            ])
        }
        _ => Line::from(line),
    }
}

/// 1 frame of the screen at time `now` (the header clock and the pools'
/// relative times). Pure.
pub fn draw(frame: &mut Frame, snap: &Snapshot, view: &ViewState, now: DateTime<Utc>) {
    let area = frame.area();
    draw_buf(frame.buffer_mut(), area, snap, view, now);
}

/// [`draw`] into a buffer.
pub fn draw_buf(
    buf: &mut Buffer,
    area: Rect,
    snap: &Snapshot,
    view: &ViewState,
    now: DateTime<Utc>,
) {
    let width = area.width as usize;
    let a = areas(snap, view, area);
    let para = |buf: &mut Buffer, lines: Vec<Line<'static>>, r: Rect| {
        ratatui::widgets::Widget::render(Paragraph::new(lines), r, buf);
    };
    para(
        buf,
        vec![Line::from(header(snap, view, width, now))],
        a.header,
    );
    let pools = pool_lines(snap, area.width, now)
        .into_iter()
        .enumerate()
        .map(|(i, l)| {
            if i == 0 {
                Line::from(l)
            } else {
                pool_line(l, view.color)
            }
        })
        .collect();
    para(buf, pools, a.pools);

    let live = live_rows(snap, view);
    let offset = view.tables[LIVE].offset;
    let mut last_project: Option<String> = None;
    let lines: Vec<(String, Style)> = live
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let project = project_name(r.project.as_deref());
            // A project shows once per run of rows, and on the first row
            // on the screen.
            let shown = if i != offset && last_project.as_deref() == Some(project.as_str()) {
                String::new()
            } else {
                project.clone()
            };
            last_project = Some(project);
            let style = if r.idle {
                Style::default().add_modifier(Modifier::DIM)
            } else {
                Style::default()
            };
            (live_line(r, &shown, width), style)
        })
        .collect();
    draw_table(
        buf,
        a.live,
        "live panes",
        live_header(width),
        lines,
        &view.tables[LIVE],
        view.focus == LIVE,
    );

    if let Some(r) = a.rollup {
        let rows = rollup_rows(snap, view);
        let total: f64 = rows.iter().map(|r| r.cost_usd).sum();
        let title = format!("{} by {}", view.window_name(), view.group_name());
        let lines = rows
            .iter()
            .map(|row| (rollup_line(row, total, width), Style::default()))
            .collect();
        draw_table(
            buf,
            r,
            &title,
            rollup_header(&title, width),
            lines,
            &view.tables[ROLLUP],
            view.focus == ROLLUP,
        );
    }

    para(
        buf,
        facts(snap, view, area.width)
            .into_iter()
            .map(Line::from)
            .collect(),
        a.facts,
    );
    let mut footer = Style::default();
    if view.color {
        footer = footer.fg(Color::DarkGray);
    }
    para(
        buf,
        vec![Line::styled(clip(FOOTER, width), footer)],
        a.footer,
    );

    if view.help {
        let w =
            (HELP.iter().map(|l| l.chars().count()).max().unwrap_or(0) as u16 + 4).min(area.width);
        let h = (HELP.len() as u16 + 2).min(area.height);
        let r = Rect::new(
            area.x + (area.width - w) / 2,
            area.y + (area.height - h) / 2,
            w,
            h,
        );
        ratatui::widgets::Widget::render(Clear, r, buf);
        let text: Vec<Line> = HELP.iter().map(|l| Line::from(format!(" {l}"))).collect();
        ratatui::widgets::Widget::render(
            Paragraph::new(text).block(Block::bordered().title(" help ")),
            r,
            buf,
        );
    }
}

/// The text of a buffer: 1 line per row, trailing spaces trimmed.
pub fn buffer_text(buf: &Buffer) -> Vec<String> {
    let area = buf.area;
    (area.y..area.y + area.height)
        .map(|y| {
            let mut line = String::new();
            for x in area.x..area.x + area.width {
                line.push_str(buf[(x, y)].symbol());
            }
            line.trim_end().to_string()
        })
        .collect()
}

/// 1 frame drawn into a `TestBackend` of `width` x `height`, as text
/// (SPC-04). The snapshot's `generated_at` is the frame's time, so a
/// golden stays fixed.
pub fn render(snap: &Snapshot, view: &ViewState, width: u16, height: u16) -> Vec<String> {
    let backend = TestBackend::new(width.max(20), height.max(6));
    let mut terminal = Terminal::new(backend).expect("a TestBackend never fails");
    terminal
        .draw(|f| draw(f, snap, view, snapshot_time(snap)))
        .expect("a TestBackend never fails");
    buffer_text(terminal.backend().buffer())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn rows(live: usize, rollup: usize, page: usize) -> Rows {
        Rows {
            keys: [
                (0..live).map(|i| format!("l{i}")).collect(),
                (0..rollup).map(|i| format!("r{i}")).collect(),
            ],
            page: [page, page],
            projects: vec!["/a".into(), "/b".into()],
        }
    }

    fn press(s: ViewState, code: KeyCode, r: &Rows) -> ViewState {
        on_key(s, key(code), r).0
    }

    /// SPC-07: `j` on the last row stays there.
    #[test]
    fn spc_07_j_at_the_last_row_stays() {
        let r = rows(5, 0, 20);
        let mut s = ViewState::default();
        for _ in 0..4 {
            s = press(s, KeyCode::Char('j'), &r);
        }
        assert_eq!(s.tables[LIVE].sel, 4);
        let (s, action) = on_key(s, key(KeyCode::Char('j')), &r);
        assert_eq!(s.tables[LIVE].sel, 4);
        assert_eq!(action, Action::None);
        let s = press(s, KeyCode::Char('k'), &r);
        assert_eq!(s.tables[LIVE].sel, 3);
    }

    /// SPC-07: `PgDn` on 87 rows with a page of 20 shows rows 21-40; `End`
    /// shows the last page; `Home` the first.
    #[test]
    fn spc_07_page_down_and_end() {
        let r = rows(87, 0, 20);
        let s = press(ViewState::default(), KeyCode::PageDown, &r);
        let t = &s.tables[LIVE];
        assert_eq!((t.offset + 1, t.offset + 20), (21, 40));
        assert_eq!(t.sel, 20);
        let s = press(s, KeyCode::End, &r);
        let t = &s.tables[LIVE];
        assert_eq!((t.offset + 1, t.offset + 20, t.sel), (68, 87, 86));
        let s = press(s, KeyCode::PageUp, &r);
        assert_eq!(s.tables[LIVE].offset, 47);
        let s = press(s, KeyCode::Home, &r);
        assert_eq!((s.tables[LIVE].offset, s.tables[LIVE].sel), (0, 0));
    }

    /// SPC-07: `Tab` cycles the focus over the tables that exist: 1 in the
    /// `live` window, 2 in the others.
    #[test]
    fn spc_07_tab_cycles_over_the_tables_that_exist() {
        let r = rows(5, 5, 20);
        let live = ViewState::with("teammate", "live").unwrap();
        let (s, action) = on_key(live, key(KeyCode::Tab), &r);
        assert_eq!((s.focus, action), (LIVE, Action::None));
        let s = ViewState::with("teammate", "7d").unwrap();
        let s = press(s, KeyCode::Tab, &r);
        assert_eq!(s.focus, ROLLUP);
        let s = press(s, KeyCode::Char('j'), &r);
        assert_eq!((s.tables[ROLLUP].sel, s.tables[LIVE].sel), (1, 0));
        let s = press(s, KeyCode::BackTab, &r);
        assert_eq!(s.focus, LIVE);
        // `w` to `live` moves the focus off a rollup that is gone.
        let s = press(press(s, KeyCode::Tab, &r), KeyCode::Char('w'), &r);
        assert_eq!((s.window_name(), s.focus), ("live", LIVE));
    }

    /// SPC-07: a selection keeps its row key across an update that inserts
    /// a row above it; a key that is gone clamps.
    #[test]
    fn spc_07_selection_keeps_its_row_key_across_an_update() {
        let mut r = rows(5, 0, 20);
        let mut s = ViewState::default();
        s = press(press(s, KeyCode::Down, &r), KeyCode::Down, &r);
        s.sync(&r);
        assert_eq!(s.tables[LIVE].key.as_deref(), Some("l2"));
        r.keys[LIVE].insert(0, "new".into());
        s.sync(&r);
        assert_eq!(s.tables[LIVE].sel, 3);
        assert_eq!(s.tables[LIVE].key.as_deref(), Some("l2"));
        r.keys[LIVE] = vec!["x".into()];
        s.sync(&r);
        assert_eq!(
            (s.tables[LIVE].sel, s.tables[LIVE].key.as_deref()),
            (0, Some("x"))
        );
    }

    /// SPC-07: `Esc` closes the help before it clears the project filter.
    #[test]
    fn spc_07_esc_closes_help_before_it_clears_the_project() {
        let r = rows(5, 0, 20);
        let (s, action) = on_key(ViewState::default(), key(KeyCode::Char('p')), &r);
        assert_eq!(action, Action::SetProject(Some("/a".into())));
        let s = press(s, KeyCode::Char('?'), &r);
        assert!(s.help);
        let (s, action) = on_key(s, key(KeyCode::Esc), &r);
        assert_eq!(
            (s.help, s.project.as_deref(), action),
            (false, Some("/a"), Action::Redraw)
        );
        let (s, action) = on_key(s, key(KeyCode::Esc), &r);
        assert_eq!((s.project, action), (None, Action::SetProject(None)));
    }

    /// SPC-07: the wheel scrolls the table and keeps the selection on the
    /// screen.
    #[test]
    fn spc_07_wheel_scrolls_and_keeps_the_selection_visible() {
        let r = rows(30, 0, 10);
        let (s, action) = on_wheel(ViewState::default(), LIVE, true, &r);
        assert_eq!(action, Action::Redraw);
        assert_eq!((s.tables[LIVE].offset, s.tables[LIVE].sel), (3, 3));
        let mut s = s;
        for _ in 0..20 {
            s = on_wheel(s, LIVE, true, &r).0;
        }
        assert_eq!(s.tables[LIVE].offset, 20);
        let (s, _) = on_wheel(s, LIVE, false, &r);
        assert_eq!(s.tables[LIVE].offset, 17);
        let (_, action) = on_wheel(ViewState::default(), LIVE, false, &r);
        assert_eq!(action, Action::None);
    }

    /// The live screen draws at the time it is given, not at the snapshot's
    /// `generated_at` (the last content change on disk).
    #[test]
    fn spc_04_the_header_clock_is_the_given_time() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/golden/snapshot-fixture.json");
        let snap = Snapshot::read(&path).unwrap();
        let at = snapshot_time(&snap);
        let later = at + chrono::Duration::minutes(95);
        let area = Rect::new(0, 0, 120, 40);
        let text = |now| {
            let mut buf = Buffer::empty(area);
            draw_buf(&mut buf, area, &snap, &ViewState::default(), now);
            buffer_text(&buf)[0].clone()
        };
        assert!(text(at).contains(&at.format("%H:%MZ").to_string()));
        let head = text(later);
        assert!(head.contains(&later.format("%H:%MZ").to_string()), "{head}");
        assert!(!head.contains(&at.format("%H:%MZ").to_string()), "{head}");
    }

    /// `p` cycles none, each project, none; `q` and Ctrl-C quit; `r`
    /// refreshes.
    #[test]
    fn spc_04_keys_p_q_r() {
        let r = rows(1, 0, 20);
        let s = press(ViewState::default(), KeyCode::Char('p'), &r);
        let (s, a) = on_key(s, key(KeyCode::Char('p')), &r);
        assert_eq!(a, Action::SetProject(Some("/b".into())));
        let (_, a) = on_key(s, key(KeyCode::Char('p')), &r);
        assert_eq!(a, Action::SetProject(None));
        let s = ViewState::default();
        assert_eq!(
            on_key(s.clone(), key(KeyCode::Char('q')), &r).1,
            Action::Quit
        );
        let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(on_key(s.clone(), ctrl_c, &r).1, Action::Quit);
        assert_eq!(on_key(s, key(KeyCode::Char('r')), &r).1, Action::Refresh);
    }
}
