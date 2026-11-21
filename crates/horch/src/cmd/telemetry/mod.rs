use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};
use horch_core::clock;
use horch_core::routing::quota::{self, PoolReading, POOLS};
use horch_core::runtime::RuntimeContext;
use horch_core::telemetry::collect::{self, Collector, Probing, Snapshot};
use horch_core::telemetry::lock::{self, Holder};
use horch_core::telemetry::{start, store};
use horch_core::usage::Locations;
use horch_core::workspace::PaneHost;

use crate::output;

mod screen;
mod view;

use screen::{Feed, LiveFeed, Source};
pub use view::{render, ViewState};

pub const WORKSPACE_LABEL: &str = "horch telemetry";

pub const VIEW_WINDOWS: [&str; 4] = ["live", "5h", "today", "7d"];

static SHUTDOWN: AtomicBool = AtomicBool::new(false);

fn install_signal_handlers() {
    use horch_platform::signal::Termination;
    let _ = horch_platform::signal::on_termination_set(
        &SHUTDOWN,
        &[
            Termination::Terminate,
            Termination::Interrupt,
            Termination::Hangup,
        ],
    );
}

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

fn pct(x: f64) -> String {
    format!("{:.0}%", (x * 100.0).round() + 0.0)
}

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
    out.extend(names.into_iter().map(|name| {
        let line = match pools.get(name) {
            None => format!("{}no reading", pad(name, 14)),
            Some(r) => pool_row(name, r, now),
        };
        clip(line.trim_end(), width)
    }));
    out
}

fn pool_row(name: &str, r: &PoolReading, now: DateTime<Utc>) -> String {
    let (state, shown) = pool_state(r);
    match (name == quota::POOL_ZEN, r.windows.is_empty()) {
        (true, true) => zen_row(name, r, state, shown),
        (false, true) => no_window_row(name, r, &shown),
        (_, false) => window_row(name, r, &shown, now),
    }
}

fn zen_row(name: &str, r: &PoolReading, state: &str, shown: String) -> String {
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
}

fn no_window_row(name: &str, r: &PoolReading, shown: &str) -> String {
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
        pad(shown, 12),
        why
    )
}

fn most_used(r: &PoolReading, now: DateTime<Utc>, scoped: bool) -> Option<&quota::Window> {
    r.windows
        .iter()
        .filter(|w| w.scope_model.is_some() == scoped)
        .max_by(|a, b| {
            a.used_at(now)
                .partial_cmp(&b.used_at(now))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
}

fn window_row(name: &str, r: &PoolReading, shown: &str, now: DateTime<Utc>) -> String {
    let unscoped = |mins: u64| unscoped_pct(r, mins, now);
    let scoped = most_used(r, now, true)
        .map(|w| {
            format!(
                "{} {}",
                w.scope_model.as_deref().unwrap_or_default(),
                pct(w.used_at(now))
            )
        })
        .unwrap_or_else(|| "-".into());
    let resets = most_used(r, now, false)
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
        pad(shown, 12),
        pad(&resets, 15),
        headroom
    )
}

fn pool_state(r: &PoolReading) -> (&str, String) {
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
    (state, shown)
}

fn unscoped_pct(r: &PoolReading, mins: u64, now: DateTime<Utc>) -> String {
    r.windows
        .iter()
        .find(|w| w.minutes == mins && w.scope_model.is_none())
        .map(|w| pct(w.used_at(now)))
        .unwrap_or_else(|| "-".into())
}

pub const COMPACT_STATE: usize = 32;

pub fn pool_compact(
    pools: &std::collections::BTreeMap<String, PoolReading>,
    now: DateTime<Utc>,
    width: usize,
    only_with_reading: &[&str],
) -> Vec<String> {
    let mut names: Vec<&str> = POOLS
        .into_iter()
        .filter(|p| !only_with_reading.contains(p) || pools.contains_key(*p))
        .collect();
    for extra in pools.keys() {
        if !names.contains(&extra.as_str()) {
            names.push(extra);
        }
    }
    names
        .into_iter()
        .map(|name| {
            let line = match pools.get(name) {
                None => format!("{} no reading", pad(name, 13)),
                Some(r) => {
                    let (state, shown) = pool_state(r);
                    let shown = if name == quota::POOL_ZEN && r.windows.is_empty() && state == "ok"
                    {
                        "ok*".to_string()
                    } else {
                        shown
                    };
                    format!(
                        "{} 5h {} 7d {} {}",
                        pad(name, 13),
                        pad(&unscoped_pct(r, 300, now), 5),
                        pad(&unscoped_pct(r, 10_080, now), 5),
                        shown
                    )
                }
            };
            clip(&line, width)
        })
        .collect()
}

pub const MIN_SIZE: (u16, u16) = (20, 6);

fn parse_size(size: &str) -> Result<(u16, u16)> {
    let (w, h) = size
        .split_once('x')
        .and_then(|(w, h)| Some((w.parse::<u16>().ok()?, h.parse::<u16>().ok()?)))
        .with_context(|| format!("--size takes WIDTHxHEIGHT, e.g. 120x40, not '{size}'"))?;
    if w < MIN_SIZE.0 || h < MIN_SIZE.1 {
        bail!(
            "--size {size} is too small: the minimum is {}x{}",
            MIN_SIZE.0,
            MIN_SIZE.1
        );
    }
    Ok((w, h))
}

fn short_model(model: &str) -> String {
    let m = model.rsplit('/').next().unwrap_or(model);
    let m = m.strip_prefix("claude-").unwrap_or(m);
    let m = m.strip_prefix("gpt-").unwrap_or(m);
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

fn money(cost: f64, unpriced: u64) -> String {
    if unpriced > 0 {
        format!("{cost:.2}*")
    } else {
        format!("{cost:.2}")
    }
}

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
            ensure(ctx, &PaneHost::of(ctx), false)?;
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
            let (w, h) = parse_size(&size)?;
            let view = ViewState::with(&group, &window)?;
            output::println(&render(&snap, &view, w, h).join("\n"));
            Ok(ExitCode::SUCCESS)
        }
    }
}

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

