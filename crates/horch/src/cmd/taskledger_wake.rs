//! Waking the orchestrator from the task ledger (section 9): 1 line through
//! the channel relay and durable mail, as `horch tell orchestrator` sends,
//! tagged `[horch]`. A wake never fails the ledger operation that sent it.

use horch_core::ids::WorkspaceId;
use horch_core::runtime::RuntimeContext;
use horch_core::taskledger::model::{Actor, EventKind};
use horch_core::taskledger::runs::Run;
use horch_core::taskledger::stop::Panes;
use horch_core::taskledger::tasks;
use horch_core::taskledger::util::{self, About};
use horch_core::taskledger::workers;
use horch_core::taskledger::Store;
use horch_core::workspace::herdr::Herdr;

/// `HORCH_NO_WAKE=1` turns wakes off (tests that run no herdr).
fn disabled() -> bool {
    std::env::var("HORCH_NO_WAKE").is_ok_and(|v| v == "1")
}

/// Send `text` to the run's orchestrator, and record it. Best effort: a
/// failure is printed and recorded, never returned.
pub fn wake(ctx: &RuntimeContext, store: &Store, run: &Run, text: &str) {
    let line = if text.starts_with("[horch]") {
        text.to_string()
    } else {
        format!("[horch] {text}")
    };
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
        super::messaging::tell_as(&ctx, Some("horch".into()), "orchestrator", &line)
    };
    let note = match &sent {
        Ok(horch_core::execution::lifecycle::ReportStatus::Delivered) => {
            format!("Wake delivered to {target}: {line}")
        }
        Ok(horch_core::execution::lifecycle::ReportStatus::Queued(reason)) => {
            format!("Wake queued, not yet acknowledged ({reason}), for {target}: {line}")
        }
        Err(e) => format!("Wake not delivered ({e:#}), for {target}: {line}"),
    };
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

/// After a task finishes: wake the orchestrator when tasks can start that
/// no live worker is on its way to claim, or when the run is complete.
pub fn after_done(ctx: &RuntimeContext, store: &Store, run: &Run) {
    match tasks::all_finished(store.conn(), &run.run_id) {
        Ok(true) => {
            let open_worktrees: i64 = store
                .conn()
                .query_row(
                    "SELECT count(*) FROM worktrees w JOIN tasks t ON t.task_id = w.task_id
                     WHERE t.run_id = ?1 AND w.status NOT IN ('cleaned', 'unlanded', 'abandoned')",
                    [&run.run_id],
                    |r| r.get(0),
                )
                .unwrap_or(0);
            if open_worktrees == 0 {
                wake(
                    ctx,
                    store,
                    run,
                    &format!(
                        "NOTE: All tasks are done. Run horch ledger status, verify the run, then close it with horch ledger run close --run {} and report to the operator.",
                        run.slug
                    ),
                );
                return;
            }
        }
        Ok(false) => {}
        Err(e) => eprintln!("horch ledger: {e:#}"),
    }
    nudge_startable(ctx, store, run);
    nudge_stuck(ctx, store, run);
}

/// After a task was cancelled (or parked unlanded): tell the orchestrator
/// which open tasks now wait for it (#119). No wake when none do.
pub fn cancelled_dependents(
    ctx: &RuntimeContext,
    store: &Store,
    run: &Run,
    task: &str,
    dependents: &[tasks::Task],
) {
    let Some(first) = dependents.first() else {
        return;
    };
    let names: Vec<String> = dependents.iter().map(|t| t.short()).collect();
    wake(
        ctx,
        store,
        run,
        &format!(
            "NOTE: {task} was cancelled. {} wait for it. Run horch ledger unblock {} --from {task}, or cancel them, or requeue {task}.",
            names.join(", "),
            first.short()
        ),
    );
}

/// Wake the orchestrator once when the run has open tasks, none can start
/// and no worker is live: nothing else would ever wake it (#119).
pub fn nudge_stuck(ctx: &RuntimeContext, store: &Store, run: &Run) {
    let Some(text) = stuck_line(store, run) else {
        return;
    };
    // Once per stuck state: the same line is not sent twice in a row.
    let last: Option<String> = store
        .conn()
        .query_row(
            "SELECT text FROM events WHERE run_id = ?1 AND kind = 'woke'
               AND text LIKE '%the run is stuck:%' ORDER BY event_id DESC LIMIT 1",
            [&run.run_id],
            |r| r.get(0),
        )
        .ok();
    if last.is_some_and(|l| l.ends_with(&text)) {
        return;
    }
    wake(ctx, store, run, &text);
}

fn stuck_line(store: &Store, run: &Run) -> Option<String> {
    let conn = store.conn();
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

/// Wake the orchestrator when workers should be spawned now.
pub fn nudge_startable(ctx: &RuntimeContext, store: &Store, run: &Run) {
    if let Some(advice) = spawn_advice(store, run) {
        wake(ctx, store, run, &format!("NOTE: {advice}"));
    }
}

/// "Tasks can start: T-1. Spawn: opus x1." when workers should be spawned
/// now, else `None`.
fn spawn_advice(store: &Store, run: &Run) -> Option<String> {
    let plan = tasks::plan_workers(store.conn(), &run.run_id).ok()?;
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
    let ready: Vec<String> = tasks::startable(store.conn(), &run.run_id)
        .map(|v| v.iter().map(|t| t.short()).collect())
        .unwrap_or_default();
    Some(format!(
        "Tasks can start: {}. Spawn: {}.",
        ready.join(", "),
        spawn.join(", ")
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
            }
            None => {
                let state = pane
                    .and_then(|id| herdr.pane_get(id).ok())
                    .and_then(|p| p.agent_status)
                    .unwrap_or_else(|| "unknown".into());
                wake(
                    ctx,
                    store,
                    run,
                    &format!(
                        "NOTE: {}. Look at its pane {} (herdr status {state}): it may wait at a login, trust or quota prompt.",
                        p.note,
                        pane.unwrap_or("(none)")
                    ),
                );
            }
        }
    }
    if !ended.is_empty() {
        let advice = spawn_advice(store, run).unwrap_or_default();
        wake(
            ctx,
            store,
            run,
            format!(
                "NOTE: These workers ended before they claimed a task: {}. {advice}",
                ended.join(", ")
            )
            .trim_end(),
        );
    }
}

/// The QUESTION for the operator when the task of `worker_id` is held after
/// too many attempts (#168), else `None`.
fn held_question(store: &Store, worker_id: &str) -> Option<String> {
    let w = workers::get(store.conn(), worker_id).ok()?;
    let task = tasks::get(store.conn(), w.task_id.as_deref()?).ok()?;
    let held = task.held_reason.as_deref()?;
    let ends = tasks::attempt_ends(store.conn(), &task).unwrap_or_default();
    Some(format!(
        "QUESTION: {t} is held: {held} ({}). Ask the operator: requeue it (horch ledger requeue {t} \"<reason>\"), change the ticket, or cancel it (horch ledger cancel {t} \"<reason>\"). Spawn no replacement.",
        ends.join("; "),
        t = task.short()
    ))
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
    if let Some(question) = held_question(store, &report.stop.worker_id) {
        wake(ctx, store, run, &question);
        return;
    }
    let persona = report.persona.as_deref().unwrap_or("the same persona");
    wake(
        ctx,
        store,
        run,
        &format!(
            "NOTE: {who} was reaped and its task is queued again. Spawn: horch spawn {persona} --replaces {}",
            report.stop.worker_id
        ),
    );
}
