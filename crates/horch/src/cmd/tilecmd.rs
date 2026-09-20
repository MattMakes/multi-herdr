//! `horch tile` - rearrange every pane in the workspace into the fleet grid.
//!
//! The planning is pure and lives in [`horch_core::tile`]; this is the part that
//! talks to herdr. What it adds on top of the plan is everything that needs the
//! live workspace: reading it, substituting the ids of tabs the plan creates,
//! stopping when herdr declines a move, putting focus back, evening the columns
//! out, and keeping two tilers from running at once.
//!
//! `horch spawn` and `horch done` call this themselves (see [`after_change`] and
//! [`settle_after_close`]), so the grid is laid out without an agent spending a
//! token on it. `HORCH_TILE=0` puts both back to the old behaviour of only
//! evening out the columns.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use anyhow::{bail, Context, Result};
use horch_core::herdr::{Herdr, Layout};
use horch_core::mailbox::Mailbox;
use horch_core::tile::{self, Fleet, FocusState, Op, Plan, TabRef, TabShape, Worker};

use crate::output;

/// One resize per divider, plus slack for a divider that lands a cell out and
/// wants a second nudge. The same budget `horch balance` uses.
const MAX_BALANCE_OPS: usize = 24;

/// How long a tiler waits for another one to finish before giving up.
const LOCK_WAIT: Duration = Duration::from_secs(20);
/// A lock older than this is from a run that died; no tile takes minutes.
const LOCK_STALE: Duration = Duration::from_secs(60);

/// One tab of the workspace as it is now.
struct TabState {
    tab_id: String,
    layout: Layout,
}

/// The whole workspace as one gather read it.
struct Snapshot {
    tabs: Vec<TabState>,
    orchestrator: String,
    orchestrator_tab: String,
    /// The tab the workspace is showing, so focus can be handed back.
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

    /// Where the operator is looking: the viewed tab and the pane that tab hands
    /// the keyboard to. `None` when herdr reported neither, which is the case on
    /// a herdr too old to report `focused_pane_id`; the caller then keeps the
    /// old tab-only restore.
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

    /// The tab that is zoomed, if any. herdr refuses to move panes into or out of
    /// a zoomed tab, reporting `changed: false` rather than failing, so a tiler
    /// that ignored this would walk through a plan achieving nothing.
    fn zoomed_tab(&self) -> Option<&str> {
        self.tabs
            .iter()
            .find(|t| t.layout.zoomed)
            .map(|t| t.tab_id.as_str())
    }
}

/// Read every tab of the workspace, and work out which pane is the orchestrator.
///
/// One `tab list` plus one `pane list` plus one `pane layout` per tab: `pane
/// layout` reports the whole tab that holds the pane it is asked about, and every
/// pane in `pane list` carries its `tab_id`, so one pane per tab is enough.
fn gather(herdr: &Herdr, workspace_id: &str) -> Result<Snapshot> {
    let tabs = herdr.tab_list(workspace_id)?;
    let panes = herdr.pane_list(workspace_id)?;
    let mut states = Vec::new();
    for tab in &tabs {
        let Some(probe) = panes
            .iter()
            .find(|p| p.tab_id.as_deref() == Some(tab.tab_id.as_str()))
        else {
            continue;
        };
        states.push(TabState {
            tab_id: tab.tab_id.clone(),
            layout: herdr.pane_layout(Some(&probe.pane_id))?,
        });
    }
    if states.is_empty() {
        bail!("workspace {workspace_id} has no panes");
    }

    // The mailbox is the registry; the leftmost full-height pane of the first tab
    // is the same guess `horch layout` has always made. Never a silent third
    // option: tiling a workspace whose orchestrator cannot be identified would
    // file it as a worker and move it.
    let registered = Mailbox::new(workspace_id)
        .panes_to_roles()
        .into_iter()
        .find(|(_, role)| role == "orchestrator")
        .map(|(pane_id, _)| pane_id)
        .filter(|id| {
            states
                .iter()
                .any(|t| t.layout.panes.iter().any(|p| &p.pane_id == id))
        });
    let orchestrator = match registered {
        Some(id) => id,
        None => horch_core::layout::analyze_with(
            &states[0].layout,
            horch_core::layout::Orchestrator::Infer,
        )
        .orchestrator
        .context(
            "no orchestrator in this workspace: no pane is registered for the role and no \
             full-height pane sits on the first tab. Register one with `horch register \
             orchestrator` in its pane, then tile again",
        )?,
    };
    let orchestrator_tab = states
        .iter()
        .find(|t| t.layout.panes.iter().any(|p| p.pane_id == orchestrator))
        .map(|t| t.tab_id.clone())
        .expect("the orchestrator was found in one of these tabs");

    let active_tab = herdr
        .workspace_list()
        .ok()
        .and_then(|list| {
            list.into_iter()
                .find(|w| w.workspace_id == workspace_id)
                .and_then(|w| w.active_tab_id)
        })
        .or_else(|| Some(orchestrator_tab.clone()));

    Ok(Snapshot {
        tabs: states,
        orchestrator,
        orchestrator_tab,
        active_tab,
    })
}