fn space(ctx: &RuntimeContext) -> Result<ExitCode> {
    install_signal_handlers();
    let root = ctx.paths.state_root.clone();
    let started_for = start::started_for();
    match &started_for {
        Some(id) => {
            if let Err(e) = start::mark(&root, id, &clock::now_stamp()) {
                eprintln!("horch: could not mark the telemetry collector start: {e:#}");
            }
        }
        None => report_failed_start(&root),
    }
    let me = lock::this_process(&clock::now_stamp(), ctx);
    let source = match lock::acquire(&root, &me)? {
        Ok(held) => {
            if let Some(request) = start::current(&root) {
                close_after_claim(ctx, &root, &request, started_for.as_deref());
                start::clear(&root);
            }
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
            if let Some(id) = &started_for {
                eprintln!(
                    "horch: collector pid {} is live; this telemetry collector start ends",
                    other.pid
                );
                start::withdraw(&root, id);
                return Ok(ExitCode::SUCCESS);
            }
            eprintln!(
                "horch: collector pid {} is live; this is a read-only viewer",
                other.pid
            );
            Source::Viewer
        }
    };
    let mut feed = LiveFeed::new(ctx, &root, source);
    let tick = Duration::from_millis(
        super::quotacmd::load_policy(ctx, &root)
            .map(|p| p.tick_ms)
            .unwrap_or(2000),
    );
    if std::io::stdout().is_terminal() {
        screen::screen(&mut feed, tick)?;
    } else {
        while !SHUTDOWN.load(Ordering::SeqCst) {
            if let Err(e) = feed.next() {
                eprintln!("horch telemetry: {e:#}");
            }
            sleep_until_shutdown(tick);
        }
    }
    drop(feed);
    Ok(ExitCode::SUCCESS)
}

fn sleep_until_shutdown(d: Duration) {
    let until = Instant::now() + d;
    while Instant::now() < until && !SHUTDOWN.load(Ordering::SeqCst) {
        std::thread::sleep(Duration::from_millis(50));
    }
}

pub fn ensure(ctx: &RuntimeContext, herdr: &PaneHost, quiet: bool) -> Result<()> {
    ensure_current(ctx, herdr, quiet, &ctx.bins.exe()?, true)
}

