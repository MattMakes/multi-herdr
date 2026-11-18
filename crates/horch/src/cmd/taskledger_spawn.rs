//! `horch spawn`'s task-ledger side (#34): the worker row is written before
//! the pane opens, its id travels in the brief, and the role and pane are
//! recorded once the split is done.

use std::path::Path;

use anyhow::{bail, ensure, Result};
use horch_core::execution::store::ExecutionStore;
use horch_core::fleetd::fleet_runtime::adapters::config_file;
use horch_core::fleetd::fleet_runtime::adapters::effects::{self, LaunchRecord};
use horch_core::fleetd::fleet_runtime::application::holds;
use horch_core::fleetd::fleet_runtime::application::jobs::JobCtx;
use horch_core::fleetd::fleet_runtime::application::ledger::{self, Cause};
use horch_core::fleetd::fleet_runtime::domain::health::Finding;
use horch_core::fleetd::fleet_runtime::domain::ledger::Entity;
use horch_core::fleetd::ideas::application::needs_you::NeedsYouList;
use horch_core::fleetd::task_execution::adapters::sqlite as attempts_store;
use horch_core::fleetd::task_execution::adapters::task_files::AttemptFiles;
use horch_core::fleetd::task_execution::application::dispatch::{
    bind, choices, write_task_files, Bound, CheckSandboxes, DispatchCtx,
};
use horch_core::fleetd::task_execution::application::quota::ADOPT_WORKER_ENV;
use horch_core::fleetd::task_execution::domain::dispatch::{Choice, Holds};
use horch_core::fleetd::task_execution::domain::task::AttemptState;
use horch_core::fleetd::worktree_custody::adapters::sqlite::{self as trees, WorktreeState};
use horch_core::fleetd::worktree_custody::application::create::{self, JobProcess};
use horch_core::runtime::RuntimeContext;
use horch_core::taskledger::git::Git;
use horch_core::taskledger::model::{
    Actor, EventKind, RunStatus, TaskStatus, WorkerKind, WorkerStatus,
};
use horch_core::taskledger::runs::{self, Run};
use horch_core::taskledger::tasks::{self, Task};
use horch_core::taskledger::util::{self, About};
use horch_core::taskledger::workers::{self, NewWorker};
use horch_core::taskledger::{self, Store};

use super::spawn::SpawnArgs;
use super::taskledger::{project_root, RunArg};

/// The first line of the pane of an adopted restart on a fallback (RR-11).
/// Its attempt began with a claim, so it ends by the old path. The Phase 2
/// bridge: ticket 10 of Phase 4 launches a restart through the bind and
/// `--attempt`.
const ADOPTED_CLAIM_LINE: &str = "Run horch ledger claim. Read the ticket file it prints and follow it exactly. If it prints that there is no task for you, send the line it prints, then end your turn.";

/// The environment variable that names the task a spawn binds first
/// (TRF-24, L2). Reviewer auto-spawn sets it on its `horch spawn` child.
pub const CLAIM_TARGET_ENV: &str = "HORCH_CLAIM_TARGET";

/// The fault point before the commit of the bind of a spawn.
pub const FAULT_BEFORE_BIND_COMMIT: &str = "spawn-bind-before-commit";
/// The fault point after the commit of the bind and before its effects:
/// the tree, the task files, the launch record and the pane.
pub const FAULT_AFTER_BIND_COMMIT: &str = "spawn-bind-after-commit";

/// The first line of the pane of a push attempt (Q8 of
/// `docs/tickets/fleetd-p4/MAP.md`): 1 fixed format with the absolute path
/// of its `START.md`, because a pane has no task folder as its cwd.
pub fn start_line(start_md: &Path) -> String {
    format!("Read {} and continue.", start_md.display())
}

/// A spawn's worker row.
pub struct LedgerSpawn {
    store: std::cell::RefCell<Store>,
    pub run: Run,
    pub worker_id: String,
    /// The first line of the pane: [`start_line`] of a push attempt, or the
    /// claim of an adopted restart. `None` for a role, whose brief is its
    /// task.
    pub first_line: Option<String>,
    /// The push attempt of the worker, with its files.
    pub push: Option<Pushed>,
}

/// The push attempt that a spawn opens the pane of.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pushed {
    pub attempt_id: String,
    pub task_id: String,
    pub files: AttemptFiles,
    /// This spawn bound the attempt; `false` for `--attempt`, whose
    /// attempt fleetd bound.
    pub own_bind: bool,
}

/// The outcome of the attempt of a spawn that failed before its agent ran.
const LAUNCH_FAILED: &str = "launch_failed";

/// Undo the bind of a spawn that failed before its agent ran, for example
/// a refusal of the spawn gate: its attempt ends `launch_failed`, its tree
/// goes back to its task, and the task goes back to `ready`. No attempt is
/// burned: the floor of its attempt count rises with the count.
fn unbind(tx: &rusqlite::Connection, p: &Pushed, reason: &str) -> Result<()> {
    let text = format!("its spawn failed before its agent ran: {reason}");
    let why = || Cause {
        actor: Actor::Horch,
        text: &text,
    };
    attempts_store::set_ended(tx, &p.attempt_id, LAUNCH_FAILED, &util::now())?;
    let ended = AttemptState::Ended.as_str();
    ledger::transition(
        tx,
        None,
        Entity::Attempt,
        &p.attempt_id,
        ended,
        why(),
        &NeedsYouList,
    )?;
    if let Some(row) = trees::of_task(tx, &p.task_id)?.filter(|r| {
        r.state == WorktreeState::Active && r.owner_attempt.as_deref() == Some(&p.attempt_id)
    }) {
        create::release(tx, &row, &text)?;
    }
    let ready = TaskStatus::Ready.as_str();
    ledger::transition(
        tx,
        None,
        Entity::Task,
        &p.task_id,
        ready,
        why(),
        &NeedsYouList,
    )?;
    tx.execute(
        "UPDATE tasks SET attempt_floor = attempt_floor + 1, current_worker_id = NULL
         WHERE task_id = ?1",
        [&p.task_id],
    )?;
    Ok(())
}

/// Write the worker row, or adopt the row that fleetd wrote for a restart
/// (`HORCH_ADOPT_WORKER`), when this spawn joins a task-ledger run: it names
/// one with `--run` (or `HORCH_RUN_ID`), it replaces a ledger worker, its
/// workspace has an active run, or a `spawn_only` controller in an active
/// run sends a role its brief. `None` otherwise: a controller outside every
/// run sends its brief outside every run too.
///
/// `controller` is the caller's ledger row when the caller has
/// `spawn_only` and passed its check (T-38). Only such a spawn carries task
/// text: `horch spawn` refuses every other (TRF-43).
pub fn prepare(
    ctx: &RuntimeContext,
    args: &SpawnArgs,
    controller: Option<&str>,
) -> Result<Option<LedgerSpawn>> {
    refuse_on_hold(ctx)?;
    let mut store = taskledger::open(ctx)?;
    let role_run = controller_run(&store, args, controller)?;
    let run = match &role_run {
        Some(run) => run.clone(),
        None if !args.task.trim().is_empty() => return Ok(None),
        None => {
            let Some(explicit) = ledger_run_wanted(ctx, &store, args)? else {
                return Ok(None);
            };
            super::taskledger::resolve_run(ctx, &store, &RunArg { run: explicit })?
        }
    };
    ensure_active(&run)?;
    let persona = spawn_persona(ctx, args)?;
    ensure_replaceable(&store, args, &run)?;
    let (id, first_line, push) = match (adopt_wanted(), role_run.is_some()) {
        (Some(row), false) => {
            let id = adopt(&mut store, &row, &run, &persona, claim_target().as_deref())?;
            (id, Some(ADOPTED_CLAIM_LINE.to_string()), None)
        }
        (Some(row), true) => {
            bail!("horch spawn refused: a controller's brief adopts no worker row ({row})")
        }
        (None, true) => {
            let id = insert_row(ctx, &mut store, args, &run, persona)?;
            mark_no_claim(&mut store, &run, &id, controller.unwrap_or_default())?;
            (id, None, None)
        }
        (None, false) => {
            let target = claim_target();
            let (worker, pushed) = push(ctx, &mut store, args, &run, &persona, target)?;
            (
                worker,
                Some(start_line(&pushed.files.start_md)),
                Some(pushed),
            )
        }
    };
    Ok(Some(LedgerSpawn {
        store: std::cell::RefCell::new(store),
        run,
        worker_id: id,
        first_line,
        push,
    }))
}

