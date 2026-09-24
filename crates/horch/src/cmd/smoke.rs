//! `horch smoke` - the port of the `horch1-smoke` and `herdr-fleet-smoke` recipes.
//!
//! These are the acceptance tests for the machinery. They spend no LLM tokens and
//! clean up after themselves on success, leaving the scratch workspace open for
//! inspection on failure.

use std::path::Path;
use std::process::ExitCode;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::Subcommand;
use horch_core::herdr::{Direction, Herdr, Layout, Rect};
use horch_core::ledger::Ledger;
use horch_core::mailbox::Mailbox;
use horch_core::paneshell::PaneShell;

use super::doctor;
use super::spawn::{spawn, SpawnArgs};

#[derive(Subcommand)]
pub enum SmokeCommand {
    /// Two-pane check of the herdr messaging primitives, with no agents involved.
    /// Run this first against a new herdr install.
    Messaging,
    /// End-to-end check of the fleet machinery using a token-free fake agent.
    Fleet,
    /// Check that `horch tile` rebuilds the grid across tabs without killing a
    /// pane, and that `horch spawn` and a worker's own `horch done` run it.
    Tile,
}

pub fn run(command: SmokeCommand) -> Result<ExitCode> {
    match command {
        SmokeCommand::Messaging => messaging(),
        SmokeCommand::Fleet => fleet(),
        SmokeCommand::Tile => tile(),
    }
}

/// Poll `check` every `interval` until it returns true or `attempts` run out.
fn poll(attempts: u32, interval: Duration, mut check: impl FnMut() -> bool) -> bool {
    for _ in 0..attempts {
        if check() {
            return true;
        }
        std::thread::sleep(interval);
    }
    false
}

/// A shell command that prints `SMOKE_TEST_42` only when the pane's shell actually
/// evaluates it.
///
/// Matching the arithmetic *result* rather than the sent text is the point:
/// matching the text alone would false-pass on an input line that was pasted but
/// never submitted, which is exactly the failure `horch tell` works around.
fn arithmetic_echo(shell: PaneShell) -> &'static str {
    match shell {
        PaneShell::Posix => "echo SMOKE_TEST_$((40+2))",
        PaneShell::PowerShell => "Write-Output \"SMOKE_TEST_$(40+2)\"",
    }
}

/// Cheap, self-verifying 2-pane check of the herdr messaging primitives
/// (send-text + send-keys enter, and the mailbox registry).
fn messaging() -> Result<ExitCode> {
    doctor::check()?;
    let herdr = Herdr::new();
    let exe = std::env::current_exe().context("locating the horch binary")?;
    let shell = PaneShell::host();

    let ws = herdr.workspace_create("horch smoke test", None, false)?;
    let a = ws.root_pane_id.clone();
    let b = herdr.pane_split(&a, Direction::Right)?;

    for (pane, role) in [(&a, "a"), (&b, "b")] {
        herdr.pane_run(pane, &shell.command_line(&exe, &["register", role]))?;
    }

    // Registration shells out to `herdr pane get`; poll the mailbox rather than
    // guessing a sleep.
    let mailbox = Mailbox::new(&ws.workspace_id);
    let registered = poll(20, Duration::from_millis(500), || {
        mailbox.pane_for("a").is_some() && mailbox.pane_for("b").is_some()
    });
    if !registered {
        eprintln!(
            "FAIL: panes never registered (no {}/{{a,b}}.id). \
             Leaving workspace {} open for inspection.",
            mailbox.dir().display(),
            ws.workspace_id
        );
        return Ok(ExitCode::FAILURE);
    }

    println!("Sending a test message a -> b via horch tell...");
    let target = mailbox.pane_for("b").expect("just polled");
    herdr.send_line(&target, arithmetic_echo(shell))?;

    if herdr.wait_output(&b, "SMOKE_TEST_42", 15_000)? {
        println!(
            "PASS: send-text + send-keys enter delivered and executed the message end to end."
        );
        herdr.workspace_close(&ws.workspace_id)?;
        return Ok(ExitCode::SUCCESS);
    }

    eprintln!(
        "FAIL: SMOKE_TEST_42 not observed on pane {b}. \
         Leaving workspace {} open for inspection.",
        ws.workspace_id
    );
    eprintln!("--- pane {b} contents ---");
    match herdr.pane_read(&b, "recent-unwrapped") {
        Ok(contents) => eprintln!("{contents}"),
        Err(e) => eprintln!("(could not read pane: {e:#})"),
    }
    Ok(ExitCode::FAILURE)
}

