//! The handlers of the worker request kinds: `spawn`, `worker_stop`,
//! `worker_reap`, `worker_new`, `worker_pid`, `orchestrator_claim` and
//! `orchestrator_stop`.
//!
//! Requirements: RR-5, RR-8. Ticket 19 of fleetd-p5.
//!
//! Each handler writes the rows that its `horch` command writes today,
//! through the same functions of `taskledger/` and `fleetd/`, in the
//! transaction of its request. It does no effect:
//!
//! - `spawn` binds a task to a new attempt with [`dispatch::bind`]. The
//!   pass launches the attempt in `launching` (ticket 10 of fleetd-p4).
//! - `worker_stop` and `worker_reap` end the worker row. The pass stops each
//!   process of an ended row by its PID and start time, and closes its pane
//!   only with a fact that proves it (`commands::mark_orphans`).
//!
//! The payload holds the arguments of the command; the run is its id, as
//! the command resolves it. The result holds what the command prints.

use std::sync::Arc;

use anyhow::{bail, ensure, Context, Result};
use rusqlite::{Connection, Transaction};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::fleetd::fleet_runtime::adapters::clock_store;
use crate::fleetd::fleet_runtime::application::jobs::{JobCtx, NeedsYouItems};
use crate::fleetd::fleet_runtime::application::requests::Handlers;
use crate::fleetd::fleet_runtime::domain::config::FleetConfig;
use crate::fleetd::fleet_runtime::domain::ledger::Generation;
use crate::fleetd::fleet_runtime::domain::requests::Kind;
use crate::fleetd::task_execution::application::dispatch::{self, CheckSandboxes, DispatchCtx};
use crate::fleetd::task_execution::domain::dispatch::{Choice, Holds};
use crate::taskledger::git::Git;
use crate::taskledger::model::{
    Actor, EventKind, RunStatus, StopRequester, WorkerKind, WorkerStatus,
};
use crate::taskledger::orchestrator::{self, Claimed, Holder, Probe};
use crate::taskledger::runs::{self, Run};
use crate::taskledger::stop;
use crate::taskledger::tasks;
use crate::taskledger::util::{self, About};
use crate::taskledger::workers::{self, NewWorker};

/// The text that starts the event of a role that claims no task. It is the
/// prefix of the verdict of `workers::sweep_preclaim`, as `horch spawn`
/// writes it today.
const NO_CLAIM: &str = "No claim:";

/// What the handlers of this module read outside their payload.
#[derive(Clone)]
pub struct WorkerDeps {
    /// The generation of this fleetd.
    pub guard: Generation,
    pub config: Arc<FleetConfig>,
    pub needs_you: Arc<dyn NeedsYouItems + Send + Sync>,
    /// The 1 git runner of fleetd: the base tip of a new tree of a bind.
    pub git: Arc<Git>,
    /// The holds of dispatch now: the holds of the last pass.
    pub holds: Arc<dyn Fn() -> Holds + Send + Sync>,
    pub sandboxes: Arc<dyn CheckSandboxes + Send + Sync>,
    /// The live holder of the orchestrator lock of a workspace
    /// (`orchestrator::holder`).
    pub lock_holder: Arc<dyn Fn(&str) -> Option<Holder> + Send + Sync>,
}

/// Register the handler of each kind of this module in `handlers`.
pub fn register(handlers: &mut Handlers, deps: &WorkerDeps) {
    let d = deps.clone();
    handlers.insert(Kind::Spawn, move |tx, p| spawn(tx, &d, &parse(p)?));
    handlers.insert(Kind::WorkerStop, |tx, p| worker_stop(tx, &parse(p)?));
    handlers.insert(Kind::WorkerReap, |tx, p| worker_reap(tx, &parse(p)?));
    handlers.insert(Kind::WorkerNew, |tx, p| worker_new(tx, &parse(p)?));
    handlers.insert(Kind::WorkerPid, |tx, p| worker_pid(tx, &parse(p)?));
    let d = deps.clone();
    handlers.insert(Kind::OrchestratorClaim, move |tx, p| {
        orchestrator_claim(tx, &d, &parse(p)?)
    });
    handlers.insert(Kind::OrchestratorStop, |tx, p| {
        orchestrator_stop(tx, &parse(p)?)
    });
}

