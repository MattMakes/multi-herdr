//! The sub-orchestrators: 1 for each open feature, a fresh process for each
//! phase.
//!
//! `keep_sub_orchestrators` is the use case `StartSubOrchestrator`
//! (docs/specs/fleetd/design.md, "Ideas: the dispatcher and
//! sub-orchestrators": How many sub-orchestrators, A fresh process for
//! every phase; I7, I13, FM-16, FM-31; docs/tickets/fleetd-p6/28-sub-orch.md).
//! [`check_i7`] and [`check_i13`] are the 2 checks.
//!
//! A sub-orchestrator attempt has no task. Its `idea_id` is the idea of its
//! phase, its worker has the persona [`PERSONA`], and its id is
//! `<idea>/<phase>/<n>` ([`attempt_id`]), so 1 process never plans 2 ideas.
//! The phase of an idea is [`phase_of`] its state: `awaiting_approval` is
//! the end of `clarifying` (the process waits for you), and `in_progress`
//! and `integrating` are `supervising`.
//!
//! The owner process of a feature (MAP.md, Q11):
//! - The feature needs a live process when an idea of it is in
//!   `clarifying`, `specifying` or `ticketing`, or when it has an open
//!   worker question or a `failed` task that waits for its replan.
//! - While 1 idea of the feature is in planning ([`PLANNING`]), the live
//!   process is the process of that idea and its phase. It also owns the
//!   questions of the whole feature. An idea in `awaiting_approval`, and an
//!   idea that waits for you whose process the idle rule stopped
//!   ([`IDLE_STOP`], ticket 29), wait for you: no process starts for them
//!   unless the feature has a question or a failed task.
//! - Else any live process of the feature in the current phase of its idea
//!   stays, and a feature with work and no such process gets 1 for the idea
//!   of its oldest open question, else of its oldest failed task.
//!
//! Each pass, for each feature of the work budget from the cursor
//! [`CURSOR`]: a live attempt whose process is dead ends `no_result`; each
//! live attempt that is not the 1 process of the feature ends `stopped`
//! (a second process, an old phase, or a `merged` or `abandoned` feature);
//! then a missing process is 1 new attempt in `launching`, which the
//! launcher starts after the commit. Its first instruction is its launch
//! argument: the idea, the phase and the idea folder ([`Started`], FR-7).
//!
//! The stops come from I5 on the ended rows: the process group of each
//! process that is alive with its recorded start time, and the pane only
//! when herdr shows a recorded process of the row in it. Else the pane
//! stays, with 1 needs-you item. A new attempt takes the pane of an
//! attempt of its feature when herdr shows that pane with the process of
//! that attempt, alive or just gone; else the launch opens a new pane
//! named [`pane_name`].

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use rusqlite::{Connection, Transaction};

use crate::fleetd::fleet_runtime::adapters::cursors_store;
use crate::fleetd::fleet_runtime::application::ledger::{created, transition, Cause};
use crate::fleetd::fleet_runtime::domain::budget::PassBudget;
use crate::fleetd::fleet_runtime::domain::ledger::{Entity, Generation};
use crate::fleetd::ideas::adapters::ideas_store;
use crate::fleetd::ideas::application::dispatcher::PLANNING;
use crate::fleetd::ideas::application::needs_you::{self, NeedsYouList};
use crate::fleetd::ideas::domain::needs_you::Write;
use crate::fleetd::ideas::domain::questions::State as QuestionState;
use crate::fleetd::integration::adapters::sqlite as feature_rows;
use crate::fleetd::task_execution::adapters::sqlite as attempts;
use crate::fleetd::task_execution::application::commands::keep_groups;
use crate::fleetd::task_execution::domain::task::{
    orphans, AttemptRow, AttemptState, Facts, InPane, Recorded, RowRef, Seen, Stop,
};
use crate::taskledger::model::{Actor, TaskStatus, WorkerKind, WorkerStatus};
use crate::taskledger::workers::{self, NewWorker};
use crate::taskledger::{runs, util};

/// The persona of each sub-orchestrator attempt.
pub const PERSONA: &str = "idea-lead";

/// The kind of the row of `pass_cursors` of the sub-orchestrators.
pub const CURSOR: &str = "sub_orchestrators";