/// The push attempt of a worker spawn, before its pane exists (FR-5,
/// FM-2): the attempt that `--attempt` names, which fleetd bound, else a
/// new bind of the first ready task of `persona` in `run`, `target` first.
/// A new bind makes
/// its tree, writes `task.yaml` and `START.md`, and writes the launch
/// record of the attempt, so the pass does not launch it a second time.
/// Gives the worker row and the attempt.
fn push(
    ctx: &RuntimeContext,
    store: &mut Store,
    args: &SpawnArgs,
    run: &Run,
    persona: &str,
    target: Option<String>,
) -> Result<(String, Pushed)> {
    let config = config_file::load(&ctx.paths.fleet_root)?;
    let jobs = JobCtx {
        guard: None,
        config: &config,
        needs_you: &NeedsYouList,
    };
    let (worker, attempt_id, task_id) = match &args.attempt {
        Some(attempt) => bound_attempt(store, attempt, run, persona)?,
        None => {
            let b = bind_task(ctx, store, &jobs, args, run, persona, target)?;
            if let Some(tree) = b.worktree.as_ref().filter(|t| t.add_job.is_some()) {
                // The `worktree_add` job runs with no transaction open, on
                // its own connection, as `horch ledger worktree new` runs it.
                let process = JobProcess::of_this_process()?;
                let mut conn = rusqlite::Connection::open(store.path())?;
                conn.busy_timeout(std::time::Duration::from_secs(30))?;
                create::finish_reserved(&mut conn, &jobs, &process, &tree.worktree_id)?;
            }
            (b.worker_id, b.attempt_id, b.task_id)
        }
    };
    let files = write_task_files(store.conn(), &ctx.paths.fleet_root, &attempt_id, &[])?;
    if args.attempt.is_none() {
        let record = LaunchRecord {
            attempt: attempt_id.clone(),
            launched_at: create::fleet_clock(store.conn())?,
            process: None,
        };
        effects::write_launch(&ctx.paths.fleet_root, &record)?;
    }
    Ok((
        worker,
        Pushed {
            attempt_id,
            task_id,
            files,
            own_bind: args.attempt.is_none(),
        },
    ))
}

/// Each persona has a check sandbox until ticket 18 of Phase 4 gives the
/// check sandbox of each persona on this host.
struct EverySandbox;

impl CheckSandboxes for EverySandbox {
    fn supported(&self, _persona: &str) -> bool {
        true
    }
}

/// Bind the first ready task of `persona` in `run` to a new attempt, in 1
/// transaction (ticket 08). `target`, the task of [`CLAIM_TARGET_ENV`],
/// else the task of the worker that `--replaces` names, goes first (L2). A
/// spawn with no task to bind binds nothing: the error names each task of
/// `persona` that a hold keeps ready, with its hold.
fn bind_task(
    ctx: &RuntimeContext,
    store: &mut Store,
    jobs: &JobCtx<'_>,
    args: &SpawnArgs,
    run: &Run,
    persona: &str,
    target: Option<String>,
) -> Result<Bound> {
    let git = Git::new(&ctx.bins.harness.git);
    let faults = &ctx.settings.faults;
    let worker = NewWorker {
        workspace_id: ctx.herdr.workspace.as_ref().map(|w| w.as_str().to_string()),
        previous_worker_id: args.replaces.clone(),
        resumed_from_session: args.resume.clone(),
        ..Default::default()
    };
    let bound = store.immediate(|tx| {
        let target = match &target {
            Some(t) => Some(t.clone()),
            None => replaced_task(tx, args)?,
        };
        let clock = create::fleet_clock(tx)?;
        let holds = Holds {
            clock: clock.0,
            ..Holds::default()
        };
        let task_id = chosen_task(tx, run, persona, target.as_deref(), &holds)?;
        let dctx = DispatchCtx {
            jobs: JobCtx {
                guard: jobs.guard,
                config: jobs.config,
                needs_you: jobs.needs_you,
            },
            clock,
            git: &git,
        };
        let bound = bind(tx, &dctx, &task_id, persona, &worker)?;
        if faults.has(FAULT_BEFORE_BIND_COMMIT) {
            bail!("HORCH_FAULT {FAULT_BEFORE_BIND_COMMIT}");
        }
        Ok(bound)
    })?;
    if faults.has(FAULT_AFTER_BIND_COMMIT) {
        bail!("HORCH_FAULT {FAULT_AFTER_BIND_COMMIT}");
    }
    eprintln!(
        "horch spawn: bound {} to attempt {} of worker {}.",
        tasks::get(store.conn(), &bound.task_id)?.short(),
        bound.attempt_id,
        bound.worker_id
    );
    Ok(bound)
}

/// The task of the worker that `--replaces` names, if it has one.
fn replaced_task(conn: &rusqlite::Connection, args: &SpawnArgs) -> Result<Option<String>> {
    let Some(prev) = &args.replaces else {
        return Ok(None);
    };
    Ok(workers::find(conn, prev)?.and_then(|w| w.task_id))
}

/// The persona of the next attempt of `t`: `claimable_by`, else its
/// primary (the rule of the bind).
fn next_persona(t: &Task) -> Option<&str> {
    t.claimable_by.as_deref().or(t.primary_persona.as_deref())
}

/// The first task of `run` and `persona` that the dispatch choice binds
/// now, `target` first. Else the refusal: no ready task, or each hold.
fn chosen_task(
    tx: &rusqlite::Connection,
    run: &Run,
    persona: &str,
    target: Option<&str>,
    holds: &Holds,
) -> Result<String> {
    let mut held = Vec::new();
    for choice in choices(tx, holds, &EverySandbox, target)? {
        let (task_id, hold) = match choice {
            Choice::Bind { task_id, .. } => (task_id, None),
            Choice::Held { task_id, hold } => (task_id, Some(hold)),
        };
        let t = tasks::get(tx, &task_id)?;
        if t.run_id != run.run_id || next_persona(&t) != Some(persona) {
            continue;
        }
        match hold {
            None => return Ok(task_id),
            Some(hold) => held.push(format!("{}: {hold}", t.short())),
        }
    }
    if held.is_empty() {
        bail!(
            "horch spawn refused: run {} has no ready task for persona {persona}",
            run.slug
        );
    }
    bail!(
        "horch spawn refused: no ready task for persona {persona} in run {} can start now:\n  - {}",
        run.slug,
        held.join("\n  - ")
    )
}

/// The push attempt `attempt` that fleetd bound, for `--attempt`: it is in
/// `launching`, it began with a push dispatch, and its worker is a
/// `spawning` row of `run` and `persona` with no agent. Gives its worker,
/// the attempt and its task.
fn bound_attempt(
    store: &mut Store,
    attempt: &str,
    run: &Run,
    persona: &str,
) -> Result<(String, String, String)> {
    let refusal =
        |why: &str| anyhow::anyhow!("horch spawn refused: it cannot open attempt {attempt}: {why}");
    let row: Option<(Option<String>, String, String, String)> = store
        .conn()
        .query_row(
            "SELECT task_id, worker_id, state, via FROM attempts WHERE id = ?1",
            [attempt],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .map(Some)
        .or_else(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            e => Err(e),
        })?;
    let Some((task, worker_id, state, via)) = row else {
        return Err(refusal("no attempt row has this id"));
    };
    let Some(task) = task else {
        return Err(refusal("the attempt has no task"));
    };
    if via != "push" {
        return Err(refusal("the attempt began with a claim"));
    }
    if state != AttemptState::Launching.as_str() {
        return Err(refusal(&format!("the attempt is {state}, not launching")));
    }
    let w = workers::get(store.conn(), &worker_id)?;
    if w.run_id.as_deref() != Some(run.run_id.as_str()) {
        return Err(refusal(&format!("its worker is not of run {}", run.slug)));
    }
    if w.persona.as_deref() != Some(persona) {
        return Err(refusal(&format!("its worker is not of persona {persona}")));
    }
    if w.status != WorkerStatus::Spawning.as_str() || w.agent_pid.is_some() {
        return Err(refusal("its worker has started"));
    }
    Ok((worker_id, attempt.to_string(), task))
}

/// Refuse a spawn while a health guard holds new dispatch (OR-5), before
/// any row is written. The refusal names each hold with its floor and its
/// reading. A guard with no reading prints 1 note and does not hold
/// (`preservation-06`, row S12).
fn refuse_on_hold(ctx: &RuntimeContext) -> Result<()> {
    let mut held = Vec::new();
    for finding in holds::holds(ctx)? {
        match finding {
            Finding::Hold(h) => held.push(h.cause().to_string()),
            Finding::Unknown { fact, why } => eprintln!(
                "horch: NOTE: the health guard {} does not hold dispatch: {why}",
                fact.as_str()
            ),
        }
    }
    if held.is_empty() {
        return Ok(());
    }
    bail!(
        "horch spawn refused: new dispatch is on hold until the machine recovers:\n  - {}",
        held.join("\n  - ")
    )
}

/// Whether this spawn joins a task-ledger run. `None`: it does not. `Some`
/// holds the run it joins: the run `--run` (or `HORCH_RUN_ID`) names, the run
/// of the worker `--replaces` names, else the 1 active run of the caller's
/// herdr workspace (TRF-34). A planned run there wins over solo runs. A spawn never joins the run of another
/// workspace unless it names that run.
fn ledger_run_wanted(
    ctx: &RuntimeContext,
    store: &Store,
    args: &SpawnArgs,
) -> Result<Option<Option<String>>> {
    let workspace = super::sessionscmd::caller_workspace(ctx);
    run_to_join(
        store,
        &project_root(ctx)?,
        explicit_run(args),
        workspace.as_deref(),
        args,
    )
}