fn parse<T: DeserializeOwned>(payload: &Value) -> Result<T> {
    serde_json::from_value(payload.clone()).context("the payload does not fit its kind")
}

/// The active run `run_id`.
fn active_run(tx: &Connection, run_id: &str) -> Result<Run> {
    let run = runs::get(tx, run_id)?;
    ensure!(
        run.status == RunStatus::Active.as_str(),
        "run {} is {}; a closed run takes no new worker",
        run.slug,
        run.status
    );
    Ok(run)
}

// ─── spawn ──────────────────────────────────────────────────────────────

/// The arguments of `horch spawn` and `horch ledger spawn` in a ledger run.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpawnPayload {
    pub run_id: String,
    pub persona: String,
    /// The task that the spawn binds first (`HORCH_CLAIM_TARGET`).
    #[serde(default)]
    pub target_task_id: Option<String>,
    #[serde(default)]
    pub workspace_id: Option<String>,
    /// `--replaces`: the worker that this one replaces.
    #[serde(default)]
    pub replaces: Option<String>,
    /// `--resume`: the session that this worker resumes.
    #[serde(default)]
    pub resume: Option<String>,
    /// The `spawn_only` controller of a role: the role works from its brief,
    /// so the spawn binds no task.
    #[serde(default)]
    pub controller: Option<String>,
}

fn spawn(tx: &Transaction<'_>, deps: &WorkerDeps, p: &SpawnPayload) -> Result<Value> {
    let run = active_run(tx, &p.run_id)?;
    ensure_replaceable(tx, p.replaces.as_deref(), &run)?;
    if let Some(controller) = &p.controller {
        return spawn_role(tx, p, &run, controller);
    }
    let task_id = task_to_bind(tx, deps, p)?;
    let ctx = DispatchCtx {
        jobs: JobCtx {
            guard: Some(deps.guard),
            config: &deps.config,
            needs_you: deps.needs_you.as_ref(),
        },
        clock: clock_store::read(tx)?,
        git: &deps.git,
    };
    let worker = NewWorker {
        workspace_id: p.workspace_id.clone(),
        previous_worker_id: p.replaces.clone(),
        resumed_from_session: p.resume.clone(),
        ..Default::default()
    };
    let b = dispatch::bind(tx, &ctx, &task_id, &p.persona, &worker)?;
    Ok(json!({
        "run_id": run.run_id,
        "worker_id": b.worker_id,
        "task_id": b.task_id,
        "task": tasks::get(tx, &b.task_id)?.short(),
        "attempt_id": b.attempt_id,
        "attempt": b.attempt,
        "persona": b.persona,
        "worktree": b.worktree.map(|t| json!({
            "worktree_id": t.worktree_id,
            "path": t.path,
            "branch": t.branch,
            "add_job": t.add_job,
        })),
    }))
}

