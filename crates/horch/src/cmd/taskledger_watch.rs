//! `horch ledger watch`: the watchdog process of a run (#42), and the
//! restart every `horch` command makes when it has gone quiet.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use anyhow::{bail, Result};
use horch_core::runtime::RuntimeContext;
use horch_core::taskledger::finish::land;
use horch_core::taskledger::git::Git;
use horch_core::taskledger::model::StopRequester;
use horch_core::taskledger::runs::{self, Run};
use horch_core::taskledger::stop;
use horch_core::taskledger::watchdog::{self, Observation, Observer};
use horch_core::taskledger::workers::Worker;
use horch_core::taskledger::{self, Store};
use horch_core::workspace::herdr::Herdr;

use super::taskledger::{stop_grace, HerdrPanes};
use super::taskledger_wake;
use crate::output;

/// What the watchdog reads: herdr for the pane, the disk for the transcript.
struct Live {
    herdr: Herdr,
}

impl Observer for Live {
    fn observe(&self, w: &Worker) -> Observation {
        let agent_alive = match (w.agent_pid, w.agent_started) {
            (Some(p), Some(s)) => horch_core::procid::is_same(p as u32, s as u64),
            (Some(p), None) => horch_core::procid::alive(p as u32, None),
            _ => false,
        };
        let pane = w
            .pane_id
            .as_deref()
            .and_then(|p| self.herdr.pane_get(p).ok());
        let screen = w
            .pane_id
            .as_deref()
            .and_then(|p| self.herdr.pane_read(p, "visible").ok());
        let meta = w
            .transcript_path
            .as_deref()
            .and_then(|t| std::fs::metadata(t).ok());
        Observation {
            agent_alive,
            herdr_status: pane.as_ref().and_then(|p| p.agent_status.clone()),
            herdr_seq: pane.as_ref().and_then(|p| p.state_change_seq),
            transcript_bytes: meta.as_ref().map(|m| m.len() as i64),
            transcript_mtime: meta
                .and_then(|m| m.modified().ok())
                .map(|t| horch_core::clock::stamp(chrono::DateTime::<chrono::Utc>::from(t))),
            screen,
        }
    }
}

/// The single-instance lock of a run's watchdog.
fn lock_dir(ctx: &RuntimeContext) -> PathBuf {
    ctx.paths
        .ledger_db
        .parent()
        .map(|p| p.join("locks"))
        .unwrap_or_else(|| PathBuf::from("locks"))
}

fn lock_held(ctx: &RuntimeContext, run: &Run) -> Option<u32> {
    let owner = lock_dir(ctx)
        .join(format!("watchdog-{}.lock", run.run_id))
        .join("owner");
    let v: serde_json::Value = serde_json::from_slice(&std::fs::read(owner).ok()?).ok()?;
    let pid = v["pid"].as_u64()? as u32;
    let started = v["started"].as_u64();
    horch_core::procid::alive(pid, started).then_some(pid)
}

/// 1 pass: check, reap the dead, wake on changes, retry cleanups.
fn pass(
    ctx: &RuntimeContext,
    store: &mut Store,
    run: &Run,
    force: bool,
) -> Result<Vec<watchdog::Checked>> {
    let me = std::process::id();
    watchdog::checkin(
        store.conn(),
        &run.run_id,
        me,
        horch_core::procid::start_time(me),
    )?;
    let live = Live {
        herdr: Herdr::with_bin(&ctx.bins.harness.herdr),
    };
    if let Some(workspace) = run.workspace_id.as_deref() {
        horch_core::taskledger::workers::sync_questions(
            store.conn(),
            &ctx.paths.state_root,
            workspace,
        )?;
    }
    let checked = watchdog::check_run(store, run, &live, force)?;
    // 1 push budget for every wake of this pass (#67, #177): an orchestrator
    // pane that is not ready holds the pass no longer, and the courier
    // delivers the queued wakes.
    let _budget = horch_core::messaging::delivery::start_push_budget(
        horch_core::messaging::delivery::DONE_PUSH_BUDGET,
    );
    for c in &checked {
        let who = c.role.as_deref().unwrap_or(&c.worker_id);
        let on = c.task.as_deref().unwrap_or("its task");
        match c.verdict.as_str() {
            "dead" => {
                let panes = HerdrPanes::new(ctx);
                match stop::reap(store, &c.worker_id, StopRequester::Watchdog, &c.note, &panes, &stop::Os, stop_grace()) {
                    Ok(report) => taskledger_wake::after_reap(ctx, store, run, &report),
                    Err(e) => eprintln!("horch ledger watch: reaping {who} failed: {e:#}"),
                }
            }
            "no_progress" if c.first_stall => taskledger_wake::wake(
                ctx,
                store,
                run,
                &format!(
                    "NOTE: {who} on {on} shows no progress ({}). Ask it for an update: horch tell {who} \"[orchestrator] QUESTION: What is your status?\"",
                    c.note
                ),
            ),
            "progress" | "proxy_checkin" if c.changed => {
                // Back to normal after a stall: nothing to do, no wake.
            }
            _ => {}
        }
    }
    let git = Git::new(&ctx.bins.harness.git);
    for (path, o) in land::retry_cleanup(store, &git)? {
        eprintln!("horch ledger watch: cleanup of {path}: {}", o.note);
    }
    Ok(checked)
}

