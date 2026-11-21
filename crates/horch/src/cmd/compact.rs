use std::cell::RefCell;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitCode, ExitStatus, Stdio};
use std::time::{Duration, Instant, SystemTime};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use horch_core::compaction::job::{self, JobPlan, JobPorts, Step};
use horch_core::compaction::jobfile::{self, JobRecord, Started, JOB_FILE, LOG_FILE};
use horch_core::compaction::policy::{
    self, HandoffProblem, JobLiveness, RowState, EVENT_FAILED, EVENT_REQUESTED, EVENT_WARNED,
};
use horch_core::execution::legacy::Record;
use horch_core::execution::records::SessionLedger;
use horch_core::harness::codex::PaneMode;
use horch_core::harness::HarnessKind;
use horch_core::messaging::delivery::{self, Readiness, Timing};
use horch_core::messaging::mailbox::Mailbox;
use horch_core::prompts;
use horch_core::runtime::process::spawn_detached;
use horch_core::runtime::RuntimeContext;
use horch_core::taskledger::sql_words;
use horch_core::telemetry::context::{read_current, Reading};
use horch_core::telemetry::readers::Unreadable;
use horch_core::usage::Locations;
use horch_core::workspace::client::WorkspaceClient;
use horch_core::workspace::PaneHost;

use super::context::{self, build_row, record_kind, record_workdir, Row, Sources};
use super::mode;

pub const START_TIMEOUT: Duration = Duration::from_secs(30);

const START_POLL: Duration = Duration::from_millis(50);

pub const EXIT_REPORTED: u8 = horch::exit::COMPACTION_REPORTED;

const POLL: Duration = Duration::from_secs(2);
const IDLE_POLLS: u8 = 2;
const IDLE_TIMEOUT_S: u64 = 1800;
const RESEND_AFTER: Duration = Duration::from_secs(30);
const MARKER_TIMEOUT: Duration = Duration::from_secs(600);
const SETTLE_TIMEOUT: Duration = Duration::from_secs(600);
const READING_TIMEOUT: Duration = Duration::from_secs(180);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompactArgs {
    pub role: String,
    pub request: bool,
    pub force: bool,
    pub foreground: bool,
    pub timeout: Option<u64>,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Outcome {
    pub code: u8,
    pub out: Vec<String>,
    pub err: Vec<String>,
}

impl Outcome {
    fn ok(line: String) -> Self {
        Outcome {
            code: 0,
            out: vec![line],
            err: vec![],
        }
    }

    fn refused(reason: impl std::fmt::Display) -> Self {
        Outcome {
            code: 1,
            out: vec![],
            err: vec![format!("horch compact: {reason}")],
        }
    }

    fn code(code: u8) -> Self {
        Outcome {
            code,
            ..Outcome::default()
        }
    }

    fn from_result(r: Result<String, String>) -> Self {
        r.map_or_else(Outcome::refused, Outcome::ok)
    }
}

fn why(e: anyhow::Error) -> String {
    format!("{e:#}")
}

pub(crate) struct Env<'a> {
    pub src: Sources<'a>,
    pub ws: &'a dyn WorkspaceClient,
    pub mailbox: &'a Mailbox,
    pub exe: PathBuf,
    pub start_timeout: Duration,
    pub mode_timing: mode::Timing,
}

impl Env<'_> {
    fn workspace(&self) -> &str {
        self.mailbox.workspace_id()
    }

    fn job_dir(&self, role: &str) -> PathBuf {
        jobfile::job_dir(&self.src.ctx.paths.state_root, self.workspace(), role)
    }

    fn message(&self, key: &str, vars: &[(&str, &str)]) -> Result<String> {
        prompts::context_message(self.src.roster, key, &vars.iter().copied().collect())
    }

    fn failed_line(&self, role: &str, step: &str, reason: &str, log: &Path) -> Result<String> {
        let log = log.display().to_string();
        self.message(
            "failed",
            &[
                ("role", role),
                ("step", step),
                ("reason", reason),
                ("log", &log),
            ],
        )
    }
}

pub fn compact(ctx: &RuntimeContext, args: &CompactArgs) -> Result<ExitCode> {
    let mailbox = match Mailbox::resolve_in(&PaneHost::of(ctx), ctx) {
        Ok(m) => m,
        Err(e) => return Ok(print(Outcome::refused(format!("{e:#}")))),
    };
    let herdr = PaneHost::for_workspace(ctx, mailbox.workspace_id());
    let ledger = SessionLedger::open_in(ctx)?;
    let roster = match super::load_roster_unwarned(ctx, None) {
        Ok(r) => r,
        Err(_) => horch_core::roster::Roster::builtin()?,
    };
    let loc = Locations::from_context(ctx);
    let cache = context::cache_dir(ctx);
    let env = Env {
        src: Sources {
            ctx,
            ledger: &ledger,
            roster: &roster,
            loc: &loc,
            cache_dir: &cache,
        },
        ws: &herdr,
        mailbox: &mailbox,
        exe: ctx.bins.exe()?,
        start_timeout: START_TIMEOUT,
        mode_timing: mode::Timing::DEFAULT,
    };
    Ok(print(run(&env, args)))
}

pub fn worker_note_context(ctx: &RuntimeContext, text: &str) -> Result<()> {
    let Some(record_id) = ctx.worker.as_ref().and_then(|w| w.record_id.as_deref()) else {
        return Ok(());
    };
    let ledger = SessionLedger::open_in(ctx)?;
    let record = ledger.get(record_id)?;
    let roster = super::load_roster_unwarned(ctx, None)
        .or_else(|_| horch_core::roster::Roster::builtin())?;
    let loc = Locations::from_context(ctx);
    let cache = context::cache_dir(ctx);
    let src = Sources {
        ctx,
        ledger: &ledger,
        roster: &roster,
        loc: &loc,
        cache_dir: &cache,
    };
    let row = build_row(
        &src,
        record,
        ctx.herdr.workspace.as_ref().map(|w| w.as_str()),
    );
    let asked = policy::already_asked(&row.record.history, row.last_compaction());
    if let Some(line) = super::messaging::note_check(&src, record_id)? {
        println!("{line}");
    }
    let handoff = policy::handoff_path(&row.record.role);
    if !asked
        || !row.record.execution_status().is_live()
        || !text.contains(&handoff)
        || row.tokens().is_none_or(|tokens| tokens < row.threshold)
        || row.job == JobLiveness::Live
    {
        return Ok(());
    }
    let herdr = PaneHost::of(ctx);
    let mailbox = Mailbox::resolve_in(&herdr, ctx)?;
    let env = Env {
        src,
        ws: &herdr,
        mailbox: &mailbox,
        exe: ctx.bins.exe()?,
        start_timeout: START_TIMEOUT,
        mode_timing: mode::Timing::DEFAULT,
    };
    let outcome = schedule(
        &env,
        &CompactArgs {
            role: row.record.role.clone(),
            request: false,
            force: false,
            foreground: false,
            timeout: None,
        },
    );
    if outcome.code == 0 && outcome.out.iter().any(|s| s.contains(" scheduled;")) {
        let line = format!("[horch] compaction started for {}", row.record.role);
        if let Err(error) = report(&env, &line) {
            eprintln!("horch: {line}; orchestrator notification failed: {error:#}");
        }
    }
    let _ = print(outcome);
    Ok(())
}

fn print(o: Outcome) -> ExitCode {
    for l in &o.out {
        println!("{l}");
    }
    for l in &o.err {
        eprintln!("{l}");
    }
    ExitCode::from(o.code)
}

pub(crate) fn run(env: &Env, args: &CompactArgs) -> Outcome {
    if args.request {
        request(env, &args.role)
    } else if args.foreground {
        foreground(env, args)
    } else {
        schedule(env, args)
    }
}

struct Target {
    row: Row,
    pane: String,
}

fn target(env: &Env, role: &str) -> Result<Target, String> {
    target_for(env, role, true)
}

fn repeat_refusal(t: &Target, role: &str, plan_override: bool) -> Option<String> {
    let history = &t.row.record.history;
    let last = t.row.last_compaction();
    if plan_override || !policy::already_asked(history, last) {
        return None;
    }
    if matches!(
        t.row.state,
        RowState::RequestStale | RowState::CompactFailed
    ) {
        let asks = policy::request_count(history, last);
        return (asks >= policy::MAX_REQUESTS)
            .then(|| format!("already asked {role} {asks} times; use the fresh route"));
    }
    let at = policy::asked_at(history)
        .map(|s| horch_core::clock::stamp(DateTime::<Utc>::from(s)))
        .unwrap_or_else(|| "an earlier time".into());
    Some(format!("already asked {role} at {at}"))
}

