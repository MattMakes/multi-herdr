use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use horch_core::clock;
use horch_core::routing::quota;
use horch_core::telemetry::collect::{Insights, LiveRow, Snapshot};
use horch_core::telemetry::store::{RollupRow, GROUPS};
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::{Frame, Terminal};

use super::{
    clip, human, money, pad, pct, pool_compact, pool_table, project_name, short_model, short_phase,
    unpriced_line, COMPACT_STATE, MIN_SIZE, VIEW_WINDOWS,
};

pub const LIVE: usize = 0;
pub const ROLLUP: usize = 1;

const WHEEL_ROWS: usize = 3;

const FOOTER: &str = "? help  q quit  Tab focus  ↑↓ jk move  PgUp PgDn  Home End  g group  w window  p project  r refresh";

const HELP_KEYS: &[&str] = &[
    "Tab  Shift-Tab   focus the other table",
    "↓ j  ↑ k         move the selection 1 row",
    "PgDn  PgUp       move 1 page",
    "Home  End        the first or the last row",
    "mouse wheel      scroll the table under the pointer",
    "g  w  p          cycle the group, window, project",
    "r                refresh now",
    "Esc              close help, else clear the project",
    "?                show or hide this help",
    "q  Ctrl-C        quit",
];

const HELP_MORE: &[&str] = &[
    "",
    "groups:  teammate, phase, agent, project, plan, kind",
    "windows:",
    "live    the live panes only ($* counts the last 5h's unpriced)",
    "5h      the live panes, and the rollup of the last 5 hours",
    "today   the live panes, and the rollup since 00:00 UTC",
    "7d      the live panes, and the rollup of the last 7 days",
];

pub const REFRESHING: &str = "refreshing…";

const SHORT_ROWS: u16 = 24;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TableState {
    pub sel: usize,
    pub offset: usize,
    pub key: Option<String>,
}

impl TableState {
    fn select(&mut self, sel: usize, n: usize, page: usize) {
        self.sel = sel.min(n.saturating_sub(1));
        self.key = None;
        self.fit(n, page);
    }

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
    pub group: usize,
    pub window: usize,
    pub project: Option<String>,
    pub status: Option<String>,
    pub mode: Option<&'static str>,
    pub focus: usize,
    pub tables: [TableState; 2],
    pub help: bool,
    pub loading: bool,
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

    pub fn has_rollup(&self) -> bool {
        self.window_name() != "live"
    }

