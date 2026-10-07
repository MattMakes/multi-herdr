//! `horch telemetry` - the fleet telemetry space (design section 12).
//!
//! One collector per state root holds `telemetry/collector.lock`; every other
//! `horch telemetry` is a read-only viewer of the files it writes (SPC-01,
//! SPC-02). `ensure` opens the collector in its own herdr workspace without
//! touching anything the operator can see (SPC-03).
//!
//! The screen is plain text: [`render`] turns a snapshot into lines, which
//! is what the goldens compare and what `horch telemetry render` prints.
//! `crossterm` only puts those lines on a terminal and reads keys.

use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};
use horch_core::clock;
use horch_core::routing::quota::{self, PoolReading, POOLS};
use horch_core::runtime::RuntimeContext;
use horch_core::telemetry::collect::{self, Collector, LiveRow, Probing, Snapshot};
use horch_core::telemetry::lock::{self, Holder};
use horch_core::telemetry::store::{self, RollupRow, GROUPS};
use horch_core::usage::Locations;
use horch_core::workspace::herdr::Herdr;

use crate::output;

/// The label of the telemetry workspace. `horch tile` never touches it.
pub const WORKSPACE_LABEL: &str = "horch telemetry";

/// The windows the `w` key cycles through.
pub const VIEW_WINDOWS: [&str; 4] = ["live", "5h", "today", "7d"];

static SHUTDOWN: AtomicBool = AtomicBool::new(false);

#[cfg(unix)]
extern "C" fn on_signal(_: libc::c_int) {
    SHUTDOWN.store(true, Ordering::SeqCst);
}

/// SIGINT and SIGTERM end the loop, so the lock is released on the way out.
fn install_signal_handlers() {
    #[cfg(unix)]
    unsafe {
        let handler = on_signal as extern "C" fn(libc::c_int) as libc::sighandler_t;
        libc::signal(libc::SIGTERM, handler);
        libc::signal(libc::SIGINT, handler);
    }
}

// ─── view state and text ────────────────────────────────────────────────────

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
}

/// `412k`, `9.8M`, `88M`: a token count in at most 5 characters.
pub fn human(n: u64) -> String {
    let f = n as f64;
    if n >= 10_000_000 {
        format!("{:.0}M", f / 1e6)
    } else if n >= 1_000_000 {
        format!("{:.1}M", f / 1e6)
    } else if n >= 10_000 {
        format!("{:.0}k", f / 1e3)
    } else if n >= 1_000 {
        format!("{:.1}k", f / 1e3)
    } else {
        n.to_string()
    }
}

/// `s` in at most `n` characters, with `…` when cut.
pub fn clip(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        return s.to_string();
    }
    let mut out: String = s.chars().take(n.saturating_sub(1)).collect();
    out.push('…');
    out
}

fn pad(s: &str, n: usize) -> String {
    let s = clip(s, n);
    let len = s.chars().count();
    format!("{s}{}", " ".repeat(n.saturating_sub(len)))
}

/// `x` as a whole percent. Adding `0.0` turns a rounded `-0` into `0`.
fn pct(x: f64) -> String {
    format!("{:.0}%", (x * 100.0).round() + 0.0)
}

