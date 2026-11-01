//! `horch spawn`'s task-ledger side (#34): the worker row is written before
//! the pane opens, its id travels in the brief, and the role and pane are
//! recorded once the split is done.

use std::path::Path;

use anyhow::{bail, ensure, Result};
use horch_core::execution::store::ExecutionStore;
use horch_core::runtime::RuntimeContext;
use horch_core::taskledger::model::{RunStatus, WorkerKind};
use horch_core::taskledger::runs::{self, Run};
use horch_core::taskledger::workers::{self, NewWorker};
use horch_core::taskledger::{self, Store};

use super::spawn::SpawnArgs;
use super::taskledger::{project_root, RunArg};

/// The fixed task of a worker spawned with no task text.
pub const CLAIM_TASK: &str = "Run horch ledger claim. Read the ticket file it prints and follow it exactly. If it prints that there is no task for you, send the line it prints, then end your turn.";

/// A spawn's worker row.
pub struct LedgerSpawn {
    store: std::cell::RefCell<Store>,
    pub run: Run,
    pub worker_id: String,
}

/// Write the worker row when this spawn joins a task-ledger run: it names
/// one with `--run` (or `HORCH_RUN_ID`), it replaces a ledger worker, or it
/// has no task text and the project has an active run. `None` otherwise: a
/// spawn with task text runs outside every run, with 1 `NOTE:` (#89).
pub fn prepare(ctx: &RuntimeContext, args: &SpawnArgs) -> Result<Option<LedgerSpawn>> {
    let mut store = taskledger::open(ctx)?;
    let Some(explicit) = ledger_run_wanted(ctx, &store, args)? else {
        return Ok(None);
    };
    let run = super::taskledger::resolve_run(ctx, &store, &RunArg { run: explicit })?;
    ensure_active(&run)?;
    let persona = spawn_persona(ctx, args)?;
    ensure_replaceable(&store, args, &run)?;
    let id = insert_row(ctx, &mut store, args, &run, persona)?;
    Ok(Some(LedgerSpawn {
        store: std::cell::RefCell::new(store),
        run,
        worker_id: id,
    }))
}

/// Whether this spawn joins a task-ledger run. `None`: it does not. `Some`
/// holds the run it names (`--run` or `HORCH_RUN_ID`), or `None` for the
/// project's 1 active run. A spawn with task text names no run and says so.
fn ledger_run_wanted(
    ctx: &RuntimeContext,
    store: &Store,
    args: &SpawnArgs,
) -> Result<Option<Option<String>>> {
    let root = project_root(ctx)?;
    let explicit = explicit_run(args);
    let active = active_runs(store, &root)?;
    let joins = explicit.is_some() || args.replaces.is_some() || args.task.trim().is_empty();
    if !joins {
        note_outside_run(&active);
        return Ok(None);
    }
    if explicit.is_none() && active.is_empty() {
        ensure!(
            args.replaces.is_none(),
            "--replaces needs an active task-ledger run in {}",
            root.display()
        );
        return Ok(None);
    }
    Ok(Some(explicit))
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
                ..Default::default()
            },
        )
    })
}

/// Say that a spawn with task text runs outside the active runs.
fn note_outside_run(active: &[Run]) {
    if active.is_empty() {
        return;
    }
    let slugs: Vec<&str> = active.iter().map(|r| r.slug.as_str()).collect();
    eprintln!(
        "horch spawn: NOTE: this spawn has task text, so it runs outside task-ledger run {}. Give --run <slug> to join a run.",
        slugs.join(", ")
    );
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

    /// The spawn failed before the worker ran.
    pub fn launch_failed(&self, reason: &str) {
        let r = self
            .store
            .borrow_mut()
            .immediate(|tx| workers::launch_failed(tx, &self.worker_id, reason));
        if let Err(e) = r {
            eprintln!("horch spawn: the task ledger did not record the failed start: {e:#}");
        }
    }
}