/// Refuse to replace a worker that is still live.
fn ensure_replaceable(tx: &Connection, replaces: Option<&str>, run: &Run) -> Result<()> {
    let Some(prev) = replaces else {
        return Ok(());
    };
    let p = workers::get(tx, prev)?;
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

/// The ready task of the run that the spawn binds: the target when the
/// dispatch rules let it bind now, else the first task that they let bind
/// for the persona. The refusal names the hold of each held task.
fn task_to_bind(tx: &Connection, deps: &WorkerDeps, p: &SpawnPayload) -> Result<String> {
    let mut holds = (deps.holds)();
    holds.clock = clock_store::read(tx)?.0;
    let choices = dispatch::choices(
        tx,
        &holds,
        deps.sandboxes.as_ref(),
        p.target_task_id.as_deref(),
    )?;
    let mut held = Vec::new();
    for choice in choices {
        if tasks::get(tx, choice_task(&choice))?.run_id != p.run_id {
            continue;
        }
        match choice {
            Choice::Bind { task_id, persona } => {
                let wanted = match &p.target_task_id {
                    Some(target) => *target == task_id,
                    None => persona == p.persona,
                };
                if wanted {
                    return Ok(task_id);
                }
            }
            Choice::Held { task_id, hold } => {
                held.push(format!("{}: {hold}", tasks::get(tx, &task_id)?.short()))
            }
        }
    }
    let what = match &p.target_task_id {
        Some(target) => format!(
            "task {} is not ready to bind",
            tasks::get(tx, target)?.short()
        ),
        None => format!("no ready task for persona {}", p.persona),
    };
    if held.is_empty() {
        bail!("horch spawn refused: {what}");
    }
    bail!(
        "horch spawn refused: {what}. Held:\n  - {}",
        held.join("\n  - ")
    )
}

fn choice_task(c: &Choice) -> &str {
    match c {
        Choice::Bind { task_id, .. } | Choice::Held { task_id, .. } => task_id,
    }
}

/// The rows of a role of a `spawn_only` controller: its worker row, and the
/// event that says it claims no task.
fn spawn_role(tx: &Connection, p: &SpawnPayload, run: &Run, controller: &str) -> Result<Value> {
    let id = workers::insert(
        tx,
        &NewWorker {
            run_id: run.run_id.clone(),
            kind: Some(WorkerKind::Worker),
            persona: p.persona.clone(),
            workspace_id: p.workspace_id.clone(),
            previous_worker_id: p.replaces.clone(),
            resumed_from_session: p.resume.clone(),
            target_task_id: p.target_task_id.clone(),
            ..Default::default()
        },
    )?;
    util::event(
        tx,
        &About {
            run: Some(&run.run_id),
            task: None,
            worker: Some(&id),
        },
        Actor::Horch,
        EventKind::Verdict,
        &format!(
            "{NO_CLAIM} a role of controller worker {controller}; it works from its brief and claims no task."
        ),
    )?;
    Ok(json!({ "run_id": run.run_id, "worker_id": id }))
}

// ─── worker_stop, worker_reap ───────────────────────────────────────────

/// The arguments of `horch ledger stop` and `horch ledger reap`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StopPayload {
    pub run_id: String,
    /// A worker id or role.
    pub worker: String,
    pub reason: String,
}

/// Record the stop of 1 worker: its row ends `stopped`, and the task it
/// held is queued again. The pass stops its processes.
fn worker_stop(tx: &Transaction<'_>, p: &StopPayload) -> Result<Value> {
    let w = workers::resolve(tx, &p.run_id, &p.worker)?;
    let by = StopRequester::Orchestrator;
    stop::check_stoppable(tx, &w, by, &p.reason)?;
    let requeued = stop::end_row_in(tx, &w, by, &p.reason, WorkerStatus::Stopped)?;
    Ok(json!({
        "worker_id": w.worker_id,
        "worker": w.label(),
        "requeued": requeued,
    }))
}

/// Record the reap of a dead worker: its row ends `incomplete`, the task it
/// held is queued again, and 1 `reaped` event. The pass stops its
/// processes.
fn worker_reap(tx: &Transaction<'_>, p: &StopPayload) -> Result<Value> {
    let w = workers::resolve(tx, &p.run_id, &p.worker)?;
    let by = StopRequester::Orchestrator;
    stop::check_reapable(&w)?;
    let dead = format!("dead: {}", p.reason);
    stop::check_stoppable(tx, &w, by, &dead)?;
    let requeued = stop::end_row_in(tx, &w, by, &dead, WorkerStatus::Incomplete)?;
    let persona = match &w.task_id {
        Some(t) => {
            stop::requeue_if_held(tx, t, &w.worker_id, &p.reason)?
                .0
                .claimable_by
        }
        None => w.persona.clone(),
    };
    util::event(
        tx,
        &About {
            run: w.run_id.as_deref(),
            task: w.task_id.as_deref(),
            worker: Some(&w.worker_id),
        },
        stop::reap_actor(by),
        EventKind::Reaped,
        &format!("{} reaped: {}", w.label(), p.reason),
    )?;
    Ok(json!({
        "worker_id": w.worker_id,
        "worker": w.label(),
        "requeued": requeued,
        "persona": persona,
    }))
}

// ─── worker_new, worker_pid ─────────────────────────────────────────────