    pub fn sync(&mut self, rows: &Rows) {
        for t in [LIVE, ROLLUP] {
            self.tables[t].sync(&rows.keys[t], rows.page[t]);
        }
        if !self.has_rollup() {
            self.focus = LIVE;
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    None,
    Redraw,
    Refresh,
    SetProject(Option<String>),
    Quit,
}

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

fn live_key(r: &LiveRow) -> String {
    if r.record_id.is_empty() {
        format!("{}\t{}", r.project.as_deref().unwrap_or("-"), r.role)
    } else {
        r.record_id.clone()
    }
}

fn page(r: Rect) -> usize {
    (r.height as usize).saturating_sub(2).max(1)
}

fn is_motion(code: KeyCode) -> bool {
    matches!(
        code,
        KeyCode::Down
            | KeyCode::Up
            | KeyCode::Char('j')
            | KeyCode::Char('k')
            | KeyCode::PageDown
            | KeyCode::PageUp
            | KeyCode::Home
            | KeyCode::End
    )
}

fn is_quit(key: &KeyEvent) -> bool {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    matches!(
        (key.code, ctrl),
        (KeyCode::Char('q'), _) | (KeyCode::Char('c'), true)
    )
}

fn counts(s: &ViewState, key: &KeyEvent) -> bool {
    let pressed = match key.kind {
        KeyEventKind::Press => true,
        KeyEventKind::Repeat => is_motion(key.code),
        KeyEventKind::Release => false,
    };
    let allowed = !s.help || is_quit(key) || matches!(key.code, KeyCode::Char('?') | KeyCode::Esc);
    pressed && allowed
}

fn move_selection(table: &mut TableState, code: KeyCode, n: usize, pg: usize) {
    match code {
        KeyCode::Down | KeyCode::Char('j') => table.select(table.sel + 1, n, pg),
        KeyCode::Up | KeyCode::Char('k') => table.select(table.sel.saturating_sub(1), n, pg),
        KeyCode::PageDown => {
            table.offset = (table.offset + pg).min(n.saturating_sub(pg));
            table.select(table.sel + pg, n, pg);
        }
        KeyCode::PageUp => {
            table.offset = table.offset.saturating_sub(pg);
            table.select(table.sel.saturating_sub(pg), n, pg);
        }
        KeyCode::Home => table.select(0, n, pg),
        _ => table.select(n.saturating_sub(1), n, pg),
    }
}

fn on_help_or_esc(s: &mut ViewState, code: KeyCode) -> Action {
    if code == KeyCode::Char('?') {
        s.help = !s.help;
        return Action::Redraw;
    }
    if s.help {
        s.help = false;
        Action::Redraw
    } else if s.project.is_some() {
        s.project = None;
        Action::SetProject(None)
    } else {
        Action::None
    }
}

fn on_view_key(s: &mut ViewState, code: KeyCode) {
    match code {
        KeyCode::Char('g') => {
            s.group = (s.group + 1) % GROUPS.len();
            s.tables[ROLLUP] = TableState::default();
        }
        KeyCode::Char('w') => {
            s.window = (s.window + 1) % VIEW_WINDOWS.len();
            s.tables[ROLLUP] = TableState::default();
            if !s.has_rollup() {
                s.focus = LIVE;
            }
        }
        _ => {
            if s.has_rollup() {
                s.focus = 1 - s.focus.min(1);
            }
        }
    }
}

pub fn on_key(mut s: ViewState, key: KeyEvent, rows: &Rows) -> (ViewState, Action) {
    if !counts(&s, &key) {
        return (s, Action::None);
    }
    let before = s.clone();
    let action = dispatch(&mut s, &key, rows);
    let quiet = action == Action::Redraw && s == before;
    (s, if quiet { Action::None } else { action })
}

fn dispatch(s: &mut ViewState, key: &KeyEvent, rows: &Rows) -> Action {
    let t = s.focus;
    match key.code {
        _ if is_quit(key) => Action::Quit,
        KeyCode::Char('?') | KeyCode::Esc => on_help_or_esc(s, key.code),
        KeyCode::Tab | KeyCode::BackTab | KeyCode::Char('g') | KeyCode::Char('w') => {
            on_view_key(s, key.code);
            Action::Redraw
        }
        code if is_motion(code) => {
            move_selection(&mut s.tables[t], code, rows.count(t), rows.page[t]);
            Action::Redraw
        }
        code => on_data_key(s, code, rows),
    }
}

fn on_data_key(s: &mut ViewState, code: KeyCode, rows: &Rows) -> Action {
    match code {
        KeyCode::Char('p') => {
            s.project = next_project(&rows.projects, s.project.take());
            Action::SetProject(s.project.clone())
        }
        KeyCode::Char('r') => {
            s.status = Some(REFRESHING.into());
            Action::Refresh
        }
        _ => Action::None,
    }
}

pub fn on_wheel(mut s: ViewState, t: usize, down: bool, rows: &Rows) -> (ViewState, Action) {
    if s.help {
        return (s, Action::None);
    }
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

fn next_project(projects: &[String], current: Option<String>) -> Option<String> {
    match current {
        None => projects.first().cloned(),
        Some(c) => projects.iter().skip_while(|p| **p != c).nth(1).cloned(),
    }
}

fn live_rows<'a>(snap: &'a Snapshot, view: &ViewState) -> Vec<&'a LiveRow> {
    snap.live
        .iter()
        .filter(|r| view.project.is_none() || r.project == view.project)
        .collect()
}

fn rollup_rows<'a>(snap: &'a Snapshot, view: &ViewState) -> &'a [RollupRow] {
    if !view.has_rollup() || view.loading {
        return &[];
    }
    snap.rollups
        .get(view.window_name())
        .and_then(|r| r.group(view.group_name()))
        .unwrap_or(&[])
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Areas {
    header: Rect,
    pools: Rect,
    live: Rect,
    rollup: Option<Rect>,
    facts: Rect,
    footer: Rect,
}

fn short(area: Rect) -> bool {
    area.height <= SHORT_ROWS
}

