//! `horch compact`: ask a session to prepare, and compact it in place
//! (CTX-11, CTX-13 to CTX-16, CTX-18; design §6.8, §6.8a, §6.10).
//!
//! - `horch compact <role> --request` types the rendered `request` line into
//!   the role's pane and records `compact-requested`. Asking twice in 1
//!   compaction cycle types nothing (rule A).
//! - `horch compact <role>` checks the record, the pane, the route and the
//!   handoff, then starts the job: a detached `horch compact <role>
//!   --foreground`. It prints `scheduled` only when the child wrote
//!   `job.json` at step `wait-idle` within [`START_TIMEOUT`]; else it records
//!   `compact-failed` and prints the rendered `failed` line. A start that
//!   works returns at once; a failed start returns within the timeout.
//! - `--foreground` is that child. It checks again, holds `job.json`, beats
//!   the shared heartbeat (`crate::heartbeat` in core, as the judge job
//!   does) and runs `compaction::job::run` with the real ports. Every exit
//!   but a second job for the role (`AlreadyExists`) ends in 1 event and 1
//!   report to the orchestrator pane.
//!
//! The rules (states, rule A, the handoff rule, the job steps) are in
//! `horch_core::compaction`; this module gathers inputs and prints.

use std::cell::RefCell;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitCode, Stdio};
use std::time::{Duration, Instant, SystemTime};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use horch_core::compaction::job::{self, JobPlan, JobPorts, Step};
use horch_core::compaction::jobfile::{self, JobRecord, Started, JOB_FILE, LOG_FILE};
use horch_core::compaction::policy::{
    self, HandoffProblem, JobLiveness, EVENT_FAILED, EVENT_REQUESTED, EVENT_WARNED,
};
use horch_core::execution::legacy::Record;
use horch_core::execution::records::Ledger;
use horch_core::harness::codex::PaneMode;
use horch_core::harness::HarnessKind;
use horch_core::messaging::delivery::{self, Readiness, Timing};
use horch_core::messaging::mailbox::Mailbox;
use horch_core::prompts;
use horch_core::runtime::process::spawn_detached;
use horch_core::runtime::RuntimeContext;
use horch_core::telemetry::context::{read_current, Reading};
use horch_core::telemetry::readers::Unreadable;
use horch_core::usage::Locations;
use horch_core::workspace::client::WorkspaceClient;
use horch_core::workspace::herdr::Herdr;

use super::context::{self, build_row, record_kind, record_workdir, Row, Sources};
use super::mode;

/// The start handshake: the child must write `job.json` at step
/// `wait-idle` within this (design §6.8a). The wait ends as soon as the job
/// file appears, so only a failed start waits this long. 5 s, not the
/// design's 1.5 s: at load 211 a child once needed more than 1.5 s.
pub const START_TIMEOUT: Duration = Duration::from_secs(5);

/// The poll of the start handshake.
const START_POLL: Duration = Duration::from_millis(50);

/// The child's exit code when it has recorded `compact-failed` and sent the
/// `failed` report itself.
pub const EXIT_REPORTED: u8 = 3;

/// The job's waits (design §6.8).
const POLL: Duration = Duration::from_secs(2);
const IDLE_POLLS: u8 = 2;
const IDLE_TIMEOUT_S: u64 = 1800;
const RESEND_AFTER: Duration = Duration::from_secs(30);
const MARKER_TIMEOUT: Duration = Duration::from_secs(600);
const SETTLE_TIMEOUT: Duration = Duration::from_secs(600);
const READING_TIMEOUT: Duration = Duration::from_secs(180);

/// The command line of `horch compact`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompactArgs {
    pub role: String,
    pub request: bool,
    pub force: bool,
    pub foreground: bool,
    /// `--timeout`: the wait for idle before the send, in seconds.
    pub timeout: Option<u64>,
}

/// What 1 run prints and its exit code. Tests read it; the CLI prints it.
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
}

