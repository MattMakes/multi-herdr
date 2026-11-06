//! `horch spawn` - the port of `bin/horch-spawn`.
//!
//! Creates a new herdr pane running a worker agent, either as a FRESH session or
//! RESUMING a previous session id from the project ledger. Prints the new pane id
//! on stdout (human detail goes to stderr) so callers can chain splits when
//! building layouts. The rules live in `execution::plan` and the steps in
//! `execution::service`; this reads the inputs and prints. Then it waits up to
//! [`SPAWN_WAIT`] for the worker to register, and fails with the worker's
//! startup error when the worker dies first.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use anyhow::{bail, Context, Result};
use horch_core::execution::legacy::Record;
use horch_core::execution::lifecycle::{await_worker, StartupKey, WorkerStart};
use horch_core::execution::plan::{self, GateInputs, MintedIds, PlanInputs};
use horch_core::execution::service::{ExecutionService, SpawnError, SpawnOutcome};
use horch_core::execution::store::ExecutionStore;
use horch_core::execution::{
    ExecutionPlan, ExecutionStatus, LaunchStage, SessionMode, SpawnRequest, TilingMode,
};
use horch_core::ids::{ExecutionId, SessionId, TeammateName};
use horch_core::messaging::mailbox::Mailbox;
use horch_core::roster::{HorchAccess, Phase, Roster, Teammate};
use horch_core::routing::decision::{GateFlags, RoutingDecision, RoutingProvenance};
use horch_core::routing::policy::BalanceMode;
use horch_core::routing::quota::QuotaView;
use horch_core::runtime::RuntimeContext;
use horch_core::skills::{harness_support, SkillResolution};
use horch_core::taskledger::model::{Actor, EventKind};
use horch_core::taskledger::util::{event as ledger_event, About};
use horch_core::workspace::arrange;
use horch_core::workspace::herdr::Herdr;
use horch_core::workspace::model::Direction;

/// How long `horch spawn` waits for its worker to register or to leave a
/// startup error (`HORCH_SPAWN_WAIT_MS` overrides it). After it, the spawn
/// succeeds with a warning, and a worker that registers later still works.
pub const SPAWN_WAIT: Duration = Duration::from_secs(15);

/// How often the wait looks for the registration and the error file.
const SPAWN_POLL: Duration = Duration::from_millis(100);

pub struct SpawnArgs {
    pub teammate: Option<String>,
    /// Task text. Empty, except for a `spawn_only` controller's brief to a
    /// report role ([`check_task_text`]); every other spawn refuses it.
    pub task: String,
    pub resume: Option<String>,
    pub phase: Option<Phase>,
    /// Effort for this spawn only, over the teammate file's (or the record's).
    pub effort: Option<String>,
    pub role: Option<String>,
    pub from_pane: Option<String>,
    pub direction: Direction,
    /// `Disabled` leaves the grid alone. `HORCH_TILE=0` says the same thing
    /// for every spawn.
    pub tiling: TilingMode,
    /// Never substitute a fallback teammate (the usage-limit gate).
    pub exact: bool,
    /// Never refuse over usage limits.
    pub force: bool,
    /// The dead task-ledger worker this spawn replaces.
    pub replaces: Option<String>,
    /// The task-ledger run to join (`--run`).
    pub run: Option<String>,
}

/// The teammate `horch spawn`'s positional arguments name.
///
/// The two forms are `horch spawn <teammate>` and `horch spawn --resume <id>`.
/// Task text is refused (TRF-43, D1): pull is the only dispatch model, so a
/// worker gets its work by `horch ledger claim`. The refusal prints the
/// `horch ledger task` command that queues the same work. This is the view
/// of a spawn with no `spawn_only` controller ([`check_task_text`]).
#[cfg(test)]
pub fn resolve_positionals(positionals: &[String], resume: Option<&str>) -> Result<Option<String>> {
    let (teammate, task) = split_positionals(positionals, resume)?;
    if !task.trim().is_empty() {
        bail!(refuse_task_text(teammate.as_deref(), &task));
    }
    Ok(teammate)
}

/// The teammate and the task text of `horch spawn`'s positionals, with no
/// refusal of the task text. [`spawn`] refuses it unless
/// [`check_task_text`] allows it.
pub fn split_positionals(
    positionals: &[String],
    resume: Option<&str>,
) -> Result<(Option<String>, String)> {
    if resume.is_some() {
        return match positionals {
            [] => Ok((None, String::new())),
            [task] => Ok((None, task.clone())),
            _ => bail!("--resume takes no positional argument; usage: horch spawn --resume <id>"),
        };
    }
    match positionals {
        [] => bail!("give a teammate (e.g. `horch spawn sonnet`) or --resume <id>"),
        [name] => Ok((Some(name.clone()), String::new())),
        [name, task] => Ok((Some(name.clone()), task.clone())),
        _ => bail!("too many arguments; usage: horch spawn <teammate>"),
    }
}

/// Whether this spawn carries task text.
fn has_task_text(args: &SpawnArgs) -> bool {
    !args.task.trim().is_empty()
}

/// The 1 exception to TRF-43: a `spawn_only` controller sends a brief to a
/// role whose pane has `horch_access: report`. A report pane cannot run
/// `horch ledger claim` (its socket takes only progress, tell and done), so
/// the brief is its task. `controller` comes from [`spawn_only_check`]: the
/// caller's registered pane, its horch-written brief and that teammate's
/// globs, never a flag or a variable. Every other task text is refused.
fn check_task_text(
    args: &SpawnArgs,
    controller: Option<&str>,
    target: Option<&Teammate>,
) -> Result<()> {
    if !has_task_text(args) {
        return Ok(());
    }
    let report = target.is_some_and(|t| t.horch_access == HorchAccess::Report);
    if controller.is_some() && report && args.resume.is_none() {
        return Ok(());
    }
    bail!(refuse_task_text(args.teammate.as_deref(), &args.task))
}