/// The outcome of an attempt that the idle stop ended (ticket 29).
pub const IDLE_STOP: &str = "idle_stop";

/// The outcome of an attempt that this use case stops.
pub const STOPPED: &str = "stopped";

/// The outcome of a live attempt whose process is gone.
pub const NO_RESULT: &str = "no_result";

/// The states of I7: from `clarifying` to `integrating`.
pub const IN_FLIGHT: [&str; 6] = [
    "clarifying",
    "awaiting_approval",
    "specifying",
    "ticketing",
    "in_progress",
    "integrating",
];

/// The states of a feature that has no sub-orchestrator.
const ENDED_FEATURE: [&str; 2] = ["merged", "abandoned"];

/// The kind of the needs-you item of a pane that is not closed, as I5
/// writes it.
const PANE_KEPT: &str = "pane_kept";

/// The phase of the process of an idea in `state`, or `None` when no
/// process works on it.
pub fn phase_of(state: &str) -> Option<&'static str> {
    match state {
        "clarifying" | "awaiting_approval" => Some("clarifying"),
        "specifying" => Some("specifying"),
        "ticketing" => Some("ticketing"),
        "in_progress" | "integrating" => Some("supervising"),
        _ => None,
    }
}

/// The id of attempt `n` of the phase `phase` of `idea`.
pub fn attempt_id(idea: &str, phase: &str, n: i64) -> String {
    format!("{idea}/{phase}/{n}")
}

/// The phase in the id of the attempt `id` of `idea`, if the id has one.
fn phase_in(id: &str, idea: &str) -> Option<String> {
    let rest = id.strip_prefix(idea)?.strip_prefix('/')?;
    let (phase, n) = rest.split_once('/')?;
    n.parse::<i64>().ok().map(|_| phase.to_string())
}

/// The name of the pane of the feature whose first idea is `idea`:
/// `I-0012` gives `idea-0012`.
pub fn pane_name(idea: &str) -> String {
    format!("idea-{}", idea.strip_prefix("I-").unwrap_or(idea))
}

/// What 1 pass needs besides the ledger.
pub struct Keep<'a> {
    /// The generation of the fleetd that writes.
    pub guard: Option<Generation>,
    /// The folder of the idea folders.
    pub ideas: &'a Path,
    /// What part 1 of the pass saw of the processes and the herdr panes.
    pub facts: &'a Facts,
    /// The time stamp of the pass, for the cursor.
    pub stamp: &'a str,
    pub budget: PassBudget,
}

/// 1 new sub-orchestrator attempt in `launching`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Started {
    pub attempt: String,
    pub feature: String,
    pub idea: String,
    pub phase: String,
    pub worker_id: String,
    /// The pane of the feature that the new process takes; `None` opens a
    /// new pane named `pane_name`.
    pub pane: Option<String>,
    pub pane_name: String,
    /// The idea folder: the only memory of the process.
    pub folder: PathBuf,
}

/// 1 sub-orchestrator attempt that the pass ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ended {
    pub attempt: String,
    pub outcome: String,
}

/// What 1 pass did. `stops` are the effects to do after the commit, each
/// process before each pane.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Kept {
    pub started: Vec<Started>,
    pub ended: Vec<Ended>,
    pub stops: Vec<Stop>,
}

/// 1 breach of I7 or I13.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Breach {
    /// The feature needs a process of `idea` in `phase`, and has none.
    Missing {
        feature: String,
        idea: String,
        phase: String,
    },
    /// The live attempt `attempt` is not the 1 process of its feature.
    Extra { feature: String, attempt: String },
}

/// 1 sub-orchestrator attempt that is not ended.
#[derive(Debug, Clone)]
struct Live {
    id: String,
    idea: String,
    /// The state of its idea now.
    idea_state: String,
    /// The phase in its id.
    phase: Option<String>,
    worker: String,
    live: bool,
    process: Option<Recorded>,
    pane: Option<String>,
}

impl Live {
    /// Whether it works on the current phase of its idea.
    fn current(&self) -> bool {
        self.phase
            .as_deref()
            .is_some_and(|p| phase_of(&self.idea_state) == Some(p))
    }
}

