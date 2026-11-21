use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::Subcommand;
use horch_core::execution::legacy::Record;
use horch_core::execution::records::SessionLedger;
use horch_core::execution::store::ExecutionStore;
use horch_core::execution::TilingMode;
use horch_core::messaging::mailbox::Mailbox;
use horch_core::runtime::RuntimeContext;
use horch_core::workspace::model::{Direction, Layout, NewWorkspace, Rect};
use horch_core::workspace::paneshell::PaneShell;
use horch_core::workspace::PaneHost;

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

pub fn run(ctx: &RuntimeContext, command: SmokeCommand) -> Result<ExitCode> {
    match command {
        SmokeCommand::Messaging => messaging(ctx),
        SmokeCommand::Fleet => fleet(ctx),
        SmokeCommand::Tile => tile(ctx),
    }
}

fn poll(attempts: u32, interval: Duration, mut check: impl FnMut() -> bool) -> bool {
    for _ in 0..attempts {
        if check() {
            return true;
        }
        std::thread::sleep(interval);
    }
    false
}

fn arithmetic_echo(shell: PaneShell) -> &'static str {
    match shell {
        PaneShell::Posix => "echo SMOKE_TEST_$((40+2))",
        PaneShell::PowerShell => "Write-Output \"SMOKE_TEST_$(40+2)\"",
    }
}

fn horch_line(ctx: &RuntimeContext, shell: PaneShell, exe: &Path, args: &[&str]) -> String {
    let data_root = ctx.paths.data_root.to_string_lossy();
    let state_root = ctx.paths.state_root.to_string_lossy();
    let env = [
        ("HORCH_DATA_DIR", data_root.as_ref()),
        ("HORCH_STATE_DIR", state_root.as_ref()),
    ];
    shell.command_line_keep_shell(exe, &env, args)
}

struct Failure(Vec<String>);

impl Failure {
    fn line(text: String) -> Self {
        Self(vec![text])
    }

    fn listed(workspace_id: &str, what: &str, complaints: &[String]) -> Self {
        let mut lines = vec![format!("FAIL: {what}")];
        lines.extend(complaints.iter().map(|c| format!("  - {c}")));
        lines.push(format!(
            "Workspace {workspace_id} left open for inspection."
        ));
        Self(lines)
    }
}

enum Smoke {
    Failed(Failure),
    Error(anyhow::Error),
}

impl From<anyhow::Error> for Smoke {
    fn from(e: anyhow::Error) -> Self {
        Smoke::Error(e)
    }
}

impl From<std::io::Error> for Smoke {
    fn from(e: std::io::Error) -> Self {
        Smoke::Error(e.into())
    }
}

type Step<T = ()> = Result<T, Smoke>;

fn finish(step: Step) -> Result<ExitCode> {
    match step {
        Ok(()) => Ok(ExitCode::SUCCESS),
        Err(Smoke::Failed(Failure(lines))) => {
            lines.iter().for_each(|line| eprintln!("{line}"));
            Ok(ExitCode::FAILURE)
        }
        Err(Smoke::Error(e)) => Err(e),
    }
}

fn ensure(ok: bool, failure: Failure) -> Step {
    if ok {
        Ok(())
    } else {
        Err(Smoke::Failed(failure))
    }
}

fn check(ok: bool, workspace_id: &str, what: &str, complaints: &[String]) -> Step {
    ensure(ok, Failure::listed(workspace_id, what, complaints))
}

struct Scratch {
    ctx: RuntimeContext,
    herdr: PaneHost,
    exe: PathBuf,
    shell: PaneShell,
    ws: NewWorkspace,
    mailbox: Mailbox,
    _state: tempfile::TempDir,
    _project: tempfile::TempDir,
}

fn scratch_context(ctx: &RuntimeContext, label: &str) -> Result<Scratch> {
    doctor::check(ctx)?;
    let herdr = PaneHost::of(ctx);
    let exe = ctx.bins.exe()?;
    let shell = PaneShell::host();
    let state = tempfile::Builder::new()
        .prefix("horch-smoke-state")
        .tempdir()?;
    let project = tempfile::Builder::new()
        .prefix("horch-smoke-proj")
        .tempdir()?;
    let project_path = project.path().to_string_lossy().into_owned();
    let mut ctx = ctx.clone();
    ctx.paths.set_state_dir(state.path());
    ctx.paths.project_dir = Some(project_path.clone().into());

    let ws = herdr.workspace_create(label, Some(&project_path), false)?;
    ctx.herdr.workspace = Some(horch_core::ids::WorkspaceId::new(ws.workspace_id.as_str())?);
    let mailbox = Mailbox::in_context(&ctx, &ws.workspace_id);
    Ok(Scratch {
        ctx,
        herdr,
        exe,
        shell,
        ws,
        mailbox,
        _state: state,
        _project: project,
    })
}