/// The refusal of a spawn with task text: the `horch ledger task` command
/// that queues `task` for `teammate` instead.
fn refuse_task_text(teammate: Option<&str>, task: &str) -> String {
    format!(
        "horch spawn takes no task text: a worker claims its task from the task ledger. \
         Queue the task with:\n  horch ledger task {} --persona {}\n\
         It prints the horch spawn command that starts a worker for it.",
        shell_quote(task),
        teammate.unwrap_or("<persona>")
    )
}

/// `text` as 1 POSIX shell word.
fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

/// Spawn a worker pane, returning its pane id so callers can chain splits.
/// A refusal is a [`SpawnError::Refused`]; its REFUSED line is on stdout.
pub fn spawn(ctx: &RuntimeContext, args: SpawnArgs) -> Result<String> {
    // Before the ledger row: a refused caller leaves nothing behind.
    let roster_dir = super::path_text(ctx.bins.roster_override.as_deref());
    let caller = caller(ctx);
    let controller = spawn_only_check(caller.as_ref(), &args, roster_dir.as_deref())?;
    let brief = has_task_text(&args);
    if brief {
        let roster = super::load_roster(ctx, roster_dir.as_deref())?;
        let target = args.teammate.as_deref().and_then(|t| roster.get(t));
        check_task_text(&args, controller.as_deref(), target)?;
    }
    let sandboxed = sandboxed(caller.as_ref());
    // The task ledger: when this spawn joins a run, the worker gets a row
    // and the task to claim one. A controller's role joins the
    // controller's run and works from its brief (T-38).
    let controller_row = controller
        .as_ref()
        .and(caller.as_ref())
        .and_then(|c| c.worker_id.as_deref());
    let ledger = super::taskledger_spawn::prepare(ctx, &args, controller_row)?;
    let result = match ledger.as_ref() {
        Some(l) if !brief => refuse_over_plan(ctx, l)
            .and_then(|()| spawn_worker(ctx, args, ledger.as_ref(), controller, sandboxed)),
        _ => spawn_worker(ctx, args, ledger.as_ref(), controller, sandboxed),
    };
    // No row outlives a spawn that failed before its worker ran (#190).
    if let (Err(e), Some(l)) = (&result, &ledger) {
        let reason = match e.downcast_ref::<SpawnError>() {
            Some(SpawnError::Refused { line, .. }) => line.clone(),
            _ => format!("{e:#}"),
        };
        l.launch_failed(&reason);
    }
    result
}

/// The task of this spawn's worker: [`super::taskledger_spawn::CLAIM_TASK`]
/// when it joins a run, a `--resume` spawn too (TRF-33). Else none.
pub(crate) fn worker_task(joins_run: bool) -> &'static str {
    if joins_run {
        super::taskledger_spawn::CLAIM_TASK
    } else {
        ""
    }
}

/// Refuse a claiming spawn that its run's plan does not need (gap G-A): the
/// workers of its persona on their way already cover the startable tasks.
fn refuse_over_plan(ctx: &RuntimeContext, l: &super::taskledger_spawn::LedgerSpawn) -> Result<()> {
    let Some(store) = horch_core::taskledger::open_existing(ctx)? else {
        return Ok(());
    };
    match horch_core::taskledger::tasks::spawn_refusal(store.conn(), &l.run.run_id, &l.worker_id)? {
        Some(why) => bail!("horch spawn refused: {why}"),
        None => Ok(()),
    }
}

/// What `spawn_worker` reads before it plans, and the steps after it use.
struct SpawnContext {
    roster: Roster,
    herdr: Herdr,
    mailbox: Mailbox,
    project: PathBuf,
    store: ExecutionStore,
}

fn spawn_worker(
    ctx: &RuntimeContext,
    args: SpawnArgs,
    ledger: Option<&super::taskledger_spawn::LedgerSpawn>,
    controller: Option<String>,
    sandboxed: bool,
) -> Result<String> {
    let task = if has_task_text(&args) {
        args.task.clone()
    } else {
        worker_task(ledger.is_some()).to_string()
    };
    let req = build_request(&args, &task)?;
    let sc = spawn_context(ctx)?;
    let existing = resume_record(&req, &sc)?;
    let planned = plan_request(ctx, &req, &sc, existing.as_ref());
    if let Some(rec) = GateRecord::of(&planned) {
        record_gate(
            ctx,
            ledger.map(|l| (l.run.run_id.as_str(), l.worker_id.as_str())),
            &rec,
        );
    }
    let mut plan = planned?;
    plan.controller = controller;
    launch(ctx, &sc, plan, args.role.as_deref(), ledger, sandboxed)
}

/// 1 spawn-gate decision, as the task ledger records it (TRF-41). The
/// record is for measurement only: nothing reads it to decide a spawn.
#[derive(Debug, Clone, PartialEq, Eq)]
struct GateRecord {
    teammate: String,
    outcome: &'static str,
    pool: String,
    reason: String,
}

impl GateRecord {
    /// The gate decision of a planned spawn. `None` when no gate spoke: no
    /// gate ran, or the gate let the teammate run with no note.
    fn of(planned: &Result<ExecutionPlan>) -> Option<GateRecord> {
        match planned {
            Ok(p) if p.gate_line.is_some() => p.execution.routing.as_ref().map(Self::spoken),
            Ok(_) => None,
            Err(e) => match e.downcast_ref::<SpawnError>() {
                Some(SpawnError::Refused { decision, .. }) => Self::refused(decision),
                _ => None,
            },
        }
    }

    /// A NOTE, or a SUBSTITUTED when another teammate's settings run.
    fn spoken(r: &RoutingProvenance) -> GateRecord {
        let substituted = r.resolved != r.requested;
        let reason = r.reason.clone().unwrap_or_default();
        GateRecord {
            teammate: r.requested.as_str().to_string(),
            outcome: if substituted { "SUBSTITUTED" } else { "NOTE" },
            pool: r.pool.clone(),
            reason: if substituted {
                format!("runs on {}: {reason}", r.resolved.as_str())
            } else {
                reason
            },
        }
    }

