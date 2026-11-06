//! The orchestrator's respawn, rarely and under guards (trace-fixes C7, D6).
//!
//! - The fleet orchestrator's pane launcher holds 1 lock per herdr workspace
//!   ([`lock`]) while its agent runs. So 2 orchestrators never run in 1
//!   workspace. The launcher waits for its agent, so the lock lives exactly
//!   as long as the agent.
//! - Each watchdog pass runs [`check`]. A live lock holder is the agent PID
//!   of the run's registered orchestrator: [`check`] records it on the
//!   worker row. A recorded PID that is gone, with no live lock holder, is a
//!   confirmed death.
//! - A confirmed death in an active run with no deliberate stop
//!   ([`record_stop`]) respawns the orchestrator: at most 1 per
//!   [`RESPAWN_EVERY_S`] and [`RESPAWNS_PER_RUN`] per run. The old row ends
//!   handed off to [`HANDOFF_FILE`] (the C4 path), and a new orchestrator row
//!   takes its place. The caller starts the agent with [`first_instruction`].
//! - The new row is launch-pending (`spawning`) until a pass observes the
//!   lock holder: then it is `working`. A pending row with no lock holder
//!   did not start (a crash before the launch, or a failed launch). Each
//!   pass launches it again inside the same guards, and each launch counts 1
//!   respawn (TRF-38).

use std::path::{Path, PathBuf};

use anyhow::Result;
use rusqlite::{params, Connection, Transaction};
use serde::Serialize;

use super::model::{Actor, EventKind, OutboxEvent, RunStatus, WorkerKind, WorkerStatus};
use super::outbox::{self, Wake};
use super::runs;
use super::sql_words::word;
use super::util::{self, About};
use super::workers::{self, NewWorker, Worker};

/// At most 1 respawn in this many seconds (spec section 5).
pub const RESPAWN_EVERY_S: i64 = 600;

/// At most this many respawns in 1 run (spec section 5).
pub const RESPAWNS_PER_RUN: i64 = 3;

/// The handoff file a respawned orchestrator reads first (C7).
pub const HANDOFF_FILE: &str = "ai_docs/handoffs/orchestrator-whats-next.md";

/// The environment variable that tells a fleet orchestrator pane it is a
/// respawn: `<run slug> <respawn number>`.
pub const RESPAWN_ENV: &str = "HORCH_ORCHESTRATOR_RESPAWN";

/// The text every respawn event starts with, so status and history find it.
pub const RESPAWNED: &str = "orchestrator-respawned";

/// The decisions file of run `slug` (C7).
pub fn decisions_file(slug: &str) -> String {
    format!("ai_docs/plans/{slug}/decisions.md")
}

/// The first instruction of a respawned orchestrator (C7): the handoff, the
/// decisions and the run status, in that order.
pub fn first_instruction(slug: &str, number: i64) -> String {
    format!(
        "== Respawn ==\n\
         horch respawned this orchestrator because the agent of the earlier one exited. \
         This is respawn {number} of {RESPAWNS_PER_RUN} in run {slug}. \
         Before you do other work, do these 3 steps in order:\n\
         1. Read {HANDOFF_FILE}.\n\
         2. Read {decisions}.\n\
         3. Run horch ledger status, and continue run {slug} from it.\n\
         A file that does not exist gives no information. Do not stop because of it.",
        decisions = decisions_file(slug),
    )
}

/// Read [`RESPAWN_ENV`]: the run slug and the respawn number.
pub fn parse_respawn_env(value: &str) -> Option<(String, i64)> {
    let (slug, number) = value.trim().rsplit_once(' ')?;
    let number = number.parse().ok()?;
    (!slug.is_empty()).then(|| (slug.to_string(), number))
}

/// The respawn guard of 1 run, from its `runs` row.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Guard {
    /// When `horch fleet stop` or `run close` stopped the orchestrator.
    pub stopped_at: Option<String>,
    pub respawns: i64,
    pub last_respawn_at: Option<String>,
}

/// The respawn guard of `run_id`.
pub fn guard(conn: &Connection, run_id: &str) -> Result<Guard> {
    Ok(conn.query_row(
        "SELECT orchestrator_stopped_at, orchestrator_respawns, last_respawn_at
         FROM runs WHERE run_id = ?1",
        [run_id],
        |r| {
            Ok(Guard {
                stopped_at: r.get(0)?,
                respawns: r.get(1)?,
                last_respawn_at: r.get(2)?,
            })
        },
    )?)
}

/// Record a deliberate stop of the orchestrator of `run_id`: no respawn
/// follows. The first stop time stays. True when this call recorded it.
pub fn record_stop(conn: &Connection, run_id: &str) -> Result<bool> {
    let n = conn.execute(
        "UPDATE runs SET orchestrator_stopped_at = ?2
         WHERE run_id = ?1 AND orchestrator_stopped_at IS NULL",
        params![run_id, util::now()],
    )?;
    Ok(n == 1)
}

/// The run that an orchestrator launched in `workspace` of project `root`
/// continues (TRF-38): the active run of the respawn slug `respawn`, else the
/// 1 active non-solo run of `workspace`, else the 1 active non-solo run of
/// the project. `None` when 0 or 2+ runs fit.
pub fn launch_run(
    conn: &Connection,
    root: &Path,
    workspace: &str,
    respawn: Option<&str>,
) -> Result<Option<runs::Run>> {
    let active: Vec<runs::Run> = runs::of_project(conn, root)?
        .into_iter()
        .filter(|r| r.status == RunStatus::Active.as_str())
        .collect();
    if let Some(slug) = respawn {
        return Ok(active.into_iter().find(|r| r.slug == slug));
    }
    let planned: Vec<runs::Run> = active.into_iter().filter(|r| !runs::is_solo(r)).collect();
    let own: Vec<&runs::Run> = planned
        .iter()
        .filter(|r| r.workspace_id.as_deref() == Some(workspace))
        .collect();
    Ok(match (own.as_slice(), planned.as_slice()) {
        ([one], _) => Some((*one).clone()),
        ([], [one]) => Some(one.clone()),
        _ => None,
    })
}

