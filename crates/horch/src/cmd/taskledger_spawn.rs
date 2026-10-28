//! `horch spawn`'s task-ledger side (#34): the worker row is written before
//! the pane opens, its id travels in the brief, and the role and pane are
//! recorded once the split is done.

use anyhow::{bail, Result};
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

/// Write the worker row when the project has an active run. `None` when it
/// has none: the spawn is not part of a ledger run.
pub fn prepare(ctx: &RuntimeContext, args: &SpawnArgs) -> Result<Option<LedgerSpawn>> {
    let mut store = taskledger::open(ctx)?;
    let root = project_root(ctx)?;
    let explicit = std::env::var("HORCH_RUN_ID").ok().filter(|v| !v.is_empty());
    let active: Vec<Run> = runs::of_project(store.conn(), &root)?
        .into_iter()
        .filter(|r| r.status == RunStatus::Active.as_str())
        .collect();
    if explicit.is_none() && active.is_empty() {
        if args.replaces.is_some() {
            bail!(
                "--replaces needs an active task-ledger run in {}",
                root.display()
            );
        }
        return Ok(None);
    }
    let run = super::taskledger::resolve_run(
        ctx,
        &store,
        &RunArg {
            run: explicit.clone(),
        },
    )?;
    let persona = match (&args.teammate, &args.resume) {
        (Some(t), _) => t.clone(),
        (None, Some(key)) => ExecutionStore::open_in(ctx)?.get(key)?.tier,
        (None, None) => bail!("give a teammate or --resume <id>"),
    };
    if let Some(prev) = &args.replaces {
        let p = workers::get(store.conn(), prev)?;
        if p.status()?.is_live() {
            bail!(
                "worker {} is still {}; reap or stop it before you replace it",
                p.label(),
                p.status
            );
        }
    }
    let id = store.immediate(|tx| {
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
    })?;
    Ok(Some(LedgerSpawn {
        store: std::cell::RefCell::new(store),
        run,
        worker_id: id,
    }))
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
