//! Waking the orchestrator from the task ledger (section 9, trace-fixes C3):
//! a wake is 1 outbox row first, then a drain moves each undelivered row
//! into durable mail, as `horch tell orchestrator` sends, tagged `[horch]`.
//! A failed send leaves the row for the next drain. A wake never fails the
//! ledger operation that wrote it.

use horch_core::execution::lifecycle::ReportStatus;
use horch_core::ids::WorkspaceId;
use horch_core::messaging::mail;
use horch_core::runtime::RuntimeContext;
use horch_core::taskledger::model::{Actor, EventKind, OutboxEvent};
use horch_core::taskledger::outbox::{self, Row, Scope, Wake, Written};
use horch_core::taskledger::runs::{self, Run};
use horch_core::taskledger::status;
use horch_core::taskledger::stop::Panes;
use horch_core::taskledger::tasks;
use horch_core::taskledger::util::{self, About};
use horch_core::taskledger::watchdog;
use horch_core::taskledger::workers;
use horch_core::taskledger::Store;
use horch_core::workspace::herdr::Herdr;
use rusqlite::{Connection, Transaction};

/// `HORCH_NO_WAKE=1` turns wakes off (tests that run no herdr). A wake is
/// still written; its send fails, so the row stays undelivered.
fn disabled() -> bool {
    std::env::var("HORCH_NO_WAKE").is_ok_and(|v| v == "1")
}

/// Wake the run's orchestrator with the free-text line `text`. Every call
/// is a new wake, as before the outbox. The event comes from the text
/// ([`event_of`]); a caller that knows its event uses [`wake_as`].
pub fn wake(ctx: &RuntimeContext, store: &Store, run: &Run, text: &str) {
    let wake = Wake::new(event_of(text), util::new_id(), tagged(text));
    wake_as(ctx, store, run, |_| Ok(Some(wake)));
}

/// The outbox event of a free-text wake. The column allows only the C3
/// events: a stall or suspect line about a worker is `held`.
fn event_of(text: &str) -> OutboxEvent {
    if text.contains("The reviews of") {
        OutboxEvent::ReviewsIn
    } else if text.contains("was reaped") {
        OutboxEvent::Reaped
    } else if text.contains("Spawn") {
        OutboxEvent::Startable
    } else {
        OutboxEvent::Held
    }
}

/// Write the wake that `decide` returns, right after the ledger commit and
/// before any send, then drain the run's outbox. `decide` reads the board
/// in the same transaction as the write. The result is what was written;
/// an error is printed, never returned.
pub fn wake_as(
    ctx: &RuntimeContext,
    store: &Store,
    run: &Run,
    decide: impl FnOnce(&Transaction) -> anyhow::Result<Option<Wake>>,
) -> Written {
    let written = match outbox::write_now(store, &run.run_id, decide) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("horch ledger: the wake was not written: {e:#}");
            return Written::Nothing;
        }
    };
    let new = match &written {
        Written::New(id) => Some(id.as_str()),
        _ => None,
    };
    drain_run(ctx, store, run, new);
    written
}

/// Drain the outbox rows of `run` into durable mail. `new` is the row that
/// the caller just wrote: only its failed send is printed and recorded.
/// An older row failed before, and fails again in silence.
pub fn drain_run(ctx: &RuntimeContext, store: &Store, run: &Run, new: Option<&str>) {
    let drained = outbox::drain(store, Scope::Run(&run.run_id), |row| {
        send_row(ctx, store, run, row, new == Some(row.id.as_str()))
    });
    if let Err(e) = drained {
        eprintln!("horch ledger: the outbox drain failed: {e:#}");
    }
}

/// The courier's drain: the outbox rows of the runs of `workspace`. A
/// missing ledger has no rows; an error is printed, never returned.
pub fn drain_workspace(ctx: &RuntimeContext, workspace: &str) {
    let store = match horch_core::taskledger::open_existing(ctx) {
        Ok(Some(store)) => store,
        Ok(None) => return,
        Err(e) => {
            eprintln!("horch courier: the ledger did not open: {e:#}");
            return;
        }
    };
    let drained = outbox::drain(&store, Scope::Workspace(workspace), |row| {
        let run = runs::get(store.conn(), &row.run_id)?;
        send_row(ctx, &store, &run, row, false)
    });
    if let Err(e) = drained {
        eprintln!("horch courier: the outbox drain failed: {e:#}");
    }
}