/// The POOL block (section 12.4), shared by the screen and `horch quota`.
/// A pool in `only_with_reading` gets a row only when it has a reading.
pub fn pool_table(
    pools: &std::collections::BTreeMap<String, PoolReading>,
    now: DateTime<Utc>,
    width: usize,
    only_with_reading: &[&str],
) -> Vec<String> {
    let mut out = vec![clip(
        &format!(
            "{}{}{}{}{}{}{}",
            pad("POOL", 14),
            pad("5h", 8),
            pad("7d", 7),
            pad("scoped", 14),
            pad("state", 12),
            pad("resets", 15),
            "headroom/h"
        ),
        width,
    )];
    let mut names: Vec<&str> = POOLS
        .into_iter()
        .filter(|p| !only_with_reading.contains(p) || pools.contains_key(*p))
        .collect();
    for extra in pools.keys() {
        if !names.contains(&extra.as_str()) {
            names.push(extra);
        }
    }
    for name in names {
        let Some(r) = pools.get(name) else {
            out.push(clip(&format!("{}no reading", pad(name, 14)), width));
            continue;
        };
        let state = if r.state.is_empty() {
            "unknown"
        } else {
            r.state.as_str()
        };
        let shown = if state == "ok" {
            "ok".to_string()
        } else {
            state.to_ascii_uppercase()
        };
        let line = if name == quota::POOL_ZEN && r.windows.is_empty() {
            let (s, note) = if state == "ok" {
                ("ok*".to_string(), "* public work only")
            } else {
                (shown, "")
            };
            let resets = r
                .cooling_until
                .as_deref()
                .map(quota::short_time)
                .unwrap_or_else(|| "-".into());
            format!(
                "{}{}{}{}{}{}",
                pad(name, 14),
                pad("no signal", 29),
                pad(&s, 12),
                pad(&resets, 15),
                pad("-", 11),
                note
            )
        } else if r.windows.is_empty() {
            let why = r
                .error
                .clone()
                .or_else(|| r.reason.clone())
                .unwrap_or_else(|| "-".into());
            format!(
                "{}{}{}{}{}{}",
                pad(name, 14),
                pad("-", 8),
                pad("-", 7),
                pad("-", 14),
                pad(&shown, 12),
                why
            )
        } else {
            let unscoped = |mins: u64| {
                r.windows
                    .iter()
                    .find(|w| w.minutes == mins && w.scope_model.is_none())
                    .map(|w| pct(w.used_at(now)))
                    .unwrap_or_else(|| "-".into())
            };
            let scoped = r
                .windows
                .iter()
                .filter(|w| w.scope_model.is_some())
                .max_by(|a, b| {
                    a.used_at(now)
                        .partial_cmp(&b.used_at(now))
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .map(|w| {
                    format!(
                        "{} {}",
                        w.scope_model.as_deref().unwrap_or_default(),
                        pct(w.used_at(now))
                    )
                })
                .unwrap_or_else(|| "-".into());
            let worst = r
                .windows
                .iter()
                .filter(|w| w.scope_model.is_none())
                .max_by(|a, b| {
                    a.used_at(now)
                        .partial_cmp(&b.used_at(now))
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
            let resets = worst
                .and_then(|w| w.resets_at.as_deref())
                .map(quota::short_time)
                .unwrap_or_else(|| "-".into());
            let headroom = r
                .windows
                .iter()
                .filter(|w| w.scope_model.is_none())
                .map(|w| w.headroom_per_h(now))
                .fold(f64::INFINITY, f64::min);
            let headroom = if headroom.is_finite() {
                format!("{:.1}%", headroom * 100.0)
            } else {
                "-".into()
            };
            format!(
                "{}{}{}{}{}{}{}",
                pad(name, 14),
                pad(&unscoped(300), 8),
                pad(&unscoped(10_080), 7),
                pad(&scoped, 14),
                pad(&shown, 12),
                pad(&resets, 15),
                headroom
            )
        };
        out.push(clip(line.trim_end(), width));
    }
    out
}

fn short_model(model: &str) -> String {
    let m = model.rsplit('/').next().unwrap_or(model);
    let m = m.strip_prefix("claude-").unwrap_or(m);
    let m = m.strip_prefix("gpt-").unwrap_or(m);
    // opus-5-5 -> opus-5.5
    match m.split_once('-') {
        Some((fam, ver)) if ver.chars().all(|c| c.is_ascii_digit() || c == '-') => {
            format!("{fam}-{}", ver.replace('-', "."))
        }
        _ => m.to_string(),
    }
}

fn short_phase(p: Option<&str>) -> &'static str {
    match p {
        Some("research") => "res",
        Some("plan") => "plan",
        Some("implementation") => "impl",
        Some("validation") => "val",
        _ => "-",
    }
}

fn project_name(p: Option<&str>) -> String {
    p.and_then(|p| {
        Path::new(p)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
    })
    .unwrap_or_else(|| "-".into())
}

fn live_lines(rows: &[&LiveRow], width: usize, max: usize) -> Vec<String> {
    let mut out = vec![clip(
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
    )];
    let mut last_project: Option<String> = None;
    for (i, r) in rows.iter().enumerate() {
        if i >= max {
            out.push(format!("… {} more", rows.len() - i));
            break;
        }
        let project = project_name(r.project.as_deref());
        let shown = if last_project.as_deref() == Some(project.as_str()) {
            String::new()
        } else {
            project.clone()
        };
        last_project = Some(project);
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
        out.push(clip(
            &format!(
                "{}{}{}{}{}{}{}{}{}{}{}",
                pad(&shown, 14),
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
        ));
    }
    out
}

/// Dollars, with a `*` when some events have no price: their cost is not
/// in the figure, and the `UNPRICED` line lists them (never $0).
fn money(cost: f64, unpriced: u64) -> String {
    if unpriced > 0 {
        format!("{cost:.2}*")
    } else {
        format!("{cost:.2}")
    }
}

/// The `UNPRICED` line for one window: the events whose model has no price.
fn unpriced_line(rows: &[store::UnpricedRow], width: usize) -> Option<String> {
    if rows.is_empty() {
        return None;
    }
    let events: u64 = rows.iter().map(|u| u.events).sum();
    let tokens: u64 = rows.iter().map(|u| u.tokens.total()).sum();
    let models: Vec<String> = rows
        .iter()
        .map(|u| format!("{} {}", u.model, u.events))
        .collect();
    Some(clip(
        &format!(
            "* UNPRICED  plus {events} event(s), {} tokens, not in $: {}",
            human(tokens),
            models.join(", ")
        ),
        width,
    ))
}

fn rollup_lines(title: &str, rows: &[RollupRow], width: usize, max: usize) -> Vec<String> {
    let total: f64 = rows.iter().map(|r| r.cost_usd).sum();
    let mut out = vec![clip(
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
    )];
    for (i, r) in rows.iter().enumerate() {
        if i >= max {
            out.push(format!("… {} more", rows.len() - i));
            break;
        }
        out.push(clip(
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
        ));
    }
    out
}

/// One frame of the screen, as lines of at most `width` characters, at most
/// `height` of them (SPC-04). Pure.
pub fn render(snap: &Snapshot, view: &ViewState, width: u16, height: u16) -> Vec<String> {
    let (width, height) = (width.max(20) as usize, height.max(6) as usize);
    let now = clock::parse(&snap.generated_at).unwrap_or_else(clock::now);
    let live: Vec<&LiveRow> = snap
        .live
        .iter()
        .filter(|r| view.project.is_none() || r.project == view.project)
        .collect();
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
    let mut right = format!(
        "group: {}   window: {}",
        view.group_name(),
        view.window_name()
    );
    if let Some(p) = &view.project {
        right = format!("project: {}   {right}", project_name(Some(p)));
    }
    // The left part gives way first: the view's group and window must show.
    let left = clip(&left, width.saturating_sub(right.chars().count() + 2));
    let gap = width
        .saturating_sub(left.chars().count() + right.chars().count())
        .max(2);
    let mut lines = vec![clip(&format!("{left}{}{right}", " ".repeat(gap)), width)];
    // No probe reads the Google pool yet, so the screen shows it only when
    // a reading exists; `horch quota` always lists it.
    lines.extend(pool_table(&snap.pools, now, width, &[quota::POOL_GOOGLE]));
    lines.push(String::new());

    // What is left is shared by the live rows and the rollup.
    let mut tail: Vec<String> = Vec::new();
    if !snap.unread.is_empty() {
        let items: Vec<String> = snap
            .unread
            .iter()
            .map(|u| format!("{}: {}", u.role, u.reason))
            .collect();
        tail.push(clip(&format!("UNREAD  {}", items.join("; ")), width));
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
        tail.push(clip(&facts.join(" · "), width));
    }
    if let Some(s) = &view.status {
        tail.push(clip(&format!("! {s}"), width));
    }
    let window = view.window_name();
    // The live view counts the last 5 hours' unpriced events.
    let priced_window = if window == "live" { "5h" } else { window };
    if let Some(line) = snap
        .rollups
        .get(priced_window)
        .and_then(|r| unpriced_line(&r.unpriced, width))
    {
        tail.push(line);
    }

    let room = height.saturating_sub(lines.len() + tail.len());
    if window == "live" {
        lines.extend(live_lines(&live, width, room.saturating_sub(1)));
    } else {
        let live_room = (room / 2).max(2);
        let shown = live_lines(&live, width, live_room.saturating_sub(1));
        let used = shown.len();
        lines.extend(shown);
        lines.push(String::new());
        let rows = snap
            .rollups
            .get(window)
            .and_then(|r| r.group(view.group_name()))
            .unwrap_or(&[]);
        let title = format!("{window} by {}", view.group_name());
        let rest = room.saturating_sub(used + 2);
        lines.extend(rollup_lines(&title, rows, width, rest));
    }
    lines.truncate(height.saturating_sub(tail.len()));
    lines.extend(tail);
    lines.truncate(height);
    lines
}

// ─── commands ───────────────────────────────────────────────────────────────

pub enum TelemetryCommand {
    Run,
    Ensure,
    CollectOnce,
    Render {
        snapshot: Option<String>,
        size: String,
        group: String,
        window: String,
    },
}

pub fn run(ctx: &RuntimeContext, command: TelemetryCommand) -> Result<ExitCode> {
    match command {
        TelemetryCommand::Run => space(ctx),
        TelemetryCommand::Ensure => {
            ensure(ctx, &Herdr::with_bin(&ctx.bins.harness.herdr), false)?;
            Ok(ExitCode::SUCCESS)
        }
        TelemetryCommand::CollectOnce => collect_once(ctx),
        TelemetryCommand::Render {
            snapshot,
            size,
            group,
            window,
        } => {
            let path = snapshot
                .map(PathBuf::from)
                .unwrap_or_else(|| collect::snapshot_path(&ctx.paths.state_root));
            let snap = Snapshot::read(&path)?;
            let (w, h) = size
                .split_once('x')
                .and_then(|(w, h)| Some((w.parse::<u16>().ok()?, h.parse::<u16>().ok()?)))
                .with_context(|| format!("--size takes WIDTHxHEIGHT, e.g. 120x40, not '{size}'"))?;
            let view = ViewState::with(&group, &window)?;
            output::println(&render(&snap, &view, w, h).join("\n"));
            Ok(ExitCode::SUCCESS)
        }
    }
}

/// `horch telemetry collect --once`: one tick, no screen. Exit 2 when a live
/// collector holds the lock.
fn collect_once(ctx: &RuntimeContext) -> Result<ExitCode> {
    let root = ctx.paths.state_root.clone();
    let now = clock::now();
    let held = match lock::acquire(&root, &lock::this_process(&clock::stamp(now), ctx))? {
        Ok(held) => held,
        Err(other) => {
            eprintln!(
                "horch: a live collector (pid {}) holds the telemetry lock",
                other.pid
            );
            return Ok(ExitCode::from(horch::exit::COLLECTOR_HELD));
        }
    };
    let mut c = Collector::open_in(ctx, Locations::from_context(ctx), Probing::Scheduled, now)?;
    let snap = c.tick(now);
    held.release();
    let snap = snap?;
    eprintln!(
        "telemetry: 1 tick, {} live pane(s), {} unread record(s)",
        snap.live.len(),
        snap.unread.len()
    );
    Ok(ExitCode::SUCCESS)
}

/// Where a snapshot comes from on each refresh. The collector variant holds
/// the lock for as long as it lives. `_lock` is never read; its `Drop`
/// releases the lock.
enum Source {
    Collector {
        collector: Box<Collector>,
        _lock: lock::CollectorLock,
    },
    Viewer,
}

impl Source {
    fn refresh(&mut self, ctx: &RuntimeContext, root: &Path) -> Result<Snapshot> {
        // A viewer whose collector died takes over (the next `ensure` would).
        if matches!(self, Source::Viewer) && !lock::collector_live(root) {
            if let Ok(Ok(held)) = lock::acquire(root, &lock::this_process(&clock::now_stamp(), ctx))
            {
                let c = Collector::open_in(
                    ctx,
                    Locations::from_context(ctx),
                    Probing::Scheduled,
                    clock::now(),
                )?;
                *self = Source::Collector {
                    collector: Box::new(c),
                    _lock: held,
                };
            }
        }
        match self {
            Source::Collector { collector, .. } => collector.tick(clock::now()),
            Source::Viewer => Snapshot::read(&collect::snapshot_path(root)),
        }
    }
}

/// `horch telemetry`: the collector with its screen, or a viewer.
fn space(ctx: &RuntimeContext) -> Result<ExitCode> {
    install_signal_handlers();
    let root = ctx.paths.state_root.clone();
    let me = lock::this_process(&clock::now_stamp(), ctx);
    let mut source = match lock::acquire(&root, &me)? {
        Ok(held) => {
            let c = Collector::open_in(
                ctx,
                Locations::from_context(ctx),
                Probing::Scheduled,
                clock::now(),
            )?;
            Source::Collector {
                collector: Box::new(c),
                _lock: held,
            }
        }
        Err(other) => {
            eprintln!(
                "horch: collector pid {} is live; this is a read-only viewer",
                other.pid
            );
            Source::Viewer
        }
    };
    let tick = Duration::from_millis(
        super::quotacmd::load_policy(ctx, &root)
            .map(|p| p.tick_ms)
            .unwrap_or(2000),
    );
    if std::io::stdout().is_terminal() {
        screen(ctx, &root, &mut source, tick)?;
    } else {
        // No terminal (a pane started by a test, or output to a file): keep
        // collecting until told to stop.
        while !SHUTDOWN.load(Ordering::SeqCst) {
            if let Err(e) = source.refresh(ctx, &root) {
                eprintln!("horch telemetry: {e:#}");
            }
            sleep_until_shutdown(tick);
        }
    }
    drop(source);
    Ok(ExitCode::SUCCESS)
}

fn sleep_until_shutdown(d: Duration) {
    let until = Instant::now() + d;
    while Instant::now() < until && !SHUTDOWN.load(Ordering::SeqCst) {
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Restores the terminal however the screen loop ends.
struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = crossterm::terminal::disable_raw_mode();
        let _ = crossterm::execute!(
            std::io::stdout(),
            crossterm::cursor::Show,
            crossterm::terminal::LeaveAlternateScreen
        );
    }
}

fn screen(ctx: &RuntimeContext, root: &Path, source: &mut Source, tick: Duration) -> Result<()> {
    use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};

    crossterm::terminal::enable_raw_mode()?;
    let _guard = TerminalGuard;
    crossterm::execute!(
        std::io::stdout(),
        crossterm::terminal::EnterAlternateScreen,
        crossterm::cursor::Hide
    )?;
    let mut view = ViewState::default();
    let mut snap = source.refresh(ctx, root).unwrap_or_default();
    let mut last = Instant::now();
    loop {
        let (w, h) = crossterm::terminal::size().unwrap_or((120, 40));
        let shown = filtered(&snap, &view, root);
        let frame = render(&shown, &view, w, h);
        let mut out = std::io::stdout().lock();
        crossterm::queue!(
            out,
            crossterm::cursor::MoveTo(0, 0),
            crossterm::terminal::Clear(crossterm::terminal::ClearType::All)
        )?;
        out.write_all(frame.join("\r\n").as_bytes())?;
        out.flush()?;
        drop(out);

        let wait = tick
            .saturating_sub(last.elapsed())
            .min(Duration::from_millis(250));
        if event::poll(wait)? {
            if let Event::Key(k) = event::read()? {
                if k.kind == KeyEventKind::Press {
                    match k.code {
                        KeyCode::Char('q') => break,
                        KeyCode::Char('c') if k.modifiers.contains(KeyModifiers::CONTROL) => break,
                        KeyCode::Char('g') => view.group = (view.group + 1) % GROUPS.len(),
                        KeyCode::Char('w') => view.window = (view.window + 1) % VIEW_WINDOWS.len(),
                        KeyCode::Char('p') => {
                            view.project = next_project(&snap, view.project.take())
                        }
                        _ => {}
                    }
                }
            }
        }
        if SHUTDOWN.load(Ordering::SeqCst) {
            break;
        }
        if last.elapsed() >= tick {
            match source.refresh(ctx, root) {
                Ok(s) => {
                    snap = s;
                    view.status = None;
                }
                Err(e) => view.status = Some(format!("{e:#}")),
            }
            last = Instant::now();
        }
    }
    Ok(())
}

/// The projects the `p` key cycles through: none (all), then each one.
fn next_project(snap: &Snapshot, current: Option<String>) -> Option<String> {
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
    match current {
        None => projects.into_iter().next(),
        Some(c) => projects.into_iter().skip_while(|p| *p != c).nth(1),
    }
}

/// The snapshot with its rollups recomputed for one project, from the event
/// files, when the project filter is on.
fn filtered(snap: &Snapshot, view: &ViewState, root: &Path) -> Snapshot {
    let Some(project) = &view.project else {
        return snap.clone();
    };
    let events: Vec<_> = store::read_all(&horch_core::telemetry::dir(root))
        .into_iter()
        .filter(|e| e.project.as_deref() == Some(project.as_str()))
        .collect();
    let done = collect::done_record_ids(&collect::read_ledgers(root));
    let now = clock::parse(&snap.generated_at).unwrap_or_else(clock::now);
    let mut out = snap.clone();
    for w in collect::WINDOWS {
        let since = collect::window_start(w, now);
        out.rollups.insert(
            w.to_string(),
            store::rollup(&events, since.as_deref(), &done),
        );
    }
    out
}

/// `horch telemetry ensure` (SPC-03): nothing when a live collector runs
/// this binary; else a new workspace, never focused, running
/// `horch telemetry`. A live collector on another binary (an older build)
/// is stopped first. Besides read-only list calls, only `workspace create
/// --no-focus`, `pane run` and the `workspace close` of an old collector
/// workspace touch herdr (W17).
pub fn ensure(ctx: &RuntimeContext, herdr: &Herdr, quiet: bool) -> Result<()> {
    ensure_current(ctx, herdr, quiet, &ctx.bins.exe()?, true)
}

/// After `horch install`: restart a live collector that does not run the
/// binary just installed at `installed`. Starts none when none is live.
pub fn restart_if_stale(ctx: &RuntimeContext, herdr: &Herdr, installed: &Path) -> Result<()> {
    ensure_current(ctx, herdr, false, installed, false)
}

/// The one check behind [`ensure`] and [`restart_if_stale`]: a live
/// collector whose recorded binary is not `exe` is stopped (by pid and start
/// time, [`lock::stop`]) and a new one runs `exe`. A collector that cannot
/// be stopped safely is reported live, with a warning, and left running.
fn ensure_current(
    ctx: &RuntimeContext,
    herdr: &Herdr,
    quiet: bool,
    exe: &Path,
    start_if_none: bool,
) -> Result<()> {
    let root = ctx.paths.state_root.clone();
    match lock::holder(&root) {
        Holder::Live(info) if lock::runs_other_binary(&info, lock::exe_identity(exe).as_ref()) => {
            // A collector horch cannot prove it owns (no recorded start time,
            // a reused pid) is left running: `ensure` runs on fleet start and
            // must not fail because an old collector is live.
            if let Err(e) = lock::stop(&root, &info, Duration::from_secs(5)) {
                eprintln!(
                    "horch: telemetry collector pid {} runs another binary, but horch did not stop it: {e:#}. To replace it, first check that pid {} runs `horch telemetry collect` (`ps -p {} -o command=`), then stop it, and run `horch telemetry ensure` again.",
                    info.pid, info.pid, info.pid
                );
                report_live(&info, quiet);
                return Ok(());
            }
            output::println(&format!(
                "stopped telemetry collector pid {}: it ran {}, not the build at {}",
                info.pid,
                info.exe
                    .as_ref()
                    .map(|e| format!("another build of {}", e.path))
                    .unwrap_or_else(|| "a binary it did not record".into()),
                exe.display()
            ));
        }
        Holder::Live(info) => {
            report_live(&info, quiet);
            return Ok(());
        }
        _ if !start_if_none => return Ok(()),
        _ => {}
    }
    start(ctx, herdr, quiet, exe)
}

fn report_live(info: &lock::LockInfo, quiet: bool) {
    if !quiet {
        output::println(&format!(
            "telemetry collector is live: pid {}{}{}",
            info.pid,
            info.workspace_id
                .as_deref()
                .map(|w| format!(", workspace {w}"))
                .unwrap_or_default(),
            info.pane_id
                .as_deref()
                .map(|p| format!(", pane {p}"))
                .unwrap_or_default(),
        ));
    }
}

/// Where the live collector runs, as far as herdr can tell.
#[derive(Debug, Clone, PartialEq, Eq)]
enum LiveIn {
    /// No live collector.
    Nothing,
    /// The live collector's pane is in this workspace.
    Workspace(String),
    /// A live collector whose workspace is not known (it recorded no pane,
    /// or herdr does not know it). No telemetry workspace can be proved free.
    Unknown,
}

fn live_collector_in(herdr: &Herdr, root: &Path) -> LiveIn {
    let Holder::Live(info) = lock::holder(root) else {
        return LiveIn::Nothing;
    };
    let Some(pane) = info.pane_id else {
        return LiveIn::Unknown;
    };
    // herdr pane ids are `<workspace>:p<n>`: a pane herdr no longer reports
    // still names its workspace.
    let ws = herdr
        .pane_get(&pane)
        .ok()
        .and_then(|p| p.workspace_id)
        .or(info.workspace_id)
        .or_else(|| pane.split_once(':').map(|(w, _)| w.to_string()));
    ws.map(LiveIn::Workspace).unwrap_or(LiveIn::Unknown)
}

/// The workspaces labelled [`WORKSPACE_LABEL`] that hold no live collector:
/// a collector that died or was stopped leaves its workspace with its pane at
/// a shell prompt. A workspace with another label is never one of them.
fn old_collector_workspaces(
    listed: &[horch_core::workspace::model::Workspace],
    live: &LiveIn,
) -> Vec<String> {
    if *live == LiveIn::Unknown {
        return Vec::new();
    }
    listed
        .iter()
        .filter(|w| w.label.as_deref() == Some(WORKSPACE_LABEL))
        .filter(|w| *live != LiveIn::Workspace(w.workspace_id.clone()))
        .map(|w| w.workspace_id.clone())
        .collect()
}

/// Wait up to 5 s for a live collector; its pid.
fn wait_for_collector(root: &Path) -> Option<u32> {
    let until = Instant::now() + Duration::from_secs(5);
    while Instant::now() < until {
        if let Holder::Live(info) = lock::holder(root) {
            if info.pid != 0 {
                return Some(info.pid);
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    None
}

/// `exe telemetry` in the telemetry workspace, never focused. A restart
/// leaves 1 telemetry workspace: an old collector workspace with 1 pane is
/// reused, and every other old one is closed once the new collector holds
/// the lock.
fn start(ctx: &RuntimeContext, herdr: &Herdr, quiet: bool, exe: &Path) -> Result<()> {
    let root = ctx.paths.state_root.clone();
    let command = collector_command(ctx, exe);
    let old = herdr
        .workspace_list()
        .map(|all| old_collector_workspaces(&all, &live_collector_in(herdr, &root)))
        .unwrap_or_default();
    let mut started = None;
    if let Some((ws, pane)) = old.iter().find_map(|ws| match herdr.pane_list(ws) {
        Ok(panes) if panes.len() == 1 => Some((ws.clone(), panes[0].pane_id.clone())),
        _ => None,
    }) {
        if herdr.pane_run(&pane, &command).is_ok() {
            started = wait_for_collector(&root).map(|pid| (pid, ws.clone()));
        }
        if started.is_none() {
            eprintln!(
                "horch: the telemetry collector did not start in the old workspace {ws}; starting it in a new one"
            );
        }
    }
    let (pid, ws) = match started {
        Some(s) => s,
        None => {
            let ws = herdr
                .workspace_create(WORKSPACE_LABEL, None, false)
                .context("creating the telemetry workspace")?;
            herdr.pane_run(&ws.root_pane_id, &command)?;
            let Some(pid) = wait_for_collector(&root) else {
                bail!(
                    "the telemetry collector did not start within 5s in workspace {}",
                    ws.workspace_id
                )
            };
            (pid, ws.workspace_id)
        }
    };
    if !quiet {
        output::println(&format!(
            "telemetry collector started: pid {pid}, workspace {ws}"
        ));
    }
    close_old_workspaces(herdr, &root, &old, &ws, quiet);
    Ok(())
}

/// Close the old collector workspaces in `old`, except `keep`. Each one is
/// checked again first: it still has the telemetry label, and the live
/// collector does not run in it.
fn close_old_workspaces(herdr: &Herdr, root: &Path, old: &[String], keep: &str, quiet: bool) {
    if old.iter().all(|w| w == keep) {
        return;
    }
    let Ok(all) = herdr.workspace_list() else {
        return;
    };
    for ws in old_collector_workspaces(&all, &live_collector_in(herdr, root)) {
        if ws == keep || !old.contains(&ws) {
            continue;
        }
        match herdr.workspace_close(&ws) {
            Ok(()) if !quiet => {
                output::println(&format!("closed the old telemetry workspace {ws}"))
            }
            Ok(()) => {}
            Err(e) => eprintln!("horch: could not close the old telemetry workspace {ws}: {e:#}"),
        }
    }
}

/// The shell line that runs `exe telemetry` in a pane.
fn collector_command(ctx: &RuntimeContext, exe: &Path) -> String {
    let mut args = vec!["telemetry".to_string()];
    // A pane does not inherit this process's environment.
    if let Some(dir) = super::path_text(ctx.paths.state_override.as_deref()) {
        args.push("--state-dir".into());
        args.push(dir);
    }
    let data_root = ctx.paths.data_root.to_string_lossy();
    horch_core::workspace::paneshell::PaneShell::host().command_line_with_env(
        exe,
        &[("HORCH_DATA_DIR", data_root.as_ref())],
        &args,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tiny negative share rounds to zero and prints `0%`, never `-0%`.
    #[test]
    fn pct_never_prints_a_negative_zero() {
        assert_eq!(pct(-0.0004), "0%");
        assert_eq!(pct(-0.0), "0%");
        assert_eq!(pct(0.0), "0%");
        assert_eq!(pct(0.256), "26%");
        assert_eq!(pct(-0.25), "-25%");
    }

    /// `horch quota` lists the Google pool always; the screen lists it only
    /// when it has a reading.
    #[test]
    fn google_pool_row_only_with_a_reading_on_the_screen() {
        let mut pools = std::collections::BTreeMap::new();
        let now = Utc::now();
        let has_google = |rows: &[String]| rows.iter().any(|r| r.starts_with("google "));
        assert!(has_google(&pool_table(&pools, now, 100, &[])));
        assert!(!has_google(&pool_table(
            &pools,
            now,
            100,
            &[quota::POOL_GOOGLE]
        )));
        pools.insert(quota::POOL_GOOGLE.to_string(), PoolReading::default());
        assert!(has_google(&pool_table(
            &pools,
            now,
            100,
            &[quota::POOL_GOOGLE]
        )));
    }

    fn fixture_snapshot() -> Snapshot {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/snapshot-fixture.json");
        Snapshot::read(&path).unwrap()
    }

    /// Compare a frame with its golden. `HORCH_BLESS=1` writes the golden
    /// instead, for a deliberate layout change; review the diff it leaves.
    fn check_golden(name: &str, frame: &str) {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/golden")
            .join(name);
        if std::env::var("HORCH_BLESS").ok().as_deref() == Some("1") {
            std::fs::write(&path, frame).unwrap();
            return;
        }
        let want = std::fs::read_to_string(&path).unwrap_or_else(|e| {
            panic!(
                "{}: {e}; run with HORCH_BLESS=1 once and review it",
                path.display()
            )
        });
        assert_eq!(frame, want, "{name}");
    }

    /// Never zero the unpriced (W9): a row with unpriced events marks its
    /// cost with `*`, and the window's `UNPRICED` line counts the events,
    /// their tokens and their models.
    #[test]
    fn tel_10_the_screen_lists_unpriced_events() {
        let mut snap = fixture_snapshot();
        let today = snap.rollups.get_mut("today").unwrap();
        today.by_teammate[0].unpriced_events = 3;
        today.unpriced = vec![store::UnpricedRow {
            model: "codex-auto-review".into(),
            events: 3,
            tokens: horch_core::telemetry::TokenClasses {
                input: 12_000,
                ..Default::default()
            },
        }];
        let cost = format!("{:.2}*", today.by_teammate[0].cost_usd);
        let frame = render(
            &snap,
            &ViewState::with("teammate", "today").unwrap(),
            120,
            40,
        );
        let text = frame.join("\n");
        assert!(
            text.contains("* UNPRICED  plus 3 event(s), 12k tokens, not in $: codex-auto-review 3"),
            "{text}"
        );
        assert!(text.contains(&cost), "{text}");
        let plain = render(
            &fixture_snapshot(),
            &ViewState::with("teammate", "today").unwrap(),
            120,
            40,
        );
        assert!(!plain.join("\n").contains("UNPRICED"));
    }

    /// A fake collector: a sleeping child recorded as the live lock holder
    /// with the binary `exe`, and a thread that reaps it.
    #[cfg(unix)]
    fn fake_collector(
        state: &Path,
        exe: Option<lock::ExeIdentity>,
    ) -> (u32, std::sync::mpsc::Receiver<()>) {
        let mut child = std::process::Command::new("/bin/sleep")
            .arg("30")
            .spawn()
            .unwrap();
        let pid = child.id();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = child.wait();
            let _ = tx.send(());
        });
        let tel = horch_core::telemetry::dir(state);
        std::fs::create_dir_all(tel.join("collector.lock")).unwrap();
        let info = lock::LockInfo {
            pid,
            started_at: "2026-10-04T15:39:46Z".into(),
            pid_start: horch_core::procid::start_time(pid),
            exe,
            ..lock::LockInfo::default()
        };
        std::fs::write(
            tel.join("collector.json"),
            serde_json::to_string(&info).unwrap(),
        )
        .unwrap();
        (pid, rx)
    }

    /// W9 step 4: `ensure` and `horch install` share one check. A live
    /// collector on the current binary is left alone; one on another binary
    /// (or one that recorded none: an older horch) is stopped before a new
    /// one starts; with none live, the install check starts nothing.
    #[cfg(unix)]
    #[test]
    fn spc_03_ensure_restarts_a_collector_on_an_older_binary() {
        let tmp = tempfile::tempdir().unwrap();
        let state = tmp.path().join("state");
        let bin = tmp.path().join("horch");
        std::fs::write(&bin, b"new build").unwrap();
        let ctx = RuntimeContext::from_env(
            &horch_core::runtime::MapEnv::new("/")
                .with_exe(&bin)
                .with("HORCH_STATE_DIR", &state.to_string_lossy()),
        )
        .unwrap();
        // No herdr: a start would fail, so a pass proves none was tried.
        let herdr = Herdr::with_bin(tmp.path().join("no-herdr"));
        restart_if_stale(&ctx, &herdr, &bin).expect("none live: nothing to do");

        let (pid, ended) = fake_collector(&state, lock::exe_identity(&bin));
        if horch_core::procid::start_time(pid).is_none() {
            return; // No start times on this platform.
        }
        ensure(&ctx, &herdr, true).expect("the current binary is left alone");
        restart_if_stale(&ctx, &herdr, &bin).unwrap();
        assert!(ended.recv_timeout(Duration::from_millis(200)).is_err());
        std::fs::remove_dir_all(horch_core::telemetry::dir(&state)).unwrap();
        // SAFETY: our own child, still unreaped by its waiter.
        unsafe { libc::kill(pid as i32, libc::SIGKILL) };
        ended.recv_timeout(Duration::from_secs(5)).unwrap();

        for exe in [
            None,
            Some(lock::ExeIdentity {
                len: 1,
                ..lock::exe_identity(&bin).unwrap()
            }),
        ] {
            let (_, ended) = fake_collector(&state, exe);
            let err = restart_if_stale(&ctx, &herdr, &bin).unwrap_err();
            assert!(
                format!("{err:#}").contains("telemetry workspace"),
                "{err:#}"
            );
            ended
                .recv_timeout(Duration::from_secs(5))
                .expect("the stale collector was stopped");
            std::fs::remove_dir_all(horch_core::telemetry::dir(&state)).unwrap();
        }
    }

    /// W12: a live collector that horch cannot stop safely (no recorded start
    /// time) is warned about and reported live. `ensure` returns `Ok`, signals
    /// nothing and starts nothing.
    #[cfg(unix)]
    #[test]
    fn spc_03_ensure_leaves_a_collector_it_cannot_stop() {
        let tmp = tempfile::tempdir().unwrap();
        let state = tmp.path().join("state");
        let bin = tmp.path().join("horch");
        std::fs::write(&bin, b"new build").unwrap();
        let ctx = RuntimeContext::from_env(
            &horch_core::runtime::MapEnv::new("/")
                .with_exe(&bin)
                .with("HORCH_STATE_DIR", &state.to_string_lossy()),
        )
        .unwrap();
        // No herdr: a start would fail, so a pass proves none was tried.
        let herdr = Herdr::with_bin(tmp.path().join("no-herdr"));
        let (pid, ended) = fake_collector(&state, None);
        let tel = horch_core::telemetry::dir(&state);
        let path = tel.join("collector.json");
        let mut info: lock::LockInfo =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        info.pid_start = None;
        std::fs::write(&path, serde_json::to_string(&info).unwrap()).unwrap();
        ensure(&ctx, &herdr, true).expect("a collector that cannot be stopped is not an error");
        restart_if_stale(&ctx, &herdr, &bin).expect("the install check is no error either");
        assert!(ended.recv_timeout(Duration::from_millis(200)).is_err());
        // SAFETY: our own child, still unreaped by its waiter.
        unsafe { libc::kill(pid as i32, libc::SIGKILL) };
        ended.recv_timeout(Duration::from_secs(5)).unwrap();
    }

    fn ws(id: &str, label: &str) -> horch_core::workspace::model::Workspace {
        serde_json::from_value(serde_json::json!({"workspace_id": id, "label": label})).unwrap()
    }

    /// W17 (SPC-03): an old collector workspace is one labelled `horch
    /// telemetry` that does not hold the live collector. A workspace with
    /// another label is never one. With a live collector whose workspace is
    /// not known, no workspace is.
    #[test]
    fn spc_03_old_collector_workspaces() {
        let listed = [
            ws("w1", "multi-herdr"),
            ws("w2", WORKSPACE_LABEL),
            ws("w3", WORKSPACE_LABEL),
            ws("w4", "horch telemetry 2"),
        ];
        assert_eq!(
            old_collector_workspaces(&listed, &LiveIn::Nothing),
            ["w2", "w3"]
        );
        assert_eq!(
            old_collector_workspaces(&listed, &LiveIn::Workspace("w3".into())),
            ["w2"]
        );
        assert_eq!(
            old_collector_workspaces(&listed, &LiveIn::Workspace("w1".into())),
            ["w2", "w3"]
        );
        assert!(old_collector_workspaces(&listed, &LiveIn::Unknown).is_empty());
    }

    /// A change in layout is a change to these files.
    #[test]
    fn spc_04_render_goldens() {
        let snap = fixture_snapshot();
        for (size, group, window, file) in [
            (
                (120, 40),
                "teammate",
                "7d",
                "telemetry-120x40-teammate-7d.txt",
            ),
            (
                (80, 24),
                "teammate",
                "live",
                "telemetry-80x24-teammate-live.txt",
            ),
        ] {
            let view = ViewState::with(group, window).unwrap();
            let frame = render(&snap, &view, size.0, size.1).join("\n") + "\n";
            check_golden(file, &frame);
        }
    }

    #[test]
    fn spc_04_render_every_group_and_window_fits() {
        let snap = fixture_snapshot();
        for (w, h) in [(120u16, 40u16), (80, 24)] {
            for g in GROUPS {
                for win in VIEW_WINDOWS {
                    let view = ViewState::with(g, win).unwrap();
                    let frame = render(&snap, &view, w, h);
                    assert!(frame.len() <= h as usize, "{g} {win} {w}x{h}");
                    assert!(
                        frame.iter().all(|l| l.chars().count() <= w as usize),
                        "{g} {win} {w}x{h}"
                    );
                    assert!(frame[0].contains(&format!("group: {g}")));
                    assert!(frame[0].contains(&format!("window: {win}")));
                    if win != "live" {
                        assert!(
                            frame.iter().any(|l| l.starts_with(&format!(
                                "{} BY {}",
                                win.to_uppercase(),
                                g.to_uppercase()
                            ))),
                            "{g} {win}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn numbers_read_short() {
        assert_eq!(human(412_000), "412k");
        assert_eq!(human(9_800_000), "9.8M");
        assert_eq!(human(88_000_000), "88M");
        assert_eq!(human(950), "950");
        assert_eq!(short_model("claude-opus-5-5"), "opus-5.5");
        assert_eq!(short_model("gpt-5.6-sol"), "5.6-sol");
        assert_eq!(
            clip("golden-prompts-whitespace", 20),
            "golden-prompts-whit…"
        );
    }
}