/// Everything `horch compact` works with. Tests pass the fake workspace,
/// a temp mailbox, a test exe and a start timeout.
pub(crate) struct Env<'a> {
    pub src: Sources<'a>,
    pub ws: &'a dyn WorkspaceClient,
    pub mailbox: &'a Mailbox,
    /// The `horch` that runs the detached child.
    pub exe: PathBuf,
    pub start_timeout: Duration,
    /// The waits of a `horch mode` switch of a plan pane (X6).
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

    /// The rendered `failed` line.
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

/// `horch compact`.
pub fn compact(ctx: &RuntimeContext, args: &CompactArgs) -> Result<ExitCode> {
    let herdr = Herdr::with_bin(&ctx.bins.harness.herdr);
    let mailbox = match Mailbox::resolve_in(&herdr, ctx) {
        Ok(m) => m,
        Err(e) => return Ok(print(Outcome::refused(format!("{e:#}")))),
    };
    let ledger = Ledger::open_in(ctx)?;
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

fn print(o: Outcome) -> ExitCode {
    for l in &o.out {
        println!("{l}");
    }
    for l in &o.err {
        eprintln!("{l}");
    }
    ExitCode::from(o.code)
}

/// The 3 forms, in the order of design §6.10.
pub(crate) fn run(env: &Env, args: &CompactArgs) -> Outcome {
    if args.request {
        request(env, &args.role)
    } else if args.foreground {
        foreground(env, args)
    } else {
        schedule(env, args)
    }
}

/// The record and pane a compaction targets.
struct Target {
    row: Row,
    pane: String,
}

/// Step 1: a live record in this workspace, its pane, and a harness with a
/// compact command (`--request`: any harness but `none`). The text is the
/// refusal reason.
fn target(env: &Env, role: &str) -> Result<Target, String> {
    target_for(env, role, true)
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

/// Steps 3 and 4: the in-place route and the handoff guard, unless forced.
fn route_and_handoff(env: &Env, t: &Target, force: bool) -> Result<(), String> {
    if force {
        return Ok(());
    }
    let role = &t.row.record.role;
    if !env.src.roster.compacts_in_place(t.row.kind) {
        return Err(format!(
            "refused: {} uses the fresh route; tell {role} to run horch done",
            t.row.kind.as_str()
        ));
    }
    let path = handoff_file(env.src.ctx, &t.row.record);
    let mtime = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
    let asked = policy::asked_at(&t.row.record.history);
    policy::handoff_fresh(mtime, asked, SystemTime::now()).map_err(|p| {
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
    })
}

/// The handoff file of a record, resolved against its workdir (review
/// finding 14), never against this process's cwd.
fn handoff_file(ctx: &RuntimeContext, record: &Record) -> PathBuf {
    record_workdir(ctx, record).join(policy::handoff_path(&record.role))
}

// ─── --request ──────────────────────────────────────────────────────────────

fn request(env: &Env, role: &str) -> Outcome {
    if role == "orchestrator" {
        return Outcome::refused(
            "refused: the orchestrator is not asked; it compacts itself at its stopping point",
        );
    }
    let t = match target_for(env, role, false) {
        Ok(t) => t,
        Err(why) => return Outcome::refused(why),
    };
    let record = &t.row.record;
    let last = t
        .row
        .reading
        .as_ref()
        .ok()
        .and_then(|r| r.last_compaction.as_ref());
    // A Codex pane with `permission_mode: plan` in read-only cannot write
    // its handoff (X6). It is switched to write below; a warning in this
    // cycle does not stop that, as the warned pane could not act on it.
    let read_only = mode::is_plan_pane(env.src.roster, record)
        && mode::current_mode(&record.history) == PaneMode::Plan;
    let warned_only = record
        .history
        .iter()
        .rev()
        .find(|e| e.event == EVENT_REQUESTED || e.event == EVENT_WARNED)
        .is_some_and(|e| e.event == EVENT_WARNED);
    if policy::already_asked(&record.history, last) && !(read_only && warned_only) {
        let at = policy::asked_at(&record.history)
            .map(|s| horch_core::clock::stamp(DateTime::<Utc>::from(s)))
            .unwrap_or_else(|| "an earlier time".into());
        return Outcome::ok(format!("already asked {role} at {at}"));
    }
    let tokens = t
        .row
        .tokens()
        .map_or_else(|| "unknown".to_string(), |n| n.to_string());
    let threshold = t.row.threshold.to_string();
    let handoff = policy::handoff_path(role);
    let line = match env.message(
        "request",
        &[
            ("role", role),
            ("tokens", &tokens),
            ("threshold", &threshold),
            ("handoff", &handoff),
        ],
    ) {
        Ok(l) => l,
        Err(e) => return Outcome::refused(format!("{e:#}")),
    };
    if read_only {
        if let Err(e) = mode::switch_record(
            env.src.ledger,
            env.ws,
            record,
            &t.pane,
            PaneMode::Write,
            &env.mode_timing,
        ) {
            return Outcome::refused(format!("switching {role} to write: {e}"));
        }
    }
    if let Err(e) = delivery::send_line_when_ready(
        env.ws,
        &t.pane,
        &line,
        Duration::ZERO,
        &Readiness::DEFAULT,
        &Timing::DEFAULT,
    ) {
        return Outcome::refused(format!("{e:#}"));
    }
    if let Err(e) = env.src.ledger.record_event(
        &record.record_id,
        EVENT_REQUESTED,
        &format!("tokens {tokens} threshold {threshold}"),
    ) {
        return Outcome::refused(format!("{e:#}"));
    }
    Outcome::ok(format!("asked {role} to write {handoff}"))
}

// ─── the parent: checks, start, handshake ──────────────────────────────────

fn schedule(env: &Env, args: &CompactArgs) -> Outcome {
    let role = &args.role;
    let t = match target(env, role) {
        Ok(t) => t,
        Err(why) => return Outcome::refused(why),
    };
    let dir = env.job_dir(role);
    let log = dir.join(LOG_FILE);
    // A lost job was claimed by `build_row`; the start below clears its dir.
    if t.row.job == JobLiveness::Live {
        return Outcome::ok(already_running(role, &log));
    }
    if let Err(why) = route_and_handoff(env, &t, args.force) {
        return Outcome::refused(why);
    }
    start(env, args, &t.row.record, &dir)
}

fn already_running(role: &str, log: &Path) -> String {
    format!(
        "compaction of {role} already running; log {}",
        log.display()
    )
}

/// Spawn the child under the start lock, then wait for its handshake.
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
    let reason = match (started, child) {
        (Ok(Started::AlreadyRunning), _) => return Outcome::ok(already_running(role, &log)),
        (Ok(Started::Spawned { .. }), Some(child)) => match handshake(env, dir, child) {
            Handshake::Scheduled => {
                return Outcome::ok(format!(
                    "compaction of {role} scheduled; log {}",
                    log.display()
                ))
            }
            Handshake::AlreadyRunning => return Outcome::ok(already_running(role, &log)),
            Handshake::Reported(line) => {
                return Outcome {
                    code: 1,
                    out: vec![line],
                    err: vec![],
                }
            }
            Handshake::Failed(reason) => reason,
        },
        (Ok(Started::Spawned { .. }), None) => "the job was not spawned".to_string(),
        (Err(e), _) => format!("{e:#}"),
    };
    jobfile::clear_on_exit(dir);
    let text = format!("step {}: {reason}", Step::Start);
    let _ = env
        .src
        .ledger
        .record_event(&record.record_id, EVENT_FAILED, &text);
    let line = env
        .failed_line(role, Step::Start.as_str(), &reason, &log)
        .unwrap_or_else(|_| {
            format!(
                "[horch] BLOCKED: Compaction of {role} failed at step start: {reason}. Log: {}.",
                log.display()
            )
        });
    Outcome {
        code: 1,
        out: vec![line],
        err: vec![],
    }
}

/// `<exe> compact <role> --foreground [--force] [--timeout N]`, detached,
/// stdin null, stdout and stderr appended to `job.log`.
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
    // `spawn_detached` scrubs the forbidden keys itself.
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
    /// The child reported itself: the `failed` line it sent.
    Reported(String),
    Failed(String),
}