    /// A REFUSED. Its pool names every pool the gate found closed.
    fn refused(d: &RoutingDecision) -> Option<GateRecord> {
        let RoutingDecision::Refuse {
            requested,
            reason,
            pools,
        } = d
        else {
            return None;
        };
        let pool: Vec<&str> = pools.iter().map(|p| p.pool.as_str()).collect();
        Some(GateRecord {
            teammate: requested.clone(),
            outcome: "REFUSED",
            pool: pool.join(","),
            reason: reason.clone(),
        })
    }

    /// The event text: `spawn gate: teammate=<t> outcome=<o> pool=<p> reason=<r>`.
    fn text(&self) -> String {
        format!(
            "spawn gate: teammate={} outcome={} pool={} reason={}",
            self.teammate, self.outcome, self.pool, self.reason
        )
    }
}

/// Write 1 ledger event for a gate decision, about the run and the worker
/// this spawn joins (`about`), if any. Best effort: a failed write prints a
/// warning and never changes the spawn.
fn record_gate(ctx: &RuntimeContext, about: Option<(&str, &str)>, rec: &GateRecord) {
    let write = || -> Result<()> {
        let mut store = horch_core::taskledger::open(ctx)?;
        let about = About {
            run: about.map(|(run, _)| run),
            task: None,
            worker: about.map(|(_, worker)| worker),
        };
        store.immediate(|tx| ledger_event(tx, &about, Actor::Horch, EventKind::Note, &rec.text()))
    };
    if let Err(e) = write() {
        eprintln!("horch spawn: the task ledger did not record the gate decision: {e:#}");
    }
}

/// The worker that runs this `horch spawn`: the role registered on this
/// pane, in the pane's own workspace, and the teammate its brief resolved.
/// `HORCH_ROLE` is not read. No pane, no registered role or no readable
/// brief: no caller (the orchestrator, an operator shell).
struct Caller {
    role: String,
    teammate: Teammate,
    teammates_dir: Option<String>,
    /// The caller's task-ledger row, from its brief.
    worker_id: Option<String>,
}

fn caller(ctx: &RuntimeContext) -> Option<Caller> {
    let pane = ctx.herdr.pane.as_ref()?;
    let info = Herdr::with_bin(&ctx.bins.harness.herdr)
        .pane_get(pane.as_str())
        .ok()?;
    let workspace = info
        .workspace_id
        .clone()
        .or_else(|| ctx.herdr.workspace.as_ref().map(|w| w.to_string()))?;
    let mailbox = Mailbox::in_context(ctx, &workspace);
    let (role, _) = mailbox
        .roles()
        .into_iter()
        .find(|(_, p)| *p == info.pane_id)?;
    let brief = mailbox.read_brief(&role).ok()?;
    Some(Caller {
        role,
        teammate: brief.resolved?,
        teammates_dir: brief.teammates_dir,
        worker_id: brief.worker_id,
    })
}

/// Whether a sandboxed worker runs this spawn.
fn sandboxed(caller: Option<&Caller>) -> bool {
    caller.is_some_and(|c| c.teammate.sandbox.is_some())
}

/// Run the state sweep, unless a sandboxed worker runs this spawn: its
/// sandbox denies the removal of skill bundles (ticket 28). A skipped
/// sweep writes no stamp, so the next spawn outside a sandbox sweeps on
/// time.
fn sweep_state(ctx: &RuntimeContext, sandboxed: bool) {
    if !sandboxed {
        horch_core::sweep::run_due(ctx);
    }
}

/// Refuse what the caller's `spawn_only` does not allow: a name that
/// matches no glob, `--resume`, and a roster folder other than the one its
/// brief names. Returns the controller for the new worker's brief: the
/// caller's role when it has `spawn_only`, else none.
fn spawn_only_check(
    caller: Option<&Caller>,
    args: &SpawnArgs,
    roster_dir: Option<&str>,
) -> Result<Option<String>> {
    let Some(c) = caller.filter(|c| !c.teammate.spawn_only.is_empty()) else {
        return Ok(None);
    };
    let who = format!("{} (teammate {})", c.role, c.teammate.name);
    let only = c.teammate.spawn_only.join(", ");
    if args.resume.is_some() {
        bail!("horch spawn refused: {who} has spawn_only [{only}], so --resume is refused");
    }
    if roster_dir != c.teammates_dir.as_deref() {
        bail!(
            "horch spawn refused: {who} has spawn_only [{only}], so HORCH_TEAMMATES_DIR={} \
             is refused; its brief names {}",
            roster_dir.unwrap_or("(unset)"),
            c.teammates_dir.as_deref().unwrap_or("(unset)")
        );
    }
    let name = args.teammate.as_deref().unwrap_or_default();
    if !c.teammate.may_spawn(name) {
        bail!("horch spawn refused: {who} may spawn only [{only}], and '{name}' matches none");
    }
    Ok(Some(c.role.clone()))
}

/// The record a resume continues, when the request has one.
fn resume_record(req: &SpawnRequest, sc: &SpawnContext) -> Result<Option<Record>> {
    // A record whose pane closed without `horch done` ends here, so it resumes.
    if let Some(key) = req.resume.as_deref() {
        sc.store.end_if_pane_closed(key, &sc.herdr)?;
    }
    let existing = req.resume.as_deref().and_then(|key| sc.store.get(key).ok());
    refuse_competition_record(existing.as_ref())?;
    Ok(existing)
}

/// Gate, mint ids, read skills and plan. Print the gate line.
fn plan_request(
    ctx: &RuntimeContext,
    req: &SpawnRequest,
    sc: &SpawnContext,
    existing: Option<&Record>,
) -> Result<ExecutionPlan> {
    let gated = gate_inputs(ctx, req, &sc.roster)?;
    let ids = MintedIds {
        execution: ExecutionId::new(horch_core::mint_uuid())?,
        session: SessionId::new(horch_core::mint_uuid())?,
    };
    let skills = read_skills(ctx, req, &sc.roster, existing)?;
    let inputs = PlanInputs {
        roster: &sc.roster,
        skills: &skills,
        gate: gated.as_ref().map(|(view, balance)| GateInputs {
            view,
            balance: *balance,
        }),
        existing,
        now: horch_core::clock::now(),
        ids: &ids,
        project: &sc.project,
    };
    let plan = plan(req, &inputs)?;
    if let Some(line) = &plan.gate_line {
        crate::output::println(line);
    }
    Ok(plan)
}