/// Send 1 outbox row to the orchestrator of `run` as durable mail, and
/// record the result. `Ok`: the mail is written, and the courier delivers
/// it if the push did not. `Err`: nothing is queued; the row stays.
fn send_row(
    ctx: &RuntimeContext,
    store: &Store,
    run: &Run,
    row: &Row,
    first: bool,
) -> anyhow::Result<()> {
    let line = &row.text;
    // The run's workspace wins (#218): the caller can be a pane of another
    // workspace, or a watchdog started from one. The caller's workspace
    // counts only for a run with no workspace.
    let mut ctx = ctx.clone();
    if let Some(id) = run
        .workspace_id
        .as_deref()
        .and_then(|ws| WorkspaceId::new(ws).ok())
    {
        ctx.herdr.workspace = Some(id);
    }
    let target = match &ctx.herdr.workspace {
        Some(ws) if run.workspace_id.is_some() => format!("the run's workspace {}", ws.as_str()),
        Some(ws) => format!("the caller's workspace {}; the run has none", ws.as_str()),
        None => "no workspace".to_string(),
    };
    let sent = if disabled() {
        Err(anyhow::anyhow!("wakes are off (HORCH_NO_WAKE=1)"))
    } else {
        // The row id keys the mail: a replay after a crash before
        // `mark_delivered` reuses the first mail and writes no 2nd file.
        mail::keyed(&row.id, || {
            super::messaging::tell_as(&ctx, Some("horch".into()), "orchestrator", line)
        })
    };
    let note = match &sent {
        Ok(ReportStatus::Delivered) => format!("Wake delivered to {target}: {line}"),
        Ok(ReportStatus::Queued(reason)) => {
            format!("Wake queued, not yet acknowledged ({reason}), for {target}: {line}")
        }
        Err(e) => {
            format!("Wake not delivered ({e:#}), for {target}; it stays in the outbox: {line}")
        }
    };
    if sent.is_ok() || first {
        if let Err(e) = &sent {
            eprintln!("horch ledger: the orchestrator was not woken: {e:#}");
        }
        let _ = util::event(
            store.conn(),
            &About {
                run: Some(&run.run_id),
                ..Default::default()
            },
            Actor::Horch,
            EventKind::Woke,
            &note,
        );
    }
    sent.map(|_| ())
}

/// The line of a wake: tagged `[horch]`.
fn tagged(text: &str) -> String {
    if text.starts_with("[horch]") {
        text.to_string()
    } else {
        format!("[horch] {text}")
    }
}

/// The settled wake line of `run`, or `None` while the run is not settled
/// (C6: [`status::unsettled`], the rule `run close` uses).
#[cfg(test)]
pub(crate) fn settled_note(store: &Store, run: &Run) -> anyhow::Result<Option<String>> {
    settled_line(store.conn(), run)
}