/// Poll every 50 ms: `job.json` past step `start` is a start; a child that
/// exits first says why by its code; past the timeout the child is stopped.
fn handshake(env: &Env, dir: &Path, mut child: Child) -> Handshake {
    let begin = Instant::now();
    loop {
        if job_started(dir) {
            reap(child);
            return Handshake::Scheduled;
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                return match status.code() {
                    Some(0) => Handshake::AlreadyRunning,
                    Some(c) if c == i32::from(EXIT_REPORTED) => {
                        match last_blocked_line(&dir.join(LOG_FILE)) {
                            Some(line) => Handshake::Reported(line),
                            None => Handshake::Failed(format!(
                                "the job exited with {c} and no report in its log"
                            )),
                        }
                    }
                    Some(c) => {
                        Handshake::Failed(format!("the job exited with {c} before it started"))
                    }
                    None => {
                        Handshake::Failed("the job exited with a signal before it started".into())
                    }
                };
            }
            Ok(None) => {}
            Err(e) => return Handshake::Failed(format!("waiting for the job: {e}")),
        }
        if begin.elapsed() >= env.start_timeout {
            // The handle is not reaped yet, so its pid is still this child.
            let _ = child.kill();
            let _ = child.wait();
            return Handshake::Failed(format!(
                "the job did not start in {} s",
                env.start_timeout.as_secs_f64()
            ));
        }
        std::thread::sleep(START_POLL);
    }
}

