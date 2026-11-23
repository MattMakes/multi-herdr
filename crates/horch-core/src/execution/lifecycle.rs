use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use anyhow::{bail, Context, Result};
use horch_platform::start::{self, Command};
use serde::{Deserialize, Serialize};

use crate::execution::legacy::HistoryEntry;
use crate::execution::store::ExecutionStore;
use crate::execution::SessionMode;
use crate::fleetd::fleet_runtime::adapters::channel_client;
use crate::fleetd::fleet_runtime::domain::pass::START_TIMEOUT_S;
use crate::fleetd::task_execution::domain::context_limit;
use crate::fleetd::worker_profiles::domain::profile::Mode;
use crate::harness::launch::{self, DiscoveryTarget, LaunchRequest};
use crate::harness::HarnessKind;
use crate::ids::SessionId;
use crate::messaging::brief::Brief;
use crate::messaging::mailbox::Mailbox;
use crate::messaging::message::strip_done_prefix;
use crate::messaging::report;
use crate::roster::{HorchAccess, Roster, Teammate};
use crate::runtime::{process, RuntimeContext};
use crate::taskledger::supervisor::{AgentFacts, AttemptFiles, Checkins, OutcomeHint};
use crate::workspace::client::WorkspaceClient;
use crate::workspace::host::PaneHost;
use crate::workspace::model::Pane;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReportTarget {
    Orchestrator,
    None,
}