/// The arguments of `horch ledger worker new`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerNewPayload {
    pub run_id: String,
    pub persona: String,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub replaces: Option<String>,
    #[serde(default)]
    pub pane: Option<String>,
}

fn worker_new(tx: &Transaction<'_>, p: &WorkerNewPayload) -> Result<Value> {
    let id = workers::insert(
        tx,
        &NewWorker {
            run_id: runs::get(tx, &p.run_id)?.run_id,
            kind: Some(WorkerKind::Worker),
            persona: p.persona.clone(),
            role: p.role.clone(),
            previous_worker_id: p.replaces.clone(),
            pane_id: p.pane.clone(),
            ..Default::default()
        },
    )?;
    Ok(json!({ "worker_id": id }))
}

/// 1 process of `horch ledger worker pid`: the command reads the start time
/// of its PID, as it does today.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pid {
    pub pid: u32,
    #[serde(default)]
    pub started: Option<u64>,
}

/// The arguments of `horch ledger worker pid`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerPidPayload {
    pub worker: String,
    #[serde(default)]
    pub supervisor: Option<Pid>,
    #[serde(default)]
    pub agent: Option<Pid>,
}

fn worker_pid(tx: &Transaction<'_>, p: &WorkerPidPayload) -> Result<Value> {
    if let Some(s) = &p.supervisor {
        workers::supervisor_started(tx, &p.worker, s.pid, s.started)?;
    }
    if let Some(a) = &p.agent {
        workers::agent_started(tx, &p.worker, a.pid, a.started)?;
    }
    Ok(json!({ "worker_id": p.worker }))
}

// ─── orchestrator_claim, orchestrator_stop ──────────────────────────────

/// The orchestrator claim of `horch fleet start`, and the harness that it
/// writes on the orchestrator rows of the workspace.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OrchestratorClaimPayload {
    pub run_id: String,
    pub workspace: String,
    #[serde(default)]
    pub pane: Option<String>,
    /// The agent of the orchestrator teammate (TRF-44).
    #[serde(default)]
    pub harness: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
}

/// The lock facts of [`WorkerDeps`] as the [`Probe`] of a claim.
struct LockProbe<'a>(&'a (dyn Fn(&str) -> Option<Holder> + Send + Sync));

impl Probe for LockProbe<'_> {
    fn holder(&self, workspace: &str) -> Option<Holder> {
        (self.0)(workspace)
    }

    fn alive(&self, pid: u32, started: Option<u64>) -> bool {
        crate::procid::alive(pid, started)
    }

    /// As `horch fleet start` today: herdr cannot say.
    fn workspace_open(&self, _workspace: &str) -> Option<bool> {
        None
    }
}

fn orchestrator_claim(
    tx: &Transaction<'_>,
    deps: &WorkerDeps,
    p: &OrchestratorClaimPayload,
) -> Result<Value> {
    let probe = LockProbe(deps.lock_holder.as_ref());
    let claimed = orchestrator::claim(tx, &p.run_id, &p.workspace, p.pane.as_deref(), &probe)?;
    let harness_rows = match &p.harness {
        Some(h) => orchestrator::record_harness(tx, &p.workspace, h, p.model.as_deref())?,
        None => 0,
    };
    let claim = match claimed {
        Claimed::Unchanged => json!({ "state": "unchanged" }),
        Claimed::Changed(text) => json!({ "state": "changed", "text": text }),
        Claimed::Skipped {
            text,
            workspace,
            pid,
        } => json!({ "state": "skipped", "text": text, "workspace": workspace, "pid": pid }),
    };
    Ok(json!({ "claim": claim, "harness_rows": harness_rows }))
}

/// The runs of `horch fleet stop`: the active runs of the project.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OrchestratorStopPayload {
    pub run_ids: Vec<String>,
}

fn orchestrator_stop(tx: &Transaction<'_>, p: &OrchestratorStopPayload) -> Result<Value> {
    let mut out = Vec::new();
    for id in &p.run_ids {
        let run = runs::get(tx, id)?;
        let recorded = orchestrator::record_stop(tx, &run.run_id)?;
        out.push(json!({ "run_id": run.run_id, "slug": run.slug, "recorded": recorded }));
    }
    Ok(json!({ "runs": out }))
}
