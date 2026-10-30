//! `horch ledger watch`: the watchdog process of a run (#42), and the
//! restart every `horch` command makes when it has gone quiet.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::{bail, Result};
use horch_core::exeid::exe_identity;
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

/// A SIGTERM came: the watchdog loop ends after the pass it runs.
static TERM: AtomicBool = AtomicBool::new(false);

#[cfg(unix)]
extern "C" fn on_term(_: libc::c_int) {
    TERM.store(true, Ordering::SeqCst);
}

/// SIGTERM ends the watchdog loop between passes, never inside one: a pass
/// can be in a reap, which requeues the task before it stops the agent
/// (#216 restart, review of T-3).
fn exit_between_passes() {
    #[cfg(unix)]
    // SAFETY: the handler only stores to an atomic, which is
    // async-signal-safe.
    unsafe {
        let handler = on_term as extern "C" fn(libc::c_int) as libc::sighandler_t;
        libc::signal(libc::SIGTERM, handler);
    }
}

/// Sleep `d`, or less when a SIGTERM comes.
fn sleep_unless_term(d: Duration) {
    let until = std::time::Instant::now() + d;
    while !TERM.load(Ordering::SeqCst) && std::time::Instant::now() < until {
        std::thread::sleep(Duration::from_millis(100));
    }
}

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
    watchdog::holder(&lock_dir(ctx), &run.run_id).map(|h| h.pid)
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
            "suspect" => taskledger_wake::wake(
                ctx,
                store,
                run,
                &format!(
                    "NOTE: {who} on {on} may be stuck in pane {}: {}. The watchdog reaps it at the next check unless something else moves.",
                    c.pane_id.as_deref().unwrap_or("unknown"),
                    c.note
                ),
            ),
            "progress" | "proxy_checkin" if c.changed => {
                // Back to normal after a stall: nothing to do, no wake.
            }
            _ => {}
        }
    }
    taskledger_wake::after_watch(ctx, store, run);
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
    // A slow start, as on a loaded machine: the e2e tests of `ensure` hold
    // a new watchdog before its lock with `HORCH_FAULT=watchdog-start-ms:<n>`.
    if let Some(ms) = ctx
        .settings
        .faults
        .indexed("watchdog-start-ms")
        .and_then(|s| s.parse::<u64>().ok())
    {
        std::thread::sleep(Duration::from_millis(ms.min(120_000)));
    }
    if let Some(pid) = lock_held(ctx, run) {
        if pid != std::process::id() {
            bail!("the watchdog of run {} already runs (PID {pid})", run.slug);
        }
    }
    let guard = watchdog::lock(&lock_dir(ctx), &run.run_id)?;
    let Some(guard) = guard else {
        bail!("the watchdog of run {} already runs", run.slug);
    };
    // The starter compares this with the binary it would start (#216).
    let exe = std::env::current_exe().ok().and_then(|p| exe_identity(&p));
    watchdog::record_exe(&guard, exe.as_ref())?;
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
    exit_between_passes();
    loop {
        let run_now = runs::get(store.conn(), &run.run_id)?;
        if run_now.status != "active" || TERM.load(Ordering::SeqCst) {
            return Ok(ExitCode::SUCCESS);
        }
        if let Err(e) = pass(ctx, &mut store, &run_now, false) {
            eprintln!("horch ledger watch: {e:#}");
        }
        let tick = (run_now.check_every_s / 10).clamp(1, 30) as u64;
        sleep_unless_term(Duration::from_secs(tick));
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
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    for run in all.into_iter().filter(|r| r.status == "active") {
        if let Some(h) = watchdog::holder(&lock_dir(ctx), &run.run_id) {
            // Only a watchdog whose own binary file changed moves, and onto
            // that file: a horch at another path does not take it over.
            if h.binary_replaced() {
                let target = h
                    .exe
                    .as_ref()
                    .map(|e| PathBuf::from(&e.path))
                    .filter(|p| p.is_file())
                    .unwrap_or_else(|| exe.clone());
                replace(ctx, &store, &run, &root, &h, &target, true);
            }
            continue;
        }
        if !watchdog::needs_restart(store.conn(), &run).unwrap_or(false) {
            continue;
        }
        match start_detached(ctx, &store, &run, &root, &exe) {
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

/// After `horch install`: restart the watchdog of every active run that
/// runs another binary than the one just installed at `installed` (#216).
/// Prints 1 line per restarted watchdog. Starts none that is not live.
pub fn restart_if_stale(ctx: &RuntimeContext, installed: &Path) {
    if !ctx.paths.ledger_db.exists() {
        return;
    }
    let Ok(store) = taskledger::open(ctx) else {
        return;
    };
    let Ok(all) = watchdog::active_runs(store.conn()) else {
        return;
    };
    let want = exe_identity(installed);
    for run in all {
        if let Some(h) = watchdog::holder(&lock_dir(ctx), &run.run_id) {
            if h.runs_other_binary(want.as_ref()) {
                let root = PathBuf::from(&run.project_root);
                replace(ctx, &store, &run, &root, &h, installed, false);
            }
        }
    }
}

/// Stop the live watchdog `h` of `run`, which runs another binary, by its
/// exact PID and start time, then start one on `exe`. A watchdog keeps no
/// state between passes, so the restart loses nothing. It ends after its
/// current pass, and a reap in that pass waits up to the stop grace.
fn replace(
    ctx: &RuntimeContext,
    store: &Store,
    run: &Run,
    root: &Path,
    h: &watchdog::LockHolder,
    exe: &Path,
    as_warning: bool,
) {
    if let Err(e) = h.stop(stop_grace() + Duration::from_secs(5)) {
        eprintln!(
            "horch: warning: the watchdog of run {} (PID {}) runs another build of horch, but horch did not stop it: {e:#}",
            run.slug, h.pid
        );
        return;
    }
    let line = match start_detached(ctx, store, run, root, exe) {
        Ok(pid) => format!(
            "restarted the watchdog of run {} on {}: PID {} ran another build of horch, PID {pid} runs the new one",
            run.slug,
            exe.display(),
            h.pid
        ),
        Err(e) => {
            eprintln!(
                "horch: warning: stopped the old watchdog of run {}, but could not start a new one: {e:#}",
                run.slug
            );
            return;
        }
    };
    if as_warning {
        eprintln!("horch: warning: {line}");
    } else {
        output::println(&line);
    }
}

/// `horch ledger watch --run <id>` of `exe`, detached in its own session so
/// a pane close does not end it. Its output goes to
/// `<ledger dir>/watchdog-<run>.log`. The run records the new process at
/// once, so a command before its first checkin does not start another.
fn start_detached(
    ctx: &RuntimeContext,
    store: &Store,
    run: &Run,
    root: &Path,
    exe: &Path,
) -> Result<u32> {
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
        .env("HORCH_PROJECT_DIR", root)
        .env("HORCH_LEDGER_DB", &ctx.paths.ledger_db)
        .stdin(std::process::Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log);
    if let Some(ws) = &run.workspace_id {
        cmd.env("HORCH_WORKSPACE_ID", ws);
    }
    let child = horch_core::runtime::process::spawn_detached(&mut cmd)?;
    let pid = child.id();
    // It runs either way; without the record, the next command can start a
    // second one, which finds the lock taken and ends.
    if let Err(e) = watchdog::record_start(
        store.conn(),
        &run.run_id,
        pid,
        horch_core::procid::start_time(pid),
    ) {
        eprintln!(
            "horch: warning: the run {} did not record its new watchdog (PID {pid}): {e:#}",
            run.slug
        );
    }
    Ok(pid)
}
