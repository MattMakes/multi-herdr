use std::collections::BTreeSet;
use std::path::Path;

use anyhow::{Context, Result};
use chrono::{DateTime, Datelike, FixedOffset, Timelike, Utc};
use horch_platform::channel::User;
use rusqlite::{params, Connection, Transaction};
use serde_json::json;

use crate::clock;
use crate::fleetd::fleet_runtime::adapters::cursors_store;
use crate::fleetd::fleet_runtime::adapters::sqlite::facts_of;
use crate::fleetd::fleet_runtime::application::jobs::JobCtx;
use crate::fleetd::fleet_runtime::application::ledger::{record, transition, Cause};
use crate::fleetd::fleet_runtime::domain::budget::PassBudget;
use crate::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use crate::fleetd::fleet_runtime::domain::config::{FleetConfig, Weekday};
use crate::fleetd::fleet_runtime::domain::ledger::{Entity, Generation};
use crate::fleetd::ideas::adapters::idea_folder_files::{FolderError, IdeaFolder};
use crate::fleetd::ideas::adapters::ideas_store::{self, IdeaRow};
use crate::fleetd::ideas::adapters::needs_you_folder::Mirrored;
use crate::fleetd::ideas::adapters::notify::{notify, Notice};
use crate::fleetd::ideas::adapters::questions_store;
use crate::fleetd::ideas::application::needs_you::{self, NeedsYouList};
use crate::fleetd::ideas::application::notes_ingest::{QUESTION_ANSWERED, QUESTION_ASKED};
use crate::fleetd::ideas::application::park_cancel::{self, PARKED};
use crate::fleetd::ideas::application::sub_orchestrators::{IDLE_STOP, IN_FLIGHT};
use crate::fleetd::ideas::domain::needs_you::Write;
use crate::fleetd::task_execution::adapters::leases_store;
use crate::fleetd::task_execution::adapters::sqlite as attempts;
use crate::fleetd::task_execution::application::commands;
use crate::fleetd::task_execution::domain::task::{AttemptRow, AttemptState, Facts, RowRef, Stop};
use crate::taskledger::model::{Actor, EventKind, TaskStatus, WorkerStatus};
use crate::taskledger::{tasks, util, workers};
use crate::workspace::PaneHost;

pub const WAITING: &str = "waiting_on_you";

pub const PARKED_NOTE: &str = "idea_parked";

pub const CURSOR: &str = "waits";

const PHASE: &str = "clarifying";

const REMINDER: &str = "waiting on you: reminder";

const PAUSED: &str = "waiting on you: lease paused";

const IDLE_STOPPED: &str = "waiting on you: idle stop";

pub struct Wait<'a> {
    pub guard: Option<Generation>,
    pub config: &'a FleetConfig,
    pub clock: FleetSeconds,
    pub now: DateTime<FixedOffset>,
    pub asking: &'a BTreeSet<String>,
    pub facts: &'a Facts,
    pub ideas: &'a Path,
    pub owner: User,
    pub budget: PassBudget,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Waits {
    pub started: Vec<String>,
    pub ended: Vec<String>,
    pub idle_stopped: Vec<String>,
    pub reminded: Vec<String>,
    pub parked: Vec<String>,
    pub stops: Vec<Stop>,
}

pub fn waiting_key(idea: &str) -> String {
    format!("{WAITING}:{idea}")
}

pub fn parked_key(idea: &str) -> String {
    format!("{PARKED_NOTE}:{idea}")
}

pub fn keep_waits(tx: &Transaction<'_>, ctx: &Wait<'_>) -> Result<Waits> {
    let stamp = clock::stamp(ctx.now.with_timezone(&Utc));
    let keys = idea_ids(tx)?;
    let mut cursor = cursors_store::load(tx, CURSOR, &stamp)?;
    let chosen: Vec<String> = cursor
        .take(&keys, ctx.budget, &stamp)
        .into_iter()
        .map(str::to_string)
        .collect();
    cursors_store::save(tx, CURSOR, &cursor)?;
    let mut out = Waits::default();
    let mut workers_ended = BTreeSet::new();
    for idea in chosen {
        keep_idea(tx, ctx, &idea, &stamp, &mut out, &mut workers_ended)
            .with_context(|| format!("the wait of idea {idea}"))?;
    }
    if !out.idle_stopped.is_empty() {
        let ours = |row: &RowRef| match row {
            RowRef::Attempt(id) => out.idle_stopped.contains(id),
            RowRef::Worker(id) => workers_ended.contains(id),
        };
        out.stops = commands::mark_orphans(tx, ctx.facts)?
            .into_iter()
            .filter(|s| ours(&s.row))
            .collect();
    }
    Ok(out)
}