/// Start the planned worker, then wait for it and report it.
fn launch(
    ctx: &RuntimeContext,
    sc: &SpawnContext,
    plan: ExecutionPlan,
    role: Option<&str>,
    ledger: Option<&super::taskledger_spawn::LedgerSpawn>,
    sandboxed: bool,
) -> Result<String> {
    // Advisory probes belong after validation: a refused/headless request
    // must not contact herdr or create diagnostic state.
    fleet_warnings(ctx, true);

    let tile = |pane: &str, tiling: TilingMode| tile_pane(ctx, sc, pane, tiling);
    let service = ExecutionService {
        ctx,
        store: &sc.store,
        workspace: &sc.herdr,
        mailbox: &sc.mailbox,
        tile: &tile,
        ledger_worker: ledger.map(|l| l.worker_id.as_str()),
    };
    let started = SystemTime::now();
    let out = service.spawn(plan, role).inspect_err(|e| {
        if let Some(l) = ledger {
            l.launch_failed(&format!("{e:#}"));
        }
    })?;
    sweep_state(ctx, sandboxed);
    let launch = &out.plan.launch;
    if let Some(l) = ledger {
        l.spawned(
            out.plan.execution.role.as_str(),
            &out.pane,
            launch.teammate.agent.as_str(),
            &launch.model,
            out.plan.execution.effort.as_deref(),
        );
    }
    wait_for_start(ctx, sc, &out, started, ledger)?;
    eprintln!("{}", spawned_line(&out));
    Ok(out.pane)
}

/// The request `horch spawn` plans, from its arguments.
fn build_request(args: &SpawnArgs, task: &str) -> Result<SpawnRequest> {
    let mut req = SpawnRequest::worker(
        args.teammate
            .as_deref()
            .map(TeammateName::new)
            .transpose()?,
        task.to_string(),
    );
    if req.teammate.is_none() && args.resume.is_none() {
        return Err(SpawnError::from(plan::PlanError::NothingToSpawn).into());
    }
    (req.resume, req.phase, req.effort) = (args.resume.clone(), args.phase, args.effort.clone());
    (req.from_pane, req.direction, req.tiling) =
        (args.from_pane.clone(), args.direction, args.tiling);
    req.flags = GateFlags {
        exact: args.exact,
        force: args.force,
    };
    Ok(req)
}

/// The roster, the workspace, the mailbox, the project and the store.
fn spawn_context(ctx: &RuntimeContext) -> Result<SpawnContext> {
    let roster_dir = super::path_text(ctx.bins.roster_override.as_deref());
    let roster = super::load_roster(ctx, roster_dir.as_deref())?;
    let herdr = Herdr::with_bin(&ctx.bins.harness.herdr);
    let mailbox = Mailbox::resolve_in(&herdr, ctx)
        .context("horch spawn needs HORCH_WORKSPACE_ID set, or to run inside a herdr pane")?;
    std::fs::create_dir_all(mailbox.dir())?;
    // Read once, so the ledger this process writes and the one the worker
    // later reads (its brief carries the project) are the same file.
    let project = ctx.paths.project()?;
    // The project's facts add the teammate's `skills_when` skills at planning.
    let roster = roster.with_project_facts(horch::project::project_facts(&project));
    let store = ExecutionStore::open_in(ctx)?;
    Ok(SpawnContext {
        roster,
        herdr,
        mailbox,
        project,
        store,
    })
}

/// A competition record, from an older ledger, is never resumed: the
/// competition rounds that ran it are removed.
fn refuse_competition_record(existing: Option<&Record>) -> Result<()> {
    let Some(r) = existing.filter(|r| r.round_id.is_some()) else {
        return Ok(());
    };
    bail!(
        "record {} is a competition {} (round {}) from a removed feature, \
         so horch spawn --resume refuses it",
        r.record_id,
        if r.experiment_id.is_some() {
            "candidate"
        } else {
            "judge"
        },
        r.round_id.as_deref().unwrap_or_default()
    )
}

/// The quota view and balance mode, when the request spends quota.
fn gate_inputs(
    ctx: &RuntimeContext,
    req: &SpawnRequest,
    roster: &Roster,
) -> Result<Option<(QuotaView, BalanceMode)>> {
    if !plan::needs_gate(req, roster) {
        return Ok(None);
    }
    let root = &ctx.paths.state_root;
    let policy = super::quotacmd::load_policy(ctx, root)?;
    let env = super::quotacmd::quota_env(ctx);
    let now = horch_core::clock::now();
    let view = horch_core::routing::snapshot::obtain(root, now, &policy, true, &env)?;
    Ok(Some((view, policy.balance_mode)))
}

/// Planning reads no files, so the operator and plugin skills of the
/// teammate that will launch are read here. Then the record lists them,
/// as the launch bundle does.
fn read_skills(
    ctx: &RuntimeContext,
    req: &SpawnRequest,
    roster: &Roster,
    existing: Option<&Record>,
) -> Result<SkillResolution> {
    let launching = req
        .teammate
        .as_ref()
        .map(|t| t.as_str())
        .or(existing.map(|r| r.tier.as_str()));
    SkillResolution::read(
        roster.skill_catalog()?,
        launching.and_then(|name| roster.get(name)),
        ctx.inherited.home_var.as_deref().map(Path::new),
        harness_support,
    )
}

