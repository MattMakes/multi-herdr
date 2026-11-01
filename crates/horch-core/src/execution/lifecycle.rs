//! A worker's life: [`run_worker`] starts its agent and records how it ended;
//! [`done`] is the worker's own shutdown.

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
use crate::roster::{Roster, Teammate};
use crate::runtime::{process, RuntimeContext};
use crate::taskledger::supervisor::Checkins;
use crate::workspace::client::WorkspaceClient;
use crate::workspace::herdr::Herdr;
use crate::workspace::model::Pane;

/// Who hears about an execution when it ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReportTarget {
    Orchestrator,
    None,
}

/// What `horch done` knows about the worker that runs it.
#[derive(Debug, Clone)]
pub struct DoneRequest<'a> {
    pub record_id: &'a str,
    pub role: &'a str,
    /// This pane's `HERDR_PANE_ID`, which `pane get` upgrades to the public id.
    pub pane: &'a str,
    /// `HORCH_WORKSPACE_ID` when it is set; else the pane's own workspace.
    pub workspace: Option<&'a str>,
    pub summary: &'a str,
    pub report_to: ReportTarget,
    pub unlanded: Option<&'a str>,
}

/// The live facts needed to protect and report one open worktree claim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimLine {
    pub branch: String,
    pub short_sha: String,
    pub ahead: usize,
    pub base: String,
    pub path: PathBuf,
}

/// The result of [`DoneSteps::push_report`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReportStatus {
    Delivered,
    Queued(String),
}

/// The steps of [`done`] that are not workspace calls. The CLI runs them
/// against the ledger, `horch tell`, the mailbox and the tiler.
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
    /// Make the report durable, or fail as nothing changed. A push that can
    /// take long belongs in [`DoneSteps::push_report`], after the record is
    /// done.
    fn queue_report(&self, line: &str) -> Result<()>;
    /// Push what [`DoneSteps::queue_report`] queued, after the record is
    /// done, so a push killed by the agent's command timeout leaves the work
    /// done. Best effort; the default has nothing to push.
    fn push_report(&self) -> ReportStatus {
        ReportStatus::Delivered
    }
    /// Run after the record is done and before the report push: `horch
    /// ledger done` pushes its wake lines here, inside the same push budget,
    /// so a kill during them leaves the work done. The default does nothing.
    fn after_mark_done(&self) {}
    /// Whether an earlier `done` of this record reported and recorded its
    /// summary, so a retry only closes the pane. The default says no.
    fn recorded(&self, _record_id: &str) -> Result<bool> {
        Ok(false)
    }
    /// Note in the record's history that the report failed with `error`.
    /// Best effort; the default does nothing.
    fn report_failed(&self, _record_id: &str, _error: &str) {}
    /// Note that a durable report awaits redelivery. Best effort.
    fn report_queued(&self, _record_id: &str, _error: &str) {}
    fn unregister(&self, workspace: &str, role: &str);
    /// [`DoneSteps::unregister`], where a removal that fails is returned. The
    /// error names the role and the file that stayed; `done` warns about it
    /// and still closes the pane. The default cannot fail.
    fn try_unregister(&self, workspace: &str, role: &str) -> Result<()> {
        self.unregister(workspace, role);
        Ok(())
    }
    fn settle(&self, workspace: &str);
    /// One last session discovery, before the pane close ends the discovery
    /// thread. Best effort; the default does nothing.
    fn record_session(&self, _workspace: &str, _role: &str, _record_id: &str) -> Result<()> {
        Ok(())
    }
    /// Write the record's fleet run row (fleet-dataset §4.2). Best effort;
    /// the default does nothing.
    fn record_run(&self, _record_id: &str) -> Result<()> {
        Ok(())
    }
}