pub fn restart_if_stale(ctx: &RuntimeContext, herdr: &PaneHost, installed: &Path) -> Result<()> {
    ensure_current(ctx, herdr, false, installed, false)
}

fn ensure_current(
    ctx: &RuntimeContext,
    herdr: &PaneHost,
    quiet: bool,
    exe: &Path,
    start_if_none: bool,
) -> Result<()> {
    let root = ctx.paths.state_root.clone();
    let mut old_location: Option<(Option<String>, Option<String>)> = None;
    let go = match lock::holder(&root) {
        Holder::Live(info)
            if lock::runs_other_binary(&info, horch_core::exeid::exe_identity(exe).as_ref()) =>
        {
            old_location = Some((info.workspace_id.clone(), info.pane_id.clone()));
            replace_other_binary(&root, &info, exe, quiet)
        }
        Holder::Live(info) => {
            report_live(&info, quiet);
            false
        }
        _ => start_if_none,
    };
    if !go || start_pending(&root, quiet) {
        return Ok(());
    }
    let target_host = old_location
        .and_then(|(ws, pane)| host_holding_collector(ctx, herdr, ws.as_deref(), pane.as_deref()))
        .unwrap_or_else(|| herdr.clone());
    start(ctx, &target_host, quiet, exe)
}

fn candidate_hosts(ctx: &RuntimeContext, primary: &PaneHost) -> Vec<PaneHost> {
    let mut out = vec![primary.clone()];
    for h in horch_core::workspace::Hosts::reachable(ctx).hosts {
        if h.backend() != primary.backend() {
            out.push(h);
        }
    }
    out
}

fn host_holding_collector(
    ctx: &RuntimeContext,
    primary: &PaneHost,
    workspace_id: Option<&str>,
    pane_id: Option<&str>,
) -> Option<PaneHost> {
    for h in candidate_hosts(ctx, primary) {
        if let Some(ws) = workspace_id {
            if h.workspace_list()
                .ok()
                .is_some_and(|all| all.iter().any(|w| w.workspace_id == ws))
            {
                return Some(h);
            }
        }
        if let Some(pane) = pane_id {
            if h.pane_get(pane).is_ok() {
                return Some(h);
            }
        }
    }
    None
}

fn replace_other_binary(root: &Path, info: &lock::LockInfo, exe: &Path, quiet: bool) -> bool {
    let stopped = lock::stop(root, info, Duration::from_secs(5));
    match &stopped {
        Err(e) => {
            eprintln!(
                "horch: telemetry collector pid {} runs another binary, but horch did not stop it: {e:#}. To replace it, first check that pid {} runs `horch telemetry collect` (`ps -p {} -o command=`), then stop it, and run `horch telemetry ensure` again.",
                info.pid, info.pid, info.pid
            );
            report_live(info, quiet);
        }
        Ok(_) => output::println(&replaced_line(info.pid, info.exe.as_ref(), exe)),
    }
    stopped.is_ok()
}

fn replaced_line(pid: u32, ran: Option<&horch_core::exeid::ExeIdentity>, exe: &Path) -> String {
    let now = exe.canonicalize().unwrap_or_else(|_| exe.to_path_buf());
    let why = match ran {
        None => "it did not record its binary".to_string(),
        Some(r) if Path::new(&r.path) == now => {
            format!(
                "{} was rebuilt or reinstalled after it started",
                now.display()
            )
        }
        Some(r) => format!("it ran {}, not {}", r.path, now.display()),
    };
    format!("stopped telemetry collector pid {pid}: {why}")
}