/// Plan the launch. A refusal prints its REFUSED line before it fails.
fn plan(req: &SpawnRequest, inputs: &PlanInputs) -> Result<ExecutionPlan> {
    plan::plan_launch(req, inputs)
        .map_err(SpawnError::from)
        .inspect_err(|e| {
            if let SpawnError::Refused { line, .. } = e {
                crate::output::println(line);
            }
        })
        .map_err(anyhow::Error::from)
}

/// Lay the grid out, which also puts the pane where it belongs: a split
/// halves its parent, and the tiler moves the worker into the next free
/// slot and evens every column. Best effort: a grid that will not lay out
/// must not fail a spawn whose worker is already running.
fn tile_pane(ctx: &RuntimeContext, sc: &SpawnContext, pane: &str, tiling: TilingMode) {
    if tiling != TilingMode::Disabled {
        arrange::after_change(ctx, &sc.herdr, sc.mailbox.workspace_id(), Some(pane));
    } else if let Ok(layout) = sc.herdr.pane_layout(Some(pane)) {
        arrange::equalize_quietly(ctx, &sc.herdr, layout);
    }
}

/// Wait for the worker to register. A pane line starts with `exec`, so a
/// worker that fails at startup closes its pane and takes its stderr with
/// it. It leaves the error in a file instead, and this wait reads it.
fn wait_for_start(
    ctx: &RuntimeContext,
    sc: &SpawnContext,
    out: &SpawnOutcome,
    started: SystemTime,
    ledger: Option<&super::taskledger_spawn::LedgerSpawn>,
) -> Result<()> {
    let wait = ctx.settings.spawn_wait.unwrap_or(SPAWN_WAIT);
    if wait.is_zero() {
        return Ok(());
    }
    let (role, pane) = (&out.plan.execution.role, &out.pane);
    let mut registered = || sc.mailbox.pane_for(role.as_str()).as_deref() == Some(pane.as_str());
    // The worker keys its error by this record, or by its pane when it
    // could not read its brief (#192).
    let keys = [
        StartupKey::Record(out.plan.execution.id.as_str()),
        StartupKey::Pane(pane.as_str()),
    ];
    let data_root = &ctx.paths.data_root;
    match await_worker(data_root, &keys, started, wait, SPAWN_POLL, &mut registered) {
        WorkerStart::Registered => Ok(()),
        WorkerStart::Failed(error) => {
            record_failed_start(sc, out, &error, ledger);
            bail!("worker {role} (pane {pane}) failed to start: {error}")
        }
        WorkerStart::Waiting => {
            eprintln!(
                "horch spawn: worker {role} (pane {pane}) has not registered after {} ms; \
                 it can still start",
                wait.as_millis()
            );
            Ok(())
        }
    }
}

/// End the record of a worker that failed at startup, and clean up after it.
fn record_failed_start(
    sc: &SpawnContext,
    out: &SpawnOutcome,
    error: &str,
    ledger: Option<&super::taskledger_spawn::LedgerSpawn>,
) {
    let id = out.plan.execution.id.as_str();
    let state = ExecutionStatus::LaunchFailed {
        stage: LaunchStage::Run,
        reason: error.to_string(),
    };
    if let Err(e) = sc.store.end_live(id, state) {
        eprintln!("horch spawn: recording the failed start of {id} failed: {e:#}");
    }
    // The pane closed with the worker, or the worker closed it;
    // this close is for a pane that did neither.
    let _ = sc.herdr.pane_close(&out.pane);
    sc.mailbox.unregister(out.plan.execution.role.as_str());
    if let Some(l) = ledger {
        l.launch_failed(error);
    }
}

/// The line `horch spawn` prints on stderr when the worker is up.
fn spawned_line(out: &SpawnOutcome) -> String {
    let (launch, role, pane) = (&out.plan.launch, &out.plan.execution.role, &out.pane);
    let what = format!("{} {}", launch.teammate.agent, launch.model);
    match &launch.session {
        SessionMode::Resume(id) => {
            format!("spawned {role} (pane {pane}): {what}, RESUMING session {id}")
        }
        SessionMode::Fresh(None) => format!("spawned {role} (pane {pane}): {what}, new session"),
        SessionMode::Fresh(Some(id)) => {
            format!("spawned {role} (pane {pane}): {what}, new session {id}")
        }
    }
}