/// Self-verifying check of the fleet machinery (spawn -> brief -> register ->
/// ledger add/note/done -> tell -> pane self-close) using a token-free fake agent
/// in a scratch workspace with an isolated ledger.
fn fleet() -> Result<ExitCode> {
    doctor::check()?;
    let herdr = Herdr::new();
    let exe = std::env::current_exe().context("locating the horch binary")?;
    let shell = PaneShell::host();

    // Isolated state so the check never touches a real project ledger.
    let state_dir = tempfile::Builder::new()
        .prefix("horch-smoke-state")
        .tempdir()?;
    let project_dir = tempfile::Builder::new()
        .prefix("horch-smoke-proj")
        .tempdir()?;
    let project = project_dir.path().to_string_lossy().into_owned();
    std::env::set_var("HORCH_STATE_DIR", state_dir.path());
    std::env::set_var("HORCH_PROJECT_DIR", &project);

    let ws = herdr.workspace_create("herdr-fleet smoke", Some(&project), false)?;
    std::env::set_var("HORCH_WORKSPACE_ID", &ws.workspace_id);
    let mailbox = Mailbox::new(&ws.workspace_id);

    // A fake orchestrator (no agent) so the worker's `horch done` has a
    // `horch tell` target to report DONE to.
    herdr.pane_run(
        &ws.root_pane_id,
        &shell.command_line(&exe, &["register", "orchestrator"]),
    )?;
    if !poll(20, Duration::from_millis(500), || {
        mailbox.pane_for("orchestrator").is_some()
    }) {
        eprintln!(
            "FAIL: orchestrator pane never registered. Workspace {} left open.",
            ws.workspace_id
        );
        return Ok(ExitCode::FAILURE);
    }

    println!("Spawning smoke worker...");
    let pane = spawn(SpawnArgs {
        teammate: Some("smoke".to_string()),
        phase: None,
        effort: None,
        task: "verify fleet machinery".to_string(),
        resume: None,
        role: None,
        from_pane: Some(ws.root_pane_id.clone()),
        direction: Direction::Right,
        // Tiling on, so this check covers the hook `horch spawn` now runs.
        no_tile: false,
    })?;

    let ledger = Ledger::open()?;
    let reached_done = poll(30, Duration::from_secs(1), || smoke_session_done(&ledger));
    if !reached_done {
        eprintln!(
            "FAIL: smoke session never reached done in the ledger. Workspace {} left open.",
            ws.workspace_id
        );
        dump_ledger(ledger.path());
        return Ok(ExitCode::FAILURE);
    }

    // The worker must have closed its own pane.
    if !poll(10, Duration::from_secs(1), || !herdr.pane_exists(&pane)) {
        eprintln!(
            "FAIL: smoke worker did not close its own pane {pane}. Workspace {} left open.",
            ws.workspace_id
        );
        return Ok(ExitCode::FAILURE);
    }

    println!(
        "PASS: spawn, brief, register, ledger add/note/done, tell, and pane self-close \
         all verified."
    );
    herdr.workspace_close(&ws.workspace_id)?;
    Ok(ExitCode::SUCCESS)
}

/// A pane that counts forever and writes the count to a FILE.
///
/// The file, not the terminal, is what proves the process survived a move: a pane
/// that has just been squeezed to a few columns by a deliberately bad split has
/// nothing readable on screen, and `pane read` would report that as a dead
/// process.
fn ticker(shell: PaneShell, path: &Path) -> String {
    let path = path.to_string_lossy();
    match shell {
        PaneShell::Posix => {
            format!("i=0; while true; do i=$((i+1)); echo $i > '{path}'; sleep 1; done")
        }
        PaneShell::PowerShell => format!(
            "$i=0; while($true){{$i++; Set-Content -Path '{path}' -Value $i; Start-Sleep 1}}"
        ),
    }
}

/// The count a ticker pane has reached, or None while it has not written yet.
fn ticks(path: &Path) -> Option<u64> {
    std::fs::read_to_string(path).ok()?.trim().parse().ok()
}