/// What 1 orchestrator launch did to its run ([`claim`]).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum Claimed {
    /// The run is not active, or it already had this workspace and no stop.
    Unchanged,
    /// The workspace or the stop changed: the text of the 1 `note` event.
    Changed(String),
    /// The run's orchestrator still runs in the run's workspace: the run
    /// stays there. The text says why.
    Skipped(String),
}

/// An orchestrator took the pane-launch lock of `workspace` in `pane` for
/// `run_id` (TRF-38). In `tx`: the run moves to `workspace`, its stop is
/// cleared, and the run's orchestrator row is a live row of this pane.
/// 1 `note` event names the old and new values when the workspace or the
/// stop changes. A run never moves to a workspace with another active
/// non-solo run, and never away from a workspace whose orchestrator lives.
pub fn claim(
    tx: &Transaction,
    run_id: &str,
    workspace: &str,
    pane: Option<&str>,
    probe: &dyn Probe,
) -> Result<Claimed> {
    let run = runs::get(tx, run_id)?;
    if run.status != RunStatus::Active.as_str() {
        return Ok(Claimed::Unchanged);
    }
    let old_ws = run.workspace_id.as_deref().filter(|w| *w != workspace);
    if let Some(old) = old_ws {
        if let Some(h) = probe.holder(old) {
            return Ok(Claimed::Skipped(format!(
                "Run {} stays in workspace {old}: its orchestrator runs there (PID {}).",
                run.slug, h.pid
            )));
        }
        refuse_a_taken_workspace(tx, &run, workspace)?;
    }
    let stopped_at = guard(tx, run_id)?.stopped_at;
    tx.execute(
        "UPDATE runs SET workspace_id = ?2, orchestrator_stopped_at = NULL WHERE run_id = ?1",
        params![run_id, workspace],
    )?;
    let orch = live_orchestrator(tx, &run, workspace, pane)?;
    let mut changes = Vec::new();
    if run.workspace_id.as_deref() != Some(workspace) {
        changes.push(format!(
            "workspace {} -> {workspace}",
            run.workspace_id.as_deref().unwrap_or("(none)")
        ));
    }
    if let Some(at) = &stopped_at {
        changes.push(format!("orchestrator stop {at} -> cleared"));
    }
    if changes.is_empty() {
        return Ok(Claimed::Unchanged);
    }
    let text = format!(
        "An orchestrator launch claims run {}: {}.",
        run.slug,
        changes.join("; ")
    );
    util::event(
        tx,
        &About {
            run: Some(run_id),
            worker: Some(&orch),
            ..Default::default()
        },
        Actor::Horch,
        EventKind::Note,
        &text,
    )?;
    Ok(Claimed::Changed(text))
}

/// Fail when another active non-solo run of the project of `run` is in
/// `workspace` (D13: 1 run per workspace).
fn refuse_a_taken_workspace(tx: &Transaction, run: &runs::Run, workspace: &str) -> Result<()> {
    let other = runs::of_project(tx, Path::new(&run.project_root))?
        .into_iter()
        .find(|r| {
            r.run_id != run.run_id
                && r.status == RunStatus::Active.as_str()
                && !runs::is_solo(r)
                && r.workspace_id.as_deref() == Some(workspace)
        });
    match other {
        Some(o) => anyhow::bail!(
            "run {} cannot move to workspace {workspace}: run {} is active there",
            run.slug,
            o.slug
        ),
        None => Ok(()),
    }
}

/// The run's orchestrator row as a live row of `workspace` and `pane`: the
/// registered row when it is live, else a new row that follows it. The
/// watchdog then registers this launcher's PID on it.
fn live_orchestrator(
    tx: &Transaction,
    run: &runs::Run,
    workspace: &str,
    pane: Option<&str>,
) -> Result<String> {
    let current = run
        .orchestrator_worker_id
        .as_deref()
        .map(|id| workers::find(tx, id))
        .transpose()?
        .flatten();
    if let Some(orch) = current
        .as_ref()
        .filter(|o| o.status().is_ok_and(|s| s.is_live()))
    {
        tx.execute(
            "UPDATE workers SET workspace_id = ?2, pane_id = coalesce(?3, pane_id)
             WHERE worker_id = ?1",
            params![orch.worker_id, workspace, pane],
        )?;
        return Ok(orch.worker_id.clone());
    }
    let id = workers::insert(
        tx,
        &NewWorker {
            run_id: run.run_id.clone(),
            kind: Some(WorkerKind::Orchestrator),
            persona: "orchestrator".into(),
            role: Some("orchestrator".into()),
            workspace_id: Some(workspace.to_string()),
            pane_id: pane.map(str::to_string),
            previous_worker_id: current.map(|o| o.worker_id),
            ..Default::default()
        },
    )?;
    tx.execute(
        &format!(
            "UPDATE workers SET status = {working}, started_at = spawned_at WHERE worker_id = ?1",
            working = word(WorkerStatus::Working)
        ),
        [&id],
    )?;
    runs::set_orchestrator(tx, &run.run_id, &id)?;
    Ok(id)
}

/// The folder of the workspace locks: beside the ledger, as the watchdog's.
pub fn lock_dir(ledger_db: &Path) -> PathBuf {
    ledger_db
        .parent()
        .map(|p| p.join("locks"))
        .unwrap_or_else(|| PathBuf::from("locks"))
}

/// The lock name of `workspace`, safe as a file name.
fn lock_name(workspace: &str) -> String {
    let safe: String = workspace
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    format!("orchestrator-{safe}")
}

