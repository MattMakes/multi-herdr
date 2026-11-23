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

pub const PERSONA: &str = "idea-lead";

pub const CURSOR: &str = "sub_orchestrators";

pub const IDLE_STOP: &str = "idle_stop";

pub const STOPPED: &str = "stopped";

pub const NO_RESULT: &str = "no_result";

pub const IN_FLIGHT: [&str; 6] = [
    "clarifying",
    "awaiting_approval",
    "specifying",
    "ticketing",
    "in_progress",
    "integrating",
];

const ENDED_FEATURE: [&str; 2] = ["merged", "abandoned"];

const PANE_KEPT: &str = "pane_kept";

pub fn phase_of(state: &str) -> Option<&'static str> {
    match state {
        "clarifying" | "awaiting_approval" => Some("clarifying"),
        "specifying" => Some("specifying"),
        "ticketing" => Some("ticketing"),
        "in_progress" | "integrating" => Some("supervising"),
        _ => None,
    }
}

pub fn attempt_id(idea: &str, phase: &str, n: i64) -> String {
    format!("{idea}/{phase}/{n}")
}

fn phase_in(id: &str, idea: &str) -> Option<String> {
    let rest = id.strip_prefix(idea)?.strip_prefix('/')?;
    let (phase, n) = rest.split_once('/')?;
    n.parse::<i64>().ok().map(|_| phase.to_string())
}

pub fn pane_name(idea: &str) -> String {
    format!("idea-{}", idea.strip_prefix("I-").unwrap_or(idea))
}

pub struct Keep<'a> {
    pub guard: Option<Generation>,
    pub ideas: &'a Path,
    pub facts: &'a Facts,
    pub stamp: &'a str,
    pub budget: PassBudget,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Started {
    pub attempt: String,
    pub feature: String,
    pub idea: String,
    pub phase: String,
    pub worker_id: String,
    pub pane: Option<String>,
    pub pane_name: String,
    pub folder: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ended {
    pub attempt: String,
    pub outcome: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Kept {
    pub started: Vec<Started>,
    pub ended: Vec<Ended>,
    pub stops: Vec<Stop>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Breach {
    Missing {
        feature: String,
        idea: String,
        phase: String,
    },
    Extra {
        feature: String,
        attempt: String,
    },
}

#[derive(Debug, Clone)]
struct Live {
    id: String,
    idea: String,
    idea_state: String,
    phase: Option<String>,
    worker: String,
    live: bool,
    process: Option<Recorded>,
    pane: Option<String>,
}

impl Live {
    fn current(&self) -> bool {
        self.phase
            .as_deref()
            .is_some_and(|p| phase_of(&self.idea_state) == Some(p))
    }
}

#[derive(Debug, Clone)]
struct Need {
    idea: String,
    phase: &'static str,
    waits: bool,
}

#[derive(Debug, Clone)]
struct View {
    id: String,
    open: bool,
    in_flight: bool,
    planning: Option<Need>,
    work: Option<Need>,
    live: Vec<Live>,
    ideas: Vec<IdeaFacts>,
}

#[derive(Debug, Clone)]
struct IdeaFacts {
    id: String,
    state: String,
    waiting: bool,
    attempts: Vec<(AttemptRow, Option<String>)>,
}

#[derive(Debug, Default)]
struct Decision {
    extra: Vec<usize>,
    start: Option<(String, &'static str)>,
}

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

pub fn check_i13(conn: &Connection) -> Result<Vec<Breach>> {
    let mut out = Vec::new();
    for id in feature_ids(conn)? {
        let v = view(conn, &id)?;
        out.extend(breaches(&v, &decide(&v), !v.open));
    }
    Ok(out)
}

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

fn next_n(conn: &Connection, idea: &str, phase: &str) -> Result<i64> {
    let prefix = format!("{idea}/{phase}/");
    let top = attempts::idea_attempts(conn, idea)?
        .iter()
        .filter_map(|(a, _)| a.id.strip_prefix(&prefix)?.parse::<i64>().ok())
        .max()
        .unwrap_or(0);
    Ok(top + 1)
}

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

fn word_list(states: &[&str]) -> String {
    states
        .iter()
        .map(|s| format!("'{s}'"))
        .collect::<Vec<_>>()
        .join(", ")
}

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

fn has_failed_task(conn: &Connection, idea: &str) -> Result<bool> {
    attempts::idea_has_task_in(conn, idea, TaskStatus::Failed)
}

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