/// [`ledger_run_wanted`] with its inputs read.
fn run_to_join(
    store: &Store,
    root: &Path,
    explicit: Option<String>,
    workspace: Option<&str>,
    args: &SpawnArgs,
) -> Result<Option<Option<String>>> {
    if explicit.is_some() {
        return Ok(Some(explicit));
    }
    if let Some(prev) = &args.replaces {
        if let Some(run) = workers::find(store.conn(), prev)?.and_then(|w| w.run_id) {
            return Ok(Some(Some(run)));
        }
    }
    let active = active_runs(store, root)?;
    // A solo run never makes the workspace's planned run ambiguous (TRF-34).
    let own = runs::planned_first(
        active
            .iter()
            .filter(|r| workspace.is_some() && r.workspace_id.as_deref() == workspace)
            .collect(),
    );
    match own.as_slice() {
        [one] => Ok(Some(Some(one.run_id.clone()))),
        [] => {
            ensure!(
                args.replaces.is_none(),
                "--replaces needs an active task-ledger run in {}",
                root.display()
            );
            note_other_workspaces(&active, workspace);
            Ok(None)
        }
        many => bail!(
            "{} active runs in workspace {}: {}. Name one with --run <slug>",
            many.len(),
            workspace.unwrap_or_default(),
            many.iter()
                .map(|r| r.slug.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

/// Say that a spawn joins no run, because each active run of the
/// project belongs to another workspace than the caller's.
fn note_other_workspaces(active: &[Run], workspace: Option<&str>) {
    if active.is_empty() {
        return;
    }
    let runs: Vec<String> = active
        .iter()
        .map(|r| {
            format!(
                "{} (workspace {})",
                r.slug,
                r.workspace_id.as_deref().unwrap_or("(none)")
            )
        })
        .collect();
    eprintln!(
        "horch spawn: NOTE: this spawn joins no task-ledger run. Run {} is not of this workspace {}. Give --run <slug> to join it.",
        runs.join(", "),
        workspace.unwrap_or("(none)")
    );
}

/// The run a role joins (T-38, decision Q4): the active run of the
/// `spawn_only` controller's ledger row, when the spawn has task text and
/// names no run. Only a controller passes `controller`, so every other
/// spawn with task text keeps #89.
fn controller_run(
    store: &Store,
    args: &SpawnArgs,
    controller: Option<&str>,
) -> Result<Option<Run>> {
    let Some(wid) = controller else {
        return Ok(None);
    };
    if explicit_run(args).is_some() || args.task.trim().is_empty() {
        return Ok(None);
    }
    let Some(run_id) = workers::find(store.conn(), wid)?.and_then(|w| w.run_id) else {
        return Ok(None);
    };
    let run = runs::get(store.conn(), &run_id)?;
    Ok((run.status == RunStatus::Active.as_str()).then_some(run))
}

/// The text a role's row starts its "no claim" verdict with. It is the
/// prefix of the watchdog's own verdict (`workers::sweep_preclaim`), which
/// reports a `claiming` worker once and never again after it.
const NO_CLAIM: &str = "No claim:";

/// Record that a role works from its brief and claims no task, so the
/// watchdog never reports it as a worker that has not claimed one.
fn mark_no_claim(store: &mut Store, run: &Run, worker_id: &str, controller: &str) -> Result<()> {
    store.immediate(|tx| {
        util::event(
            tx,
            &About {
                run: Some(&run.run_id),
                task: None,
                worker: Some(worker_id),
            },
            Actor::Horch,
            EventKind::Verdict,
            &format!(
                "{NO_CLAIM} a role of controller worker {controller}; it works from its brief and claims no task."
            ),
        )
    })
}

/// The run `--run` or `HORCH_RUN_ID` names.
fn explicit_run(args: &SpawnArgs) -> Option<String> {
    args.run
        .clone()
        .or_else(|| std::env::var("HORCH_RUN_ID").ok().filter(|v| !v.is_empty()))
}

/// The active runs of the project.
fn active_runs(store: &Store, root: &Path) -> Result<Vec<Run>> {
    let all = runs::of_project(store.conn(), root)?;
    Ok(all
        .into_iter()
        .filter(|r| r.status == RunStatus::Active.as_str())
        .collect())
}

/// Insert the worker row of this spawn and return its id.
fn insert_row(
    ctx: &RuntimeContext,
    store: &mut Store,
    args: &SpawnArgs,
    run: &Run,
    persona: String,
) -> Result<String> {
    store.immediate(|tx| {
        workers::insert(
            tx,
            &NewWorker {
                run_id: run.run_id.clone(),
                kind: Some(WorkerKind::Worker),
                persona,
                workspace_id: ctx.herdr.workspace.as_ref().map(|w| w.as_str().to_string()),
                previous_worker_id: args.replaces.clone(),
                resumed_from_session: args.resume.clone(),
                target_task_id: claim_target(),
                ..Default::default()
            },
        )
    })
}

/// The cause of the event that records the adoption of a worker row.
const ADOPTED: &str = "adopted_by_spawn";

/// The worker row that [`ADOPT_WORKER_ENV`] names, if it is set: fleetd
/// wrote it for a restart on a fallback.
fn adopt_wanted() -> Option<String> {
    std::env::var(ADOPT_WORKER_ENV)
        .ok()
        .filter(|v| !v.trim().is_empty())
}

/// Adopt the worker row `worker_id` that fleetd wrote for a restart on a
/// fallback (RR-11), in place of a new row. The variable is an input that
/// any process can set, so the row must be in `spawning` with no agent
/// PID, hold exactly 1 attempt, in `launching`, and have the run, the
/// persona and the target task of this spawn. The check and the record of
/// the adoption are 1 immediate transaction through the door, so 2 spawns
/// that name 1 row cannot both adopt it. Each other case refuses and
/// adopts nothing.
///
/// The Phase 2 bridge: Phase 4 replaces it with the push dispatch, which
/// binds the task to a worker before the pane opens.
fn adopt(
    store: &mut Store,
    worker_id: &str,
    run: &Run,
    persona: &str,
    target: Option<&str>,
) -> Result<String> {
    store.immediate(|tx| {
        let refusal = |why: &str| {
            anyhow::anyhow!("horch spawn refused: it cannot adopt worker row {worker_id}: {why}")
        };
        let Some(w) = workers::find(tx, worker_id)? else {
            return Err(refusal("no worker row has this id"));
        };
        if w.run_id.as_deref() != Some(run.run_id.as_str()) {
            return Err(refusal(&format!(
                "the row is not a row of run {}",
                run.slug
            )));
        }
        if w.status != WorkerStatus::Spawning.as_str() {
            return Err(refusal(&format!("the row is {}, not spawning", w.status)));
        }
        if w.agent_pid.is_some() {
            return Err(refusal("the row has an agent PID"));
        }
        if w.persona.as_deref() != Some(persona) {
            return Err(refusal(&format!(
                "the row is not a row of persona {persona}"
            )));
        }
        if target.is_none() || w.target_task_id.as_deref() != target {
            return Err(refusal(&format!(
                "the row is not spawned for the task that {CLAIM_TARGET_ENV} names"
            )));
        }
        let attempts = attempts_store::attempts_of_worker(tx, worker_id)?;
        let launching = matches!(
            attempts.as_slice(),
            [a] if a.state == AttemptState::Launching && a.task_id.as_deref() == target
        );
        if !launching {
            return Err(refusal(
                "the row does not have exactly 1 attempt in launching",
            ));
        }
        let adopted: i64 = tx.query_row(
            "SELECT COUNT(*) FROM events WHERE entity = ?1 AND entity_id = ?2 AND cause = ?3",
            rusqlite::params![Entity::Worker.as_str(), worker_id, ADOPTED],
            |r| r.get(0),
        )?;
        if adopted > 0 {
            return Err(refusal("another spawn adopted the row"));
        }
        let by = Cause {
            actor: Actor::Worker,
            text: ADOPTED,
        };
        let data = serde_json::json!({ "spawn_pid": std::process::id() });
        ledger::record(
            tx,
            None,
            EventKind::Note,
            Entity::Worker,
            worker_id,
            by,
            &data,
        )?;
        Ok(worker_id.to_string())
    })
}

/// The task [`CLAIM_TARGET_ENV`] names, if it is set.
fn claim_target() -> Option<String> {
    std::env::var(CLAIM_TARGET_ENV)
        .ok()
        .filter(|v| !v.trim().is_empty())
}

/// A closed run takes no new worker.
fn ensure_active(run: &Run) -> Result<()> {
    ensure!(
        run.status == RunStatus::Active.as_str(),
        "run {} is {}; a closed run takes no new worker",
        run.slug,
        run.status
    );
    Ok(())
}

/// The persona of the new worker: the teammate, or the tier of the
/// execution `--resume` names.
fn spawn_persona(ctx: &RuntimeContext, args: &SpawnArgs) -> Result<String> {
    match (&args.teammate, &args.resume) {
        (Some(t), _) => Ok(t.clone()),
        (None, Some(key)) => Ok(ExecutionStore::open_in(ctx)?.get(key)?.tier),
        (None, None) => bail!("give a teammate or --resume <id>"),
    }
}

/// Refuse to replace a worker that is still live.
fn ensure_replaceable(store: &Store, args: &SpawnArgs, run: &Run) -> Result<()> {
    let Some(prev) = &args.replaces else {
        return Ok(());
    };
    let p = workers::get(store.conn(), prev)?;
    ensure!(
        !p.status()?.is_live(),
        "worker {} is still {}. Run horch ledger stop {} \"<reason>\" --run {}, then spawn again",
        p.label(),
        p.status,
        p.worker_id,
        run.slug
    );
    Ok(())
}

impl LedgerSpawn {
    /// The pane is open and runs `horch worker <role>`.
    pub fn spawned(
        &self,
        role: &str,
        pane: &str,
        harness: &str,
        model: &str,
        effort: Option<&str>,
    ) {
        let r = self.store.borrow_mut().immediate(|tx| {
            workers::set_pane(tx, &self.worker_id, pane)?;
            workers::set_launch(tx, &self.worker_id, role, harness, model, effort)
        });
        if let Err(e) = r {
            eprintln!("horch spawn: the task ledger did not record pane {pane}: {e:#}");
        }
        eprintln!(
            "horch spawn: task ledger worker {} (run {}).",
            self.worker_id, self.run.slug
        );
    }

    /// The spawn failed before the worker ran. A bind of this spawn is
    /// undone in the same transaction ([`unbind`]).
    pub fn launch_failed(&self, reason: &str) {
        let r = self.store.borrow_mut().immediate(|tx| {
            workers::launch_failed(tx, &self.worker_id, reason)?;
            match self.push.as_ref().filter(|p| p.own_bind) {
                Some(p) => unbind(tx, p, reason),
                None => Ok(()),
            }
        });
        if let Err(e) = r {
            eprintln!("horch spawn: the task ledger did not record the failed start: {e:#}");
        }
    }
}

/// What 1 reviewer spawn did (TRF-24).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReviewSpawn {
    /// The worker runs as `role`. `substituted` is the gate's SUBSTITUTED line.
    Spawned {
        role: String,
        substituted: Option<String>,
    },
    /// The gate refused: its REFUSED line.
    Refused(String),
    /// The spawn failed for another reason.
    Failed(String),
}

/// Spawns 1 claiming worker of a persona in a run, reserved for 1 task.
pub trait SpawnReviewer {
    fn spawn(&self, persona: &str, run: &Run, task_id: &str) -> ReviewSpawn;
}

/// The normal spawn path: `horch spawn <persona> --run <slug>` as a child
/// process, so the spawn gate decides and its lines stay off this stdout.
/// [`CLAIM_TARGET_ENV`] reserves the review task for the new worker.
pub struct CliSpawn;

impl SpawnReviewer for CliSpawn {
    fn spawn(&self, persona: &str, run: &Run, task_id: &str) -> ReviewSpawn {
        let exe = match std::env::current_exe() {
            Ok(exe) => exe,
            Err(e) => return ReviewSpawn::Failed(format!("finding the horch binary: {e}")),
        };
        let out = horch_platform::start::command(exe)
            .args(["spawn", persona, "--run", &run.slug])
            .env(CLAIM_TARGET_ENV, task_id)
            .stdin(std::process::Stdio::null())
            .output();
        match out {
            Ok(o) => spawn_outcome(
                o.status.code(),
                &String::from_utf8_lossy(&o.stdout),
                &String::from_utf8_lossy(&o.stderr),
            ),
            Err(e) => ReviewSpawn::Failed(format!("starting horch spawn: {e}")),
        }
    }
}

/// Read the outcome of 1 `horch spawn` from its exit code and output.
pub fn spawn_outcome(code: Option<i32>, stdout: &str, stderr: &str) -> ReviewSpawn {
    let line_of = |prefix: &str| {
        stdout
            .lines()
            .map(str::trim)
            .find(|l| l.starts_with(prefix))
            .map(str::to_string)
    };
    if code == Some(crate::exit::REFUSED as i32) {
        return ReviewSpawn::Refused(line_of("REFUSED:").unwrap_or_else(|| "REFUSED.".into()));
    }
    if code != Some(0) {
        let why = stderr
            .lines()
            .rev()
            .map(str::trim)
            .find(|l| !l.is_empty())
            .unwrap_or("horch spawn failed with no message");
        return ReviewSpawn::Failed(why.to_string());
    }
    let role = stderr
        .lines()
        .find_map(|l| l.trim().strip_prefix("spawned "))
        .and_then(|r| r.split(" (pane").next())
        .unwrap_or("a worker")
        .to_string();
    ReviewSpawn::Spawned {
        role,
        substituted: line_of("SUBSTITUTED:"),
    }
}

/// Spawn 1 claiming worker for each queued review task of `of` through
/// `spawner`, then write 1 NOTE wake that names the reviewers. Each worker
/// is reserved for its review task and claims it first. A refused or
/// failed spawn leaves its task queued and says so in the NOTE. Returns
/// the NOTE text.
pub fn spawn_reviewers(
    ctx: &RuntimeContext,
    store: &Store,
    run: &Run,
    of: &str,
    reviews: &[String],
    spawner: &dyn SpawnReviewer,
) -> Result<String> {
    let mut spawned = 0;
    let mut items = Vec::new();
    let mut shorts = Vec::new();
    for id in reviews {
        let t = horch_core::taskledger::tasks::get(store.conn(), id)?;
        let persona = t
            .claimable_by
            .clone()
            .or_else(|| t.primary_persona.clone())
            .unwrap_or_default();
        let short = t.short();
        let item = match spawner.spawn(&persona, run, id) {
            ReviewSpawn::Spawned { role, substituted } => {
                spawned += 1;
                let fallback = format!(
                    "{role} claims {short} first. If {short} is not startable at the claim, {role} claims the next task of {persona}."
                );
                match substituted {
                    Some(s) => {
                        format!("{role} ({persona}) is spawned for {short}. {s} {fallback}")
                    }
                    None => format!("{role} ({persona}) is spawned for {short}. {fallback}"),
                }
            }
            ReviewSpawn::Refused(line) => format!("{short} stays queued. {line}"),
            ReviewSpawn::Failed(why) => {
                format!("{short} stays queued. The spawn of {persona} failed: {why}")
            }
        };
        items.push(item);
        shorts.push(short);
    }
    let text = format!(
        "NOTE: horch spawned {spawned} of {} reviewers for the code-sweep of {of}. {}",
        reviews.len(),
        items.join(" ")
    );
    let wake = horch_core::taskledger::outbox::Wake::new(
        horch_core::taskledger::model::OutboxEvent::Startable,
        format!("reviewers:{}", shorts.join(",")),
        format!("[horch] {text}"),
    );
    super::taskledger_wake::wake_as(ctx, store, run, |_| Ok(Some(wake)));
    Ok(text)
}

#[cfg(test)]
mod trf_24_tests {
    use std::cell::RefCell;

    use horch_core::runtime::MapEnv;
    use horch_core::taskledger::model::{Actor, TaskKind};
    use horch_core::taskledger::runs::NewRun;
    use horch_core::taskledger::tasks::{self, NewTask};

    use super::*;

    /// A fake spawn: 1 outcome per call, in order; it records each persona.
    struct Fake {
        outcomes: RefCell<Vec<ReviewSpawn>>,
        personas: RefCell<Vec<String>>,
        targets: RefCell<Vec<String>>,
    }

    impl Fake {
        fn new(outcomes: Vec<ReviewSpawn>) -> Self {
            Fake {
                outcomes: RefCell::new(outcomes),
                personas: RefCell::new(Vec::new()),
                targets: RefCell::new(Vec::new()),
            }
        }
    }

    impl SpawnReviewer for Fake {
        fn spawn(&self, persona: &str, _run: &Run, task_id: &str) -> ReviewSpawn {
            self.personas.borrow_mut().push(persona.to_string());
            self.targets.borrow_mut().push(task_id.to_string());
            self.outcomes.borrow_mut().remove(0)
        }
    }

    fn spawned(role: &str, substituted: Option<&str>) -> ReviewSpawn {
        ReviewSpawn::Spawned {
            role: role.into(),
            substituted: substituted.map(str::to_string),
        }
    }

    /// A ledger, a run, 1 work task and 1 review task per persona.
    fn world(personas: &[&str]) -> (tempfile::TempDir, RuntimeContext, Store, Run, Vec<String>) {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("ledger.sqlite3");
        let ctx = RuntimeContext::from_env(
            &MapEnv::new("/")
                .with(
                    "HORCH_STATE_DIR",
                    dir.path().join("state").to_str().unwrap(),
                )
                .with("HORCH_LEDGER_DB", db.to_str().unwrap()),
        )
        .unwrap();
        let mut store = Store::open(&db).unwrap();
        let run = store
            .immediate(|tx| {
                runs::create(
                    tx,
                    Path::new("/repo"),
                    &NewRun {
                        slug: "r1".into(),
                        ..Default::default()
                    },
                )
            })
            .unwrap();
        let work = store
            .immediate(|tx| {
                tasks::add(
                    tx,
                    &run,
                    &NewTask::work("alpha", "tickets/alpha.md", "opus"),
                    Actor::Orchestrator,
                )
            })
            .unwrap();
        let mut reviews = Vec::new();
        for (i, p) in personas.iter().enumerate() {
            let mut t = NewTask::work(&format!("Review T-1: r{i}"), "tickets/r.md", p);
            t.kind = TaskKind::Review;
            t.parent_task_id = Some(work.task_id.clone());
            let r = store
                .immediate(|tx| tasks::add(tx, &run, &t, Actor::Orchestrator))
                .unwrap();
            reviews.push(r.task_id);
        }
        (dir, ctx, store, run, reviews)
    }

    fn outbox(store: &Store, run: &Run) -> Vec<(String, String)> {
        let mut stmt = store
            .conn()
            .prepare("SELECT event, text FROM outbox WHERE run_id = ?1 ORDER BY rowid")
            .unwrap();
        stmt.query_map([&run.run_id], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    }

    fn short(store: &Store, id: &str) -> String {
        tasks::get(store.conn(), id).unwrap().short()
    }

    #[test]
    fn trf_24_horch_spawns_1_reviewer_per_review_task_and_the_orchestrator_gets_1_note() {
        let (_dir, ctx, store, run, reviews) =
            world(&["codex-reviewer", "codex-reviewer", "opus-reviewer"]);
        let fake = Fake::new(vec![
            spawned("codex-reviewer-1", None),
            spawned(
                "codex-reviewer-2",
                Some("SUBSTITUTED: codex-reviewer runs on opus. Reason: codex 7d 100%."),
            ),
            spawned("opus-reviewer-1", None),
        ]);
        let note = spawn_reviewers(&ctx, &store, &run, "T-1", &reviews, &fake).unwrap();
        assert_eq!(
            *fake.personas.borrow(),
            ["codex-reviewer", "codex-reviewer", "opus-reviewer"]
        );
        assert_eq!(
            *fake.targets.borrow(),
            reviews,
            "each spawn names its review"
        );
        let rows = outbox(&store, &run);
        assert_eq!(rows.len(), 1, "{rows:?}");
        assert_eq!(rows[0].0, "startable");
        assert_eq!(rows[0].1, format!("[horch] {note}"));
        assert!(
            note.starts_with("NOTE: horch spawned 3 of 3 reviewers for the code-sweep of T-1."),
            "{note}"
        );
        for (role, id) in [
            ("codex-reviewer-1", &reviews[0]),
            ("codex-reviewer-2", &reviews[1]),
            ("opus-reviewer-1", &reviews[2]),
        ] {
            let item = format!("{role} (");
            assert!(note.contains(&item), "{note}");
            let s = short(&store, id);
            assert!(note.contains(&format!("spawned for {s}.")), "{note}");
            assert!(
                note.contains(&format!(
                    "{role} claims {s} first. If {s} is not startable at the claim"
                )),
                "the NOTE says what the worker claims: {note}"
            );
        }
        assert!(
            note.contains("SUBSTITUTED: codex-reviewer runs on opus."),
            "a substitution proceeds and is noted: {note}"
        );
    }

    #[test]
    fn trf_24_a_refused_spawn_leaves_its_task_queued_and_writes_a_note_row() {
        let (_dir, ctx, store, run, reviews) = world(&["codex-reviewer", "opus-reviewer"]);
        let refused = "REFUSED: codex-reviewer cannot start. codex exhausted (7d 100%). Options: wait, --force, or choose a teammate yourself.";
        let fake = Fake::new(vec![
            ReviewSpawn::Refused(refused.into()),
            spawned("opus-reviewer-1", None),
        ]);
        let note = spawn_reviewers(&ctx, &store, &run, "T-1", &reviews, &fake).unwrap();
        let t = tasks::get(store.conn(), &reviews[0]).unwrap();
        assert_eq!(t.status, "ready");
        assert!(t.current_worker_id.is_none());
        assert!(
            note.starts_with("NOTE: horch spawned 1 of 2 reviewers"),
            "{note}"
        );
        assert!(
            note.contains(&format!("{} stays queued. {refused}", t.short())),
            "{note}"
        );
        let rows = outbox(&store, &run);
        assert_eq!(rows.len(), 1, "{rows:?}");
        assert!(rows[0].1.contains(refused), "{rows:?}");
    }

    #[test]
    fn trf_24_the_spawn_outcome_comes_from_the_exit_code_and_the_gate_lines() {
        let ok = spawn_outcome(
            Some(0),
            "SUBSTITUTED: opus runs on codex-sol. Reason: claude 7d 100%.\nw1:p4\n",
            "horch spawn: task ledger worker abc (run r1).\nspawned codex-sol-3 (pane w1:p4): codex gpt-5, new session\n",
        );
        assert_eq!(
            ok,
            spawned(
                "codex-sol-3",
                Some("SUBSTITUTED: opus runs on codex-sol. Reason: claude 7d 100%.")
            )
        );
        assert_eq!(
            spawn_outcome(
                Some(0),
                "w1:p4\n",
                "spawned opus-2 (pane w1:p4): claude opus, new session\n"
            ),
            spawned("opus-2", None)
        );
        assert_eq!(
            spawn_outcome(
                Some(3),
                "REFUSED: opus cannot start. x.\n",
                "horch: refused\n"
            ),
            ReviewSpawn::Refused("REFUSED: opus cannot start. x.".into())
        );
        assert_eq!(
            spawn_outcome(Some(1), "", "horch: horch spawn refused: enough workers\n"),
            ReviewSpawn::Failed("horch: horch spawn refused: enough workers".into())
        );
    }
}

#[cfg(test)]
mod trf_33_34_tests {
    use std::time::Duration;

    use horch_core::execution::TilingMode;
    use horch_core::taskledger::model::{Actor, StopRequester};
    use horch_core::taskledger::runs::NewRun;
    use horch_core::taskledger::stop::{self, Panes, Processes};
    use horch_core::taskledger::tasks::{self, NewTask};
    use horch_core::workspace::model::Direction;

    use super::*;

    fn args(teammate: Option<&str>) -> SpawnArgs {
        SpawnArgs {
            teammate: teammate.map(str::to_string),
            task: String::new(),
            resume: None,
            phase: None,
            effort: None,
            model: None,
            role: None,
            from_pane: None,
            direction: Direction::Right,
            tiling: TilingMode::Disabled,
            exact: false,
            force: false,
            replaces: None,
            run: None,
            attempt: None,
        }
    }

    /// A ledger with 1 active run per (slug, workspace) in project `root`.
    fn world(runs_of: &[(&str, Option<&str>)]) -> (tempfile::TempDir, Store, Vec<Run>) {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("ledger.sqlite3")).unwrap();
        let made = runs_of
            .iter()
            .map(|(slug, ws)| {
                store
                    .immediate(|tx| {
                        runs::create(
                            tx,
                            dir.path(),
                            &NewRun {
                                slug: (*slug).into(),
                                workspace_id: ws.map(str::to_string),
                                ..Default::default()
                            },
                        )
                    })
                    .unwrap()
            })
            .collect();
        (dir, store, made)
    }

    /// A machine fixture with `free_gb` GB of free disk; each other
    /// reading is healthy.
    fn machine_file(dir: &Path, free_gb: u64) -> std::path::PathBuf {
        let path = dir.join(format!("machine-{free_gb}.json"));
        let gb: u64 = 1 << 30;
        let snapshot = serde_json::json!({
            "os": "macos",
            "arch": "arm64",
            "cpus": 8,
            "load_average_1m": 1.0,
            "mem_total_bytes": 16 * gb,
            "mem_available_bytes": 8 * gb,
            "disk_free_bytes": free_gb * gb,
            "disk_total_bytes": 500 * gb,
            "gpu": "none",
            "max_open_files": 256,
            "max_processes": 100_000,
        });
        std::fs::write(&path, snapshot.to_string()).unwrap();
        path
    }

    #[test]
    fn ledger_spawn_refuses_with_the_hold_and_its_cause() {
        let (dir, store, made) = world(&[("r1", None)]);
        let ctx_with = |free_gb: u64| {
            let machine = machine_file(dir.path(), free_gb);
            RuntimeContext::from_env(
                &horch_core::runtime::MapEnv::new("/")
                    .with(
                        "HORCH_STATE_DIR",
                        dir.path().join("state").to_str().unwrap(),
                    )
                    .with(
                        "HORCH_LEDGER_DB",
                        dir.path().join("ledger.sqlite3").to_str().unwrap(),
                    )
                    .with("FLEET_ROOT", dir.path().join("fleet").to_str().unwrap())
                    .with("HORCH_PROJECT_DIR", dir.path().to_str().unwrap())
                    .with("HORCH_MACHINE_FILE", machine.to_str().unwrap()),
            )
            .unwrap()
        };
        let mut spawn = args(Some("opus"));
        spawn.run = Some(made[0].slug.clone());
        let workers = || -> i64 {
            store
                .conn()
                .query_row("SELECT COUNT(*) FROM workers", [], |r| r.get(0))
                .unwrap()
        };

        let err = prepare(&ctx_with(2), &spawn, None)
            .err()
            .expect("a refusal");
        assert_eq!(
            err.to_string(),
            "horch spawn refused: new dispatch is on hold until the machine recovers:\n  \
             - free disk 2.0 GB is below the floor of 10 GB (health_min_free_disk_gb)"
        );
        assert_eq!(workers(), 0, "a refused spawn writes no row");

        // A normal probe resumes dispatch: the spawn passes the guard, and
        // the run has no ready task to bind.
        let err = prepare(&ctx_with(200), &spawn, None)
            .err()
            .expect("no ready task");
        assert_eq!(
            err.to_string(),
            "horch spawn refused: run r1 has no ready task for persona opus"
        );
        assert_eq!(workers(), 0);
    }

    /// A resume spawn joins the run of its workspace, so it binds a task
    /// as each worker spawn does (TRF-33).
    #[test]
    fn trf_33_a_resume_spawn_joins_the_run_of_its_workspace() {
        let (dir, store, made) = world(&[("r1", Some("w1"))]);
        let mut resume = args(None);
        resume.resume = Some("s-1".into());
        let joined = run_to_join(&store, dir.path(), None, Some("w1"), &resume).unwrap();
        assert_eq!(joined, Some(Some(made[0].run_id.clone())));
        // A spawn outside every run gets no task.
        assert_eq!(super::super::spawn::worker_task(None), "");
    }

    #[test]
    fn trf_34_a_task_less_spawn_joins_only_its_own_workspaces_run() {
        let (dir, store, made) = world(&[("r1", Some("w1")), ("r2", Some("w2"))]);
        let root = dir.path();
        let bare = args(Some("opus"));
        let join = |ws: Option<&str>, explicit: Option<&str>| {
            run_to_join(&store, root, explicit.map(str::to_string), ws, &bare).unwrap()
        };
        assert_eq!(join(Some("w1"), None), Some(Some(made[0].run_id.clone())));
        assert_eq!(join(Some("w2"), None), Some(Some(made[1].run_id.clone())));
        // Another workspace's 1 active run is not joined.
        assert_eq!(join(Some("w3"), None), None);
        assert_eq!(join(None, None), None);
        // --run reaches another workspace's run.
        assert_eq!(join(Some("w1"), Some("r2")), Some(Some("r2".to_string())));

        // 1 run of another workspace only: still not joined.
        let (dir, store, _) = world(&[("r9", Some("w9"))]);
        assert_eq!(
            run_to_join(&store, dir.path(), None, Some("w1"), &bare).unwrap(),
            None
        );
    }

    /// TRF-34: a solo run (`horch ledger task`) beside the workspace's
    /// planned run does not make a plain spawn ambiguous.
    #[test]
    fn trf_34_a_plain_spawn_joins_the_planned_run_beside_a_solo_run() {
        let (dir, store, made) = world(&[("solo-w1", Some("w1")), ("plan", Some("w1"))]);
        let bare = args(Some("opus"));
        let join = |explicit: Option<&str>| {
            run_to_join(
                &store,
                dir.path(),
                explicit.map(str::to_string),
                Some("w1"),
                &bare,
            )
            .unwrap()
        };
        assert_eq!(join(None), Some(Some(made[1].run_id.clone())));
        // --run still reaches the solo run.
        assert_eq!(join(Some("solo-w1")), Some(Some("solo-w1".to_string())));

        // A solo run alone is still joined.
        let (dir, store, made) = world(&[("solo-w1", Some("w1"))]);
        assert_eq!(
            run_to_join(&store, dir.path(), None, Some("w1"), &bare).unwrap(),
            Some(Some(made[0].run_id.clone()))
        );
    }

    #[test]
    fn trf_34_two_planned_runs_still_refuse() {
        let (dir, store, _) = world(&[
            ("solo-w1", Some("w1")),
            ("p1", Some("w1")),
            ("p2", Some("w1")),
        ]);
        let err = run_to_join(&store, dir.path(), None, Some("w1"), &args(Some("opus")))
            .unwrap_err()
            .to_string();
        assert!(err.contains("2 active runs in workspace w1: "), "{err}");
        assert!(err.contains("p1") && err.contains("p2"), "{err}");
        assert!(!err.contains("solo-w1"), "{err}");
    }

    struct NoPanes;
    impl Panes for NoPanes {
        fn exists(&self, _: &str) -> bool {
            false
        }
        fn close(&self, _: &str) -> Result<()> {
            Ok(())
        }
    }
    struct NoProcs;
    impl Processes for NoProcs {
        fn is_same(&self, _: u32, _: u64) -> bool {
            false
        }
        fn exists(&self, _: u32) -> bool {
            false
        }
        fn stop_same(&self, _: u32, _: u64, _: horch_core::procid::Stop) -> bool {
            false
        }
    }

    /// Ticket 19's `refuse_over_plan` guard: a `--replaces` spawn after a
    /// reap that requeued the task is not refused.
    #[test]
    fn trf_33_a_replaces_spawn_after_a_reap_and_requeue_is_not_refused() {
        let (dir, mut store, made) = world(&[("r1", Some("w1"))]);
        let run = made[0].clone();
        store
            .immediate(|tx| {
                tasks::add(
                    tx,
                    &run,
                    &NewTask::work("alpha", "tickets/a.md", "opus"),
                    Actor::Orchestrator,
                )
            })
            .unwrap();
        let new_row = |store: &mut Store, prev: Option<String>| {
            store
                .immediate(|tx| {
                    workers::insert(
                        tx,
                        &NewWorker {
                            run_id: run.run_id.clone(),
                            kind: Some(WorkerKind::Worker),
                            persona: "opus".into(),
                            workspace_id: Some("w1".into()),
                            previous_worker_id: prev.clone(),
                            ..Default::default()
                        },
                    )
                })
                .unwrap()
        };
        let first = new_row(&mut store, None);
        let claimed = store.immediate(|tx| tasks::claim(tx, &first)).unwrap();
        assert!(
            matches!(claimed, tasks::ClaimOutcome::Claimed(_)),
            "{claimed:?}"
        );
        stop::reap(
            &mut store,
            &first,
            StopRequester::Orchestrator,
            "pane gone",
            &NoPanes,
            &NoProcs,
            Duration::ZERO,
        )
        .unwrap();
        let t = &tasks::of_run(store.conn(), &run.run_id).unwrap()[0];
        assert_eq!(t.status, "ready", "the reap requeues the task");

        let mut replace = args(Some("opus"));
        replace.replaces = Some(first.clone());
        let joined = run_to_join(&store, dir.path(), None, Some("w1"), &replace).unwrap();
        assert_eq!(joined, Some(Some(run.run_id.clone())));
        ensure_replaceable(&store, &replace, &run).unwrap();
        let second = new_row(&mut store, Some(first));
        assert_eq!(
            tasks::spawn_refusal(store.conn(), &run.run_id, &second).unwrap(),
            None
        );
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use horch_core::taskledger::runs::NewRun;

    use super::*;

    /// T-38 step 3: the watchdog's pre-claim sweep reports a `claiming`
    /// worker past the claim grace, but never a role that `prepare` marked.
    /// The role is not ended either: its supervisor checks in.
    #[test]
    fn a_marked_role_is_never_reported_as_a_worker_that_did_not_claim() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Store::open(&dir.path().join("l.sqlite3")).unwrap();
        let (run, role, plain) = s
            .immediate(|tx| {
                let run = runs::create(
                    tx,
                    Path::new("/r"),
                    &NewRun {
                        slug: "s".into(),
                        ..Default::default()
                    },
                )?;
                let new = |persona: &str| NewWorker {
                    run_id: run.run_id.clone(),
                    kind: Some(WorkerKind::Worker),
                    persona: persona.into(),
                    ..Default::default()
                };
                let role = workers::insert(tx, &new("intake-analyst"))?;
                let plain = workers::insert(tx, &new("opus"))?;
                Ok((run, role, plain))
            })
            .unwrap();
        mark_no_claim(&mut s, &run, &role, "w-controller").unwrap();
        s.conn()
            .execute(
                "UPDATE workers SET status = 'claiming', started_at = '2000-01-01T00:00:00Z',
                    last_checkin_at = ?1",
                [util::now()],
            )
            .unwrap();
        let swept = s
            .immediate(|tx| workers::sweep_preclaim(tx, &run, &|_| true))
            .unwrap();
        let noted: Vec<&str> = swept.iter().map(|p| p.worker.worker_id.as_str()).collect();
        assert_eq!(noted, [plain.as_str()], "{swept:?}");
        assert!(swept.iter().all(|p| p.ended.is_none()), "{swept:?}");
    }
}

#[cfg(test)]
mod adoption_tests {
    use horch_core::fleetd::task_execution::domain::task::AttemptRow;
    use horch_core::taskledger::runs::NewRun;
    use horch_core::taskledger::tasks::{self, NewTask};

    use super::*;

    struct World {
        dir: tempfile::TempDir,
        store: Store,
        run: Run,
        task: String,
    }

    /// A run with 1 task of `dev` and a second run.
    fn world() -> World {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("ledger.sqlite3")).unwrap();
        let (run, task) = store
            .immediate(|tx| {
                let run = runs::create(
                    tx,
                    dir.path(),
                    &NewRun {
                        slug: "r1".into(),
                        ..Default::default()
                    },
                )?;
                let t = tasks::add(
                    tx,
                    &run,
                    &NewTask::work("a", "tickets/a.md", "dev"),
                    Actor::Orchestrator,
                )?;
                Ok((run, t.task_id))
            })
            .unwrap();
        World {
            dir,
            store,
            run,
            task,
        }
    }

    impl World {
        /// The rows that fleetd writes for a restart on `alt`: a worker in
        /// `spawning` for the task, with `attempts` attempts of `state`.
        fn restart_row(&mut self, attempts: &[AttemptState]) -> String {
            let (run_id, task) = (self.run.run_id.clone(), self.task.clone());
            self.store
                .immediate(|tx| {
                    let id = workers::insert(
                        tx,
                        &NewWorker {
                            run_id: run_id.clone(),
                            kind: Some(WorkerKind::Worker),
                            persona: "alt".into(),
                            target_task_id: Some(task.clone()),
                            ..Default::default()
                        },
                    )?;
                    for (n, state) in attempts.iter().enumerate() {
                        let a = AttemptRow {
                            id: format!("{task}/A{n}-{id}"),
                            task_id: Some(task.clone()),
                            worker_id: id.clone(),
                            state: *state,
                            process: None,
                            pane: None,
                        };
                        attempts_store::insert_attempt(tx, &a, &util::now())?;
                    }
                    Ok(id)
                })
                .unwrap()
        }

        fn adopt(&mut self, row: &str, persona: &str, target: Option<&str>) -> Result<String> {
            let run = self.run.clone();
            adopt(&mut self.store, row, &run, persona, target)
        }

        fn adopted_events(&self, row: &str) -> i64 {
            self.store
                .conn()
                .query_row(
                    "SELECT COUNT(*) FROM events WHERE entity_id = ?1 AND cause = ?2",
                    [row, ADOPTED],
                    |r| r.get(0),
                )
                .unwrap()
        }
    }

    fn refused(r: Result<String>, why: &str) {
        let e = format!("{:#}", r.expect_err(why));
        assert!(e.starts_with("horch spawn refused: it cannot adopt"), "{e}");
        assert!(e.contains(why), "{e} does not say {why}");
    }

    #[test]
    fn spawn_adopts_the_row_that_fleetd_wrote_once() {
        let mut w = world();
        let row = w.restart_row(&[AttemptState::Launching]);
        let task = w.task.clone();
        assert_eq!(w.adopt(&row, "alt", Some(&task)).unwrap(), row);
        assert_eq!(w.adopted_events(&row), 1);
        refused(
            w.adopt(&row, "alt", Some(&task)),
            "another spawn adopted the row",
        );
        assert_eq!(w.adopted_events(&row), 1);
    }

    #[test]
    fn spawn_refuses_each_row_that_it_cannot_adopt() {
        let mut w = world();
        let task = w.task.clone();
        refused(
            w.adopt("no-such-row", "alt", Some(&task)),
            "no worker row has this id",
        );

        let row = w.restart_row(&[AttemptState::Launching]);
        let other = w
            .store
            .immediate(|tx| {
                runs::create(
                    tx,
                    w.dir.path(),
                    &NewRun {
                        slug: "r2".into(),
                        ..Default::default()
                    },
                )
            })
            .unwrap();
        refused(
            adopt(&mut w.store, &row, &other, "alt", Some(&task)),
            "not a row of run r2",
        );
        refused(
            w.adopt(&row, "dev", Some(&task)),
            "not a row of persona dev",
        );
        refused(w.adopt(&row, "alt", None), "not spawned for the task");
        refused(
            w.adopt(&row, "alt", Some("other-task")),
            "not spawned for the task",
        );

        let row = w.restart_row(&[]);
        refused(
            w.adopt(&row, "alt", Some(&task)),
            "exactly 1 attempt in launching",
        );
        let row = w.restart_row(&[AttemptState::Ended]);
        refused(
            w.adopt(&row, "alt", Some(&task)),
            "exactly 1 attempt in launching",
        );
        let row = w.restart_row(&[AttemptState::Launching, AttemptState::Launching]);
        refused(
            w.adopt(&row, "alt", Some(&task)),
            "exactly 1 attempt in launching",
        );

        let row = w.restart_row(&[AttemptState::Launching]);
        w.store
            .immediate(|tx| workers::agent_started(tx, &row, 4_242, Some(7)))
            .unwrap();
        refused(
            w.adopt(&row, "alt", Some(&task)),
            "the row has an agent PID",
        );

        let row = w.restart_row(&[AttemptState::Launching]);
        w.store
            .immediate(|tx| workers::launch_failed(tx, &row, "x"))
            .unwrap();
        refused(w.adopt(&row, "alt", Some(&task)), "not spawning");
        assert_eq!(w.adopted_events(&row), 0);
    }

    #[test]
    fn two_spawns_that_name_1_row_cannot_both_adopt_it() {
        let mut w = world();
        let row = w.restart_row(&[AttemptState::Launching]);
        let db = w.dir.path().join("ledger.sqlite3");
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let spawns: Vec<_> = (0..2)
            .map(|_| {
                let (db, row, run, task) = (db.clone(), row.clone(), w.run.clone(), w.task.clone());
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    let mut store = Store::open(&db).unwrap();
                    barrier.wait();
                    adopt(&mut store, &row, &run, "alt", Some(&task)).is_ok()
                })
            })
            .collect();
        let adopted: Vec<bool> = spawns.into_iter().map(|s| s.join().unwrap()).collect();
        assert_eq!(adopted.iter().filter(|a| **a).count(), 1, "{adopted:?}");
        assert_eq!(w.adopted_events(&row), 1);
    }
}