fn settled_line(conn: &Connection, run: &Run) -> anyhow::Result<Option<String>> {
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
fn settled_wake(conn: &Connection, run: &Run) -> anyhow::Result<Option<Wake>> {
    let Some(line) = settled_line(conn, run)? else {
        return Ok(None);
    };
    let board = outbox::board(conn, &run.run_id, false)?;
    Ok(Some(
        Wake::new(OutboxEvent::Settled, "run", tagged(&line)).on_board(board),
    ))
}

/// After a task finishes: wake the orchestrator when tasks can start that
/// no live worker is on its way to claim, or when the run is settled.
pub fn after_done(ctx: &RuntimeContext, store: &Store, run: &Run) {
    if wake_as(ctx, store, run, |tx| settled_wake(tx, run)) != Written::Nothing {
        return;
    }
    nudge_startable(ctx, store, run);
    nudge_stuck(ctx, store, run);
}

/// After a task was cancelled or parked unlanded: tell the orchestrator
/// which open tasks now wait for it (#119). No wake when none do. A parked
/// task's wake is `held`, a cancelled task's wake is `cancelled`.
pub fn stalled_dependents(
    ctx: &RuntimeContext,
    store: &Store,
    run: &Run,
    task: &str,
    dependents: &[tasks::Task],
) {
    if dependents.is_empty() {
        return;
    }
    wake_as(ctx, store, run, |tx| {
        let t = tasks::resolve(tx, &run.run_id, task)?;
        Ok(dependents_wake(&t, dependents))
    });
}

/// The wake of blocker `t` that `dependents` wait for, else `None`. The
/// key holds the blocker's state ([`watchdog::blocker_state`]), not the
/// board: the direct wake and the reconciled wake (TRF-51) are 1 row.
fn dependents_wake(t: &tasks::Task, dependents: &[tasks::Task]) -> Option<Wake> {
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

/// The last review of `parent` is in (C3): write its keyed `reviews-in`
/// row, tell the author `role` to run finish, then mark the row delivered.
/// The outbox order is row, send, mark: a crash before the mark sends the
/// row to the orchestrator. A failed tell sends it to the orchestrator now.
pub fn reviews_in(ctx: &RuntimeContext, store: &Store, run: &Run, parent: &str, role: &str) {
    let written = outbox::write_now(store, &run.run_id, |tx| {
        let p = tasks::resolve(tx, &run.run_id, parent)?;
        reviews_in_wake(tx, &p, role).map(Some)
    });
    let id = match written {
        Ok(Written::New(id)) => id,
        Ok(_) => return drain_run(ctx, store, run, None),
        Err(e) => {
            eprintln!("horch ledger: the reviews-in wake was not written: {e:#}");
            return;
        }
    };
    let line = format!("[horch] NOTE: The reviews of {parent} are in. Run horch ledger finish.");
    let drained = outbox::drain(store, Scope::Run(&run.run_id), |row| {
        if row.id != id {
            return send_row(ctx, store, run, row, false);
        }
        match tell_author(ctx, role, &line) {
            Ok(()) => Ok(()),
            Err(e) => {
                eprintln!("horch ledger: could not tell {role}: {e:#}");
                send_row(ctx, store, run, row, true)
            }
        }
    });
    match drained {
        Ok(d) if d.busy => eprintln!(
            "horch ledger: another drain sends the reviews-in wake of {parent} to the orchestrator"
        ),
        Ok(_) => {}
        Err(e) => eprintln!("horch ledger: the outbox drain failed: {e:#}"),
    }
}

/// Tell the author `role` its line. Mail the courier still has to type is
/// not a delivery: the orchestrator is told too. The tell is not a wake, so
/// `HORCH_NO_WAKE` does not stop it (as before TRF-51).
fn tell_author(ctx: &RuntimeContext, role: &str, line: &str) -> anyhow::Result<()> {
    match super::messaging::tell_as(ctx, Some("horch".into()), role, line)? {
        ReportStatus::Delivered => Ok(()),
        ReportStatus::Queued(cause) => Err(anyhow::anyhow!(
            "{cause}; the courier types it when the pane is idle"
        )),
    }
}

/// The `reviews-in` wake of the review round of `parent` ([`watchdog::review_round`]),
/// for the orchestrator: the direct path and the reconcile pass build it alike.
fn reviews_in_wake(conn: &Connection, parent: &tasks::Task, role: &str) -> anyhow::Result<Wake> {
    let p = parent.short();
    let text = format!(
        "NOTE: The reviews of {p} are in. Make sure {role} runs horch ledger finish: horch tell {role} \"[orchestrator] NOTE: The reviews of {p} are in. Run horch ledger finish.\""
    );
    Ok(Wake::new(OutboxEvent::ReviewsIn, p, tagged(&text))
        .on_board(watchdog::review_round(conn, &parent.task_id)?))
}

/// Wake the orchestrator once when the run has open tasks, none can start
/// and no worker is live: nothing else would ever wake it (#119).
pub fn nudge_stuck(ctx: &RuntimeContext, store: &Store, run: &Run) {
    wake_as(ctx, store, run, |tx| {
        let Some(text) = stuck_line(tx, run) else {
            return Ok(None);
        };
        let line = tagged(&text);
        // Once per stuck state: the same line is not sent twice in a row.
        let prefix = format!("{}:{}:stuck", run.run_id, OutboxEvent::Held.as_str());
        if outbox::last_text(tx, &run.run_id, &prefix)?.as_deref() == Some(line.as_str()) {
            return Ok(None);
        }
        let board = outbox::board(tx, &run.run_id, true)?;
        Ok(Some(
            Wake::new(OutboxEvent::Held, "stuck", line).on_board(board),
        ))
    });
}

fn stuck_line(conn: &Connection, run: &Run) -> Option<String> {
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

/// The watchdog's ledger work after its checks: the pre-claim sweep, then
/// the stuck-run wake.
pub fn after_watch(ctx: &RuntimeContext, store: &mut Store, run: &Run) {
    sweep_preclaim(ctx, store, run);
    nudge_stuck(ctx, store, run);
}

/// The reconcile pass at `now` for the wakes that a crash between a ledger
/// commit and its wake loses (TRF-51, review 31 I-03 to I-07). Each wake
/// comes from the board, with the key and the text of its direct path, so
/// a wake that exists writes no new row. No live worker holds these wakes
/// back: each event needs the orchestrator, whoever else works.
pub fn reconcile_lost(ctx: &RuntimeContext, store: &Store, run: &Run, now: &str) {
    if let Err(e) = try_reconcile_lost(ctx, store, run, now) {
        eprintln!("horch ledger watch: the lost-wake read failed: {e:#}");
    }
}

fn try_reconcile_lost(
    ctx: &RuntimeContext,
    store: &Store,
    run: &Run,
    now: &str,
) -> anyhow::Result<()> {
    // I-03: the settled wake, 1 row per settled board state.
    wake_as(ctx, store, run, |tx| settled_wake(tx, run));
    // I-04: a review round in while its author still waits.
    for r in watchdog::reviews_waiting(store.conn(), run, now)? {
        let role = r
            .author
            .role
            .clone()
            .unwrap_or_else(|| r.author.label().to_string());
        wake_as(ctx, store, run, |tx| {
            reviews_in_wake(tx, &r.parent, &role).map(Some)
        });
    }
    // I-05, I-06: a cancelled or parked task that open tasks wait for.
    for t in watchdog::stalled_blockers(store.conn(), run)? {
        wake_as(ctx, store, run, |tx| {
            let t = tasks::get(tx, &t.task_id)?;
            Ok(dependents_wake(&t, &tasks::dependents(tx, &t.task_id)?))
        });
    }
    // I-07: a task the attempt limit holds for the operator.
    for t in watchdog::attempt_held(store.conn(), run)? {
        wake_as(ctx, store, run, |tx| {
            Ok(held_wake(tx, &tasks::get(tx, &t.task_id)?))
        });
    }
    Ok(())
}

/// Wake the orchestrator when workers should be spawned now: 1 row per
/// board state, so a repeat in the same state writes no new row.
pub fn nudge_startable(ctx: &RuntimeContext, store: &Store, run: &Run) {
    wake_as(ctx, store, run, |tx| startable_wake(tx, run));
}

fn startable_wake(conn: &Connection, run: &Run) -> anyhow::Result<Option<Wake>> {
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
fn spawn_advice(conn: &Connection, run: &Run) -> Option<(String, String)> {
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

/// The watchdog's pre-claim sweep (#190): end the rows no process stands
/// for and close their panes, ask about a worker that does not claim, and
/// wake once with the spawns the freed slots allow.
pub fn sweep_preclaim(ctx: &RuntimeContext, store: &mut Store, run: &Run) {
    let swept =
        match store.immediate(|tx| workers::sweep_preclaim(tx, run, &workers::supervisor_alive)) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("horch ledger watch: the pre-claim sweep failed: {e:#}");
                return;
            }
        };
    let herdr = Herdr::with_bin(&ctx.bins.harness.herdr);
    let panes = super::taskledger::HerdrPanes::new(ctx);
    let mut ended = Vec::new();
    let mut ended_ids = Vec::new();
    for p in &swept {
        let pane = p.worker.pane_id.as_deref();
        match &p.ended {
            Some(status) => {
                if let Some(pane) = pane.filter(|id| panes.exists(id)) {
                    if let Err(e) = panes.close(pane) {
                        eprintln!("horch ledger watch: closing pane {pane} failed: {e:#}");
                    }
                }
                ended.push(format!("{} ({status}: {})", p.worker.label(), p.note));
                ended_ids.push(p.worker.worker_id.clone());
            }
            None => {
                let state = pane
                    .and_then(|id| herdr.pane_get(id).ok())
                    .and_then(|p| p.agent_status)
                    .unwrap_or_else(|| "unknown".into());
                let text = format!(
                    "NOTE: {}. Look at its pane {} (herdr status {state}): it may wait at a login, trust or quota prompt.",
                    p.note,
                    pane.unwrap_or("(none)")
                );
                let subject = format!("asked:{}", p.worker.worker_id);
                wake_as(ctx, store, run, |_| {
                    Ok(Some(Wake::new(
                        OutboxEvent::UnclaimedSpawn,
                        subject,
                        tagged(&text),
                    )))
                });
            }
        }
    }
    if !ended.is_empty() {
        let names = ended.join(", ");
        let subject = format!("ended:{}", ended_ids.join(","));
        wake_as(ctx, store, run, |tx| {
            let advice = spawn_advice(tx, run).map(|(_, a)| a).unwrap_or_default();
            let text =
                format!("NOTE: These workers ended before they claimed a task: {names}. {advice}");
            Ok(Some(Wake::new(
                OutboxEvent::UnclaimedSpawn,
                subject,
                tagged(text.trim_end()),
            )))
        });
    }
}

/// The `held` QUESTION wake for the operator when the task of `worker_id`
/// is held after too many attempts (#168), else `None`.
fn held_question(conn: &Connection, worker_id: &str) -> Option<Wake> {
    let w = workers::get(conn, worker_id).ok()?;
    held_wake(conn, &tasks::get(conn, w.task_id.as_deref()?).ok()?)
}

/// The `held` QUESTION wake of `task`, held after too many attempts, else
/// `None`. The key holds its attempts ([`watchdog::held_state`]), not the
/// board: the reap's wake and the reconciled wake (TRF-51) are 1 row.
fn held_wake(conn: &Connection, task: &tasks::Task) -> Option<Wake> {
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

/// After a reap: tell the orchestrator which persona to spawn, and how.
pub fn after_reap(
    ctx: &RuntimeContext,
    store: &Store,
    run: &Run,
    report: &horch_core::taskledger::stop::ReapReport,
) {
    let who = report
        .stop
        .role
        .as_deref()
        .unwrap_or(&report.stop.worker_id);
    let worker_id = report.stop.worker_id.as_str();
    let persona = report.persona.as_deref().unwrap_or("the same persona");
    let reaped = format!(
        "NOTE: {who} was reaped and its task is queued again. Spawn: horch spawn {persona} --replaces {worker_id}"
    );
    wake_as(ctx, store, run, |tx| {
        if let Some(wake) = held_question(tx, worker_id) {
            return Ok(Some(wake));
        }
        Ok(Some(Wake::new(
            OutboxEvent::Reaped,
            worker_id,
            tagged(&reaped),
        )))
    });
}

#[cfg(test)]
mod trf_15_tests {
    use std::path::Path;

    use horch_core::taskledger::runs::{self, NewRun};
    use horch_core::taskledger::tasks::NewTask;

    use super::*;

    /// 1 worktree row: (status, folder on disk, triage decision, settled).
    const CASES: &[(&str, bool, Option<&str>, bool)] = &[
        ("open", true, None, false),
        ("landing", true, None, false),
        ("landed", true, None, false),
        ("cleanup_failed", true, None, false),
        ("unlanded", true, None, false),
        ("unlanded", true, Some("KEEP"), true),
        ("unlanded", false, None, true),
        ("abandoned", true, None, false),
        ("abandoned", false, None, true),
        ("cleaned", false, None, true),
        ("archived", false, None, true),
        ("trashed", false, None, true),
    ];

    #[test]
    fn trf_15_settled_wake_agrees_with_status_and_run_close() {
        for &(status, on_disk, decision, settled) in CASES {
            let dir = tempfile::tempdir().unwrap();
            let mut store = Store::open(&dir.path().join("ledger.sqlite3")).unwrap();
            let run = store
                .immediate(|tx| {
                    runs::create(
                        tx,
                        Path::new("/repo"),
                        &NewRun {
                            slug: "r1".into(),
                            ..Default::default()
                        },
                    )
                })
                .unwrap();
            let t = NewTask::work("alpha", "tickets/alpha.md", "dev");
            let task = store
                .immediate(|tx| tasks::add(tx, &run, &t, Actor::Orchestrator))
                .unwrap();
            let wt = dir.path().join("wt");
            if on_disk {
                std::fs::create_dir(&wt).unwrap();
            }
            store
                .conn()
                .execute(
                    "UPDATE tasks SET status = 'done' WHERE task_id = ?1",
                    [&task.task_id],
                )
                .unwrap();
            store
                .conn()
                .execute(
                    "INSERT INTO worktrees (worktree_id, task_id, repo_root, path, branch, base, status, decision, origin, created_at)
                     VALUES ('wt1', ?1, '/repo', ?2, 'fleet/a', 'main', ?3, ?4, 'native', '2000-01-01T00:00:00Z')",
                    (&task.task_id, wt.to_string_lossy().to_string(), status, decision),
                )
                .unwrap();
            let case = (status, on_disk, decision);
            let note = settled_note(&store, &run).unwrap();
            assert_eq!(note.is_some(), settled, "{case:?}");
            let board = status::report(store.conn(), &run).unwrap();
            assert_eq!(board.complete, settled, "{case:?}");
            let closed = runs::close_run(
                store.conn(),
                &horch_core::taskledger::git::Git::new("git"),
                &run,
                None,
            );
            assert_eq!(closed.is_ok(), settled, "{case:?}: {closed:?}");
        }
    }
}

#[cfg(test)]
mod trf_21_tests {
    use std::path::Path;

    use horch_core::messaging::mail::Mail;
    use horch_core::runtime::MapEnv;
    use horch_core::taskledger::runs::NewRun;
    use horch_core::taskledger::tasks::NewTask;

    use super::*;

    /// A ledger, a run with no workspace, and a context with no workspace
    /// and no herdr pane: a send to the orchestrator fails.
    fn world() -> (tempfile::TempDir, RuntimeContext, Store, Run) {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("ledger.sqlite3");
        let ctx = RuntimeContext::from_env(
            &MapEnv::new("/")
                .with(
                    "HORCH_STATE_DIR",
                    dir.path().join("state").to_str().unwrap(),
                )
                .with("HORCH_LEDGER_DB", db.to_str().unwrap()),
        )
        .unwrap();
        let mut store = Store::open(&db).unwrap();
        let run = store
            .immediate(|tx| {
                runs::create(
                    tx,
                    Path::new("/repo"),
                    &NewRun {
                        slug: "r1".into(),
                        ..Default::default()
                    },
                )
            })
            .unwrap();
        (dir, ctx, store, run)
    }

    fn rows(store: &Store, run: &Run) -> Vec<(String, Option<String>)> {
        let mut stmt = store
            .conn()
            .prepare("SELECT text, delivered_at FROM outbox WHERE run_id = ?1 ORDER BY rowid")
            .unwrap();
        stmt.query_map([&run.run_id], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    }

    fn woke(store: &Store, run: &Run) -> Vec<String> {
        let mut stmt = store
            .conn()
            .prepare(
                "SELECT text FROM events WHERE run_id = ?1 AND kind = 'woke' ORDER BY event_id",
            )
            .unwrap();
        stmt.query_map([&run.run_id], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    }

    /// Give the run the workspace `w1`: a send now reaches durable mail.
    fn give_workspace(store: &Store, run: &Run) -> Run {
        store
            .conn()
            .execute(
                "UPDATE runs SET workspace_id = 'w1' WHERE run_id = ?1",
                [&run.run_id],
            )
            .unwrap();
        runs::get(store.conn(), &run.run_id).unwrap()
    }

    fn orchestrator_mail(ctx: &RuntimeContext) -> Vec<String> {
        let mail = Mail::for_role(&ctx.paths.state_root, "w1", "orchestrator");
        if !mail.dir().is_dir() {
            return Vec::new();
        }
        mail.pending()
            .unwrap()
            .into_iter()
            .map(|m| m.text)
            .collect()
    }

    #[test]
    fn trf_21_a_failed_first_send_leaves_the_row_and_the_next_drain_delivers_it() {
        let (_dir, ctx, store, run) = world();
        wake(&ctx, &store, &run, "NOTE: T-1 needs you.");

        // The first send fails: the row stays, and the failure is recorded.
        let r = rows(&store, &run);
        assert_eq!(r.len(), 1, "{r:?}");
        assert_eq!(r[0].0, "[horch] NOTE: T-1 needs you.");
        assert_eq!(r[0].1, None, "a failed send leaves the row undelivered");
        let w = woke(&store, &run);
        assert_eq!(w.len(), 1, "{w:?}");
        assert!(w[0].starts_with("Wake not delivered ("), "{w:?}");
        assert!(w[0].contains("it stays in the outbox"), "{w:?}");
        assert!(orchestrator_mail(&ctx).is_empty());

        // A drain that fails again records nothing new.
        drain_run(&ctx, &store, &run, None);
        assert_eq!(woke(&store, &run).len(), 1);

        // The next drain after the cause is gone delivers into durable mail.
        let run = give_workspace(&store, &run);
        drain_run(&ctx, &store, &run, None);
        let r = rows(&store, &run);
        assert!(r[0].1.is_some(), "the drain set delivered_at: {r:?}");
        let mail = orchestrator_mail(&ctx);
        assert_eq!(mail.len(), 1, "{mail:?}");
        assert!(mail[0].contains("NOTE: T-1 needs you."), "{mail:?}");
        let w = woke(&store, &run);
        assert_eq!(w.len(), 2, "{w:?}");
        assert!(w[1].contains("the run's workspace w1"), "{w:?}");

        // A delivered row is not sent again.
        drain_run(&ctx, &store, &run, None);
        assert_eq!(orchestrator_mail(&ctx).len(), 1);
    }

    /// A crash after the send queued the mail and before `mark_delivered`
    /// leaves the row undelivered. The next drain sends it again, and the
    /// mailbox still holds 1 pending file with the first mail id.
    #[test]
    fn trf_21_replayed_row_reuses_its_mail() {
        let (_dir, ctx, store, run) = world();
        let run = give_workspace(&store, &run);
        wake(&ctx, &store, &run, "NOTE: T-1 needs you.");
        let mail = Mail::for_role(&ctx.paths.state_root, "w1", "orchestrator");
        let first = mail.pending().unwrap();
        assert_eq!(first.len(), 1, "{first:?}");

        // The crash: the row is undelivered again, and its mail stays.
        store
            .conn()
            .execute(
                "UPDATE outbox SET delivered_at = NULL WHERE run_id = ?1",
                [&run.run_id],
            )
            .unwrap();
        drain_run(&ctx, &store, &run, None);
        assert!(rows(&store, &run).iter().all(|(_, d)| d.is_some()));
        let files = std::fs::read_dir(mail.dir().join("pending"))
            .unwrap()
            .filter(|e| {
                let path = e.as_ref().unwrap().path();
                path.extension().is_some_and(|x| x == "json")
            })
            .count();
        assert_eq!(files, 1, "1 row makes 1 pending file");
        let again = mail.pending().unwrap();
        assert_eq!(again[0].id, first[0].id, "the replay reuses the mail id");

        // A replay after the orchestrator acknowledged the mail queues nothing.
        mail.ack_all().unwrap();
        store
            .conn()
            .execute(
                "UPDATE outbox SET delivered_at = NULL WHERE run_id = ?1",
                [&run.run_id],
            )
            .unwrap();
        drain_run(&ctx, &store, &run, None);
        assert!(mail.pending().unwrap().is_empty());
    }

    #[test]
    fn trf_21_the_courier_drain_delivers_the_rows_of_its_workspace() {
        let (_dir, ctx, store, run) = world();
        wake(&ctx, &store, &run, "NOTE: first.");
        wake(&ctx, &store, &run, "NOTE: second.");
        assert!(rows(&store, &run).iter().all(|(_, d)| d.is_none()));

        // The courier of another workspace takes none of them.
        give_workspace(&store, &run);
        drain_workspace(&ctx, "w2");
        assert!(rows(&store, &run).iter().all(|(_, d)| d.is_none()));

        drain_workspace(&ctx, "w1");
        assert!(rows(&store, &run).iter().all(|(_, d)| d.is_some()));
        let mail = orchestrator_mail(&ctx);
        assert_eq!(mail.len(), 2, "{mail:?}");
        assert!(mail[0].contains("NOTE: first."), "oldest first: {mail:?}");
        assert!(mail[1].contains("NOTE: second."), "{mail:?}");
    }

    #[test]
    fn trf_21_a_repeat_of_the_same_settled_state_writes_no_new_row() {
        let (_dir, ctx, mut store, run) = world();
        let settled = |store: &Store| {
            rows(store, &run)
                .into_iter()
                .filter(|(t, _)| t.contains("All tasks are done"))
                .count()
        };
        let done = |store: &mut Store, slug: &str| {
            let t = NewTask::work(slug, &format!("tickets/{slug}.md"), "dev");
            let task = store
                .immediate(|tx| tasks::add(tx, &run, &t, Actor::Orchestrator))
                .unwrap();
            store
                .conn()
                .execute(
                    "UPDATE tasks SET status = 'done' WHERE task_id = ?1",
                    [&task.task_id],
                )
                .unwrap();
        };
        done(&mut store, "alpha");
        after_done(&ctx, &store, &run);
        after_done(&ctx, &store, &run);
        assert_eq!(settled(&store), 1, "1 row per settle");

        // A new task done is a new settled state: 1 more row.
        done(&mut store, "beta");
        after_done(&ctx, &store, &run);
        after_done(&ctx, &store, &run);
        assert_eq!(settled(&store), 2);
    }

    /// Report 45: a crash after the send published the mail and before
    /// `mark_delivered`. The replay finds the mail of the row published and
    /// pushes nothing: the first send pushed it, and the courier has it.
    #[test]
    fn trf_51_replayed_published_row_pushes_nothing() {
        let (_dir, ctx, store, run) = world();
        let run = give_workspace(&store, &run);
        wake(&ctx, &store, &run, "NOTE: T-1 needs you.");
        let w = woke(&store, &run);
        assert_eq!(w.len(), 1, "{w:?}");
        assert!(
            w[0].starts_with("Wake queued"),
            "the first send pushes, and no pane takes it: {w:?}"
        );

        store
            .conn()
            .execute(
                "UPDATE outbox SET delivered_at = NULL WHERE run_id = ?1",
                [&run.run_id],
            )
            .unwrap();
        drain_run(&ctx, &store, &run, None);
        let w = woke(&store, &run);
        assert_eq!(w.len(), 2, "{w:?}");
        assert!(
            w[1].starts_with("Wake delivered"),
            "the replay tries no push: {w:?}"
        );
        assert!(rows(&store, &run).iter().all(|(_, d)| d.is_some()));
        assert_eq!(orchestrator_mail(&ctx).len(), 1, "1 mail");
    }

    #[test]
    fn trf_21_free_text_wakes_get_an_event_and_are_never_deduplicated() {
        let (_dir, ctx, store, run) = world();
        wake(&ctx, &store, &run, "NOTE: same.");
        wake(&ctx, &store, &run, "NOTE: same.");
        assert_eq!(rows(&store, &run).len(), 2);
        assert_eq!(
            event_of("NOTE: The reviews of T-1 are in, but x was not told."),
            OutboxEvent::ReviewsIn
        );
        assert_eq!(
            event_of("NOTE: w was reaped. Spawn: x"),
            OutboxEvent::Reaped
        );
        assert_eq!(
            event_of("NOTE: worktree triage queued T-1 (FINISH). Spawn a worker for it."),
            OutboxEvent::Startable
        );
        assert_eq!(
            event_of("NOTE: w on T-1 shows no progress"),
            OutboxEvent::Held
        );
    }
}
