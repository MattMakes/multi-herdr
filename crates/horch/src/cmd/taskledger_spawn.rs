//! `horch spawn`'s task-ledger side (#34): the worker row is written before
//! the pane opens, its id travels in the brief, and the role and pane are
//! recorded once the split is done.

use std::path::Path;

use anyhow::{bail, ensure, Result};
use horch_core::execution::store::ExecutionStore;
use horch_core::runtime::RuntimeContext;
use horch_core::taskledger::model::{Actor, EventKind, RunStatus, WorkerKind};
use horch_core::taskledger::runs::{self, Run};
use horch_core::taskledger::util::{self, About};
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
/// one with `--run` (or `HORCH_RUN_ID`), it replaces a ledger worker, it
/// has no task text and the project has an active run, or a `spawn_only`
/// controller in an active run spawns a role with task text. `None`
/// otherwise: a spawn with task text runs outside every run, with 1
/// `NOTE:` (#89).
///
/// `controller` is the caller's ledger row when the caller has
/// `spawn_only` and passed its check (T-38).
pub fn prepare(
    ctx: &RuntimeContext,
    args: &SpawnArgs,
    controller: Option<&str>,
) -> Result<Option<LedgerSpawn>> {
    let mut store = taskledger::open(ctx)?;
    let role_run = controller_run(&store, args, controller)?;
    let run = match &role_run {
        Some(run) => run.clone(),
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
    let id = insert_row(ctx, &mut store, args, &run, persona)?;
    if role_run.is_some() {
        mark_no_claim(&mut store, &run, &id, controller.unwrap_or_default())?;
    }
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