#[derive(Debug, Clone)]
pub struct DoneRequest<'a> {
    pub record_id: &'a str,
    pub role: &'a str,
    pub pane: &'a str,
    pub workspace: Option<&'a str>,
    pub summary: &'a str,
    pub report_to: ReportTarget,
    pub unlanded: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimLine {
    pub branch: String,
    pub short_sha: String,
    pub ahead: usize,
    pub base: String,
    pub path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReportStatus {
    Delivered,
    Queued(String),
}

pub trait DoneSteps {
    fn open_claims(&self, _record_id: &str) -> Result<Vec<ClaimLine>> {
        Ok(Vec::new())
    }
    fn mark_unlanded(&self, _record_id: &str, _reason: &str) -> Result<Vec<ClaimLine>> {
        Ok(Vec::new())
    }
    fn unregistered(&self) -> Result<Vec<String>> {
        Ok(Vec::new())
    }
    fn mark_done(&self, record_id: &str, summary: &str) -> Result<()>;
    fn queue_report(&self, line: &str) -> Result<()>;
    fn push_report(&self) -> ReportStatus {
        ReportStatus::Delivered
    }
    fn after_mark_done(&self) {}
    fn recorded(&self, _record_id: &str) -> Result<bool> {
        Ok(false)
    }
    fn report_failed(&self, _record_id: &str, _error: &str) {}
    fn report_queued(&self, _record_id: &str, _error: &str) {}
    fn unregister(&self, workspace: &str, role: &str);
    fn try_unregister(&self, workspace: &str, role: &str) -> Result<()> {
        self.unregister(workspace, role);
        Ok(())
    }
    fn settle(&self, workspace: &str);
    fn record_session(&self, _workspace: &str, _role: &str, _record_id: &str) -> Result<()> {
        Ok(())
    }
    fn record_run(&self, _record_id: &str) -> Result<()> {
        Ok(())
    }
}

pub fn done(ws: &dyn WorkspaceClient, steps: &dyn DoneSteps, req: &DoneRequest) -> Result<()> {
    if steps.recorded(req.record_id)? {
        eprintln!("horch done: an earlier horch done recorded this summary; closing the pane");
    } else {
        report_and_record(steps, req)?;
    }
    close(ws, steps, req)
}

fn warn_err(result: Result<()>, what: impl std::fmt::Display) {
    if let Err(e) = result {
        eprintln!("{what}: {e:#}");
    }
}

fn warn_worker(role: &str, what: &str, result: Result<()>) {
    warn_err(result, format!("horch worker[{role}]: {what}"));
}

fn report_and_record(steps: &dyn DoneSteps, req: &DoneRequest) -> Result<()> {
    let open = steps.open_claims(req.record_id)?;
    refuse_open_claims(&open, req.unlanded)?;
    let summary = done_summary(req, &open, unregistered_entries(steps));
    queue_done_report(steps, req, &summary)?;
    if let Some(reason) = req.unlanded {
        steps.mark_unlanded(req.record_id, reason)?;
    }
    steps.mark_done(req.record_id, &summary)?;
    steps.after_mark_done();
    if req.report_to == ReportTarget::Orchestrator {
        note_queued(steps, req, steps.push_report());
    }
    Ok(())
}

fn describe_claim(claim: &ClaimLine) -> String {
    format!(
        "{} at {} ({} commits ahead of {}), worktree {}",
        claim.branch,
        claim.short_sha,
        claim.ahead,
        claim.base,
        claim.path.display()
    )
}

fn refuse_open_claims(open: &[ClaimLine], unlanded: Option<&str>) -> Result<()> {
    if open.is_empty() || unlanded.is_some() {
        return Ok(());
    }
    let claims = open
        .iter()
        .map(describe_claim)
        .collect::<Vec<_>>()
        .join("\n");
    bail!(
        "horch done refused because this record has an open worktree claim:\n{claims}\n\
         Run: horch worktree land\n\
         If it cannot land: horch done --unlanded \"<reason>\" \"<summary>\""
    );
}

fn unregistered_entries(steps: &dyn DoneSteps) -> Vec<String> {
    steps.unregistered().unwrap_or_else(|error| {
        eprintln!("horch done: could not inspect unregistered worktrees ({error:#})");
        Vec::new()
    })
}

fn done_summary(req: &DoneRequest, open: &[ClaimLine], unregistered: Vec<String>) -> String {
    let mut summary = strip_done_prefix(req.summary, req.role).to_owned();
    if let Some(reason) = req.unlanded {
        for claim in open {
            summary.push_str(&format!(" | UNLANDED: {}: {reason}", describe_claim(claim)));
        }
    }
    for entry in unregistered {
        summary.push_str(&format!(" | UNREGISTERED: {entry}"));
    }
    summary
}

fn queue_done_report(steps: &dyn DoneSteps, req: &DoneRequest, summary: &str) -> Result<()> {
    if req.report_to != ReportTarget::Orchestrator {
        return Ok(());
    }
    steps
        .queue_report(&format!("[{}] DONE: {summary}", req.role))
        .inspect_err(|e| steps.report_failed(req.record_id, &format!("{e:#}")))
        .map_err(|e| {
            e.context(
                "horch done: the report did not reach the orchestrator.\n\
                 This pane stays open. The record is not done. Your role stays registered.\n\
                 Fix the cause if you can, then run the same horch done command again.\n\
                 If it fails again, record your summary with horch note and end your turn.\n\
                 Do not close this pane.\n\
                 The cause",
            )
        })
}

fn note_queued(steps: &dyn DoneSteps, req: &DoneRequest, status: ReportStatus) {
    if let ReportStatus::Queued(cause) = status {
        eprintln!("horch done: queued, not yet acknowledged ({cause}); redelivered until horch inbox reads it");
        steps.report_queued(req.record_id, &cause);
    }
}

const HERDR_UNREACHABLE: &str =
    "herdr is unreachable: the summary is recorded, but this pane may still be open";

fn close(ws: &dyn WorkspaceClient, steps: &dyn DoneSteps, req: &DoneRequest) -> Result<()> {
    let (pane, unreachable) = lookup_pane(ws, req.pane);
    let workspace = req
        .workspace
        .map(str::to_owned)
        .or_else(|| pane.as_ref().and_then(|p| p.workspace_id.clone()));
    let workspace = match (workspace, &pane) {
        (Some(workspace), _) => Some(workspace),
        (None, Some(_)) => bail!("could not resolve this pane's workspace"),
        (None, None) => None,
    };

    if let Some(workspace) = &workspace {
        if let Err(e) = steps.record_session(workspace, req.role, req.record_id) {
            eprintln!("horch done: could not record the session id ({e:#}); shutting down anyway");
        }
        warn_err(steps.try_unregister(workspace, req.role), "horch done");

        steps.settle(workspace);
    }

    warn_err(
        steps.record_run(req.record_id),
        "horch: NOTE: fleet run row not written",
    );

    close_pane(ws, pane, unreachable)
}

fn lookup_pane(ws: &dyn WorkspaceClient, pane: &str) -> (Option<Pane>, Option<anyhow::Error>) {
    match ws.pane_get(pane) {
        Ok(pane) => (Some(pane), None),
        Err(e) if !ws.server_reachable() => {
            eprintln!(
                "horch done: herdr is unreachable ({e:#}); the summary is recorded, \
                 but this pane may still be open"
            );
            (None, Some(e))
        }
        Err(e) => {
            eprintln!("horch done: this pane is gone ({e:#}); it is closed already");
            (None, None)
        }
    }
}

fn close_pane(
    ws: &dyn WorkspaceClient,
    pane: Option<Pane>,
    unreachable: Option<anyhow::Error>,
) -> Result<()> {
    match (unreachable, pane) {
        (Some(e), _) => Err(e.context(HERDR_UNREACHABLE)),
        (None, None) => Ok(()),
        (None, Some(pane)) => match ws.pane_close(&pane.pane_id) {
            Err(e) => closed_or(ws, &pane.pane_id, e),
            Ok(_) => Ok(()),
        },
    }
}

fn closed_or(ws: &dyn WorkspaceClient, pane_id: &str, e: anyhow::Error) -> Result<()> {
    if ws.pane_get(pane_id).is_ok() {
        Err(e)
    } else if !ws.server_reachable() {
        Err(e.context(HERDR_UNREACHABLE))
    } else {
        Ok(())
    }
}

pub trait WorkerSteps {
    fn load_brief(&mut self) -> Result<Brief>;
    fn enter_context(&mut self, brief: &Brief) -> Result<()>;
    fn register(&mut self, brief: &Brief) -> Result<()>;
    fn set_running(&mut self, brief: &Brief) -> Result<()>;
    fn launch(&mut self, brief: &Brief) -> Result<Option<i32>>;
    fn agent_exited(&mut self, brief: &Brief, code: Option<i32>) -> Result<()>;
    fn startup_failed(&mut self, _brief: &Brief, _error: &str) -> Result<()> {
        Ok(())
    }
    fn close_pane(&mut self) -> Result<()> {
        Ok(())
    }
    fn leave_startup_error(&mut self, _error: &str, _brief: Option<&Brief>) -> Result<()> {
        Ok(())
    }
}

pub fn run_worker(steps: &mut dyn WorkerSteps) -> Result<i32> {
    let brief = steps.load_brief().inspect_err(|e| {
        warn_err(
            steps.leave_startup_error(&format!("{e:#}"), None),
            "horch worker: could not leave the startup error",
        )
    })?;
    steps
        .enter_context(&brief)
        .and_then(|()| steps.register(&brief))
        .inspect_err(|e| fail_startup(steps, &brief, e))?;
    warn_worker(
        &brief.role,
        "recording the start failed",
        steps.set_running(&brief),
    );
    let code = steps.launch(&brief).inspect_err(|_| {
        warn_worker(
            &brief.role,
            "recording the exit failed",
            steps.agent_exited(&brief, None),
        )
    })?;
    warn_worker(
        &brief.role,
        "recording the exit failed",
        steps.agent_exited(&brief, code),
    );
    Ok(code.unwrap_or(1))
}

fn fail_startup(steps: &mut dyn WorkerSteps, brief: &Brief, e: &anyhow::Error) {
    let role = &brief.role;
    warn_worker(
        role,
        "recording the startup failure failed",
        steps
            .startup_failed(brief, &format!("{e:#}"))
            .and_then(|()| steps.agent_exited(brief, None)),
    );
    eprintln!("horch worker[{role}]: startup failed: {e:#}");
    warn_worker(
        role,
        "could not leave the startup error",
        steps.leave_startup_error(&format!("{e:#}"), Some(brief)),
    );
    warn_worker(role, "could not close the pane", steps.close_pane());
}

pub const EVENT_STARTUP_FAILED: &str = "startup-failed";

pub fn record_startup_failure(store: &ExecutionStore, key: &str, error: &str) -> Result<()> {
    let at = crate::clock::now_stamp();
    store.update_key(key, |r| {
        r.updated_at = at.clone();
        r.history.push(HistoryEntry {
            at: at.clone(),
            event: EVENT_STARTUP_FAILED.to_string(),
            text: error.to_string(),
        });
    })
}

pub const EVENT_EXITED: &str = "exited";

pub fn record_early_exit(store: &ExecutionStore, key: &str, code: Option<i32>) -> Result<()> {
    let at = crate::clock::now_stamp();
    let text = match code {
        Some(code) => format!("agent exited with code {code} before horch done"),
        None => "agent was ended by a signal before horch done".to_string(),
    };
    store.update_key(key, |r| {
        r.updated_at = at.clone();
        r.history.push(HistoryEntry {
            at: at.clone(),
            event: EVENT_EXITED.to_string(),
            text: text.clone(),
        });
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartupKey<'a> {
    Record(&'a str),
    Pane(&'a str),
}

impl StartupKey<'_> {
    fn file_name(&self) -> Option<String> {
        match *self {
            StartupKey::Record(id) => is_plain(id).then(|| format!("record-{id}.txt")),
            StartupKey::Pane(pane) => {
                let name = pane.replace(':', "_");
                is_plain(&name).then(|| format!("pane-{name}.txt"))
            }
        }
    }
}

fn is_plain(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

pub fn startup_error_path(data_root: &Path, key: StartupKey<'_>) -> Option<PathBuf> {
    Some(data_root.join("startup-errors").join(key.file_name()?))
}

pub fn write_startup_error(data_root: &Path, key: StartupKey<'_>, error: &str) -> Result<()> {
    let path = startup_error_path(data_root, key)
        .with_context(|| format!("{key:?} does not name a startup error file"))?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    crate::fsx::write_atomic(&path, error.as_bytes(), 0o600)
        .with_context(|| format!("writing {}", path.display()))
}

pub fn take_startup_error(
    data_root: &Path,
    key: StartupKey<'_>,
    since: SystemTime,
) -> Option<String> {
    let path = startup_error_path(data_root, key)?;
    let written = std::fs::metadata(&path).and_then(|m| m.modified()).ok()?;
    let since = since
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| SystemTime::UNIX_EPOCH + Duration::from_secs(d.as_secs()))
        .unwrap_or(since);
    let text = (written >= since)
        .then(|| std::fs::read_to_string(&path).ok())
        .flatten();
    let _ = std::fs::remove_file(&path);
    text.map(|t| t.trim_end().to_string())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkerStart {
    Registered,
    Failed(String),
    Waiting,
}

pub fn await_worker(
    data_root: &Path,
    keys: &[StartupKey<'_>],
    since: SystemTime,
    limit: Duration,
    poll: Duration,
    registered: &mut dyn FnMut() -> bool,
) -> WorkerStart {
    let until = Instant::now() + limit;
    loop {
        if let Some(start) = poll_start(data_root, keys, since, registered) {
            break start;
        }
        let left = until.saturating_duration_since(Instant::now());
        if left.is_zero() {
            break WorkerStart::Waiting;
        }
        std::thread::sleep(poll.min(left));
    }
}

fn poll_start(
    data_root: &Path,
    keys: &[StartupKey<'_>],
    since: SystemTime,
    registered: &mut dyn FnMut() -> bool,
) -> Option<WorkerStart> {
    let error = keys
        .iter()
        .find_map(|key| take_startup_error(data_root, *key, since));
    match error {
        Some(error) => Some(WorkerStart::Failed(error)),
        None => registered().then_some(WorkerStart::Registered),
    }
}

struct AgentPlan<'a> {
    roster: &'a Roster,
    teammate: &'a Teammate,
    prompt: &'a str,
}

fn run_agent(
    ctx: &RuntimeContext,
    brief: &Brief,
    mailbox: &Mailbox,
    plan: &AgentPlan<'_>,
    env: Vec<(String, String)>,
    checkins: Option<Checkins>,
) -> Result<Option<i32>> {
    let (roster, teammate) = (plan.roster, plan.teammate);
    let code = launch::run_flow_code(
        ctx,
        LaunchRequest {
            role: &brief.role,
            teammate,
            session: &brief.session,
            prompt: plan.prompt,
            model_override: None,
            exec_rules: roster.exec_rules(),
            child_env: env,
            record: Some(DiscoveryTarget {
                mailbox,
                record_id: &brief.record_id,
                workdir: brief.workdir_or_project(),
            }),
            fleet_window: roster
                .fleet_window(teammate, teammate.model.as_deref().unwrap_or_default()),
            briefing_skill: None,
        },
    );
    if let Some(c) = checkins {
        c.stop();
    }
    code
}

pub const TRANSCRIPT_JSONL: &str = "transcript.jsonl";

const START_MD: &str = "inbox/START.md";

const PANE_LINE_MAX: usize = 120;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecCli {
    Claude,
    Codex,
    Antigravity,
}

const EXEC_CLIS: [(HarnessKind, ExecCli); 3] = [
    (HarnessKind::Claude, ExecCli::Claude),
    (HarnessKind::Codex, ExecCli::Codex),
    (HarnessKind::Antigravity, ExecCli::Antigravity),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamEvent {
    Activity,
    ContextTokens(u64),
    UsageLimited,
    Result,
    Other,
}

impl StreamEvent {
    pub fn is_activity(self) -> bool {
        matches!(self, StreamEvent::Activity | StreamEvent::ContextTokens(_))
    }

    fn word(self) -> String {
        match self {
            StreamEvent::Activity => "activity".into(),
            StreamEvent::ContextTokens(n) => format!("context {n} tokens"),
            StreamEvent::UsageLimited => "usage limit".into(),
            StreamEvent::Result => "result".into(),
            StreamEvent::Other => "other".into(),
        }
    }
}

impl ExecCli {
    pub fn of(teammate: &Teammate) -> Option<ExecCli> {
        if teammate.profile().mode != Mode::Exec {
            return None;
        }
        EXEC_CLIS
            .iter()
            .find(|(kind, _)| *kind == teammate.agent)
            .map(|(_, cli)| *cli)
    }

    fn compacts(self) -> bool {
        match self {
            ExecCli::Claude | ExecCli::Codex => true,
            ExecCli::Antigravity => false,
        }
    }

    fn stop(self, event: StreamEvent) -> Option<Stop> {
        match event {
            StreamEvent::UsageLimited => Some(Stop::UsageLimited),
            StreamEvent::ContextTokens(n) if !self.compacts() && context_limit::reached(n) => {
                Some(Stop::ContextLimit)
            }
            _ => None,
        }
    }

    pub fn read(self, line: &str) -> StreamEvent {
        use crate::harness::{antigravity as agy, claude, codex};
        match self {
            ExecCli::Claude => match claude::read_event(line) {
                claude::Event::Activity => StreamEvent::Activity,
                claude::Event::ContextTokens(n) => StreamEvent::ContextTokens(n),
                claude::Event::UsageLimited => StreamEvent::UsageLimited,
                claude::Event::Result => StreamEvent::Result,
                claude::Event::Other => StreamEvent::Other,
            },
            ExecCli::Codex => match codex::read_event(line) {
                codex::ExecEvent::Activity => StreamEvent::Activity,
                codex::ExecEvent::ContextTokens(n) => StreamEvent::ContextTokens(n),
                codex::ExecEvent::UsageLimited => StreamEvent::UsageLimited,
                codex::ExecEvent::Result => StreamEvent::Result,
                codex::ExecEvent::Other => StreamEvent::Other,
            },
            ExecCli::Antigravity => match agy::read_event(line) {
                agy::Event::Activity | agy::Event::SoftDenied { .. } => StreamEvent::Activity,
                agy::Event::ContextTokens(n) => StreamEvent::ContextTokens(n),
                agy::Event::UsageLimited { .. } => StreamEvent::UsageLimited,
                agy::Event::Result { .. } => StreamEvent::Result,
                agy::Event::Other => StreamEvent::Other,
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecEnd {
    Exited(Option<i32>),
    UsageLimited(Option<i32>),
    ContextLimit(Option<i32>),
}

impl ExecEnd {
    pub fn code(self) -> Option<i32> {
        match self {
            ExecEnd::Exited(code) | ExecEnd::UsageLimited(code) | ExecEnd::ContextLimit(code) => {
                code
            }
        }
    }

    pub fn outcome_hint(self) -> Option<OutcomeHint> {
        match self {
            ExecEnd::Exited(_) => None,
            ExecEnd::UsageLimited(_) => Some(OutcomeHint::Quota),
            ExecEnd::ContextLimit(_) => Some(OutcomeHint::ContextLimit),
        }
    }

    pub fn repair_applies(self) -> bool {
        matches!(self, ExecEnd::Exited(_))
    }
}

pub struct ExecAttempt<'a> {
    pub cli: ExecCli,
    pub files: &'a AttemptFiles,
    pub facts: &'a AgentFacts,
    pub pane: &'a mut dyn std::io::Write,
}

pub fn run_exec(mut cmd: Command, attempt: &mut ExecAttempt<'_>) -> Result<ExecEnd> {
    let path = attempt.files.dir().join(TRANSCRIPT_JSONL);
    let mut transcript = open_transcript(&path)?;
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped());
    horch_platform::group::new_group(&mut cmd);
    let name = cmd.get_program().to_string_lossy().into_owned();
    let mut child = cmd.spawn().with_context(|| format!("launching {name}"))?;
    attempt.facts.agent_started(child.id());
    let stdout = child.stdout.take().context("the CLI has no stdout pipe")?;
    let read = read_stream(std::io::BufReader::new(stdout), &mut transcript, attempt);
    if !matches!(read, Ok(None)) {
        horch_platform::group::kill_child_group(&mut child);
    }
    let status = child
        .wait()
        .with_context(|| format!("waiting for {name}"))?;
    let code = status.code();
    Ok(match read? {
        None => ExecEnd::Exited(code),
        Some(Stop::UsageLimited) => ExecEnd::UsageLimited(code),
        Some(Stop::ContextLimit) => ExecEnd::ContextLimit(code),
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stop {
    UsageLimited,
    ContextLimit,
}

pub fn write_exec_exit(files: &AttemptFiles, end: ExecEnd, repaired: bool) -> Result<()> {
    files.write_exit_hinted(end.code(), repaired, end.outcome_hint())
}

fn open_transcript(path: &Path) -> Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.create(true).append(true);
    let _ = horch_platform::fs::create_mode(&mut options, 0o600);
    options
        .open(path)
        .with_context(|| format!("opening {}", path.display()))
}

fn read_stream(
    stdout: impl std::io::BufRead,
    transcript: &mut std::fs::File,
    attempt: &mut ExecAttempt<'_>,
) -> Result<Option<Stop>> {
    use std::io::Write as _;
    for line in stdout.split(b'\n') {
        let mut line = line.context("reading the stream of the CLI")?;
        if line.last() == Some(&b'\r') {
            line.pop();
        }
        line.push(b'\n');
        transcript
            .write_all(&line)
            .context("writing the transcript")?;
        let text = String::from_utf8_lossy(&line[..line.len() - 1]);
        let event = attempt.cli.read(&text);
        let _ = writeln!(attempt.pane, "{}", render(event, &text));
        let _ = attempt.pane.flush();
        if event.is_activity() {
            attempt.facts.activity();
            if let Err(e) = attempt.files.write_heartbeat(attempt.facts) {
                eprintln!("horch worker: heartbeat.json is not written: {e:#}");
            }
        }
        if let Some(stop) = attempt.cli.stop(event) {
            return Ok(Some(stop));
        }
    }
    Ok(None)
}

fn render(event: StreamEvent, line: &str) -> String {
    let what = match serde_json::from_str::<serde_json::Value>(line) {
        Ok(v) => {
            let at = |path: &str| v.pointer(path).and_then(serde_json::Value::as_str);
            let kind = at("/type").or_else(|| at("/event")).unwrap_or("?");
            match at("/subtype")
                .or_else(|| at("/item/type"))
                .or_else(|| at("/step_update/step_type"))
            {
                Some(sub) => format!("{kind}/{sub}"),
                None => kind.to_string(),
            }
        }
        Err(_) => line.trim().to_string(),
    };
    format!("[{}] {what}", event.word())
        .chars()
        .take(PANE_LINE_MAX)
        .collect()
}

fn run_exec_agent(
    ctx: &RuntimeContext,
    brief: &Brief,
    mailbox: &Mailbox,
    plan: &AgentPlan<'_>,
    env: Vec<(String, String)>,
    attempt: (ExecCli, &AttemptFiles, &AgentFacts),
) -> Result<ExecEnd> {
    let (cli, files, facts) = attempt;
    let (roster, teammate) = (plan.roster, plan.teammate);
    let req = LaunchRequest {
        role: &brief.role,
        teammate,
        session: &brief.session,
        prompt: plan.prompt,
        model_override: None,
        exec_rules: roster.exec_rules(),
        child_env: env,
        record: Some(DiscoveryTarget {
            mailbox,
            record_id: &brief.record_id,
            workdir: brief.workdir_or_project(),
        }),
        fleet_window: roster.fleet_window(teammate, teammate.model.as_deref().unwrap_or_default()),
        briefing_skill: None,
    };
    launch::run_exec_flow(ctx, req, files.dir(), |cmd| {
        let mut pane = std::io::stdout().lock();
        run_exec(
            cmd,
            &mut ExecAttempt {
                cli,
                files,
                facts,
                pane: &mut pane,
            },
        )
    })
}

pub struct PaneWorker<'a> {
    pub ctx: &'a mut RuntimeContext,
    pub role: &'a str,
    pub enter_dir: &'a dyn Fn(&Path) -> Result<()>,
    mailbox: Option<Mailbox>,
    path: Option<std::ffi::OsString>,
    ledger: Option<(PathBuf, String)>,
    facts: Option<Arc<AgentFacts>>,
    repaired: bool,
    exec: Option<(AttemptFiles, ExecEnd)>,
}

impl<'a> PaneWorker<'a> {
    pub fn new(
        ctx: &'a mut RuntimeContext,
        role: &'a str,
        enter_dir: &'a dyn Fn(&Path) -> Result<()>,
    ) -> Self {
        Self {
            ctx,
            role,
            enter_dir,
            mailbox: None,
            path: None,
            ledger: None,
            facts: None,
            repaired: false,
            exec: None,
        }
    }

    pub fn exec_end(&self) -> Option<ExecEnd> {
        self.exec.as_ref().map(|(_, end)| *end)
    }

    fn exec_plan(&self, teammate: &Teammate, resume: bool) -> Option<(ExecCli, AttemptFiles)> {
        let cli = ExecCli::of(teammate)?;
        if resume && cli != ExecCli::Claude {
            return None;
        }
        let (db, id) = self.ledger.as_ref()?;
        let files = match AttemptFiles::of_worker(db, id, &self.ctx.paths.fleet_root) {
            Ok(files) => files?,
            Err(e) => {
                eprintln!("horch worker[{}]: no exec launch: {e:#}", self.role);
                return None;
            }
        };
        if !files.dir().join(START_MD).is_file() {
            eprintln!(
                "horch worker[{}]: no exec launch: {} is missing",
                self.role,
                files.dir().join(START_MD).display()
            );
            return None;
        }
        Some((cli, files))
    }

    pub fn mark_repaired(&mut self) {
        self.repaired = true;
    }

    pub fn repaired(&self) -> bool {
        self.repaired
    }

    fn write_exit(&self, brief: &Brief, code: Option<i32>) {
        let Some((db, id)) = self.ledger.as_ref() else {
            return;
        };
        let fleet_root = &self.ctx.paths.fleet_root;
        let written = match &self.exec {
            Some((files, end)) => write_exec_exit(files, *end, self.repaired).map(|()| true),
            None => {
                crate::taskledger::supervisor::write_exit(db, id, fleet_root, code, self.repaired)
            }
        };
        if let Err(e) = written {
            eprintln!(
                "horch worker[{}]: exit.json of the attempt is not written: {e:#}",
                brief.role
            );
        }
    }

    fn has_attempt(&self) -> bool {
        self.ledger
            .as_ref()
            .is_some_and(|(db, id)| worker_has_attempt(db, id, &self.ctx.paths.fleet_root))
    }

    fn load_roster(&self, brief: &Brief) -> Result<(Roster, Teammate)> {
        let ctx: &RuntimeContext = self.ctx;
        let roster = Roster::load_layered(
            ctx.inherited.home_var.as_deref().map(Path::new),
            ctx.bins.roster_override.as_deref(),
            brief.teammates_dir.as_deref(),
        )?;
        for w in roster.load_warnings() {
            eprintln!("warning: {w}");
        }
        let teammate = match &brief.resolved {
            Some(t) => t.clone(),
            None => roster.require(&brief.teammate)?.clone(),
        };
        Ok((roster, teammate))
    }

    fn worker_prompt(&self, brief: &Brief, roster: &Roster, teammate: &Teammate) -> Result<String> {
        let ctx: &RuntimeContext = self.ctx;
        crate::prompts::worker_prompt_with_context(
            roster,
            teammate,
            self.role,
            &brief.task,
            &brief.session,
            &crate::project_notes::display(&ctx.paths.state_root, &ctx.paths.project()?)?,
            &crate::project_notes::host_line(),
        )
    }

    fn start_checkins(&self, brief: &Brief, facts: Arc<AgentFacts>) -> Option<Checkins> {
        let (db, id) = self.ledger.clone()?;
        let fleet_root = self.ctx.paths.fleet_root.clone();
        let observed = facts.clone();
        launch::observe_agent_start(move |pid| observed.agent_started(pid));
        let every = crate::taskledger::supervisor::interval(&db, &id, &fleet_root);
        let lookup = session_lookup(
            self.ctx.clone(),
            brief.record_id.clone(),
            brief.agent.clone(),
        );
        let writer = crate::taskledger::supervisor::HeartbeatWriter::new(
            db,
            id,
            fleet_root,
            facts,
            brief.agent.clone(),
            lookup,
        )
        .with_record_id(brief.record_id.clone());
        Some(crate::taskledger::supervisor::start_heartbeats(
            writer, every,
        ))
    }

    fn wait_recorded(&self) -> Result<()> {
        let Some((db, id)) = &self.ledger else {
            return Ok(());
        };
        let me = std::process::id();
        let start = crate::procid::start_time(me)
            .context("the start time of this wrapper is unreadable, so the agent does not start")?;
        let limit = Duration::from_secs(START_TIMEOUT_S.unsigned_abs());
        crate::taskledger::supervisor::wait_recorded(db, id, (me, start), limit)
    }

    fn run_smoke_agent(
        &self,
        env: &[(String, String)],
        checkins: Option<Checkins>,
    ) -> Result<Option<i32>> {
        if let Some(facts) = &self.facts {
            facts.agent_started(std::process::id());
        }
        let out = run_smoke(env).map(|()| Some(0));
        drop(checkins);
        out
    }

    pub fn ledger(&self) -> Option<(&Path, &str)> {
        self.ledger
            .as_ref()
            .map(|(db, id)| (db.as_path(), id.as_str()))
    }

    pub fn resume(&mut self, brief: &Brief, prompt: &str) -> Result<Option<i32>> {
        let id = match brief.session.id() {
            Some(id) => id.clone(),
            None => {
                let record = self.store()?.get(&brief.record_id)?;
                let id = record
                    .session_id
                    .context("the launch did not record the session id of the agent")?;
                SessionId::new(id)?
            }
        };
        let resumed = Brief {
            session: SessionMode::Resume(id),
            ..brief.clone()
        };
        self.run_launch(&resumed, Some(prompt))
    }

    fn run_launch(&mut self, brief: &Brief, prompt: Option<&str>) -> Result<Option<i32>> {
        let pane = self.ctx.herdr.pane.as_ref().map(|p| p.as_ref().to_string());
        let facts = self
            .facts
            .get_or_insert_with(|| AgentFacts::new(pane))
            .clone();
        let ctx: &RuntimeContext = self.ctx;
        let mailbox = self.mailbox()?;
        let (roster, teammate) = self.load_roster(brief)?;
        let exec = self.exec_plan(&teammate, prompt.is_some());
        let prompt = match (prompt, &exec) {
            (Some(line), _) => line.to_string(),
            (None, Some((_, files))) => files.dir().join(START_MD).to_string_lossy().into_owned(),
            (None, None) => self.worker_prompt(brief, &roster, &teammate)?,
        };
        if let Some(workdir) = &brief.workdir {
            (self.enter_dir)(Path::new(workdir))
                .with_context(|| format!("entering workdir {workdir}"))?;
        }
        let env = child_env(mailbox, brief, self.path.clone());
        let _report = start_report_server(ctx, brief, &teammate, &env)?;
        let checkins = self.start_checkins(brief, facts.clone());
        if teammate.agent != HarnessKind::None {
            self.wait_recorded()?;
        }
        if teammate.agent == HarnessKind::None {
            self.run_smoke_agent(&env, checkins)
        } else if let Some((cli, files)) = exec {
            let plan = AgentPlan {
                roster: &roster,
                teammate: &teammate,
                prompt: &prompt,
            };
            let end = run_exec_agent(ctx, brief, mailbox, &plan, env, (cli, &files, &facts));
            if let Some(c) = checkins {
                c.stop();
            }
            let end = end?;
            self.exec.get_or_insert((files, end));
            Ok(end.code())
        } else {
            let plan = AgentPlan {
                roster: &roster,
                teammate: &teammate,
                prompt: &prompt,
            };
            run_agent(ctx, brief, mailbox, &plan, env, checkins)
        }
    }

    fn herdr(&self) -> PaneHost {
        PaneHost::of(self.ctx)
    }

    fn mailbox(&self) -> Result<&Mailbox> {
        self.mailbox.as_ref().context("the brief is not loaded")
    }

    fn store(&self) -> Result<ExecutionStore> {
        ExecutionStore::open_in(self.ctx)
    }
}

impl WorkerSteps for PaneWorker<'_> {
    fn load_brief(&mut self) -> Result<Brief> {
        channel_client::mark_wrapper_process();
        let mailbox = Mailbox::resolve_in(&self.herdr(), self.ctx)?;
        let brief = mailbox.read_brief(self.role)?;
        self.mailbox = Some(mailbox);
        Ok(brief)
    }

    fn enter_context(&mut self, brief: &Brief) -> Result<()> {
        let project = PathBuf::from(&brief.project_dir);
        self.ctx.paths.project_dir = Some(project.clone());
        if let Some(db) = &brief.ledger_db {
            self.ctx.paths.ledger_db = PathBuf::from(db);
            self.ctx.paths.ledger_override = Some(PathBuf::from(db));
            self.ctx.paths.fleet_root = Path::new(db)
                .parent()
                .unwrap_or(Path::new(""))
                .join("fleet");
        }
        if let Some(root) = &brief.fleet_root {
            if !Path::new(root).is_absolute() {
                bail!("the brief fleet root '{root}' is not absolute");
            }
            self.ctx.paths.fleet_root = PathBuf::from(root);
        }
        if let Some(id) = &brief.worker_id {
            self.ledger = Some((self.ctx.paths.ledger_db.clone(), id.clone()));
        }
        if let Some(state_dir) = &brief.state_dir {
            self.ctx.paths.set_state_dir(state_dir);
        }
        self.ctx.herdr.backend = brief.backend;
        self.ctx.apply_overrides(&brief.overrides());
        if !project.is_dir() {
            bail!("project dir '{}' is missing", brief.project_dir);
        }
        (self.enter_dir)(&project)
            .with_context(|| format!("entering project dir {}", brief.project_dir))?;
        self.ctx.paths.cwd = Some(project);
        self.path = self.ctx.prepend_own_dir_to_path();
        Ok(())
    }

    fn register(&mut self, _brief: &Brief) -> Result<()> {
        if self.role == "orchestrator" {
            bail!("a worker cannot register the role orchestrator: only horch fleet takes it");
        }
        let herdr = self.herdr();
        let (mailbox, _pane) = Mailbox::register_in(&herdr, self.ctx, self.role)?;
        self.mailbox = Some(mailbox);
        Ok(())
    }

    fn set_running(&mut self, _brief: &Brief) -> Result<()> {
        Ok(())
    }

    fn launch(&mut self, brief: &Brief) -> Result<Option<i32>> {
        self.run_launch(brief, None)
    }

    fn agent_exited(&mut self, brief: &Brief, code: Option<i32>) -> Result<()> {
        if self.has_attempt() {
            self.write_exit(brief, code);
        }
        Ok(())
    }

    fn startup_failed(&mut self, _brief: &Brief, _error: &str) -> Result<()> {
        Ok(())
    }

    fn leave_startup_error(&mut self, error: &str, brief: Option<&Brief>) -> Result<()> {
        let data_root = self.ctx.paths.data_root.clone();
        if let Some(brief) = brief {
            return write_startup_error(&data_root, StartupKey::Record(&brief.record_id), error);
        }
        let pane = self
            .ctx
            .herdr
            .pane
            .as_ref()
            .context("not inside a herdr pane or a Herdr app terminal (HERDR_PANE_ID and HC_TERMINAL_ID are unset), so horch spawn cannot be told")?;
        let public = self.herdr().pane_get(pane.as_ref())?.pane_id;
        write_startup_error(&data_root, StartupKey::Pane(&public), error)
    }

    fn close_pane(&mut self) -> Result<()> {
        let pane = self
            .ctx
            .herdr
            .pane
            .as_ref()
            .context("not inside a herdr pane or a Herdr app terminal (HERDR_PANE_ID and HC_TERMINAL_ID are unset)")?;
        let herdr = self.herdr();
        let public = herdr
            .pane_get(pane.as_ref())
            .map(|p| p.pane_id)
            .unwrap_or_else(|_| pane.as_ref().to_string());
        herdr.pane_close(&public)
    }
}

fn report_target(brief: &Brief) -> Option<String> {
    report::target(brief)
}

fn report_args(request: &report::Request) -> Vec<&str> {
    match request {
        report::Request::Progress { text } => vec!["ledger", "progress", text],
        report::Request::Tell { to, text } => vec!["tell", to, text],
        report::Request::Done { text } => vec!["done", text],
    }
}

fn start_report_server(
    ctx: &RuntimeContext,
    brief: &Brief,
    teammate: &Teammate,
    env: &[(String, String)],
) -> Result<Option<report::Server>> {
    if teammate.horch_access != HorchAccess::Report {
        return Ok(None);
    }
    if !horch_platform::ipc::is_supported() {
        bail!("horch_access: report needs a Unix host");
    }
    let path = report::socket_path(&ctx.paths.state_root, &brief.record_id);
    let env = env.to_vec();
    let exec: Box<report::Executor> =
        Box::new(move |request: &report::Request| run_horch_quiet(&env, &report_args(request)));
    report::Server::start(path, report_target(brief), exec).map(Some)
}

fn worker_has_attempt(db: &Path, worker_id: &str, fleet_root: &Path) -> bool {
    matches!(
        crate::taskledger::supervisor::AttemptFiles::of_worker(db, worker_id, fleet_root),
        Ok(Some(_))
    )
}

fn session_lookup(
    ctx: RuntimeContext,
    record_id: String,
    agent: String,
) -> crate::taskledger::supervisor::SessionLookup {
    Box::new(move || {
        let sid = ExecutionStore::open_in(&ctx)
            .ok()
            .and_then(|s| s.get(&record_id).ok())
            .and_then(|r| r.session_id)
            .filter(|s| !s.is_empty());
        let transcript = sid.as_deref().and_then(|sid| {
            crate::usage::read_session(
                &crate::usage::Locations::from_context(&ctx),
                &agent,
                Some(sid),
            )
            .ok()
            .map(|(p, _)| p.to_string_lossy().into_owned())
        });
        (sid, transcript)
    })
}

pub fn child_env(
    mailbox: &Mailbox,
    brief: &Brief,
    path: Option<std::ffi::OsString>,
) -> Vec<(String, String)> {
    let mut env = brief.transport_env();
    env.push((
        "HORCH_WORKSPACE_ID".into(),
        mailbox.workspace_id().to_string(),
    ));
    if let Some(path) = path {
        env.push(("PATH".into(), path.to_string_lossy().into_owned()));
    }
    env
}

fn run_smoke(env: &[(String, String)]) -> Result<()> {
    run_horch(
        env,
        &[
            "note",
            "smoke: worker launched, brief read, ledger reachable",
        ],
    )?;
    std::thread::sleep(Duration::from_secs(1));
    let mut env = env.to_vec();
    env.push(("HORCH_TELL_GRACE_MS".into(), "0".into()));
    run_horch(&env, &["done", "smoke: machinery verified end to end"])
}

fn run_horch_quiet(env: &[(String, String)], args: &[&str]) -> Result<()> {
    let out = horch_command(env)
        .args(args)
        .output()
        .context("running horch")?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        bail!("horch {} failed: {}", args[0], stderr.trim());
    }
    Ok(())
}

fn horch_command(env: &[(String, String)]) -> Command {
    let mut cmd = start::command("horch");
    process::inherit_env(&mut cmd, env.iter().map(|(k, v)| (k, v)));
    cmd.env_remove(report::ENV);
    cmd
}

fn run_horch(env: &[(String, String)], args: &[&str]) -> Result<()> {
    let mut cmd = horch_command(env);
    let status = cmd.args(args).status().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            anyhow::anyhow!(
                "`horch` is not on PATH, so an agent in this pane could not reach the \
                 orchestrator either. Run `horch install` to put it on PATH."
            )
        } else {
            anyhow::Error::new(e).context("running horch")
        }
    })?;
    if !status.success() {
        bail!("horch {} failed: {status}", args.join(" "));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution::SessionMode;
    use crate::harness::launch::{agent_command, Session};
    use crate::messaging::brief::SCHEMA;
    use crate::runtime::{BinOverrides, MapEnv};
    use crate::workspace::testing::FakeWorkspace;
    use std::cell::RefCell;

    fn process_env() -> Vec<(std::ffi::OsString, std::ffi::OsString)> {
        let mut vars: Vec<_> = std::env::vars_os().collect();
        vars.sort();
        vars
    }

    struct ClaimSteps {
        open: RefCell<Result<Vec<ClaimLine>, String>>,
        marked_unlanded: RefCell<Vec<String>>,
        marked_done: RefCell<Vec<String>>,
        reports: RefCell<Vec<String>>,
        unregistered: RefCell<Result<Vec<String>, String>>,
        recorded: bool,
        fail_report: bool,
        report_failures: RefCell<Vec<String>>,
        push_misses: Option<String>,
        queued: RefCell<Vec<String>>,
        order: RefCell<Vec<&'static str>>,
        fail_mark_done: bool,
    }

    impl Default for ClaimSteps {
        fn default() -> Self {
            Self {
                open: RefCell::new(Ok(Vec::new())),
                marked_unlanded: RefCell::new(Vec::new()),
                marked_done: RefCell::new(Vec::new()),
                reports: RefCell::new(Vec::new()),
                unregistered: RefCell::new(Ok(Vec::new())),
                recorded: false,
                fail_report: false,
                report_failures: RefCell::new(Vec::new()),
                push_misses: None,
                queued: RefCell::new(Vec::new()),
                order: RefCell::new(Vec::new()),
                fail_mark_done: false,
            }
        }
    }

    impl ClaimSteps {
        fn with_claim() -> Self {
            Self {
                open: RefCell::new(Ok(vec![ClaimLine {
                    branch: "wt/sonnet-1-unit".into(),
                    short_sha: "abc1234".into(),
                    ahead: 2,
                    base: "main".into(),
                    path: PathBuf::from("/repo/.worktrees/sonnet-1-unit"),
                }])),
                ..Self::default()
            }
        }
    }

    impl DoneSteps for ClaimSteps {
        fn open_claims(&self, _: &str) -> Result<Vec<ClaimLine>> {
            self.open.borrow().clone().map_err(anyhow::Error::msg)
        }

        fn mark_unlanded(&self, _: &str, reason: &str) -> Result<Vec<ClaimLine>> {
            self.marked_unlanded.borrow_mut().push(reason.into());
            self.open_claims("r1")
        }

        fn unregistered(&self) -> Result<Vec<String>> {
            self.unregistered
                .borrow()
                .clone()
                .map_err(anyhow::Error::msg)
        }

        fn mark_done(&self, _: &str, summary: &str) -> Result<()> {
            self.marked_done.borrow_mut().push(summary.into());
            self.order.borrow_mut().push("mark_done");
            if self.fail_mark_done {
                bail!("the ledger is locked");
            }
            Ok(())
        }

        fn queue_report(&self, line: &str) -> Result<()> {
            self.reports.borrow_mut().push(line.into());
            if self.fail_report {
                bail!("channel relay refused");
            }
            Ok(())
        }

        fn recorded(&self, _: &str) -> Result<bool> {
            Ok(self.recorded)
        }

        fn report_failed(&self, _: &str, error: &str) {
            self.report_failures.borrow_mut().push(error.into());
        }

        fn after_mark_done(&self) {
            self.order.borrow_mut().push("after_mark_done");
        }

        fn push_report(&self) -> ReportStatus {
            self.order.borrow_mut().push("push");
            match &self.push_misses {
                Some(cause) => ReportStatus::Queued(cause.clone()),
                None => ReportStatus::Delivered,
            }
        }

        fn report_queued(&self, _: &str, error: &str) {
            self.queued.borrow_mut().push(error.into());
        }

        fn unregister(&self, _: &str, _: &str) {
            self.order.borrow_mut().push("unregister");
        }
        fn settle(&self, _: &str) {}
    }

    fn claim_request<'a>(pane: &'a str, unlanded: Option<&'a str>) -> DoneRequest<'a> {
        DoneRequest {
            record_id: "r1",
            role: "sonnet-1",
            pane,
            workspace: None,
            summary: "finished",
            report_to: ReportTarget::Orchestrator,
            unlanded,
        }
    }

    #[test]
    fn arc_31_done_refuses_with_an_open_claim() {
        let ws = FakeWorkspace::new();
        let steps = ClaimSteps::with_claim();
        let error = done(&ws, &steps, &claim_request("p1", None)).unwrap_err();
        let text = error.to_string();
        assert!(text.contains("wt/sonnet-1-unit at abc1234"), "{text}");
        assert!(text.contains("Run: horch worktree land"), "{text}");
        assert!(
            text.contains("horch done --unlanded \"<reason>\" \"<summary>\""),
            "{text}"
        );
        assert!(steps.marked_done.borrow().is_empty());
        assert!(steps.reports.borrow().is_empty());
    }

    #[test]
    fn arc_31_done_unlanded_marks_and_reports_one_line() {
        let ws = FakeWorkspace::new();
        let pane = ws.workspace_create("w", None, false).unwrap().root_pane_id;
        let steps = ClaimSteps::with_claim();
        done(&ws, &steps, &claim_request(&pane, Some("cannot rebase"))).unwrap();
        assert_eq!(&*steps.marked_unlanded.borrow(), &["cannot rebase"]);
        let reports = steps.reports.borrow();
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].lines().count(), 1);
        assert!(reports[0].contains(" | UNLANDED: wt/sonnet-1-unit at abc1234 (2 commits ahead of main), worktree /repo/.worktrees/sonnet-1-unit: cannot rebase"));
    }

    #[test]
    fn arc_31_done_without_claims_is_unchanged() {
        let ws = FakeWorkspace::new();
        let pane = ws.workspace_create("w", None, false).unwrap().root_pane_id;
        let steps = ClaimSteps::default();
        done(&ws, &steps, &claim_request(&pane, None)).unwrap();
        assert_eq!(&*steps.marked_done.borrow(), &["finished"]);
        assert_eq!(&*steps.reports.borrow(), &["[sonnet-1] DONE: finished"]);
    }

    #[test]
    fn w2_failed_report_leaves_the_claim_open_for_the_retry() {
        let ws = FakeWorkspace::new();
        let pane = ws.workspace_create("w", None, false).unwrap().root_pane_id;
        let steps = ClaimSteps {
            fail_report: true,
            ..ClaimSteps::with_claim()
        };
        let error = done(&ws, &steps, &claim_request(&pane, Some("cannot rebase"))).unwrap_err();
        assert!(
            format!("{error:#}").contains("This pane stays open. The record is not done."),
            "{error:#}"
        );
        assert!(
            format!("{error:#}").ends_with("The cause: channel relay refused"),
            "{error:#}"
        );
        assert!(steps.marked_unlanded.borrow().is_empty());
        assert!(steps.marked_done.borrow().is_empty());
        assert_eq!(steps.report_failures.borrow().len(), 1);
        assert!(steps.report_failures.borrow()[0].contains("channel relay refused"));
        assert_eq!(ws.pane_ids(), vec![pane.clone()], "the pane stays open");

        let retry = ClaimSteps::with_claim();
        done(&ws, &retry, &claim_request(&pane, Some("cannot rebase"))).unwrap();
        assert_eq!(&*retry.reports.borrow(), &*steps.reports.borrow());
        assert_eq!(&*retry.marked_unlanded.borrow(), &["cannot rebase"]);
        assert_eq!(retry.marked_done.borrow().len(), 1);
        assert!(ws.pane_ids().is_empty(), "the retry closes the pane");
    }

    #[test]
    fn done_marks_the_record_before_the_push() {
        let ws = FakeWorkspace::new();
        let pane = ws.workspace_create("w", None, false).unwrap().root_pane_id;
        let steps = ClaimSteps {
            push_misses: Some("the push budget ran out".into()),
            ..ClaimSteps::default()
        };
        done(&ws, &steps, &claim_request(&pane, None)).unwrap();
        assert_eq!(
            &*steps.order.borrow(),
            &["mark_done", "after_mark_done", "push", "unregister"]
        );
        assert_eq!(&*steps.queued.borrow(), &["the push budget ran out"]);
        assert!(ws.pane_ids().is_empty(), "the pane is closed");
    }

    #[test]
    fn done_pushes_between_the_record_and_the_close() {
        let ws = FakeWorkspace::new();
        let pane = ws.workspace_create("w", None, false).unwrap().root_pane_id;
        let steps = ClaimSteps::default();
        done(&ws, &steps, &claim_request(&pane, None)).unwrap();
        assert_eq!(
            &*steps.order.borrow(),
            &["mark_done", "after_mark_done", "push", "unregister"]
        );

        let pane = ws.workspace_create("w", None, false).unwrap().root_pane_id;
        let silent = ClaimSteps::default();
        let req = DoneRequest {
            report_to: ReportTarget::None,
            ..claim_request(&pane, None)
        };
        done(&ws, &silent, &req).unwrap();
        assert_eq!(
            &*silent.order.borrow(),
            &["mark_done", "after_mark_done", "unregister"]
        );
    }

    #[test]
    fn done_stops_before_the_push_when_the_record_is_not_written() {
        let ws = FakeWorkspace::new();
        let pane = ws.workspace_create("w", None, false).unwrap().root_pane_id;
        let steps = ClaimSteps {
            fail_mark_done: true,
            ..ClaimSteps::default()
        };
        let error = done(&ws, &steps, &claim_request(&pane, None)).unwrap_err();
        assert_eq!(error.to_string(), "the ledger is locked");
        assert_eq!(steps.reports.borrow().len(), 1, "the report is queued");
        assert_eq!(&*steps.order.borrow(), &["mark_done"], "no push, no close");
        assert_eq!(ws.pane_ids(), vec![pane], "the pane stays open");
    }

    #[test]
    fn done_does_not_push_a_report_that_failed() {
        let ws = FakeWorkspace::new();
        let pane = ws.workspace_create("w", None, false).unwrap().root_pane_id;
        let steps = ClaimSteps {
            fail_report: true,
            ..ClaimSteps::default()
        };
        done(&ws, &steps, &claim_request(&pane, None)).unwrap_err();
        assert!(steps.order.borrow().is_empty(), "no mark, no push");
    }

    #[test]
    fn w2_done_retry_after_a_recorded_done_only_closes() {
        let ws = FakeWorkspace::new();
        let pane = ws.workspace_create("w", None, false).unwrap().root_pane_id;
        let steps = ClaimSteps {
            recorded: true,
            ..ClaimSteps::default()
        };
        done(&ws, &steps, &claim_request(&pane, None)).unwrap();
        assert!(steps.reports.borrow().is_empty());
        assert!(steps.marked_done.borrow().is_empty());
        assert!(ws.pane_ids().is_empty(), "the pane is closed");
    }

    #[test]
    fn arc_31_done_refuses_when_claims_cannot_be_read() {
        let ws = FakeWorkspace::new();
        let steps = ClaimSteps {
            open: RefCell::new(Err("registry is unreadable".into())),
            ..ClaimSteps::default()
        };
        let error = done(&ws, &steps, &claim_request("p1", None)).unwrap_err();
        assert_eq!(error.to_string(), "registry is unreadable");
        assert!(steps.marked_done.borrow().is_empty());
        assert!(steps.reports.borrow().is_empty());
    }

    #[test]
    fn arc_31_done_reports_unregistered_worktrees() {
        let ws = FakeWorkspace::new();
        let pane = ws.workspace_create("w", None, false).unwrap().root_pane_id;
        let steps = ClaimSteps {
            unregistered: RefCell::new(Ok(vec![
                "/repo/.worktrees/manual branch manual ahead 1 dirty 0".into(),
            ])),
            ..ClaimSteps::default()
        };
        done(&ws, &steps, &claim_request(&pane, None)).unwrap();
        assert!(steps.reports.borrow()[0]
            .contains(" | UNREGISTERED: /repo/.worktrees/manual branch manual ahead 1 dirty 0"));
    }

    fn report_brief(report_to: ReportTarget) -> Brief {
        Brief {
            schema: SCHEMA,
            role: "intake-auditor-1".into(),
            teammate: "intake-auditor".into(),
            agent: "claude".into(),
            model: "opus".into(),
            record_id: "r1".into(),
            session: SessionMode::Fresh(None),
            task: "t".into(),
            project_dir: "/p".into(),
            state_dir: None,
            claude_bin: None,
            codex_bin: None,
            resolved: None,
            teammates_dir: None,
            workdir: None,
            bin_overrides: BinOverrides::default(),
            report_to,
            worker_id: None,
            ledger_db: None,
            fleet_root: None,
            controller: None,
            task_yaml: None,
            backend: Default::default(),
        }
    }

    #[test]
    fn a_report_request_maps_to_1_horch_command() {
        let text = || "x".to_string();
        assert_eq!(
            report_args(&report::Request::Progress { text: text() }),
            ["ledger", "progress", "x"]
        );
        assert_eq!(
            report_args(&report::Request::Tell {
                to: "orchestrator".into(),
                text: text()
            }),
            ["tell", "orchestrator", "x"]
        );
        assert_eq!(
            report_args(&report::Request::Done { text: text() }),
            ["done", "x"]
        );
        assert_eq!(
            report_target(&report_brief(ReportTarget::Orchestrator)).as_deref(),
            Some("orchestrator")
        );
        assert_eq!(report_target(&report_brief(ReportTarget::None)), None);
    }

    #[cfg(unix)]
    #[test]
    fn the_parent_serves_the_report_socket_only_for_report() {
        let tmp = tempfile::Builder::new()
            .tempdir_in("/tmp")
            .or_else(|_| tempfile::tempdir())
            .unwrap();
        let mut ctx = RuntimeContext::from_env(&MapEnv::new("/")).unwrap();
        ctx.paths.state_root = tmp.path().to_path_buf();
        let brief = report_brief(ReportTarget::Orchestrator);
        let env = vec![(
            "PATH".to_string(),
            tmp.path().to_string_lossy().into_owned(),
        )];
        let mut t = Teammate::default();
        assert!(start_report_server(&ctx, &brief, &t, &env)
            .unwrap()
            .is_none());

        t.horch_access = HorchAccess::Report;
        let server = start_report_server(&ctx, &brief, &t, &env)
            .unwrap()
            .unwrap();
        let path = report::socket_path(tmp.path(), "r1");
        assert_eq!(server.path(), path);
        let err = report::send(
            &path,
            &report::Request::Tell {
                to: "sonnet-1".into(),
                text: "x".into(),
            },
        )
        .unwrap_err();
        assert!(err.to_string().contains("only to orchestrator"), "{err}");
        let tell = report::Request::Tell {
            to: "orchestrator".into(),
            text: "x".into(),
        };
        let err = report::send(&path, &tell).unwrap_err();
        assert!(err.to_string().contains("running horch"), "{err}");
        drop(server);
        assert!(!path.exists());
    }

    #[test]
    fn arc_06_transport_env_applied_to_child() {
        let before = process_env();
        let tmp = tempfile::tempdir().unwrap();
        let mailbox = Mailbox::under(tmp.path(), "w9");
        let mut brief = Brief {
            schema: SCHEMA,
            role: "sonnet-1".into(),
            teammate: "sonnet".into(),
            agent: "claude".into(),
            model: "sonnet".into(),
            record_id: "r1".into(),
            session: SessionMode::Fresh(Some("s1".parse().unwrap())),
            task: "t".into(),
            project_dir: "/p".into(),
            state_dir: Some("/state".into()),
            claude_bin: None,
            codex_bin: None,
            resolved: None,
            teammates_dir: Some("/roster".into()),
            workdir: None,
            bin_overrides: BinOverrides::default(),
            report_to: crate::execution::ReportTarget::Orchestrator,
            worker_id: None,
            ledger_db: None,
            fleet_root: None,
            controller: None,
            task_yaml: None,
            backend: Default::default(),
        };
        let mut env = MapEnv::new("/");
        for key in BinOverrides::VARS {
            env = env.with(key, &format!("/fake/{key}"));
        }
        brief.set_overrides(BinOverrides::from_env(&env));

        let mut ctx = RuntimeContext::from_env(
            &MapEnv::new("/")
                .with_exe("/opt/horch/bin/horch")
                .with("PATH", "/usr/bin")
                .with("ANTHROPIC_API_KEY", "must-not-leak"),
        )
        .unwrap();
        ctx.apply_overrides(&brief.overrides());
        let path = ctx.prepend_own_dir_to_path();

        let roster = Roster::builtin().unwrap();
        let mut teammate = roster.require("sonnet").unwrap().clone();
        teammate
            .env
            .insert("ANTHROPIC_API_KEY".into(), "from-the-teammate".into());
        let cmd = agent_command(
            &ctx,
            &teammate,
            Session::Fresh("s1"),
            "p",
            None,
            child_env(&mailbox, &brief, path),
        )
        .unwrap();

        let envs: std::collections::BTreeMap<String, Option<String>> = cmd
            .get_envs()
            .map(|(k, v)| {
                (
                    k.to_string_lossy().into_owned(),
                    v.map(|v| v.to_string_lossy().into_owned()),
                )
            })
            .collect();
        for (key, value) in brief.transport_env() {
            assert_eq!(envs.get(&key), Some(&Some(value)), "{key}");
        }
        assert_eq!(envs["HORCH_WORKSPACE_ID"].as_deref(), Some("w9"));
        assert!(envs["PATH"]
            .as_deref()
            .unwrap()
            .starts_with("/opt/horch/bin"));
        assert_eq!(envs.get("ANTHROPIC_API_KEY"), Some(&None), "removed");
        assert_eq!(
            cmd.get_program(),
            std::ffi::OsStr::new("/fake/HORCH_CLAUDE_BIN")
        );
        assert_eq!(process_env(), before, "the test process env did not change");
    }

    struct ClosedElsewhere<'a> {
        ws: &'a FakeWorkspace,
        pane: String,
    }

    impl DoneSteps for ClosedElsewhere<'_> {
        fn mark_done(&self, _: &str, _: &str) -> Result<()> {
            Ok(())
        }
        fn queue_report(&self, _: &str) -> Result<()> {
            Ok(())
        }
        fn unregister(&self, _: &str, _: &str) {}
        fn settle(&self, _: &str) {
            if !self.pane.is_empty() {
                self.ws.pane_close(&self.pane).unwrap();
            }
        }
    }

    #[test]
    fn done_close_is_idempotent() {
        fn request(pane: &str) -> DoneRequest<'_> {
            DoneRequest {
                record_id: "r1",
                role: "candidate-A",
                pane,
                workspace: None,
                summary: "finished",
                report_to: ReportTarget::None,
                unlanded: None,
            }
        }
        fn one_pane() -> (FakeWorkspace, String) {
            let ws = FakeWorkspace::new();
            let pane = ws.workspace_create("w", None, false).unwrap().root_pane_id;
            (ws, pane)
        }

        let (ws, pane) = one_pane();
        let steps = ClosedElsewhere {
            ws: &ws,
            pane: pane.clone(),
        };
        done(&ws, &steps, &request(&pane)).expect("the pane is gone, so it is closed");
        assert!(ws.pane_ids().is_empty());

        let (ws, pane) = one_pane();
        let steps = ClosedElsewhere {
            ws: &ws,
            pane: String::new(),
        };
        ws.fail_next("pane_close", "herdr is down");
        let err = done(&ws, &steps, &request(&pane)).unwrap_err();
        assert!(err.to_string().contains("herdr is down"), "{err}");
        assert_eq!(ws.pane_ids(), [pane]);
    }

    struct UnregisterFails {
        ran: RefCell<Vec<String>>,
    }

    impl DoneSteps for UnregisterFails {
        fn mark_done(&self, _: &str, _: &str) -> Result<()> {
            self.ran.borrow_mut().push("mark_done".into());
            Ok(())
        }
        fn queue_report(&self, _: &str) -> Result<()> {
            Ok(())
        }
        fn unregister(&self, _: &str, _: &str) {}
        fn try_unregister(&self, _: &str, role: &str) -> Result<()> {
            self.ran.borrow_mut().push(format!("unregister {role}"));
            bail!("removing /state/{role}.id failed; role '{role}' stays registered")
        }
        fn settle(&self, _: &str) {
            self.ran.borrow_mut().push("settle".into());
        }
    }

    #[test]
    fn done_closes_the_pane_when_unregister_fails() {
        let ws = FakeWorkspace::new();
        let pane = ws.workspace_create("w", None, false).unwrap().root_pane_id;
        let steps = UnregisterFails {
            ran: RefCell::new(Vec::new()),
        };
        let request = DoneRequest {
            record_id: "r1",
            role: "sonnet-1",
            pane: &pane,
            workspace: None,
            summary: "finished",
            report_to: ReportTarget::None,
            unlanded: None,
        };

        done(&ws, &steps, &request).expect("a failed unregister does not fail done");

        assert!(ws.pane_ids().is_empty());
        assert_eq!(
            *steps.ran.borrow(),
            ["mark_done", "unregister sonnet-1", "settle"]
        );
    }

    struct HerdrDown<'a> {
        ws: &'a FakeWorkspace,
        pane: &'a str,
        close_pane: bool,
        at: Step,
        ran: std::cell::RefCell<Vec<String>>,
    }

    impl HerdrDown<'_> {
        fn reach(&self, step: Step) {
            if step != self.at {
                return;
            }
            if self.close_pane {
                self.ws.pane_close(self.pane).unwrap();
            } else {
                self.ws.set_reachable(false);
            }
        }
    }

    impl DoneSteps for HerdrDown<'_> {
        fn mark_done(&self, _: &str, summary: &str) -> Result<()> {
            self.ran.borrow_mut().push(format!("mark_done {summary}"));
            self.reach(Step::PaneGet);
            Ok(())
        }
        fn queue_report(&self, _: &str) -> Result<()> {
            unreachable!("the request reports to nobody")
        }
        fn record_session(&self, workspace: &str, _: &str, _: &str) -> Result<()> {
            self.ran
                .borrow_mut()
                .push(format!("record_session {workspace}"));
            Ok(())
        }
        fn unregister(&self, workspace: &str, _: &str) {
            self.ran
                .borrow_mut()
                .push(format!("unregister {workspace}"));
        }
        fn settle(&self, workspace: &str) {
            self.ran.borrow_mut().push(format!("settle {workspace}"));
            self.reach(Step::Close);
        }
    }

    #[test]
    fn done_fails_when_herdr_is_unreachable_but_not_when_the_pane_is_gone() {
        for at in [Step::PaneGet, Step::Close] {
            for workspace in [Some("w1"), None] {
                for close_pane in [false, true] {
                    let ws = FakeWorkspace::new();
                    let pane = ws.workspace_create("w", None, false).unwrap().root_pane_id;
                    let steps = HerdrDown {
                        ws: &ws,
                        pane: &pane,
                        close_pane,
                        at,
                        ran: Default::default(),
                    };
                    let req = DoneRequest {
                        record_id: "r1",
                        role: "candidate-A",
                        pane: &pane,
                        workspace,
                        summary: "finished",
                        report_to: ReportTarget::None,
                        unlanded: None,
                    };
                    let case = format!("{at:?}, {workspace:?}, close_pane {close_pane}");
                    let got = done(&ws, &steps, &req);
                    let ran = steps.ran.into_inner();
                    assert_eq!(ran[0], "mark_done finished", "{case}");
                    let known = workspace.is_some() || at == Step::Close;
                    assert_eq!(ran.len(), if known { 4 } else { 1 }, "{case}: {ran:?}");
                    ws.set_reachable(true);
                    if close_pane {
                        got.unwrap_or_else(|e| panic!("{case}: {e:#}"));
                        assert!(ws.pane_ids().is_empty(), "{case}");
                    } else {
                        let err = got.expect_err(&case);
                        assert!(
                            format!("{err:#}").contains("herdr is unreachable"),
                            "{case}: {err:#}"
                        );
                        assert_eq!(ws.pane_ids(), std::slice::from_ref(&pane), "{case}");
                    }
                    assert!(
                        ws.calls().iter().any(|c| c.method == "server_reachable"),
                        "{case}"
                    );
                }
            }
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq)]
    enum Step {
        MarkDone,
        Report,
        PaneGet,
        RecordSession,
        Unregister,
        Settle,
        Close,
    }

    struct VanishBefore<'a> {
        ws: &'a FakeWorkspace,
        pane: &'a str,
        vanish: Step,
        ran: std::cell::RefCell<Vec<String>>,
    }

    impl VanishBefore<'_> {
        fn at(&self, step: Step) {
            if step == self.vanish {
                self.ws.pane_close(self.pane).unwrap();
            }
        }
    }

    impl DoneSteps for VanishBefore<'_> {
        fn mark_done(&self, _: &str, summary: &str) -> Result<()> {
            self.at(Step::MarkDone);
            self.ran.borrow_mut().push(format!("mark_done {summary}"));
            self.at(Step::Report);
            self.at(Step::PaneGet);
            Ok(())
        }
        fn queue_report(&self, _: &str) -> Result<()> {
            unreachable!("the request reports to nobody")
        }
        fn record_session(&self, workspace: &str, _: &str, _: &str) -> Result<()> {
            self.at(Step::RecordSession);
            self.ran
                .borrow_mut()
                .push(format!("record_session {workspace}"));
            Ok(())
        }
        fn unregister(&self, workspace: &str, _: &str) {
            self.at(Step::Unregister);
            self.ran
                .borrow_mut()
                .push(format!("unregister {workspace}"));
        }
        fn settle(&self, workspace: &str) {
            self.at(Step::Settle);
            self.ran.borrow_mut().push(format!("settle {workspace}"));
            self.at(Step::Close);
        }
    }

    #[test]
    fn done_succeeds_when_the_pane_vanishes_before_any_step() {
        use Step::*;
        for vanish in [
            MarkDone,
            Report,
            PaneGet,
            RecordSession,
            Unregister,
            Settle,
            Close,
        ] {
            for workspace in [Some("w1"), None] {
                let ws = FakeWorkspace::new();
                let pane = ws.workspace_create("w", None, false).unwrap().root_pane_id;
                let steps = VanishBefore {
                    ws: &ws,
                    pane: &pane,
                    vanish,
                    ran: Default::default(),
                };
                let req = DoneRequest {
                    record_id: "r1",
                    role: "candidate-A",
                    pane: &pane,
                    workspace,
                    summary: "finished",
                    report_to: ReportTarget::None,
                    unlanded: None,
                };
                done(&ws, &steps, &req)
                    .unwrap_or_else(|e| panic!("vanish before {vanish:?}, {workspace:?}: {e:#}"));
                assert!(ws.pane_ids().is_empty(), "{vanish:?}");
                let ran = steps.ran.into_inner();
                assert_eq!(ran[0], "mark_done finished", "{vanish:?}");
                let known = workspace.is_some() || ![MarkDone, Report, PaneGet].contains(&vanish);
                let expected = if known { 4 } else { 1 };
                assert_eq!(ran.len(), expected, "{vanish:?}, {workspace:?}: {ran:?}");
            }
        }
    }

    mod startup_failure {
        use super::*;

        struct Steps<'a> {
            ws: &'a FakeWorkspace,
            pane: String,
            calls: Vec<&'static str>,
            fail_at: &'static str,
            startup_error: Option<String>,
            left_error: Option<String>,
            left_for: Option<Option<String>>,
        }

        impl Steps<'_> {
            fn brief() -> Brief {
                Brief {
                    schema: SCHEMA,
                    role: "sonnet-1".into(),
                    teammate: "sonnet".into(),
                    agent: "claude".into(),
                    model: "sonnet".into(),
                    record_id: "r1".into(),
                    session: SessionMode::Fresh(Some("s1".parse().unwrap())),
                    task: "t".into(),
                    project_dir: "/p".into(),
                    state_dir: None,
                    claude_bin: None,
                    codex_bin: None,
                    resolved: None,
                    teammates_dir: None,
                    workdir: None,
                    bin_overrides: BinOverrides::default(),
                    report_to: ReportTarget::Orchestrator,
                    worker_id: None,
                    ledger_db: None,
                    fleet_root: None,
                    controller: None,
                    task_yaml: None,
                    backend: Default::default(),
                }
            }
        }

        impl WorkerSteps for Steps<'_> {
            fn load_brief(&mut self) -> Result<Brief> {
                if self.fail_at == "load_brief" {
                    bail!("no brief at /tmp/m/sonnet-1.brief.json");
                }
                Ok(Self::brief())
            }
            fn enter_context(&mut self, _: &Brief) -> Result<()> {
                self.calls.push("enter_context");
                if self.fail_at == "enter_context" {
                    bail!("enter_context failed");
                }
                Ok(())
            }
            fn register(&mut self, _: &Brief) -> Result<()> {
                self.calls.push("register");
                Ok(())
            }
            fn set_running(&mut self, _: &Brief) -> Result<()> {
                self.calls.push("set_running");
                Ok(())
            }
            fn launch(&mut self, _: &Brief) -> Result<Option<i32>> {
                self.calls.push("launch");
                Ok(Some(0))
            }
            fn agent_exited(&mut self, _: &Brief, _: Option<i32>) -> Result<()> {
                self.calls.push("agent_exited");
                Ok(())
            }
            fn startup_failed(&mut self, _: &Brief, error: &str) -> Result<()> {
                self.calls.push("startup_failed");
                self.startup_error = Some(error.to_string());
                Ok(())
            }
            fn close_pane(&mut self) -> Result<()> {
                self.calls.push("close_pane");
                self.ws.pane_close(&self.pane)
            }
            fn leave_startup_error(&mut self, error: &str, brief: Option<&Brief>) -> Result<()> {
                self.calls.push("leave_startup_error");
                self.left_error = Some(error.to_string());
                self.left_for = Some(brief.map(|b| b.record_id.clone()));
                Ok(())
            }
        }

        fn steps<'a>(ws: &'a FakeWorkspace, fail_at: &'static str) -> Steps<'a> {
            let pane = ws.workspace_create("w", None, false).unwrap().root_pane_id;
            Steps {
                ws,
                pane,
                calls: Vec::new(),
                fail_at,
                startup_error: None,
                left_error: None,
                left_for: None,
            }
        }

        #[test]
        fn closes_the_pane_after_it_records_the_failure() {
            let ws = FakeWorkspace::new();
            let mut steps = steps(&ws, "enter_context");
            let e = run_worker(&mut steps).unwrap_err();
            assert_eq!(e.to_string(), "enter_context failed");
            assert_eq!(
                steps.calls,
                [
                    "enter_context",
                    "startup_failed",
                    "agent_exited",
                    "leave_startup_error",
                    "close_pane"
                ]
            );
            assert!(ws.pane_ids().is_empty(), "the pane is closed");
            assert_eq!(steps.left_error.as_deref(), Some("enter_context failed"));
            assert_eq!(steps.left_for, Some(Some("r1".to_string())));
        }

        #[test]
        fn a_missing_brief_leaves_its_error_for_the_spawner() {
            let ws = FakeWorkspace::new();
            let mut steps = steps(&ws, "load_brief");
            let e = run_worker(&mut steps).unwrap_err();
            assert_eq!(steps.calls, ["leave_startup_error"]);
            assert_eq!(steps.left_for, Some(None), "no brief, no record key");
            assert_eq!(steps.left_error, Some(format!("{e:#}")));
            assert!(steps.left_error.unwrap().starts_with("no brief at "));
        }

        struct Flow {
            calls: Vec<String>,
            launch: Result<Option<i32>, &'static str>,
            running_fails: bool,
        }

        impl WorkerSteps for Flow {
            fn load_brief(&mut self) -> Result<Brief> {
                Ok(Steps::brief())
            }
            fn enter_context(&mut self, _: &Brief) -> Result<()> {
                Ok(())
            }
            fn register(&mut self, _: &Brief) -> Result<()> {
                Ok(())
            }
            fn set_running(&mut self, _: &Brief) -> Result<()> {
                self.calls.push("set_running".into());
                if self.running_fails {
                    bail!("ledger is read-only");
                }
                Ok(())
            }
            fn launch(&mut self, _: &Brief) -> Result<Option<i32>> {
                self.calls.push("launch".into());
                self.launch.map_err(|e| anyhow::anyhow!(e))
            }
            fn agent_exited(&mut self, _: &Brief, code: Option<i32>) -> Result<()> {
                self.calls.push(format!("agent_exited({code:?})"));
                bail!("exit not recorded")
            }
        }

        #[test]
        fn a_failed_ledger_write_does_not_stop_the_worker() {
            let mut flow = Flow {
                calls: Vec::new(),
                launch: Ok(Some(3)),
                running_fails: true,
            };
            assert_eq!(run_worker(&mut flow).unwrap(), 3);
            assert_eq!(
                flow.calls,
                ["set_running", "launch", "agent_exited(Some(3))"]
            );
        }

        #[test]
        fn a_signal_exit_is_code_1() {
            let mut flow = Flow {
                calls: Vec::new(),
                launch: Ok(None),
                running_fails: false,
            };
            assert_eq!(run_worker(&mut flow).unwrap(), 1);
            assert_eq!(flow.calls.last().unwrap(), "agent_exited(None)");
        }

        #[test]
        fn a_failed_launch_records_the_exit_and_returns_its_error() {
            let mut flow = Flow {
                calls: Vec::new(),
                launch: Err("no such harness"),
                running_fails: false,
            };
            let e = run_worker(&mut flow).unwrap_err();
            assert_eq!(e.to_string(), "no such harness");
            assert_eq!(flow.calls, ["set_running", "launch", "agent_exited(None)"]);
        }

        const KEY: StartupKey<'static> = StartupKey::Record("e1");

        #[test]
        fn a_startup_error_is_read_once_then_deleted() {
            let tmp = tempfile::tempdir().unwrap();
            let since = std::time::SystemTime::now() - Duration::from_secs(1);
            write_startup_error(tmp.path(), KEY, "no brief\n").unwrap();
            let path = startup_error_path(tmp.path(), KEY).unwrap();
            assert!(path.is_file());
            assert_eq!(
                take_startup_error(tmp.path(), KEY, since).as_deref(),
                Some("no brief")
            );
            assert!(!path.exists(), "the file is deleted after it is read");
            assert_eq!(take_startup_error(tmp.path(), KEY, since), None);
        }

        #[test]
        fn a_stale_startup_error_is_deleted_and_ignored() {
            let tmp = tempfile::tempdir().unwrap();
            write_startup_error(tmp.path(), KEY, "old").unwrap();
            let later = std::time::SystemTime::now() + Duration::from_secs(60);
            assert_eq!(take_startup_error(tmp.path(), KEY, later), None);
            assert!(!startup_error_path(tmp.path(), KEY).unwrap().exists());
        }

        fn write_startup_error_at(dir: &Path, at: SystemTime) {
            write_startup_error(dir, KEY, "boom").unwrap();
            let path = startup_error_path(dir, KEY).unwrap();
            let file = std::fs::File::options().write(true).open(path).unwrap();
            file.set_modified(at).unwrap();
        }

        #[test]
        fn a_startup_error_in_the_spawns_second_counts() {
            let tmp = tempfile::tempdir().unwrap();
            let second = SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000);
            write_startup_error_at(tmp.path(), second);
            let since = second + Duration::from_millis(900);
            assert_eq!(
                take_startup_error(tmp.path(), KEY, since).as_deref(),
                Some("boom")
            );
        }

        #[test]
        fn a_startup_error_from_the_second_before_is_stale() {
            let tmp = tempfile::tempdir().unwrap();
            let second = SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000);
            write_startup_error_at(tmp.path(), second - Duration::from_millis(1));
            let since = second + Duration::from_millis(900);
            assert_eq!(take_startup_error(tmp.path(), KEY, since), None);
            assert!(!startup_error_path(tmp.path(), KEY).unwrap().exists());
        }

        #[test]
        fn a_key_that_is_a_path_writes_no_startup_error() {
            let tmp = tempfile::tempdir().unwrap();
            for key in [
                StartupKey::Record("../x"),
                StartupKey::Record(""),
                StartupKey::Pane("w1:../x"),
                StartupKey::Pane(""),
            ] {
                assert!(startup_error_path(tmp.path(), key).is_none(), "{key:?}");
                assert!(write_startup_error(tmp.path(), key, "e").is_err());
            }
            assert!(!tmp.path().join("x.txt").exists());
        }

        #[test]
        fn a_startup_key_names_its_file_by_its_kind() {
            for (key, want) in [
                (StartupKey::Record("e1"), Some("record-e1.txt")),
                (StartupKey::Record("Rec_a-9"), Some("record-Rec_a-9.txt")),
                (StartupKey::Pane("w1:p2"), Some("pane-w1_p2.txt")),
                (StartupKey::Pane("w-1:p_2"), Some("pane-w-1_p_2.txt")),
                (StartupKey::Pane("a:b:c"), Some("pane-a_b_c.txt")),
                (StartupKey::Pane(":"), Some("pane-_.txt")),
                (StartupKey::Record(""), None),
                (StartupKey::Record("w1:p2"), None),
                (StartupKey::Record("a.b"), None),
                (StartupKey::Record("a b"), None),
                (StartupKey::Record("é"), None),
                (StartupKey::Record("a/b"), None),
                (StartupKey::Record("a\\b"), None),
                (StartupKey::Pane(""), None),
                (StartupKey::Pane("w1:p2.json"), None),
                (StartupKey::Pane("w1:p 2"), None),
                (StartupKey::Pane("w1:é"), None),
                (StartupKey::Pane("w1/2:p"), None),
            ] {
                assert_eq!(key.file_name().as_deref(), want, "{key:?}");
            }
        }

        #[test]
        fn an_error_reaches_only_the_wait_of_its_record() {
            let tmp = tempfile::tempdir().unwrap();
            let since = std::time::SystemTime::now() - Duration::from_secs(1);
            let poll = Duration::from_millis(5);
            let short = Duration::from_millis(30);
            let a = [StartupKey::Record("rec-a"), StartupKey::Pane("wA:p2")];
            let b = [StartupKey::Record("rec-b"), StartupKey::Pane("wB:p2")];
            write_startup_error(tmp.path(), StartupKey::Record("rec-b"), "B failed").unwrap();
            assert_eq!(
                await_worker(tmp.path(), &a, since, short, poll, &mut || false),
                WorkerStart::Waiting,
                "A does not take B's error"
            );
            assert_eq!(
                await_worker(tmp.path(), &b, since, short, poll, &mut || false),
                WorkerStart::Failed("B failed".into())
            );

            let no_brief = StartupKey::Pane("wA:p2");
            write_startup_error(tmp.path(), no_brief, "A has no brief").unwrap();
            assert_eq!(
                await_worker(tmp.path(), &b, since, short, poll, &mut || false),
                WorkerStart::Waiting
            );
            assert_eq!(
                await_worker(tmp.path(), &a, since, short, poll, &mut || false),
                WorkerStart::Failed("A has no brief".into())
            );
        }

        #[test]
        fn await_worker_returns_the_first_of_error_registration_and_timeout() {
            let tmp = tempfile::tempdir().unwrap();
            let since = std::time::SystemTime::now() - Duration::from_secs(1);
            let poll = Duration::from_millis(5);
            let long = Duration::from_secs(30);
            assert_eq!(
                await_worker(tmp.path(), &[KEY], since, long, poll, &mut || true),
                WorkerStart::Registered
            );
            assert_eq!(
                await_worker(
                    tmp.path(),
                    &[KEY],
                    since,
                    Duration::from_millis(30),
                    poll,
                    &mut || false
                ),
                WorkerStart::Waiting
            );
            let mut polls = 0;
            let start = await_worker(tmp.path(), &[KEY], since, long, poll, &mut || {
                polls += 1;
                if polls == 3 {
                    write_startup_error(tmp.path(), KEY, "boom").unwrap();
                }
                false
            });
            assert_eq!(start, WorkerStart::Failed("boom".into()));
            assert!(!startup_error_path(tmp.path(), KEY).unwrap().exists());
        }

        #[test]
        fn records_the_startup_error_text_before_the_pane_closes() {
            let ws = FakeWorkspace::new();
            let mut steps = steps(&ws, "enter_context");
            let e = run_worker(&mut steps).unwrap_err();
            assert_eq!(
                steps.startup_error.as_deref(),
                Some(format!("{e:#}").as_str())
            );
            assert_eq!(steps.startup_error.as_deref(), Some("enter_context failed"));
        }

        #[test]
        fn horch_sessions_shows_the_startup_error() {
            let tmp = tempfile::tempdir().unwrap();
            let _fleetd = crate::execution::store::test_serve(
                &tmp.path().join(crate::execution::store::STATE_DB),
            );
            let ledger = crate::execution::records::SessionLedger::for_project(tmp.path(), "/p");
            ledger
                .add(crate::execution::records::NewRecord {
                    record_id: "r1",
                    agent: "claude",
                    tier: "sonnet",
                    model: "sonnet",
                    role: "sonnet-1",
                    session_id: None,
                    task: "t",
                    phase: None,
                })
                .unwrap();
            let store = ExecutionStore::for_project(tmp.path(), "/p");
            record_startup_failure(&store, "r1", "project dir '/p' is missing").unwrap();
            let r = store.get("r1").unwrap();
            let h = r.history.last().unwrap();
            assert_eq!(h.event, EVENT_STARTUP_FAILED);
            assert_eq!(h.text, "project dir '/p' is missing");
            let out = ledger.render().unwrap();
            assert!(
                out.contains("  startup-failed @ ")
                    && out.contains(": project dir '/p' is missing\n"),
                "{out}"
            );
        }

        #[test]
        fn a_close_failure_does_not_hide_the_startup_error() {
            let ws = FakeWorkspace::new();
            let mut steps = steps(&ws, "enter_context");
            ws.fail_next("pane_close", "herdr is down");
            let e = run_worker(&mut steps).unwrap_err();
            assert_eq!(e.to_string(), "enter_context failed");
            assert_eq!(steps.calls.last(), Some(&"close_pane"));
            assert_eq!(
                ws.pane_ids().len(),
                1,
                "the pane stays when the close fails"
            );
        }

        #[test]
        fn pane_worker_records_a_startup_failure_in_the_briefs_ledger() {
            let tmp = tempfile::tempdir().unwrap();
            let state = tmp.path().join("state");
            std::fs::create_dir_all(&state).unwrap();
            let _fleetd =
                crate::execution::store::test_serve(&state.join(crate::execution::store::STATE_DB));
            let project = tmp.path().join("missing");
            let project_text = project.to_string_lossy().into_owned();
            let ledger =
                crate::execution::records::SessionLedger::for_project(&state, &project_text);
            ledger
                .add(crate::execution::records::NewRecord {
                    record_id: "r1",
                    agent: "claude",
                    tier: "sonnet",
                    model: "sonnet",
                    role: "sonnet-1",
                    session_id: None,
                    task: "t",
                    phase: None,
                })
                .unwrap();
            let home = tmp.path().join("home");
            let mut ctx = RuntimeContext::from_env(&MapEnv::new(&home)).unwrap();
            let enter_dir = |_: &Path| -> Result<()> { Ok(()) };
            let mut worker = PaneWorker::new(&mut ctx, "sonnet-1", &enter_dir);
            let mut brief = Steps::brief();
            brief.project_dir = project_text.clone();
            brief.state_dir = Some(state.to_string_lossy().into_owned());

            let since = SystemTime::now();
            let e = worker.enter_context(&brief).unwrap_err();
            let error = format!("{e:#}");
            worker.startup_failed(&brief, &error).unwrap();
            worker.leave_startup_error(&error, Some(&brief)).unwrap();
            worker.agent_exited(&brief, None).unwrap();

            let data_root = ctx.paths.data_root.clone();
            assert_eq!(
                take_startup_error(&data_root, StartupKey::Record("r1"), since),
                Some(format!("project dir '{project_text}' is missing"))
            );
            let r = ExecutionStore::for_project(&state, &project_text)
                .get("r1")
                .unwrap();
            assert!(
                r.history.iter().all(|h| h.event != EVENT_STARTUP_FAILED),
                "the wrapper writes no startup failure row: {:?}",
                r.history
            );
            assert!(
                r.execution_status().is_live(),
                "the wrapper writes no exit: {:?}",
                r.execution_status()
            );
        }

        struct LedgerWorld {
            tmp: tempfile::TempDir,
            db: PathBuf,
            worker: String,
            task: String,
            state: PathBuf,
            brief: Brief,
        }

        impl LedgerWorld {
            fn new(attempt: bool) -> LedgerWorld {
                use crate::taskledger::model::Actor;
                use crate::taskledger::runs::{self, NewRun};
                use crate::taskledger::tasks::{self, NewTask};
                use crate::taskledger::workers::{self, NewWorker};
                let tmp = tempfile::tempdir().unwrap();
                let top = horch_platform::path::for_child(&tmp.path().canonicalize().unwrap());
                let project = top.join("repo");
                std::fs::create_dir_all(&project).unwrap();
                let project_text = project.to_string_lossy().into_owned();
                let state = top.join("state");
                let db = top.join("ledger.sqlite3");
                let mut store = crate::taskledger::Store::open(&db).unwrap();
                let (worker, task) = store
                    .immediate(|tx| {
                        let run = runs::create(
                            tx,
                            &project,
                            &NewRun {
                                slug: "s".into(),
                                ..Default::default()
                            },
                        )?;
                        crate::taskledger::schema::run_idea(
                            tx,
                            &run.run_id,
                            &crate::taskledger::util::now(),
                            true,
                        )?;
                        let task = tasks::add(
                            tx,
                            &run,
                            &NewTask::work("a", "tickets/a.md", "opus"),
                            Actor::Orchestrator,
                        )?;
                        let worker = workers::insert(
                            tx,
                            &NewWorker {
                                run_id: run.run_id.clone(),
                                persona: "opus".into(),
                                role: Some("sonnet-1".into()),
                                ..Default::default()
                            },
                        )?;
                        if attempt {
                            tx.execute(
                                "INSERT INTO attempts (id, task_id, worker_id, state)
                                 VALUES (?1, ?2, ?3, 'live')",
                                [&format!("{}/A1", task.task_id), &task.task_id, &worker],
                            )?;
                        }
                        Ok((worker, task.task_id))
                    })
                    .unwrap();
                std::fs::create_dir_all(&state).unwrap();
                let fleetd = crate::execution::store::test_serve(&db);
                let ctx = RuntimeContext::from_env(
                    &MapEnv::new(&project)
                        .with("HOME", &top.join("home").to_string_lossy())
                        .with("HORCH_STATE_DIR", &state.to_string_lossy())
                        .with("HORCH_LEDGER_DB", &db.to_string_lossy()),
                )
                .unwrap();
                crate::execution::records::SessionLedger::open_in(&ctx)
                    .unwrap()
                    .add(crate::execution::records::NewRecord {
                        record_id: "r1",
                        agent: "claude",
                        tier: "sonnet",
                        model: "sonnet",
                        role: "sonnet-1",
                        session_id: None,
                        task: "t",
                        phase: None,
                    })
                    .unwrap();
                drop(fleetd);
                let mut brief = Steps::brief();
                brief.project_dir = project_text;
                brief.state_dir = Some(state.to_string_lossy().into_owned());
                brief.ledger_db = Some(db.to_string_lossy().into_owned());
                brief.worker_id = Some(worker.clone());
                brief.fleet_root = Some(top.join("fleet").to_string_lossy().into_owned());
                LedgerWorld {
                    tmp,
                    db,
                    worker,
                    task,
                    state,
                    brief,
                }
            }

            fn worker_row(&self) -> serde_json::Value {
                let store = crate::taskledger::Store::open_read_only(&self.db).unwrap();
                store
                    .conn()
                    .query_row(
                        "SELECT legacy_record_id, supervisor_pid, agent_pid, last_checkin_at,
                                status, exit_code FROM workers WHERE worker_id = ?1",
                        [&self.worker],
                        |r| {
                            Ok(serde_json::json!([
                                r.get::<_, Option<String>>(0)?,
                                r.get::<_, Option<i64>>(1)?,
                                r.get::<_, Option<i64>>(2)?,
                                r.get::<_, Option<String>>(3)?,
                                r.get::<_, String>(4)?,
                                r.get::<_, Option<i64>>(5)?,
                            ]))
                        },
                    )
                    .unwrap()
            }

            fn record(&self) -> serde_json::Value {
                let r = ExecutionStore::in_db(&self.state, &self.db, &self.brief.project_dir)
                    .get("r1")
                    .unwrap();
                serde_json::to_value(r).unwrap()
            }

            fn start_and_exit(&self) {
                let mut ctx =
                    RuntimeContext::from_env(&MapEnv::new(self.tmp.path().join("home"))).unwrap();
                let enter_dir = |_: &Path| -> Result<()> { Ok(()) };
                let mut worker = PaneWorker::new(&mut ctx, "sonnet-1", &enter_dir);
                worker.enter_context(&self.brief).unwrap();
                worker.set_running(&self.brief).unwrap();
                worker.agent_exited(&self.brief, Some(1)).unwrap();
            }
        }

        #[test]
        fn the_wrapper_writes_no_ledger_row() {
            let w = LedgerWorld::new(true);
            let (row, record) = (w.worker_row(), w.record());
            w.start_and_exit();
            assert_eq!(w.worker_row(), row, "no worker column changes");
            assert_eq!(w.record(), record, "the session record does not change");
            let exit = Path::new(w.brief.fleet_root.as_deref().unwrap())
                .join("tasks")
                .join(&w.task)
                .join("attempts/A1")
                .join(crate::taskledger::supervisor::EXIT_JSON);
            assert!(exit.is_file(), "{}", exit.display());

            let w = LedgerWorld::new(false);
            let (row, record) = (w.worker_row(), w.record());
            w.start_and_exit();
            assert_eq!(w.worker_row(), row, "no worker column changes");
            assert_eq!(w.record(), record, "the session record does not change");
            assert!(!Path::new(w.brief.fleet_root.as_deref().unwrap())
                .join("tasks")
                .exists());
        }

        #[test]
        fn a_brief_ledger_moves_the_fleet_root_beside_it() {
            let tmp = tempfile::tempdir().unwrap();
            let ledger = tmp.path().join("isolated").join("ledger.sqlite3");
            let mut ctx = RuntimeContext::from_env(&MapEnv::new(tmp.path().join("home"))).unwrap();
            let enter_dir = |_: &Path| -> Result<()> { Ok(()) };
            let mut worker = PaneWorker::new(&mut ctx, "sonnet-1", &enter_dir);
            let mut brief = Steps::brief();
            brief.project_dir = tmp.path().to_string_lossy().into_owned();
            brief.ledger_db = Some(ledger.to_string_lossy().into_owned());
            worker.enter_context(&brief).unwrap();
            assert_eq!(
                ctx.paths.fleet_root,
                tmp.path().join("isolated").join("fleet")
            );
        }

        #[test]
        fn a_brief_fleet_root_wins_over_the_folder_beside_the_ledger() {
            let tmp = tempfile::tempdir().unwrap();
            let ledger = tmp.path().join("isolated").join("ledger.sqlite3");
            let root = tmp.path().join("elsewhere").join("my-fleet");
            let spawner = RuntimeContext::from_env(
                &MapEnv::new(tmp.path().join("home"))
                    .with("HORCH_LEDGER_DB", &ledger.to_string_lossy())
                    .with("FLEET_ROOT", &root.to_string_lossy()),
            )
            .unwrap();
            let mut ctx = RuntimeContext::from_env(&MapEnv::new(tmp.path().join("home"))).unwrap();
            let enter_dir = |_: &Path| -> Result<()> { Ok(()) };
            let mut worker = PaneWorker::new(&mut ctx, "sonnet-1", &enter_dir);
            let mut brief = Steps::brief();
            brief.project_dir = tmp.path().to_string_lossy().into_owned();
            brief.ledger_db = Some(ledger.to_string_lossy().into_owned());
            brief.fleet_root = Some(spawner.paths.fleet_root.to_string_lossy().into_owned());
            worker.enter_context(&brief).unwrap();
            assert_eq!(ctx.paths.fleet_root, root);
        }

        #[test]
        fn a_relative_brief_fleet_root_is_refused() {
            let tmp = tempfile::tempdir().unwrap();
            let mut ctx = RuntimeContext::from_env(&MapEnv::new(tmp.path().join("home"))).unwrap();
            let enter_dir = |_: &Path| -> Result<()> { Ok(()) };
            let mut worker = PaneWorker::new(&mut ctx, "sonnet-1", &enter_dir);
            let mut brief = Steps::brief();
            brief.project_dir = tmp.path().to_string_lossy().into_owned();
            brief.fleet_root = Some("fleet".into());
            let e = worker.enter_context(&brief).unwrap_err();
            assert_eq!(
                e.to_string(),
                "the brief fleet root 'fleet' is not absolute"
            );
        }

        #[test]
        fn a_worker_that_starts_keeps_its_pane() {
            let ws = FakeWorkspace::new();
            let mut steps = steps(&ws, "none");
            assert_eq!(run_worker(&mut steps).unwrap(), 0);
            assert!(!steps.calls.contains(&"close_pane"));
            assert_eq!(ws.pane_ids().len(), 1);
        }

        #[test]
        fn trf_39_a_worker_cannot_register_the_orchestrator_role() {
            let tmp = tempfile::tempdir().unwrap();
            let mut ctx = RuntimeContext::from_env(
                &MapEnv::new(tmp.path())
                    .with(
                        "HORCH_STATE_DIR",
                        tmp.path().join("state").to_str().unwrap(),
                    )
                    .with("HORCH_HERDR_BIN", "/nonexistent/herdr"),
            )
            .unwrap();
            let enter_dir = |_: &Path| -> Result<()> { Ok(()) };
            let mut worker = PaneWorker::new(&mut ctx, "orchestrator", &enter_dir);
            let e = format!("{:#}", worker.register(&Steps::brief()).unwrap_err());
            assert_eq!(
                e,
                "a worker cannot register the role orchestrator: only horch fleet takes it"
            );
            assert!(!tmp.path().join("state").exists(), "no mailbox is written");
        }
    }

    #[cfg(unix)]
    mod starts {
        use super::*;
        use crate::harness::launch::FORBIDDEN_ENV;
        use std::os::unix::fs::PermissionsExt;

        const ENV_OUT: &str = "HORCH_TEST_START_ENV";

        #[test]
        fn the_horch_of_the_wrapper_starts_without_a_forbidden_name() {
            let dir = tempfile::tempdir().unwrap();
            let out = dir.path().join("env.txt");
            let mut parent = std::process::Command::new(std::env::current_exe().unwrap());
            parent
                .args([
                    "--exact",
                    "execution::lifecycle::tests::starts::the_horch_of_the_wrapper_child",
                    "--ignored",
                    "--test-threads=1",
                ])
                .env(ENV_OUT, &out);
            for key in FORBIDDEN_ENV {
                parent.env(key, "from-the-parent");
            }
            let run = parent.output().unwrap();
            let stdout = String::from_utf8_lossy(&run.stdout);
            assert!(
                run.status.success() && stdout.contains("1 passed"),
                "{stdout}\n{}",
                String::from_utf8_lossy(&run.stderr)
            );
            let seen = std::fs::read_to_string(&out).unwrap();
            assert!(
                seen.lines().any(|l| l.starts_with(&format!("{ENV_OUT}="))),
                "the rest of the environment is missing:\n{seen}"
            );
            for key in FORBIDDEN_ENV {
                assert!(
                    !seen.lines().any(|l| l.starts_with(&format!("{key}="))),
                    "{key} reached the child"
                );
            }
        }

        #[test]
        #[ignore = "run by the_horch_of_the_wrapper_starts_without_a_forbidden_name"]
        fn the_horch_of_the_wrapper_child() {
            let bin = tempfile::tempdir().unwrap();
            let horch = bin.path().join("horch");
            std::fs::write(&horch, format!("#!/bin/sh\nenv > \"${ENV_OUT}\"\n")).unwrap();
            std::fs::set_permissions(&horch, std::fs::Permissions::from_mode(0o755)).unwrap();
            let path = format!("{}:/usr/bin:/bin", bin.path().display());
            run_horch_quiet(&[("PATH".into(), path)], &["note", "a start"]).unwrap();
        }
    }
}