fn target_for(env: &Env, role: &str, need_command: bool) -> Result<Target, String> {
    let record = env
        .src
        .ledger
        .live_for_role(role, Some(env.workspace()))
        .map_err(|e| format!("{e:#}"))?
        .ok_or_else(|| {
            format!(
                "no live record for role {role} in workspace {}",
                env.workspace()
            )
        })?;
    let pane = env
        .mailbox
        .pane_for(role)
        .ok_or_else(|| format!("role {role} has no pane in workspace {}", env.workspace()))?;
    let kind = record_kind(&record);
    let usable = if need_command {
        kind.capabilities().compaction.has_command()
    } else {
        kind != HarnessKind::None
    };
    if !usable {
        return Err(format!(
            "role {role} runs harness {}, which has no compact command",
            kind.as_str()
        ));
    }
    let row = build_row(&env.src, record, Some(env.workspace()));
    Ok(Target { row, pane })
}

fn route_and_handoff(env: &Env, t: &Target, force: bool) -> Result<(), String> {
    if force {
        Ok(())
    } else if let Some(reason) = handoff_route(t) {
        Err(reason)
    } else if !env.src.roster.compacts_in_place(t.row.kind) {
        Err(format!(
            "refused: {} uses the fresh route; tell {} to run horch done",
            t.row.kind.as_str(),
            t.row.record.role
        ))
    } else {
        check_handoff(env, t)
    }
}

fn handoff_route(t: &Target) -> Option<String> {
    (t.row.state == RowState::Handoff).then(|| {
        let role = &t.row.record.role;
        format!(
            "refused: {role} is still over its threshold after 1 compaction; \
             use the handoff route: {role} writes {} and runs horch ledger handoff",
            policy::handoff_path(role)
        )
    })
}

fn check_handoff(env: &Env, t: &Target) -> Result<(), String> {
    let path = handoff_file(env.src.ctx, &t.row.record);
    let mtime = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
    let asked = policy::asked_at(&t.row.record.history);
    policy::handoff_fresh(mtime, asked, SystemTime::now())
        .map_err(|p| handoff_problem_text(&path, p))
}

fn handoff_problem_text(path: &Path, p: HandoffProblem) -> String {
    let path = path.display();
    match p {
        HandoffProblem::Missing => format!("refused: handoff {path} is missing"),
        HandoffProblem::BeforeAsk { asked_at } => {
            format!("refused: handoff {path} is older than the request at {asked_at}")
        }
        HandoffProblem::Stale { minutes } => {
            format!("refused: handoff {path} is {minutes} minutes old (limit 60)")
        }
    }
}

fn handoff_file(ctx: &RuntimeContext, record: &Record) -> PathBuf {
    record_workdir(ctx, record).join(policy::handoff_path(&record.role))
}

fn request(env: &Env, role: &str) -> Outcome {
    Outcome::from_result(request_target(env, role).and_then(|t| {
        if let Some(reason) = handoff_route(&t) {
            return Err(reason);
        }
        already_asked_line(env, &t, role).map_or_else(|| send_request(env, &t, role), Ok)
    }))
}

fn request_target(env: &Env, role: &str) -> Result<Target, String> {
    if role == "orchestrator" {
        Err(
            "refused: the orchestrator is not asked; it compacts itself at its stopping point"
                .into(),
        )
    } else {
        target_for(env, role, false)
    }
}

fn read_only(env: &Env, t: &Target) -> bool {
    mode::is_plan_pane(env.src.roster, &t.row.record)
        && mode::current_mode(&t.row.record.history) == PaneMode::Plan
}

fn already_asked_line(env: &Env, t: &Target, role: &str) -> Option<String> {
    let warned_only = t
        .row
        .record
        .history
        .iter()
        .rev()
        .find(|e| e.event == EVENT_REQUESTED || e.event == EVENT_WARNED)
        .is_some_and(|e| e.event == EVENT_WARNED);
    repeat_refusal(t, role, read_only(env, t) && warned_only)
}

fn send_request(env: &Env, t: &Target, role: &str) -> Result<String, String> {
    let record = &t.row.record;
    let tokens = t
        .row
        .tokens()
        .map_or_else(|| "unknown".to_string(), |n| n.to_string());
    let threshold = t.row.threshold.to_string();
    let handoff = policy::handoff_path(role);
    let line = env
        .message(
            "request",
            &[
                ("role", role),
                ("tokens", &tokens),
                ("threshold", &threshold),
                ("handoff", &handoff),
            ],
        )
        .map_err(why)?;
    if read_only(env, t) {
        mode::switch_record(
            env.src.ledger,
            env.ws,
            record,
            &t.pane,
            PaneMode::Write,
            &env.mode_timing,
        )
        .map_err(|e| format!("switching {role} to write: {e}"))?;
    }
    delivery::send_line_when_ready(
        env.ws,
        &t.pane,
        &line,
        Duration::ZERO,
        &Readiness::DEFAULT,
        &Timing::DEFAULT,
    )
    .map_err(why)?;
    env.src
        .ledger
        .record_event(
            &record.record_id,
            EVENT_REQUESTED,
            &format!("tokens {tokens} threshold {threshold}"),
        )
        .map_err(why)?;
    Ok(format!("asked {role} to write {handoff}"))
}

fn schedule(env: &Env, args: &CompactArgs) -> Outcome {
    let role = &args.role;
    let dir = env.job_dir(role);
    match target(env, role) {
        Err(reason) => Outcome::refused(reason),
        Ok(t) if t.row.job == JobLiveness::Live => {
            Outcome::ok(already_running(role, &dir.join(LOG_FILE)))
        }
        Ok(t) => route_and_handoff(env, &t, args.force)
            .map_or_else(Outcome::refused, |()| start(env, args, &t.row.record, &dir)),
    }
}

fn already_running(role: &str, log: &Path) -> String {
    format!(
        "compaction of {role} already running; log {}",
        log.display()
    )
}

fn start(env: &Env, args: &CompactArgs, record: &Record, dir: &Path) -> Outcome {
    let role = &args.role;
    let log = dir.join(LOG_FILE);
    let mut child: Option<Child> = None;
    let started = jobfile::start_locked(dir, || {
        let c = spawn_child(env, args, dir)?;
        let pid = c.id();
        child = Some(c);
        Ok(pid)
    });
    spawned(env, dir, role, &log, started, child)
        .unwrap_or_else(|reason| start_failed(env, role, record, dir, &log, &reason))
}

fn spawned(
    env: &Env,
    dir: &Path,
    role: &str,
    log: &Path,
    started: Result<Started>,
    child: Option<Child>,
) -> Result<Outcome, String> {
    match (started, child) {
        (Ok(Started::AlreadyRunning), _) => Ok(Outcome::ok(already_running(role, log))),
        (Ok(Started::Spawned { .. }), Some(child)) => match handshake(env, dir, child) {
            Handshake::Scheduled => Ok(Outcome::ok(format!(
                "compaction of {role} scheduled; log {}",
                log.display()
            ))),
            Handshake::AlreadyRunning => Ok(Outcome::ok(already_running(role, log))),
            Handshake::Reported(line) => Ok(Outcome {
                code: 1,
                out: vec![line],
                err: vec![],
            }),
            Handshake::Failed(reason) => Err(reason),
        },
        (Ok(Started::Spawned { .. }), None) => Err("the job was not spawned".to_string()),
        (Err(e), _) => Err(why(e)),
    }
}

fn start_failed(
    env: &Env,
    role: &str,
    record: &Record,
    dir: &Path,
    log: &Path,
    reason: &str,
) -> Outcome {
    jobfile::clear_on_exit(dir);
    let text = format!("step {}: {reason}", Step::Start);
    let _ = env
        .src
        .ledger
        .record_event(&record.record_id, EVENT_FAILED, &text);
    let line = env
        .failed_line(role, Step::Start.as_str(), reason, log)
        .unwrap_or_else(|_| {
            format!(
                "[horch] BLOCKED: Compaction of {role} failed at step start: {reason}. Log: {}. \
                 Run horch compact {role} --request, or use horch ledger handoff.",
                log.display()
            )
        });
    Outcome {
        code: 1,
        out: vec![line],
        err: vec![],
    }
}