/// Every tab of a workspace, in tab-strip order.
fn tabs_of(herdr: &Herdr, workspace_id: &str) -> Result<Vec<Layout>> {
    let tabs = herdr.tab_list(workspace_id)?;
    let panes = herdr.pane_list(workspace_id)?;
    let mut out = Vec::new();
    for tab in &tabs {
        if let Some(probe) = panes
            .iter()
            .find(|p| p.tab_id.as_deref() == Some(tab.tab_id.as_str()))
        {
            out.push(herdr.pane_layout(Some(&probe.pane_id))?);
        }
    }
    Ok(out)
}

/// One tab split into the orchestrator, the top row and the bottom row, each row
/// left to right.
fn rows(layout: &Layout, orchestrator: Option<&str>) -> (Option<Rect>, Vec<Rect>, Vec<Rect>) {
    let area = layout.area;
    let orch = orchestrator.and_then(|id| {
        layout
            .panes
            .iter()
            .find(|p| p.pane_id == id)
            .map(|p| p.rect)
    });
    let mut top: Vec<Rect> = Vec::new();
    let mut bottom: Vec<Rect> = Vec::new();
    for pane in &layout.panes {
        if Some(pane.pane_id.as_str()) == orchestrator {
            continue;
        }
        if (pane.rect.y - area.y).abs() <= 1 {
            top.push(pane.rect);
        } else {
            bottom.push(pane.rect);
        }
    }
    top.sort_by_key(|r| r.x);
    bottom.sort_by_key(|r| r.x);
    (orch, top, bottom)
}

/// Check one tab against the shape the fleet wants, by rectangle rather than by
/// asking the tiler what it thinks it built. Returns every complaint.
///
/// `workers` is how many panes the tab should hold, and the row counts follow:
/// every column has a top, so `ceil(workers / 2)` columns and the rest bottoms,
/// with the last bottom spanning the free slot when the count is odd.
fn tab_complaints(layout: &Layout, orchestrator: Option<&str>, workers: usize) -> Vec<String> {
    let mut bad = Vec::new();
    let area = layout.area;
    let (orch, top, bottom) = rows(layout, orchestrator);
    let tab = &layout.tab_id;

    let mut x0 = area.x;
    if orchestrator.is_some() {
        match orch {
            None => bad.push(format!("{tab}: the orchestrator pane is not on this tab")),
            Some(o) => {
                if (o.height - area.height).abs() > 1 {
                    bad.push(format!(
                        "{tab}: the orchestrator is {} rows tall, not the full {}",
                        o.height, area.height
                    ));
                }
                if (o.x - area.x).abs() > 1 {
                    bad.push(format!(
                        "{tab}: the orchestrator is not leftmost (x {})",
                        o.x
                    ));
                }
                x0 = o.x + o.width;
            }
        }
    }
    let columns = workers.div_ceil(2);
    if top.len() + bottom.len() != workers {
        bad.push(format!(
            "{tab}: {} worker pane(s), expected {workers}",
            top.len() + bottom.len()
        ));
        return bad;
    }
    if top.len() != columns || bottom.len() != workers - columns {
        bad.push(format!(
            "{tab}: {} top and {} bottom, expected {columns} top and {} bottom",
            top.len(),
            bottom.len(),
            workers - columns
        ));
        return bad;
    }
    // Both rows start at the worker area and reach its right edge, with no gap
    // and no overlap in between.
    for (name, row) in [("top", &top), ("bottom", &bottom)] {
        if row.is_empty() {
            continue;
        }
        if (row[0].x - x0).abs() > 1 {
            bad.push(format!(
                "{tab}: the {name} row starts at {}, not {x0}",
                row[0].x
            ));
        }
        let right = row[row.len() - 1].x + row[row.len() - 1].width;
        if (right - (area.x + area.width)).abs() > 1 {
            bad.push(format!(
                "{tab}: the {name} row ends at {right}, not {}",
                area.x + area.width
            ));
        }
        for pair in row.windows(2) {
            if (pair[0].x + pair[0].width - pair[1].x).abs() > 1 {
                bad.push(format!(
                    "{tab}: a gap or overlap in the {name} row at x {}",
                    pair[1].x
                ));
            }
        }
    }
    if !bottom.is_empty() {
        if (bottom[0].y - (area.y + top[0].height)).abs() > 1 {
            bad.push(format!(
                "{tab}: the bottom row starts at y {}, not under the top row at {}",
                bottom[0].y,
                area.y + top[0].height
            ));
        }
        if (top[0].height + bottom[0].height - area.height).abs() > 1 {
            bad.push(format!(
                "{tab}: the two rows are {} rows, not the tab's {}",
                top[0].height + bottom[0].height,
                area.height
            ));
        }
        // Columns are filled top before bottom, so each bottom pane sits under the
        // column of the same index and only the last may span the free slot.
        for (i, b) in bottom.iter().enumerate() {
            if (b.x - top[i].x).abs() > 1 {
                bad.push(format!(
                    "{tab}: bottom pane at x {} is not under the column at x {}",
                    b.x, top[i].x
                ));
            }
        }
    }
    bad
}

