use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use anyhow::{bail, Context, Result};

use crate::execution::TilingMode;
use crate::messaging::mailbox::Mailbox;
use crate::runtime::process::spawn_detached;
use crate::runtime::RuntimeContext;
use crate::workspace::balance;
use crate::workspace::host::PaneHost;
use crate::workspace::layout::{self, Orchestrator};
use crate::workspace::model::{Direction, Layout};
use crate::workspace::tile::{self, Fleet, FocusState, Op, Plan, TabRef, TabShape, Worker};

pub const TELEMETRY_WORKSPACE_LABEL: &str = "horch telemetry";

const MAX_BALANCE_OPS: usize = 24;

const LOCK_WAIT: Duration = Duration::from_secs(20);
const LOCK_STALE: Duration = Duration::from_secs(60);

fn is_telemetry_space(herdr: &PaneHost, workspace_id: &str) -> bool {
    herdr.workspace_list().is_ok_and(|list| {
        list.iter().any(|w| {
            w.workspace_id == workspace_id && w.label.as_deref() == Some(TELEMETRY_WORKSPACE_LABEL)
        })
    })
}

struct TabState {
    tab_id: String,
    layout: Layout,
}

struct Snapshot {
    tabs: Vec<TabState>,
    orchestrator: String,
    orchestrator_tab: String,
    active_tab: Option<String>,
}

impl Snapshot {
    fn shapes(&self) -> Vec<TabShape> {
        self.tabs
            .iter()
            .map(|t| TabShape::from_layout(&t.layout))
            .collect()
    }

    fn workers(&self) -> Vec<Worker> {
        self.tabs
            .iter()
            .flat_map(|t| {
                t.layout
                    .panes
                    .iter()
                    .filter(|p| p.pane_id != self.orchestrator)
                    .map(|p| Worker {
                        pane: p.pane_id.clone(),
                        tab_id: t.tab_id.clone(),
                        x: p.rect.x,
                        y: p.rect.y,
                    })
            })
            .collect()
    }

    fn fleet(&self, newcomer: Option<&str>) -> Fleet {
        Fleet {
            tabs: self.tabs.iter().map(|t| t.tab_id.clone()).collect(),
            orchestrator: self.orchestrator.clone(),
            orchestrator_tab: self.orchestrator_tab.clone(),
            workers: self.workers(),
            newcomer: newcomer.map(str::to_owned),
        }
    }

    fn focus_state(&self) -> Option<FocusState> {
        let tab = self.active_tab.clone()?;
        let pane = self
            .tabs
            .iter()
            .find(|t| t.tab_id == tab)?
            .layout
            .focused_pane_id
            .clone()?;
        Some(FocusState { tab, pane })
    }

    fn zoomed_tab(&self) -> Option<&str> {
        self.tabs
            .iter()
            .find(|t| t.layout.zoomed)
            .map(|t| t.tab_id.as_str())
    }
}

fn gather(ctx: &RuntimeContext, herdr: &PaneHost, workspace_id: &str) -> Result<Snapshot> {
    if is_telemetry_space(herdr, workspace_id) {
        bail!("workspace {workspace_id} is the telemetry space; horch tile leaves it alone");
    }
    let states = read_tabs(herdr, workspace_id)?;
    let orchestrator = find_orchestrator(ctx, workspace_id, &states)?;
    let orchestrator_tab = states
        .iter()
        .find(|t| t.layout.panes.iter().any(|p| p.pane_id == orchestrator))
        .map(|t| t.tab_id.clone())
        .expect("the orchestrator was found in one of these tabs");

    let active_tab = active_tab(herdr, workspace_id).or_else(|| Some(orchestrator_tab.clone()));

    Ok(Snapshot {
        tabs: states,
        orchestrator,
        orchestrator_tab,
        active_tab,
    })
}

fn read_tabs(herdr: &PaneHost, workspace_id: &str) -> Result<Vec<TabState>> {
    let tabs = herdr.tab_list(workspace_id)?;
    let panes = herdr.pane_list(workspace_id)?;
    let states = tabs
        .iter()
        .filter_map(|tab| {
            let probe = panes
                .iter()
                .find(|p| p.tab_id.as_deref() == Some(tab.tab_id.as_str()))?;
            Some(
                herdr
                    .pane_layout(Some(&probe.pane_id))
                    .map(|layout| TabState {
                        tab_id: tab.tab_id.clone(),
                        layout,
                    }),
            )
        })
        .collect::<Result<Vec<_>>>()?;
    if states.is_empty() {
        bail!("workspace {workspace_id} has no panes");
    }
    Ok(states)
}