fn spawn_child(env: &Env, args: &CompactArgs, dir: &Path) -> Result<Child> {
    let log = open_log(&dir.join(LOG_FILE))?;
    let ctx = env.src.ctx;
    let mut cmd = Command::new(&env.exe);
    cmd.args(["compact", &args.role, "--foreground"]);
    if args.force {
        cmd.arg("--force");
    }
    if let Some(n) = args.timeout {
        cmd.args(["--timeout", &n.to_string()]);
    }
    cmd.current_dir(ctx.paths.project().unwrap_or_else(|_| PathBuf::from(".")))
        .stdin(Stdio::null())
        .stdout(log.try_clone().context("duplicating the job log")?)
        .stderr(log)
        .env("HORCH_STATE_DIR", &ctx.paths.state_root)
        .env("HORCH_WORKSPACE_ID", env.workspace());
    if let Ok(project) = ctx.paths.project() {
        cmd.env("HORCH_PROJECT_DIR", project);
    }
    spawn_detached(&mut cmd).with_context(|| format!("starting {}", env.exe.display()))
}

fn open_log(path: &Path) -> Result<std::fs::File> {
    let mut opts = std::fs::OpenOptions::new();
    opts.create(true).append(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut opts, horch_core::fsx::PRIVATE_FILE);
    opts.open(path)
        .with_context(|| format!("opening {}", path.display()))
}

enum Handshake {
    Scheduled,
    AlreadyRunning,
    Reported(String),
    Failed(String),
}

fn handshake(env: &Env, dir: &Path, mut child: Child) -> Handshake {
    let begin = Instant::now();
    let h = loop {
        if let Some(h) = poll(env, dir, &mut child, begin) {
            break h;
        }
        std::thread::sleep(START_POLL);
    };
    if matches!(h, Handshake::Scheduled) {
        reap(child);
    }
    h
}

fn poll(env: &Env, dir: &Path, child: &mut Child, begin: Instant) -> Option<Handshake> {
    if job_started(dir) {
        return Some(Handshake::Scheduled);
    }
    match child.try_wait() {
        Ok(Some(status)) => Some(exited(dir, status)),
        Err(e) => Some(Handshake::Failed(format!("waiting for the job: {e}"))),
        Ok(None) if begin.elapsed() >= env.start_timeout => Some(timed_out(env, child)),
        Ok(None) => None,
    }
}

fn exited(dir: &Path, status: ExitStatus) -> Handshake {
    if job_started(dir) {
        return Handshake::Scheduled;
    }
    match status.code() {
        Some(0) => Handshake::AlreadyRunning,
        Some(c) if c == i32::from(EXIT_REPORTED) => last_blocked_line(&dir.join(LOG_FILE))
            .map_or_else(
                || Handshake::Failed(format!("the job exited with {c} and no report in its log")),
                Handshake::Reported,
            ),
        Some(c) => Handshake::Failed(format!("the job exited with {c} before it started")),
        None => Handshake::Failed("the job exited with a signal before it started".into()),
    }
}

fn timed_out(env: &Env, child: &mut Child) -> Handshake {
    let _ = child.kill();
    let _ = child.wait();
    Handshake::Failed(format!(
        "the job did not start in {} s",
        env.start_timeout.as_secs_f64()
    ))
}

fn reap(mut child: Child) {
    std::thread::spawn(move || {
        let _ = child.wait();
    });
}

fn job_started(dir: &Path) -> bool {
    std::fs::read(dir.join(JOB_FILE))
        .ok()
        .and_then(|b| serde_json::from_slice::<JobRecord>(&b).ok())
        .is_some_and(|j| j.step != Step::Start.as_str())
}

fn last_blocked_line(log: &Path) -> Option<String> {
    let text = std::fs::read_to_string(log).ok()?;
    text.lines()
        .rev()
        .find_map(|l| l.find("[horch] BLOCKED:").map(|i| l[i..].to_string()))
}

enum Stop {
    Duplicate,
    Refused {
        record: Option<Box<Record>>,
        why: String,
        clear: bool,
    },
}

impl Stop {
    fn refused(record: Option<&Record>, why: String, clear: bool) -> Stop {
        Stop::Refused {
            record: record.cloned().map(Box::new),
            why,
            clear,
        }
    }
}

fn foreground(env: &Env, args: &CompactArgs) -> Outcome {
    let role = &args.role;
    let dir = env.job_dir(role);
    let log = dir.join(LOG_FILE);
    let record = env
        .src
        .ledger
        .live_for_role(role, Some(env.workspace()))
        .ok()
        .flatten();
    let mut job = new_job(role, record.as_ref());
    match prepare(env, args, &dir, record.as_ref(), &mut job) {
        Ok((t, plan)) => run_job(env, &t, &plan, &dir, &log, job),
        Err(Stop::Duplicate) => Outcome::code(0),
        Err(Stop::Refused { record, why, clear }) => {
            if clear {
                jobfile::clear_on_exit(&dir);
            }
            refuse_child(env, role, record.as_deref(), &log, &why)
        }
    }
}

fn new_job(role: &str, record: Option<&Record>) -> JobRecord {
    JobRecord {
        v: jobfile::JOB_VERSION,
        role: role.into(),
        record_id: record.map(|r| r.record_id.clone()).unwrap_or_default(),
        started_at: horch_core::clock::stamp(Utc::now()),
        step: Step::Start.as_str().into(),
    }
}

fn prepare(
    env: &Env,
    args: &CompactArgs,
    dir: &Path,
    record: Option<&Record>,
    job: &mut JobRecord,
) -> Result<(Target, JobPlan), Stop> {
    create_job_file(env, dir, record, job)?;
    let (t, plan) = target(env, &args.role)
        .and_then(|t| {
            route_and_handoff(env, &t, args.force)?;
            let plan = plan_for(env, &t, args.timeout).map_err(why)?;
            Ok((t, plan))
        })
        .map_err(|reason| Stop::refused(record, reason, true))?;
    job.step = Step::WaitIdle.as_str().into();
    jobfile::write_step(dir, job).map_err(|e| Stop::refused(Some(&t.row.record), why(e), true))?;
    Ok((t, plan))
}

fn create_job_file(
    env: &Env,
    dir: &Path,
    record: Option<&Record>,
    job: &JobRecord,
) -> Result<(), Stop> {
    let root = jobfile::compact_root(&env.src.ctx.paths.state_root);
    horch_core::fsx::ensure_private_dir(&root)
        .and_then(|()| horch_core::fsx::ensure_private_dir(dir))
        .map_err(|e| Stop::refused(record, format!("{e:#}"), false))?;
    jobfile::create_job(dir, job).map_err(|e| {
        if e.kind() == std::io::ErrorKind::AlreadyExists {
            Stop::Duplicate
        } else {
            let reason = format!("creating {}: {e}", dir.join(JOB_FILE).display());
            Stop::refused(record, reason, false)
        }
    })
}

fn run_job(
    env: &Env,
    t: &Target,
    plan: &JobPlan,
    dir: &Path,
    log: &Path,
    job: JobRecord,
) -> Outcome {
    let beat = horch_core::heartbeat::start(dir);
    let ports = Ports {
        env,
        record: &t.row.record,
        pane: &t.pane,
        log,
        dir,
        job: RefCell::new(job),
        begin: Instant::now(),
    };
    let result = job::run(&ports, plan);
    beat.stop();
    jobfile::clear_on_exit(dir);
    match result {
        Ok(_) => Outcome::code(0),
        Err(_) => Outcome::code(EXIT_REPORTED),
    }
}

fn refuse_child(
    env: &Env,
    role: &str,
    record: Option<&Record>,
    log: &Path,
    reason: &str,
) -> Outcome {
    if let Some(r) = record {
        let _ = env.src.ledger.record_event(
            &r.record_id,
            EVENT_FAILED,
            &format!("step {}: {reason}", Step::Start),
        );
    }
    let line = env
        .failed_line(role, Step::Start.as_str(), reason, log)
        .unwrap_or_else(|_| {
            format!(
                "[horch] BLOCKED: Compaction of {role} failed at step start: {reason}. Log: {}. \
                 Run horch compact {role} --request, or use horch ledger handoff.",
                log.display()
            )
        });
    append_log(log, Step::Start, &line);
    if let Err(e) = report(env, &line) {
        append_log(log, Step::Start, &format!("not sent: {e:#}"));
    }
    Outcome::code(EXIT_REPORTED)
}

