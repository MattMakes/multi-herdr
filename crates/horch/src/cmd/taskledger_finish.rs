//! `horch ledger finish` (#160 phase 2). In the worker's own pane, where
//! horch mail reaches it, a plain finish starts `finish --run-steps` as a
//! detached process and returns at once; the detached run sends the result
//! to the worker as durable mail. Elsewhere, and with `--json` or `--wait`,
//! the steps run in the foreground.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

use anyhow::{bail, Context, Result};
use horch_core::messaging::mailbox::Mailbox;
use horch_core::runtime::RuntimeContext;
use horch_core::taskledger::finish::{self, detach, land};
use horch_core::taskledger::model::StopRequester;
use horch_core::taskledger::runs;
use horch_core::taskledger::stop;
use horch_core::taskledger::workers;
use horch_core::taskledger::Store;
use horch_core::workspace::herdr::Herdr;

use super::taskledger::{
    codex_personas, exit_of, json, reviewers_down, stop_grace, tools, worker_id, HerdrPanes,
};
use crate::output;

/// How `horch ledger finish` was called.
pub struct How {
    pub as_json: bool,
    pub rereview: bool,
    pub wait: bool,
    pub run_steps: bool,
}

/// The sender tag of horch's own notices.
const HORCH: &str = "horch";

/// `horch ledger finish`.
pub fn finish_cmd(ctx: &RuntimeContext, store: &mut Store, how: How) -> Result<ExitCode> {
    let wid = worker_id(ctx)?;
    let run = run_file(ctx, store, &wid)?;
    if how.run_steps {
        return detached_run(ctx, store, &wid, run.as_deref(), how.rereview);
    }
    answer_reviews_in(ctx, store, &wid)?;
    if let Some(r) = run.as_deref().and_then(|r| still_running(store, &wid, r)) {
        output::println(&runs_line(r.pid));
        return Ok(ExitCode::SUCCESS);
    }
    match run.filter(|_| detaches(ctx, store, &wid, &how)) {
        Some(run) => start_detached(ctx, store, &wid, &run, how.rereview),
        None => foreground(ctx, store, &wid, how.as_json, how.rereview),
    }
}

/// The run file of the worker's attempt; `None` when it holds no task.
fn run_file(ctx: &RuntimeContext, store: &Store, wid: &str) -> Result<Option<PathBuf>> {
    let w = workers::get(store.conn(), wid)?;
    Ok(w.task_id
        .map(|t| detach::run_path(&tools(ctx).evidence_root, &t, wid)))
}

fn runs_line(pid: u32) -> String {
    format!("finish runs (PID {pid}); horch tells you the result. End your turn now")
}

/// The detached finish that still runs. A lost one is reported and
/// forgotten, so finish starts again.
fn still_running(store: &Store, wid: &str, run: &Path) -> Option<detach::RunFile> {
    match detach::state(run) {
        detach::Detached::Running(r) => Some(r),
        detach::Detached::Lost(r) => {
            output::println(&format!(
                "the detached finish (PID {}) ended without a result; finish runs again",
                r.pid
            ));
            detach::clear(run);
            if let Err(e) = workers::clear_wait(store.conn(), wid, detach::FINISH_WAIT) {
                eprintln!("horch ledger: could not end the finish wait: {e:#}");
            }
            None
        }
        detach::Detached::Idle => None,
    }
}

/// Whether a plain finish detaches: only in the worker's own registered
/// pane, where horch mail reaches it. Otherwise 1 line says why not.
fn detaches(ctx: &RuntimeContext, store: &Store, wid: &str, how: &How) -> bool {
    if how.as_json || how.wait {
        return false;
    }
    match mail_unreachable(ctx, store, wid) {
        None => true,
        Some(why) => {
            eprintln!("horch ledger: finish runs in the foreground: {why}");
            false
        }
    }
}

/// Why horch mail to the worker would not reach this pane, if it would not.
fn mail_unreachable(ctx: &RuntimeContext, store: &Store, wid: &str) -> Option<String> {
    let role = match workers::get(store.conn(), wid) {
        Ok(w) => w.role,
        Err(e) => return Some(format!("{e:#}")),
    };
    let Some(role) = role else {
        return Some("this worker has no role".into());
    };
    let herdr = Herdr::with_bin(&ctx.bins.harness.herdr);
    let registered = Mailbox::resolve_in(&herdr, ctx)
        .ok()
        .and_then(|m| m.pane_for(&role));
    let here = ctx.herdr.pane.as_ref().map(|p| p.as_str().to_string());
    if registered.is_some() && registered == here {
        None
    } else {
        Some(format!(
            "this command does not run in the registered pane of {role}, so horch mail cannot reach it"
        ))
    }
}

/// Start the detached run, record it, and cover the idle worker with a
/// wait (#70) as long as 3 checks and the rest of the steps can take.
fn start_detached(
    ctx: &RuntimeContext,
    store: &Store,
    wid: &str,
    run: &Path,
    rereview: bool,
) -> Result<ExitCode> {
    let child = spawn_run_steps(run, rereview)?;
    let recorded = detach::record(run, child.id())?;
    let task = workers::get(store.conn(), wid)?
        .task_id
        .context("this worker holds no task")?;
    let check = land::check_timeout(&tools(ctx).evidence_root, &task);
    workers::set_wait(
        store.conn(),
        wid,
        detach::FINISH_WAIT,
        &format!("finish runs (PID {})", recorded.pid),
        &[],
        3 * check.as_secs() as i64 + 3600,
    )?;
    output::println(&runs_line(recorded.pid));
    Ok(ExitCode::SUCCESS)
}