/// `horch ledger watch`.
pub fn watch(
    ctx: &RuntimeContext,
    mut store: Store,
    run: &Run,
    once: bool,
    force: bool,
    as_json: bool,
) -> Result<ExitCode> {
    if let Some(pid) = lock_held(ctx, run) {
        if pid != std::process::id() {
            bail!("the watchdog of run {} already runs (PID {pid})", run.slug);
        }
    }
    let guard = watchdog::lock(&lock_dir(ctx), &run.run_id)?;
    let Some(_guard) = guard else {
        bail!("the watchdog of run {} already runs", run.slug);
    };
    if once {
        let checked = pass(ctx, &mut store, run, force)?;
        if as_json {
            output::println(&serde_json::to_string_pretty(&checked)?);
        } else if checked.is_empty() {
            output::println("No worker was due for a check.");
        } else {
            for c in &checked {
                output::println(&format!(
                    "{} {}: {} (round {}): {}",
                    c.role.as_deref().unwrap_or(&c.worker_id),
                    c.task.as_deref().unwrap_or("-"),
                    c.verdict,
                    c.round,
                    c.note
                ));
            }
        }
        return Ok(ExitCode::SUCCESS);
    }
    // The loop: a pass on every tick; each worker is checked when it is due.
    loop {
        let run_now = runs::get(store.conn(), &run.run_id)?;
        if run_now.status != "active" {
            return Ok(ExitCode::SUCCESS);
        }
        if let Err(e) = pass(ctx, &mut store, &run_now, false) {
            eprintln!("horch ledger watch: {e:#}");
        }
        let tick = (run_now.check_every_s / 10).clamp(1, 30) as u64;
        std::thread::sleep(Duration::from_secs(tick));
    }
}

/// Restart the watchdog of each active run of this project that has gone
/// quiet while a task runs. Best effort, silent unless it restarts one.
pub fn ensure(ctx: &RuntimeContext) {
    if std::env::var("HORCH_NO_WATCHDOG").is_ok_and(|v| v == "1") {
        return;
    }
    // Never create the ledger just to look.
    if !ctx.paths.ledger_db.exists() {
        return;
    }
    let Ok(root) = super::taskledger::project_root(ctx) else {
        return;
    };
    let Ok(store) = taskledger::open(ctx) else {
        return;
    };
    let Ok(all) = runs::of_project(store.conn(), &root) else {
        return;
    };
    for run in all.into_iter().filter(|r| r.status == "active") {
        if lock_held(ctx, &run).is_some() {
            continue;
        }
        if !watchdog::needs_restart(store.conn(), &run).unwrap_or(false) {
            continue;
        }
        match start_detached(ctx, &run, &root) {
            Ok(pid) => eprintln!(
                "horch: warning: the watchdog of run {} was not running; started it (PID {pid})",
                run.slug
            ),
            Err(e) => eprintln!(
                "horch: warning: could not start the watchdog of run {}: {e:#}",
                run.slug
            ),
        }
    }
}

/// `horch ledger watch --run <id>`, detached in its own session so a pane
/// close does not end it. Its output goes to `<ledger dir>/watchdog-<run>.log`.
fn start_detached(ctx: &RuntimeContext, run: &Run, root: &Path) -> Result<u32> {
    let exe = std::env::current_exe()?;
    let log_path = ctx
        .paths
        .ledger_db
        .parent()
        .map(|p| p.join(format!("watchdog-{}.log", run.run_id)))
        .unwrap_or_else(|| PathBuf::from("watchdog.log"));
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)?;
    let mut cmd = std::process::Command::new(exe);
    cmd.args(["ledger", "watch", "--run", &run.run_id])
        .current_dir(root)
        .env_remove("ANTHROPIC_API_KEY")
        .env("HORCH_PROJECT_DIR", root)
        .env("HORCH_LEDGER_DB", &ctx.paths.ledger_db)
        .stdin(std::process::Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log);
    if let Some(ws) = &run.workspace_id {
        cmd.env("HORCH_WORKSPACE_ID", ws);
    }
    let child = horch_core::runtime::process::spawn_detached(&mut cmd)?;
    Ok(child.id())
}
