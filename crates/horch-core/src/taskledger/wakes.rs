//! The wakes of the orchestrator: the text, the event and the key of each
//! outbox row that the task ledger, `horch ledger watch` and the pass of
//! fleetd write (`fleetd/fleet_runtime/adapters/watchdog_duties.rs`).
//!
//! Each builder reads the board in the caller's transaction and returns
//! the [`Wake`]; the caller writes it with [`outbox::write`]. A wake with a
//! key of its board state, its blocker state or its attempts is written
//! once for each state, so the direct path and a reconcile pass write 1 row.

use anyhow::Result;
use rusqlite::Connection;

use super::model::OutboxEvent;
use super::outbox::{self, Wake};
use super::runs::Run;
use super::status;
use super::tasks;
use super::util;
use super::watchdog::{self, Checked};
use super::workers::{self, Preclaim, Worker};

/// The settled wake line of `run`, or `None` while the run is not settled
/// (C6: [`status::unsettled`], the rule `run close` uses).
pub fn settled_line(conn: &Connection, run: &Run) -> Result<Option<String>> {
    if !status::unsettled(conn, &run.run_id)?.is_empty() {
        return Ok(None);
    }
    Ok(Some(format!(
        "NOTE: All tasks are done. Run horch ledger status, verify the run, then close it with horch ledger run close --run {} and report to the operator.",
        run.slug
    )))
}

/// The settled wake of `run`: 1 row per settled board state. A repeat of
/// the same settled state writes no new row.
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
            "NOTE: {task} was cancelled. {} wait for it. Run horch ledger unblock {} --from {task}, or cancel them, or requeue {task}.",
            names.join(", "),
            first.short()
        );
        (OutboxEvent::Cancelled, text)
    };
    Some(Wake::new(event, task, tagged(&text)).on_board(watchdog::blocker_state(t)))
}

pub fn stuck_line(conn: &Connection, run: &Run) -> Option<String> {
    let open: Vec<tasks::Task> = tasks::of_run(conn, &run.run_id)
        .ok()?
        .into_iter()
        .filter(|t| matches!(t.status.as_str(), "queued" | "running" | "landing"))
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
                (None, None) => format!("{} is {}", t.short(), t.status),
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

/// The startable tasks ("T-1,T-2") and "Tasks can start: T-1. Spawn: opus
/// x1." when workers should be spawned now, else `None`.
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

/// The wake after the reap of `worker_id`: the held QUESTION of its task,
/// else which persona to spawn, and how.
pub fn reaped_wake(conn: &Connection, who: &str, worker_id: &str, persona: Option<&str>) -> Wake {
    if let Some(wake) = held_question(conn, worker_id) {
        return wake;
    }
    let persona = persona.unwrap_or("the same persona");
    let reaped = format!(
        "NOTE: {who} was reaped and its task is queued again. Spawn: horch spawn {persona} --replaces {worker_id}"
    );
    Wake::new(OutboxEvent::Reaped, worker_id, tagged(&reaped))
}

/// The `held` QUESTION wake of the task of `worker_id`, when the attempt
/// limit holds it.
pub fn held_question(conn: &Connection, worker_id: &str) -> Option<Wake> {
    let w = workers::get(conn, worker_id).ok()?;
    held_wake(conn, &tasks::get(conn, w.task_id.as_deref()?).ok()?)
}

/// The `held` QUESTION wake of `task`, held after too many attempts, else
/// `None`. The key holds its attempts ([`held_state`]), not the board: the
/// reap's wake and the reconciled wake (TRF-51) are 1 row.
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
pub fn lost_wakes(conn: &Connection, run: &Run, now: &str) -> Result<Vec<Wake>> {
    // I-03: the settled wake, 1 row per settled board state.
    let mut out: Vec<Wake> = settled_wake(conn, run)?.into_iter().collect();
    // I-04: a review round in while its author still waits.
    for r in watchdog::reviews_waiting(conn, run, now)? {
        let role = r
            .author
            .role
            .clone()
            .unwrap_or_else(|| r.author.label().to_string());
        out.push(reviews_in_wake(conn, &r.parent, &role)?);
    }
    // I-05, I-06: a cancelled or parked task that open tasks wait for.
    for t in watchdog::stalled_blockers(conn, run)? {
        let t = tasks::get(conn, &t.task_id)?;
        out.extend(dependents_wake(&t, &tasks::dependents(conn, &t.task_id)?));
    }
    // I-07: a task the attempt limit holds for the operator.
    for t in watchdog::attempt_held(conn, run)? {
        out.extend(held_wake(conn, &tasks::get(conn, &t.task_id)?));
    }
    Ok(out)
}

/// The wakes of the pre-claim sweep `swept` of `run` (#190): 1 question
/// for each live worker that does not claim, with the herdr status of its
/// pane from `status`, and 1 wake for the workers that ended, with the
/// spawns that the freed slots allow.
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
