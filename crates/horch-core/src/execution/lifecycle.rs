use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant, SystemTime};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::execution::legacy::HistoryEntry;
use crate::execution::store::ExecutionStore;
use crate::harness::launch::{self, DiscoveryTarget, LaunchRequest};
use crate::harness::HarnessKind;
use crate::messaging::brief::Brief;
use crate::messaging::mailbox::Mailbox;
use crate::messaging::message::strip_done_prefix;
use crate::messaging::report;
use crate::roster::{HorchAccess, Roster, Teammate};
use crate::runtime::{process, RuntimeContext};
use crate::taskledger::supervisor::Checkins;
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
    fn close_failed(&self, _pane: &str, _error: &anyhow::Error) {}
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

    close_pane(ws, pane, unreachable).inspect_err(|e| steps.close_failed(req.pane, e))
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

pub struct PaneWorker<'a> {
    pub ctx: &'a mut RuntimeContext,
    pub role: &'a str,
    pub enter_dir: &'a dyn Fn(&Path) -> Result<()>,
    mailbox: Option<Mailbox>,
    path: Option<std::ffi::OsString>,
    launched: bool,
    ledger: Option<(PathBuf, String)>,
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
            launched: false,
            ledger: None,
        }
    }

    fn ledger_exit(&self, brief: &Brief, code: Option<i32>) -> Option<String> {
        use crate::taskledger::supervisor::Ended;
        let (db, id) = self.ledger.as_ref()?;
        match crate::taskledger::supervisor::agent_exited(db, id, code) {
            Ok(Ended::Requeued { task, persona }) => Some(format!(
                " The task ledger queued {task} again. Spawn {persona} with: \
                 horch spawn {persona} --replaces {id}"
            )),
            Ok(Ended::Held { task }) => Some(format!(
                " QUESTION: {task} used up its attempts and is held. Ask the operator: \
                 requeue it (horch ledger requeue {task} \"<reason>\"), change the ticket, \
                 or cancel it. Spawn no replacement."
            )),
            Ok(_) => None,
            Err(e) => {
                eprintln!(
                    "horch worker[{}]: the task ledger did not record the exit: {e:#}",
                    brief.role
                );
                None
            }
        }
    }

    fn tell_early_exit(
        &self,
        brief: &Brief,
        code: Option<i32>,
        requeued: Option<String>,
    ) -> Result<()> {
        let env = child_env(self.mailbox()?, brief, self.path.clone());
        let mut line = early_exit_line(brief, code);
        line.extend(requeued);
        warn_worker(
            &brief.role,
            "could not tell the orchestrator about the exit",
            run_horch(&env, &["tell", "orchestrator", &line]),
        );
        Ok(())
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

    fn start_checkins(&self, brief: &Brief) -> Option<Checkins> {
        self.ledger.clone().map(|(db, id)| {
            let (obs_db, obs_id, role) = (db.clone(), id.clone(), brief.role.clone());
            launch::observe_agent_start(move |pid| {
                warn_worker(
                    &role,
                    "the task ledger did not record the agent PID",
                    crate::taskledger::supervisor::agent_started(&obs_db, &obs_id, pid),
                );
            });
            let every = crate::taskledger::supervisor::interval(&db, &id);
            let lookup = session_lookup(
                self.ctx.clone(),
                brief.record_id.clone(),
                brief.agent.clone(),
            );
            crate::taskledger::supervisor::start_checkins(db, id, every, lookup)
        })
    }

    fn run_smoke_agent(
        &self,
        env: &[(String, String)],
        checkins: Option<Checkins>,
    ) -> Result<Option<i32>> {
        if let Some((db, id)) = &self.ledger {
            let _ = crate::taskledger::supervisor::agent_started(db, id, std::process::id());
        }
        let out = run_smoke(env).map(|()| Some(0));
        drop(checkins);
        out
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

    fn set_running(&mut self, brief: &Brief) -> Result<()> {
        if let Some((db, id)) = &self.ledger {
            crate::taskledger::Store::open(db).and_then(|store| {
                crate::taskledger::workers::set_record(store.conn(), id, &brief.record_id)
            })?;
            if let Err(e) = crate::taskledger::supervisor::started(db, id, &brief.role) {
                eprintln!(
                    "horch worker[{}]: the task ledger did not record the start: {e:#}",
                    brief.role
                );
            }
        }
        self.store()?.mark_running(&brief.record_id)
    }

    fn launch(&mut self, brief: &Brief) -> Result<Option<i32>> {
        let ctx: &RuntimeContext = self.ctx;
        let mailbox = self.mailbox()?;
        let (roster, teammate) = self.load_roster(brief)?;
        let prompt = self.worker_prompt(brief, &roster, &teammate)?;
        if let Some(workdir) = &brief.workdir {
            (self.enter_dir)(Path::new(workdir))
                .with_context(|| format!("entering workdir {workdir}"))?;
        }
        let env = child_env(mailbox, brief, self.path.clone());
        let _report = start_report_server(ctx, brief, &teammate, &env)?;
        let checkins = self.start_checkins(brief);
        if teammate.agent == HarnessKind::None {
            self.run_smoke_agent(&env, checkins)
        } else {
            let plan = AgentPlan {
                roster: &roster,
                teammate: &teammate,
                prompt: &prompt,
            };
            let code = run_agent(ctx, brief, mailbox, &plan, env, checkins)?;
            self.launched = true;
            Ok(code)
        }
    }

    fn agent_exited(&mut self, brief: &Brief, code: Option<i32>) -> Result<()> {
        let requeued = self.ledger_exit(brief, code);
        let store = self.store()?;
        let ended = store.record_exit(&brief.record_id, code)?;
        if ended && self.launched {
            record_early_exit(&store, &brief.record_id, code)?;
            if brief.report_to == ReportTarget::Orchestrator {
                self.tell_early_exit(brief, code, requeued)?;
            }
        }
        Ok(())
    }

    fn startup_failed(&mut self, brief: &Brief, error: &str) -> Result<()> {
        record_startup_failure(&self.store()?, &brief.record_id, error)
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
        let out = herdr.pane_close(&public);
        if let Some((db, id)) = &self.ledger {
            let _ = crate::taskledger::supervisor::closed_own_pane(db, id, &public, &out);
        }
        out
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

#[cfg(unix)]
fn start_report_server(
    ctx: &RuntimeContext,
    brief: &Brief,
    teammate: &Teammate,
    env: &[(String, String)],
) -> Result<Option<report::Server>> {
    if teammate.horch_access != HorchAccess::Report {
        return Ok(None);
    }
    let path = report::socket_path(&ctx.paths.state_root, &brief.record_id);
    let env = env.to_vec();
    let exec: Box<report::Executor> =
        Box::new(move |request: &report::Request| run_horch_quiet(&env, &report_args(request)));
    report::Server::start(path, report_target(brief), exec).map(Some)
}

#[cfg(not(unix))]
fn start_report_server(
    _ctx: &RuntimeContext,
    _brief: &Brief,
    teammate: &Teammate,
    _env: &[(String, String)],
) -> Result<Option<()>> {
    if teammate.horch_access == HorchAccess::Report {
        bail!("horch_access: report needs a Unix host");
    }
    Ok(None)
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

fn early_exit_line(brief: &Brief, code: Option<i32>) -> String {
    let how = match code {
        Some(code) => format!("exited with code {code}"),
        None => "was ended by a signal".to_string(),
    };
    format!(
        "[{}] BLOCKED: My agent {how} before horch done. The work can be incomplete. \
         Resume it with: horch spawn --resume {}",
        brief.role, brief.record_id
    )
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
    let mut cmd = Command::new("horch");
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
        fn close_failed(&self, _: &str, _: &anyhow::Error) {
            self.order.borrow_mut().push("close_failed");
        }
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
            controller: None,
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
        let err = report::send(&path, &report::Request::Progress { text: "x".into() }).unwrap_err();
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
            controller: None,
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
        failed: std::cell::RefCell<Vec<String>>,
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
        fn close_failed(&self, pane: &str, _: &anyhow::Error) {
            self.failed.borrow_mut().push(pane.to_string());
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
                        failed: Default::default(),
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
                    let failed = steps.failed.into_inner();
                    assert_eq!(failed.is_empty(), got.is_ok(), "{case}: {failed:?}");
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
                    controller: None,
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

            let e = worker.enter_context(&brief).unwrap_err();
            worker.startup_failed(&brief, &format!("{e:#}")).unwrap();
            worker.agent_exited(&brief, None).unwrap();

            let r = ExecutionStore::for_project(&state, &project_text)
                .get("r1")
                .unwrap();
            let h = r.history.last().unwrap();
            assert_eq!(h.event, EVENT_STARTUP_FAILED);
            assert_eq!(h.text, format!("project dir '{project_text}' is missing"));
            assert!(
                !r.execution_status().is_live(),
                "{:?}",
                r.execution_status()
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
}
