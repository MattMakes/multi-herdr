use anyhow::Result;
use rusqlite::{Connection, OptionalExtension};

use crate::fleetd::ideas::application::{needs_you, question_flow};
use crate::fleetd::ideas::domain::needs_you::Write;
use crate::fleetd::integration::domain::feature::FeatureState;

use super::model::{OutboxEvent, TaskStatus, WorkerKind, WorkerStatus};
use super::outbox::{self, Wake};
use super::runs::Run;
use super::sql_words;
use super::status;
use super::tasks;
use super::util;
use super::watchdog::{self, Checked};
use super::workers;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Today {
    Settled,
    Dependents,
    Startable,
    SpawnAdvice,
    Verdict,
    Reaped,
    HeldQuestion,
    Held,
    ReviewsIn,
    Stuck,
    Lost,
    Blocker,
    Preclaim,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewOwner {
    Fleetd,
    You,
    SubOrchestrator,
}

impl Today {
    pub const ALL: [Today; 13] = [
        Today::Settled,
        Today::Dependents,
        Today::Startable,
        Today::SpawnAdvice,
        Today::Verdict,
        Today::Reaped,
        Today::HeldQuestion,
        Today::Held,
        Today::ReviewsIn,
        Today::Stuck,
        Today::Lost,
        Today::Blocker,
        Today::Preclaim,
    ];

    pub fn owner(self) -> NewOwner {
        match self {
            Today::Startable
            | Today::SpawnAdvice
            | Today::ReviewsIn
            | Today::Reaped
            | Today::Preclaim => NewOwner::Fleetd,
            Today::Stuck
            | Today::Lost
            | Today::HeldQuestion
            | Today::Held
            | Today::Blocker
            | Today::Dependents
            | Today::Settled => NewOwner::You,
            Today::Verdict => NewOwner::SubOrchestrator,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub key: String,
    pub kind: &'static str,
    pub entity: &'static str,
    pub entity_id: String,
    pub text: String,
}

impl Item {
    fn new(kind: &'static str, entity: &'static str, entity_id: &str, text: String) -> Item {
        Item {
            key: format!("{kind}:{entity_id}"),
            kind,
            entity,
            entity_id: entity_id.to_string(),
            text,
        }
    }

    pub fn open(&self, tx: &Connection) -> Result<()> {
        needs_you::open_item(
            tx,
            &Write {
                key: &self.key,
                kind: self.kind,
                entity: self.entity,
                entity_id: &self.entity_id,
                text: &self.text,
            },
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Routed {
    NeedsYou(Item),
    Doorbell { feature_id: String, text: String },
}

fn feature_of_run(conn: &Connection, run: &Run) -> Result<Option<(String, String)>> {
    Ok(conn
        .query_row(
            "SELECT f.id, f.state FROM runs r JOIN features f ON f.id = printf('F-%04d', r.rowid)
             WHERE r.run_id = ?1",
            [&run.run_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?)
}

pub fn settled_item(conn: &Connection, run: &Run) -> Result<Option<Item>> {
    let Some((feature, state)) = feature_of_run(conn, run)? else {
        return Ok(None);
    };
    if state != FeatureState::Ready.as_str() || settled_line(conn, run)?.is_none() {
        return Ok(None);
    }
    let text = format!(
        "NOTE: All tasks of feature {feature} are done and the feature is ready. Merge its pull request, or run horch feature abandon {feature}."
    );
    Ok(Some(Item::new("feature_ready", "feature", &feature, text)))
}

pub fn blocker_item(conn: &Connection, task_id: &str) -> Result<Option<Item>> {
    let t = tasks::get(conn, task_id)?;
    if !t.is_parked() && t.status != TaskStatus::Cancelled.as_str() {
        return Ok(None);
    }
    let dependents = tasks::dependents(conn, task_id)?;
    if dependents.is_empty() {
        return Ok(None);
    }
    let task = t.short();
    let names: Vec<String> = dependents.iter().map(|d| d.short()).collect();
    let text = if t.is_parked() {
        format!(
            "NOTE: {} {}: {task} is parked unlanded. Run horch task requeue {task} to resume it.",
            names.join(", "),
            if names.len() == 1 { "waits" } else { "wait" }
        )
    } else {
        format!(
            "NOTE: {task} was cancelled. {} wait for it. Cancel them with horch task cancel, or add the work of {task} again with horch idea add --direct.",
            names.join(", ")
        )
    };
    Ok(Some(Item::new("blocker", "task", task_id, text)))
}

pub fn held_item(conn: &Connection, task: &tasks::Task) -> Result<Option<Item>> {
    let Some(held) = task.held_reason.as_deref() else {
        return Ok(None);
    };
    if task.is_parked() {
        return Ok(None);
    }
    let ends = tasks::attempt_ends(conn, task)?;
    let text = format!(
        "QUESTION: {t} is held: {held} ({}). Requeue it (horch task requeue {t}), change its ticket, or cancel it (horch task cancel {t}).",
        ends.join("; "),
        t = task.short()
    );
    Ok(Some(Item::new("held", "task", &task.task_id, text)))
}

pub fn stuck_item(conn: &Connection, run: &Run) -> Result<Option<Item>> {
    let Some(text) = stuck_line(conn, run) else {
        return Ok(None);
    };
    Ok(Some(match feature_of_run(conn, run)? {
        Some((feature, _)) => Item::new("stuck", "feature", &feature, text),
        None => Item::new("stuck", "run", &run.run_id, text),
    }))
}

pub fn lost_items(conn: &Connection, run: &Run) -> Result<Vec<Item>> {
    let mut out: Vec<Item> = settled_item(conn, run)?.into_iter().collect();
    for t in watchdog::stalled_blockers(conn, run)? {
        out.extend(blocker_item(conn, &t.task_id)?);
    }
    for t in watchdog::attempt_held(conn, run)? {
        out.extend(held_item(conn, &tasks::get(conn, &t.task_id)?)?);
    }
    Ok(out)
}

pub fn verdict_doorbell(conn: &Connection, c: &Checked) -> Result<Option<Routed>> {
    let who = c.role.as_deref().unwrap_or(&c.worker_id);
    let on = c.task.as_deref().unwrap_or("its task");
    let text = match c.verdict.as_str() {
        "no_progress" if c.first_stall => {
            format!("NOTE: {who} on {on} shows no progress ({}).", c.note)
        }
        "suspect" => format!(
            "NOTE: {who} on {on} may be stuck in pane {}: {}. fleetd ends it at its stall limit unless something else moves.",
            c.pane_id.as_deref().unwrap_or("unknown"),
            c.note
        ),
        _ => return Ok(None),
    };
    let task = task_of(conn, &c.worker_id);
    let feature = match &task {
        Some(t) => question_flow::feature_of_task(conn, &t.task_id)?,
        None => None,
    };
    Ok(Some(match feature {
        Some(feature_id) => Routed::Doorbell { feature_id, text },
        None => {
            let id = task.map_or_else(|| c.worker_id.clone(), |t| t.task_id);
            Routed::NeedsYou(Item::new("stall", "task", &id, text))
        }
    }))
}

pub fn settled_line(conn: &Connection, run: &Run) -> Result<Option<String>> {
    let open: i64 = conn.query_row(
        &format!(
            "SELECT count(*) FROM tasks WHERE run_id = ?1 AND status NOT IN ({ended})",
            ended = sql_words::list(sql_words::TASKS_ENDED)
        ),
        [&run.run_id],
        |r| r.get(0),
    )?;
    if open > 0 || !status::unsettled(conn, &run.run_id)?.is_empty() {
        return Ok(None);
    }
    Ok(Some(format!(
        "NOTE: All tasks are done. Run horch ledger status, verify the run, then close it with horch ledger run close --run {} and report to the operator.",
        run.slug
    )))
}

pub fn settled_wake(conn: &Connection, run: &Run) -> Result<Option<Wake>> {
    let Some(line) = settled_line(conn, run)? else {
        return Ok(None);
    };
    let board = outbox::board(conn, &run.run_id, false)?;
    Ok(Some(
        Wake::new(OutboxEvent::Settled, "run", tagged(&line)).on_board(board),
    ))
}

pub fn dependents_wake(t: &tasks::Task, dependents: &[tasks::Task]) -> Option<Wake> {
    let first = dependents.first()?;
    let task = t.short();
    let names: Vec<String> = dependents.iter().map(|t| t.short()).collect();
    let (event, text) = if t.is_parked() {
        let text = format!(
            "NOTE: {} {}: {task} is parked unlanded. Run horch ledger requeue {task} to resume it.",
            names.join(", "),
            if names.len() == 1 { "waits" } else { "wait" }
        );
        (OutboxEvent::Held, text)
    } else {
        let text = format!(
            "NOTE: {task} was cancelled. {} wait for it. Run horch ledger unblock {} --from {task}, or cancel them. {task} cannot run again: add its ticket again with horch ledger add.",
            names.join(", "),
            first.short()
        );
        (OutboxEvent::Cancelled, text)
    };
    Some(Wake::new(event, task, tagged(&text)).on_board(watchdog::blocker_state(t)))
}

const OPEN: [TaskStatus; 5] = [
    TaskStatus::Pending,
    TaskStatus::Ready,
    TaskStatus::Escalated,
    TaskStatus::Running,
    TaskStatus::Verifying,
];

fn stuck_line(conn: &Connection, run: &Run) -> Option<String> {
    let may_be_stuck: bool = conn
        .query_row(
            &format!(
                "SELECT EXISTS (SELECT 1 FROM tasks WHERE run_id = ?1 AND status IN ({open}))
                    AND NOT EXISTS (SELECT 1 FROM workers
                        WHERE run_id = ?1 AND kind = {worker} AND status = {working})",
                open = sql_words::list(&OPEN),
                worker = sql_words::word(WorkerKind::Worker),
                working = sql_words::word(WorkerStatus::Working),
            ),
            [&run.run_id],
            |r| r.get(0),
        )
        .ok()?;
    if !may_be_stuck {
        return None;
    }
    let open: Vec<tasks::Task> = tasks::of_run(conn, &run.run_id)
        .ok()?
        .into_iter()
        .filter(|t| OPEN.iter().any(|s| s.as_str() == t.status))
        .collect();
    if open.is_empty() || !tasks::startable(conn, &run.run_id).ok()?.is_empty() {
        return None;
    }
    let now = util::now();
    let live = workers::of_run(conn, &run.run_id).ok()?.iter().any(|w| {
        w.kind == "worker"
            && w.status().is_ok_and(|s| s.is_live())
            && !workers::is_stale(w, run.heartbeat_stale_s, &now)
    });
    if live {
        return None;
    }
    let waits = tasks::waits(conn, &run.run_id).ok()?;
    let parts: Vec<String> = open
        .iter()
        .map(|t| {
            match (
                &t.held_reason,
                waits.iter().find(|(w, _)| w.task_id == t.task_id),
            ) {
                (Some(r), _) => format!("{} is held ({r})", t.short()),
                (None, Some((_, w))) => format!("{}: {w}", t.short()),
                (None, None) => format!("{} is {}", t.short(), t.shown_status()),
            }
        })
        .collect();
    Some(format!(
        "NOTE: the run is stuck: {}. No worker is live and no task can start.",
        parts.join("; ")
    ))
}

pub fn startable_wake(conn: &Connection, run: &Run) -> Result<Option<Wake>> {
    let Some((ready, advice)) = spawn_advice(conn, run) else {
        return Ok(None);
    };
    let board = outbox::board(conn, &run.run_id, true)?;
    Ok(Some(
        Wake::new(
            OutboxEvent::Startable,
            ready,
            tagged(&format!("NOTE: {advice}")),
        )
        .on_board(board),
    ))
}

pub fn spawn_advice(conn: &Connection, run: &Run) -> Option<(String, String)> {
    let plan = tasks::plan_workers(conn, &run.run_id).ok()?;
    let spawn: Vec<String> = plan
        .iter()
        .filter(|p| p.spawn > 0)
        .map(|p| {
            if p.replaces.is_empty() {
                format!("{} x{}", p.persona, p.spawn)
            } else {
                format!(
                    "{} x{} (--replaces {})",
                    p.persona,
                    p.spawn,
                    p.replaces.join(", ")
                )
            }
        })
        .collect();
    if spawn.is_empty() {
        return None;
    }
    let ready: Vec<String> = tasks::startable(conn, &run.run_id)
        .map(|v| v.iter().map(|t| t.short()).collect())
        .unwrap_or_default();
    Some((
        ready.join(","),
        format!(
            "Tasks can start: {}. Spawn: {}.",
            ready.join(", "),
            spawn.join(", ")
        ),
    ))
}

pub fn tagged(text: &str) -> String {
    if text.starts_with("[horch]") {
        text.to_string()
    } else {
        format!("[horch] {text}")
    }
}

pub fn reaped_wake(conn: &Connection, who: &str, worker_id: &str, persona: Option<&str>) -> Wake {
    if let Some(wake) = held_question(conn, worker_id) {
        return wake;
    }
    if let Some(t) = task_of(conn, worker_id).filter(|t| t.status == TaskStatus::Failed.as_str()) {
        let reaped = format!(
            "NOTE: {who} was reaped. Its task {} failed and waits for its replan. Spawn no worker.",
            t.short()
        );
        return Wake::new(OutboxEvent::Reaped, worker_id, tagged(&reaped));
    }
    let persona = persona.unwrap_or("the same persona");
    let reaped = format!(
        "NOTE: {who} was reaped and its task is queued again. Spawn: horch spawn {persona} --replaces {worker_id}"
    );
    Wake::new(OutboxEvent::Reaped, worker_id, tagged(&reaped))
}

pub fn held_question(conn: &Connection, worker_id: &str) -> Option<Wake> {
    held_wake(conn, &task_of(conn, worker_id)?)
}

fn task_of(conn: &Connection, worker_id: &str) -> Option<tasks::Task> {
    let w = workers::get(conn, worker_id).ok()?;
    tasks::get(conn, w.task_id.as_deref()?).ok()
}

pub fn held_wake(conn: &Connection, task: &tasks::Task) -> Option<Wake> {
    let held = task.held_reason.as_deref()?;
    if task.is_parked() {
        return None;
    }
    let ends = tasks::attempt_ends(conn, task).unwrap_or_default();
    let question = format!(
        "QUESTION: {t} is held: {held} ({}). Ask the operator: requeue it (horch ledger requeue {t} \"<reason>\"), change the ticket, or cancel it (horch ledger cancel {t} \"<reason>\"). Spawn no replacement.",
        ends.join("; "),
        t = task.short()
    );
    Some(
        Wake::new(OutboxEvent::Held, task.task_id.clone(), tagged(&question))
            .on_board(watchdog::held_state(task)),
    )
}

pub fn reviews_in_wake(conn: &Connection, parent: &tasks::Task, role: &str) -> Result<Wake> {
    let p = parent.short();
    let text = format!(
        "NOTE: The reviews of {p} are in. Make sure {role} runs horch ledger finish: horch tell {role} \"[orchestrator] NOTE: The reviews of {p} are in. Run horch ledger finish.\""
    );
    Ok(Wake::new(OutboxEvent::ReviewsIn, p, tagged(&text))
        .on_board(watchdog::review_round(conn, &parent.task_id)?))
}

pub fn stuck_wake(conn: &Connection, run: &Run) -> Result<Option<Wake>> {
    let Some(text) = stuck_line(conn, run) else {
        return Ok(None);
    };
    let line = tagged(&text);
    let prefix = format!("{}:{}:stuck", run.run_id, OutboxEvent::Held.as_str());
    if outbox::last_text(conn, &run.run_id, &prefix)?.as_deref() == Some(line.as_str()) {
        return Ok(None);
    }
    let board = outbox::board(conn, &run.run_id, true)?;
    Ok(Some(
        Wake::new(OutboxEvent::Held, "stuck", line).on_board(board),
    ))
}

pub fn lost_wakes(conn: &Connection, run: &Run, now: &str) -> Result<Vec<Wake>> {
    let mut out: Vec<Wake> = settled_wake(conn, run)?.into_iter().collect();
    out.extend(reviews_in_wakes(conn, run, now)?);
    for t in watchdog::stalled_blockers(conn, run)? {
        out.extend(blocker_wake(conn, &t.task_id)?);
    }
    for t in watchdog::attempt_held(conn, run)? {
        out.extend(held_wake(conn, &tasks::get(conn, &t.task_id)?));
    }
    Ok(out)
}

pub fn reviews_in_wakes(conn: &Connection, run: &Run, now: &str) -> Result<Vec<Wake>> {
    let mut out = Vec::new();
    for r in watchdog::reviews_waiting(conn, run, now)? {
        let role = r
            .author
            .role
            .clone()
            .unwrap_or_else(|| r.author.label().to_string());
        out.push(reviews_in_wake(conn, &r.parent, &role)?);
    }
    Ok(out)
}

pub fn blocker_wake(conn: &Connection, task_id: &str) -> Result<Option<Wake>> {
    let t = tasks::get(conn, task_id)?;
    Ok(dependents_wake(&t, &tasks::dependents(conn, task_id)?))
}
