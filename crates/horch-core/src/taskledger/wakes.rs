//! The wakes of today's orchestrator, and the new owner of each one
//! (docs/specs/fleetd/design.md, FM-16, OR-3, "The needs-you list";
//! docs/tickets/fleetd-p6/46-wakes.md). No wake goes to an orchestrator.
//!
//! | Wake of today | New owner | What the owner gets |
//! |---|---|---|
//! | `startable_wake` | fleetd | Nothing: the dispatcher launches each ready task. |
//! | `spawn_advice` | fleetd | Nothing: fleetd launches each attempt. |
//! | `reviews_in_wake(s)` | fleetd | Nothing: the verifier runs the finish steps. |
//! | `reaped_wake` | fleetd | Nothing: fleetd queues the task again and launches the next attempt. |
//! | `preclaim_wakes` | fleetd | Nothing: fleetd binds a task before the pane exists. |
//! | `stuck_wake` | you | 1 needs-you item `stuck:<feature>` ([`stuck_item`]). |
//! | `lost_wakes` | you | The needs-you items of the reconcile pass ([`lost_items`]). |
//! | `held_wake`, `held_question` | you | 1 needs-you item `held:<task>` ([`held_item`]). |
//! | `blocker_wake`, `dependents_wake` | you | 1 needs-you item `blocker:<task>` ([`blocker_item`]). |
//! | `settled_wake` | you | 1 needs-you item `feature_ready:<feature>`, only for a feature in `ready` ([`settled_item`]). |
//! | `verdict_wake` | the sub-orchestrator of the feature of the task | 1 doorbell ([`verdict_doorbell`]). |
//! | A worker question | the sub-orchestrator of the feature of the task | The question flow (`fleetd/ideas/application/question_flow.rs`). |
//!
//! [`Today::owner`] holds this table. A doorbell names 1 feature: the
//! feature of the idea of its task. A wake of one feature never reaches
//! the sub-orchestrator of another feature. A wake whose feature is not
//! known goes to you as a needs-you item, because a guard whose fact is
//! unknown does not hold.
//!
//! The outbox builders below (`settled_wake` to `preclaim_wakes`) are
//! interim: their callers still write outbox rows. They are removed when
//! T-547 and T-553 wire the routed functions of this module into fleetd.
//!
//! Each interim builder reads the board in the caller's transaction and
//! returns the [`Wake`]; the caller writes it with [`outbox::write`]. A
//! wake with a key of its board state, its blocker state or its attempts
//! is written once for each state, so the direct path and a reconcile pass
//! write 1 row.

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
use super::workers::{self, Preclaim, Worker};

/// Each wake of today's orchestrator, by the builder that makes it.
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

/// The new owner of a wake of today. No owner is an orchestrator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewOwner {
    /// fleetd acts in code, and nobody gets a line.
    Fleetd,
    /// You, through 1 needs-you item by key.
    You,
    /// The sub-orchestrator of the feature, through 1 doorbell.
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

    /// The table of the module doc.
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

/// 1 needs-you item of a wake, written by its key.
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

    /// Open the item of [`Item::key`] in `tx`. An open item of the key gets
    /// the new text, so a repeat pass keeps 1 item.
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

/// A wake with its new owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Routed {
    NeedsYou(Item),
    /// 1 doorbell to the live sub-orchestrator of `feature_id`. The caller
    /// rings only that feature.
    Doorbell {
        feature_id: String,
        text: String,
    },
}

/// The feature of `run` (its internal run, MAP.md of fleetd-p6, Q5) and
/// its state, or `None`.
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

/// The needs-you item of a settled `run` whose feature is `ready`, else
/// `None`. A settled run of a feature that is not `ready` waits for the
/// feature rules of fleetd, and nobody gets a line.
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

/// The needs-you item of the cancelled or parked blocker `task_id` that
/// open tasks wait for, else `None`.
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

/// The needs-you item of `task`, held after too many attempts, else
/// `None`. A parked task is a blocker item ([`blocker_item`]).
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

/// The needs-you item of a stuck `run`: it has open tasks, none can start
/// and no worker is live. Its key names the feature of the run, so each
/// pass keeps 1 item.
pub fn stuck_item(conn: &Connection, run: &Run) -> Result<Option<Item>> {
    let Some(text) = stuck_line(conn, run) else {
        return Ok(None);
    };
    Ok(Some(match feature_of_run(conn, run)? {
        Some((feature, _)) => Item::new("stuck", "feature", &feature, text),
        None => Item::new("stuck", "run", &run.run_id, text),
    }))
}

/// The needs-you items that a crash between a ledger commit and its item
/// loses, from the board of `run`: the ready feature, each stalled blocker
/// and each held task. A review round needs no item: fleetd runs the
/// finish steps.
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