/// Worker-facing self-shutdown, run only when the work is truly complete (not
/// while waiting on a question).
///
/// The order is fixed: queue the report to the orchestrator, mark the claims
/// unlanded, mark done in the ledger, run [`DoneSteps::after_mark_done`],
/// push the report (best effort, within the push budget), record a session
/// id still undiscovered, unregister the mailbox entry, settle the grid,
/// write the fleet run row, close the pane. Everything happens BEFORE the
/// pane close, because closing the pane kills this very process tree. The
/// underlying agent session stays on disk and resumable.
///
/// A failed report stops `done` before any change: the record is not done,
/// the claims stay open, the role stays registered and the pane stays open.
/// The error tells the worker to retry; a worker whose report was lost must
/// not leave. A retry after a `done` that recorded its summary (and then
/// could not close the pane) only closes the pane. A pane that is gone at
/// any step after the summary is recorded counts as closed: `done` succeeds.
pub fn done(ws: &dyn WorkspaceClient, steps: &dyn DoneSteps, req: &DoneRequest) -> Result<()> {
    if steps.recorded(req.record_id)? {
        eprintln!("horch done: an earlier horch done recorded this summary; closing the pane");
    } else {
        report_and_record(steps, req)?;
    }
    close(ws, steps, req)
}

/// Print `what: error` when `result` failed. For the best-effort steps.
fn warn_err(result: Result<()>, what: impl std::fmt::Display) {
    if let Err(e) = result {
        eprintln!("{what}: {e:#}");
    }
}

/// [`warn_err`] for a step of the worker `role`.
fn warn_worker(role: &str, what: &str, result: Result<()>) {
    warn_err(result, format!("horch worker[{role}]: {what}"));
}