fn active_tab(herdr: &PaneHost, workspace_id: &str) -> Option<String> {
    herdr
        .workspace_list()
        .ok()?
        .into_iter()
        .find(|w| w.workspace_id == workspace_id)?
        .active_tab_id
}

fn find_orchestrator(
    ctx: &RuntimeContext,
    workspace_id: &str,
    states: &[TabState],
) -> Result<String> {
    let registered = Mailbox::in_context(ctx, workspace_id)
        .panes_to_roles()
        .into_iter()
        .find(|(_, role)| role == "orchestrator")
        .map(|(pane_id, _)| pane_id)
        .filter(|id| {
            states
                .iter()
                .any(|t| t.layout.panes.iter().any(|p| &p.pane_id == id))
        });
    match registered {
        Some(id) => Ok(id),
        None => layout::analyze_with(&states[0].layout, layout::Orchestrator::Infer)
            .orchestrator
            .context(
                "no orchestrator in this workspace: no pane is registered for the role and no \
             full-height pane sits on the first tab. Register one with `horch register \
             orchestrator` in its pane, then tile again",
            ),
    }
}

fn apply(herdr: &PaneHost, plan: &Plan) -> Result<usize> {
    let mut created: HashMap<usize, String> = HashMap::new();
    let mut moved = 0;
    for op in plan.ops() {
        moved += usize::from(apply_op(herdr, op, &mut created)?);
    }
    Ok(moved)
}

fn apply_op(herdr: &PaneHost, op: &Op, created: &mut HashMap<usize, String>) -> Result<bool> {
    match op {
        Op::NewTab {
            pane,
            label,
            creates,
        } => {
            created.insert(*creates, new_tab(herdr, op, pane, label)?);
            Ok(true)
        }
        Op::Move {
            pane,
            tab,
            split,
            target,
            ratio,
        } => move_pane(herdr, op, (pane, tab, created), (*split, target, *ratio)).map(|()| true),
        Op::FocusTab { tab } => {
            if let TabRef::Existing(id) = tab {
                herdr.tab_focus(id)?;
            }
            Ok(false)
        }
    }
}

fn new_tab(herdr: &PaneHost, op: &Op, pane: &str, label: &str) -> Result<String> {
    let result = herdr.pane_move_new_tab(pane, label)?;
    declined(op, result.changed, result.reason.as_deref())?;
    result
        .created_tab
        .map(|t| t.tab_id)
        .or_else(|| result.pane.tab_id.clone())
        .with_context(|| format!("herdr moved {pane} to a new tab but did not name it"))
}

fn move_pane(
    herdr: &PaneHost,
    op: &Op,
    (pane, tab, created): (&str, &TabRef, &HashMap<usize, String>),
    (split, target, ratio): (Direction, &str, Option<f64>),
) -> Result<()> {
    let tab_id = match tab {
        TabRef::Existing(id) => id,
        TabRef::Created(i) => created
            .get(i)
            .with_context(|| format!("tab {i} was never created"))?,
    };
    let result = herdr.pane_move(pane, tab_id, split, Some(target), ratio)?;
    declined(op, result.changed, result.reason.as_deref())
}

fn declined(op: &Op, changed: bool, reason: Option<&str>) -> Result<()> {
    if changed {
        return Ok(());
    }
    let why = match reason {
        Some("zoomed_tab") => "a tab is zoomed; unzoom it and tile again".to_string(),
        Some(r) => format!("herdr declined it ({r})"),
        None => "herdr declined it".to_string(),
    };
    bail!(
        "stopped part way: {why}.\n  the move that failed: {}\n  no pane was closed, so nothing \
         is lost; run `horch tile` again to finish",
        tile::command_line(op)
    );
}