/// Take the orchestrator lock of `workspace`. `None` while a live process
/// holds it. A dead holder's lock is broken. The guard releases on drop.
pub fn lock(dir: &Path, workspace: &str) -> Result<Option<crate::fsx::DirLockGuard>> {
    match crate::fsx::DirLock::acquire(
        dir,
        &lock_name(workspace),
        std::time::Duration::from_secs(365 * 24 * 3600),
        std::time::Duration::ZERO,
    ) {
        Ok(g) => Ok(Some(g)),
        Err(crate::fsx::FsxError::LockTimeout { .. }) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// The live process that holds the orchestrator lock of a workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Holder {
    pub pid: u32,
    pub started: Option<u64>,
}

/// The live holder of the orchestrator lock of `workspace`, if any.
pub fn holder(dir: &Path, workspace: &str) -> Option<Holder> {
    let owner = dir
        .join(format!("{}.lock", lock_name(workspace)))
        .join("owner");
    let v: serde_json::Value = serde_json::from_slice(&std::fs::read(owner).ok()?).ok()?;
    let pid = u32::try_from(v["pid"].as_u64()?).ok()?;
    let started = v["started"].as_u64();
    crate::procid::alive(pid, started).then_some(Holder { pid, started })
}

/// What [`check`] reads outside the ledger.
pub trait Probe {
    /// The live holder of the orchestrator lock of `workspace`.
    fn holder(&self, workspace: &str) -> Option<Holder>;
    /// Whether `pid` runs with start time `started`.
    fn alive(&self, pid: u32, started: Option<u64>) -> bool;
    /// Whether herdr still has `workspace`. `None` when herdr cannot say.
    fn workspace_open(&self, workspace: &str) -> Option<bool>;
}

/// The probe of a real machine; `herdr_open` answers for the workspace.
pub struct Os<'a> {
    pub lock_dir: &'a Path,
    pub herdr_open: &'a dyn Fn(&str) -> Option<bool>,
}

impl Probe for Os<'_> {
    fn holder(&self, workspace: &str) -> Option<Holder> {
        holder(self.lock_dir, workspace)
    }

    fn alive(&self, pid: u32, started: Option<u64>) -> bool {
        match started {
            Some(s) => crate::procid::is_same(pid, s),
            None => crate::procid::alive(pid, None),
        }
    }

    fn workspace_open(&self, workspace: &str) -> Option<bool> {
        (self.herdr_open)(workspace)
    }
}

/// 1 respawn that [`check`] decided: the caller starts the agent.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Respawn {
    pub run_id: String,
    pub slug: String,
    pub project_root: String,
    pub workspace_id: String,
    /// The pane of the dead orchestrator, which may still exist.
    pub old_pane_id: Option<String>,
    pub old_worker_id: String,
    pub new_worker_id: String,
    /// 1 for the first respawn of the run.
    pub number: i64,
    /// The finish steps of the run, for the briefing's profile.
    pub finish_steps: Vec<String>,
}

impl Respawn {
    /// The value of [`RESPAWN_ENV`] for this respawn.
    pub fn env_value(&self) -> String {
        format!("{} {}", self.slug, self.number)
    }
}

/// What 1 [`check`] did.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum Outcome {
    /// Nothing to do: the run is not active, the stop is recorded, the
    /// orchestrator lives, ended, or never had a PID recorded.
    Nothing,
    /// The live lock holder's PID is now the orchestrator's agent PID.
    Registered(u32),
    /// The orchestrator is dead; the next respawn is allowed in this many s.
    Waiting(i64),
    /// The orchestrator is dead and ended with no respawn, for this reason.
    NotRespawned(String),
    /// The orchestrator is dead and a new row takes its place.
    Respawned(Respawn),
}

/// The result of 1 [`check`]: the outcome and the new outbox row, if any.
#[derive(Debug, Clone, PartialEq)]
pub struct Checked {
    pub outcome: Outcome,
    pub wake_id: Option<String>,
}

/// 1 watchdog pass over the orchestrator of `run_id` at `now`, in `tx`.
pub fn check(tx: &Transaction, run_id: &str, now: &str, probe: &dyn Probe) -> Result<Checked> {
    let nothing = || {
        Ok(Checked {
            outcome: Outcome::Nothing,
            wake_id: None,
        })
    };
    let run = runs::get(tx, run_id)?;
    if run.status != RunStatus::Active.as_str() || guard(tx, run_id)?.stopped_at.is_some() {
        return nothing();
    }
    let Some(orch) = run
        .orchestrator_worker_id
        .as_deref()
        .map(|id| workers::find(tx, id))
        .transpose()?
        .flatten()
    else {
        return nothing();
    };
    let Some(workspace) = orch.workspace_id.clone().or(run.workspace_id.clone()) else {
        return nothing();
    };
    let status = orch.status()?;
    if !status.is_live() {
        return nothing();
    }
    if let Some(h) = probe.holder(&workspace) {
        return register(tx, &orch, h, now);
    }
    if status == WorkerStatus::Spawning {
        return pending(tx, &run, &orch, &workspace, now, probe);
    }
    let Some(pid) = orch.agent_pid else {
        // Never registered: a death cannot be confirmed.
        return nothing();
    };
    if probe.alive(pid as u32, orch.agent_started.map(|s| s as u64)) {
        return nothing();
    }
    match probe.workspace_open(&workspace) {
        None => nothing(),
        Some(false) => end_without_respawn(
            tx,
            &run,
            &orch,
            &format!("the orchestrator agent PID {pid} exited"),
            &format!("its workspace {workspace} is gone"),
        ),
        Some(true) => dead(tx, &run, &orch, &workspace, pid, now),
    }
}