fn report(env: &Env, line: &str) -> Result<()> {
    let line = &horch_core::messaging::mail::horch_line(line);
    if let Err(error) = horch_core::messaging::mail::queue_notice(
        env.src.ctx,
        env.workspace(),
        "orchestrator",
        line,
    ) {
        eprintln!("horch: queueing the line as mail failed: {error:#}");
    }
    let pane = env
        .mailbox
        .pane_for("orchestrator")
        .context("no orchestrator is registered in this workspace; the line is queued as mail")?;
    delivery::send_line_when_ready(
        env.ws,
        &pane,
        line,
        Duration::ZERO,
        &Readiness::DEFAULT,
        &Timing::DEFAULT,
    )
}

fn append_log(log: &Path, step: Step, text: &str) {
    if let Ok(mut f) = open_log(log) {
        let _ = writeln!(f, "{} {step} {text}", horch_core::clock::stamp(Utc::now()));
    }
}

fn plan_for(env: &Env, t: &Target, timeout: Option<u64>) -> Result<JobPlan> {
    let record = &t.row.record;
    let role = record.role.as_str();
    let handoff = policy::handoff_path(role);
    let log = env.job_dir(role).join(LOG_FILE).display().to_string();
    let key = if record.is_orchestrator() {
        "instructions-orchestrator"
    } else {
        "instructions"
    };
    let caps = t.row.kind.capabilities().compaction;
    let instructions = env.message(key, &[("role", role), ("handoff", &handoff)])?;
    let compact_line = policy::compact_line(caps, &instructions)
        .with_context(|| format!("harness {} has no compact line", t.row.kind.as_str()))?;
    Ok(JobPlan {
        compact_line,
        max_sends: caps.max_sends,
        resume_line: env.message(
            if record.is_orchestrator() {
                "resume-orchestrator"
            } else {
                "resume"
            },
            &[("role", role), ("handoff", &handoff)],
        )?,
        reported_tmpl: env.message(
            "reported",
            &[
                ("role", role),
                ("pre", "{pre}"),
                ("post", "{post}"),
                ("handoff", &handoff),
            ],
        )?,
        failed_tmpl: env.message(
            "failed",
            &[
                ("role", role),
                ("step", "{step}"),
                ("reason", "{reason}"),
                ("log", &log),
            ],
        )?,
        handoff,
        role: role.into(),
        harness: t.row.kind.as_str().into(),
        failure_lines: caps.failure_lines.iter().map(|s| s.to_string()).collect(),
        log_path: log,
        poll: POLL,
        idle_polls: IDLE_POLLS,
        idle_timeout: Duration::from_secs(timeout.unwrap_or(IDLE_TIMEOUT_S)),
        resend_after: RESEND_AFTER,
        marker_timeout: MARKER_TIMEOUT,
        settle_timeout: SETTLE_TIMEOUT,
        reading_timeout: READING_TIMEOUT,
    })
}

struct Ports<'a> {
    env: &'a Env<'a>,
    record: &'a Record,
    pane: &'a str,
    log: &'a Path,
    dir: &'a Path,
    job: RefCell<JobRecord>,
    begin: Instant,
}

impl JobPorts for Ports<'_> {
    fn pane_status(&self) -> Result<Option<String>> {
        Ok(self.env.ws.pane_get(self.pane)?.agent_status)
    }

    fn send(&self, line: &str) -> Result<()> {
        delivery::send_line(self.env.ws, self.pane, line)
    }

    fn pane_text(&self) -> Result<String> {
        self.env.ws.pane_read(self.pane, "recent-unwrapped")
    }

    fn reading(&self) -> Result<Reading, Unreadable> {
        let sid = self
            .env
            .src
            .ledger
            .get(&self.record.record_id)
            .ok()
            .and_then(|r| r.session_id)
            .or_else(|| self.record.session_id.clone());
        read_current(
            self.env.src.loc,
            self.env.src.cache_dir,
            &self.record.agent,
            sid.as_deref(),
        )
    }

    fn record_event(&self, event: &str, text: &str) -> Result<()> {
        self.env
            .src
            .ledger
            .record_event(&self.record.record_id, event, text)?;
        if event == policy::EVENT_COMPACTED {
            let ctx = self.env.src.ctx;
            let store = horch_core::taskledger::open(ctx)?;
            let mut statement = store.conn().prepare(&format!(
                "SELECT worker_id FROM workers WHERE legacy_record_id = ?1 AND status IN ({live})",
                live = sql_words::list(sql_words::WORKERS_LIVE)
            ))?;
            let ids = statement
                .query_map([&self.record.record_id], |r| r.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            for id in ids {
                use horch_core::taskledger::model::{EventKind, ProgressSource};
                horch_core::taskledger::workers::progress(
                    store.conn(),
                    &id,
                    ProgressSource::Transcript,
                    EventKind::Progress,
                    &format!("compaction completed: {text}"),
                )?;
            }
        }
        Ok(())
    }

    fn report(&self, line: &str) -> Result<()> {
        append_log(self.log, Step::Report, line);
        report(self.env, line)
    }

    fn restore_mode(&self) -> Option<Result<()>> {
        let record = self.env.src.ledger.get(&self.record.record_id).ok()?;
        if !mode::is_plan_pane(self.env.src.roster, &record)
            || mode::current_mode(&record.history) != PaneMode::Write
        {
            return None;
        }
        Some(
            mode::switch_record(
                self.env.src.ledger,
                self.env.ws,
                &record,
                self.pane,
                PaneMode::Plan,
                &self.env.mode_timing,
            )
            .map_err(|e| anyhow::anyhow!("switching back to plan: {e}")),
        )
    }

    fn log(&self, step: Step, text: &str) {
        append_log(self.log, step, text);
        let mut job = self.job.borrow_mut();
        if step != Step::Start && job.step != step.as_str() {
            job.step = step.as_str().into();
            let _ = jobfile::write_step(self.dir, &job);
        }
    }

    fn sleep(&self, d: Duration) {
        std::thread::sleep(d);
    }

    fn elapsed(&self) -> Duration {
        self.begin.elapsed()
    }
}

#[cfg(test)]
mod tests {
    use super::super::context::testkit::*;
    use super::super::mode::sim::SimCodex;
    use super::*;
    use horch_core::roster::{PermissionMode, Teammate};
    use horch_core::workspace::testing::FakeWorkspace;

    struct Fleet {
        w: World,
        ws: FakeWorkspace,
        mailbox: Mailbox,
        exe: PathBuf,
        start_timeout: Duration,
    }

    impl Fleet {
        fn new() -> Fleet {
            let w = World::new();
            let mailbox = Mailbox::under(w.tmp.path().join("mail"), "w1");
            std::fs::create_dir_all(mailbox.dir()).unwrap();
            Fleet {
                exe: PathBuf::from("/nonexistent/horch"),
                start_timeout: START_TIMEOUT,
                w,
                ws: FakeWorkspace::new(),
                mailbox,
            }
        }

        fn pane(&self, role: &str) -> String {
            let pane = self
                .ws
                .workspace_create(role, None, false)
                .unwrap()
                .root_pane_id;
            self.ws
                .set_agent_states(&pane, &[(Some("claude"), Some("idle"))]);
            std::fs::write(self.mailbox.dir().join(format!("{role}.id")), &pane).unwrap();
            pane
        }

        fn env(&self) -> Env<'_> {
            self.env_on(&self.ws)
        }

        fn env_on<'a>(&'a self, ws: &'a dyn WorkspaceClient) -> Env<'a> {
            Env {
                src: self.w.sources(),
                ws,
                mailbox: &self.mailbox,
                exe: self.exe.clone(),
                start_timeout: self.start_timeout,
                mode_timing: MODE_FAST,
            }
        }

        fn plan_pane(&mut self) {
            self.w.roster.insert_for_test(Teammate {
                name: "codex-plan".into(),
                agent: HarnessKind::Codex,
                permission_mode: Some(PermissionMode::Plan),
                ..Default::default()
            });
            self.w
                .worker("r-1", "codex-plan-1", "codex", "codex-plan", None);
            std::fs::write(self.mailbox.dir().join("codex-plan-1.id"), "p1").unwrap();
        }

        fn request_on(&self, ws: &dyn WorkspaceClient) -> Outcome {
            let args = CompactArgs {
                role: "codex-plan-1".into(),
                request: true,
                force: false,
                foreground: false,
                timeout: None,
            };
            run(&self.env_on(ws), &args)
        }