/// Check the whole workspace, tab by tab. `per_tab` is how many workers each tab
/// should hold, in tab order.
fn grid_complaints(
    herdr: &Herdr,
    workspace_id: &str,
    orchestrator: &str,
    per_tab: &[usize],
) -> Result<Vec<String>> {
    let tabs = tabs_of(herdr, workspace_id)?;
    if tabs.len() != per_tab.len() {
        return Ok(vec![format!(
            "{} tab(s), expected {}: {:?}",
            tabs.len(),
            per_tab.len(),
            tabs.iter().map(|t| t.tab_id.clone()).collect::<Vec<_>>()
        )]);
    }
    let mut bad = Vec::new();
    for (i, layout) in tabs.iter().enumerate() {
        let orch = if i == 0 { Some(orchestrator) } else { None };
        bad.extend(tab_complaints(layout, orch, per_tab[i]));
    }
    Ok(bad)
}

/// Where the operator is looking: the viewed tab, and the pane that tab hands
/// the keyboard to.
///
/// The pair `horch tile` has to leave alone. Before the focus restore landed,
/// tiling handed the keyboard to the orchestrator pane every time, because the
/// park phase moves the focused pane out with `--no-focus` and herdr then picks
/// a pane that stayed (`ai_docs/reports/horch-tile-focus.md`).
fn viewed(herdr: &Herdr, workspace_id: &str) -> Result<(String, String)> {
    let tab = herdr
        .workspace_list()?
        .into_iter()
        .find(|w| w.workspace_id == workspace_id)
        .and_then(|w| w.active_tab_id)
        .context("herdr did not say which tab the workspace is showing")?;
    let probe = herdr
        .pane_list(workspace_id)?
        .into_iter()
        .find(|p| p.tab_id.as_deref() == Some(tab.as_str()))
        .context("the viewed tab has no panes")?;
    let pane = herdr
        .pane_layout(Some(&probe.pane_id))?
        .focused_pane_id
        .context("herdr did not say which pane has the focus")?;
    Ok((tab, pane))
}

fn fail(workspace_id: &str, what: &str, complaints: &[String]) -> ExitCode {
    eprintln!("FAIL: {what}");
    for c in complaints {
        eprintln!("  - {c}");
    }
    eprintln!("Workspace {workspace_id} left open for inspection.");
    ExitCode::FAILURE
}