/// Record the live lock holder as the orchestrator's agent, when it is not
/// already recorded. A launch-pending row becomes `working` here, and only
/// here.
fn register(tx: &Transaction, orch: &Worker, h: Holder, now: &str) -> Result<Checked> {
    let same =
        orch.agent_pid == Some(h.pid as i64) && orch.agent_started == h.started.map(|s| s as i64);
    let pending = orch.status == WorkerStatus::Spawning.as_str();
    if same && !pending {
        return Ok(Checked {
            outcome: Outcome::Nothing,
            wake_id: None,
        });
    }
    tx.execute(
        &format!(
            "UPDATE workers SET agent_pid = ?2, agent_started = ?3, status = {working},
                started_at = coalesce(started_at, ?4)
             WHERE worker_id = ?1",
            working = word(WorkerStatus::Working)
        ),
        params![
            orch.worker_id,
            h.pid as i64,
            h.started.map(|s| s as i64),
            now
        ],
    )?;
    Ok(Checked {
        outcome: Outcome::Registered(h.pid),
        wake_id: None,
    })
}

/// A confirmed death: respawn inside the limits, wait for the 10 min, or
/// end with no respawn after the last one.
fn dead(
    tx: &Transaction,
    run: &runs::Run,
    orch: &Worker,
    workspace: &str,
    pid: i64,
    now: &str,
) -> Result<Checked> {
    let number = match allowed(tx, &run.run_id, now)? {
        Allowed::Next(n) => n,
        Allowed::Wait(left) => {
            return Ok(Checked {
                outcome: Outcome::Waiting(left),
                wake_id: None,
            })
        }
        Allowed::Spent => {
            return end_without_respawn(
                tx,
                run,
                orch,
                &format!("the orchestrator agent PID {pid} exited"),
                &format!("the run used its {RESPAWNS_PER_RUN} respawns"),
            )
        }
    };
    let handoff = Path::new(&run.project_root).join(HANDOFF_FILE);
    workers::hand_off(tx, &orch.worker_id, &handoff.to_string_lossy())?;
    let new_id = workers::insert(
        tx,
        &NewWorker {
            run_id: run.run_id.clone(),
            kind: Some(WorkerKind::Orchestrator),
            persona: "orchestrator".into(),
            role: Some("orchestrator".into()),
            workspace_id: Some(workspace.to_string()),
            previous_worker_id: Some(orch.worker_id.clone()),
            ..Default::default()
        },
    )?;
    // `spawning` is launch-pending: it is `working` after a pass observes
    // the lock holder ([`register`]).
    tx.execute(
        &format!(
            "UPDATE workers SET status = {spawning} WHERE worker_id = ?1",
            spawning = word(WorkerStatus::Spawning)
        ),
        [&new_id],
    )?;
    runs::set_orchestrator(tx, &run.run_id, &new_id)?;
    count_respawn(tx, &run.run_id, number, now)?;
    let text = format!(
        "{RESPAWNED}: respawn {number} of {RESPAWNS_PER_RUN}. The agent PID {pid} of orchestrator \
         {old} exited. Orchestrator {new_id} starts in workspace {workspace} and reads \
         {HANDOFF_FILE}, {decisions} and horch ledger status first.",
        old = orch.worker_id,
        decisions = decisions_file(&run.slug),
    );
    util::event(
        tx,
        &About {
            run: Some(&run.run_id),
            worker: Some(&new_id),
            ..Default::default()
        },
        Actor::Watchdog,
        EventKind::Note,
        &text,
    )?;
    let wake_id = outbox::write(
        tx,
        &run.run_id,
        &Wake::new(
            OutboxEvent::OrchestratorRespawned,
            new_id.clone(),
            format!("NOTE: {text}"),
        ),
    )?;
    Ok(Checked {
        outcome: Outcome::Respawned(Respawn {
            run_id: run.run_id.clone(),
            slug: run.slug.clone(),
            project_root: run.project_root.clone(),
            workspace_id: workspace.to_string(),
            old_pane_id: orch.pane_id.clone(),
            old_worker_id: orch.worker_id.clone(),
            new_worker_id: new_id,
            number,
            finish_steps: run.finish_steps.clone(),
        }),
        wake_id,
    })
}

/// Whether the guards allow the next respawn of `run_id` at `now`.
enum Allowed {
    /// Yes: the number of the next respawn.
    Next(i64),
    /// Not yet: the seconds until the next respawn is allowed.
    Wait(i64),
    /// No: the run used its [`RESPAWNS_PER_RUN`].
    Spent,
}

fn allowed(tx: &Transaction, run_id: &str, now: &str) -> Result<Allowed> {
    let g = guard(tx, run_id)?;
    if g.respawns >= RESPAWNS_PER_RUN {
        return Ok(Allowed::Spent);
    }
    let since = g
        .last_respawn_at
        .as_deref()
        .and_then(|t| util::seconds_between(t, now));
    Ok(match since {
        Some(since) if since < RESPAWN_EVERY_S => Allowed::Wait(RESPAWN_EVERY_S - since),
        _ => Allowed::Next(g.respawns + 1),
    })
}

/// Count respawn `number` of `run_id` at `now`.
fn count_respawn(tx: &Transaction, run_id: &str, number: i64, now: &str) -> Result<()> {
    tx.execute(
        "UPDATE runs SET orchestrator_respawns = ?2, last_respawn_at = ?3 WHERE run_id = ?1",
        params![run_id, number, now],
    )?;
    Ok(())
}