fn areas(snap: &Snapshot, view: &ViewState, area: Rect) -> Areas {
    let row = |y: u16, h: u16| Rect::new(area.x, area.y + y, area.width, h);
    let h = area.height;
    let pools_want = pool_lines(snap, area, snapshot_time(snap)).len() as u16;
    let facts_want = fact_lines(snap, view, area).len() as u16;
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
        let rollup_want = (rollup_rows(snap, view).len() as u16).max(1) + 2;
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

fn pool_lines(snap: &Snapshot, area: Rect, now: DateTime<Utc>) -> Vec<String> {
    let only = &[quota::POOL_GOOGLE];
    let width = area.width as usize;
    if short(area) {
        pool_compact(&snap.pools, now, width, only)
    } else {
        pool_table(&snap.pools, now, width, only)
    }
}

pub fn snapshot_time(snap: &Snapshot) -> DateTime<Utc> {
    clock::parse(&snap.generated_at).unwrap_or_else(clock::now)
}

fn facts(snap: &Snapshot, view: &ViewState) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(s) = &view.status {
        out.push(format!("! {s}"));
    }
    let window = view.window_name();
    let priced_window = if window == "live" { "5h" } else { window };
    if let Some(line) = snap
        .rollups
        .get(priced_window)
        .filter(|_| !view.loading)
        .and_then(|r| unpriced_line(&r.unpriced, usize::MAX))
    {
        out.push(line);
    }
    if !snap.unread.is_empty() {
        let items: Vec<String> = snap
            .unread
            .iter()
            .map(|u| format!("{}: {}", u.role, u.reason))
            .collect();
        out.push(format!("UNREAD  {}", items.join("; ")));
    }
    out.extend(insight_line(&snap.insights));
    out
}

fn insight_line(i: &Insights) -> Option<String> {
    let facts: Vec<String> = [
        i.orchestrator_share
            .map(|s| format!("orchestrators {} of spend", pct(s))),
        i.cache_hit.map(|h| format!("cache hit {}", pct(h))),
        i.idle_spend_share
            .map(|s| format!("{} of spend while idle", pct(s))),
        i.top_plan.as_ref().map(|p| format!("top plan {p}")),
    ]
    .into_iter()
    .flatten()
    .collect();
    (!facts.is_empty()).then(|| facts.join(" · "))
}

fn fact_lines(snap: &Snapshot, view: &ViewState, area: Rect) -> Vec<String> {
    let width = area.width as usize;
    let facts = facts(snap, view);
    if short(area) && !facts.is_empty() {
        return vec![clip(&facts.join("  ·  "), width)];
    }
    facts.iter().map(|f| clip(f, width)).collect()
}

fn project_labels(snap: &Snapshot) -> std::collections::BTreeMap<String, String> {
    let projects = projects(snap);
    let tail = |p: &str, n: usize| {
        let parts: Vec<&str> = p.split('/').filter(|x| !x.is_empty()).collect();
        parts[parts.len().saturating_sub(n)..].join("/")
    };
    let mut out = std::collections::BTreeMap::new();
    for p in &projects {
        let base = project_name(Some(p));
        let twins = projects
            .iter()
            .filter(|q| project_name(Some(q)) == base)
            .count();
        let label = if twins < 2 {
            base
        } else {
            let two = tail(p, 2);
            let same = projects.iter().filter(|q| tail(q, 2) == two).count();
            if same < 2 {
                two
            } else {
                p.clone()
            }
        };
        out.insert(p.clone(), label);
    }
    out
}

fn label(labels: &std::collections::BTreeMap<String, String>, p: Option<&str>) -> String {
    p.and_then(|p| labels.get(p).cloned())
        .unwrap_or_else(|| project_name(p))
}

fn header(snap: &Snapshot, view: &ViewState, width: usize, now: DateTime<Utc>) -> String {
    let live = live_rows(snap, view);
    let fleets = live
        .iter()
        .filter(|r| r.kind == horch_core::execution::legacy::KIND_ORCHESTRATOR)
        .count();
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
            .map(|p| label(&project_labels(snap), Some(p)))
            .unwrap_or_else(|| "all".into())
    ));
    let clock = now.format("%H:%MZ");
    let panes = format!(
        "{} live pane{}",
        live.len(),
        if live.len() == 1 { "" } else { "s" }
    );
    let fleets = format!("{fleets} fleet{}", if fleets == 1 { "" } else { "s" });
    let room = width.saturating_sub(right.chars().count() + 2);
    let short = format!("horch telemetry · {clock}");
    let left = [
        format!("horch telemetry · {fleets} · {panes} · {clock}"),
        format!("horch telemetry · {panes} · {clock}"),
    ]
    .into_iter()
    .find(|l| l.chars().count() <= room)
    .unwrap_or_else(|| clip(&short, room));
    let gap = width
        .saturating_sub(left.chars().count() + right.chars().count())
        .max(2);
    clip(&format!("{left}{}{right}", " ".repeat(gap)), width)
}

#[derive(Debug, Clone, Copy)]
struct Col {
    head: &'static str,
    cap: usize,
    drop: Option<u8>,
    shrink: Option<(u8, usize)>,
    grow: Option<u8>,
}

const fn col(head: &'static str, cap: usize) -> Col {
    Col {
        head,
        cap,
        drop: None,
        shrink: None,
        grow: None,
    }
}