fn balance_tab(herdr: &PaneHost, mut layout: Layout, orchestrator: Option<&str>) -> Result<usize> {
    let mut applied = 0;
    let mut last: Option<(String, i64)> = None;
    for _ in 0..MAX_BALANCE_OPS {
        let shape = TabShape::from_layout(&layout);
        let Some(op) = tile::balance_op(&shape, orchestrator) else {
            break;
        };
        if last.as_ref() == Some(&(op.pane.clone(), op.from_x)) {
            break;
        }
        last = Some((op.pane.clone(), op.from_x));
        let out = herdr.pane_resize(&op.pane, op.direction.as_str(), op.amount)?;
        applied += 1;
        if !out.changed {
            break;
        }
        layout = out.layout;
    }
    Ok(applied)
}

fn balance_all(herdr: &PaneHost, snapshot: &Snapshot) -> Result<usize> {
    let mut applied = 0;
    for tab in &snapshot.tabs {
        let orchestrator =
            (tab.tab_id == snapshot.orchestrator_tab).then_some(snapshot.orchestrator.as_str());
        applied += balance_tab(herdr, tab.layout.clone(), orchestrator)?;
    }
    Ok(applied)
}

fn restore_focus(
    herdr: &PaneHost,
    after: &Snapshot,
    before: Option<&FocusState>,
    before_tab: Option<&str>,
) -> Option<String> {
    let Some(before) = before else {
        let tab = before_tab?;
        let _ = herdr.tab_focus(if after.tabs.iter().any(|t| t.tab_id == tab) {
            tab
        } else {
            &after.orchestrator_tab
        });
        return None;
    };
    let target = tile::focus_target(before, &after.shapes(), &after.orchestrator)?;
    let _ = herdr.tab_focus(&target.tab);
    let (pane, landed) = match target.pane.as_deref() {
        Some(pane) => (
            pane.to_string(),
            herdr.pane_focus_walk(&target.tab, pane).unwrap_or(false),
        ),
        None => ("herdr's own choice".to_string(), true),
    };
    Some(if !landed {
        format!(
            "focus: {} {} is out of reach; herdr keeps its own choice\n",
            target.tab, pane
        )
    } else if target.kept {
        format!("focus: kept {} {}\n", target.tab, pane)
    } else {
        format!(
            "focus: moved to {} {} ({})\n",
            target.tab, pane, target.reason
        )
    })
}

struct TileLock {
    path: PathBuf,
}

impl TileLock {
    fn acquire(ctx: &RuntimeContext, workspace_id: &str) -> Result<Option<Self>> {
        let dir = Mailbox::in_context(ctx, workspace_id).dir().to_path_buf();
        std::fs::create_dir_all(&dir)?;
        Self::at(dir.join("tile.lock"), LOCK_STALE)
    }