/// An idea that needs, or may need, the process of its feature.
#[derive(Debug, Clone)]
struct Need {
    idea: String,
    phase: &'static str,
    /// It waits for you: no process starts for it alone.
    waits: bool,
}

/// The facts of 1 feature.
#[derive(Debug, Clone)]
struct View {
    id: String,
    /// Not `merged` and not `abandoned`.
    open: bool,
    /// Whether an idea of it is in [`IN_FLIGHT`].
    in_flight: bool,
    planning: Option<Need>,
    /// The idea of the oldest open question, else of the oldest failed task.
    work: Option<Need>,
    live: Vec<Live>,
    /// Its ideas, oldest first.
    ideas: Vec<IdeaFacts>,
}

/// 1 idea of a feature, with each of its sub-orchestrator attempts and
/// their outcomes, oldest first.
#[derive(Debug, Clone)]
struct IdeaFacts {
    id: String,
    state: String,
    /// `waiting_on_you_since` is set.
    waiting: bool,
    attempts: Vec<(AttemptRow, Option<String>)>,
}

/// The decision for 1 feature.
#[derive(Debug, Default)]
struct Decision {
    extra: Vec<usize>,
    /// (idea, phase) of the process to start.
    start: Option<(String, &'static str)>,
}

/// The decision for `v`: the index in `v.live` of each extra attempt, and
/// the process to start.
fn decide(v: &View) -> Decision {
    if !v.open {
        return Decision {
            extra: (0..v.live.len()).collect(),
            start: None,
        };
    }
    let fits = |a: &Live| match &v.planning {
        Some(p) => a.idea == p.idea && a.phase.as_deref() == Some(p.phase),
        None => a.current(),
    };
    let keep = v.live.iter().position(fits);
    let extra = (0..v.live.len()).filter(|i| Some(*i) != keep).collect();
    let start = match (keep, &v.planning, &v.work) {
        (Some(_), _, _) => None,
        (None, Some(p), work) if !p.waits || work.is_some() => Some(p),
        (None, Some(_), _) => None,
        (None, None, work) => work.as_ref(),
    };
    Decision {
        extra,
        start: start.map(|n| (n.idea.clone(), n.phase)),
    }
}

/// The breaches of `v`: a process to start is missing, and each attempt of
/// `extra` is extra.
fn breaches(v: &View, d: &Decision, extra: bool) -> Vec<Breach> {
    let mut out: Vec<Breach> = d
        .start
        .iter()
        .map(|(idea, phase)| Breach::Missing {
            feature: v.id.clone(),
            idea: idea.clone(),
            phase: phase.to_string(),
        })
        .collect();
    if extra {
        out.extend(d.extra.iter().map(|i| Breach::Extra {
            feature: v.id.clone(),
            attempt: v.live[*i].id.clone(),
        }));
    }
    out
}

/// I7, the live part: each feature that has an idea from `clarifying` to
/// `integrating` has exactly 1 live sub-orchestrator attempt, in the phase
/// of its idea, or waits for you.
pub fn check_i7(conn: &Connection) -> Result<Vec<Breach>> {
    let mut out = Vec::new();
    for id in feature_ids(conn)? {
        let v = view(conn, &id)?;
        if v.in_flight {
            out.extend(breaches(&v, &decide(&v), true));
        }
    }
    Ok(out)
}

/// I13: each feature that is not merged and not abandoned has its owner
/// process, live or stopped by rule, and no `merged` or `abandoned`
/// feature has a live one.
pub fn check_i13(conn: &Connection) -> Result<Vec<Breach>> {
    let mut out = Vec::new();
    for id in feature_ids(conn)? {
        let v = view(conn, &id)?;
        out.extend(breaches(&v, &decide(&v), !v.open));
    }
    Ok(out)
}

/// `StartSubOrchestrator` for the features of the work budget, in the
/// transaction `tx` of part 2 of a pass. No process starts or stops here:
/// the new attempts are rows in `launching`, and the stops are returned.
pub fn keep_sub_orchestrators(tx: &Transaction<'_>, ctx: &Keep<'_>) -> Result<Kept> {
    let keys = feature_ids(tx)?;
    let mut cursor = cursors_store::load(tx, CURSOR, ctx.stamp)?;
    let chosen: Vec<String> = cursor
        .take(&keys, ctx.budget, ctx.stamp)
        .into_iter()
        .map(str::to_string)
        .collect();
    cursors_store::save(tx, CURSOR, &cursor)?;
    let mut kept = Kept::default();
    let mut workers_ended = BTreeSet::new();
    for feature in chosen {
        keep_feature(tx, ctx, &feature, &mut kept, &mut workers_ended)
            .with_context(|| format!("the sub-orchestrator of feature {feature}"))?;
    }
    kept.stops = stops(tx, ctx, &kept.ended, &workers_ended)?;
    Ok(kept)
}

/// The use case for 1 feature.
fn keep_feature(
    tx: &Connection,
    ctx: &Keep<'_>,
    feature: &str,
    kept: &mut Kept,
    workers_ended: &mut BTreeSet<String>,
) -> Result<()> {
    let mut v = view(tx, feature)?;
    let (dead, alive): (Vec<Live>, Vec<Live>) = v
        .live
        .drain(..)
        .partition(|a| a.live && seen(a, ctx.facts) == Seen::Dead);
    v.live = alive;
    for a in &dead {
        end(
            tx,
            ctx,
            a,
            NO_RESULT,
            WorkerStatus::Incomplete,
            "its process is gone",
        )?;
        kept.ended.push(ended(a, NO_RESULT));
        workers_ended.insert(a.worker.clone());
    }
    let d = decide(&v);
    for i in &d.extra {
        let a = &v.live[*i];
        end(
            tx,
            ctx,
            a,
            STOPPED,
            WorkerStatus::Stopped,
            &why_extra(&v, a),
        )?;
        kept.ended.push(ended(a, STOPPED));
        workers_ended.insert(a.worker.clone());
    }
    if let Some((idea, phase)) = d.start {
        kept.started.push(start(tx, ctx, &v, &idea, phase)?);
    }
    Ok(())
}

fn ended(a: &Live, outcome: &str) -> Ended {
    Ended {
        attempt: a.id.clone(),
        outcome: outcome.to_string(),
    }
}

/// Why the live attempt `a` of `v` is extra.
fn why_extra(v: &View, a: &Live) -> String {
    if !v.open {
        format!(
            "feature {} is merged or abandoned: it has no sub-orchestrator",
            v.id
        )
    } else if !a.current() {
        format!(
            "idea {} is {}: a fresh process starts for each phase",
            a.idea, a.idea_state
        )
    } else {
        format!("feature {} has another live sub-orchestrator", v.id)
    }
}

/// What part 1 saw of the live attempt `a`: its process, else its pane.
/// An attempt with neither is dead: nothing can hold it.
fn seen(a: &Live, facts: &Facts) -> Seen {
    match (a.process, &a.pane) {
        (Some(p), _) => facts.processes.get(&p).copied().unwrap_or(Seen::Unknown),
        (None, Some(pane)) => match &facts.panes {
            Some(panes) if panes.contains_key(pane) => Seen::Alive,
            Some(_) => Seen::Dead,
            None => Seen::Unknown,
        },
        (None, None) => Seen::Dead,
    }
}

fn by_fleetd(text: &str) -> Cause<'_> {
    Cause {
        actor: Actor::Horch,
        text,
    }
}