const LIVE_COLS: [Col; 11] = [
    Col {
        shrink: Some((1, 6)),
        grow: Some(1),
        ..col("LIVE", 13)
    },
    Col {
        shrink: Some((0, 8)),
        grow: Some(0),
        ..col("", 16)
    },
    Col {
        drop: Some(4),
        ..col("model", 10)
    },
    Col {
        drop: Some(2),
        ..col("phase", 5)
    },
    Col {
        drop: Some(0),
        grow: Some(2),
        ..col("plan", 20)
    },
    Col {
        drop: Some(3),
        ..col("fresh", usize::MAX)
    },
    Col {
        drop: Some(1),
        ..col("c.read", usize::MAX)
    },
    col("out", usize::MAX),
    col("$", usize::MAX),
    col("tok/min", usize::MAX),
    col("state", usize::MAX),
];

const ROLLUP_COLS: [Col; 6] = [
    Col {
        shrink: Some((0, 10)),
        grow: Some(0),
        ..col("", 30)
    },
    col("tokens", usize::MAX),
    col("$", usize::MAX),
    col("share", usize::MAX),
    Col {
        drop: Some(1),
        ..col("cache hit", usize::MAX)
    },
    Col {
        drop: Some(0),
        ..col("$/DONE", usize::MAX)
    },
];

fn fit_columns(
    cols: &[Col],
    heads: &[String],
    rows: &[Vec<String>],
    width: usize,
) -> Vec<Option<usize>> {
    let natural: Vec<usize> = (0..cols.len())
        .map(|i| {
            let cells = rows.iter().map(|r| r[i].chars().count());
            let longest = cells.max().unwrap_or(0);
            if longest == 0 {
                0
            } else {
                longest.max(heads[i].chars().count())
            }
        })
        .collect();
    let mut w: Vec<Option<usize>> = cols
        .iter()
        .zip(&natural)
        .map(|(c, &n)| (n > 0).then(|| n.min(c.cap)))
        .collect();
    let total = |w: &[Option<usize>]| {
        let kept: Vec<usize> = w.iter().flatten().copied().collect();
        kept.iter().sum::<usize>() + kept.len().saturating_sub(1)
    };
    let ordered = |key: &dyn Fn(&Col) -> Option<u8>| {
        let mut o: Vec<(u8, usize)> = cols
            .iter()
            .enumerate()
            .filter_map(|(i, c)| key(c).map(|k| (k, i)))
            .collect();
        o.sort();
        o.into_iter().map(|(_, i)| i).collect::<Vec<_>>()
    };
    for i in ordered(&|c| c.drop) {
        if total(&w) <= width {
            break;
        }
        w[i] = None;
    }
    for i in ordered(&|c| c.shrink.map(|s| s.0)) {
        let over = total(&w).saturating_sub(width);
        if let (Some(c), Some((_, min))) = (w[i], cols[i].shrink) {
            w[i] = Some(c - over.min(c.saturating_sub(min)));
        }
    }
    for i in ordered(&|c| c.grow) {
        let spare = width.saturating_sub(total(&w));
        if let Some(c) = w[i] {
            w[i] = Some(c + spare.min(natural[i].saturating_sub(c)));
        }
    }
    w
}

fn table_line(cells: &[String], widths: &[Option<usize>], width: usize) -> String {
    let parts: Vec<String> = cells
        .iter()
        .zip(widths)
        .filter_map(|(c, w)| w.map(|w| pad(c, w)))
        .collect();
    clip(parts.join(" ").trim_end(), width)
}

fn live_cells(r: &LiveRow, project: String) -> Vec<String> {
    let mut role = r.role.clone();
    if let Some(via) = &r.via {
        role = format!("{role}>{via}");
    }
    let mut flags = Vec::new();
    if r.idle {
        flags.push("idle".to_string());
    }
    if matches!(
        r.pool_state.as_str(),
        "tight" | "exhausted" | "broken" | "cooling"
    ) {
        flags.push(format!("{}:{}", r.pool, r.pool_state));
    }
    vec![
        project,
        role,
        short_model(&r.model),
        short_phase(r.phase.as_deref()).into(),
        r.plan.clone().unwrap_or_else(|| "-".into()),
        human(r.tokens.fresh()),
        human(r.tokens.cache_read),
        human(r.tokens.output),
        money(r.cost_usd, r.unpriced_events),
        human(r.rate_tokens_per_min.round() as u64),
        flags.join(" "),
    ]
}

fn rollup_cells(r: &RollupRow, total: f64, key: String) -> Vec<String> {
    vec![
        key,
        human(r.tokens.total()),
        money(r.cost_usd, r.unpriced_events),
        if total > 0.0 {
            pct(r.cost_usd / total)
        } else {
            "-".into()
        },
        r.cache_hit.map(pct).unwrap_or_else(|| "-".into()),
        r.cost_per_done
            .map(|c| format!("{c:.2}"))
            .unwrap_or_else(|| "-".into()),
    ]
}