#[cfg(test)]
mod push_tests {
    use horch_core::execution::TilingMode;
    use horch_core::fleetd::task_execution::application::dispatch::write_missing_task_files;
    use horch_core::runtime::MapEnv;
    use horch_core::taskledger::model::{Actor, WorktreeMode};
    use horch_core::taskledger::runs::NewRun;
    use horch_core::taskledger::tasks::NewTask;
    use horch_core::workspace::model::Direction;

    use super::*;

    const HEALTHY: &str = r#"{"os":"macos","arch":"arm64","cpus":8,"load_average_1m":1.0,"mem_total_bytes":17179869184,"mem_available_bytes":8589934592,"disk_free_bytes":214748364800,"disk_total_bytes":536870912000,"gpu":"none","max_open_files":256,"max_processes":100000}"#;

    struct World {
        dir: tempfile::TempDir,
        store: Store,
        run: Run,
    }

    /// A ledger with the active run `r1` in temp folders, and a healthy
    /// machine fixture.
    fn world() -> World {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("machine.json"), HEALTHY).unwrap();
        let mut store = Store::open(&dir.path().join("ledger.sqlite3")).unwrap();
        let run = store
            .immediate(|tx| {
                runs::create(
                    tx,
                    dir.path(),
                    &NewRun {
                        slug: "r1".into(),
                        base_branch: Some("main".into()),
                        ..Default::default()
                    },
                )
            })
            .unwrap();
        World { dir, store, run }
    }

    impl World {
        fn fleet_root(&self) -> std::path::PathBuf {
            self.dir.path().join("fleet")
        }

        fn ctx(&self, faults: Option<&str>) -> RuntimeContext {
            let p = |name: &str| self.dir.path().join(name).to_str().unwrap().to_string();
            let mut env = MapEnv::new("/")
                .with("HORCH_STATE_DIR", &p("state"))
                .with("HORCH_DATA_DIR", &p("data"))
                .with("HORCH_LEDGER_DB", &p("ledger.sqlite3"))
                .with("FLEET_ROOT", &p("fleet"))
                .with("HORCH_PROJECT_DIR", &p(""))
                .with("HORCH_MACHINE_FILE", &p("machine.json"));
            if let Some(f) = faults {
                env = env.with("HORCH_FAULT", f);
            }
            RuntimeContext::from_env(&env).unwrap()
        }

        /// A ready shared-checkout task of `persona`, so no git tree is
        /// needed.
        fn task(&mut self, title: &str, persona: &str) -> String {
            let mut new = NewTask::work(title, &format!("tickets/{title}.md"), persona);
            new.worktree_mode = WorktreeMode::Shared;
            new.ticket_snapshot = Some(format!("# {title}\n"));
            let run = self.run.clone();
            self.store
                .immediate(|tx| tasks::add(tx, &run, &new, Actor::Orchestrator))
                .unwrap()
                .task_id
        }

        fn count(&self, sql: &str) -> i64 {
            self.store.conn().query_row(sql, [], |r| r.get(0)).unwrap()
        }

        fn status(&self, task: &str) -> String {
            tasks::get(self.store.conn(), task).unwrap().status
        }
    }

    fn args(persona: &str) -> SpawnArgs {
        SpawnArgs {
            teammate: Some(persona.into()),
            task: String::new(),
            resume: None,
            phase: None,
            effort: None,
            model: None,
            role: None,
            from_pane: None,
            direction: Direction::Right,
            tiling: TilingMode::Disabled,
            exact: false,
            force: false,
            replaces: None,
            run: Some("r1".into()),
            attempt: None,
        }
    }

    /// FM-2 and FR-5: `prepare`, which runs before the pane opens, binds
    /// the ready task to 1 push attempt, writes `task.yaml`, `START.md` and
    /// the launch record, and gives the first line that names `START.md`.
    #[test]
    fn spawn_binds_the_task_before_the_pane() {
        let mut w = world();
        let task = w.task("a", "dev");
        let l = prepare(&w.ctx(None), &args("dev"), None)
            .unwrap()
            .expect("the spawn joins run r1");

        let push = l.push.clone().expect("a push attempt");
        assert_eq!(push.task_id, task);
        assert_eq!(push.attempt_id, format!("{task}/A1"));
        assert_eq!(w.status(&task), "leased");
        let (state, via, worker): (String, String, String) = w
            .store
            .conn()
            .query_row(
                "SELECT state, via, worker_id FROM attempts WHERE id = ?1",
                [&push.attempt_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!((state.as_str(), via.as_str()), ("launching", "push"));
        assert_eq!(worker, l.worker_id);
        let dir = w.fleet_root().join("tasks").join(&task).join("attempts/A1");
        assert_eq!(push.files.task_yaml, dir.join("task.yaml"));
        assert!(push.files.task_yaml.is_file() && push.files.start_md.is_file());
        assert_eq!(
            l.first_line.as_deref(),
            Some(
                format!(
                    "Read {} and continue.",
                    dir.join("inbox/START.md").display()
                )
                .as_str()
            )
        );
        assert!(
            effects::read_launch(&w.fleet_root(), &push.attempt_id)
                .unwrap()
                .is_some(),
            "the launch record keeps the pass from a second launch"
        );
        assert_eq!(w.count("SELECT COUNT(*) FROM workers"), 1);
    }

    /// A spawn with no task to bind binds nothing and refuses before the
    /// pane: with no ready task of its persona, and with a held task, whose
    /// hold it names.
    #[test]
    fn spawn_with_no_ready_task_opens_no_pane() {
        let mut w = world();
        w.task("other", "ops");
        let err = prepare(&w.ctx(None), &args("dev"), None)
            .err()
            .expect("a refusal");
        assert_eq!(
            err.to_string(),
            "horch spawn refused: run r1 has no ready task for persona dev"
        );

        let held = w.task("later", "dev");
        w.store
            .conn()
            .execute(
                "UPDATE tasks SET not_before = 600 WHERE task_id = ?1",
                [&held],
            )
            .unwrap();
        let err = prepare(&w.ctx(None), &args("dev"), None)
            .err()
            .expect("a refusal");
        assert_eq!(
            err.to_string(),
            "horch spawn refused: no ready task for persona dev in run r1 can start now:\n  \
             - T-2: retry after 600 s"
        );
        assert_eq!(w.count("SELECT COUNT(*) FROM workers"), 0);
        assert_eq!(w.count("SELECT COUNT(*) FROM attempts"), 0);
        assert_eq!(w.status(&held), "ready");
    }

    /// The target task of a reviewer spawn goes first (L2).
    #[test]
    fn spawn_binds_its_target_task_first() {
        let mut w = world();
        w.task("older", "dev");
        let target = w.task("review", "dev");
        let ctx = w.ctx(None);
        let run = w.run.clone();
        let (_, pushed) = push(
            &ctx,
            &mut w.store,
            &args("dev"),
            &run,
            "dev",
            Some(target.clone()),
        )
        .unwrap();
        assert_eq!(pushed.task_id, target);
    }

    /// The crash test of the bind. A fault before the commit: no row, the
    /// task stays ready, no file. A fault after the commit and before the
    /// effects: the task is leased with 1 launching attempt and no files;
    /// the next pass writes the files 1 time, and a second pass changes
    /// nothing.
    #[test]
    fn a_crash_around_the_bind_commit_loses_no_task() {
        let mut w = world();
        let task = w.task("a", "dev");
        let before = prepare(&w.ctx(Some(FAULT_BEFORE_BIND_COMMIT)), &args("dev"), None);
        assert!(format!("{:#}", before.err().unwrap()).contains(FAULT_BEFORE_BIND_COMMIT));
        assert_eq!(w.status(&task), "ready");
        assert_eq!(w.count("SELECT COUNT(*) FROM attempts"), 0);
        assert_eq!(w.count("SELECT COUNT(*) FROM workers"), 0);
        assert!(!w.fleet_root().join("tasks").exists());

        let after = prepare(&w.ctx(Some(FAULT_AFTER_BIND_COMMIT)), &args("dev"), None);
        assert!(format!("{:#}", after.err().unwrap()).contains(FAULT_AFTER_BIND_COMMIT));
        assert_eq!(w.status(&task), "leased");
        assert_eq!(
            w.count("SELECT COUNT(*) FROM attempts WHERE state = 'launching' AND via = 'push'"),
            1
        );
        let yaml = w
            .fleet_root()
            .join("tasks")
            .join(&task)
            .join("attempts/A1/task.yaml");
        assert!(!yaml.exists());

        let pass = || write_missing_task_files(w.store.conn(), &w.fleet_root(), &[]).unwrap();
        let first = pass();
        assert_eq!(first.len(), 1);
        assert!(first[0].1.is_ok() && yaml.is_file());
        let bytes = std::fs::read(&yaml).unwrap();
        assert!(pass().is_empty(), "the next pass writes nothing again");
        assert_eq!(std::fs::read(&yaml).unwrap(), bytes);
        assert_eq!(w.count("SELECT COUNT(*) FROM attempts"), 1);
    }

    /// A spawn that fails before its agent runs (a refusal of the spawn
    /// gate, no herdr) gives its task back: the attempt ends
    /// `launch_failed`, the task is ready again, and no attempt is burned.
    /// The next spawn binds attempt A2.
    #[test]
    fn a_failed_spawn_gives_its_task_back() {
        let mut w = world();
        let task = w.task("a", "dev");
        let l = prepare(&w.ctx(None), &args("dev"), None).unwrap().unwrap();
        l.launch_failed("REFUSED: dev cannot start.");
        drop(l);
        assert_eq!(w.status(&task), "ready");
        let t = tasks::get(w.store.conn(), &task).unwrap();
        assert_eq!(
            (t.attempts, t.attempt_floor),
            (1, 1),
            "no attempt is burned"
        );
        assert_eq!(t.current_worker_id, None);
        assert_eq!(
            w.count(
                "SELECT COUNT(*) FROM attempts WHERE state = 'ended' AND outcome = 'launch_failed'"
            ),
            1
        );
        assert_eq!(
            w.count("SELECT COUNT(*) FROM workers WHERE status = 'launch_failed'"),
            1
        );
        let next = prepare(&w.ctx(None), &args("dev"), None).unwrap().unwrap();
        assert_eq!(next.push.unwrap().attempt_id, format!("{task}/A2"));
    }

    /// `--attempt` opens the pane of an attempt that fleetd bound: it binds
    /// nothing, and it refuses an attempt that began with a claim.
    #[test]
    fn spawn_of_a_bound_attempt_binds_nothing() {
        let mut w = world();
        let task = w.task("a", "dev");
        w.task("b", "dev");
        let first = prepare(&w.ctx(None), &args("dev"), None).unwrap().unwrap();
        let attempt = first.push.unwrap().attempt_id;
        drop(first.store);

        let mut again = args("dev");
        again.attempt = Some(attempt.clone());
        let l = prepare(&w.ctx(None), &again, None).unwrap().unwrap();
        let push = l.push.unwrap();
        assert_eq!(
            (push.task_id.as_str(), push.attempt_id.as_str()),
            (task.as_str(), attempt.as_str())
        );
        assert_eq!(
            w.count("SELECT COUNT(*) FROM attempts"),
            1,
            "no second bind"
        );
        assert_eq!(w.count("SELECT COUNT(*) FROM workers"), 1);

        w.store
            .conn()
            .execute(
                "UPDATE attempts SET via = 'claim' WHERE id = ?1",
                [&attempt],
            )
            .unwrap();
        let err = prepare(&w.ctx(None), &again, None).err().unwrap();
        assert!(err.to_string().contains("began with a claim"), "{err:#}");
    }

    /// `horch result` finds its attempt by `HORCH_TASK_YAML`. The pane of
    /// an attempt that fleetd launches with `--attempt` gets the variable
    /// from these files (`spawn::launch`, the brief of the pane): the
    /// `task.yaml` of that attempt, under the fleet root.
    #[test]
    fn spawn_of_a_bound_attempt_names_its_task_yaml() {
        let mut w = world();
        let task = w.task("a", "dev");
        let first = prepare(&w.ctx(None), &args("dev"), None).unwrap().unwrap();
        let attempt = first.push.unwrap().attempt_id;
        drop(first.store);

        let mut again = args("dev");
        again.attempt = Some(attempt.clone());
        let l = prepare(&w.ctx(None), &again, None).unwrap().unwrap();
        let push = l.push.unwrap();
        let dir = w.fleet_root().join("tasks").join(&task).join("attempts/A1");
        assert_eq!(push.files.task_yaml, dir.join("task.yaml"));
        assert!(push.files.task_yaml.is_file());
    }
}
