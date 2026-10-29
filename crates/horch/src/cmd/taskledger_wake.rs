//! Waking the orchestrator from the task ledger (section 9): 1 line through
//! the channel relay and durable mail, as `horch tell orchestrator` sends,
//! tagged `[horch]`. A wake never fails the ledger operation that sent it.

use horch_core::ids::WorkspaceId;
use horch_core::runtime::RuntimeContext;
use horch_core::taskledger::model::{Actor, EventKind};
use horch_core::taskledger::runs::Run;
use horch_core::taskledger::tasks;
use horch_core::taskledger::util::{self, About};
use horch_core::taskledger::Store;

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
            format!("Wake queued for {target}, not yet acknowledged ({reason}): {line}")
        }
        Err(e) => format!("Wake for {target} not delivered ({e:#}): {line}"),
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
                    "NOTE: All tasks are done. Run horch ledger status, verify the run, then report to the operator.",
                );
                return;
            }
        }
        Ok(false) => {}
        Err(e) => eprintln!("horch ledger: {e:#}"),
    }
    nudge_startable(ctx, store, run);
}

/// Wake the orchestrator when workers should be spawned now.
pub fn nudge_startable(ctx: &RuntimeContext, store: &Store, run: &Run) {
    let Ok(plan) = tasks::plan_workers(store.conn(), &run.run_id) else {
        return;
    };
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
        return;
    }
    let ready: Vec<String> = tasks::startable(store.conn(), &run.run_id)
        .map(|v| v.iter().map(|t| t.short()).collect())
        .unwrap_or_default();
    wake(
        ctx,
        store,
        run,
        &format!(
            "NOTE: Tasks can start: {}. Spawn: {}.",
            ready.join(", "),
            spawn.join(", ")
        ),
    );
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