fn range(offset: usize, shown: usize, n: usize) -> String {
    if n == 0 {
        "rows 0 of 0".into()
    } else {
        format!("rows {}-{} of {n}", offset + 1, offset + shown)
    }
}

struct TableBody<'a> {
    title: &'a str,
    head: String,
    lines: Vec<(String, Style)>,
    note: Option<String>,
}

fn draw_table(
    buf: &mut Buffer,
    area: Rect,
    body: TableBody<'_>,
    table: &TableState,
    focused: bool,
) {
    let TableBody {
        title,
        head,
        lines,
        note,
    } = body;
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
    if n == 0 {
        if let Some(note) = note {
            text.push(Line::styled(
                note,
                Style::default().add_modifier(Modifier::DIM),
            ));
        }
    }
    ratatui::widgets::Widget::render(Paragraph::new(text), inner, buf);
}

fn pool_line(line: String, color: bool, at: (usize, usize)) -> Line<'static> {
    let state: String = line.chars().skip(at.0).take(at.1 - at.0).collect();
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
            let head: String = line.chars().take(at.0).collect();
            let tail: String = line.chars().skip(at.1).collect();
            Line::from(vec![
                Span::raw(head),
                Span::styled(state, Style::default().fg(fg)),
                Span::raw(tail),
            ])
        }
        _ => Line::from(line),
    }
}

pub fn draw(frame: &mut Frame, snap: &Snapshot, view: &ViewState, now: DateTime<Utc>) {
    let area = frame.area();
    draw_buf(frame.buffer_mut(), area, snap, view, now);
}

fn pool_paragraph(
    snap: &Snapshot,
    view: &ViewState,
    area: Rect,
    now: DateTime<Utc>,
) -> Vec<Line<'static>> {
    let (state_at, first) = if short(area) {
        ((COMPACT_STATE, COMPACT_STATE + 12), 0)
    } else {
        ((43, 55), 1)
    };
    pool_lines(snap, area, now)
        .into_iter()
        .enumerate()
        .map(|(i, l)| {
            if i < first {
                Line::from(l)
            } else {
                pool_line(l, view.color, state_at)
            }
        })
        .collect()
}

fn draw_live(
    buf: &mut Buffer,
    r: Rect,
    snap: &Snapshot,
    view: &ViewState,
    labels: &std::collections::BTreeMap<String, String>,
) {
    let width = buf.area.width as usize;
    let live = live_rows(snap, view);
    let cells: Vec<Vec<String>> = live
        .iter()
        .map(|r| live_cells(r, label(labels, r.project.as_deref())))
        .collect();
    let heads: Vec<String> = LIVE_COLS.iter().map(|c| c.head.to_string()).collect();
    let widths = fit_columns(&LIVE_COLS, &heads, &cells, width);
    let offset = view.tables[LIVE].offset;
    let mut last_project: Option<String> = None;
    let lines: Vec<(String, Style)> = live
        .iter()
        .zip(cells)
        .enumerate()
        .map(|(i, (r, mut cells))| {
            let project = std::mem::take(&mut cells[0]);
            if i == offset || last_project.as_deref() != Some(project.as_str()) {
                cells[0] = project.clone();
            }
            last_project = Some(project);
            let style = if r.idle {
                Style::default().add_modifier(Modifier::DIM)
            } else {
                Style::default()
            };
            (table_line(&cells, &widths, width), style)
        })
        .collect();
    draw_table(
        buf,
        r,
        TableBody {
            title: "live panes",
            head: table_line(&heads, &widths, width),
            lines,
            note: None,
        },
        &view.tables[LIVE],
        view.focus == LIVE,
    );
}

fn draw_rollup(
    buf: &mut Buffer,
    r: Rect,
    snap: &Snapshot,
    view: &ViewState,
    labels: &std::collections::BTreeMap<String, String>,
) {
    let width = buf.area.width as usize;

    let rows = rollup_rows(snap, view);
    let total: f64 = rows.iter().map(|r| r.cost_usd).sum();
    let title = format!("{} by {}", view.window_name(), view.group_name());
    let by_project = view.group_name() == "project";
    let cells: Vec<Vec<String>> = rows
        .iter()
        .map(|row| {
            let key = if by_project && row.key != "-" {
                label(labels, Some(&row.key))
            } else {
                row.key.clone()
            };
            rollup_cells(row, total, key)
        })
        .collect();
    let mut heads: Vec<String> = ROLLUP_COLS.iter().map(|c| c.head.to_string()).collect();
    heads[0] = title.to_uppercase();
    let widths = fit_columns(&ROLLUP_COLS, &heads, &cells, width);
    let lines = cells
        .iter()
        .map(|c| (table_line(c, &widths, width), Style::default()))
        .collect();
    let note = view
        .loading
        .then(|| format!("loading {}…", label(labels, view.project.as_deref())));
    draw_table(
        buf,
        r,
        TableBody {
            title: &title,
            head: table_line(&heads, &widths, width),
            lines,
            note,
        },
        &view.tables[ROLLUP],
        view.focus == ROLLUP,
    );
}