/// `horch ledger finish --run-steps` in its own process group, its output
/// in the log beside the run file.
fn spawn_run_steps(run: &Path, rereview: bool) -> Result<std::process::Child> {
    std::fs::create_dir_all(run.parent().expect("a worker folder"))?;
    let log = std::fs::File::create(detach::log_path(run))?;
    let mut cmd = Command::new(std::env::current_exe().context("finding the horch binary")?);
    cmd.args(["ledger", "finish", "--run-steps"]);
    if rereview {
        cmd.arg("--rereview");
    }
    cmd.stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log);
    // Its own group: the signal that ends the agent's command does not end it.
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
    cmd.spawn().context("starting the detached finish")
}

/// The detached run: the steps, then the result as durable horch mail to
/// the worker's role. Its wait and run file end with it.
fn detached_run(
    ctx: &RuntimeContext,
    store: &mut Store,
    wid: &str,
    run: Option<&Path>,
    rereview: bool,
) -> Result<ExitCode> {
    let run = run.context("this worker holds no task")?;
    if !detach::wait_for_record(run) {
        bail!("{} does not name this process; nothing ran", run.display());
    }
    let role = workers::get(store.conn(), wid)?
        .role
        .context("this worker has no role")?;
    let text = match steps(ctx, store, wid, rereview) {
        Ok(report) => result_note(&report),
        Err(e) => format!(
            "NOTE: finish stopped with an error: {e:#}. Fix it, then run horch ledger finish again."
        ),
    };
    let told = super::messaging::tell_as(ctx, Some(HORCH.into()), &role, &text);
    workers::clear_wait(store.conn(), wid, detach::FINISH_WAIT)?;
    detach::clear(run);
    told.map(|_| ExitCode::SUCCESS)
}

/// The mail of a finished detached run.
fn result_note(report: &finish::FinishReport) -> String {
    let steps: Vec<String> = report
        .steps
        .iter()
        .map(|s| format!("{} {}", s.name, s.status))
        .collect();
    format!(
        "NOTE: The finish of {} ended. {} Steps: {}.",
        report.task,
        report.message,
        steps.join(", ")
    )
}

/// Run the steps, then stop superseded reviewers and wake the orchestrator
/// when reviews were queued.
fn steps(
    ctx: &RuntimeContext,
    store: &mut Store,
    wid: &str,
    rereview: bool,
) -> Result<finish::FinishReport> {
    let report = run_finish(ctx, store, wid, rereview)?;
    stop_superseded(ctx, store, &report.superseded_workers);
    if !report.queued_reviews.is_empty() {
        nudge_run_of(ctx, store, wid)?;
    }
    Ok(report)
}

/// The steps in this process; the exit code is 1 while a step has not
/// passed.
fn foreground(
    ctx: &RuntimeContext,
    store: &mut Store,
    wid: &str,
    as_json: bool,
    rereview: bool,
) -> Result<ExitCode> {
    let report = steps(ctx, store, wid, rereview)?;
    print_finish(&report, as_json)
}

/// Start a new code-sweep round when asked, then run the finish steps.
fn run_finish(
    ctx: &RuntimeContext,
    store: &mut Store,
    wid: &str,
    rereview: bool,
) -> Result<finish::FinishReport> {
    let mut tools = tools(ctx);
    tools.reviewers_down = reviewers_down(ctx);
    tools.codex.personas = codex_personas(ctx);
    if rereview {
        finish::rereview(store, &tools, wid)?;
    }
    finish::finish(store, &tools, wid)
}

/// finish answers a reviews-in notice: its mail and wait end.
fn answer_reviews_in(ctx: &RuntimeContext, store: &Store, wid: &str) -> Result<()> {
    if workers::waiting(store.conn(), wid)?
        .iter()
        .any(workers::holds_for_finish)
    {
        workers::clear_wait(store.conn(), wid, workers::REVIEWS_WAIT)?;
    }
    if let Some(role) = workers::get(store.conn(), wid)?.role {
        super::messaging::ack_role_mail(ctx, &role);
    }
    Ok(())
}

/// Wake the orchestrator of the worker's run about the tasks that can start.
fn nudge_run_of(ctx: &RuntimeContext, store: &Store, wid: &str) -> Result<()> {
    let w = workers::get(store.conn(), wid)?;
    if let Some(r) = &w.run_id {
        let run = runs::get(store.conn(), r)?;
        super::taskledger_wake::nudge_startable(ctx, store, &run);
    }
    Ok(())
}

/// Stop the reviewers whose review a new code-sweep round superseded.
fn stop_superseded(ctx: &RuntimeContext, store: &mut Store, superseded: &[String]) {
    for w in superseded {
        let stopped = stop::stop_worker(
            store,
            w,
            StopRequester::Watchdog,
            "its review was superseded by a new code-sweep round",
            &HerdrPanes::new(ctx),
            &stop::Os,
            &stop::StopOptions {
                grace: stop_grace(),
                ..Default::default()
            },
        );
        if let Err(e) = stopped {
            eprintln!("horch ledger: could not stop reviewer {w}: {e:#}");
        }
    }
}

/// Print the steps; the exit code is 1 while a step has not passed.
fn print_finish(report: &finish::FinishReport, as_json: bool) -> Result<ExitCode> {
    if as_json {
        json(report)?;
        return Ok(exit_of(report.complete));
    }
    for st in &report.steps {
        output::println(&format!(
            "{:<2} {:<17} {:<15} {}",
            st.seq,
            st.name,
            st.status,
            st.verdict.as_deref().unwrap_or("")
        ));
    }
    output::println(&report.message);
    Ok(exit_of(report.complete))
}