pub fn remind(herdr: &PaneHost, reminded: usize, open: usize, seq: u64) -> Notice {
    let mirrored = Mirrored {
        new: reminded,
        open,
        ..Mirrored::default()
    };
    notify(herdr, &mirrored, seq)
}

fn idea_ids(conn: &Connection) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT id FROM ideas
         WHERE state IN ('clarifying', 'awaiting_approval') OR waiting_on_you_since IS NOT NULL",
    )?;
    let mut ids: BTreeSet<String> = stmt
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    let prefix = format!("{PARKED_NOTE}:");
    for key in needs_you::open_keys(conn, PARKED_NOTE)? {
        if let Some(id) = key.strip_prefix(&prefix) {
            ids.insert(id.to_string());
        }
    }
    Ok(ids.into_iter().collect())
}

fn keep_idea(
    tx: &Connection,
    ctx: &Wait<'_>,
    idea: &str,
    stamp: &str,
    out: &mut Waits,
    workers_ended: &mut BTreeSet<String>,
) -> Result<()> {
    let Some(row) = ideas_store::idea(tx, idea)? else {
        return Ok(());
    };
    if row.state != PARKED {
        needs_you::resolve(tx, &parked_key(idea))?;
    }
    let tries = phase_attempts(tx, idea)?;
    let live = tries
        .iter()
        .map(|(a, _)| a)
        .find(|a| a.state != AttemptState::Ended);
    let question = match row.state.as_str() {
        "clarifying" => open_interview_question(tx, ctx, idea)?,
        _ => None,
    };
    if !waits(tx, ctx, &row, live, &tries, question.as_deref())? {
        if row.waiting_on_you_since.is_some() {
            set_waiting(tx, idea, None)?;
            needs_you::resolve(tx, &waiting_key(idea))?;
            out.ended.push(idea.to_string());
        }
        return Ok(());
    }
    let started = row.waiting_on_you_since.is_none();
    let since = match &row.waiting_on_you_since {
        Some(s) => s.clone(),
        None => {
            set_waiting(tx, idea, Some(stamp))?;
            out.started.push(idea.to_string());
            stamp.to_string()
        }
    };
    let now = ctx.now.with_timezone(&Utc);
    let since_at = clock::parse(&since).unwrap_or(now);
    if (now - since_at).num_seconds() >= days_s(ctx.config.park_after_days) {
        return park(tx, ctx, idea, out);
    }
    let n = reminders(tx, ctx, idea, &since, since_at, started, out)?;
    needs_you::open_item(
        tx,
        &Write {
            key: &waiting_key(idea),
            kind: WAITING,
            entity: Entity::Idea.as_str(),
            entity_id: idea,
            text: &waiting_text(&row.state, idea, &since, n, question.as_deref()),
        },
    )?;
    if let Some(a) = live {
        idle_rule(tx, ctx, &row, a, started, out, workers_ended)?;
    }
    Ok(())
}

fn waits(
    tx: &Connection,
    ctx: &Wait<'_>,
    row: &IdeaRow,
    live: Option<&AttemptRow>,
    tries: &[(AttemptRow, Option<String>)],
    question: Option<&str>,
) -> Result<bool> {
    let last_outcome = tries.last().and_then(|(_, o)| o.as_deref());
    Ok(match row.state.as_str() {
        "awaiting_approval" => true,
        "clarifying" if question.is_none() => false,
        "clarifying" => match live {
            Some(_) => ctx.asking.contains(&row.id),
            None => {
                row.waiting_on_you_since.is_some()
                    && last_outcome == Some(IDLE_STOP)
                    && !answered(tx, ctx, &row.id)?
            }
        },
        _ => false,
    })
}