    fn at(path: PathBuf, stale_after: Duration) -> Result<Option<Self>> {
        let deadline = SystemTime::now() + LOCK_WAIT;
        loop {
            if try_create(&path)? {
                break Ok(Some(Self { path }));
            }
            if clear_stale(&path, stale_after) {
                continue;
            }
            if SystemTime::now() > deadline {
                break Ok(None);
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    }
}

fn try_create(path: &std::path::Path) -> Result<bool> {
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
    {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
        Err(e) => Err(e).context("taking the tile lock"),
    }
}

fn clear_stale(path: &std::path::Path, stale_after: Duration) -> bool {
    let stale = std::fs::metadata(path)
        .and_then(|m| m.modified())
        .map(|m| m.elapsed().unwrap_or_default() >= stale_after)
        .unwrap_or(false);
    if stale {
        let _ = std::fs::remove_file(path);
    }
    stale
}

impl Drop for TileLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn roles(ctx: &RuntimeContext, workspace_id: &str) -> HashMap<String, String> {
    Mailbox::in_context(ctx, workspace_id)
        .panes_to_roles()
        .into_iter()
        .collect()
}

pub fn run(
    ctx: &RuntimeContext,
    herdr: &PaneHost,
    workspace_id: &str,
    newcomer: Option<&str>,
    plan_only: bool,
) -> Result<String> {
    if !herdr.caps().geometry {
        return Ok("the Herdr app arranges its own panes; nothing to tile\n".to_string());
    }
    let snapshot = gather(ctx, herdr, workspace_id)?;
    let plan = tile::plan(&snapshot.fleet(newcomer));
    if plan_only {
        Ok(plan_report(ctx, &snapshot, &plan, workspace_id))
    } else {
        run_live(ctx, herdr, workspace_id, snapshot, &plan)
    }
}

fn plan_report(
    ctx: &RuntimeContext,
    snapshot: &Snapshot,
    plan: &Plan,
    workspace_id: &str,
) -> String {
    let workers = plan.placement.len();
    let mut out = format!(
        "{} worker(s) over {} tab(s); {} move(s) to get there\n\n",
        workers,
        tile::tab_count(workers),
        plan.ops().count()
    );
    if tile::is_canonical(&snapshot.shapes(), &snapshot.orchestrator, workers) {
        out.push_str(
            "the grid is already laid out; `horch tile` would only even out the \
                      columns and change nothing else\n\n",
        );
    } else {
        out.push_str("park every worker in a scratch tab:\n");
        push_ops(&mut out, &plan.park);
        out.push_str("\nthen place them, slot by slot:\n");
        push_ops(&mut out, &plan.place);
        out.push_str(
            "\nemptied tabs close themselves, so no tab is ever closed by hand.\n\
             then: even out every column, and put focus back where it was.\n\n",
        );
    }
    out.push_str(&tile::render_target(
        plan,
        &snapshot.orchestrator,
        &roles(ctx, workspace_id),
    ));
    out
}

fn push_ops(out: &mut String, ops: &[Op]) {
    for op in ops {
        out.push_str(&format!("  {}\n", tile::command_line(op)));
    }
}

fn ready_to_tile(
    ctx: &RuntimeContext,
    snapshot: &Snapshot,
    workspace_id: &str,
) -> Result<TileLock> {
    if let Some(tab) = snapshot.zoomed_tab() {
        bail!(
            "tab {tab} is zoomed, and herdr refuses to move panes into or out of a zoomed tab. \
             Unzoom it, then tile again"
        );
    }
    TileLock::acquire(ctx, workspace_id)?.with_context(|| {
        format!(
            "another tile is still running in workspace {workspace_id} (waited {}s). Try again",
            LOCK_WAIT.as_secs()
        )
    })
}

fn run_live(
    ctx: &RuntimeContext,
    herdr: &PaneHost,
    workspace_id: &str,
    snapshot: Snapshot,
    plan: &Plan,
) -> Result<String> {
    let _lock = ready_to_tile(ctx, &snapshot, workspace_id)?;
    let workers = plan.placement.len();
    let already = tile::is_canonical(&snapshot.shapes(), &snapshot.orchestrator, workers);
    let was = snapshot.focus_state();
    let was_tab = snapshot.active_tab.clone();
    let (moved, snapshot) = if already {
        (0, snapshot)
    } else {
        (apply(herdr, plan)?, gather(ctx, herdr, workspace_id)?)
    };

    let resized = balance_all(herdr, &snapshot)?;
    let focus = (!already)
        .then(|| restore_focus(herdr, &snapshot, was.as_ref(), was_tab.as_deref()))
        .flatten();
    let mut out = format!(
        "{} worker(s) over {} tab(s): {} pane move(s), {} resize(s)\n",
        workers,
        snapshot.tabs.len(),
        moved,
        resized
    );
    out.extend(focus);
    if already {
        out.push_str("the grid was already laid out; only the columns needed evening\n");
    }
    out.push('\n');
    out.push_str(&report(ctx, herdr, workspace_id)?);
    Ok(out)
}

pub fn workspace_of(
    ctx: &RuntimeContext,
    herdr: &PaneHost,
    pane: Option<&str>,
    workspace: Option<&str>,
) -> Result<String> {
    match (workspace.filter(|s| !s.is_empty()), pane) {
        (Some(ws), _) => Ok(ws.to_string()),
        (None, Some(pane)) => herdr
            .pane_get(pane)?
            .workspace_id
            .with_context(|| format!("herdr did not say which workspace holds {pane}")),
        (None, None) => Ok(Mailbox::resolve_in(herdr, ctx)
            .context(
                "horch needs --workspace, or to run inside a herdr pane or a Herdr app terminal \
                 (HERDR_PANE_ID and HC_TERMINAL_ID are unset) or with HORCH_WORKSPACE_ID set",
            )?
            .workspace_id()
            .to_string()),
    }
}

pub fn after_change(
    ctx: &RuntimeContext,
    herdr: &PaneHost,
    workspace_id: &str,
    newcomer: Option<&str>,
) {
    if !herdr.caps().geometry {
        return;
    }
    if ctx.settings.tiling == TilingMode::Disabled {
        if let Ok(layout) = herdr.pane_layout(newcomer) {
            equalize_quietly(ctx, herdr, layout);
        }
        return;
    }
    match run(ctx, herdr, workspace_id, newcomer, false) {
        Ok(_) => {}
        Err(e) => eprintln!("horch: could not lay the grid out ({e:#}); carrying on"),
    }
}

pub fn settle_after_close(ctx: &RuntimeContext, workspace_id: &str) {
    if !PaneHost::for_workspace(ctx, workspace_id).caps().geometry {
        return;
    }
    let Some(exe) = ctx.bins.current_exe.clone() else {
        return;
    };
    let mut cmd = std::process::Command::new(exe);
    let subcommand = if ctx.settings.tiling == TilingMode::Automatic {
        "tile"
    } else {
        "balance"
    };
    cmd.args([
        subcommand,
        "--workspace",
        workspace_id,
        "--settle-ms",
        "600",
    ])
    .stdin(std::process::Stdio::null())
    .stdout(std::process::Stdio::null())
    .stderr(std::process::Stdio::null());
    let _ = spawn_detached(&mut cmd);
}

pub fn report(ctx: &RuntimeContext, herdr: &PaneHost, workspace_id: &str) -> Result<String> {
    if !herdr.caps().geometry {
        return report_without_geometry(ctx, herdr, workspace_id);
    }
    let tabs = herdr.tab_list(workspace_id)?;
    let panes = herdr.pane_list(workspace_id)?;

    let roles: HashMap<String, String> = Mailbox::in_context(ctx, workspace_id)
        .panes_to_roles()
        .into_iter()
        .collect();

    let mut layouts = Vec::new();
    for tab in &tabs {
        if let Some(probe) = panes
            .iter()
            .find(|p| p.tab_id.as_deref() == Some(tab.tab_id.as_str()))
        {
            layouts.push(herdr.pane_layout(Some(&probe.pane_id))?);
        }
    }

    let orchestrator = roles
        .iter()
        .find(|(_, role)| role.as_str() == "orchestrator")
        .map(|(pane_id, _)| pane_id.clone())
        .filter(|id| layouts.iter().any(|l| holds(l, id)))
        .or_else(|| {
            layouts
                .first()
                .and_then(|l| layout::analyze_with(l, Orchestrator::Infer).orchestrator)
        });

    let workers = panes.len() - usize::from(orchestrator.is_some());
    let mut analyses = Vec::new();
    let mut notes = Vec::new();
    for (i, data) in layouts.iter().enumerate() {
        let on_this_tab = orchestrator.as_deref().filter(|id| holds(data, id));
        let who = on_this_tab.map_or(Orchestrator::Absent, Orchestrator::Pane);
        notes.push(verdict(
            data,
            on_this_tab,
            tile::workers_on_tab(i + 1, workers),
        ));
        analyses.push(layout::analyze_with(data, who));
    }
    Ok(layout::render_all(&analyses, &notes, &roles))
}

fn report_without_geometry(
    ctx: &RuntimeContext,
    herdr: &PaneHost,
    workspace_id: &str,
) -> Result<String> {
    let tabs = herdr.tab_list(workspace_id)?;
    let mut panes = herdr.pane_list(workspace_id)?;
    panes.sort_by(|a, b| {
        crate::workspace::place::address_key(&a.pane_id)
            .cmp(&crate::workspace::place::address_key(&b.pane_id))
    });
    let roles: HashMap<String, String> = Mailbox::in_context(ctx, workspace_id)
        .panes_to_roles()
        .into_iter()
        .collect();

    let mut out = format!("space {workspace_id}\n");
    for tab in &tabs {
        let label_suffix = tab
            .label
            .as_deref()
            .filter(|l| !l.is_empty())
            .map(|l| format!(" ({l})"))
            .unwrap_or_default();
        out.push_str(&format!(
            "  tab {} {}{label_suffix}\n",
            tab.number, tab.tab_id
        ));
        for pane in panes
            .iter()
            .filter(|p| p.tab_id.as_deref() == Some(tab.tab_id.as_str()))
        {
            let role = roles
                .get(&pane.pane_id)
                .map(|r| format!(" [{r}]"))
                .unwrap_or_default();
            let agent = pane.agent.as_deref().unwrap_or("shell");
            let status = pane.agent_status.as_deref().unwrap_or("none");
            out.push_str(&format!("    {}{role}: {agent} ({status})\n", pane.pane_id));
        }
    }
    Ok(out)
}

fn holds(layout: &Layout, pane_id: &str) -> bool {
    layout.panes.iter().any(|p| p.pane_id == pane_id)
}

fn verdict(layout: &Layout, orchestrator: Option<&str>, expected: usize) -> String {
    let shape = tile::TabShape::from_layout(layout);
    let panes = shape.panes.len() - usize::from(orchestrator.is_some());
    let slots = tile::capacity(if orchestrator.is_some() { 1 } else { 2 });
    if tile::tab_is_canonical(&shape, orchestrator, expected) {
        format!("matches the fleet grid, {panes}/{slots} slots filled")
    } else {
        format!("NOT the fleet grid; run `horch tile` ({panes} worker pane(s))")
    }
}

const MAX_OPS: usize = 24;

pub fn orchestrator_of(ctx: &RuntimeContext, layout: &Layout) -> Option<String> {
    Mailbox::in_context(ctx, &layout.workspace_id)
        .panes_to_roles()
        .into_iter()
        .find(|(_, role)| role == "orchestrator")
        .map(|(pane_id, _)| pane_id)
}

pub fn equalize(ctx: &RuntimeContext, herdr: &PaneHost, mut layout: Layout) -> Result<usize> {
    if !herdr.caps().geometry {
        return Ok(0);
    }
    let orch = orchestrator_of(ctx, &layout);
    let mut applied = 0;
    let mut last: Option<(String, i64)> = None;

    for _ in 0..MAX_OPS {
        let Some(op) = balance::plan(&layout, orch.as_deref()).into_iter().next() else {
            break;
        };
        if last.as_ref() == Some(&(op.pane.clone(), op.from_x)) {
            break;
        }
        last = Some((op.pane.clone(), op.from_x));

        let out = herdr.pane_resize(&op.pane, op.direction.as_str(), op.amount)?;
        applied += 1;
        if !out.changed {
            break;
        }
        layout = out.layout;
    }
    Ok(applied)
}

pub fn equalize_quietly(ctx: &RuntimeContext, herdr: &PaneHost, layout: Layout) {
    if let Err(e) = equalize(ctx, herdr, layout) {
        eprintln!("horch: could not even out the worker columns ({e:#}); carrying on");
    }
}

pub fn balance_target(
    ctx: &RuntimeContext,
    herdr: &PaneHost,
    pane: Option<&str>,
    workspace: Option<&str>,
) -> Result<Layout> {
    let target: Option<String> = match (pane, workspace) {
        (Some(p), _) => Some(p.to_string()),
        (None, Some(ws)) => herdr.pane_list(ws)?.first().map(|p| p.pane_id.clone()),
        (None, None) => match &ctx.herdr.pane {
            Some(internal) => Some(herdr.pane_get(internal.as_str())?.pane_id),
            None => None,
        },
    };
    herdr.pane_layout(target.as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_declined_move_names_the_command_and_says_nothing_was_lost() {
        let op = Op::Move {
            pane: "w0:p2".into(),
            tab: TabRef::Existing("w0:t1".into()),
            split: crate::workspace::model::Direction::Right,
            target: "w0:p1".into(),
            ratio: Some(0.5),
        };
        let err = declined(&op, false, Some("zoomed_tab"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("unzoom"), "{err}");
        assert!(err.contains("herdr pane move w0:p2"), "{err}");
        assert!(err.contains("no pane was closed"), "{err}");
        assert!(declined(&op, true, None).is_ok());
    }

    #[test]
    fn the_tile_lock_is_created_and_released_on_drop() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("tile.lock");
        let first = TileLock::at(path.clone(), LOCK_STALE)
            .unwrap()
            .expect("a free lock");
        assert!(path.exists());
        drop(first);
        assert!(!path.exists(), "the lock is released on drop");
    }

    #[test]
    fn a_lock_that_cannot_be_created_is_an_error_not_a_wait() {
        let tmp = tempfile::tempdir().unwrap();
        let err = TileLock::at(tmp.path().join("missing").join("tile.lock"), LOCK_STALE)
            .err()
            .expect("no directory, no lock");
        assert!(
            format!("{err:#}").starts_with("taking the tile lock: "),
            "{err:#}"
        );
    }

    #[test]
    fn a_stale_lock_is_taken_over_rather_than_waited_on() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("tile.lock");
        std::fs::write(&path, "").unwrap();
        let taken = TileLock::at(path.clone(), Duration::ZERO).unwrap();
        assert!(
            taken.is_some(),
            "a lock older than the stale age is taken over"
        );
        assert!(path.exists(), "and the new holder now owns it");
        drop(taken);
        assert!(!path.exists());
    }

    #[cfg(unix)]
    #[test]
    fn hcb_09_tilers_do_nothing_without_geometry() {
        use crate::runtime::MapEnv;
        use crate::workspace::hc::HcApp;
        use crate::workspace::model::{LayoutPane, Rect};

        let dir = tempfile::tempdir().unwrap();
        let hc_bin = dir.path().join("hc");
        let app_bin = dir.path().join("herdr-app");
        let sock = dir.path().join("default.sock");
        std::fs::write(&sock, b"").unwrap();
        let hc_body = r#"#!/bin/sh
if [ "$1" = warm ]; then exit 0; fi
d=$(dirname "$0")
printf 'hc %s\n' "$*" >> "$d/calls"
case "$1" in
ls) printf '{"hidden_finished":0,"terminals":[{"address":"t1"},{"address":"t2"}]}\n';;
get)
  case "$3" in
  t1) printf '{"address":"t1","state":"live","labels":{"herdr.space":"sp1","herdr.tab":"tb1"},"status":{"app":"claude-code","state":"idle"},"status_ms":10}\n';;
  t2) printf '{"address":"t2","state":"live","labels":{"herdr.space":"sp1","herdr.tab":"tb1"},"status":{"app":"codex","state":"working"},"status_ms":20}\n';;
  esac;;