/// A launch-pending orchestrator with no lock holder: its launch did not
/// happen or did not start an agent (C-01). Launch it again inside the
/// guards; each launch counts 1 respawn. After the last respawn it ends
/// `launch_failed`. A gone workspace ends it; an unknown one waits.
fn pending(
    tx: &Transaction,
    run: &runs::Run,
    orch: &Worker,
    workspace: &str,
    now: &str,
    probe: &dyn Probe,
) -> Result<Checked> {
    let what = "the orchestrator respawn did not start";
    match probe.workspace_open(workspace) {
        None => {
            return Ok(Checked {
                outcome: Outcome::Nothing,
                wake_id: None,
            })
        }
        Some(false) => {
            return end_without_respawn(
                tx,
                run,
                orch,
                what,
                &format!("its workspace {workspace} is gone"),
            )
        }
        Some(true) => {}
    }
    let number = match allowed(tx, &run.run_id, now)? {
        Allowed::Next(n) => n,
        Allowed::Wait(left) => {
            return Ok(Checked {
                outcome: Outcome::Waiting(left),
                wake_id: None,
            })
        }
        Allowed::Spent => {
            return end_without_respawn(
                tx,
                run,
                orch,
                what,
                &format!("the run used its {RESPAWNS_PER_RUN} respawns"),
            )
        }
    };
    count_respawn(tx, &run.run_id, number, now)?;
    let previous = orch
        .previous_worker_id
        .as_deref()
        .map(|id| workers::find(tx, id))
        .transpose()?
        .flatten();
    let text = format!(
        "{RESPAWNED}: respawn {number} of {RESPAWNS_PER_RUN}. Orchestrator {id} did not start          after the last launch. horch launches it again in workspace {workspace}.",
        id = orch.worker_id,
    );
    util::event(
        tx,
        &About {
            run: Some(&run.run_id),
            worker: Some(&orch.worker_id),
            ..Default::default()
        },
        Actor::Watchdog,
        EventKind::Note,
        &text,
    )?;
    Ok(Checked {
        outcome: Outcome::Respawned(Respawn {
            run_id: run.run_id.clone(),
            slug: run.slug.clone(),
            project_root: run.project_root.clone(),
            workspace_id: workspace.to_string(),
            old_pane_id: orch
                .pane_id
                .clone()
                .or_else(|| previous.as_ref().and_then(|p| p.pane_id.clone())),
            old_worker_id: orch
                .previous_worker_id
                .clone()
                .unwrap_or_else(|| orch.worker_id.clone()),
            new_worker_id: orch.worker_id.clone(),
            number,
            finish_steps: run.finish_steps.clone(),
        }),
        // The first launch's wake names this row; durable mail keeps it.
        wake_id: None,
    })
}

/// End the orchestrator with no respawn, and say why in 1 event. A
/// launch-pending row ends `launch_failed`, a dead agent `incomplete`.
fn end_without_respawn(
    tx: &Transaction,
    run: &runs::Run,
    orch: &Worker,
    what: &str,
    why: &str,
) -> Result<Checked> {
    let reason = format!("{what}; no respawn: {why}");
    let end_as = if orch.status == WorkerStatus::Spawning.as_str() {
        WorkerStatus::LaunchFailed
    } else {
        WorkerStatus::Incomplete
    };
    workers::end(tx, &orch.worker_id, end_as, &reason, None)?;
    util::event(
        tx,
        &About {
            run: Some(&run.run_id),
            worker: Some(&orch.worker_id),
            ..Default::default()
        },
        Actor::Watchdog,
        EventKind::Note,
        &format!("Orchestrator {}: {reason}.", orch.worker_id),
    )?;
    Ok(Checked {
        outcome: Outcome::NotRespawned(why.to_string()),
        wake_id: None,
    })
}

/// Record that the start of `respawn` failed. The respawn still counts, so
/// a launch that always fails cannot loop. The row stays launch-pending: a
/// later pass launches it again inside the guards, until the last respawn
/// ends it (TRF-38).
pub fn launch_failed(conn: &Connection, respawn: &Respawn, error: &str) -> Result<()> {
    util::event(
        conn,
        &About {
            run: Some(&respawn.run_id),
            worker: Some(&respawn.new_worker_id),
            ..Default::default()
        },
        Actor::Watchdog,
        EventKind::Note,
        &format!(
            "Orchestrator respawn {} did not start: {error}",
            respawn.number
        ),
    )
}