impl Scratch {
    fn register_orchestrator(&self, never: Failure) -> Step {
        let line = horch_line(
            &self.ctx,
            self.shell,
            &self.exe,
            &["register", "orchestrator"],
        );
        self.herdr.pane_run(&self.ws.root_pane_id, &line)?;
        let registered = poll(20, Duration::from_millis(500), || {
            self.mailbox.pane_for("orchestrator").is_some()
        });
        ensure(registered, never)
    }

    fn spawn_smoke(&self) -> Result<String> {
        spawn(&self.ctx, smoke_spawn_args(&self.ws.root_pane_id))
    }
}

fn smoke_spawn_args(root_pane: &str) -> SpawnArgs {
    SpawnArgs {
        teammate: Some("smoke".to_string()),
        task: String::new(),
        phase: None,
        effort: None,
        model: None,
        resume: None,
        role: None,
        from_pane: Some(root_pane.to_string()),
        direction: Direction::Right,
        tiling: TilingMode::Automatic,
        exact: false,
        force: true,
        replaces: None,
        run: None,
    }
}

fn messaging(ctx: &RuntimeContext) -> Result<ExitCode> {
    finish(messaging_run(ctx))
}

fn messaging_run(ctx: &RuntimeContext) -> Step {
    doctor::check(ctx)?;
    let herdr = PaneHost::of(ctx);
    let exe = ctx.bins.exe()?;
    let shell = PaneShell::host();

    let ws = herdr.workspace_create("horch smoke test", None, false)?;
    let a = ws.root_pane_id.clone();
    let b = herdr.pane_split(&a, Direction::Right)?;
    for (pane, role) in [(&a, "a"), (&b, "b")] {
        herdr.pane_run(pane, &horch_line(ctx, shell, &exe, &["register", role]))?;
    }
    let mailbox = Mailbox::in_context(ctx, &ws.workspace_id);
    await_registered(&mailbox, &ws.workspace_id)?;
    verify_delivery(&herdr, &mailbox, shell, &ws.workspace_id, &b)
}

fn await_registered(mailbox: &Mailbox, workspace_id: &str) -> Step {
    let registered = poll(20, Duration::from_millis(500), || {
        mailbox.pane_for("a").is_some() && mailbox.pane_for("b").is_some()
    });
    ensure(
        registered,
        Failure::line(format!(
            "FAIL: panes never registered (no {}/{{a,b}}.id). \
             Leaving workspace {workspace_id} open for inspection.",
            mailbox.dir().display(),
        )),
    )
}

fn verify_delivery(
    herdr: &PaneHost,
    mailbox: &Mailbox,
    shell: PaneShell,
    workspace_id: &str,
    b: &str,
) -> Step {
    println!("Sending a test message a -> b via horch tell...");
    let target = mailbox.pane_for("b").expect("just polled");
    horch_core::messaging::delivery::send_line(herdr, &target, arithmetic_echo(shell))?;
    let seen = herdr.wait_output(b, "SMOKE_TEST_42", 15_000)?;
    if !seen {
        return Err(Smoke::Failed(pane_dump(herdr, workspace_id, b)));
    }
    println!("PASS: send-text + send-keys enter delivered and executed the message end to end.");
    Ok(herdr.workspace_close(workspace_id)?)
}

fn pane_dump(herdr: &PaneHost, workspace_id: &str, pane: &str) -> Failure {
    let contents = herdr
        .pane_read(pane, "recent-unwrapped")
        .unwrap_or_else(|e| format!("(could not read pane: {e:#})"));
    Failure(vec![
        format!(
            "FAIL: SMOKE_TEST_42 not observed on pane {pane}. \
             Leaving workspace {workspace_id} open for inspection."
        ),
        format!("--- pane {pane} contents ---"),
        contents,
    ])
}

fn fleet(ctx: &RuntimeContext) -> Result<ExitCode> {
    finish(fleet_run(ctx))
}