esac"#;
        let app_body = r#"#!/bin/sh
if [ "$1" = warm ]; then exit 0; fi
d=$(dirname "$0")
printf 'app %s\n' "$*" >> "$d/calls"
case "$1" in
ls) printf '{"spaces":[{"id":"sp1","name":"fleet","folder":"/w","active":true,"tabs":[{"id":"tb1","name":"1","active":true,"settled":false,"terminals":[{"terminal":"t1","pane":"p1"},{"terminal":"t2","pane":"p2"}]}]}]}\n';;
*) exit 1;;
esac"#;
        assert!(
            crate::test_support::write_warmed(&hc_bin, hc_body, &["warm"])
                .status
                .success()
        );
        assert!(
            crate::test_support::write_warmed(&app_bin, app_body, &["warm"])
                .status
                .success()
        );

        let state = dir.path().join("state");
        let env = MapEnv::new(dir.path())
            .with("HOME", &dir.path().to_string_lossy())
            .with("HORCH_STATE_DIR", &state.to_string_lossy())
            .with("HC_TERMINAL_ID", "t1");
        let ctx = RuntimeContext::from_env(&env).unwrap();
        let host = PaneHost::hc_app(HcApp::new(&hc_bin, &app_bin, &sock));

        after_change(&ctx, &host, "sp1", Some("t2"));
        settle_after_close(&ctx, "sp1");
        let tile_out = run(&ctx, &host, "sp1", None, false).unwrap();
        assert_eq!(
            tile_out.trim(),
            "the Herdr app arranges its own panes; nothing to tile"
        );

        let dummy_layout = Layout {
            workspace_id: "sp1".into(),
            tab_id: "tb1".into(),
            area: Rect {
                x: 0,
                y: 0,
                width: 200,
                height: 60,
            },
            panes: vec![LayoutPane {
                pane_id: "t1".into(),
                rect: Rect {
                    x: 0,
                    y: 0,
                    width: 20,
                    height: 60,
                },
            }],
            focused_pane_id: None,
            zoomed: false,
        };
        assert_eq!(equalize(&ctx, &host, dummy_layout).unwrap(), 0);
        assert!(
            !dir.path().join("calls").exists(),
            "after_change, settle_after_close, run and equalize must make zero host calls without geometry"
        );

        let rep = report(&ctx, &host, "sp1").unwrap();
        assert!(rep.contains("sp1"), "{rep}");
        assert!(rep.contains("tb1"), "{rep}");
        assert!(rep.contains("t1") && rep.contains("idle"), "{rep}");
        assert!(rep.contains("t2") && rep.contains("working"), "{rep}");
    }
}
