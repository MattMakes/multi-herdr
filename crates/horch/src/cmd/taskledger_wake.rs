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
    let sent = if disabled() {
        Err(anyhow::anyhow!("wakes are off (HORCH_NO_WAKE=1)"))
    } else {
        let mut ctx = ctx.clone();
        if ctx.herdr.workspace.is_none() {
            if let Some(ws) = run.workspace_id.as_deref() {
                if let Ok(id) = WorkspaceId::new(ws) {
                    ctx.herdr.workspace = Some(id);
                }
            }
        }
        super::messaging::tell_as(&ctx, Some("horch".into()), "orchestrator", &line).map(|_| ())
    };
    let note = match &sent {
        Ok(()) => format!("Woke the orchestrator: {line}"),
        Err(e) => format!("Wake not delivered ({e:#}): {line}"),
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