/// The steps of [`done`] up to the summary in the ledger.
fn report_and_record(steps: &dyn DoneSteps, req: &DoneRequest) -> Result<()> {
    let open = steps.open_claims(req.record_id)?;
    refuse_open_claims(&open, req.unlanded)?;
    let summary = done_summary(req, &open, unregistered_entries(steps));
    queue_done_report(steps, req, &summary)?;
    // After the report: a claim marked unlanded is not open, and a retry of
    // a failed report must find it open to report the same line.
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

/// The facts of one open claim, as a refusal and a summary show them.
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

/// Refuse `done` while a claim is open and no `--unlanded` reason is given.
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

/// The unregistered worktrees, or none when they cannot be inspected.
fn unregistered_entries(steps: &dyn DoneSteps) -> Vec<String> {
    steps.unregistered().unwrap_or_else(|error| {
        eprintln!("horch done: could not inspect unregistered worktrees ({error:#})");
        Vec::new()
    })
}

/// The summary `done` records and reports.
fn done_summary(req: &DoneRequest, open: &[ClaimLine], unregistered: Vec<String>) -> String {
    // `done` adds the tag and keyword itself; a summary that repeats them would
    // arrive as `[r] DONE: [r] DONE: ...`.
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

/// Queue the report of `done` for the orchestrator, when the record reports
/// to it. A failure stops `done` before any change.
fn queue_done_report(steps: &dyn DoneSteps, req: &DoneRequest, summary: &str) -> Result<()> {
    if req.report_to != ReportTarget::Orchestrator {
        return Ok(());
    }
    steps
        .queue_report(&format!("[{}] DONE: {summary}", req.role))
        .inspect_err(|e| steps.report_failed(req.record_id, &format!("{e:#}")))
        // A context, not a new error: the CLI's sandbox hint looks for
        // the herdr failure in the chain.
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

/// A pushed report remains queued until its explicit inbox acknowledgement.
fn note_queued(steps: &dyn DoneSteps, req: &DoneRequest, status: ReportStatus) {
    if let ReportStatus::Queued(cause) = status {
        eprintln!("horch done: queued, not yet acknowledged ({cause}); redelivered until horch inbox reads it");
        steps.report_queued(req.record_id, &cause);
    }
}

/// What `done` returns when herdr does not answer.
const HERDR_UNREACHABLE: &str =
    "herdr is unreachable: the summary is recorded, but this pane may still be open";

/// The steps of [`done`] after the summary is in the ledger: the pane closes.
fn close(ws: &dyn WorkspaceClient, steps: &dyn DoneSteps, req: &DoneRequest) -> Result<()> {
    // The record holds the summary now. The dataset coordinator closes the
    // pane of a candidate whose record is done, so from here on it can close
    // this pane first, at any step. A pane that is gone is closed: the steps
    // that need only the workspace still run, and nothing fails. When herdr
    // itself does not answer, the pane may still be open: the same steps
    // run, and `done` fails at the end so the caller knows.
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

        // A departing worker leaves a hole and hands its width to whichever
        // neighbour happens to be its split sibling, which lumps the grid
        // rather than spreading it. Hand the tidy to a detached child: the
        // close below kills this process tree, so by the time the hole
        // exists, this process is gone. The child pulls the last worker into
        // the free slot, and an overflow tab that lost its last worker closes
        // itself.
        steps.settle(workspace);
    }

    // After the session discovery, so a Codex row finds its transcript.
    warn_err(
        steps.record_run(req.record_id),
        "horch: NOTE: fleet run row not written",
    );

    close_pane(ws, pane, unreachable)
}

/// This pane, or why it is not known: `None` with no error when the pane is
/// gone, `None` with the error when herdr does not answer.
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

/// Close the pane. A pane that is gone counts as closed; a herdr that does
/// not answer is an error.
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

/// The outcome of a pane close that failed with `e`: an error only while the
/// pane is still there or herdr does not answer.
fn closed_or(ws: &dyn WorkspaceClient, pane_id: &str, e: anyhow::Error) -> Result<()> {
    if ws.pane_get(pane_id).is_ok() {
        Err(e)
    } else if !ws.server_reachable() {
        Err(e.context(HERDR_UNREACHABLE))
    } else {
        Ok(())
    }
}

/// The steps of [`run_worker`]. [`PaneWorker`] runs them for real; a test
/// records their order.
pub trait WorkerSteps {
    /// Find this pane's mailbox and read the brief `horch spawn` wrote.
    fn load_brief(&mut self) -> Result<Brief>;
    /// Take the brief's project, ledger and binary overrides as this
    /// process's context.
    fn enter_context(&mut self, brief: &Brief) -> Result<()>;
    /// Register the role in the mailbox, so `horch tell` reaches this pane.
    fn register(&mut self, brief: &Brief) -> Result<()>;
    /// The record is `Running`.
    fn set_running(&mut self, brief: &Brief) -> Result<()>;
    /// Run the agent through the harness flow and wait for it. `None`: a
    /// signal ended it.
    fn launch(&mut self, brief: &Brief) -> Result<Option<i32>>;
    /// Record how the agent ended ([`ExecutionStore::record_exit`]).
    fn agent_exited(&mut self, brief: &Brief, code: Option<i32>) -> Result<()>;
    /// Record the startup error text in the record's history
    /// ([`record_startup_failure`]). The pane closes next and takes its
    /// stderr with it, so the ledger keeps the only copy. The default
    /// records nothing.
    fn startup_failed(&mut self, _brief: &Brief, _error: &str) -> Result<()> {
        Ok(())
    }
    /// Close this worker's pane after a startup failure, so a dead worker
    /// does not hold a pane. The default leaves the pane open.
    fn close_pane(&mut self) -> Result<()> {
        Ok(())
    }
    /// Leave the startup error text for `horch spawn`
    /// ([`write_startup_error`]), keyed by the brief's record, or `None`
    /// when the brief did not load. The pane closes when this process ends
    /// and takes its stderr with it. The default leaves nothing.
    fn leave_startup_error(&mut self, _error: &str, _brief: Option<&Brief>) -> Result<()> {
        Ok(())
    }
}

/// Run a worker: load the brief, build the context, register the mailbox,
/// set `Running`, launch the agent through the harness flow, wait, and
/// record its exit. Returns the agent's exit code (1 when a signal ended
/// it), which `horch worker` exits with.
///
/// The order is Spec A §8 (architecture design §4.5). The worker never
/// calls `done`: the agent runs `horch done`, which closes this pane.
///
/// A ledger write that fails is logged and the worker goes on: the agent
/// must start even when its bookkeeping cannot be written. That includes
/// `set_running`: the agent still starts, and its exit ends the record.
///
/// A failure after the brief loads and before the agent launches
/// (`enter_context`, `register`) ends the record at once as
/// `Failed(AgentExited{None})`, the same state a launch failure gets, then
/// returns the original error. The record's history gets the error text first
/// (event [`EVENT_STARTUP_FAILED`]), which `horch sessions` shows. Without it the record stays `Starting` with a
/// dead worker until a deadline. The recording is best effort: its own error
/// is logged. Then the worker closes its own pane, as `done` does; a close
/// that fails is logged and the startup error still returns. A failure of
/// `load_brief` records nothing in the ledger: the record is unknown.
///
/// Every startup failure also leaves its error text for `horch spawn`
/// ([`WorkerSteps::leave_startup_error`]) before the pane closes. A pane
/// line starts with `exec`, so the pane closes when this process ends, also
/// after a `load_brief` failure, and the spawner's error is the only one the
/// operator sees.
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

/// Clean up after a startup failure `e`: record it, leave it for the
/// spawner and close the pane.
fn fail_startup(steps: &mut dyn WorkerSteps, brief: &Brief, e: &anyhow::Error) {
    let role = &brief.role;
    warn_worker(
        role,
        "recording the startup failure failed",
        steps
            .startup_failed(brief, &format!("{e:#}"))
            .and_then(|()| steps.agent_exited(brief, None)),
    );
    // The close ends this process, so the error is printed and left
    // for the spawner first.
    eprintln!("horch worker[{role}]: startup failed: {e:#}");
    warn_worker(
        role,
        "could not leave the startup error",
        steps.leave_startup_error(&format!("{e:#}"), Some(brief)),
    );
    warn_worker(role, "could not close the pane", steps.close_pane());
}

/// The history event of a worker that failed before its agent launched. Its
/// text is the error.
pub const EVENT_STARTUP_FAILED: &str = "startup-failed";

/// Add an [`EVENT_STARTUP_FAILED`] history entry with `error` to the records
/// addressed by `key`.
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

/// The history event of a record whose agent exited before `horch done`.
/// Its text says how the agent ended.
pub const EVENT_EXITED: &str = "exited";

/// Add an [`EVENT_EXITED`] history entry for an agent that ended with `code`
/// to the records addressed by `key`.
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

/// Whose startup error a file holds (#192). Roles repeat across
/// workspaces, so the role alone does not name one worker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartupKey<'a> {
    /// The record the spawner minted, which the brief carries.
    Record(&'a str),
    /// A worker that could not read its brief: its public pane id, which
    /// herdr gives no other pane. Its workspace can be the wrong one: that
    /// is a common reason for a missing brief.
    Pane(&'a str),
}

impl StartupKey<'_> {
    /// The file name, or `None` when a part could name a path.
    fn file_name(&self) -> Option<String> {
        match *self {
            StartupKey::Record(id) => is_plain(id).then(|| format!("record-{id}.txt")),
            StartupKey::Pane(pane) => {
                // A public pane id is `<workspace>:<pane>`.
                let name = pane.replace(':', "_");
                is_plain(&name).then(|| format!("pane-{name}.txt"))
            }
        }
    }
}

/// Whether `s` is letters, digits, `-` and `_` only, and not empty.
fn is_plain(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Where a worker's startup error waits for `horch spawn`: under the data
/// root, which the pane command sets (`HORCH_DATA_DIR`), so the worker and
/// its spawner agree on it before the brief is read. The data root is
/// shared by every workspace of this user, so the file is named by
/// [`StartupKey`], not by role. `None` when the key could name a path.
pub fn startup_error_path(data_root: &Path, key: StartupKey<'_>) -> Option<PathBuf> {
    Some(data_root.join("startup-errors").join(key.file_name()?))
}

/// Write `error` to [`startup_error_path`], so `horch spawn` can show it.
pub fn write_startup_error(data_root: &Path, key: StartupKey<'_>, error: &str) -> Result<()> {
    let path = startup_error_path(data_root, key)
        .with_context(|| format!("{key:?} does not name a startup error file"))?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    crate::fsx::write_atomic(&path, error.as_bytes(), 0o600)
        .with_context(|| format!("writing {}", path.display()))
}

/// Read and delete the startup error under `key`. A file written before
/// `since` belongs to an earlier worker: it is deleted and not returned.
/// `since` is compared at whole seconds, the coarsest mtime a file system
/// keeps, so a file written in the same second as `since` counts even when
/// its mtime reads earlier.
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

/// How a spawned worker's start went, as [`await_worker`] saw it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkerStart {
    /// The worker registered its role.
    Registered,
    /// The worker failed before it registered. The text is its error.
    Failed(String),
    /// Neither happened within the wait. The worker can still register.
    Waiting,
}

/// Wait up to `limit` for the worker started at `since` to register
/// (`registered`) or to leave its startup error under one of `keys`,
/// whichever comes first. Polls every `poll`; an error found is deleted.
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

/// One look at how the worker's start went: `None` while neither happened.
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

/// What a harness launch needs besides the process context.
struct AgentPlan<'a> {
    roster: &'a Roster,
    teammate: &'a Teammate,
    prompt: &'a str,
}

/// Run the agent through its harness and wait. Every flag comes from the
/// teammate file, and every harness difference (codex rules, the Prime
/// daemon, session discovery) from its harness module: this only says which
/// pane, record and rules apply.
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

/// The worker in a real herdr pane.
pub struct PaneWorker<'a> {
    pub ctx: &'a mut RuntimeContext,
    pub role: &'a str,
    /// Make the project the process's working directory. The CLI does it:
    /// the agent starts there, and a relative roster path resolves from it.
    pub enter_dir: &'a dyn Fn(&Path) -> Result<()>,
    mailbox: Option<Mailbox>,
    path: Option<std::ffi::OsString>,
    /// The agent ran: [`WorkerSteps::launch`] returned its exit.
    launched: bool,
    /// The task ledger file and this worker's row in it, from the brief.
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

    /// Settle the worker row in the task ledger. The text to add to the
    /// orchestrator's line when the task is queued again or held.
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

    /// Tell the orchestrator that the agent exited early.
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

    /// The roster of this launch and the teammate the brief names.
    fn load_roster(&self, brief: &Brief) -> Result<(Roster, Teammate)> {
        let ctx: &RuntimeContext = self.ctx;
        // The teammate resolved at spawn time travels in the brief. Falling
        // back to the roster keeps briefs written by an older horch loadable.
        let roster = Roster::load_layered(
            ctx.inherited.home_var.as_deref().map(Path::new),
            ctx.bins.roster_override.as_deref(),
            brief.teammates_dir.as_deref(),
        )?;
        // F7: an overlay file that does not load is named once, in the pane.
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

    /// The task ledger gets the agent's PID the moment it starts, and a
    /// checkin from this process while the agent runs.
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

    /// A teammate with no harness: only a note, as a smoke test of the pane.
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

    fn herdr(&self) -> Herdr {
        Herdr::with_bin(&self.ctx.bins.harness.herdr)
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
        // The brief is this worker's context: its project, its ledger and
        // the binary overrides the spawner ran with. A pane is a fresh
        // shell, so none of them are in this process's environment. They
        // apply before any step that can fail (N-11): a startup failure is
        // recorded in the spawner's ledger, and the pane closes through the
        // spawner's herdr.
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
        self.ctx.apply_overrides(&brief.overrides());
        if !project.is_dir() {
            bail!("project dir '{}' is missing", brief.project_dir);
        }
        (self.enter_dir)(&project)
            .with_context(|| format!("entering project dir {}", brief.project_dir))?;
        self.ctx.paths.cwd = Some(project);
        // The briefing tells the agent to run `horch tell` / `horch note` /
        // `horch done` by bare name, so this binary's directory has to be
        // reachable. Without it a worker launched from a build directory
        // would have no channel at all.
        self.path = self.ctx.prepend_own_dir_to_path();
        Ok(())
    }

    fn register(&mut self, _brief: &Brief) -> Result<()> {
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
        // The agent starts in its workdir (a candidate's worktree), once the
        // roster has resolved from the project.
        if let Some(workdir) = &brief.workdir {
            (self.enter_dir)(Path::new(workdir))
                .with_context(|| format!("entering workdir {workdir}"))?;
        }
        let env = child_env(mailbox, brief, self.path.clone());
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

    /// An agent that exits before `horch done` reported nothing, and the
    /// orchestrator would wait for it forever: the record gets an
    /// [`EVENT_EXITED`] entry, and this process, still alive in the pane,
    /// tells the orchestrator. Only after an agent ran: a startup failure is
    /// `horch spawn`'s to report. A failed tell is printed.
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
        // herdr gives this pane its internal id; the spawner knows the
        // public one.
        let pane = self
            .ctx
            .herdr
            .pane
            .as_ref()
            .context("HERDR_PANE_ID is not set, so horch spawn cannot be told")?;
        let public = self.herdr().pane_get(pane.as_ref())?.pane_id;
        write_startup_error(&data_root, StartupKey::Pane(&public), error)
    }

    fn close_pane(&mut self) -> Result<()> {
        let pane = self
            .ctx
            .herdr
            .pane
            .as_ref()
            .context("HERDR_PANE_ID is not set")?;
        // herdr gives the pane its internal id; close it by its public id,
        // the one the ledgers record.
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

/// Read the session id the launch recorded in the session ledger, and find
/// its transcript, for the task ledger's worker row.
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
            crate::competition::observe::TelemetryUsage {
                locations: crate::usage::Locations::from_context(&ctx),
            }
            .transcript(&agent, Some(sid))
            .map(|p| p.to_string_lossy().into_owned())
        });
        (sid, transcript)
    })
}