/// Self-verifying check of `horch tile`: a deliberately bad shape becomes the
/// canonical grid across two tabs without killing a pane, `horch spawn` lays the
/// grid out by itself, and a departing worker's hole is filled.
///
/// Spends no tokens: the worker panes are shells counting into a file, and the one
/// real spawn uses the `smoke` teammate, which has no agent.
fn tile() -> Result<ExitCode> {
    doctor::check()?;
    let herdr = Herdr::new();
    let exe = std::env::current_exe().context("locating the horch binary")?;
    let shell = PaneShell::host();
    let counters = tempfile::Builder::new()
        .prefix("horch-smoke-tick")
        .tempdir()?;

    // Isolated state, so the spawn in part 2 never touches a real ledger.
    let state_dir = tempfile::Builder::new()
        .prefix("horch-smoke-state")
        .tempdir()?;
    let project_dir = tempfile::Builder::new()
        .prefix("horch-smoke-proj")
        .tempdir()?;
    let project = project_dir.path().to_string_lossy().into_owned();
    std::env::set_var("HORCH_STATE_DIR", state_dir.path());
    std::env::set_var("HORCH_PROJECT_DIR", &project);

    let ws = herdr.workspace_create("horch tile smoke", Some(&project), false)?;
    std::env::set_var("HORCH_WORKSPACE_ID", &ws.workspace_id);
    let orchestrator = ws.root_pane_id.clone();
    let mailbox = Mailbox::new(&ws.workspace_id);

    // The orchestrator must be REGISTERED: the bad shape below leaves no
    // full-height pane, so the positional guess has nothing to go on and tiling
    // refuses rather than moving the orchestrator like a worker.
    herdr.pane_run(
        &orchestrator,
        &shell.command_line(&exe, &["register", "orchestrator"]),
    )?;
    if !poll(20, Duration::from_millis(500), || {
        mailbox.pane_for("orchestrator").is_some()
    }) {
        return Ok(fail(
            &ws.workspace_id,
            "the orchestrator pane never registered",
            &[],
        ));
    }

    // Part 1: 7 workers in the worst shape there is - one column of down splits,
    // so the last of them are squeezed to nothing - and every one of them running.
    println!("Building a deliberately bad shape: 7 panes in one column...");
    let mut workers = Vec::new();
    let mut previous = orchestrator.clone();
    for i in 1..=7 {
        let pane = herdr.pane_split(&previous, Direction::Down)?;
        let counter = counters.path().join(format!("w{i}"));
        herdr.pane_run(&pane, &ticker(shell, &counter))?;
        workers.push((pane.clone(), counter));
        previous = pane;
    }
    let started = poll(30, Duration::from_millis(500), || {
        workers.iter().all(|(_, c)| ticks(c).is_some())
    });
    if !started {
        return Ok(fail(
            &ws.workspace_id,
            "not every worker pane started counting",
            &workers
                .iter()
                .filter(|(_, c)| ticks(c).is_none())
                .map(|(p, _)| format!("{p} never wrote a count"))
                .collect::<Vec<_>>(),
        ));
    }
    let before: Vec<u64> = workers.iter().map(|(_, c)| ticks(c).unwrap()).collect();

    // Watch a WORKER, not the orchestrator: the orchestrator is the pane the
    // tiler used to hand the keyboard to, so focusing it would pass either way.
    // The topmost worker of the single column sorts first, so it lands in slot 1
    // of tab 1 and is still there afterwards.
    let tab1 = herdr.pane_layout(Some(&orchestrator))?.tab_id;
    let watched = workers[0].0.clone();
    if !herdr.pane_focus_walk(&tab1, &watched)? {
        return Ok(fail(
            &ws.workspace_id,
            "could not put the focus on a worker pane before tiling",
            &[format!("wanted {watched} on {tab1}")],
        ));
    }
    let (tab_before, pane_before) = viewed(&herdr, &ws.workspace_id)?;

    println!("Tiling...");
    super::tilecmd::tile(None, Some(&ws.workspace_id), false, 0)?;

    // The watched pane stayed on tab 1, so the viewed tab AND the focused pane
    // must both be the ones the operator had.
    let (tab_after, pane_after) = viewed(&herdr, &ws.workspace_id)?;
    if tab_after != tab_before || pane_after != pane_before {
        return Ok(fail(
            &ws.workspace_id,
            "tiling moved the operator's view",
            &[
                format!("viewed tab {tab_before} -> {tab_after}"),
                format!("focused pane {pane_before} -> {pane_after}"),
            ],
        ));
    }

    // 7 workers is tab 1 with 4 in 2x2, then 3 on tab 2: column 1 top, column 1
    // bottom, column 2 top - so its bottom row is one pane spanning both columns.
    let complaints = grid_complaints(&herdr, &ws.workspace_id, &orchestrator, &[4, 3])?;
    if !complaints.is_empty() {
        return Ok(fail(
            &ws.workspace_id,
            "tiling did not produce the canonical grid",
            &complaints,
        ));
    }

    // Every pane still exists, and every process in it is still counting: a move
    // must not restart or kill an agent.
    let mut dead: Vec<String> = workers
        .iter()
        .filter(|(p, _)| !herdr.pane_exists(p))
        .map(|(p, _)| format!("pane {p} is gone"))
        .collect();
    if !herdr.pane_exists(&orchestrator) {
        dead.push(format!("the orchestrator pane {orchestrator} is gone"));
    }
    let advanced = poll(20, Duration::from_millis(500), || {
        workers
            .iter()
            .zip(&before)
            .all(|((_, c), was)| ticks(c).is_some_and(|now| now > *was))
    });
    if !advanced {
        dead.extend(
            workers
                .iter()
                .zip(&before)
                .filter(|((_, c), was)| !ticks(c).is_some_and(|now| now > **was))
                .map(|((p, _), was)| format!("pane {p} stopped counting at {was}")),
        );
    }
    if !dead.is_empty() {
        return Ok(fail(
            &ws.workspace_id,
            "tiling did not keep every pane and its process alive",
            &dead,
        ));
    }
    println!("PASS: a bad shape became the canonical grid over 2 tabs, all 7 panes still running.");
    println!("PASS: focus stayed on {tab_after} {pane_after}.");

    // Part 2: `horch spawn` with no placement flags must lay the grid out itself.
    // Break the shape first, so a canonical grid afterwards can only be the
    // spawn's doing.
    println!("Breaking the shape again, then spawning a worker with no placement flags...");
    let (first_worker, _) = &workers[0];
    let scrambled = herdr.pane_split(first_worker, Direction::Down)?;
    let counter = counters.path().join("w8");
    herdr.pane_run(&scrambled, &ticker(shell, &counter))?;
    workers.push((scrambled, counter));

    // Focus a worker again, so the spawn's own tiling has a view to put back.
    // The split above passed `--no-focus`, so this is only here to be explicit
    // about what the check is watching.
    if !herdr.pane_focus_walk(&tab1, &watched)? {
        return Ok(fail(
            &ws.workspace_id,
            "could not put the focus on a worker pane before spawning",
            &[format!("wanted {watched} on {tab1}")],
        ));
    }

    let spawned = spawn(SpawnArgs {
        teammate: Some("smoke".to_string()),
        phase: None,
        effort: None,
        task: "verify the spawn hook tiles".to_string(),
        resume: None,
        role: None,
        // What `horch spawn` does by default: split the calling pane, which in a
        // fleet is the orchestrator's. Named explicitly because this check runs
        // outside the workspace it is testing, where the default would resolve to
        // the checker's own pane in another workspace.
        from_pane: Some(orchestrator.clone()),
        direction: Direction::Right,
        no_tile: false,
    })?;

    // 9 workers: 4 on tab 1, 5 on tab 2. The spawned pane is the newcomer, so it
    // takes the last slot - and it may already have closed itself, since the
    // `smoke` teammate reports done and exits. Either way the 8 counting panes
    // must be in the grid, which only the spawn's own tiling can have done.
    let mut complaints = grid_complaints(&herdr, &ws.workspace_id, &orchestrator, &[4, 5])?;
    if !complaints.is_empty() && !herdr.pane_exists(&spawned) {
        // The fake worker finished during the tiling, so the grid is the one for 8.
        complaints = grid_complaints(&herdr, &ws.workspace_id, &orchestrator, &[4, 4])?;
    }
    if !complaints.is_empty() {
        return Ok(fail(
            &ws.workspace_id,
            "horch spawn did not lay the grid out",
            &complaints,
        ));
    }
    println!("PASS: horch spawn tiled the workspace with no placement flags and no tokens.");

    // The `smoke` teammate closes its own pane, and `horch done` starts a
    // detached tiler for the hole that leaves. Either tiling must end with the
    // same pane focused, so poll rather than race the detached one.
    let kept = poll(20, Duration::from_millis(500), || {
        viewed(&herdr, &ws.workspace_id).is_ok_and(|(_, pane)| pane == watched)
    });
    if !kept {
        let now = viewed(&herdr, &ws.workspace_id)
            .map(|(tab, pane)| format!("{tab} {pane}"))
            .unwrap_or_else(|e| format!("unreadable ({e:#})"));
        return Ok(fail(
            &ws.workspace_id,
            "horch spawn moved the operator's view",
            &[format!("focus was {watched}, is now {now}")],
        ));
    }
    println!("PASS: horch spawn left focus on {watched}.");

    // Part 3: a worker leaving frees a slot, and the workers on the overflow tab
    // move down into it. `horch done` starts a detached child that does this, and
    // that child may well get there first; the tiler is also run here directly so
    // the check asserts the end state rather than racing a background process.
    println!("Closing a worker and filling the hole it leaves...");
    if !poll(30, Duration::from_secs(1), || !herdr.pane_exists(&spawned)) {
        return Ok(fail(
            &ws.workspace_id,
            "the smoke worker never closed its own pane",
            &[format!("pane {spawned} is still there")],
        ));
    }
    // 8 workers now. Close four of them, leaving 4: they must all fit on tab 1 and
    // the overflow tab must be gone, with the panes that were on it moved down.
    let mut survivors = Vec::new();
    for (i, (pane, counter)) in workers.iter().enumerate() {
        if i < 4 {
            herdr.pane_close(pane)?;
        } else {
            survivors.push((pane.clone(), counter.clone()));
        }
    }
    super::tilecmd::tile(None, Some(&ws.workspace_id), false, 600)?;
    let complaints = grid_complaints(&herdr, &ws.workspace_id, &orchestrator, &[4])?;
    if !complaints.is_empty() {
        return Ok(fail(
            &ws.workspace_id,
            "the hole a departing worker left was not filled",
            &complaints,
        ));
    }
    let still_running: Vec<String> = survivors
        .iter()
        .filter(|(p, _)| !herdr.pane_exists(p))
        .map(|(p, _)| format!("pane {p} did not survive the refill"))
        .collect();
    if !still_running.is_empty() {
        return Ok(fail(
            &ws.workspace_id,
            "refilling the grid lost a pane",
            &still_running,
        ));
    }
    println!(
        "PASS: four workers left, the remaining four are on tab 1, and the overflow tab is gone."
    );

    herdr.workspace_close(&ws.workspace_id)?;
    Ok(ExitCode::SUCCESS)
}