fn fleet_run(ctx: &RuntimeContext) -> Step {
    let sc = scratch_context(ctx, "herdr-fleet smoke")?;
    let id = sc.ws.workspace_id.as_str();
    sc.register_orchestrator(Failure::line(format!(
        "FAIL: orchestrator pane never registered. Workspace {id} left open."
    )))?;

    println!("Spawning smoke worker...");
    let pane = sc.spawn_smoke()?;
    verify_worker_done(&sc, &pane)?;

    println!(
        "PASS: spawn, brief, register, ledger add/note/done, tell, and pane self-close \
         all verified."
    );
    Ok(sc.herdr.workspace_close(id)?)
}

fn verify_worker_done(sc: &Scratch, pane: &str) -> Step {
    let id = sc.ws.workspace_id.as_str();
    let ledger = SessionLedger::open_in(&sc.ctx)?;
    let reached_done = poll(30, Duration::from_secs(1), || smoke_session_done(&ledger));
    if !reached_done {
        let mut lines = vec![format!(
            "FAIL: smoke session never reached done in the ledger. Workspace {id} left open."
        )];
        lines.extend(ledger_dump(ledger.store()));
        return Err(Smoke::Failed(Failure(lines)));
    }

    let closed = poll(10, Duration::from_secs(1), || !sc.herdr.pane_exists(pane));
    ensure(
        closed,
        Failure::line(format!(
            "FAIL: smoke worker did not close its own pane {pane}. Workspace {id} left open."
        )),
    )
}

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

fn ticks(path: &Path) -> Option<u64> {
    std::fs::read_to_string(path).ok()?.trim().parse().ok()
}