/// The status line of the respawns of `run_id`; `None` before the first
/// respawn and before a stop.
pub fn status_line(conn: &Connection, run_id: &str) -> Result<Option<String>> {
    let g = guard(conn, run_id)?;
    let mut parts = Vec::new();
    if g.respawns > 0 {
        parts.push(format!(
            "respawned {} of {RESPAWNS_PER_RUN}, last at {}",
            g.respawns,
            g.last_respawn_at.as_deref().unwrap_or("-")
        ));
    }
    if let Some(at) = &g.stopped_at {
        parts.push(format!("stopped at {at}, no respawn"));
    }
    Ok((!parts.is_empty()).then(|| format!("Orchestrator: {}.", parts.join("; "))))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::taskledger::runs::NewRun;
    use crate::taskledger::Store;
    use std::cell::Cell;

    /// A probe with a set lock holder, set live PIDs and a set workspace.
    struct Fake {
        holder: Cell<Option<Holder>>,
        alive: Cell<bool>,
        open: Cell<Option<bool>>,
    }

    impl Fake {
        fn dead() -> Fake {
            Fake {
                holder: Cell::new(None),
                alive: Cell::new(false),
                open: Cell::new(Some(true)),
            }
        }
    }

    impl Probe for Fake {
        fn holder(&self, _: &str) -> Option<Holder> {
            self.holder.get()
        }
        fn alive(&self, _: u32, _: Option<u64>) -> bool {
            self.alive.get()
        }
        fn workspace_open(&self, _: &str) -> Option<bool> {
            self.open.get()
        }
    }

    /// A run in workspace `w1` with an orchestrator row whose agent PID 4242
    /// is recorded.
    fn world() -> (tempfile::TempDir, Store, String) {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("ledger.sqlite3")).unwrap();
        let run_id = store
            .immediate(|tx| {
                let r = runs::create(
                    tx,
                    dir.path(),
                    &NewRun {
                        slug: "r1".into(),
                        workspace_id: Some("w1".into()),
                        ..Default::default()
                    },
                )?;
                let id = workers::insert(
                    tx,
                    &NewWorker {
                        run_id: r.run_id.clone(),
                        kind: Some(WorkerKind::Orchestrator),
                        persona: "orchestrator".into(),
                        role: Some("orchestrator".into()),
                        workspace_id: Some("w1".into()),
                        pane_id: Some("p1".into()),
                        ..Default::default()
                    },
                )?;
                tx.execute(
                    "UPDATE workers SET status = 'working', agent_pid = 4242, agent_started = 1
                     WHERE worker_id = ?1",
                    [&id],
                )?;
                runs::set_orchestrator(tx, &r.run_id, &id)?;
                Ok(r.run_id)
            })
            .unwrap();
        (dir, store, run_id)
    }

    /// The fake clock: the ledger stamp `secs` after `t0`.
    fn at(t0: &chrono::DateTime<chrono::Utc>, secs: i64) -> String {
        crate::clock::stamp(*t0 + chrono::Duration::seconds(secs))
    }

    fn pass(store: &mut Store, run_id: &str, now: &str, probe: &Fake) -> Outcome {
        store
            .immediate(|tx| check(tx, run_id, now, probe))
            .unwrap()
            .outcome
    }

    /// Make the current orchestrator row a registered, now dead, agent, as
    /// a pass that observed its lock holder leaves it.
    fn register_dead(store: &Store, run_id: &str) {
        let run = runs::get(store.conn(), run_id).unwrap();
        store
            .conn()
            .execute(
                "UPDATE workers SET status = 'working', agent_pid = 5000, agent_started = 2
                 WHERE worker_id = ?1",
                [run.orchestrator_worker_id.unwrap()],
            )
            .unwrap();
    }

    #[test]
    fn trf_38_a_dead_orchestrator_in_an_active_run_respawns_with_the_handoff_decisions_and_status()
    {
        let (dir, mut store, run_id) = world();
        let t0 = crate::clock::now();
        let Outcome::Respawned(r) = pass(&mut store, &run_id, &at(&t0, 0), &Fake::dead()) else {
            panic!("a confirmed death respawns");
        };
        assert_eq!((r.number, r.workspace_id.as_str()), (1, "w1"));
        assert_eq!(r.old_pane_id.as_deref(), Some("p1"));

        let old = workers::get(store.conn(), &r.old_worker_id).unwrap();
        assert_eq!(old.status, "incomplete");
        assert_eq!(old.end_reason.as_deref(), Some("handed_off"));
        let handoff = dir.path().canonicalize().unwrap().join(HANDOFF_FILE);
        assert_eq!(old.handoff_path.as_deref(), handoff.to_str());

        let new = workers::get(store.conn(), &r.new_worker_id).unwrap();
        assert_eq!(
            (new.kind.as_str(), new.status.as_str()),
            ("orchestrator", "spawning"),
            "launch-pending until a pass observes the lock holder"
        );
        assert_eq!(
            new.previous_worker_id.as_deref(),
            Some(r.old_worker_id.as_str())
        );
        let run = runs::get(store.conn(), &run_id).unwrap();
        assert_eq!(
            run.orchestrator_worker_id.as_deref(),
            Some(r.new_worker_id.as_str())
        );

        let first = first_instruction(&r.slug, r.number);
        let handoff_at = first.find(HANDOFF_FILE).unwrap();
        let decisions_at = first.find("ai_docs/plans/r1/decisions.md").unwrap();
        let status_at = first.find("horch ledger status").unwrap();
        assert!(
            handoff_at < decisions_at && decisions_at < status_at,
            "{first}"
        );
        assert_eq!(
            parse_respawn_env(&r.env_value()),
            Some(("r1".to_string(), 1))
        );
    }

    #[test]
    fn trf_38_a_live_lock_holder_is_registered_and_never_respawned() {
        let (_dir, mut store, run_id) = world();
        let now = crate::clock::now_stamp();
        let probe = Fake::dead();
        probe.holder.set(Some(Holder {
            pid: 777,
            started: Some(9),
        }));
        assert_eq!(
            pass(&mut store, &run_id, &now, &probe),
            Outcome::Registered(777)
        );
        let run = runs::get(store.conn(), &run_id).unwrap();
        let w = workers::get(store.conn(), run.orchestrator_worker_id.as_deref().unwrap()).unwrap();
        assert_eq!((w.agent_pid, w.agent_started), (Some(777), Some(9)));
        assert_eq!(pass(&mut store, &run_id, &now, &probe), Outcome::Nothing);

        // A live recorded PID with no lock (an old launcher) is alive too.
        probe.holder.set(None);
        probe.alive.set(true);
        assert_eq!(pass(&mut store, &run_id, &now, &probe), Outcome::Nothing);
    }

    #[test]
    fn trf_38_an_unconfirmed_death_never_respawns() {
        let (_dir, mut store, run_id) = world();
        let now = crate::clock::now_stamp();
        let run = runs::get(store.conn(), &run_id).unwrap();
        let orch = run.orchestrator_worker_id.unwrap();
        // herdr cannot say whether the workspace exists.
        let probe = Fake::dead();
        probe.open.set(None);
        assert_eq!(pass(&mut store, &run_id, &now, &probe), Outcome::Nothing);
        // No agent PID was ever recorded.
        store
            .conn()
            .execute(
                "UPDATE workers SET agent_pid = NULL WHERE worker_id = ?1",
                [&orch],
            )
            .unwrap();
        assert_eq!(
            pass(&mut store, &run_id, &now, &Fake::dead()),
            Outcome::Nothing
        );
        assert_eq!(guard(store.conn(), &run_id).unwrap().respawns, 0);
    }

    #[test]
    fn trf_38_a_gone_workspace_ends_the_orchestrator_with_no_respawn() {
        let (_dir, mut store, run_id) = world();
        let now = crate::clock::now_stamp();
        let probe = Fake::dead();
        probe.open.set(Some(false));
        let out = pass(&mut store, &run_id, &now, &probe);
        assert!(
            matches!(&out, Outcome::NotRespawned(why) if why.contains("w1 is gone")),
            "{out:?}"
        );
        assert_eq!(
            pass(&mut store, &run_id, &now, &probe),
            Outcome::Nothing,
            "1 event only"
        );
        assert_eq!(guard(store.conn(), &run_id).unwrap().respawns, 0);
    }

    #[test]
    fn trf_39_no_respawn_after_a_deliberate_stop() {
        let (_dir, mut store, run_id) = world();
        let now = crate::clock::now_stamp();
        assert!(record_stop(store.conn(), &run_id).unwrap());
        let first = guard(store.conn(), &run_id).unwrap().stopped_at;
        assert!(
            !record_stop(store.conn(), &run_id).unwrap(),
            "the first stop stays"
        );
        assert_eq!(guard(store.conn(), &run_id).unwrap().stopped_at, first);
        assert_eq!(
            pass(&mut store, &run_id, &now, &Fake::dead()),
            Outcome::Nothing
        );
        assert_eq!(guard(store.conn(), &run_id).unwrap().respawns, 0);
        let line = status_line(store.conn(), &run_id).unwrap().unwrap();
        assert!(line.contains("stopped at"), "{line}");
    }

    #[test]
    fn trf_39_run_close_records_the_stop_and_a_closed_run_never_respawns() {
        let (_dir, mut store, run_id) = world();
        let now = crate::clock::now_stamp();
        let run = runs::get(store.conn(), &run_id).unwrap();
        store
            .immediate(|tx| runs::close_run(tx, &run, Some("test")))
            .unwrap();
        assert!(guard(store.conn(), &run_id).unwrap().stopped_at.is_some());
        // Even with the stop cleared, a closed run does not respawn.
        store
            .conn()
            .execute(
                "UPDATE runs SET orchestrator_stopped_at = NULL WHERE run_id = ?1",
                [&run_id],
            )
            .unwrap();
        assert_eq!(
            pass(&mut store, &run_id, &now, &Fake::dead()),
            Outcome::Nothing
        );
        assert_eq!(guard(store.conn(), &run_id).unwrap().respawns, 0);
    }

    #[test]
    fn trf_40_at_most_1_respawn_per_10_min_and_3_per_run() {
        let (_dir, mut store, run_id) = world();
        let t0 = crate::clock::now();
        let probe = Fake::dead();
        let mut respawns = Vec::new();
        // A pass every 30 s for 50 min; each new orchestrator dies at once.
        for secs in (0..=3000).step_by(30) {
            match pass(&mut store, &run_id, &at(&t0, secs), &probe) {
                Outcome::Respawned(r) => {
                    respawns.push((secs, r.number));
                    register_dead(&store, &run_id);
                }
                Outcome::Waiting(left) => assert!(left > 0 && left <= RESPAWN_EVERY_S),
                Outcome::NotRespawned(why) => {
                    assert!(why.contains("3 respawns"), "{why}");
                    respawns.push((secs, 0));
                }
                Outcome::Nothing => {}
                other => panic!("at {secs} s: {other:?}"),
            }
        }
        // After the 3rd respawn the next death ends with no wait.
        assert_eq!(respawns, vec![(0, 1), (600, 2), (1200, 3), (1230, 0)]);
        let g = guard(store.conn(), &run_id).unwrap();
        assert_eq!((g.respawns, g.last_respawn_at), (3, Some(at(&t0, 1200))));

        // Each respawn is 1 event, 1 wake, and status shows the count.
        let events: i64 = store
            .conn()
            .query_row(
                "SELECT count(*) FROM events WHERE run_id = ?1 AND text LIKE 'orchestrator-respawned:%'",
                [&run_id],
                |r| r.get(0),
            )
            .unwrap();
        let wakes: i64 = store
            .conn()
            .query_row(
                "SELECT count(*) FROM outbox WHERE run_id = ?1 AND event = 'orchestrator-respawned'",
                [&run_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!((events, wakes), (3, 3));
        let run = runs::get(store.conn(), &run_id).unwrap();
        let board = crate::taskledger::status::render(
            &crate::taskledger::status::report(store.conn(), &run).unwrap(),
        );
        assert!(
            board.contains(&format!(
                "Orchestrator: respawned 3 of 3, last at {}.",
                at(&t0, 1200)
            )),
            "{board}"
        );
    }

    fn claim_in(store: &mut Store, run_id: &str, ws: &str, probe: &Fake) -> Result<Claimed> {
        store.immediate(|tx| claim(tx, run_id, ws, Some("p9"), probe))
    }

    /// The `note` events of `run_id` that a launch claim wrote.
    fn claim_events(store: &Store, run_id: &str) -> Vec<String> {
        let mut stmt = store
            .conn()
            .prepare(
                "SELECT text FROM events WHERE run_id = ?1
                 AND text LIKE 'An orchestrator launch claims%' ORDER BY rowid",
            )
            .unwrap();
        stmt.query_map([run_id], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<Vec<String>>>()
            .unwrap()
    }

    #[test]
    fn trf_38_launch_after_fleet_stop_clears_the_stop() {
        let (dir, mut store, run_id) = world();
        assert!(record_stop(store.conn(), &run_id).unwrap());
        let at = guard(store.conn(), &run_id).unwrap().stopped_at.unwrap();
        let run = runs::get(store.conn(), &run_id).unwrap();
        assert_eq!(
            launch_run(store.conn(), dir.path(), "w1", None)
                .unwrap()
                .map(|r| r.run_id),
            Some(run_id.clone()),
            "the workspace's 1 active run"
        );

        let out = claim_in(&mut store, &run_id, "w1", &Fake::dead()).unwrap();
        let Claimed::Changed(text) = out else {
            panic!("{out:?}")
        };
        assert!(
            text.contains(&format!("orchestrator stop {at} -> cleared")),
            "{text}"
        );
        assert!(!text.contains("workspace"), "{text}");
        assert_eq!(guard(store.conn(), &run_id).unwrap().stopped_at, None);
        assert_eq!(claim_events(&store, &run_id), vec![text]);
        // The live orchestrator row stays the run's, now in this pane.
        let after = runs::get(store.conn(), &run_id).unwrap();
        assert_eq!(after.orchestrator_worker_id, run.orchestrator_worker_id);
        let w = workers::get(
            store.conn(),
            after.orchestrator_worker_id.as_deref().unwrap(),
        )
        .unwrap();
        assert_eq!(w.pane_id.as_deref(), Some("p9"));

        // A 2nd launch changes nothing and writes no event.
        assert_eq!(
            claim_in(&mut store, &run_id, "w1", &Fake::dead()).unwrap(),
            Claimed::Unchanged
        );
        assert_eq!(claim_events(&store, &run_id).len(), 1);
        // The watchdog now respawns again.
        let now = crate::clock::now_stamp();
        assert!(matches!(
            pass(&mut store, &run_id, &now, &Fake::dead()),
            Outcome::Respawned(_)
        ));
    }

    #[test]
    fn trf_38_launch_in_a_new_workspace_moves_the_run() {
        let (dir, mut store, run_id) = world();
        // The old workspace was gone: the watchdog ended the orchestrator.
        let probe = Fake::dead();
        probe.open.set(Some(false));
        let now = crate::clock::now_stamp();
        assert!(matches!(
            pass(&mut store, &run_id, &now, &probe),
            Outcome::NotRespawned(_)
        ));
        let old = runs::get(store.conn(), &run_id)
            .unwrap()
            .orchestrator_worker_id;
        assert_eq!(
            launch_run(store.conn(), dir.path(), "w2", None)
                .unwrap()
                .map(|r| r.run_id),
            Some(run_id.clone()),
            "the project's 1 active run"
        );

        let out = claim_in(&mut store, &run_id, "w2", &Fake::dead()).unwrap();
        let Claimed::Changed(text) = out else {
            panic!("{out:?}")
        };
        assert!(text.contains("workspace w1 -> w2"), "{text}");
        assert_eq!(claim_events(&store, &run_id), vec![text]);
        let run = runs::get(store.conn(), &run_id).unwrap();
        assert_eq!(run.workspace_id.as_deref(), Some("w2"));
        // A new live orchestrator row in w2 follows the ended one.
        let w = workers::get(store.conn(), run.orchestrator_worker_id.as_deref().unwrap()).unwrap();
        assert_ne!(run.orchestrator_worker_id, old);
        assert_eq!(w.previous_worker_id, old);
        assert_eq!(
            (
                w.status.as_str(),
                w.workspace_id.as_deref(),
                w.pane_id.as_deref()
            ),
            ("working", Some("w2"), Some("p9"))
        );
        // The watchdog registers the launcher that holds the w2 lock.
        let live = Fake::dead();
        live.holder.set(Some(Holder {
            pid: 31,
            started: Some(3),
        }));
        assert_eq!(
            pass(&mut store, &run_id, &now, &live),
            Outcome::Registered(31)
        );

        // A run whose orchestrator lives in its workspace does not move.
        assert_eq!(
            claim_in(&mut store, &run_id, "w3", &live).unwrap(),
            Claimed::Skipped(
                "Run r1 stays in workspace w2: its orchestrator runs there (PID 31).".into()
            )
        );
        let run = runs::get(store.conn(), &run_id).unwrap();
        assert_eq!(run.workspace_id.as_deref(), Some("w2"));
    }

    #[test]
    fn trf_38_launch_refuses_a_workspace_with_another_active_run() {
        let (dir, mut store, run_id) = world();
        assert!(record_stop(store.conn(), &run_id).unwrap());
        store
            .immediate(|tx| {
                runs::create(
                    tx,
                    dir.path(),
                    &NewRun {
                        slug: "r2".into(),
                        workspace_id: Some("w2".into()),
                        ..Default::default()
                    },
                )?;
                // A solo run never blocks a move.
                runs::create(tx, dir.path(), &runs::solo_new_run("w3"))
            })
            .unwrap();
        let err = claim_in(&mut store, &run_id, "w2", &Fake::dead())
            .unwrap_err()
            .to_string();
        assert_eq!(
            err,
            "run r1 cannot move to workspace w2: run r2 is active there"
        );
        let run = runs::get(store.conn(), &run_id).unwrap();
        assert_eq!(run.workspace_id.as_deref(), Some("w1"));
        assert!(guard(store.conn(), &run_id).unwrap().stopped_at.is_some());
        assert!(claim_events(&store, &run_id).is_empty());

        // 2 active runs and none in w4: a launch there claims neither.
        assert!(launch_run(store.conn(), dir.path(), "w4", None)
            .unwrap()
            .is_none());
        // A respawn names its run.
        assert_eq!(
            launch_run(store.conn(), dir.path(), "w4", Some("r1"))
                .unwrap()
                .map(|r| r.slug),
            Some("r1".into())
        );
        assert!(matches!(
            claim_in(&mut store, &run_id, "w3", &Fake::dead()).unwrap(),
            Claimed::Changed(_)
        ));
    }

    #[test]
    fn trf_40_the_workspace_lock_admits_1_orchestrator() {
        let dir = tempfile::tempdir().unwrap();
        let held = lock(dir.path(), "w1")
            .unwrap()
            .expect("the first launcher locks");
        let me = holder(dir.path(), "w1").expect("this process holds it");
        assert_eq!(me.pid, std::process::id());
        assert!(
            lock(dir.path(), "w1").unwrap().is_none(),
            "a 2nd orchestrator refuses"
        );
        assert!(
            lock(dir.path(), "w2").unwrap().is_some(),
            "another workspace has its own"
        );
        drop(held);
        assert!(holder(dir.path(), "w1").is_none());
        assert!(
            lock(dir.path(), "w1").unwrap().is_some(),
            "free after the holder ends"
        );
        assert_eq!(lock_name("ws/1:a"), "orchestrator-ws_1_a");
    }
}