/// Run the plan. Returns how many panes moved.
///
/// A declined move aborts the run rather than carrying on: every later op targets
/// a pane the earlier ones were supposed to have placed, so continuing would
/// build a shape nobody planned. Aborting is safe - no pane is ever closed, so
/// the workers sit in the scratch tab and the next run gathers them like any
/// other panes and finishes the job.
fn apply(herdr: &Herdr, plan: &Plan) -> Result<usize> {
    let mut created: HashMap<usize, String> = HashMap::new();
    let mut moved = 0;

    for op in plan.ops() {
        match op {
            Op::NewTab {
                pane,
                label,
                creates,
            } => {
                let result = herdr.pane_move_new_tab(pane, label)?;
                declined(op, result.changed, result.reason.as_deref())?;
                let tab = result
                    .created_tab
                    .map(|t| t.tab_id)
                    .or_else(|| result.pane.tab_id.clone())
                    .with_context(|| {
                        format!("herdr moved {pane} to a new tab but did not name it")
                    })?;
                created.insert(*creates, tab);
                moved += 1;
            }
            Op::Move {
                pane,
                tab,
                split,
                target,
                ratio,
            } => {
                let tab_id = match tab {
                    TabRef::Existing(id) => id.clone(),
                    TabRef::Created(i) => created
                        .get(i)
                        .cloned()
                        .with_context(|| format!("tab {i} was never created"))?,
                };
                let result = herdr.pane_move(pane, &tab_id, *split, Some(target), *ratio)?;
                declined(op, result.changed, result.reason.as_deref())?;
                moved += 1;
            }
            Op::FocusTab { tab } => {
                if let TabRef::Existing(id) = tab {
                    herdr.tab_focus(id)?;
                }
            }
        }
    }
    Ok(moved)
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

/// Even out one tab's columns, returning how many resizes were sent.
///
/// Re-plans against the layout herdr hands back after each resize, so rounding
/// cannot accumulate. Two things stop it: `changed: false`, which is how the
/// minimum pane width announces itself, and a divider asked to move from the same
/// place twice, which means the previous op did not land.
fn balance_tab(herdr: &Herdr, mut layout: Layout, orchestrator: Option<&str>) -> Result<usize> {
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

/// Even out every tab: equal columns, and the orchestrator on its share of tab 1.
fn balance_all(herdr: &Herdr, snapshot: &Snapshot) -> Result<usize> {
    let mut applied = 0;
    for tab in &snapshot.tabs {
        let orchestrator = if tab.tab_id == snapshot.orchestrator_tab {
            Some(snapshot.orchestrator.as_str())
        } else {
            None
        };
        applied += balance_tab(herdr, tab.layout.clone(), orchestrator)?;
    }
    Ok(applied)
}

/// Put the operator's view back where it was, and say what happened.
///
/// Why this is needed at all: the park phase moves every worker out of its tab
/// with `--no-focus`, so herdr hands that tab's focus to a pane that stayed -
/// on tab 1 always the orchestrator - and nothing gives it back, because every
/// move that follows also passes `--no-focus`. Restoring the tab alone is not
/// enough, and restoring the tab BY ID is not enough either: an overflow tab
/// loses its last pane to the park phase and is rebuilt under a new id, so the
/// view has to follow the pane. `ai_docs/reports/horch-tile-focus.md` measures
/// both.
///
/// Best effort throughout. A view that cannot be restored must never fail a
/// tiling that has already moved the panes.
fn restore_focus(herdr: &Herdr, after: &Snapshot, before: Option<&FocusState>) -> Option<String> {
    let Some(before) = before else {
        // herdr never told us which pane had the keyboard, so this is the old
        // behaviour: the viewed tab if it survived, else the orchestrator's.
        let tab = after.active_tab.clone()?;
        let _ = herdr.tab_focus(if after.tabs.iter().any(|t| t.tab_id == tab) {
            &tab
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
        // Nothing to aim at, so herdr's own choice is the answer, not a failure.
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

/// A workspace-wide lock, so two tilers never interleave their moves.
///
/// `horch balance` needs no lock: its target is absolute, so two passes aim at
/// the same schedule and at worst repeat each other's work. Tiling is not like
/// that - it moves panes out of their tabs and back, and a second tiler reading
/// the workspace half way through that would see a shape neither of them planned.
///
/// A tiler WAITS rather than skipping, because the caller has a change to apply:
/// a spawn that skipped tiling would leave its new worker wherever the split put
/// it until something else happened to tile.
struct TileLock {
    path: PathBuf,
}

impl TileLock {
    fn acquire(workspace_id: &str) -> Result<Option<Self>> {
        let dir = Mailbox::new(workspace_id).dir().to_path_buf();
        std::fs::create_dir_all(&dir)?;
        Self::at(dir.join("tile.lock"), LOCK_STALE)
    }

    /// `stale_after` is a parameter so a test can say "anything already there is
    /// stale" without waiting a minute.
    fn at(path: PathBuf, stale_after: Duration) -> Result<Option<Self>> {
        let deadline = SystemTime::now() + LOCK_WAIT;
        loop {
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(_) => return Ok(Some(Self { path })),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(e).context("taking the tile lock"),
            }
            // A run that died leaves its lock behind; nothing legitimate holds it
            // for a minute.
            let stale = std::fs::metadata(&path)
                .and_then(|m| m.modified())
                .map(|m| m.elapsed().unwrap_or_default() >= stale_after)
                .unwrap_or(false);
            if stale {
                let _ = std::fs::remove_file(&path);
                continue;
            }
            if SystemTime::now() > deadline {
                return Ok(None);
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    }
}

impl Drop for TileLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Is automatic tiling on? `HORCH_TILE=0` turns it off and puts `horch spawn` and
/// `horch done` back to only evening out the columns.
pub fn enabled() -> bool {
    !matches!(
        std::env::var("HORCH_TILE").unwrap_or_default().as_str(),
        "0" | "false" | "no" | "off"
    )
}

/// Roles by pane id, for labelling a report.
fn roles(workspace_id: &str) -> HashMap<String, String> {
    Mailbox::new(workspace_id)
        .panes_to_roles()
        .into_iter()
        .collect()
}

/// Lay the whole workspace out. The one path both the command and the hooks use.
///
/// `newcomer` is a pane that must be placed last whatever its rectangle says;
/// `horch spawn` passes the pane it just made.
fn run(
    herdr: &Herdr,
    workspace_id: &str,
    newcomer: Option<&str>,
    plan_only: bool,
) -> Result<String> {
    let snapshot = gather(herdr, workspace_id)?;
    let fleet = snapshot.fleet(newcomer);
    let plan = tile::plan(&fleet);
    let workers = plan.placement.len();

    if plan_only {
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
            for op in &plan.park {
                out.push_str(&format!("  {}\n", tile::command_line(op)));
            }
            out.push_str("\nthen place them, slot by slot:\n");
            for op in &plan.place {
                out.push_str(&format!("  {}\n", tile::command_line(op)));
            }
            out.push_str(
                "\nemptied tabs close themselves, so no tab is ever closed by hand.\n\
                 then: even out every column, and put focus back where it was.\n\n",
            );
        }
        out.push_str(&tile::render_target(
            &plan,
            &snapshot.orchestrator,
            &roles(workspace_id),
        ));
        return Ok(out);
    }

    if let Some(tab) = snapshot.zoomed_tab() {
        bail!(
            "tab {tab} is zoomed, and herdr refuses to move panes into or out of a zoomed tab. \
             Unzoom it, then tile again"
        );
    }

    let Some(_lock) = TileLock::acquire(workspace_id)? else {
        bail!(
            "another tile is still running in workspace {workspace_id} (waited {}s). Try again",
            LOCK_WAIT.as_secs()
        );
    };

    // Fast path: the grid is already the right shape, so nothing is moved and only
    // the columns are evened out. This is the common case for a re-run, and it is
    // what keeps `horch spawn` from rebuilding a grid that was already right.
    let already = tile::is_canonical(&snapshot.shapes(), &snapshot.orchestrator, workers);
    let mut moved = 0;
    // Read where the operator is looking BEFORE anything moves. Both values are
    // already in the snapshot, so this costs no herdr call.
    let was = snapshot.focus_state();
    let snapshot = if already {
        snapshot
    } else {
        moved = apply(herdr, &plan)?;
        // Tabs have appeared and disappeared; read it all again for the balance
        // and the report.
        gather(herdr, workspace_id)?
    };

    let resized = balance_all(herdr, &snapshot)?;
    // After the balance, not before it: the view should settle once, on the
    // finished grid. The fast path moved nothing, so there is nothing to put
    // back - and a focus call herdr does not need would pull the workspace into
    // view for no reason.
    let focus = if already {
        None
    } else {
        restore_focus(herdr, &snapshot, was.as_ref())
    };
    let mut out = format!(
        "{} worker(s) over {} tab(s): {} pane move(s), {} resize(s)\n",
        workers,
        snapshot.tabs.len(),
        moved,
        resized
    );
    if let Some(line) = focus {
        out.push_str(&line);
    }
    if already && moved == 0 {
        out.push_str("the grid was already laid out; only the columns needed evening\n");
    }
    out.push('\n');
    out.push_str(&crate::cmd::layoutcmd::report(herdr, workspace_id)?);
    Ok(out)
}

/// `horch tile`.
pub fn tile(
    pane: Option<&str>,
    workspace: Option<&str>,
    plan_only: bool,
    settle_ms: u64,
) -> Result<()> {
    if settle_ms > 0 {
        std::thread::sleep(Duration::from_millis(settle_ms));
    }
    let herdr = Herdr::new();
    let workspace_id = workspace_of(&herdr, pane, workspace)?;
    let out = run(&herdr, &workspace_id, None, plan_only)?;
    output::print(&out);
    Ok(())
}

/// Which workspace to work on: one named explicitly, the one holding a named
/// pane, else this pane's own.
pub fn workspace_of(herdr: &Herdr, pane: Option<&str>, workspace: Option<&str>) -> Result<String> {
    if let Some(ws) = workspace.filter(|s| !s.is_empty()) {
        return Ok(ws.to_string());
    }
    if let Some(pane) = pane {
        return herdr
            .pane_get(pane)?
            .workspace_id
            .with_context(|| format!("herdr did not say which workspace holds {pane}"));
    }
    Ok(Mailbox::resolve(herdr)
        .context(
            "horch needs --workspace, or to run inside a herdr pane (HERDR_PANE_ID) or with \
             HORCH_WORKSPACE_ID set",
        )?
        .workspace_id()
        .to_string())
}

/// Lay the grid out after something changed it, without failing the caller.
///
/// `horch spawn` calls this once the new pane is running. A grid that will not
/// lay out must never fail a spawn, so problems are reported and life goes on.
pub fn after_change(herdr: &Herdr, workspace_id: &str, newcomer: Option<&str>) {
    if !enabled() {
        // The operator asked for the old behaviour: even the columns and nothing
        // else.
        if let Ok(layout) = herdr.pane_layout(newcomer) {
            crate::cmd::balancecmd::equalize_quietly(herdr, layout);
        }
        return;
    }
    match run(herdr, workspace_id, newcomer, false) {
        Ok(_) => {}
        Err(e) => eprintln!("horch: could not lay the grid out ({e:#}); carrying on"),
    }
}

/// Lay the grid out once this pane has finished closing.
///
/// `herdr pane close` takes down the calling pane's whole process tree, so a
/// worker shutting itself down cannot do this in-process: by the time the hole it
/// leaves exists, it has been killed. This hands the job to a detached child that
/// outlives the pane, which fills the hole and lets herdr close an emptied
/// overflow tab.
pub fn settle_after_close(workspace_id: &str) {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let mut cmd = std::process::Command::new(exe);
    let subcommand = if enabled() { "tile" } else { "balance" };
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
    // A new SESSION, not merely a new process group. Measured against herdr
    // 0.7.4: closing a pane tears down its whole session, so a child that only
    // left the process group is killed along with it, while one that called
    // `setsid` survives.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // SAFETY: runs in the forked child between fork and exec. `setsid` is
        // async-signal-safe and touches no state this process shares.
        unsafe {
            cmd.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }
    }
    let _ = cmd.spawn();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiling_is_on_unless_the_operator_switches_it_off() {
        for (value, on) in [
            ("", true),
            ("1", true),
            ("yes", true),
            ("0", false),
            ("false", false),
            ("no", false),
            ("off", false),
        ] {
            std::env::set_var("HORCH_TILE", value);
            assert_eq!(enabled(), on, "HORCH_TILE={value}");
        }
        std::env::remove_var("HORCH_TILE");
        assert!(enabled(), "on by default");
    }

    #[test]
    fn a_declined_move_names_the_command_and_says_nothing_was_lost() {
        let op = Op::Move {
            pane: "w0:p2".into(),
            tab: TabRef::Existing("w0:t1".into()),
            split: horch_core::herdr::Direction::Right,
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

    /// One tiler at a time, and the lock goes away when the run ends however it
    /// ends.
    #[test]
    fn the_tile_lock_excludes_a_second_tiler_and_is_released_on_drop() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("tile.lock");
        let first = TileLock::at(path.clone(), LOCK_STALE)
            .unwrap()
            .expect("a free lock");
        assert!(path.exists());
        drop(first);
        assert!(!path.exists(), "the lock is released on drop");
    }

    /// A lock left behind by a run that died must not wedge the fleet: nothing
    /// legitimate holds it for a minute, so an old one is taken over.
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
}