fn tabs_of(herdr: &PaneHost, workspace_id: &str) -> Result<Vec<Layout>> {
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

fn off(a: i64, b: i64) -> bool {
    (a - b).abs() > 1
}

fn tab_complaints(layout: &Layout, orchestrator: Option<&str>, workers: usize) -> Vec<String> {
    let area = layout.area;
    let (orch, top, bottom) = rows(layout, orchestrator);
    let tab = &layout.tab_id;
    let (mut bad, x0) = orchestrator_complaints(tab, area, orchestrator.is_some(), orch);
    match shape_complaint(tab, (top.len(), bottom.len()), workers) {
        Some(shape) => bad.push(shape),
        None => {
            for (name, row) in [("top", &top), ("bottom", &bottom)] {
                bad.extend(row_complaints(tab, area, x0, name, row));
            }
            bad.extend(bottom_complaints(tab, area, &top, &bottom));
        }
    }
    bad
}

fn orchestrator_complaints(
    tab: &str,
    area: Rect,
    expected: bool,
    orch: Option<Rect>,
) -> (Vec<String>, i64) {
    match (expected, orch) {
        (false, _) => (Vec::new(), area.x),
        (true, None) => (
            vec![format!("{tab}: the orchestrator pane is not on this tab")],
            area.x,
        ),
        (true, Some(o)) => {
            let mut bad = Vec::new();
            if off(o.height, area.height) {
                bad.push(format!(
                    "{tab}: the orchestrator is {} rows tall, not the full {}",
                    o.height, area.height
                ));
            }
            bad.extend(
                off(o.x, area.x)
                    .then(|| format!("{tab}: the orchestrator is not leftmost (x {})", o.x)),
            );
            (bad, o.x + o.width)
        }
    }
}

fn shape_complaint(tab: &str, (top, bottom): (usize, usize), workers: usize) -> Option<String> {
    let columns = workers.div_ceil(2);
    if top + bottom != workers {
        Some(format!(
            "{tab}: {} worker pane(s), expected {workers}",
            top + bottom
        ))
    } else if top != columns || bottom != workers - columns {
        Some(format!(
            "{tab}: {top} top and {bottom} bottom, expected {columns} top and {} bottom",
            workers - columns
        ))
    } else {
        None
    }
}

fn row_complaints(tab: &str, area: Rect, x0: i64, name: &str, row: &[Rect]) -> Vec<String> {
    let (Some(first), Some(last)) = (row.first(), row.last()) else {
        return Vec::new();
    };
    let right = last.x + last.width;
    let edge = area.x + area.width;
    let mut bad = Vec::new();
    bad.extend(
        off(first.x, x0).then(|| format!("{tab}: the {name} row starts at {}, not {x0}", first.x)),
    );
    bad.extend(
        off(right, edge).then(|| format!("{tab}: the {name} row ends at {right}, not {edge}")),
    );
    bad.extend(
        row.windows(2)
            .filter(|pair| off(pair[0].x + pair[0].width, pair[1].x))
            .map(|pair| {
                format!(
                    "{tab}: a gap or overlap in the {name} row at x {}",
                    pair[1].x
                )
            }),
    );
    bad
}

fn bottom_complaints(tab: &str, area: Rect, top: &[Rect], bottom: &[Rect]) -> Vec<String> {
    let (Some(t), Some(b)) = (top.first(), bottom.first()) else {
        return Vec::new();
    };
    let mut bad = Vec::new();
    if off(b.y, area.y + t.height) {
        bad.push(format!(
            "{tab}: the bottom row starts at y {}, not under the top row at {}",
            b.y,
            area.y + t.height
        ));
    }
    if off(t.height + b.height, area.height) {
        bad.push(format!(
            "{tab}: the two rows are {} rows, not the tab's {}",
            t.height + b.height,
            area.height
        ));
    }
    bad.extend(
        bottom
            .iter()
            .zip(top)
            .filter(|(b, t)| off(b.x, t.x))
            .map(|(b, t)| {
                format!(
                    "{tab}: bottom pane at x {} is not under the column at x {}",
                    b.x, t.x
                )
            }),
    );
    bad
}

fn grid_complaints(
    herdr: &PaneHost,
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

fn viewed(herdr: &PaneHost, workspace_id: &str) -> Result<(String, String)> {
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

type Worker = (String, PathBuf);

fn tile(ctx: &RuntimeContext) -> Result<ExitCode> {
    finish(tile_run(ctx))
}

fn tile_run(ctx: &RuntimeContext) -> Step {
    let sc = scratch_context(ctx, "horch tile smoke")?;
    let counters = tempfile::Builder::new()
        .prefix("horch-smoke-tick")
        .tempdir()?;
    let dir = counters.path();
    let mut workers = build_bad_shape(&sc, dir)?;
    let (tab1, watched) = tile_bad_shape(&sc, &workers)?;
    let spawned = spawn_tiles(&sc, dir, &mut workers, (&tab1, &watched))?;
    check_spawn_view(&sc, &watched)?;
    refill(&sc, &workers, &spawned)?;
    Ok(sc.herdr.workspace_close(&sc.ws.workspace_id)?)
}

fn build_bad_shape(sc: &Scratch, dir: &Path) -> Step<Vec<Worker>> {
    let id = &sc.ws.workspace_id;
    let never = Failure::listed(id, "the orchestrator pane never registered", &[]);
    sc.register_orchestrator(never)?;

    println!("Building a deliberately bad shape: 7 panes in one column...");
    let mut workers = Vec::new();
    let mut previous = sc.ws.root_pane_id.clone();
    for i in 1..=7 {
        let worker = split_worker(sc, &previous, &dir.join(format!("w{i}")))?;
        previous = worker.0.clone();
        workers.push(worker);
    }
    let started = poll(30, Duration::from_millis(500), || {
        workers.iter().all(|(_, c)| ticks(c).is_some())
    });
    let what = "not every worker pane started counting";
    check(started, id, what, &silent_workers(&workers))?;
    Ok(workers)
}

fn silent_workers(workers: &[Worker]) -> Vec<String> {
    let silent = workers.iter().filter(|(_, c)| ticks(c).is_none());
    silent
        .map(|(p, _)| format!("{p} never wrote a count"))
        .collect()
}

fn split_worker(sc: &Scratch, below: &str, counter: &Path) -> Result<Worker> {
    let pane = sc.herdr.pane_split(below, Direction::Down)?;
    sc.herdr.pane_run(&pane, &ticker(sc.shell, counter))?;
    Ok((pane, counter.to_path_buf()))
}

fn focus_worker(sc: &Scratch, (tab1, watched): (&str, &str), when: &str) -> Step {
    let focused = sc.herdr.pane_focus_walk(tab1, watched)?;
    check(
        focused,
        &sc.ws.workspace_id,
        &format!("could not put the focus on a worker pane {when}"),
        &[format!("wanted {watched} on {tab1}")],
    )
}

fn tile_bad_shape(sc: &Scratch, workers: &[Worker]) -> Step<(String, String)> {
    let id = sc.ws.workspace_id.clone();
    let before = counts(workers);

    let tab1 = sc.herdr.pane_layout(Some(&sc.ws.root_pane_id))?.tab_id;
    let watched = workers[0].0.clone();
    focus_worker(sc, (&tab1, &watched), "before tiling")?;
    let (tab_before, pane_before) = viewed(&sc.herdr, &id)?;

    println!("Tiling...");
    super::tilecmd::tile(&sc.ctx, None, Some(&id), false, 0)?;

    let (tab_after, pane_after) = viewed_unmoved(sc, (&tab_before, &pane_before))?;

    check_grid(sc, &[4, 3], "tiling did not produce the canonical grid")?;
    check_alive(sc, workers, &before)?;
    println!("PASS: a bad shape became the canonical grid over 2 tabs, all 7 panes still running.");
    println!("PASS: focus stayed on {tab_after} {pane_after}.");
    Ok((tab1, watched))
}

fn counts(workers: &[Worker]) -> Vec<u64> {
    workers.iter().filter_map(|(_, c)| ticks(c)).collect()
}

fn viewed_unmoved(sc: &Scratch, (tab_before, pane_before): (&str, &str)) -> Step<(String, String)> {
    let id = &sc.ws.workspace_id;
    let (tab_after, pane_after) = viewed(&sc.herdr, id)?;
    check(
        tab_after == tab_before && pane_after == pane_before,
        id,
        "tiling moved the operator's view",
        &[
            format!("viewed tab {tab_before} -> {tab_after}"),
            format!("focused pane {pane_before} -> {pane_after}"),
        ],
    )?;
    Ok((tab_after, pane_after))
}

fn check_grid(sc: &Scratch, per_tab: &[usize], what: &str) -> Step {
    let id = &sc.ws.workspace_id;
    let complaints = grid_complaints(&sc.herdr, id, &sc.ws.root_pane_id, per_tab)?;
    check(complaints.is_empty(), id, what, &complaints)
}

fn check_alive(sc: &Scratch, workers: &[Worker], before: &[u64]) -> Step {
    let mut dead = missing_panes(sc, workers);
    let advanced = poll(20, Duration::from_millis(500), || {
        workers
            .iter()
            .zip(before)
            .all(|((_, c), was)| ticks(c).is_some_and(|now| now > *was))
    });
    if !advanced {
        dead.extend(
            workers
                .iter()
                .zip(before)
                .filter(|((_, c), was)| ticks(c).is_none_or(|now| now <= **was))
                .map(|((p, _), was)| format!("pane {p} stopped counting at {was}")),
        );
    }
    check(
        dead.is_empty(),
        &sc.ws.workspace_id,
        "tiling did not keep every pane and its process alive",
        &dead,
    )
}

fn missing_panes(sc: &Scratch, workers: &[Worker]) -> Vec<String> {
    let orchestrator = &sc.ws.root_pane_id;
    let mut gone: Vec<String> = workers
        .iter()
        .filter(|(p, _)| !sc.herdr.pane_exists(p))
        .map(|(p, _)| format!("pane {p} is gone"))
        .collect();
    gone.extend(
        (!sc.herdr.pane_exists(orchestrator))
            .then(|| format!("the orchestrator pane {orchestrator} is gone")),
    );
    gone
}

fn spawn_tiles(
    sc: &Scratch,
    dir: &Path,
    workers: &mut Vec<Worker>,
    watched_on: (&str, &str),
) -> Step<String> {
    println!("Breaking the shape again, then spawning a worker with no placement flags...");
    workers.push(split_worker(sc, &workers[0].0, &dir.join("w8"))?);

    focus_worker(sc, watched_on, "before spawning")?;
    let spawned = sc.spawn_smoke()?;
    let complaints = grid_after_spawn(sc, &spawned)?;
    check(
        complaints.is_empty(),
        &sc.ws.workspace_id,
        "horch spawn did not lay the grid out",
        &complaints,
    )?;
    println!("PASS: horch spawn tiled the workspace with no placement flags and no tokens.");
    Ok(spawned)
}

fn grid_after_spawn(sc: &Scratch, spawned: &str) -> Result<Vec<String>> {
    let (herdr, id, orchestrator) = (&sc.herdr, &sc.ws.workspace_id, &sc.ws.root_pane_id);
    let complaints = grid_complaints(herdr, id, orchestrator, &[4, 5])?;
    if complaints.is_empty() || herdr.pane_exists(spawned) {
        return Ok(complaints);
    }
    grid_complaints(herdr, id, orchestrator, &[4, 4])
}

fn check_spawn_view(sc: &Scratch, watched: &str) -> Step {
    let id = &sc.ws.workspace_id;
    let kept = poll(20, Duration::from_millis(500), || {
        viewed(&sc.herdr, id).is_ok_and(|(_, pane)| pane == watched)
    });
    if !kept {
        let now = viewed(&sc.herdr, id)
            .map(|(tab, pane)| format!("{tab} {pane}"))
            .unwrap_or_else(|e| format!("unreadable ({e:#})"));
        let complaints = [format!("focus was {watched}, is now {now}")];
        let failure = Failure::listed(id, "horch spawn moved the operator's view", &complaints);
        return Err(Smoke::Failed(failure));
    }
    println!("PASS: horch spawn left focus on {watched}.");
    Ok(())
}

fn refill(sc: &Scratch, workers: &[Worker], spawned: &str) -> Step {
    println!("Closing a worker and filling the hole it leaves...");
    let id = sc.ws.workspace_id.clone();
    let gone = poll(30, Duration::from_secs(1), || {
        !sc.herdr.pane_exists(spawned)
    });
    check(
        gone,
        &id,
        "the smoke worker never closed its own pane",
        &[format!("pane {spawned} is still there")],
    )?;
    let survivors = close_first_four(sc, workers)?;
    super::tilecmd::tile(&sc.ctx, None, Some(&id), false, 600)?;
    check_grid(sc, &[4], "the hole a departing worker left was not filled")?;
    check_survivors(sc, survivors)?;
    println!(
        "PASS: four workers left, the remaining four are on tab 1, and the overflow tab is gone."
    );
    Ok(())
}

fn close_first_four<'a>(sc: &Scratch, workers: &'a [Worker]) -> Result<&'a [Worker]> {
    let (closing, survivors) = workers.split_at(workers.len().min(4));
    for (pane, _) in closing {
        sc.herdr.pane_close(pane)?;
    }
    Ok(survivors)
}

fn check_survivors(sc: &Scratch, survivors: &[Worker]) -> Step {
    let lost: Vec<String> = survivors
        .iter()
        .filter(|(p, _)| !sc.herdr.pane_exists(p))
        .map(|(p, _)| format!("pane {p} did not survive the refill"))
        .collect();
    check(
        lost.is_empty(),
        &sc.ws.workspace_id,
        "refilling the grid lost a pane",
        &lost,
    )
}

fn smoke_session_done(ledger: &SessionLedger) -> bool {
    ledger
        .read()
        .ok()
        .is_some_and(|records| matches!(records.as_slice(), [record] if session_is_done(record)))
}

fn session_is_done(record: &Record) -> bool {
    let recorded = |event: &str| record.history.iter().any(|h| h.event == event);
    record.status == horch_core::execution::legacy::STATUS_DONE
        && recorded("note")
        && recorded("done")
}

fn ledger_dump(store: &ExecutionStore) -> [String; 2] {
    [
        format!("--- session records ({}) ---", store.db().display()),
        store
            .stored_json()
            .unwrap_or_else(|e| format!("(could not read session records: {e})")),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn horch_line_carries_the_state_folder() {
        for (variable, root, state) in [
            ("HORCH_STATE_DIR", "/smoke-state", "/smoke-state"),
            ("XDG_STATE_HOME", "/xdg-state", "/xdg-state/horch"),
        ] {
            let ctx = RuntimeContext::from_env(
                &horch_core::runtime::MapEnv::new("/")
                    .with_exe("/opt/horch/bin/horch")
                    .with(variable, root),
            )
            .unwrap();
            let state = Path::new(state).display();
            for (shell, set) in [
                (PaneShell::Posix, format!(" 'HORCH_STATE_DIR={state}' ")),
                (
                    PaneShell::PowerShell,
                    format!("$env:HORCH_STATE_DIR = '{state}'; "),
                ),
            ] {
                let exe = Path::new("/opt/horch/bin/horch");
                let line = horch_line(&ctx, shell, exe, &["register", "a"]);
                assert!(line.contains(&set), "{variable}: {line}");
            }
        }
    }

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
    fn trf_43_the_smoke_spawn_carries_no_task_text() {
        let args = smoke_spawn_args("w1:p1");
        assert_eq!(args.teammate.as_deref(), Some("smoke"));
        assert_eq!(args.from_pane.as_deref(), Some("w1:p1"));
        assert!(args.run.is_none(), "smoke joins no task-ledger run");
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
        let ledger = SessionLedger::for_project(tmp.path(), "/p");
        assert!(!smoke_session_done(&ledger), "empty ledger");

        ledger
            .add(horch_core::execution::records::NewRecord {
                record_id: "r1",
                agent: "none",
                tier: "smoke",
                model: "none",
                role: "smoke-1",
                session_id: Some("s1"),
                task: "t",
                phase: None,
            })
            .unwrap();
        assert!(!smoke_session_done(&ledger), "still working");

        ledger.note("r1", "progress").unwrap();
        assert!(!smoke_session_done(&ledger), "noted but not done");

        ledger.done("r1", "finished").unwrap();
        assert!(smoke_session_done(&ledger));

        ledger
            .add(horch_core::execution::records::NewRecord {
                record_id: "r2",
                agent: "none",
                tier: "smoke",
                model: "none",
                role: "smoke-2",
                session_id: Some("s2"),
                task: "t",
                phase: None,
            })
            .unwrap();
        assert!(!smoke_session_done(&ledger));
    }

    #[test]
    fn a_done_session_without_a_note_fails_the_check() {
        let tmp = tempfile::tempdir().unwrap();
        let ledger = SessionLedger::for_project(tmp.path(), "/p");
        ledger
            .add(horch_core::execution::records::NewRecord {
                record_id: "r1",
                agent: "none",
                tier: "smoke",
                model: "none",
                role: "smoke-1",
                session_id: Some("s1"),
                task: "t",
                phase: None,
            })
            .unwrap();
        ledger.done("r1", "finished").unwrap();
        assert!(!smoke_session_done(&ledger));
    }

    fn rect(x: i64, y: i64, width: i64, height: i64) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    fn tab(panes: &[(&str, Rect)]) -> Layout {
        Layout {
            workspace_id: "w".into(),
            tab_id: "t1".into(),
            area: rect(0, 0, 100, 40),
            panes: panes
                .iter()
                .map(|(id, rect)| horch_core::workspace::model::LayoutPane {
                    pane_id: id.to_string(),
                    rect: *rect,
                })
                .collect(),
            focused_pane_id: None,
            zoomed: false,
        }
    }

    fn canonical() -> Vec<(&'static str, Rect)> {
        vec![
            ("o", rect(0, 0, 30, 40)),
            ("a", rect(30, 0, 35, 20)),
            ("b", rect(65, 0, 35, 20)),
            ("c", rect(30, 20, 35, 20)),
            ("d", rect(65, 20, 35, 20)),
        ]
    }

    fn replace(panes: &mut [(&'static str, Rect)], id: &str, to: Rect) {
        panes.iter_mut().find(|(p, _)| *p == id).unwrap().1 = to;
    }

    #[test]
    fn a_canonical_tab_has_no_complaints() {
        let layout = tab(&canonical());
        assert!(tab_complaints(&layout, Some("o"), 4).is_empty());
    }

    #[test]
    fn an_odd_overflow_tab_lets_the_last_bottom_span_the_free_slot() {
        let layout = tab(&[
            ("a", rect(0, 0, 50, 20)),
            ("b", rect(50, 0, 50, 20)),
            ("c", rect(0, 20, 100, 20)),
        ]);
        assert!(tab_complaints(&layout, None, 3).is_empty());
    }

    #[test]
    fn rows_split_the_workers_into_sorted_top_and_bottom_rows() {
        let layout = tab(&canonical());
        let (orch, top, bottom) = rows(&layout, Some("o"));
        assert_eq!(orch.unwrap().width, 30);
        assert_eq!(top.iter().map(|r| r.x).collect::<Vec<_>>(), [30, 65]);
        assert_eq!(bottom.iter().map(|r| r.x).collect::<Vec<_>>(), [30, 65]);
        let (none, _, _) = rows(&layout, Some("missing"));
        assert!(none.is_none());
    }

    #[test]
    fn a_missing_orchestrator_is_a_complaint() {
        let layout = tab(&canonical()[1..]);
        let bad = tab_complaints(&layout, Some("o"), 4);
        assert_eq!(bad[0], "t1: the orchestrator pane is not on this tab");
        assert_eq!(bad.len(), 3, "the rows are still checked: {bad:?}");
    }

    #[test]
    fn a_short_or_misplaced_orchestrator_is_a_complaint() {
        let mut panes = canonical();
        replace(&mut panes, "o", rect(2, 0, 28, 30));
        let bad = tab_complaints(&tab(&panes), Some("o"), 4);
        assert_eq!(
            bad[..2],
            [
                "t1: the orchestrator is 30 rows tall, not the full 40",
                "t1: the orchestrator is not leftmost (x 2)",
            ]
        );
    }

    #[test]
    fn a_wrong_worker_count_stops_the_check() {
        let layout = tab(&canonical()[..4]);
        assert_eq!(
            tab_complaints(&layout, Some("o"), 4),
            ["t1: 3 worker pane(s), expected 4"]
        );
    }

    #[test]
    fn a_wrong_row_split_stops_the_check() {
        let mut panes = canonical();
        replace(&mut panes, "b", rect(65, 20, 35, 20));
        replace(&mut panes, "d", rect(65, 20, 35, 20));
        assert_eq!(
            tab_complaints(&tab(&panes), Some("o"), 4),
            ["t1: 1 top and 3 bottom, expected 2 top and 2 bottom"]
        );
    }

    #[test]
    fn a_row_that_misses_either_edge_or_has_a_gap_is_a_complaint() {
        let mut panes = canonical();
        replace(&mut panes, "a", rect(34, 0, 31, 20));
        replace(&mut panes, "b", rect(70, 0, 25, 20));
        assert_eq!(
            tab_complaints(&tab(&panes), Some("o"), 4),
            [
                "t1: the top row starts at 34, not 30",
                "t1: the top row ends at 95, not 100",
                "t1: a gap or overlap in the top row at x 70",
                "t1: bottom pane at x 30 is not under the column at x 34",
                "t1: bottom pane at x 65 is not under the column at x 70",
            ]
        );
    }

    #[test]
    fn a_bottom_row_must_sit_under_the_top_row_in_the_same_columns() {
        let mut panes = canonical();
        replace(&mut panes, "c", rect(30, 25, 35, 10));
        replace(&mut panes, "d", rect(70, 25, 30, 10));
        assert_eq!(
            tab_complaints(&tab(&panes), Some("o"), 4),
            [
                "t1: a gap or overlap in the bottom row at x 70",
                "t1: the bottom row starts at y 25, not under the top row at 20",
                "t1: the two rows are 30 rows, not the tab's 40",
                "t1: bottom pane at x 70 is not under the column at x 65",
            ]
        );
    }

    #[test]
    fn a_listed_failure_names_the_check_each_complaint_and_the_workspace() {
        let Failure(lines) = Failure::listed("w1", "the grid is wrong", &["a".into(), "b".into()]);
        assert_eq!(
            lines,
            [
                "FAIL: the grid is wrong",
                "  - a",
                "  - b",
                "Workspace w1 left open for inspection.",
            ]
        );
    }

    #[test]
    fn check_stops_the_step_only_when_the_condition_fails() {
        assert!(check(true, "w1", "fine", &["unused".into()]).is_ok());
        match check(false, "w1", "broken", &["why".into()]) {
            Err(Smoke::Failed(Failure(lines))) => assert_eq!(lines[1], "  - why"),
            _ => panic!("a failed check must stop the step with its failure"),
        }
    }

    #[test]
    fn finish_maps_a_failed_check_to_failure_and_a_broken_step_to_an_error() {
        assert_eq!(finish(Ok(())).unwrap(), ExitCode::SUCCESS);
        let failed = finish(Err(Smoke::Failed(Failure::line("FAIL: x".into()))));
        assert_eq!(failed.unwrap(), ExitCode::FAILURE);
        let broken = finish(Err(Smoke::Error(anyhow::anyhow!("herdr is gone"))));
        assert_eq!(broken.unwrap_err().to_string(), "herdr is gone");
    }

    #[test]
    fn trf_45_ledger_dump_prints_the_stored_session_records() {
        let tmp = tempfile::tempdir().unwrap();
        let ledger = SessionLedger::for_project(tmp.path(), "/p");
        ledger
            .add(horch_core::execution::records::NewRecord {
                record_id: "r1",
                agent: "none",
                tier: "smoke",
                model: "none",
                role: "smoke-1",
                session_id: Some("s1"),
                task: "t",
                phase: None,
            })
            .unwrap();
        let store = ledger.store();
        let [head, body] = ledger_dump(store);
        assert_eq!(
            head,
            format!("--- session records ({}) ---", store.db().display())
        );
        assert_eq!(body, store.stored_json().unwrap());
        assert!(body.contains("\"record_id\": \"r1\""), "{body}");
        assert!(!store.path().exists(), "no JSON session ledger is written");
    }
}