/// Advisory collision/load diagnostics; abandoned workspaces do not count.
pub fn fleet_warnings(ctx: &RuntimeContext, as_note: bool) {
    let prefix = if as_note { "NOTE:" } else { "WARNING:" };
    if let Some(warning) = ctx.paths.ledger_isolation_warning() {
        eprintln!("horch: {prefix} {warning}");
    }
    let caller = super::sessionscmd::caller_workspace(ctx);
    let herdr = Herdr::with_bin(&ctx.bins.harness.herdr);
    if let Ok(workspaces) = herdr.workspace_list() {
        let listed = |w: &str| workspaces.iter().any(|ws| ws.workspace_id == w);
        let mut active = std::collections::BTreeMap::<String, Option<String>>::new();
        // A read only: a scratch fleet must not create the ledger (#199).
        if let Ok(Some(store)) = horch_core::taskledger::open_existing(ctx) {
            if let Ok(root) = super::taskledger::project_root(ctx) {
                if let Ok(runs) = horch_core::taskledger::runs::of_project(store.conn(), &root) {
                    for run in runs.into_iter().filter(|r| r.status == "active") {
                        if let Some(w) = run
                            .workspace_id
                            .filter(|w| caller.as_deref() != Some(w) && listed(w))
                        {
                            active.insert(w, Some(run.slug));
                        }
                    }
                }
            }
        }
        if let Ok(store) = ExecutionStore::open_in(ctx) {
            for r in store
                .read()
                .unwrap_or_default()
                .into_iter()
                .filter(|r| r.is_orchestrator() && r.execution_status().is_live())
            {
                if let Some(w) = r
                    .workspace_id
                    .filter(|w| caller.as_deref() != Some(w) && listed(w))
                {
                    active.entry(w).or_insert(None);
                }
            }
        }
        if !active.is_empty() {
            let names: Vec<_> = active
                .into_iter()
                .map(|(w, slug)| slug.map_or_else(|| w.clone(), |s| format!("{w} (run {s})")))
                .collect();
            eprintln!(
                "horch: {prefix} live orchestrator in workspace {}",
                names.join(", ")
            );
        }
    }
    if as_note {
        let project = ctx
            .paths
            .project()
            .unwrap_or_else(|_| Path::new(".").to_path_buf());
        let machine = horch_core::runtime::machine::probe(
            &project,
            &Default::default(),
            ctx.settings.machine_file.as_deref(),
        );
        if machine.os != "windows" {
            if let (Some(load), horch_core::runtime::machine::Known::Known(cpus)) =
                (machine.load_average_1m, machine.cpus)
            {
                if load.is_finite() && cpus > 0 && load >= f64::from(cpus) {
                    eprintln!(
                        "horch: NOTE: host 1-minute load {load:.2} is at or above {cpus} CPUs"
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use horch_core::roster::Roster;

    #[test]
    fn a_bare_teammate_spawns_an_idle_worker() {
        let name = resolve_positionals(&["sonnet".into()], None).unwrap();
        assert_eq!(name.as_deref(), Some("sonnet"));
    }

    #[test]
    fn trf_43_spawn_with_task_text_prints_the_ledger_task_command() {
        let err = resolve_positionals(&["codex-sol".into(), "fix Bob's auth".into()], None)
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("horch ledger task 'fix Bob'\\''s auth' --persona codex-sol"),
            "{err}"
        );
    }

    #[test]
    fn trf_43_resume_with_task_text_prints_the_ledger_task_command() {
        let err = resolve_positionals(&["continue the refactor".into()], Some("s-1"))
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("horch ledger task 'continue the refactor' --persona <persona>"),
            "{err}"
        );
    }

    #[test]
    fn resume_without_a_task_is_allowed() {
        assert_eq!(resolve_positionals(&[], Some("s-1")).unwrap(), None);
    }

    #[test]
    fn no_arguments_at_all_explains_both_forms() {
        let err = resolve_positionals(&[], None).unwrap_err().to_string();
        assert!(err.contains("horch spawn sonnet"), "{err}");
        assert!(err.contains("--resume"), "{err}");
    }

    /// Teammate names come from a directory, not a closed enum, so argv parsing
    /// accepts any word and the roster is what rejects it - with a list of what
    /// actually exists on this machine, which a compiled-in enum could not give.
    #[test]
    fn an_unknown_teammate_lists_the_valid_ones() {
        let name = resolve_positionals(&["haiku".into()], None).unwrap();
        let err = Roster::builtin()
            .unwrap()
            .require(name.as_deref().unwrap())
            .unwrap_err()
            .to_string();
        assert!(err.contains("unknown teammate 'haiku'"), "{err}");
        assert!(err.contains("codex-terra"), "{err}");
    }

    fn args(teammate: Option<&str>, resume: Option<&str>) -> SpawnArgs {
        SpawnArgs {
            teammate: teammate.map(str::to_string),
            task: String::new(),
            resume: resume.map(str::to_string),
            phase: None,
            effort: None,
            role: None,
            from_pane: None,
            direction: Direction::Right,
            tiling: TilingMode::Disabled,
            exact: false,
            force: false,
            replaces: None,
            run: None,
        }
    }

    fn importer(spawn_only: &[&str]) -> Caller {
        Caller {
            role: "skill-importer-1".into(),
            teammate: horch_core::roster::Teammate {
                name: "skill-importer".into(),
                spawn_only: spawn_only.iter().map(|s| s.to_string()).collect(),
                ..Default::default()
            },
            teammates_dir: Some("/repo/teammates".into()),
            worker_id: None,
        }
    }

    /// A spawn skips the state sweep only when a sandboxed worker runs it
    /// (ticket 28): an operator shell, the orchestrator and an unsandboxed
    /// worker sweep.
    #[test]
    fn only_a_sandboxed_caller_skips_the_sweep() {
        let mut boxed = importer(&[]);
        boxed.teammate.sandbox = Some(serde_json::Map::new());
        assert!(sandboxed(Some(&boxed)));
        assert!(!sandboxed(Some(&importer(&[]))));
        assert!(!sandboxed(None));
    }

    /// A skipped sweep writes no stamp, so the next unsandboxed spawn
    /// sweeps on time; a sweep that runs writes it.
    #[test]
    fn a_skipped_sweep_writes_no_stamp() {
        let tmp = tempfile::tempdir().unwrap();
        let state = tmp.path().join("state");
        std::fs::create_dir(&state).unwrap();
        // A herdr that does not answer leaves all state alone.
        let env = horch_core::runtime::MapEnv::new(tmp.path())
            .with("HOME", &tmp.path().to_string_lossy())
            .with("HORCH_STATE_DIR", &state.to_string_lossy())
            .with(
                "HORCH_HERDR_BIN",
                &tmp.path().join("no-herdr").to_string_lossy(),
            );
        let ctx = RuntimeContext::from_env(&env).unwrap();
        let stamp = state.join(".sweep-stamp");
        sweep_state(&ctx, true);
        assert!(!stamp.exists(), "a sandboxed spawn must not stamp");
        sweep_state(&ctx, false);
        assert!(stamp.exists(), "an unsandboxed spawn sweeps and stamps");
    }

    /// A caller with `spawn_only` spawns a matching teammate, and the new
    /// worker's controller is the caller's role.
    #[test]
    fn spawn_only_allows_a_matching_name_and_names_the_controller() {
        let c = importer(&["intake-*"]);
        let got = spawn_only_check(
            Some(&c),
            &args(Some("intake-auditor"), None),
            Some("/repo/teammates"),
        )
        .unwrap();
        assert_eq!(got.as_deref(), Some("skill-importer-1"));
    }

    #[test]
    fn spawn_only_refuses_a_name_that_matches_no_glob() {
        let c = importer(&["intake-*"]);
        let err = spawn_only_check(Some(&c), &args(Some("opus"), None), Some("/repo/teammates"))
            .unwrap_err()
            .to_string();
        assert_eq!(
            err,
            "horch spawn refused: skill-importer-1 (teammate skill-importer) may spawn \
             only [intake-*], and 'opus' matches none"
        );
    }

    /// The caller cannot point the roster at a folder it wrote: only the
    /// folder its own brief names passes.
    #[test]
    fn spawn_only_refuses_another_teammates_dir() {
        let c = importer(&["intake-*"]);
        for dir in [Some("/tmp/evil"), None] {
            let err = spawn_only_check(Some(&c), &args(Some("intake-auditor"), None), dir)
                .unwrap_err()
                .to_string();
            assert!(
                err.starts_with(
                    "horch spawn refused: skill-importer-1 (teammate skill-importer) has \
                     spawn_only [intake-*], so HORCH_TEAMMATES_DIR="
                ),
                "{err}"
            );
        }
    }

    #[test]
    fn spawn_only_refuses_resume() {
        let c = importer(&["intake-*"]);
        let err = spawn_only_check(Some(&c), &args(None, Some("s-1")), Some("/repo/teammates"))
            .unwrap_err()
            .to_string();
        assert_eq!(
            err,
            "horch spawn refused: skill-importer-1 (teammate skill-importer) has \
             spawn_only [intake-*], so --resume is refused"
        );
    }

    /// A caller without the key, and no caller (the orchestrator or an
    /// operator shell), spawn as before: any name, any roster, resume.
    #[test]
    fn a_caller_without_spawn_only_is_unchanged() {
        let c = importer(&[]);
        for caller in [Some(&c), None] {
            for a in [args(Some("opus"), None), args(None, Some("s-1"))] {
                assert_eq!(
                    spawn_only_check(caller, &a, Some("/tmp/other")).unwrap(),
                    None
                );
            }
        }
    }

    /// TRF-43's 1 exception: `skill-importer` (spawn_only `intake-*`) sends a
    /// brief to `intake-writer`, a `horch_access: report` pane.
    #[test]
    fn trf_43_spawn_only_controller_may_send_a_brief() {
        let roster = Roster::builtin().unwrap();
        let importer_caller = importer(&["intake-*"]);
        let mut a = args(Some("intake-writer"), None);
        a.task = "Read and follow /x/brief.md exactly.".into();
        let controller =
            spawn_only_check(Some(&importer_caller), &a, Some("/repo/teammates")).unwrap();
        assert_eq!(controller.as_deref(), Some("skill-importer-1"));
        let target = roster.get("intake-writer");
        assert_eq!(target.unwrap().horch_access, HorchAccess::Report);
        check_task_text(&a, controller.as_deref(), target).unwrap();
        // No task text needs no exception.
        check_task_text(&args(Some("opus"), None), None, roster.get("opus")).unwrap();
    }

    /// Every other task text keeps the TRF-43 refusal: no controller, a
    /// controller's brief to a pane with full horch access, an unknown
    /// teammate, and a resume.
    #[test]
    fn trf_43_task_text_refused_outside_spawn_only() {
        let roster = Roster::builtin().unwrap();
        let brief = |teammate: Option<&str>, resume: Option<&str>| {
            let mut a = args(teammate, resume);
            a.task = "fix Bob's auth".into();
            a
        };
        let refused = |a: &SpawnArgs, controller: Option<&str>, target: Option<&Teammate>| {
            let err = check_task_text(a, controller, target)
                .unwrap_err()
                .to_string();
            assert_eq!(err, refuse_task_text(a.teammate.as_deref(), &a.task));
            assert!(
                err.contains("horch ledger task 'fix Bob'\\''s auth'"),
                "{err}"
            );
        };
        let writer = brief(Some("intake-writer"), None);
        refused(&writer, None, roster.get("intake-writer"));
        let opus = brief(Some("opus"), None);
        refused(&opus, Some("skill-importer-1"), roster.get("opus"));
        refused(&brief(Some("nobody"), None), Some("skill-importer-1"), None);
        refused(
            &brief(None, Some("s-1")),
            Some("skill-importer-1"),
            roster.get("intake-writer"),
        );
        // A caller without spawn_only is no controller, so its brief is refused.
        let plain = importer(&[]);
        let controller = spawn_only_check(Some(&plain), &writer, Some("/repo/teammates")).unwrap();
        assert_eq!(controller, None);
        refused(&writer, controller.as_deref(), roster.get("intake-writer"));
    }

    #[test]
    fn extra_positionals_are_rejected_rather_than_silently_dropped() {
        assert!(
            resolve_positionals(&["sonnet".into(), "task".into(), "extra".into()], None).is_err()
        );
        assert!(resolve_positionals(&["task".into(), "extra".into()], Some("s-1")).is_err());
    }
}

#[cfg(test)]
mod trf_41_tests {
    use std::path::Path;

    use horch_core::ids::TeammateName;
    use horch_core::routing::decision::{PoolLine, RoutingMode};
    use horch_core::routing::quota::State;
    use horch_core::runtime::MapEnv;
    use horch_core::taskledger::runs::{self, NewRun, Run};
    use horch_core::taskledger::tasks::{self, NewTask};
    use horch_core::taskledger::Store;

    use super::*;

    fn provenance(requested: &str, resolved: &str, reason: Option<&str>) -> RoutingProvenance {
        RoutingProvenance {
            requested: TeammateName::new(requested).unwrap(),
            resolved: TeammateName::new(resolved).unwrap(),
            fallback_index: (requested != resolved).then_some(0),
            pool: "codex".into(),
            pool_state: State::Ok,
            reason: reason.map(str::to_string),
            mode: RoutingMode::Auto,
        }
    }

    fn refusal() -> RoutingDecision {
        RoutingDecision::Refuse {
            requested: "opus".into(),
            reason: "every pool is exhausted".into(),
            pools: vec![
                PoolLine {
                    pool: "claude".into(),
                    state: "exhausted".into(),
                    detail: "7d 100%".into(),
                },
                PoolLine {
                    pool: "codex".into(),
                    state: "exhausted".into(),
                    detail: "7d 99%".into(),
                },
            ],
        }
    }

    /// A ledger with 1 run and 1 startable `opus` task.
    fn world() -> (tempfile::TempDir, RuntimeContext, Store, Run) {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("ledger.sqlite3");
        let ctx = RuntimeContext::from_env(
            &MapEnv::new("/")
                .with(
                    "HORCH_STATE_DIR",
                    dir.path().join("state").to_str().unwrap(),
                )
                .with("HORCH_LEDGER_DB", db.to_str().unwrap()),
        )
        .unwrap();
        let mut store = Store::open(&db).unwrap();
        let run = store
            .immediate(|tx| {
                runs::create(
                    tx,
                    Path::new("/repo"),
                    &NewRun {
                        slug: "r1".into(),
                        ..Default::default()
                    },
                )
            })
            .unwrap();
        store
            .immediate(|tx| {
                tasks::add(
                    tx,
                    &run,
                    &NewTask::work("alpha", "tickets/alpha.md", "opus"),
                    Actor::Orchestrator,
                )
            })
            .unwrap();
        (dir, ctx, store, run)
    }

    /// The gate events of a run: (actor, kind, worker, text).
    fn gate_events(store: &Store, run: &Run) -> Vec<(String, String, Option<String>, String)> {
        let mut stmt = store
            .conn()
            .prepare(
                "SELECT actor, kind, worker_id, text FROM events
                 WHERE run_id = ?1 AND text LIKE 'spawn gate:%' ORDER BY event_id",
            )
            .unwrap();
        stmt.query_map([&run.run_id], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
    }

    #[test]
    fn trf_41_a_note_records_the_teammate_its_pool_and_the_note() {
        let rec = GateRecord::spoken(&provenance("opus", "opus", Some("claude 7d 91%")));
        assert_eq!(
            rec.text(),
            "spawn gate: teammate=opus outcome=NOTE pool=codex reason=claude 7d 91%"
        );
    }

    #[test]
    fn trf_41_a_substitution_records_the_requested_teammate_and_the_one_that_runs() {
        let rec = GateRecord::spoken(&provenance("opus", "codex-sol", Some("claude 7d 100%")));
        assert_eq!(rec.outcome, "SUBSTITUTED");
        assert_eq!(rec.teammate, "opus");
        assert_eq!(rec.reason, "runs on codex-sol: claude 7d 100%");
    }

    #[test]
    fn trf_41_a_refusal_records_every_closed_pool() {
        let rec = GateRecord::refused(&refusal()).unwrap();
        assert_eq!(
            rec.text(),
            "spawn gate: teammate=opus outcome=REFUSED pool=claude,codex reason=every pool is exhausted"
        );
        let err: anyhow::Error = SpawnError::Refused {
            decision: refusal(),
            line: "REFUSED: opus cannot start.".into(),
        }
        .into();
        assert_eq!(GateRecord::of(&Err(err)), Some(rec));
    }

    #[test]
    fn trf_41_a_failure_that_is_not_a_gate_decision_records_nothing() {
        assert_eq!(GateRecord::of(&Err(anyhow::anyhow!("herdr is gone"))), None);
        let spawn = RoutingDecision::Spawn {
            teammate: "opus".into(),
        };
        assert_eq!(GateRecord::refused(&spawn), None);
    }

    #[test]
    fn trf_41_each_decision_is_1_ledger_event_about_the_run_and_the_worker() {
        let (_dir, ctx, store, run) = world();
        let about = Some((run.run_id.as_str(), "w-1"));
        record_gate(
            &ctx,
            about,
            &GateRecord::spoken(&provenance("opus", "opus", Some("n"))),
        );
        record_gate(&ctx, about, &GateRecord::refused(&refusal()).unwrap());
        let events = gate_events(&store, &run);
        assert_eq!(events.len(), 2, "{events:?}");
        for (actor, kind, worker, _) in &events {
            assert_eq!(
                (actor.as_str(), kind.as_str(), worker.as_deref()),
                ("horch", "note", Some("w-1"))
            );
        }
        assert!(events[0].3.contains("outcome=NOTE"));
        assert!(events[1].3.contains("outcome=REFUSED"));
    }

    #[test]
    fn trf_41_the_record_never_changes_routing() {
        let (_dir, ctx, store, run) = world();
        let before = tasks::plan_workers(store.conn(), &run.run_id).unwrap();
        let about = Some((run.run_id.as_str(), "w-1"));
        record_gate(&ctx, about, &GateRecord::refused(&refusal()).unwrap());
        record_gate(
            &ctx,
            about,
            &GateRecord::spoken(&provenance("opus", "codex-sol", None)),
        );
        assert_eq!(gate_events(&store, &run).len(), 2);
        assert_eq!(
            tasks::plan_workers(store.conn(), &run.run_id).unwrap(),
            before
        );
        // The gate itself takes no ledger: its inputs are the roster and
        // the quota view, so a record cannot reach a decision.
        let src = include_str!("../../../horch-core/src/routing/decision.rs");
        assert!(
            !src.contains("events"),
            "the gate must not read ledger events"
        );
    }

    #[test]
    fn trf_41_a_ledger_that_cannot_be_written_does_not_fail_the_spawn() {
        let dir = tempfile::tempdir().unwrap();
        let bad = dir.path().join("no-dir").join("x").join("ledger.sqlite3");
        std::fs::write(dir.path().join("no-dir"), "a file, not a directory").unwrap();
        let ctx = RuntimeContext::from_env(
            &MapEnv::new("/")
                .with(
                    "HORCH_STATE_DIR",
                    dir.path().join("state").to_str().unwrap(),
                )
                .with("HORCH_LEDGER_DB", bad.to_str().unwrap()),
        )
        .unwrap();
        record_gate(&ctx, None, &GateRecord::refused(&refusal()).unwrap());
    }
}