/// The doorbell of a verdict to the sub-orchestrator of the feature of the
/// task of its worker: a first stall and a suspect. `None` for each other
/// verdict. A task with no feature gives a needs-you item.
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

/// The settled wake line of `run`, or `None` while the run is not settled
/// (C6: [`status::unsettled`], the rule `run close` uses).
///
/// Interim: removed when T-547 and T-553 wire the routed functions.
pub fn settled_line(conn: &Connection, run: &Run) -> Result<Option<String>> {
    // 1 count query first: a run with an open task is not settled, and the
    // pass reads no row of it (PL-5).
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

/// The settled wake of `run`: 1 row per settled board state. A repeat of
/// the same settled state writes no new row.
///
/// Interim: removed when T-547 and T-553 wire the routed functions.
pub fn settled_wake(conn: &Connection, run: &Run) -> Result<Option<Wake>> {
    let Some(line) = settled_line(conn, run)? else {
        return Ok(None);
    };
    let board = outbox::board(conn, &run.run_id, false)?;
    Ok(Some(
        Wake::new(OutboxEvent::Settled, "run", tagged(&line)).on_board(board),
    ))
}

/// The wake of blocker `t` that `dependents` wait for, else `None`. The
/// key holds the blocker's state ([`watchdog::blocker_state`]), not the
/// board: the direct wake and the reconciled wake (TRF-51) are 1 row.
///
/// Interim: removed when T-547 and T-553 wire the routed functions.
pub fn dependents_wake(t: &tasks::Task, dependents: &[tasks::Task]) -> Option<Wake> {
    let first = dependents.first()?;
    let task = t.short();
    let names: Vec<String> = dependents.iter().map(|t| t.short()).collect();
    // A parked task is not cancelled: it resumes on a requeue (D16).
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

/// The statuses of an open task of a stuck run.
const OPEN: [TaskStatus; 5] = [
    TaskStatus::Pending,
    TaskStatus::Ready,
    TaskStatus::Escalated,
    TaskStatus::Running,
    TaskStatus::Verifying,
];

fn stuck_line(conn: &Connection, run: &Run) -> Option<String> {
    // 1 count query first: a run with no open task or with a working
    // worker is not stuck, and the pass reads no row of it (PL-5).
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

/// Interim: removed when T-547 and T-553 wire the routed functions.
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

/// The startable tasks ("T-1,T-2") and "Tasks can start: T-1. Spawn: opus
/// x1." when workers should be spawned now, else `None`.
///
/// Interim: removed when T-547 and T-553 wire the routed functions.
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

/// The `[horch]` tag in front of a wake line, once.
pub fn tagged(text: &str) -> String {
    if text.starts_with("[horch]") {
        text.to_string()
    } else {
        format!("[horch] {text}")
    }
}

/// The wake of a verdict: ask a worker on its first stall for an update,
/// and name the pane of a suspect. `None` for each other verdict.
///
/// Interim: removed when T-547 and T-553 wire the routed functions.
pub fn verdict_wake(c: &Checked) -> Option<Wake> {
    let who = c.role.as_deref().unwrap_or(&c.worker_id);
    let on = c.task.as_deref().unwrap_or("its task");
    let text = match c.verdict.as_str() {
        "no_progress" if c.first_stall => format!(
            "NOTE: {who} on {on} shows no progress ({}). Ask it for an update: horch tell {who} \"[orchestrator] QUESTION: What is your status?\"",
            c.note
        ),
        "suspect" => format!(
            "NOTE: {who} on {on} may be stuck in pane {}: {}. The watchdog reaps it at the next check unless something else moves.",
            c.pane_id.as_deref().unwrap_or("unknown"),
            c.note
        ),
        _ => return None,
    };
    Some(Wake::new(OutboxEvent::Held, util::new_id(), tagged(&text)))
}

/// The wake after the reap of `worker_id`: the held QUESTION of its task;
/// for a `failed` task, no spawn (its replan wake asks for the decision);
/// else which persona to spawn, and how.
///
/// Interim: removed when T-547 and T-553 wire the routed functions.
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

/// The `held` QUESTION wake of the task of `worker_id`, when the attempt
/// limit holds it.
///
/// Interim: removed when T-547 and T-553 wire the routed functions.
pub fn held_question(conn: &Connection, worker_id: &str) -> Option<Wake> {
    held_wake(conn, &task_of(conn, worker_id)?)
}

/// The task of worker `worker_id`, if it has one.
fn task_of(conn: &Connection, worker_id: &str) -> Option<tasks::Task> {
    let w = workers::get(conn, worker_id).ok()?;
    tasks::get(conn, w.task_id.as_deref()?).ok()
}

/// The `held` QUESTION wake of `task`, held after too many attempts, else
/// `None`. The key holds its attempts ([`held_state`]), not the board: the
/// reap's wake and the reconciled wake (TRF-51) are 1 row.
///
/// Interim: removed when T-547 and T-553 wire the routed functions.
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

/// The `reviews_in` wake of `parent`, whose author `role` still waits.
///
/// Interim: removed when T-547 and T-553 wire the routed functions.
pub fn reviews_in_wake(conn: &Connection, parent: &tasks::Task, role: &str) -> Result<Wake> {
    let p = parent.short();
    let text = format!(
        "NOTE: The reviews of {p} are in. Make sure {role} runs horch ledger finish: horch tell {role} \"[orchestrator] NOTE: The reviews of {p} are in. Run horch ledger finish.\""
    );
    Ok(Wake::new(OutboxEvent::ReviewsIn, p, tagged(&text))
        .on_board(watchdog::review_round(conn, &parent.task_id)?))
}

/// The stuck wake of `run`: it has open tasks, none can start and no
/// worker is live, so nothing else would ever wake the orchestrator (#119).
/// Once for each stuck state: the same line is not sent twice in a row.
///
/// Interim: removed when T-547 and T-553 wire the routed functions.
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

/// The wakes at `now` that a crash between a ledger commit and its wake
/// loses (TRF-51, review 31 I-03 to I-07). Each comes from the board, with
/// the key and the text of its direct path, so a wake that exists writes no
/// new row. No live worker holds these wakes back: each event needs the
/// orchestrator, whoever else works.
///
/// Interim: removed when T-547 and T-553 wire the routed functions.
pub fn lost_wakes(conn: &Connection, run: &Run, now: &str) -> Result<Vec<Wake>> {
    // I-03: the settled wake, 1 row per settled board state.
    let mut out: Vec<Wake> = settled_wake(conn, run)?.into_iter().collect();
    // I-04: a review round in while its author still waits.
    out.extend(reviews_in_wakes(conn, run, now)?);
    // I-05, I-06: a cancelled or parked task that open tasks wait for.
    for t in watchdog::stalled_blockers(conn, run)? {
        out.extend(blocker_wake(conn, &t.task_id)?);
    }
    // I-07: a task the attempt limit holds for the operator.
    for t in watchdog::attempt_held(conn, run)? {
        out.extend(held_wake(conn, &tasks::get(conn, &t.task_id)?));
    }
    Ok(out)
}

/// The lost wakes I-04 of `run` at `now` ([`lost_wakes`]).
///
/// Interim: removed when T-547 and T-553 wire the routed functions.
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

/// The lost wake I-05 or I-06 of the stalled blocker `task_id`
/// ([`lost_wakes`]).
///
/// Interim: removed when T-547 and T-553 wire the routed functions.
pub fn blocker_wake(conn: &Connection, task_id: &str) -> Result<Option<Wake>> {
    let t = tasks::get(conn, task_id)?;
    Ok(dependents_wake(&t, &tasks::dependents(conn, task_id)?))
}

/// The wakes of the pre-claim sweep `swept` of `run` (#190): 1 question
/// for each live worker that does not claim, with the herdr status of its
/// pane from `status`, and 1 wake for the workers that ended, with the
/// spawns that the freed slots allow.
///
/// Interim: removed when T-547 and T-553 wire the routed functions.
pub fn preclaim_wakes(
    conn: &Connection,
    run: &Run,
    swept: &[Preclaim],
    status: &dyn Fn(&Worker) -> Option<String>,
) -> Vec<Wake> {
    let mut out = Vec::new();
    let mut ended = Vec::new();
    let mut ended_ids = Vec::new();
    for p in swept {
        match &p.ended {
            Some(end) => {
                ended.push(format!("{} ({end}: {})", p.worker.label(), p.note));
                ended_ids.push(p.worker.worker_id.clone());
            }
            None => {
                let state = status(&p.worker).unwrap_or_else(|| "unknown".into());
                let text = format!(
                    "NOTE: {}. Look at its pane {} (herdr status {state}): it may wait at a login, trust or quota prompt.",
                    p.note,
                    p.worker.pane_id.as_deref().unwrap_or("(none)")
                );
                let subject = format!("asked:{}", p.worker.worker_id);
                out.push(Wake::new(
                    OutboxEvent::UnclaimedSpawn,
                    subject,
                    tagged(&text),
                ));
            }
        }
    }
    if !ended.is_empty() {
        let advice = spawn_advice(conn, run).map(|(_, a)| a).unwrap_or_default();
        let text = format!(
            "NOTE: These workers ended before they claimed a task: {}. {advice}",
            ended.join(", ")
        );
        out.push(Wake::new(
            OutboxEvent::UnclaimedSpawn,
            format!("ended:{}", ended_ids.join(",")),
            tagged(text.trim_end()),
        ));
    }
    out
}