/// What the agent inherits on top of this process's environment: the brief's
/// transport variables (how `horch note` and `horch done` run from inside the
/// agent know which record they belong to), the workspace this pane
/// registered in, and the PATH that reaches this binary. Applied to the child
/// only; this process's environment is never changed.
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

/// The smoke teammate: a fake agent that verifies the machinery (spawn,
/// register, ledger, tell, self-close) without spending any tokens.
///
/// These run as subprocesses invoked by bare name, deliberately: that is
/// exactly how a real agent calls them from its shell tool, so the check also
/// proves `horch` is reachable on the PATH the briefings promise.
fn run_smoke(env: &[(String, String)]) -> Result<()> {
    run_horch(
        env,
        &[
            "note",
            "smoke: worker launched, brief read, ledger reachable",
        ],
    )?;
    std::thread::sleep(Duration::from_secs(1));
    // The smoke orchestrator is a plain shell: no agent ever shows in its
    // pane, so `done`'s report must not wait for one (TELL_GRACE).
    let mut env = env.to_vec();
    env.push(("HORCH_TELL_GRACE_MS".into(), "0".into()));
    // `done` closes this pane, which kills this process tree; nothing after
    // it runs.
    run_horch(&env, &["done", "smoke: machinery verified end to end"])
}

/// The report of a worker whose agent exited before `horch done`.
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