/// The started child runs on; a thread reaps it if this process lives on.
fn reap(mut child: Child) {
    std::thread::spawn(move || {
        let _ = child.wait();
    });
}

/// `job.json` exists with a step past `start`.
fn job_started(dir: &Path) -> bool {
    std::fs::read(dir.join(JOB_FILE))
        .ok()
        .and_then(|b| serde_json::from_slice::<JobRecord>(&b).ok())
        .is_some_and(|j| j.step != Step::Start.as_str())
}

/// The last `[horch] BLOCKED:` line of the job log, from its tag on.
fn last_blocked_line(log: &Path) -> Option<String> {
    let text = std::fs::read_to_string(log).ok()?;
    text.lines()
        .rev()
        .find_map(|l| l.find("[horch] BLOCKED:").map(|i| l[i..].to_string()))
}

// ─── the child: --foreground ───────────────────────────────────────────────

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
    // `job.json` first, at step `start`: a second job for the role stops
    // here with no event. The parent waits for step `wait-idle`.
    let mut job = JobRecord {
        v: jobfile::JOB_VERSION,
        role: role.clone(),
        record_id: record
            .as_ref()
            .map(|r| r.record_id.clone())
            .unwrap_or_default(),
        started_at: horch_core::clock::stamp(Utc::now()),
        step: Step::Start.as_str().into(),
    };
    let created = horch_core::fsx::ensure_private_dir(&dir)
        .map_err(|e| format!("{e:#}"))
        .and_then(|_| {
            jobfile::create_job(&dir, &job).map_err(|e| {
                if e.kind() == std::io::ErrorKind::AlreadyExists {
                    String::new()
                } else {
                    format!("creating {}: {e}", dir.join(JOB_FILE).display())
                }
            })
        });
    match created {
        Err(why) if why.is_empty() => return Outcome::code(0),
        Err(why) => return refuse_child(env, role, record.as_ref(), &log, &why),
        Ok(()) => {}
    }
    let ready = target(env, role).and_then(|t| {
        route_and_handoff(env, &t, args.force)?;
        let plan = plan_for(env, &t, args.timeout).map_err(|e| format!("{e:#}"))?;
        Ok((t, plan))
    });
    let (t, plan) = match ready {
        Ok(x) => x,
        Err(why) => {
            jobfile::clear_on_exit(&dir);
            return refuse_child(env, role, record.as_ref(), &log, &why);
        }
    };
    job.step = Step::WaitIdle.as_str().into();
    if let Err(e) = jobfile::write_step(&dir, &job) {
        jobfile::clear_on_exit(&dir);
        return refuse_child(env, role, Some(&t.row.record), &log, &format!("{e:#}"));
    }
    let beat = horch_core::heartbeat::start(&dir);
    let ports = Ports {
        env,
        record: &t.row.record,
        pane: &t.pane,
        log: &log,
        dir: &dir,
        job: RefCell::new(job),
        begin: Instant::now(),
    };
    let result = job::run(&ports, &plan);
    beat.stop();
    jobfile::clear_on_exit(&dir);
    match result {
        Ok(_) => Outcome::code(0),
        Err(_) => Outcome::code(EXIT_REPORTED),
    }
}