/// End the attempt `a` with `outcome`, and its worker with `status`.
fn end(
    tx: &Connection,
    ctx: &Keep<'_>,
    a: &Live,
    outcome: &str,
    status: WorkerStatus,
    why: &str,
) -> Result<()> {
    attempts::set_ended(tx, &a.id, outcome, &util::now())?;
    let ended = AttemptState::Ended.as_str();
    let text = format!("sub-orchestrator attempt {} ends {outcome}: {why}", a.id);
    transition(
        tx,
        ctx.guard,
        Entity::Attempt,
        &a.id,
        ended,
        by_fleetd(&text),
        &NeedsYouList,
    )?;
    workers::end(tx, &a.worker, status, why, None)?;
    Ok(())
}

/// A new attempt of `idea` in `phase`, in `launching`, with a new worker of
/// [`PERSONA`] in the internal run of `feature`.
fn start(tx: &Connection, ctx: &Keep<'_>, v: &View, idea: &str, phase: &str) -> Result<Started> {
    let feature = v.id.as_str();
    let run = runs::feature_run(tx, feature)?
        .with_context(|| format!("the task ledger has no internal run of feature {feature}"))?;
    let id = attempt_id(idea, phase, next_n(tx, idea, phase)?);
    let worker_id = workers::insert(
        tx,
        &NewWorker {
            run_id: run.run_id,
            kind: Some(WorkerKind::Worker),
            persona: PERSONA.to_string(),
            ..NewWorker::default()
        },
    )?;
    let pane = proven_pane(v, ctx.facts);
    let row = AttemptRow {
        id: id.clone(),
        task_id: None,
        worker_id: worker_id.clone(),
        state: AttemptState::Launching,
        process: None,
        pane: pane.clone(),
    };
    attempts::insert_attempt(tx, &row, &util::now())?;
    attempts::set_idea(tx, &id, idea)?;
    let text = format!("feature {feature} gets sub-orchestrator attempt {id} of idea {idea}");
    created(tx, ctx.guard, Entity::Attempt, &id, by_fleetd(&text))?;
    Ok(Started {
        attempt: id,
        feature: feature.to_string(),
        idea: idea.to_string(),
        phase: phase.to_string(),
        worker_id,
        pane,
        pane_name: pane_name(v.ideas.first().map_or(idea, |i| i.id.as_str())),
        folder: ctx.ideas.join(idea),
    })
}