fn idle_rule(
    tx: &Connection,
    ctx: &Wait<'_>,
    row: &IdeaRow,
    a: &AttemptRow,
    started: bool,
    out: &mut Waits,
    workers_ended: &mut BTreeSet<String>,
) -> Result<()> {
    let pause = last_note(tx, &row.id, PAUSED)?;
    let paused_at = match pause {
        Some(p) if !started && p["attempt"].as_str() == Some(a.id.as_str()) => {
            p["at"].as_i64().unwrap_or(ctx.clock.0)
        }
        _ => {
            leases_store::set_active_at(tx, &a.id, ctx.clock.0)?;
            record(
                tx,
                ctx.guard,
                EventKind::Note,
                Entity::Idea,
                &row.id,
                by_fleetd(PAUSED),
                &json!({ "attempt": a.id, "at": ctx.clock.0 }),
            )?;
            ctx.clock.0
        }
    };
    let idle_stop_s = i64::try_from(ctx.config.idle_stop_s).unwrap_or(i64::MAX);
    if ctx.clock.0.saturating_sub(paused_at) < idle_stop_s || feature_has_work(tx, &row.feature_id)?
    {
        return Ok(());
    }
    let why = format!(
        "idea {} waits on you, and its sub-orchestrator was idle for {idle_stop_s} s of fleet clock",
        row.id
    );
    attempts::set_ended(tx, &a.id, IDLE_STOP, &util::now())?;
    let text = format!("sub-orchestrator attempt {} ends {IDLE_STOP}: {why}", a.id);
    transition(
        tx,
        ctx.guard,
        Entity::Attempt,
        &a.id,
        AttemptState::Ended.as_str(),
        by_fleetd(&text),
        &NeedsYouList,
    )?;
    workers::end(tx, &a.worker_id, WorkerStatus::Stopped, &why, None)?;
    record(
        tx,
        ctx.guard,
        EventKind::Note,
        Entity::Idea,
        &row.id,
        by_fleetd(IDLE_STOPPED),
        &json!({ "attempt": a.id, "amendments": amendments(ctx, &row.id)? }),
    )?;
    out.idle_stopped.push(a.id.clone());
    workers_ended.insert(a.worker_id.clone());
    Ok(())
}

fn reminders(
    tx: &Connection,
    ctx: &Wait<'_>,
    idea: &str,
    since: &str,
    since_at: DateTime<Utc>,
    started: bool,
    out: &mut Waits,
) -> Result<u64> {
    let last = last_reminder(tx, idea, since)?;
    let (base, n) = last.unwrap_or((since_at, 0));
    let now = ctx.now.with_timezone(&Utc);
    let every = i64::try_from(ctx.config.reminder_every_s).unwrap_or(i64::MAX);
    let due = (now - base).num_seconds() >= every;
    let minute = u16::try_from(ctx.now.hour() * 60 + ctx.now.minute()).unwrap_or(0);
    let in_hours = ctx
        .config
        .working_hours
        .contains(weekday(ctx.now.weekday()), minute);
    if started || !due || !in_hours {
        return Ok(n);
    }
    let n = n + 1;
    record(
        tx,
        ctx.guard,
        EventKind::Note,
        Entity::Idea,
        idea,
        by_fleetd(REMINDER),
        &json!({ "at": clock::stamp(now), "n": n }),
    )?;
    out.reminded.push(idea.to_string());
    Ok(n)
}

fn park(tx: &Connection, ctx: &Wait<'_>, idea: &str, out: &mut Waits) -> Result<()> {
    let days = ctx.config.park_after_days;
    let job = JobCtx {
        guard: ctx.guard,
        config: ctx.config,
        needs_you: &NeedsYouList,
    };
    let reason = format!("it waited on you for {days} days with no answer");
    park_cancel::park(&job, tx, idea, &reason, ctx.clock)?;
    set_waiting(tx, idea, None)?;
    needs_you::resolve(tx, &waiting_key(idea))?;
    let text = format!(
        "Idea {idea} waited on you for {days} days with no answer, and fleetd parked it. \
         `horch idea approve {idea}` or an answer resumes it."
    );
    needs_you::open_item(
        tx,
        &Write {
            key: &parked_key(idea),
            kind: PARKED_NOTE,
            entity: Entity::Idea.as_str(),
            entity_id: idea,
            text: &text,
        },
    )?;
    out.parked.push(idea.to_string());
    Ok(())
}

fn waiting_text(state: &str, idea: &str, since: &str, n: u64, question: Option<&str>) -> String {
    let mut text = if state == "awaiting_approval" {
        format!(
            "Idea {idea} waits for your approval since {since}. Read it with \
             `horch idea show {idea}`, then run `horch idea approve {idea}` or \
             `horch idea revise {idea} \"<text>\"`."
        )
    } else {
        format!(
            "Idea {idea} waits for your answer to its interview question since {since}. \
             Answer in its pane. When its process stopped, answer with \
             `horch idea amend {idea} \"<answer>\"`."
        )
    };
    if let Some(q) = question {
        text.push_str(&format!(" The question of its sub-orchestrator: {q:?}"));
    }
    if n > 0 {
        text.push_str(&format!(" Reminder {n}."));
    }
    text
}