        fn mode_events(&self, id: &str) -> Vec<String> {
            self.w
                .ledger
                .get(id)
                .unwrap()
                .history
                .into_iter()
                .map(|e| e.event)
                .filter(|e| e.starts_with("mode-") || e == EVENT_REQUESTED)
                .collect()
        }

        fn run(&self, role: &str, f: impl FnOnce(&mut CompactArgs)) -> Outcome {
            let mut args = CompactArgs {
                role: role.into(),
                request: false,
                force: false,
                foreground: false,
                timeout: None,
            };
            f(&mut args);
            run(&self.env(), &args)
        }

        fn typed(&self, pane: &str) -> Vec<String> {
            self.ws
                .calls()
                .into_iter()
                .filter(|c| c.method == "agent_prompt" && c.args[0] == pane)
                .map(|c| c.args[1].clone())
                .collect()
        }

        fn handoff(&self, dir: &Path, role: &str) -> PathBuf {
            let path = dir.join(policy::handoff_path(role));
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, "# handoff\n").unwrap();
            path
        }

        fn message(&self, key: &str, vars: &[(&str, &str)]) -> String {
            self.env().message(key, vars).unwrap()
        }
    }

    const MODE_FAST: mode::Timing = mode::Timing {
        poll: Duration::ZERO,
        idle_polls: 6,
        screen_polls: 3,
    };

    #[test]
    fn x6_request_on_a_plan_pane_switches_to_write_first() {
        let mut f = Fleet::new();
        f.plan_pane();
        let sim = SimCodex::new(PaneMode::Plan);
        let o = f.request_on(&sim);
        assert_eq!(o.code, 0, "{o:?}");
        let typed = sim.typed();
        assert_eq!(
            typed[..4],
            [
                "prompt /permissions",
                "key down",
                "key enter",
                "prompt /status"
            ]
        );
        assert_eq!(typed.len(), 5);
        assert!(typed[4].starts_with("prompt ") && typed[4].contains("codex-plan-1-whats-next.md"));
        assert_eq!(f.mode_events("r-1"), ["mode-changed", EVENT_REQUESTED]);
    }

    #[test]
    fn x6_a_failed_switch_types_no_request() {
        let mut f = Fleet::new();
        f.plan_pane();
        let sim = SimCodex::new(PaneMode::Plan);
        sim.state.borrow_mut().write_status = "Workspace (Ask for approval)".into();
        let o = f.request_on(&sim);
        assert_eq!(o.code, 1, "{o:?}");
        assert!(o.err[0].contains("/status shows Permissions"), "{o:?}");
        assert_eq!(sim.typed().len(), 4, "the switch keys only");
        assert_eq!(f.mode_events("r-1"), ["mode-failed"]);
    }

    #[test]
    fn x6_a_warned_plan_pane_is_switched_and_asked() {
        let mut f = Fleet::new();
        f.plan_pane();
        f.w.ledger
            .record_event("r-1", policy::EVENT_WARNED, "tokens 1 threshold 1")
            .unwrap();
        let sim = SimCodex::new(PaneMode::Plan);
        let o = f.request_on(&sim);
        assert_eq!(o.code, 0, "{o:?}");
        assert_eq!(f.mode_events("r-1"), ["mode-changed", EVENT_REQUESTED]);
        let again = f.request_on(&sim);
        assert!(again.out[0].starts_with("already asked"), "{again:?}");
        assert_eq!(sim.typed().len(), 5, "nothing more typed");
    }

    #[test]
    fn x6_a_plan_pane_in_write_mode_is_not_switched_again() {
        let mut f = Fleet::new();
        f.plan_pane();
        f.w.ledger
            .record_event("r-1", mode::EVENT_MODE_CHANGED, "write")
            .unwrap();
        let sim = SimCodex::new(PaneMode::Write);
        let o = f.request_on(&sim);
        assert_eq!(o.code, 0, "{o:?}");
        let typed = sim.typed();
        assert_eq!(typed.len(), 1);
        assert!(typed[0].contains("codex-plan-1-whats-next.md"));
    }

    fn ports_on<'a>(env: &'a Env<'a>, record: &'a Record, dir: &'a Path) -> Ports<'a> {
        Ports {
            env,
            record,
            pane: "p1",
            log: dir,
            dir,
            job: RefCell::new(JobRecord {
                v: 1,
                role: record.role.clone(),
                record_id: record.record_id.clone(),
                started_at: String::new(),
                step: Step::WaitIdleAfter.as_str().into(),
            }),
            begin: Instant::now(),
        }
    }

    #[test]
    fn x6_the_job_port_restores_a_switched_plan_pane() {
        let mut f = Fleet::new();
        f.plan_pane();
        let sim = SimCodex::new(PaneMode::Plan);
        assert_eq!(f.request_on(&sim).code, 0);
        let env = f.env_on(&sim);
        let record = f.w.ledger.get("r-1").unwrap();
        let dir = f.w.tmp.path().join("job");
        let ports = ports_on(&env, &record, &dir);
        assert!(ports.restore_mode().unwrap().is_ok());
        assert_eq!(sim.state.borrow().mode, PaneMode::Plan);
        assert_eq!(
            f.mode_events("r-1"),
            ["mode-changed", EVENT_REQUESTED, "mode-changed"]
        );
        assert_eq!(
            mode::current_mode(&f.w.ledger.get("r-1").unwrap().history),
            PaneMode::Plan
        );
    }

    #[test]
    fn x6_the_job_port_restores_nothing_for_an_unswitched_pane() {
        let mut f = Fleet::new();
        f.plan_pane();
        let sim = SimCodex::new(PaneMode::Plan);
        let env = f.env_on(&sim);
        let record = f.w.ledger.get("r-1").unwrap();
        let dir = f.w.tmp.path().join("job");
        assert!(ports_on(&env, &record, &dir).restore_mode().is_none());
        assert!(sim.typed().is_empty());
    }

    fn set_mtime(path: &Path, ago: Duration) {
        let f = std::fs::File::options().write(true).open(path).unwrap();
        f.set_modified(SystemTime::now() - ago).unwrap();
    }

    #[test]
    fn ctx_11_request_types_message_and_records_event() {
        let f = Fleet::new();
        let pane = f.pane("sonnet-1");
        f.w.worker("r-1", "sonnet-1", "claude", "sonnet", Some(OVER));
        let o = f.run("sonnet-1", |a| a.request = true);
        assert_eq!(o.code, 0, "{o:?}");
        let handoff = "ai_docs/handoffs/sonnet-1-whats-next.md";
        assert_eq!(o.out, [format!("asked sonnet-1 to write {handoff}")]);
        let want = f.message(
            "request",
            &[
                ("role", "sonnet-1"),
                ("tokens", "311225"),
                ("threshold", "300000"),
                ("handoff", handoff),
            ],
        );
        assert_eq!(f.typed(&pane), [want]);
        assert_eq!(
            f.w.events("r-1", EVENT_REQUESTED),
            ["tokens 311225 threshold 300000"]
        );

        let again = f.run("sonnet-1", |a| a.request = true);
        assert_eq!(again.code, 0);
        assert_eq!(again.out.len(), 1);
        assert!(
            again.out[0].starts_with("already asked sonnet-1 at 20"),
            "{again:?}"
        );
        assert_eq!(f.typed(&pane).len(), 1, "nothing typed the second time");
        assert_eq!(f.w.events("r-1", EVENT_REQUESTED).len(), 1);
    }

    #[test]
    fn a_fresh_ask_inside_the_deadline_is_not_typed_again() {
        let f = Fleet::new();
        let pane = f.pane("sonnet-1");
        f.w.worker("r-1", "sonnet-1", "claude", "sonnet", Some(OVER));
        let ago = minutes_ago;
        f.w.event_at("r-1", &ago(5), EVENT_REQUESTED, "earlier");
        let fresh = f.run("sonnet-1", |a| a.request = true);
        assert!(
            fresh.out[0].starts_with("already asked sonnet-1 at"),
            "{fresh:?}"
        );
        assert!(f.typed(&pane).is_empty());
        assert_eq!(f.w.events("r-1", EVENT_REQUESTED).len(), 1);
    }

    #[test]
    fn a_stale_row_with_two_asks_is_not_asked_a_third_time() {
        let f = Fleet::new();
        let pane = f.pane("sonnet-1");
        f.w.worker("r-1", "sonnet-1", "claude", "sonnet", Some(OVER));
        f.w.event_at("r-1", &minutes_ago(60), EVENT_REQUESTED, "first");
        f.w.event_at("r-1", &minutes_ago(25), EVENT_REQUESTED, "second");
        let o = f.run("sonnet-1", |a| a.request = true);
        assert_eq!(o.code, 0, "{o:?}");
        assert_eq!(
            o.out,
            ["already asked sonnet-1 2 times; use the fresh route"]
        );
        assert!(f.typed(&pane).is_empty());
        assert_eq!(f.w.events("r-1", EVENT_REQUESTED).len(), 2);
    }

    #[test]
    fn trf_09_a_row_still_over_after_1_compaction_is_sent_to_the_handoff_route() {
        let f = Fleet::new();
        let pane = f.pane("sonnet-1");
        f.w.worker("r-1", "sonnet-1", "claude", "sonnet", Some(OVER));
        f.w.event_at("r-1", &minutes_ago(10), EVENT_REQUESTED, "first");
        f.w.event_at(
            "r-1",
            &minutes_ago(5),
            policy::EVENT_COMPACTED,
            &policy::compacted_text(Some(330_000), Some(305_000), "h.md"),
        );
        let want = "horch compact: refused: sonnet-1 is still over its threshold after 1 \
                    compaction; use the handoff route: sonnet-1 writes \
                    ai_docs/handoffs/sonnet-1-whats-next.md and runs horch ledger handoff";
        let asked = f.run("sonnet-1", |a| a.request = true);
        assert_eq!(asked.code, 1, "{asked:?}");
        assert_eq!(asked.err, [want]);
        assert!(f.typed(&pane).is_empty());
        assert_eq!(f.w.events("r-1", EVENT_REQUESTED).len(), 1);
        let scheduled = f.run("sonnet-1", |_| {});
        assert_eq!(scheduled.code, 1, "{scheduled:?}");
        assert_eq!(scheduled.err, [want]);
        assert!(!f.env().job_dir("sonnet-1").join(JOB_FILE).exists());
    }

    #[test]
    fn a_stale_ask_with_a_live_job_is_not_typed_again() {
        let f = Fleet::new();
        let pane = f.pane("sonnet-1");
        f.w.worker("r-1", "sonnet-1", "claude", "sonnet", Some(OVER));
        f.w.event_at("r-1", &minutes_ago(25), EVENT_REQUESTED, "earlier");
        let dir = f.env().job_dir("sonnet-1");
        std::fs::create_dir_all(&dir).unwrap();
        let job = JobRecord {
            v: 1,
            role: "sonnet-1".into(),
            record_id: "r-1".into(),
            started_at: "2026-10-06T10:00:00Z".into(),
            step: "wait-idle".into(),
        };
        jobfile::create_job(&dir, &job).unwrap();
        let _beat = horch_core::heartbeat::start(&dir);
        let o = f.run("sonnet-1", |a| a.request = true);
        assert!(o.out[0].starts_with("already asked sonnet-1 at"), "{o:?}");
        assert!(f.typed(&pane).is_empty());
    }

    #[test]
    fn a_request_on_a_stale_row_types_and_records_again() {
        let f = Fleet::new();
        let pane = f.pane("sonnet-1");
        f.w.worker("r-1", "sonnet-1", "claude", "sonnet", Some(OVER));
        let ago = minutes_ago;
        f.w.event_at("r-1", &ago(25), EVENT_REQUESTED, "earlier");
        let o = f.run("sonnet-1", |a| a.request = true);
        assert_eq!(o.code, 0, "{o:?}");
        assert_eq!(
            o.out,
            ["asked sonnet-1 to write ai_docs/handoffs/sonnet-1-whats-next.md"]
        );
        assert_eq!(f.typed(&pane).len(), 1);
        assert_eq!(f.w.events("r-1", EVENT_REQUESTED).len(), 2);
        let third = f.run("sonnet-1", |a| a.request = true);
        assert!(third.out[0].starts_with("already asked"), "{third:?}");
        assert_eq!(f.typed(&pane).len(), 1);
    }

    #[test]
    fn a_request_after_a_failed_job_types_and_records_again() {
        let f = Fleet::new();
        let pane = f.pane("sonnet-1");
        f.w.worker("r-1", "sonnet-1", "claude", "sonnet", Some(OVER));
        let ago = minutes_ago;
        f.w.event_at("r-1", &ago(4), EVENT_REQUESTED, "earlier");
        f.w.event_at("r-1", &ago(2), EVENT_FAILED, "step wait-idle: timed out");
        let o = f.run("sonnet-1", |a| a.request = true);
        assert_eq!(o.code, 0, "{o:?}");
        assert_eq!(f.typed(&pane).len(), 1);
        assert_eq!(f.w.events("r-1", EVENT_REQUESTED).len(), 2);
    }

    #[test]
    fn ctx_22_request_and_warning_event_texts() {
        let f = Fleet::new();
        f.pane("sonnet-1");
        f.w.worker("r-1", "sonnet-1", "claude", "sonnet", Some(OVER));
        f.w.worker("r-2", "sonnet-2", "claude", "sonnet", Some(OVER));
        assert_eq!(f.run("sonnet-1", |a| a.request = true).code, 0);
        crate::cmd::messaging::note_check(&f.w.sources(), "r-2").unwrap();
        assert_eq!(
            f.w.history("r-1").last().unwrap(),
            &(
                "compact-requested".to_string(),
                "tokens 311225 threshold 300000".to_string()
            )
        );
        assert_eq!(
            f.w.history("r-2").last().unwrap(),
            &(
                "context-warned".to_string(),
                "tokens 311225 threshold 300000".to_string()
            )
        );
    }

    #[test]
    fn ctx_14_compact_refuses_missing_or_stale_handoff() {
        let f = Fleet::new();
        let pane = f.pane("sonnet-1");
        f.w.worker("r-1", "sonnet-1", "claude", "sonnet", Some(SMALL));
        let path =
            f.w.project()
                .join("ai_docs/handoffs/sonnet-1-whats-next.md");
        let job_dir = f.env().job_dir("sonnet-1");
        let check = |want: String| {
            let o = f.run("sonnet-1", |_| {});
            assert_eq!(o.code, 1, "{o:?}");
            assert_eq!(o.err, [want]);
            assert!(o.out.is_empty());
            assert!(f.typed(&pane).is_empty());
            assert!(!job_dir.exists(), "no job dir");
        };
        check(format!(
            "horch compact: refused: handoff {} is missing",
            path.display()
        ));

        f.handoff(&f.w.project(), "sonnet-1");
        set_mtime(&path, Duration::from_secs(61 * 60));
        check(format!(
            "horch compact: refused: handoff {} is 61 minutes old (limit 60)",
            path.display()
        ));

        let asked = minutes_ago(5);
        f.w.event_at("r-1", &asked, EVENT_REQUESTED, "tokens 1 threshold 2");
        set_mtime(&path, Duration::from_secs(10 * 60));
        check(format!(
            "horch compact: refused: handoff {} is older than the request at {asked}",
            path.display()
        ));
    }

    #[test]
    fn ctx_14_handoff_resolves_against_the_record_workdir() {
        let f = Fleet::new();
        f.pane("sonnet-1");
        let a = f.w.tmp.path().join("a");
        std::fs::create_dir_all(&a).unwrap();
        f.w.ledger
            .insert(Record {
                record_id: "r-1".into(),
                session_id: Some(SMALL.into()),
                agent: "claude".into(),
                tier: "sonnet".into(),
                model: "sonnet".into(),
                role: "sonnet-1".into(),
                task: "work".into(),
                workspace_id: Some("w1".into()),
                workdir: Some(a.to_string_lossy().into_owned()),
                ..Record::default()
            })
            .unwrap();
        f.handoff(&f.w.project(), "sonnet-1");
        let t = target(&f.env(), "sonnet-1").ok().unwrap();
        let in_a = a.join("ai_docs/handoffs/sonnet-1-whats-next.md");
        assert_eq!(
            route_and_handoff(&f.env(), &t, false),
            Err(format!("refused: handoff {} is missing", in_a.display()))
        );
        f.handoff(&a, "sonnet-1");
        assert_eq!(route_and_handoff(&f.env(), &t, false), Ok(()));
    }

    #[cfg(unix)]
    #[test]
    fn ctx_15_compact_start_handshake_fails_without_job_file() {
        let mut f = Fleet::new();
        let bin = f.w.tmp.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let exe = bin.join("sleeper");
        let warm = crate::test_support::write_warmed(
            &exe,
            "#!/bin/sh\nif [ \"$1\" = warm ]; then exit 0; fi\nexec sleep 30\n",
            &["warm"],
        );
        assert!(warm.status.success());
        f.exe = exe;
        f.start_timeout = Duration::from_millis(1500);
        f.pane("sonnet-1");
        f.w.worker("r-1", "sonnet-1", "claude", "sonnet", Some(SMALL));
        f.handoff(&f.w.project(), "sonnet-1");

        let begin = Instant::now();
        let o = f.run("sonnet-1", |_| {});
        let took = begin.elapsed();
        assert!(took >= f.start_timeout, "{took:?}");
        assert_eq!(o.code, 1, "{o:?}");
        let log = f.env().job_dir("sonnet-1").join(LOG_FILE);
        let reason = "the job did not start in 1.5 s";
        assert_eq!(
            o.out,
            [f.message(
                "failed",
                &[
                    ("role", "sonnet-1"),
                    ("step", "start"),
                    ("reason", reason),
                    ("log", &log.display().to_string()),
                ],
            )]
        );
        assert_eq!(
            f.w.events("r-1", EVENT_FAILED),
            [format!("step start: {reason}")]
        );
        let dir = f.env().job_dir("sonnet-1");
        assert_eq!(
            jobfile::liveness(&dir, Utc::now()),
            JobLiveness::None,
            "the dir does not block a retry"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_slow_child_within_the_default_start_timeout_is_scheduled() {
        let mut f = Fleet::new();
        let dir = f.env().job_dir("sonnet-1");
        let bin = f.w.tmp.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let exe = bin.join("slow-starter");
        let warm = crate::test_support::write_warmed(
            &exe,
            &format!(
                "#!/bin/sh\nif [ \"$1\" = warm ]; then exit 0; fi\nsleep 6\nprintf '%s' '{{\"v\":1,\"role\":\"sonnet-1\",\"record_id\":\"r-1\",\"started_at\":\"x\",\"step\":\"wait-idle\"}}' > '{}/job.json'\n",
                dir.display()
            ),
            &["warm"],
        );
        assert!(warm.status.success());
        f.exe = exe;
        f.pane("sonnet-1");
        f.w.worker("r-1", "sonnet-1", "claude", "sonnet", Some(SMALL));
        f.handoff(&f.w.project(), "sonnet-1");

        let o = f.run("sonnet-1", |_| {});
        assert_eq!(o.code, 0, "{o:?}");
        assert!(
            o.out[0].starts_with("compaction of sonnet-1 scheduled"),
            "{o:?}"
        );
        assert!(f.w.events("r-1", EVENT_FAILED).is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn ctx_15_compact_start_handshake_accepts_wait_idle() {
        let mut f = Fleet::new();
        let dir = f.env().job_dir("sonnet-1");
        let bin = f.w.tmp.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let exe = bin.join("starter");
        let warm = crate::test_support::write_warmed(
            &exe,
            &format!(
                "#!/bin/sh\nif [ \"$1\" = warm ]; then exit 0; fi\nsleep 0.2\nprintf '%s' '{{\"v\":1,\"role\":\"sonnet-1\",\"record_id\":\"r-1\",\"started_at\":\"x\",\"step\":\"wait-idle\"}}' > '{}/job.json'\n",
                dir.display()
            ),
            &["warm"],
        );
        assert!(warm.status.success());
        f.exe = exe;
        f.start_timeout = Duration::from_secs(10);
        f.pane("sonnet-1");
        f.w.worker("r-1", "sonnet-1", "claude", "sonnet", Some(SMALL));
        f.handoff(&f.w.project(), "sonnet-1");

        let o = f.run("sonnet-1", |_| {});
        assert_eq!(
            o,
            Outcome::ok(format!(
                "compaction of sonnet-1 scheduled; log {}",
                dir.join(LOG_FILE).display()
            ))
        );
        assert!(f.w.events("r-1", EVENT_FAILED).is_empty());
        assert!(dir.join(jobfile::SPAWNED_FILE).exists());
    }

    #[cfg(unix)]
    #[test]
    fn ctx_15_exit_after_the_job_file_is_a_start_not_already_running() {
        let f = Fleet::new();
        let dir = f.env().job_dir("sonnet-1");
        std::fs::create_dir_all(&dir).unwrap();
        let status = Command::new("true").status().unwrap();
        assert!(matches!(exited(&dir, status), Handshake::AlreadyRunning));
        std::fs::write(
            dir.join(JOB_FILE),
            r#"{"v":1,"role":"sonnet-1","record_id":"r-1","started_at":"x","step":"wait-idle"}"#,
        )
        .unwrap();
        assert!(matches!(exited(&dir, status), Handshake::Scheduled));
    }

    #[test]
    fn ctx_15_foreground_refusal_records_failed_and_reports() {
        let f = Fleet::new();
        let orch = f.pane("orchestrator");
        f.pane("sonnet-1");
        f.w.worker("r-1", "sonnet-1", "claude", "sonnet", Some(SMALL));
        let path = f.handoff(&f.w.project(), "sonnet-1");
        set_mtime(&path, Duration::from_secs(90 * 60));

        let o = f.run("sonnet-1", |a| a.foreground = true);
        assert_eq!(o.code, EXIT_REPORTED, "{o:?}");
        let failed = f.w.events("r-1", EVENT_FAILED);
        assert_eq!(failed.len(), 1, "{failed:?}");
        assert!(
            failed[0].starts_with("step start: refused: handoff "),
            "{failed:?}"
        );
        let dir = f.env().job_dir("sonnet-1");
        let reported = f.typed(&orch);
        assert_eq!(reported.len(), 1, "{reported:?}");
        assert!(reported[0].starts_with(
            "[horch] BLOCKED: Compaction of sonnet-1 failed at step start: refused: handoff "
        ));
        assert_eq!(
            last_blocked_line(&dir.join(LOG_FILE)).as_ref(),
            Some(&reported[0])
        );
        assert!(!dir.join(JOB_FILE).exists());

        let job = JobRecord {
            v: 1,
            role: "sonnet-1".into(),
            record_id: "r-1".into(),
            started_at: "2026-10-06T10:00:00Z".into(),
            step: "wait-idle".into(),
        };
        jobfile::create_job(&dir, &job).unwrap();
        let second = f.run("sonnet-1", |a| a.foreground = true);
        assert_eq!(second, Outcome::code(0));
        assert_eq!(f.w.events("r-1", EVENT_FAILED).len(), 1);
        assert_eq!(f.typed(&orch).len(), 1);
    }

    #[test]
    fn a_failed_job_with_no_orchestrator_leaves_the_failed_line_as_mail() {
        let f = Fleet::new();
        f.pane("sonnet-1");
        f.w.worker("r-1", "sonnet-1", "claude", "sonnet", Some(SMALL));
        let path = f.handoff(&f.w.project(), "sonnet-1");
        set_mtime(&path, Duration::from_secs(90 * 60));

        let o = f.run("sonnet-1", |a| a.foreground = true);
        assert_eq!(o.code, EXIT_REPORTED, "{o:?}");

        let pending = horch_core::messaging::mail::Mail::under(&f.w.ctx.paths.state_root, "w1")
            .pending()
            .unwrap();
        assert_eq!(pending.len(), 1, "{pending:?}");
        assert_eq!(pending[0].from, "horch");
        assert!(
            pending[0].text.starts_with(
                "[horch] BLOCKED: Compaction of sonnet-1 failed at step start: refused: handoff "
            ),
            "{pending:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_refused_mail_dir_does_not_stop_the_typed_report() {
        use std::os::unix::fs::PermissionsExt;
        let f = Fleet::new();
        let orch = f.pane("orchestrator");
        f.pane("sonnet-1");
        f.w.worker("r-1", "sonnet-1", "claude", "sonnet", Some(SMALL));
        let path = f.handoff(&f.w.project(), "sonnet-1");
        set_mtime(&path, Duration::from_secs(90 * 60));
        let mail = horch_core::messaging::mail::Mail::under(&f.w.ctx.paths.state_root, "w1");
        std::fs::create_dir_all(mail.dir()).unwrap();
        std::fs::set_permissions(mail.dir(), std::fs::Permissions::from_mode(0o777)).unwrap();

        let o = f.run("sonnet-1", |a| a.foreground = true);

        assert_eq!(o.code, EXIT_REPORTED, "{o:?}");
        let reported = f.typed(&orch);
        assert_eq!(reported.len(), 1, "{reported:?}");
        assert!(reported[0].starts_with("[horch] BLOCKED: Compaction of sonnet-1 failed"));
        assert!(!mail.dir().join("pending").exists(), "nothing was queued");
    }

    #[cfg(unix)]
    #[test]
    fn compact_dirs_are_0700_and_the_job_log_0600() {
        use std::os::unix::fs::PermissionsExt;
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        let f = Fleet::new();
        f.pane("orchestrator");
        f.pane("sonnet-1");
        f.w.worker("r-1", "sonnet-1", "claude", "sonnet", Some(SMALL));
        let path = f.handoff(&f.w.project(), "sonnet-1");
        set_mtime(&path, Duration::from_secs(90 * 60));

        let o = f.run("sonnet-1", |a| a.foreground = true);

        assert_eq!(o.code, EXIT_REPORTED, "{o:?}");
        let dir = f.env().job_dir("sonnet-1");
        assert_eq!(mode(dir.parent().unwrap()), 0o700);
        assert_eq!(mode(&dir), 0o700);
        assert_eq!(mode(&dir.join(LOG_FILE)), 0o600);
    }

    #[test]
    fn ctx_16_compact_refuses_fresh_route_harness() {
        let f = Fleet::new();
        f.pane("pi-1");
        f.w.worker("r-1", "pi-1", "pi", "pi", Some("s-pi"));
        let o = f.run("pi-1", |_| {});
        assert_eq!(o.code, 1);
        assert_eq!(
            o.err,
            ["horch compact: refused: pi uses the fresh route; tell pi-1 to run horch done"]
        );
        let t = target(&f.env(), "pi-1").ok().unwrap();
        assert_eq!(route_and_handoff(&f.env(), &t, true), Ok(()));
    }

    #[test]
    fn ctx_18_compact_targets_the_orchestrator_pane_and_record() {
        let f = Fleet::new();
        let orch = f.pane("orchestrator");
        f.pane("sonnet-1");
        f.w.worker("r-1", "sonnet-1", "claude", "sonnet", Some(SMALL));
        f.w.orchestrator("r-o", Some(OVER));
        let t = target(&f.env(), "orchestrator").ok().unwrap();
        assert_eq!(t.pane, orch);
        assert_eq!(t.row.record.record_id, "r-o");
        let plan = plan_for(&f.env(), &t, None).unwrap();
        let handoff = "ai_docs/handoffs/orchestrator-whats-next.md";
        let keep = f.message(
            "instructions-orchestrator",
            &[("role", "orchestrator"), ("handoff", handoff)],
        );
        assert_eq!(plan.compact_line, format!("/compact {keep}"));
        assert_eq!(plan.idle_timeout, Duration::from_secs(1800));

        let o = f.run("orchestrator", |a| a.request = true);
        assert_eq!(o.code, 1);
        assert_eq!(o.err.len(), 1);
        assert!(o.err[0].starts_with("horch compact: refused: "), "{o:?}");
        assert!(f.typed(&orch).is_empty());
    }

    #[test]
    fn request_refuses_the_orchestrator_before_any_lookup() {
        let f = Fleet::new();
        let o = f.run("orchestrator", |a| a.request = true);
        assert_eq!(
            o,
            Outcome::refused(
                "refused: the orchestrator is not asked; it compacts itself at its stopping point"
            )
        );
    }

    #[test]
    fn request_and_schedule_refuse_a_role_with_no_record_or_no_pane() {
        let f = Fleet::new();
        let missing = Outcome::refused("no live record for role ghost-1 in workspace w1");
        assert_eq!(f.run("ghost-1", |a| a.request = true), missing);
        assert_eq!(f.run("ghost-1", |_| {}), missing);
        f.w.worker("r-1", "sonnet-1", "claude", "sonnet", Some(SMALL));
        let nopane = Outcome::refused("role sonnet-1 has no pane in workspace w1");
        assert_eq!(f.run("sonnet-1", |a| a.request = true), nopane);
        assert_eq!(f.run("sonnet-1", |_| {}), nopane);
    }

    #[test]
    fn a_failed_type_into_the_pane_refuses_and_records_nothing() {
        let f = Fleet::new();
        std::fs::write(f.mailbox.dir().join("sonnet-1.id"), "no-such-pane").unwrap();
        f.w.worker("r-1", "sonnet-1", "claude", "sonnet", Some(OVER));
        let o = f.run("sonnet-1", |a| a.request = true);
        assert_eq!(o.code, 1, "{o:?}");
        assert!(o.out.is_empty());
        assert!(f.w.events("r-1", EVENT_REQUESTED).is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn a_live_job_is_reported_as_already_running_and_nothing_starts() {
        let f = Fleet::new();
        f.pane("sonnet-1");
        f.w.worker("r-1", "sonnet-1", "claude", "sonnet", Some(OVER));
        let dir = f.env().job_dir("sonnet-1");
        std::fs::create_dir_all(&dir).unwrap();
        let beat = horch_core::heartbeat::start(&dir);
        let o = f.run("sonnet-1", |_| {});
        beat.stop();
        assert_eq!(
            o,
            Outcome::ok(format!(
                "compaction of sonnet-1 already running; log {}",
                dir.join(LOG_FILE).display()
            ))
        );
    }

    #[cfg(unix)]
    fn start_with_child(body: &str) -> (Fleet, Outcome) {
        let mut f = Fleet::new();
        let bin = f.w.tmp.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let exe = bin.join("child");
        let warm = crate::test_support::write_warmed(
            &exe,
            &format!("#!/bin/sh\nif [ \"$1\" = warm ]; then exit 0; fi\n{body}\n"),
            &["warm"],
        );
        assert!(warm.status.success());
        f.exe = exe;
        f.start_timeout = Duration::from_secs(20);
        f.pane("sonnet-1");
        f.w.worker("r-1", "sonnet-1", "claude", "sonnet", Some(SMALL));
        f.handoff(&f.w.project(), "sonnet-1");
        let o = f.run("sonnet-1", |_| {});
        (f, o)
    }

    #[cfg(unix)]
    #[test]
    fn a_child_that_exits_0_means_a_job_already_runs() {
        let (f, o) = start_with_child("exit 0");
        let log = f.env().job_dir("sonnet-1").join(LOG_FILE);
        assert_eq!(o, Outcome::ok(already_running("sonnet-1", &log)));
        assert!(f.w.events("r-1", EVENT_FAILED).is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn a_child_that_reports_itself_returns_its_line_with_no_second_event() {
        let (f, o) = start_with_child(&format!(
            "echo '2026 start [horch] BLOCKED: boom'\nexit {EXIT_REPORTED}"
        ));
        assert_eq!(
            o,
            Outcome {
                code: 1,
                out: vec!["[horch] BLOCKED: boom".into()],
                err: vec![],
            }
        );
        assert!(f.w.events("r-1", EVENT_FAILED).is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn a_child_that_exits_reported_with_no_report_fails_the_start() {
        let (f, o) = start_with_child(&format!("exit {EXIT_REPORTED}"));
        assert_eq!(o.code, 1, "{o:?}");
        assert_eq!(
            f.w.events("r-1", EVENT_FAILED),
            [format!(
                "step start: the job exited with {EXIT_REPORTED} and no report in its log"
            )]
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_child_that_exits_as_a_refused_spawn_is_not_read_as_a_report() {
        let refused = horch::exit::REFUSED;
        let (f, o) = start_with_child(&format!(
            "echo '2026 start [horch] BLOCKED: boom'\nexit {refused}"
        ));
        assert_eq!(o.code, 1, "{o:?}");
        assert_eq!(
            f.w.events("r-1", EVENT_FAILED),
            [format!(
                "step start: the job exited with {refused} before it started"
            )]
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_child_that_exits_with_another_code_or_a_signal_fails_the_start() {
        let (f, o) = start_with_child("exit 7");
        assert_eq!(o.code, 1, "{o:?}");
        assert_eq!(
            f.w.events("r-1", EVENT_FAILED),
            ["step start: the job exited with 7 before it started"]
        );
        let (f, o) = start_with_child("kill -9 $$");
        assert_eq!(o.code, 1, "{o:?}");
        assert_eq!(
            f.w.events("r-1", EVENT_FAILED),
            ["step start: the job exited with a signal before it started"]
        );
    }
}
