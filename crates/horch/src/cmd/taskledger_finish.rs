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
use horch_core::workspace::PaneHost;

use super::taskledger::{
    codex_personas, exit_of, json, reviewers_down, stop_grace, tools, worker_id, HerdrPanes,
};
use crate::output;

pub struct How {
    pub as_json: bool,
    pub rereview: bool,
    pub wait: bool,
    pub run_steps: bool,
}

const HORCH: &str = "horch";

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

fn run_file(ctx: &RuntimeContext, store: &Store, wid: &str) -> Result<Option<PathBuf>> {
    let w = workers::get(store.conn(), wid)?;
    Ok(w.task_id
        .map(|t| detach::run_path(&tools(ctx).evidence_root, &t, wid)))
}

fn runs_line(pid: u32) -> String {
    format!("finish runs (PID {pid}); horch tells you the result. End your turn now")
}

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

fn mail_unreachable(ctx: &RuntimeContext, store: &Store, wid: &str) -> Option<String> {
    let w = match workers::get(store.conn(), wid) {
        Ok(w) => w,
        Err(e) => return Some(format!("{e:#}")),
    };
    let Some(role) = w.role else {
        return Some("this worker has no role".into());
    };
    let herdr = PaneHost::for_backend(ctx, w.backend);
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

fn start_detached(
    ctx: &RuntimeContext,
    store: &Store,
    wid: &str,
    run: &Path,
    rereview: bool,
) -> Result<ExitCode> {
    let task = workers::get(store.conn(), wid)?
        .task_id
        .context("this worker holds no task")?;
    let check = land::check_timeout(&tools(ctx).evidence_root, &task);
    workers::set_wait(
        store.conn(),
        wid,
        detach::FINISH_WAIT,
        "horch ledger finish runs detached",
        &[],
        3 * check.as_secs() as i64 + 3600,
    )?;
    let recorded = spawn_run_steps(run, rereview).and_then(|child| detach::record(run, child.id()));
    if recorded.is_err() {
        workers::clear_wait(store.conn(), wid, detach::FINISH_WAIT)?;
    }
    let recorded = recorded?;
    output::println(&runs_line(recorded.pid));
    Ok(ExitCode::SUCCESS)
}

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
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
    cmd.spawn().context("starting the detached finish")
}

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
    detach::clear(run);
    workers::clear_wait(store.conn(), wid, detach::FINISH_WAIT)?;
    super::messaging::tell_as(ctx, Some(HORCH.into()), &role, &text).map(|_| ExitCode::SUCCESS)
}

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

fn steps(
    ctx: &RuntimeContext,
    store: &mut Store,
    wid: &str,
    rereview: bool,
) -> Result<finish::FinishReport> {
    let report = run_finish(ctx, store, wid, rereview)?;
    stop_superseded(ctx, store, &report.superseded_workers);
    if !report.queued_reviews.is_empty() {
        spawn_reviewers_of(ctx, store, wid, &report);
        nudge_run_of(ctx, store, wid)?;
    }
    Ok(report)
}

fn spawn_reviewers_of(
    ctx: &RuntimeContext,
    store: &Store,
    wid: &str,
    report: &finish::FinishReport,
) {
    if ctx.herdr.pane.is_none() {
        return;
    }
    let spawned = workers::get(store.conn(), wid).and_then(|w| {
        let run = runs::get(store.conn(), &w.run_id.context("this worker has no run")?)?;
        super::taskledger_spawn::spawn_reviewers(
            ctx,
            store,
            &run,
            &format!(
                "{} of {}",
                report.stopped_at.as_deref().unwrap_or("code-sweep"),
                report.task
            ),
            &report.queued_reviews,
            &super::taskledger_spawn::CliSpawn,
        )
    });
    match spawned {
        Ok(note) => eprintln!("horch ledger: {note}"),
        Err(e) => eprintln!("horch ledger: horch did not spawn the reviewers: {e:#}"),
    }
}

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

fn nudge_run_of(ctx: &RuntimeContext, store: &Store, wid: &str) -> Result<()> {
    let w = workers::get(store.conn(), wid)?;
    if let Some(r) = &w.run_id {
        let run = runs::get(store.conn(), r)?;
        super::taskledger_wake::nudge_startable(ctx, store, &run);
    }
    Ok(())
}

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