/// Invoke `horch` the way an agent would: by name, off PATH, with the
/// environment an agent in this pane would have.
fn run_horch(env: &[(String, String)], args: &[&str]) -> Result<()> {
    // A PATH given to the child is also the one `Command` searches.
    let mut cmd = Command::new("horch");
    process::inherit_env(&mut cmd, env.iter().map(|(k, v)| (k, v)));
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
        /// `Some(cause)`: the push does not land.
        push_misses: Option<String>,
        queued: RefCell<Vec<String>>,
        /// `mark_done`, `after_mark_done`, `push` and `unregister`, in the
        /// order they ran.
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

    /// W2: a report that fails leaves the claim open and the record not
    /// done, so the retry with the same `--unlanded` reports the same line.
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

    /// #67: the record is done before the push, so a push the agent's
    /// command timeout kills leaves the work done. A push that does not
    /// land is noted, and `done` still closes the pane.
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

    /// #67: the push runs after the record is done and before the pane's
    /// closing steps; a worker that reports to nobody pushes nothing.
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

    /// #67: a ledger write that fails after the report is queued stops
    /// `done` before the push and the close. The queued mail stays for the
    /// courier; the retry queues the report again (as before #67, where a
    /// report was delivered before the same write).
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

    /// #67: a report that fails to queue is not pushed.
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

    /// W2: a record that an earlier `done` reported and marked is not
    /// reported or marked again; the retry only closes the pane.
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

    /// The brief's transport variables, the workspace and the PATH reach the
    /// agent's command and nothing else: the test process's own environment is
    /// the same afterwards, and `ANTHROPIC_API_KEY` is removed from the child
    /// even when the teammate's `env` block names it.
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
        // The resolved claude override is the program itself.
        assert_eq!(
            cmd.get_program(),
            std::ffi::OsStr::new("/fake/HORCH_CLAUDE_BIN")
        );
        assert_eq!(process_env(), before, "the test process env did not change");
    }

    /// `done` steps that close the pane during `settle`, the way the dataset
    /// coordinator can close it while `horch done` runs.
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

    /// A failed close is success when the pane is gone, and an error when
    /// the pane is still there.
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

    /// `done` steps whose mailbox removal fails, and record what ran.
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

    /// A mailbox removal that fails is a warning, not a stop: the pane still
    /// closes and the later steps still run.
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

    /// `done` steps that stop herdr (the server, not the pane) at a step,
    /// or close the pane there, and record what ran.
    struct HerdrDown<'a> {
        ws: &'a FakeWorkspace,
        pane: &'a str,
        /// `true`: the pane closes at `at`; `false`: herdr stops answering.
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

    /// After the summary is recorded, an unreachable herdr makes `done`
    /// fail: the pane may still be open. A gone pane on a reachable herdr
    /// is success. Both at `pane get` and at the close.
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

    /// A step of [`done`] before which the pane can vanish.
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

    /// `done` steps that close the pane before `vanish` and record what ran.
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
            // The next step is the report, or the `pane get` without one.
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

    /// The pane can vanish before any step of `done`. The summary is always
    /// recorded, `done` succeeds, and the steps that need only the workspace
    /// run when the workspace is known.
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
                // The workspace comes from the request, or from `pane get`
                // while the pane is still there.
                let known = workspace.is_some() || ![MarkDone, Report, PaneGet].contains(&vanish);
                let expected = if known { 4 } else { 1 };
                assert_eq!(ran.len(), expected, "{vanish:?}, {workspace:?}: {ran:?}");
            }
        }
    }

    /// U-40: a worker that fails at startup closes its own pane, after it
    /// records the failure. A pane that does not close does not hide the
    /// startup error.
    mod startup_failure {
        use super::*;

        /// Steps that fail at `enter_context` and close the pane through the
        /// fake workspace, the way `PaneWorker` does through herdr.
        struct Steps<'a> {
            ws: &'a FakeWorkspace,
            pane: String,
            calls: Vec<&'static str>,
            fail_at: &'static str,
            startup_error: Option<String>,
            left_error: Option<String>,
            /// The record id of the brief `leave_startup_error` got.
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
            // #192: after the brief loads, the error is keyed by its record.
            assert_eq!(steps.left_for, Some(Some("r1".to_string())));
        }

        /// W13 C4: the pane closes when `horch worker` exits, so a worker
        /// with no brief leaves its error for `horch spawn`.
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

        /// Steps that start and run, with the ledger writes made to fail.
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

        /// A ledger write that fails is logged and the worker goes on: the
        /// agent starts, and its exit code is the worker's.
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

        /// A launch that fails records the exit as a signal end, then
        /// returns the launch error.
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

        /// A file an earlier worker left is not this worker's error: it is
        /// deleted, and nothing is returned.
        #[test]
        fn a_stale_startup_error_is_deleted_and_ignored() {
            let tmp = tempfile::tempdir().unwrap();
            write_startup_error(tmp.path(), KEY, "old").unwrap();
            let later = std::time::SystemTime::now() + Duration::from_secs(60);
            assert_eq!(take_startup_error(tmp.path(), KEY, later), None);
            assert!(!startup_error_path(tmp.path(), KEY).unwrap().exists());
        }

        /// Write the startup error of [`KEY`] with its mtime set to `at`.
        fn write_startup_error_at(dir: &Path, at: SystemTime) {
            write_startup_error(dir, KEY, "boom").unwrap();
            let path = startup_error_path(dir, KEY).unwrap();
            let file = std::fs::File::options().write(true).open(path).unwrap();
            file.set_modified(at).unwrap();
        }

        /// A file system with whole-second mtimes stamps a file written in
        /// the spawn's second earlier than the spawn: it still counts.
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

        /// A file from the second before the spawn is an earlier worker's.
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

        /// A key names its file by its kind and keeps letters, digits, `-` and
        /// `_`. A pane id swaps its `:` for `_`; a record id does not. Anything
        /// else, `.` and a space and a non-ASCII letter included, could name
        /// a path and gives no file.
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

        /// #192: 2 spawns of the same role in 2 workspaces wait at the same
        /// time. The error of 1 worker reaches only the wait of its record,
        /// or of its pane when it had no brief.
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

            // A worker with no brief keys its error by its public pane id.
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
            // The error appears while the spawner waits.
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

        /// N-11: a project dir that is missing fails `enter_context` after
        /// the brief's `state_dir` and `project_dir` apply, so the startup
        /// failure and the exit reach the spawner's ledger.
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
    }
}