fn phase_attempts(conn: &Connection, idea: &str) -> Result<Vec<(AttemptRow, Option<String>)>> {
    let prefix = format!("{idea}/{PHASE}/");
    Ok(attempts::idea_attempts(conn, idea)?
        .into_iter()
        .filter(|(a, _)| a.id.starts_with(&prefix))
        .collect())
}

fn open_interview_question(
    conn: &Connection,
    ctx: &Wait<'_>,
    idea: &str,
) -> Result<Option<String>> {
    let facts = facts_of(conn, Entity::Idea, idea, EventKind::Note)?;
    let Some(at) = facts.iter().rposition(|f| f.cause == QUESTION_ASKED) else {
        return Ok(None);
    };
    if facts[at + 1..].iter().any(|f| f.cause == QUESTION_ANSWERED) {
        return Ok(None);
    }
    let data: serde_json::Value = serde_json::from_str(&facts[at].data_json).unwrap_or_default();
    let at_ask = data["amendments"].as_u64().unwrap_or(u64::MAX);
    if u64::try_from(amendments(ctx, idea)?).unwrap_or(0) > at_ask {
        return Ok(None);
    }
    Ok(Some(
        data["question"].as_str().unwrap_or_default().to_string(),
    ))
}

fn answered(conn: &Connection, ctx: &Wait<'_>, idea: &str) -> Result<bool> {
    let Some(data) = last_note(conn, idea, IDLE_STOPPED)? else {
        return Ok(false);
    };
    let at_stop = data["amendments"].as_u64().unwrap_or(u64::MAX);
    Ok(u64::try_from(amendments(ctx, idea)?).unwrap_or(0) > at_stop)
}

fn amendments(ctx: &Wait<'_>, idea: &str) -> Result<usize> {
    let folder = IdeaFolder::new(ctx.ideas, idea, ctx.owner.clone())?;
    match folder.read_amendments() {
        Ok(texts) => Ok(texts.len()),
        Err(FolderError::Missing { .. }) => Ok(0),
        Err(e) => Err(e.into()),
    }
}

fn last_reminder(
    conn: &Connection,
    idea: &str,
    since: &str,
) -> Result<Option<(DateTime<Utc>, u64)>> {
    let Some(data) = last_note(conn, idea, REMINDER)? else {
        return Ok(None);
    };
    let at = data["at"].as_str().unwrap_or_default();
    match (clock::parse(at), data["n"].as_u64()) {
        (Some(t), Some(n)) if at >= since => Ok(Some((t, n))),
        _ => Ok(None),
    }
}

fn last_note(conn: &Connection, idea: &str, cause: &str) -> Result<Option<serde_json::Value>> {
    let last = facts_of(conn, Entity::Idea, idea, EventKind::Note)?
        .into_iter()
        .rev()
        .find(|f| f.cause == cause);
    Ok(last.and_then(|f| serde_json::from_str(&f.data_json).ok()))
}

fn feature_has_work(conn: &Connection, feature: &str) -> Result<bool> {
    let in_flight: BTreeSet<String> = ideas_store::of_feature(conn, feature)?
        .into_iter()
        .filter(|i| IN_FLIGHT.contains(&i.state.as_str()))
        .map(|i| i.id)
        .collect();
    for q in questions_store::open(conn)? {
        if attempts::idea_of_task(conn, &q.task_id)?.is_some_and(|i| in_flight.contains(&i)) {
            return Ok(true);
        }
    }
    for idea in &in_flight {
        for task in attempts::open_tasks_of_idea(conn, idea)? {
            if tasks::get(conn, &task)?.status == TaskStatus::Failed.as_str() {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

fn set_waiting(conn: &Connection, idea: &str, since: Option<&str>) -> Result<()> {
    conn.execute(
        "UPDATE ideas SET waiting_on_you_since = ?2 WHERE id = ?1",
        params![idea, since],
    )?;
    Ok(())
}

fn days_s(days: u64) -> i64 {
    i64::try_from(days.saturating_mul(86_400)).unwrap_or(i64::MAX)
}

fn weekday(day: chrono::Weekday) -> Weekday {
    match day {
        chrono::Weekday::Mon => Weekday::Mon,
        chrono::Weekday::Tue => Weekday::Tue,
        chrono::Weekday::Wed => Weekday::Wed,
        chrono::Weekday::Thu => Weekday::Thu,
        chrono::Weekday::Fri => Weekday::Fri,
        chrono::Weekday::Sat => Weekday::Sat,
        chrono::Weekday::Sun => Weekday::Sun,
    }
}

fn by_fleetd(text: &str) -> Cause<'_> {
    Cause {
        actor: Actor::Horch,
        text,
    }
}