/// The number of the next attempt of `idea` in `phase`: 1 above the
/// highest.
fn next_n(conn: &Connection, idea: &str, phase: &str) -> Result<i64> {
    let prefix = format!("{idea}/{phase}/");
    let top = attempts::idea_attempts(conn, idea)?
        .iter()
        .filter_map(|(a, _)| a.id.strip_prefix(&prefix)?.parse::<i64>().ok())
        .max()
        .unwrap_or(0);
    Ok(top + 1)
}

/// The pane of an attempt of `v` that herdr shows with the process of that
/// attempt in it: alive with its start time, or gone with herdr still
/// naming its pid. The last such attempt, by idea, wins. Else `None`:
/// herdr can give an old pane id to a pane outside the fleet.
fn proven_pane(v: &View, facts: &Facts) -> Option<String> {
    let proven = |a: &AttemptRow| {
        let (Some(p), Some(pane)) = (a.process, &a.pane) else {
            return false;
        };
        match facts.panes.as_ref().and_then(|m| m.get(pane)) {
            Some(InPane::Runs { pid, pid_start }) => {
                p.pid == *pid && p.pid_start == Some(*pid_start)
            }
            Some(InPane::Gone(pid)) => {
                p.pid == *pid && facts.processes.get(&p).copied() == Some(Seen::Dead)
            }
            Some(InPane::Unknown) | None => false,
        }
    };
    v.ideas
        .iter()
        .flat_map(|i| &i.attempts)
        .map(|(a, _)| a)
        .rev()
        .find(|a| proven(a))
        .and_then(|a| a.pane.clone())
}

/// The stops of the attempts of `ended` and the workers of `workers` (I5),
/// after the commit. Each pane that no fact proves gets 1 needs-you item.
fn stops(
    tx: &Connection,
    ctx: &Keep<'_>,
    ended: &[Ended],
    workers: &BTreeSet<String>,
) -> Result<Vec<Stop>> {
    if ended.is_empty() {
        return Ok(Vec::new());
    }
    let ours = |row: &RowRef| match row {
        RowRef::Attempt(id) => ended.iter().any(|e| e.attempt == *id),
        RowRef::Worker(id) => workers.contains(id),
    };
    let cleanup = orphans(&attempts::rows(tx, &util::host_name())?, ctx.facts);
    for k in cleanup.kept.iter().filter(|k| ours(&k.row)) {
        let (entity, id) = match &k.row {
            RowRef::Worker(id) => (Entity::Worker, id),
            RowRef::Attempt(id) => (Entity::Attempt, id),
        };
        let key = format!("{PANE_KEPT}:{}", k.pane);
        let text = format!(
            "fleetd did not close pane {} of sub-orchestrator {entity} {id}: no fact proves \
             that it is the pane of the row, and herdr can give an old pane id to a pane \
             outside the fleet. Close the pane yourself when it is the pane of the row.",
            k.pane
        );
        needs_you::open_item(
            tx,
            &Write {
                key: &key,
                kind: PANE_KEPT,
                entity: entity.as_str(),
                entity_id: id,
                text: &text,
            },
        )?;
    }
    let groups: Vec<_> = cleanup
        .kept_groups
        .into_iter()
        .filter(|k| ours(&k.row))
        .collect();
    keep_groups(tx, &groups)?;
    Ok(cleanup.stops.into_iter().filter(|s| ours(&s.row)).collect())
}