pub fn draw_buf(
    buf: &mut Buffer,
    area: Rect,
    snap: &Snapshot,
    view: &ViewState,
    now: DateTime<Utc>,
) {
    let width = area.width as usize;
    let a = areas(snap, view, area);
    let labels = project_labels(snap);
    let para = |buf: &mut Buffer, lines: Vec<Line<'static>>, r: Rect| {
        ratatui::widgets::Widget::render(Paragraph::new(lines), r, buf);
    };
    para(
        buf,
        vec![Line::from(header(snap, view, width, now))],
        a.header,
    );
    para(buf, pool_paragraph(snap, view, area, now), a.pools);
    draw_live(buf, a.live, snap, view, &labels);
    if let Some(r) = a.rollup {
        draw_rollup(buf, r, snap, view, &labels);
    }

    para(
        buf,
        fact_lines(snap, view, area)
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
        draw_help(buf, area);
    }
}

fn draw_help(buf: &mut Buffer, area: Rect) {
    let wide = |lines: &[&str]| lines.iter().map(|l| l.chars().count()).max().unwrap_or(0) + 4;
    let more: Vec<&str> = HELP_KEYS.iter().chain(HELP_MORE).copied().collect();
    let lines: &[&str] =
        if more.len() + 2 <= area.height as usize && wide(&more) <= area.width as usize {
            &more
        } else {
            HELP_KEYS
        };
    let w = (wide(lines) as u16).min(area.width);
    let h = (lines.len() as u16 + 2).min(area.height);
    let r = Rect::new(
        area.x + (area.width - w) / 2,
        area.y + (area.height - h) / 2,
        w,
        h,
    );
    ratatui::widgets::Widget::render(Clear, r, buf);
    let inner = (w as usize).saturating_sub(3);
    let text: Vec<Line> = lines
        .iter()
        .map(|l| Line::from(format!(" {}", clip(l, inner))))
        .collect();
    ratatui::widgets::Widget::render(
        Paragraph::new(text).block(Block::bordered().title(" help ")),
        r,
        buf,
    );
}

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