/// A child that refuses before its first job step: `compact-failed`
/// `step start: <reason>` on the record (when there is one), the `failed`
/// report to the orchestrator pane, the same line in `job.log`, exit
/// [`EXIT_REPORTED`].
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
                "[horch] BLOCKED: Compaction of {role} failed at step start: {reason}. Log: {}.",
                log.display()
            )
        });
    append_log(log, Step::Start, &line);
    if let Err(e) = report(env, &line) {
        append_log(log, Step::Start, &format!("not sent: {e:#}"));
    }
    Outcome::code(EXIT_REPORTED)
}

/// Send 1 untagged line to the mailbox's `orchestrator` pane. Not
/// `messaging::tell`: that would add a `[<role>]` tag.
fn report(env: &Env, line: &str) -> Result<()> {
    let pane = env
        .mailbox
        .pane_for("orchestrator")
        .context("no orchestrator is registered in this workspace")?;
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

/// The job plan of a target: its compact line (with `instructions`, or
/// `instructions-orchestrator` for an orchestrator record), the resume line
/// and the 2 report templates.
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
        resume_line: env.message("resume", &[("role", role), ("handoff", &handoff)])?,
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

/// The real [`JobPorts`].
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

    /// Unwrapped, so a long failure line stays on 1 line.
    fn pane_text(&self) -> Result<String> {
        self.env.ws.pane_read(self.pane, "recent-unwrapped")
    }

    fn reading(&self) -> Result<Reading, Unreadable> {
        // The session id can arrive after the launch (Codex): read it again.
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
            .record_event(&self.record.record_id, event, text)
    }

    fn report(&self, line: &str) -> Result<()> {
        append_log(self.log, Step::Report, line);
        report(self.env, line)
    }

    /// A plan pane that `--request` switched to write goes back to plan
    /// (X6). The record is read again: a manual `horch mode` counts too.
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

    /// 1 log line; a new job step is written to `job.json` too.
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

    /// A world with a fake herdr workspace and a mailbox in `w1`.
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

        /// Register `role` on a new fake pane whose agent is idle.
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

        /// [`Fleet::env`] over another workspace, for example a simulated
        /// Codex pane.
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

        /// A Codex teammate `codex-plan` with `permission_mode: plan`, and
        /// its live record `r-1` as role `codex-plan-1` on pane `p1`.
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

        /// The names of the record's mode and request events, in order.
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

        /// The lines typed into `pane`.
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

    /// X6: a Codex pane with `permission_mode: plan` is read-only and cannot
    /// write its handoff. `--request` switches it to the fleet write profile
    /// first, then types the request.
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

    /// A failed switch refuses the request: no request line, no event.
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

    /// Rule A exception: a warning counts as asked, but a pane still in
    /// plan mode could not write its handoff. So `--request` switches and
    /// asks; a second `--request` (now in write mode) is already asked.
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

    /// A plan pane already in write mode gets the request without a switch.
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

    /// The job's port of a plan pane, on `ws`.
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

    /// X6: after a `--request` switched a plan pane to write, the job's
    /// port switches it back to plan (`Read Only`) and records the event.
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

    /// A pane that was not switched (or not a plan pane) has nothing to
    /// restore: no key.
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

    /// CTX-11, CTX-22: the request is typed and recorded once per cycle.
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

    /// CTX-22: the exact texts of the request and warning events.
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

    /// CTX-14: a missing, a stale and a pre-request handoff are refused.
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

        let asked = horch_core::clock::stamp(Utc::now() + chrono::Duration::seconds(5));
        f.w.event_at("r-1", &asked, EVENT_REQUESTED, "tokens 1 threshold 2");
        set_mtime(&path, Duration::ZERO);
        check(format!(
            "horch compact: refused: handoff {} is older than the request at {asked}",
            path.display()
        ));
    }

    /// CTX-14 (review finding 14): the handoff is looked for in the
    /// record's workdir, not in the project or the cwd.
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
        // Only in the project (the cwd here): missing, with the path in A.
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

    /// CTX-15 (review finding 1 a): a child that writes no `job.json` is
    /// stopped at the start timeout (1.5 s here, so the test stays short),
    /// and the start fails with 1 event and 1 line.
    #[cfg(unix)]
    #[test]
    fn ctx_15_compact_start_handshake_fails_without_job_file() {
        let mut f = Fleet::new();
        let bin = f.w.tmp.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let exe = bin.join("sleeper");
        std::fs::write(&exe, "#!/bin/sh\nsleep 30\n").unwrap();
        horch_core::runtime::process::make_executable(&exe).unwrap();
        f.exe = exe;
        f.start_timeout = Duration::from_millis(1500);
        f.pane("sonnet-1");
        f.w.worker("r-1", "sonnet-1", "claude", "sonnet", Some(SMALL));
        f.handoff(&f.w.project(), "sonnet-1");

        let begin = Instant::now();
        let o = f.run("sonnet-1", |_| {});
        let took = begin.elapsed();
        assert!(
            took >= f.start_timeout && took < Duration::from_secs(2),
            "{took:?}"
        );
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

    /// CTX-15: a child that writes `job.json` at `wait-idle` in time is
    /// scheduled, with no event.
    #[cfg(unix)]
    #[test]
    fn ctx_15_compact_start_handshake_accepts_wait_idle() {
        let mut f = Fleet::new();
        let dir = f.env().job_dir("sonnet-1");
        let bin = f.w.tmp.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let exe = bin.join("starter");
        std::fs::write(
            &exe,
            format!(
                "#!/bin/sh\nsleep 0.2\nprintf '%s' '{{\"v\":1,\"role\":\"sonnet-1\",\"record_id\":\"r-1\",\"started_at\":\"x\",\"step\":\"wait-idle\"}}' > '{}/job.json'\n",
                dir.display()
            ),
        )
        .unwrap();
        horch_core::runtime::process::make_executable(&exe).unwrap();
        f.exe = exe;
        // The success path does not depend on the timeout; a loaded test
        // machine can start a shell slowly.
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

    /// CTX-15 (review finding 1 b): a child that refuses records 1 event,
    /// reports 1 line and exits 3; a second child while `job.json` exists
    /// exits 0 with no event.
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

    /// CTX-16: a harness that is not in `in_place` takes the fresh route,
    /// unless forced.
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

    /// CTX-18: the orchestrator compacts in its own pane, on its own
    /// record, with its own keep-list; it is never asked.
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
}