/// The id of each feature with an idea in [`IN_FLIGHT`] or with a
/// sub-orchestrator attempt that is not ended, in id order.
fn feature_ids(conn: &Connection) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT DISTINCT feature_id FROM ideas WHERE state IN ({})",
        word_list(&IN_FLIGHT)
    ))?;
    let mut ids: BTreeSet<String> = stmt
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    for idea in attempts::ideas_with_open_attempts(conn)? {
        if let Some(row) = ideas_store::idea(conn, &idea)? {
            ids.insert(row.feature_id);
        }
    }
    Ok(ids.into_iter().collect())
}

/// `'a', 'b'` of `states`, for an `IN` list of fixed words.
fn word_list(states: &[&str]) -> String {
    states
        .iter()
        .map(|s| format!("'{s}'"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The facts of the feature `id`.
fn view(conn: &Connection, id: &str) -> Result<View> {
    let feature = feature_rows::feature(conn, id)?
        .with_context(|| format!("the ledger has no feature {id}"))?;
    let mut stmt = conn.prepare(
        "SELECT id, state, waiting_on_you_since FROM ideas WHERE feature_id = ?1 ORDER BY rowid",
    )?;
    let rows: Vec<(String, String, Option<String>)> = stmt
        .query_map([id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let mut ideas = Vec::new();
    for (idea, state, waiting) in rows {
        ideas.push(IdeaFacts {
            attempts: attempts::idea_attempts(conn, &idea)?,
            id: idea,
            state,
            waiting: waiting.is_some(),
        });
    }
    let live = ideas
        .iter()
        .flat_map(|i| i.attempts.iter().map(move |(a, _)| (i, a)))
        .filter(|(_, a)| a.state != AttemptState::Ended)
        .map(|(i, a)| Live {
            phase: phase_in(&a.id, &i.id),
            id: a.id.clone(),
            idea: i.id.clone(),
            idea_state: i.state.clone(),
            worker: a.worker_id.clone(),
            live: a.state == AttemptState::Live,
            process: a.process,
            pane: a.pane.clone(),
        })
        .collect();
    Ok(View {
        id: id.to_string(),
        open: !ENDED_FEATURE.contains(&feature.state.as_str()),
        in_flight: ideas.iter().any(|i| IN_FLIGHT.contains(&i.state.as_str())),
        planning: planning(&ideas),
        work: work(conn, &ideas)?,
        live,
        ideas,
    })
}

/// The oldest idea in [`PLANNING`]. It waits for you in
/// `awaiting_approval`, or when it waits for you and the idle rule stopped
/// the last process of its phase.
fn planning(ideas: &[IdeaFacts]) -> Option<Need> {
    let i = ideas
        .iter()
        .find(|i| PLANNING.contains(&i.state.as_str()))?;
    let phase = phase_of(&i.state)?;
    let last_outcome = i
        .attempts
        .iter()
        .rev()
        .find(|(a, _)| phase_in(&a.id, &i.id).as_deref() == Some(phase))
        .and_then(|(_, outcome)| outcome.as_deref());
    let waits = i.state == "awaiting_approval" || (i.waiting && last_outcome == Some(IDLE_STOP));
    Some(Need {
        idea: i.id.clone(),
        phase,
        waits,
    })
}

/// 1 statement for any count of tasks. A failed task is open: its status
/// is not `done` or `cancelled`.
fn has_failed_task(conn: &Connection, idea: &str) -> Result<bool> {
    let failed = conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM tasks WHERE idea_id = ?1 AND status = ?2)",
        [idea, TaskStatus::Failed.as_str()],
        |r| r.get(0),
    )?;
    Ok(failed)
}

/// The idea in [`IN_FLIGHT`] of the oldest open question of the feature of
/// `ideas`, else of its oldest `failed` task that waits for its replan.
fn work(conn: &Connection, ideas: &[IdeaFacts]) -> Result<Option<Need>> {
    let need = |idea: &str| {
        let i = ideas.iter().find(|i| i.id == idea)?;
        let phase = phase_of(&i.state)?;
        Some(Need {
            idea: i.id.clone(),
            phase,
            waits: false,
        })
    };
    let mut stmt =
        conn.prepare("SELECT task_id FROM questions WHERE state = ?1 ORDER BY asked_at, rowid")?;
    let asked: Vec<String> = stmt
        .query_map([QuestionState::Open.as_str()], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    for task in asked {
        if let Some(n) = attempts::idea_of_task(conn, &task)?.and_then(|i| need(&i)) {
            return Ok(Some(n));
        }
    }
    for i in ideas
        .iter()
        .filter(|i| IN_FLIGHT.contains(&i.state.as_str()))
    {
        if has_failed_task(conn, &i.id)? {
            return Ok(need(&i.id));
        }
    }
    Ok(None)
}

#[cfg(test)]
mod has_failed_task_tests {
    use super::*;
    use crate::taskledger::store::Store;
    use crate::taskledger::tasks;
    use crate::taskledger::testing::count_statements;
    use rusqlite::params;

    fn ledger(statuses: &[&str]) -> (tempfile::TempDir, Store, String) {
        let tmp = tempfile::tempdir().unwrap();
        let mut store = Store::open(&tmp.path().join("ledger.sqlite3")).unwrap();
        let idea = store
            .immediate(|tx| {
                tx.execute_batch(
                    "INSERT INTO projects (project_id, root, created_at) VALUES ('p1', '/p', 'now');
                     INSERT INTO runs (run_id, project_id, slug, finish_steps, status, started_at)
                     VALUES ('r1', 'p1', 'r1', '[]', 'active', 'now');",
                )?;
                let idea = crate::taskledger::schema::run_idea(tx, "r1", "now", true)?;
                for (i, status) in statuses.iter().enumerate() {
                    tx.execute(
                        "INSERT INTO tasks (task_id, run_id, seq, title, task_file,
                             primary_persona, claimable_by, status, created_at, updated_at, idea_id)
                         VALUES (?1, 'r1', ?2, ?1, 'a.md', 'opus', 'opus', ?3, 'now', 'now', ?4)",
                        params![format!("T{i:04}"), i as i64 + 1, status, idea],
                    )?;
                }
                Ok(idea)
            })
            .unwrap();
        (tmp, store, idea.expect("the run has its idea"))
    }

    fn failed_one_by_one(conn: &Connection, idea: &str) -> bool {
        attempts::open_tasks_of_idea(conn, idea)
            .unwrap()
            .iter()
            .any(|t| tasks::get(conn, t).unwrap().status == TaskStatus::Failed.as_str())
    }

    #[test]
    fn has_failed_task_gives_the_answer_of_the_read_of_each_task() {
        let mixed = [
            "running",
            "done",
            "cancelled",
            "ready",
            "pending",
            "verifying",
        ];
        for failed in [false, true] {
            let mut statuses = mixed.to_vec();
            if failed {
                statuses.push("failed");
            }
            let (_tmp, store, idea) = ledger(&statuses);
            let c = store.conn();
            assert_eq!(failed_one_by_one(c, &idea), failed);
            assert_eq!(has_failed_task(c, &idea).unwrap(), failed);
        }
    }

    #[test]
    fn has_failed_task_runs_the_same_statements_for_3_and_300_tasks() {
        let count = |n: usize| {
            let (_tmp, store, idea) = ledger(&vec!["running"; n]);
            let (failed, statements) = count_statements(store.conn(), || {
                has_failed_task(store.conn(), &idea).unwrap()
            });
            assert!(!failed);
            statements
        };
        assert_eq!(count(3), count(300));
    }
}