fn start_pending(root: &Path, quiet: bool) -> bool {
    match start::pending(root) {
        start::Pending::Starting(request) => {
            report_starting(&request, quiet);
            true
        }
        start::Pending::Failed(request, why) => {
            eprintln!(
                "horch: the telemetry collector start in workspace {} failed: {why}; starting it again",
                request.workspace_id
            );
            start::clear(root);
            false
        }
        start::Pending::None => false,
    }
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

#[derive(Debug, Clone, PartialEq, Eq)]
enum LiveIn {
    Nothing,
    Workspace(String),
    Unknown,
}

fn live_collector_in(herdr: &PaneHost, root: &Path) -> LiveIn {
    match lock::holder(root) {
        Holder::Live(info) => match info.pane_id {
            Some(pane) => workspace_of(herdr, &pane, info.workspace_id)
                .map_or(LiveIn::Unknown, LiveIn::Workspace),
            None => LiveIn::Unknown,
        },
        _ => LiveIn::Nothing,
    }
}

fn workspace_of(herdr: &PaneHost, pane: &str, recorded: Option<String>) -> Option<String> {
    herdr
        .pane_get(pane)
        .ok()
        .and_then(|p| p.workspace_id)
        .or(recorded)
        .or_else(|| pane.split_once(':').map(|(w, _)| w.to_string()))
}

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

fn start(ctx: &RuntimeContext, herdr: &PaneHost, quiet: bool, exe: &Path) -> Result<()> {
    let root = ctx.paths.state_root.clone();
    let id = start::new_id();
    let command = collector_command(ctx, exe, &id);
    let mut create_host = herdr.clone();
    let mut started = None;
    for host in candidate_hosts(ctx, herdr) {
        let Ok(all) = host.workspace_list() else {
            continue;
        };
        if all
            .iter()
            .any(|w| w.label.as_deref() == Some(WORKSPACE_LABEL))
        {
            create_host = host.clone();
        }
        let old = old_collector_workspaces(&all, &live_collector_in(&host, &root));
        if let Some(reused) = start_in_old(ctx, &host, &id, &command, &old)? {
            started = Some(reused);
            break;
        }
    }
    let (ws, pane) = match started {
        Some(started) => started,
        None => start_in_new(ctx, &create_host, &id, &command)?,
    };
    if !quiet {
        output::println(&format!(
            "telemetry collector starting: workspace {ws}, pane {pane}"
        ));
    }
    Ok(())
}

fn start_request(
    ctx: &RuntimeContext,
    host: &PaneHost,
    id: &str,
    ws: &str,
    pane: &str,
) -> start::Request {
    start::Request {
        id: id.to_string(),
        requested_at: clock::now_stamp(),
        workspace_id: ws.to_string(),
        pane_id: pane.to_string(),
        herdr: ctx.bins.harness.herdr.clone(),
        backend: host.backend(),
    }
}

fn start_in_old(
    ctx: &RuntimeContext,
    herdr: &PaneHost,
    id: &str,
    command: &str,
    old: &[String],
) -> Result<Option<(String, String)>> {
    let reusable = old.iter().find_map(|ws| match herdr.pane_list(ws) {
        Ok(panes) if panes.len() == 1 => Some((ws.clone(), panes[0].pane_id.clone())),
        _ => None,
    });
    let Some((ws, pane)) = reusable else {
        return Ok(None);
    };
    start::record(
        &ctx.paths.state_root,
        &start_request(ctx, herdr, id, &ws, &pane),
    )?;
    Ok(herdr
        .pane_run(&pane, command)
        .map(|_| (ws.clone(), pane))
        .inspect_err(|_| {
            eprintln!(
                "horch: the telemetry collector did not start in the old workspace {ws}; starting it in a new one"
            )
        })
        .ok())
}

fn start_in_new(
    ctx: &RuntimeContext,
    herdr: &PaneHost,
    id: &str,
    command: &str,
) -> Result<(String, String)> {
    let root = &ctx.paths.state_root;
    let created = herdr.workspace_create(WORKSPACE_LABEL, None, false);
    let ws = created.inspect_err(|_| start::clear(root));
    let ws = ws.context("creating the telemetry workspace")?;
    start::record(
        root,
        &start_request(ctx, herdr, id, &ws.workspace_id, &ws.root_pane_id),
    )?;
    let ran = herdr.pane_run(&ws.root_pane_id, command);
    ran.inspect_err(|_| start::clear(root))?;
    Ok((ws.workspace_id, ws.root_pane_id))
}

fn report_starting(request: &start::Request, quiet: bool) {
    if !quiet {
        output::println(&format!(
            "telemetry collector is starting: workspace {}, pane {}",
            request.workspace_id, request.pane_id
        ));
    }
}

pub(crate) fn report_failed_start(root: &Path) {
    if let start::Pending::Failed(request, why) = start::pending(root) {
        eprintln!(
            "horch: the telemetry collector start in workspace {} failed: {why}; `horch telemetry ensure` starts it again",
            request.workspace_id
        );
    }
}

fn close_after_claim(
    ctx: &RuntimeContext,
    root: &Path,
    request: &start::Request,
    started_for: Option<&str>,
) {
    let herdr = match request.backend {
        horch_core::runtime::Backend::Herdr => PaneHost::herdr_bin(&request.herdr),
        backend => PaneHost::for_backend(ctx, backend),
    };
    let keep = (started_for == Some(request.id.as_str())).then_some(request.workspace_id.as_str());
    if let Ok(all) = herdr.workspace_list() {
        let other = |w: &horch_core::workspace::model::Workspace| {
            w.label.as_deref() == Some(WORKSPACE_LABEL) && Some(w.workspace_id.as_str()) != keep
        };
        if all.iter().any(other) {
            let old = old_collector_workspaces(&all, &live_collector_in(&herdr, root));
            for ws in old.iter().filter(|ws| Some(ws.as_str()) != keep) {
                if let Err(e) = herdr.workspace_close(ws) {
                    eprintln!("horch: could not close the old telemetry workspace {ws}: {e:#}");
                }
            }
        }
    }
}

fn collector_command(ctx: &RuntimeContext, exe: &Path, id: &str) -> String {
    let mut args = vec!["telemetry".to_string()];
    if let Some(dir) = super::path_text(ctx.paths.state_override.as_deref()) {
        args.push("--state-dir".into());
        args.push(dir);
    }
    let data_root = ctx.paths.data_root.to_string_lossy();
    horch_core::workspace::paneshell::PaneShell::host().command_line_with_env(
        exe,
        &[
            ("HORCH_DATA_DIR", data_root.as_ref()),
            (start::START_ENV, id),
        ],
        &args,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_replace_line_says_what_changed() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().canonicalize().unwrap().join("horch");
        std::fs::write(&bin, b"new").unwrap();
        let other = dir.path().canonicalize().unwrap().join("old-horch");
        let id = |p: &Path| horch_core::exeid::ExeIdentity {
            path: p.display().to_string(),
            len: 3,
            mtime_ns: 1,
        };
        assert_eq!(
            replaced_line(7, Some(&id(&bin)), &bin),
            format!(
                "stopped telemetry collector pid 7: {} was rebuilt or reinstalled after it started",
                bin.display()
            )
        );
        assert_eq!(
            replaced_line(7, Some(&id(&other)), &bin),
            format!(
                "stopped telemetry collector pid 7: it ran {}, not {}",
                other.display(),
                bin.display()
            )
        );
        assert_eq!(
            replaced_line(7, None, &bin),
            "stopped telemetry collector pid 7: it did not record its binary"
        );
    }
    use horch_core::telemetry::store::GROUPS;

    #[test]
    fn pct_never_prints_a_negative_zero() {
        assert_eq!(pct(-0.0004), "0%");
        assert_eq!(pct(-0.0), "0%");
        assert_eq!(pct(0.0), "0%");
        assert_eq!(pct(0.256), "26%");
        assert_eq!(pct(-0.25), "-25%");
    }

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

    #[test]
    fn pool_table_rows_by_kind() {
        use horch_core::routing::quota::Window;
        let now = DateTime::parse_from_rfc3339("2026-10-30T10:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let soon = Some("2026-10-30T12:00:00Z".to_string());
        let mut pools = std::collections::BTreeMap::new();
        pools.insert(
            quota::POOL_ZEN.to_string(),
            PoolReading {
                state: "ok".into(),
                ..PoolReading::default()
            },
        );
        pools.insert(
            quota::POOL_GOOGLE.to_string(),
            PoolReading {
                state: "cooling".into(),
                error: Some("boom".into()),
                reason: Some("ignored".into()),
                ..PoolReading::default()
            },
        );
        pools.insert(
            "codex".to_string(),
            PoolReading {
                state: "ok".into(),
                windows: vec![
                    Window::new(300, 0.25, soon.clone(), None),
                    Window::new(10_080, 0.5, soon.clone(), None),
                    Window::new(10_080, 0.75, soon, Some("fable".into())),
                ],
                ..PoolReading::default()
            },
        );
        let rows = pool_table(&pools, now, 120, &[]);
        let want = [
            "POOL          5h      7d     scoped        state       resets         headroom/h",
            "claude        no reading",
            "codex         25%     50%    fable 75%     ok          Fri 12:00Z     25.0%",
            "opencode-zen  no signal                    ok*         -              -          * public work only",
            "google        -       -      -             COOLING     boom",
            "local         no reading",
        ];
        assert_eq!(rows, want);
    }

    fn fixture_snapshot() -> Snapshot {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/snapshot-fixture.json");
        Snapshot::read(&path).unwrap()
    }

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

    #[cfg(unix)]
    fn fake_collector(
        state: &Path,
        exe: Option<horch_core::exeid::ExeIdentity>,
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
        let herdr = PaneHost::herdr_bin(tmp.path().join("no-herdr"));
        restart_if_stale(&ctx, &herdr, &bin).expect("none live: nothing to do");

        let (pid, ended) = fake_collector(&state, horch_core::exeid::exe_identity(&bin));
        if horch_core::procid::start_time(pid).is_none() {
            return;
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
            Some(horch_core::exeid::ExeIdentity {
                len: 1,
                ..horch_core::exeid::exe_identity(&bin).unwrap()
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
        let herdr = PaneHost::herdr_bin(tmp.path().join("no-herdr"));
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

    #[cfg(unix)]
    #[test]
    fn hcb_10_telemetry_is_not_started_twice_across_hosts() {
        let tmp = tempfile::tempdir().unwrap();
        let state = tmp.path().join("state");
        let bin = tmp.path().join("horch");
        std::fs::write(&bin, b"build").unwrap();
        let herdr_bin = tmp.path().join("herdr");
        let hc_bin = tmp.path().join("hc");
        let app_bin = tmp.path().join("herdr-app");
        let sock = tmp.path().join("default.sock");
        std::fs::write(&sock, b"").unwrap();

        assert!(crate::test_support::write_warmed(
            &herdr_bin,
            "#!/bin/sh\nif [ \"$1\" = warm ]; then exit 0; fi\n\
             d=$(dirname \"$0\")\n\
             printf '%s\\n' \"$*\" >> \"$d/herdr_calls\"\n\
             case \"$1 $2\" in\n\
             'workspace list') printf '{\"result\":{\"workspaces\":[{\"workspace_id\":\"w1\",\"label\":\"horch telemetry\"}]}}\\n' ;;\n\
             'pane list') printf '{\"result\":{\"panes\":[{\"pane_id\":\"w1:p1\",\"workspace_id\":\"w1\"}]}}\\n' ;;\n\
             'pane run') printf '{\"result\":{\"type\":\"ok\"}}\\n' ;;\n\
             *) exit 1 ;;\n\
             esac\n",
            &["warm"],
        )
        .status
        .success());
        assert!(crate::test_support::write_warmed(
            &hc_bin,
            "#!/bin/sh\nif [ \"$1\" = warm ]; then exit 0; fi\n\
             case \"$1\" in\n\
             server) printf 'running\\n' ;;\n\
             *) exit 0 ;;\n\
             esac\n",
            &["warm"],
        )
        .status
        .success());
        assert!(crate::test_support::write_warmed(
            &app_bin,
            "#!/bin/sh\nif [ \"$1\" = warm ]; then exit 0; fi\n\
             d=$(dirname \"$0\")\n\
             printf '%s\\n' \"$*\" >> \"$d/app_calls\"\n\
             case \"$1\" in\n\
             ls) printf '{\"spaces\":[]}\\n' ;;\n\
             status) printf '{\"ok\":true}\\n' ;;\n\
             do) echo CREATED_ON_HC >> \"$d/hc_created\"; printf '{\"space\":\"sp1\",\"tab\":\"tb1\",\"terminal\":\"t1\"}\\n' ;;\n\
             esac\n",
            &["warm"],
        )
        .status
        .success());

        let ctx = RuntimeContext::from_env(
            &horch_core::runtime::MapEnv::new("/")
                .with_exe(&bin)
                .with("HOME", &tmp.path().to_string_lossy())
                .with("HORCH_STATE_DIR", &state.to_string_lossy())
                .with("HORCH_HERDR_BIN", &herdr_bin.to_string_lossy())
                .with("HORCH_HC_BIN", &hc_bin.to_string_lossy())
                .with("HORCH_HERDR_APP_BIN", &app_bin.to_string_lossy())
                .with("HC_TERMINAL_SOCKET", &sock.to_string_lossy())
                .with("HC_TERMINAL_ID", "t2"),
        )
        .unwrap();
        assert_eq!(ctx.herdr.backend, horch_core::runtime::Backend::Hc);

        ensure(&ctx, &PaneHost::of(&ctx), true).unwrap();
        assert!(
            !tmp.path().join("hc_created").exists(),
            "ensure must reuse the existing horch telemetry workspace on herdr instead of creating a second one on hc"
        );
        let herdr_calls = std::fs::read_to_string(tmp.path().join("herdr_calls")).unwrap();
        assert!(herdr_calls.contains("pane run w1:p1"), "{herdr_calls}");
    }

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
    fn spc_04_render_layout_goldens() {
        let snap = layout_snapshot();
        for (w, h, group, window) in [
            (80, 24, "project", "5h"),
            (100, 20, "teammate", "live"),
            (120, 40, "project", "7d"),
            (200, 50, "teammate", "5h"),
        ] {
            let view = ViewState::with(group, window).unwrap();
            let frame = render(&snap, &view, w, h).join("\n") + "\n";
            check_golden(&format!("telemetry-layout-{w}x{h}.txt"), &frame);
        }
    }

    fn layout_snapshot() -> Snapshot {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/golden/telemetry-layout-fixture.json");
        Snapshot::read(&path).unwrap()
    }

    fn table<'a>(frame: &'a [String], title: &str) -> (&'a str, Vec<&'a str>) {
        let at = frame
            .iter()
            .position(|l| l.starts_with(&format!(" {title}")))
            .unwrap_or_else(|| panic!("no table {title}:\n{}", frame.join("\n")));
        let shown = frame[at]
            .split_once("rows ")
            .and_then(|(_, r)| r.split_once(" of"))
            .and_then(|(r, _)| r.split_once('-'))
            .map(|(a, b)| b.parse::<usize>().unwrap() + 1 - a.parse::<usize>().unwrap())
            .unwrap_or(0);
        let rows = frame[at + 2..at + 2 + shown]
            .iter()
            .map(|l| l.as_str())
            .collect();
        (frame[at + 1].as_str(), rows)
    }

    fn column(head: &str, name: &str) -> Option<usize> {
        let mut at = 0;
        for word in head.split(' ') {
            if word == name {
                return Some(at);
            }
            at += word.chars().count() + 1;
        }
        None
    }

    #[test]
    fn spc_04_the_layout_keeps_money_flags_and_gutters() {
        let snap = layout_snapshot();
        let money = |s: &str| {
            let s = s.trim_end_matches('*');
            s.split_once('.')
                .is_some_and(|(a, b)| b.len() == 2 && a.chars().all(|c| c.is_ascii_digit()))
        };
        for (w, h) in [(80u16, 24u16), (100, 20), (120, 40), (200, 50)] {
            for g in GROUPS {
                for win in VIEW_WINDOWS {
                    let view = ViewState::with(g, win).unwrap();
                    let frame = render(&snap, &view, w, h);
                    let at = format!("{w}x{h} {g} {win}");
                    assert!(frame.last().unwrap().starts_with("? help  q quit"), "{at}");
                    let (head, rows) = table(&frame, "live panes");
                    assert!(!head.contains('…'), "{at}: {head}");
                    for name in ["LIVE", "out", "$", "tok/min", "state"] {
                        assert!(column(head, name).is_some(), "{at}: {name} in {head}");
                    }
                    let state = column(head, "state").unwrap();
                    for row in &rows {
                        let flags: String = row.chars().skip(state).collect();
                        assert!(!flags.contains('…'), "{at}: {row:?}");
                    }
                    let mut tables = vec![(head, rows, column(head, "$").unwrap())];
                    if win != "live" {
                        let (head, rows) = table(&frame, &format!("{win} by {g}"));
                        assert!(!head.contains('…'), "{at}: {head}");
                        for name in ["tokens", "$", "share"] {
                            assert!(column(head, name).is_some(), "{at}: {name} in {head}");
                        }
                        tables.push((head, rows, column(head, "$").unwrap()));
                    }
                    for (head, rows, cost) in tables {
                        let starts: Vec<usize> = [
                            "model", "phase", "plan", "fresh", "c.read", "out", "$", "tok/min",
                            "state", "tokens", "share", "cache", "$/DONE",
                        ]
                        .iter()
                        .filter_map(|name| column(head, name))
                        .collect();
                        for row in rows {
                            let chars: Vec<char> = row.chars().collect();
                            for &c in &starts {
                                if c < chars.len() {
                                    assert_eq!(chars[c - 1], ' ', "{at}: column {c} in {row:?}");
                                }
                            }
                            let cell: String =
                                chars[cost..].iter().take_while(|c| **c != ' ').collect();
                            assert!(money(&cell), "{at}: $ {cell:?} in {row:?}");
                        }
                    }
                    let text = frame.join("\n");
                    assert!(
                        text.contains(" idle") && text.contains(" claude:tight"),
                        "{at}"
                    );
                }
            }
        }
    }

    #[test]
    fn spc_04_project_names_and_wide_roles() {
        let snap = layout_snapshot();
        let text = render(&snap, &ViewState::with("project", "7d").unwrap(), 120, 40).join("\n");
        for name in [
            "games/lawn-mower-manor",
            "work/lawn-mower-manor",
            "multi-herdr ",
        ] {
            assert!(text.contains(name), "{name}\n{text}");
        }
        assert!(!text.contains("/Users/x"), "{text}");
        let text = render(&snap, &ViewState::with("teammate", "5h").unwrap(), 200, 50).join("\n");
        assert!(text.contains("architect-reviewer-1 "), "{text}");
        assert!(text.contains("timeline-integration-wave7"), "{text}");
    }

    #[test]
    fn spc_04_a_short_terminal_gives_the_rows_to_the_tables() {
        let snap = layout_snapshot();
        let frame = render(&snap, &ViewState::with("teammate", "live").unwrap(), 80, 24);
        let text = frame.join("\n");
        assert!(!text.contains("POOL"), "{text}");
        assert!(frame[1].starts_with("claude        5h "), "{text}");
        let (_, rows) = table(&frame, "live panes");
        assert!(rows.len() >= 8, "{} rows\n{text}", rows.len());
        let facts = frame.iter().filter(|l| l.contains("UNREAD")).count();
        assert_eq!(facts, 1, "{text}");
        assert!(
            frame[22].contains("UNREAD") && frame[22].contains("orchestrators"),
            "{text}"
        );
        assert!(frame[0].starts_with("horch telemetry · 17:42Z"), "{text}");
    }

    #[test]
    fn spc_04_render_rejects_a_size_below_the_minimum() {
        assert_eq!(parse_size("120x40").unwrap(), (120, 40));
        assert_eq!(parse_size("20x6").unwrap(), (20, 6));
        for bad in ["10x3", "19x40", "120x5"] {
            let err = format!("{:#}", parse_size(bad).unwrap_err());
            assert!(err.contains("minimum is 20x6"), "{err}");
        }
        assert!(parse_size("wide").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn spc_04_sighup_ends_the_screen_like_sigterm() {
        install_signal_handlers();
        let action = |sig: libc::c_int| {
            // SAFETY: reads the action; a null new action changes nothing.
            let mut old: libc::sigaction = unsafe { std::mem::zeroed() };
            // SAFETY: `old` is a zeroed `sigaction` the call may fill; `sig` is a valid signal number.
            unsafe { libc::sigaction(sig, std::ptr::null(), &mut old) };
            old.sa_sigaction
        };
        let handler = action(libc::SIGTERM);
        assert_ne!(handler, libc::SIG_DFL);
        assert_ne!(handler, libc::SIG_IGN);
        for sig in [libc::SIGHUP, libc::SIGTERM, libc::SIGINT] {
            assert_eq!(action(sig), handler, "signal {sig}");
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