/// Exactly one session, finished, having recorded both a progress note and a
/// completion summary.
fn smoke_session_done(ledger: &Ledger) -> bool {
    let Ok(records) = ledger.read() else {
        return false;
    };
    let [record] = records.as_slice() else {
        return false;
    };
    let events: Vec<&str> = record.history.iter().map(|h| h.event.as_str()).collect();
    record.status == horch_core::ledger::STATUS_DONE
        && events.contains(&"note")
        && events.contains(&"done")
}

fn dump_ledger(path: &Path) {
    eprintln!("--- ledger ({}) ---", path.display());
    match std::fs::read_to_string(path) {
        Ok(contents) => eprintln!("{contents}"),
        Err(e) => eprintln!("(could not read ledger: {e})"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The arithmetic must be left for the pane's own shell to evaluate, in that
    /// shell's dialect - never pre-computed here.
    #[test]
    fn arithmetic_echo_is_unevaluated_and_shell_appropriate() {
        for shell in [PaneShell::Posix, PaneShell::PowerShell] {
            let cmd = arithmetic_echo(shell);
            assert!(
                !cmd.contains("SMOKE_TEST_42"),
                "{cmd} must not be pre-evaluated"
            );
            assert!(cmd.contains("40+2"), "{cmd}");
        }
        assert!(arithmetic_echo(PaneShell::Posix).starts_with("echo "));
        assert!(arithmetic_echo(PaneShell::PowerShell).starts_with("Write-Output "));
    }

    #[test]
    fn poll_stops_at_the_first_success() {
        let mut calls = 0;
        assert!(poll(5, Duration::ZERO, || {
            calls += 1;
            calls == 2
        }));
        assert_eq!(calls, 2);
    }

    #[test]
    fn poll_gives_up_after_the_attempt_budget() {
        let mut calls = 0;
        assert!(!poll(3, Duration::ZERO, || {
            calls += 1;
            false
        }));
        assert_eq!(calls, 3);
    }

    #[test]
    fn smoke_session_is_done_only_with_one_finished_noted_record() {
        let tmp = tempfile::tempdir().unwrap();
        let ledger = Ledger::for_project(tmp.path(), "/p");
        assert!(!smoke_session_done(&ledger), "empty ledger");

        ledger
            .add("r1", "none", "smoke", "none", "smoke-1", Some("s1"), "t")
            .unwrap();
        assert!(!smoke_session_done(&ledger), "still working");

        ledger.note("r1", "progress").unwrap();
        assert!(!smoke_session_done(&ledger), "noted but not done");

        ledger.done("r1", "finished").unwrap();
        assert!(smoke_session_done(&ledger));

        // A second session means something else spawned into the scratch ledger.
        ledger
            .add("r2", "none", "smoke", "none", "smoke-2", Some("s2"), "t")
            .unwrap();
        assert!(!smoke_session_done(&ledger));
    }

    /// A session that finished without ever recording a note means `horch note`
    /// silently failed, which the check must catch.
    #[test]
    fn a_done_session_without_a_note_fails_the_check() {
        let tmp = tempfile::tempdir().unwrap();
        let ledger = Ledger::for_project(tmp.path(), "/p");
        ledger
            .add("r1", "none", "smoke", "none", "smoke-1", Some("s1"), "t")
            .unwrap();
        ledger.done("r1", "finished").unwrap();
        assert!(!smoke_session_done(&ledger));
    }
}