pub fn render(snap: &Snapshot, view: &ViewState, width: u16, height: u16) -> Vec<String> {
    let backend = TestBackend::new(width.max(MIN_SIZE.0), height.max(MIN_SIZE.1));
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

    #[test]
    fn spc_07_up_pageup_home_and_the_view_keys() {
        let r = rows(30, 30, 10);
        let mut s = ViewState::default();
        s.tables[LIVE].select(25, 30, 10);
        let up = press(s.clone(), KeyCode::Up, &r);
        assert_eq!(up.tables[LIVE].sel, 24);
        let pgup = press(s.clone(), KeyCode::PageUp, &r);
        assert_eq!(pgup.tables[LIVE].sel, 15);
        let home = press(s.clone(), KeyCode::Home, &r);
        assert_eq!((home.tables[LIVE].sel, home.tables[LIVE].offset), (0, 0));
        let k = press(s.clone(), KeyCode::Char('k'), &r);
        assert_eq!(k.tables[LIVE].sel, 24);

        let mut w = ViewState::with("teammate", "5h").unwrap();
        w.tables[ROLLUP].select(3, 30, 10);
        let g = press(w.clone(), KeyCode::Char('g'), &r);
        assert_eq!(g.group, (w.group + 1) % GROUPS.len());
        assert_eq!(g.tables[ROLLUP], TableState::default());
        let next = press(w.clone(), KeyCode::Char('w'), &r);
        assert_eq!(next.window, (w.window + 1) % VIEW_WINDOWS.len());
        assert_eq!(next.tables[ROLLUP], TableState::default());
        let live = ViewState {
            window: VIEW_WINDOWS.len() - 1,
            focus: ROLLUP,
            ..ViewState::default()
        };
        let wrapped = press(live, KeyCode::Char('w'), &r);
        assert_eq!((wrapped.window, wrapped.focus), (0, LIVE));

        let proj = ViewState {
            project: Some("/a".into()),
            ..ViewState::default()
        };
        let (cleared, a) = on_key(proj, key(KeyCode::Esc), &r);
        assert_eq!((cleared.project, a), (None, Action::SetProject(None)));
        let mut cycle = r.clone();
        cycle.projects = vec!["/a".into(), "/b".into()];
        let (first, a) = on_key(ViewState::default(), key(KeyCode::Char('p')), &cycle);
        assert_eq!(a, Action::SetProject(Some("/a".into())));
        let (last, a) = on_key(first, key(KeyCode::Char('p')), &cycle);
        let (none, b) = on_key(last, key(KeyCode::Char('p')), &cycle);
        assert_eq!(a, Action::SetProject(Some("/b".into())));
        assert_eq!((none.project, b), (None, Action::SetProject(None)));
    }

    #[test]
    fn spc_04_the_insight_line_lists_the_known_insights() {
        let snap = Snapshot {
            insights: horch_core::telemetry::collect::Insights {
                orchestrator_share: Some(0.5),
                cache_hit: None,
                idle_spend_share: Some(0.25),
                top_plan: Some("big".into()),
            },
            ..Snapshot::default()
        };
        assert_eq!(
            facts(&snap, &ViewState::default())
                .last()
                .map(String::as_str),
            Some("orchestrators 50% of spend · 25% of spend while idle · top plan big")
        );
        assert!(facts(&Snapshot::default(), &ViewState::default()).is_empty());
    }

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
        let s = press(press(s, KeyCode::Tab, &r), KeyCode::Char('w'), &r);
        assert_eq!((s.window_name(), s.focus), ("live", LIVE));
    }

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

    fn kind(code: KeyCode, kind: KeyEventKind) -> KeyEvent {
        KeyEvent::new_with_kind(code, KeyModifiers::NONE, kind)
    }

    #[test]
    fn spc_07_motion_keys_repeat_and_action_keys_do_not() {
        let r = rows(30, 0, 10);
        let mut s = ViewState::default();
        s = on_key(s, kind(KeyCode::Down, KeyEventKind::Press), &r).0;
        for _ in 0..3 {
            s = on_key(s, kind(KeyCode::Char('j'), KeyEventKind::Repeat), &r).0;
        }
        assert_eq!(s.tables[LIVE].sel, 4);
        let s = on_key(s, kind(KeyCode::PageDown, KeyEventKind::Repeat), &r).0;
        assert_eq!(s.tables[LIVE].sel, 14);
        let (s, a) = on_key(s, kind(KeyCode::Down, KeyEventKind::Release), &r);
        assert_eq!((s.tables[LIVE].sel, a), (14, Action::None));
        for code in ['p', 'w', 'g', '?', 'r', 'q'] {
            let (n, a) = on_key(
                s.clone(),
                kind(KeyCode::Char(code), KeyEventKind::Repeat),
                &r,
            );
            assert_eq!((&n, a), (&s, Action::None), "{code}");
        }
    }

    #[test]
    fn spc_04_the_help_is_modal() {
        let r = rows(30, 30, 10);
        let open = press(
            ViewState::with("teammate", "7d").unwrap(),
            KeyCode::Char('?'),
            &r,
        );
        assert!(open.help);
        for code in [
            KeyCode::Char('j'),
            KeyCode::Down,
            KeyCode::End,
            KeyCode::PageDown,
            KeyCode::Tab,
            KeyCode::Char('g'),
            KeyCode::Char('w'),
            KeyCode::Char('p'),
            KeyCode::Char('r'),
        ] {
            let (s, a) = on_key(open.clone(), key(code), &r);
            assert_eq!((&s, a), (&open, Action::None), "{code:?}");
        }
        let (s, a) = on_wheel(open.clone(), LIVE, true, &r);
        assert_eq!((&s, a), (&open, Action::None));
        assert_eq!(
            on_key(open.clone(), key(KeyCode::Char('q')), &r).1,
            Action::Quit
        );
        let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(on_key(open.clone(), ctrl_c, &r).1, Action::Quit);
        assert!(!press(open.clone(), KeyCode::Esc, &r).help);
        assert!(!press(open, KeyCode::Char('?'), &r).help);
    }

    #[test]
    fn spc_04_r_shows_refreshing() {
        let r = rows(1, 0, 20);
        let (s, a) = on_key(ViewState::default(), key(KeyCode::Char('r')), &r);
        assert_eq!(
            (s.status.as_deref(), a),
            (Some(REFRESHING), Action::Refresh)
        );
        let snap = fixture("snapshot-fixture.json");
        let text = draw_text(&snap, &s, 120, 40);
        assert!(text.contains("! refreshing…"), "{text}");
    }

    #[test]
    fn spc_07_the_wheel_keeps_a_visible_selection() {
        let r = rows(30, 0, 10);
        let mut s = ViewState::default();
        s.tables[LIVE].select(5, 30, 10);
        let s = on_wheel(s, LIVE, true, &r).0;
        assert_eq!((s.tables[LIVE].offset, s.tables[LIVE].sel), (3, 5));
        let s = on_wheel(s, LIVE, true, &r).0;
        assert_eq!((s.tables[LIVE].offset, s.tables[LIVE].sel), (6, 6));
        let s = on_wheel(s, LIVE, false, &r).0;
        assert_eq!((s.tables[LIVE].offset, s.tables[LIVE].sel), (3, 6));
    }

    fn fixture(name: &str) -> Snapshot {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/golden")
            .join(name);
        Snapshot::read(&path).unwrap()
    }

    fn draw_text(snap: &Snapshot, view: &ViewState, w: u16, h: u16) -> String {
        let area = Rect::new(0, 0, w, h);
        let mut buf = Buffer::empty(area);
        draw_buf(&mut buf, area, snap, view, snapshot_time(snap));
        buffer_text(&buf).join("\n")
    }

    #[test]
    fn spc_04_the_help_fits_60x12() {
        let snap = fixture("snapshot-fixture.json");
        let view = ViewState {
            help: true,
            ..ViewState::default()
        };
        let text = draw_text(&snap, &view, 60, 12);
        for line in HELP_KEYS {
            assert!(text.contains(line), "{line}\n{text}");
        }
        let lines: Vec<&str> = text.lines().collect();
        assert!(
            lines[0].contains("help") && lines[11].contains('┘'),
            "{text}"
        );
        let text = draw_text(&snap, &view, 120, 40);
        assert!(
            text.contains("windows:") && text.contains("groups:"),
            "{text}"
        );
    }

    #[test]
    fn spc_04_a_loading_project_shows_no_global_rollup() {
        let snap = fixture("snapshot-fixture.json");
        let view = ViewState {
            project: Some("/Users/x/other".into()),
            loading: true,
            ..ViewState::with("teammate", "7d").unwrap()
        };
        let text = draw_text(&snap, &view, 120, 40);
        assert!(text.contains("loading other…"), "{text}");
        assert!(text.contains("project: other"), "{text}");
        assert!(text.contains("7d by teammate  rows 0 of 0"), "{text}");
        assert!(!text.contains("412.00"), "{text}");
    }

    #[test]
    fn spc_04_columns_have_a_gutter() {
        let cells = vec!["lawn-mower-man".to_string(), "orchestrator".into()];
        let widths = [Some(14), Some(12)];
        assert_eq!(
            table_line(&cells, &widths, 80),
            "lawn-mower-man orchestrator"
        );
        let widths = [Some(10), Some(12)];
        assert_eq!(table_line(&cells, &widths, 80), "lawn-mowe… orchestrator");
    }

    #[test]
    fn spc_04_the_column_rule() {
        let heads: Vec<String> = LIVE_COLS.iter().map(|c| c.head.to_string()).collect();
        let row: Vec<String> = [
            "lawn-mower-manor-x",
            "architect-reviewer-12",
            "opus-5.5",
            "impl",
            "timeline-integration-wave7",
            "1.0M",
            "117M",
            "359k",
            "1234.56*",
            "47k",
            "idle claude:tight",
        ]
        .map(String::from)
        .to_vec();
        let rows = vec![row];
        let kept = |w: usize| {
            let widths = fit_columns(&LIVE_COLS, &heads, &rows, w);
            let names: Vec<&str> = widths
                .iter()
                .zip(&LIVE_COLS)
                .filter(|(w, _)| w.is_some())
                .map(|(_, c)| c.head)
                .collect();
            let total: usize = widths.iter().flatten().sum::<usize>() + names.len() - 1;
            (names, widths, total)
        };
        let (names, _, total) = kept(200);
        assert_eq!(names.len(), 11);
        assert_eq!(total, 135);
        let (names, w, total) = kept(120);
        assert_eq!((names.len(), w[1], total), (11, Some(17), 120));
        let (names, w, total) = kept(100);
        assert!(
            !names.contains(&"plan") && names.contains(&"c.read"),
            "{names:?}"
        );
        assert_eq!((w[1], total), (Some(18), 100));
        let (names, _, total) = kept(80);
        assert_eq!(names, ["LIVE", "", "model", "out", "$", "tok/min", "state"]);
        assert_eq!(total, 80);
        let (names, w, total) = kept(60);
        assert_eq!(names, ["LIVE", "", "out", "$", "tok/min", "state"]);
        assert_eq!((w[0], w[1], total), (Some(11), Some(8), 60));
    }
}
