//! `horch fleet` and `horch orchestration` - the port of the justfile recipes and
//! the per-pane launcher scripts they ran.
//!
//! Both build a herdr workspace, lay out panes, and start an agent in each. The
//! difference is lifecycle: `orchestration` is a fixed 5-pane workspace, while
//! `fleet` starts one orchestrator alone, which then grows and shrinks the fleet
//! itself through the session ledger.

use std::path::{Component, Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result};
use horch::project::project_facts;
use horch_core::execution::legacy::{Record, KIND_ORCHESTRATOR};
use horch_core::execution::records::{SessionLedger, ORCHESTRATING_TASK};
use horch_core::execution::{ExecutionStatus, LaunchStage, SessionMode};
use horch_core::harness::launch::{self, DiscoveryTarget, LaunchRequest};
use horch_core::harness::HarnessKind;
use horch_core::ids::SessionId;
use horch_core::messaging::mailbox::Mailbox;
use horch_core::prompts;
use horch_core::roster::{ExecRule, Roster, Teammate};
use horch_core::routing::quota::{self, QuotaView, State};
use horch_core::runtime::RuntimeContext;
use horch_core::skills::{harness_support, SkillResolution};
use horch_core::taskledger::model::{VerifyProfile, VERIFY_ENV};
use horch_core::taskledger::orchestrator::{self, Respawn};
use horch_core::workspace::herdr::Herdr;
use horch_core::workspace::model::{Direction, NewWorkspace};
use horch_core::workspace::paneshell::PaneShell;

use super::doctor;

/// What a launched pane should become.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneKind {
    FleetOrchestrator,
    FleetCodexOrchestrator,
    OrchestrationOrchestrator,
    OrchestrationClaude,
    OrchestrationCodex,
}

impl PaneKind {
    pub fn as_str(self) -> &'static str {
        match self {
            PaneKind::FleetOrchestrator => "fleet-orchestrator",
            PaneKind::FleetCodexOrchestrator => "fleet-codex-orchestrator",
            PaneKind::OrchestrationOrchestrator => "orchestration-orchestrator",
            PaneKind::OrchestrationClaude => "orchestration-claude",
            PaneKind::OrchestrationCodex => "orchestration-codex",
        }
    }

    /// A fleet orchestrator, as opposed to a pane of the fixed 5-pane recipe.
    fn is_fleet(self) -> bool {
        matches!(
            self,
            PaneKind::FleetOrchestrator | PaneKind::FleetCodexOrchestrator
        )
    }

    /// Which teammate file backs a pane of this kind. The model, effort level
    /// and permission mode all come from that file: a pane kind selects a
    /// file, it does not carry launch settings of its own.
    fn teammate_file(self) -> &'static str {
        match self {
            PaneKind::FleetOrchestrator => "orchestrator",
            PaneKind::FleetCodexOrchestrator => "orchestrator-codex",
            PaneKind::OrchestrationOrchestrator => "orchestration-orchestrator",
            PaneKind::OrchestrationClaude | PaneKind::OrchestrationCodex => "orchestration-worker",
        }
    }
}

impl std::str::FromStr for PaneKind {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "fleet-orchestrator" => Ok(PaneKind::FleetOrchestrator),
            "fleet-codex-orchestrator" => Ok(PaneKind::FleetCodexOrchestrator),
            "orchestration-orchestrator" => Ok(PaneKind::OrchestrationOrchestrator),
            "orchestration-claude" => Ok(PaneKind::OrchestrationClaude),
            "orchestration-codex" => Ok(PaneKind::OrchestrationCodex),
            other => Err(format!("unknown pane kind '{other}'")),
        }
    }
}

/// Which agent, on which model, orchestrates a fleet.
///
/// Only the ORCHESTRATOR pane differs. The roster it spawns workers from is the
/// same either way, so a Codex orchestrator still reaches for `opus` when a task
/// wants Claude, and a Claude one still reaches for `codex-sol`.
///
/// Two teammate files back four flavors: the flavor picks the file (Claude or
/// Codex) and passes its model as the pane's `--model`, which wins over the
/// file's own `model:`. Opus is the default because it is the cheapest model
/// that orchestrates well (cezaar#40 measured Opus 5.5 at $4/$20 against
/// Fable's $10/$50); Fable and Astra stay one word away for work that needs
/// the top tier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FleetFlavor {
    /// Claude Code, on the latest Opus.
    #[default]
    Opus,
    /// Claude Code, on Fable.
    Fable,
    /// Codex, on Astra.
    Astra,
    /// Codex, on Sol.
    Sol,
    /// Whichever of Opus and Sol the usage pools can serve (BAL-08). Resolved
    /// to one of the others before anything launches.
    Auto,
}

impl FleetFlavor {
    pub fn as_str(self) -> &'static str {
        match self {
            FleetFlavor::Opus => "opus",
            FleetFlavor::Fable => "fable",
            FleetFlavor::Astra => "astra",
            FleetFlavor::Sol => "sol",
            FleetFlavor::Auto => "auto",
        }
    }

    /// What to print while the pane comes up.
    fn label(self) -> &'static str {
        match self {
            FleetFlavor::Opus => "Claude Code on Opus",
            FleetFlavor::Fable => "Claude Code on Fable",
            FleetFlavor::Astra => "Codex on Astra",
            FleetFlavor::Sol => "Codex on Sol",
            FleetFlavor::Auto => "chosen by usage limits",
        }
    }

    /// The model the orchestrator pane runs on. A Claude alias tracks the
    /// latest release; codex has no alias mechanism, so its slugs are literal.
    pub fn model(self) -> &'static str {
        match self {
            // `Auto` never launches as itself; `fleet` resolves it first.
            FleetFlavor::Opus | FleetFlavor::Auto => "opus",
            FleetFlavor::Fable => "fable",
            FleetFlavor::Astra => "gpt-6-astra",
            FleetFlavor::Sol => "gpt-5.6-sol",
        }
    }

    /// The teammate file this flavor's orchestrator pane is briefed from.
    fn pane_kind(self) -> PaneKind {
        match self {
            FleetFlavor::Opus | FleetFlavor::Fable | FleetFlavor::Auto => {
                PaneKind::FleetOrchestrator
            }
            FleetFlavor::Astra | FleetFlavor::Sol => PaneKind::FleetCodexOrchestrator,
        }
    }
}

impl std::str::FromStr for FleetFlavor {
    type Err = String;

    /// The model names are how the choice is actually discussed ("the Fable
    /// one"). `cc`/`claude` mean the default Claude flavor, Opus; `codex` keeps
    /// meaning Astra, which is what it meant before Sol was offered.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "opus" | "cc" | "claude" => Ok(FleetFlavor::Opus),
            "fable" => Ok(FleetFlavor::Fable),
            "astra" | "codex" => Ok(FleetFlavor::Astra),
            "sol" => Ok(FleetFlavor::Sol),
            "auto" => Ok(FleetFlavor::Auto),
            other => Err(format!(
                "unknown fleet flavor '{other}' (expected opus, fable, astra, sol or auto; \
                 cc/claude mean opus, codex means astra)"
            )),
        }
    }
}

impl std::fmt::Display for FleetFlavor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Resolve the project directory a recipe should root its workspace at.
fn resolve_cwd(ctx: &RuntimeContext, cwd: Option<&str>) -> Result<String> {
    let dir = match cwd {
        Some(c) => PathBuf::from(c),
        None => ctx.paths.current_dir()?,
    };
    Ok(dir.to_string_lossy().into_owned())
}

/// The command line that turns a pane into `kind`.
fn pane_command(
    ctx: &RuntimeContext,
    role: &str,
    kind: PaneKind,
    model: Option<&str>,
) -> Result<String> {
    pane_command_for(ctx, role, kind, model, None, None)
}

/// The ledger identity an orchestrator pane runs under.
struct PaneRecord<'a> {
    record_id: &'a str,
    session_id: Option<&'a str>,
    /// Resume `session_id` rather than start it fresh.
    resume: bool,
    /// The [`orchestrator::RESPAWN_ENV`] value of a respawned orchestrator.
    respawn: Option<String>,
}

/// [`pane_command`], carrying a ledger record for the pane.
fn pane_command_for(
    ctx: &RuntimeContext,
    role: &str,
    kind: PaneKind,
    model: Option<&str>,
    record: Option<PaneRecord<'_>>,
    verify: Option<VerifyProfile>,
) -> Result<String> {
    let exe = ctx.bins.exe()?;
    let mut args = vec![
        "pane-launch".to_string(),
        "--role".to_string(),
        role.to_string(),
        "--kind".to_string(),
        kind.as_str().to_string(),
    ];
    if let Some(model) = model {
        args.push("--model".to_string());
        args.push(model.to_string());
    }
    // A pane does not inherit this process's environment, so the roster path
    // has to be written into the command line the herdr server will run.
    if let Some(dir) = super::path_text(ctx.bins.roster_override.as_deref()) {
        args.push("--teammates-dir".to_string());
        args.push(dir);
    }
    let respawn = record.as_ref().and_then(|r| r.respawn.clone());
    if let Some(record) = record {
        args.push("--record-id".to_string());
        args.push(record.record_id.to_string());
        if let Some(sid) = record.session_id {
            args.push("--session-id".to_string());
            args.push(sid.to_string());
            if record.resume {
                args.push("--resume".to_string());
            }
        }
        // The same reason as the roster path: the ledger this record lives
        // in must be the one the pane writes.
        if let Some(dir) = super::path_text(ctx.paths.state_override.as_deref()) {
            args.push("--state-dir".to_string());
            args.push(dir);
        }
    }
    // The skill store too: `pane-launch`, its agent and every `horch spawn`
    // that agent runs read `HORCH_DATA_DIR`, so they all use this store.
    let data_root = ctx.paths.data_root.to_string_lossy();
    let mut env: Vec<(&str, &str)> = vec![("HORCH_DATA_DIR", data_root.as_ref())];
    // The mailbox lives in the state folder, so the pane needs the same one,
    // whether `HORCH_STATE_DIR` or `XDG_STATE_HOME` picked it.
    let state_root = ctx.paths.state_root.to_string_lossy();
    env.push(("HORCH_STATE_DIR", state_root.as_ref()));
    // An explicit task ledger too (#199): the pane's agent and every
    // `horch spawn` it runs then use the ledger this command used.
    let ledger = super::path_text(ctx.paths.ledger_override.as_deref());
    if let Some(ledger) = ledger.as_deref() {
        env.push(("HORCH_LEDGER_DB", ledger));
    }
    // The verification profile: the briefing names it, and the
    // orchestrator's `horch ledger run new` takes its steps from it.
    if let Some(verify) = verify {
        env.push((VERIFY_ENV, verify.as_str()));
    }
    // A respawned orchestrator's briefing ends with its first instruction (C7).
    if let Some(respawn) = respawn.as_deref() {
        env.push((orchestrator::RESPAWN_ENV, respawn));
    }
    Ok(PaneShell::host().command_line_with_env(&exe, &env, &args))
}

/// Options of `horch fleet` beyond the flavor.
#[derive(Debug, Clone, Default)]
pub struct FleetOptions {
    /// `--no-update`: skip the agent CLI update step (ARC-34).
    pub no_update: bool,
    /// Some: the orchestrator resumes this session instead of starting fresh.
    pub resume: Option<String>,
    /// `--verify`: the run's finish steps. None is [`VerifyProfile::DEFAULT`].
    pub verify: Option<VerifyProfile>,
    /// Some: start a respawned orchestrator in the run's existing workspace
    /// (trace-fixes C7), not a new fleet.
    pub respawn: Option<Respawn>,
}

impl FleetOptions {
    /// The verification profile this fleet starts with.
    pub fn verify(&self) -> VerifyProfile {
        self.verify.unwrap_or(VerifyProfile::DEFAULT)
    }
}

/// The briefing section that tells a fleet orchestrator its verification
/// profile, so it neither overrides it nor expects steps the run skips.
fn verify_briefing(profile: VerifyProfile) -> String {
    let steps: Vec<&str> = profile.steps().iter().map(|s| s.as_str()).collect();
    format!(
        "== Verification ==\n\
         The operator started this fleet with verification profile `{profile}`: \
         {summary}. `horch ledger run new` takes the run's finish steps from it \
         ({steps}); do not pass --steps or --verify unless the operator asks. \
         A task is finished when `horch ledger finish` passed every step of its \
         run. An existing run keeps the steps it was created with.",
        summary = profile.summary(),
        steps = steps.join(", "),
    )
}

/// The 1 startup line that names the profile, with how to change it.
fn verify_line(profile: VerifyProfile) -> String {
    format!(
        "Verification: {profile} ({}). Change with --verify full|light|minimal or HORCH_VERIFY.",
        profile.summary()
    )
}

/// Active runs of the project whose steps are not `profile`'s: the
/// orchestrator may go on with one, and it keeps its own steps.
fn warn_about_other_run_steps(ctx: &RuntimeContext, cwd: &str, profile: VerifyProfile) {
    // Only a ledger that exists: a fleet start creates none.
    if !ctx.paths.ledger_db.is_file() {
        return;
    }
    let Ok(store) = horch_core::taskledger::Store::open(&ctx.paths.ledger_db) else {
        return;
    };
    let Ok(all) = horch_core::taskledger::runs::of_project(store.conn(), Path::new(cwd)) else {
        return;
    };
    let wanted = profile.steps();
    let active = horch_core::taskledger::model::RunStatus::Active.as_str();
    for r in all.iter().filter(|r| r.status == active) {
        let Ok(steps) = r.steps() else { continue };
        if steps != wanted {
            let theirs = VerifyProfile::of_steps(&steps)
                .map(|p| p.to_string())
                .unwrap_or_else(|| r.finish_steps.join(","));
            println!(
                "note: run {} is still active with verify {theirs}; a new run uses {profile}, \
                 an orchestrator that continues {} keeps {theirs}.",
                r.slug, r.slug
            );
        }
    }
}

/// Launch the herdr-fleet workspace.
///
/// One orchestrator pane, alone. It reads the roster, breaks the work down, and
/// spawns exactly the workers it needs with `horch spawn`; workers record
/// progress with `horch note` and shut their own pane down with `horch done`
/// when truly finished.
///
/// Earlier versions pre-spawned a fixed 2x2 grid of idle workers. That decided
/// the shape of the team before anyone knew what the work was, and burned four
/// agent sessions holding a greeting.
///
/// `options.resume` continues an
/// earlier orchestrator session instead of starting a new one. Unless
/// `options.no_update`, every agent CLI updates first (ARC-32).
pub fn fleet_with(
    ctx: &RuntimeContext,
    cwd: Option<&str>,
    flavor: FleetFlavor,
    options: &FleetOptions,
) -> Result<()> {
    println!("horch {}", super::selfupdate::CURRENT);
    println!("{}", verify_line(options.verify()));
    super::update::before_fleet(ctx, options.no_update);
    doctor::check(ctx)?;
    let herdr = Herdr::with_bin(&ctx.bins.harness.herdr);
    let cwd = resolve_cwd(ctx, cwd)?;
    let mut warning_ctx = ctx.clone();
    warning_ctx.paths.project_dir = Some(PathBuf::from(&cwd));
    super::spawn::fleet_warnings(&warning_ctx, false);

    warn_about_missing_integrations(&herdr);
    super::restore::warn_if_resume_on(ctx);

    let flavor = choose_flavor(ctx, flavor)?;
    warn_about_other_run_steps(ctx, &cwd, options.verify());

    // Every step that can fail without herdr runs before the workspace
    // exists, so such a failure leaves nothing to close (#217).
    let mut ctx = ctx.clone();
    ctx.paths.project_dir = Some(PathBuf::from(&cwd));

    // The orchestrator is a ledger record like any worker (TEL-02), so its
    // spend is counted. Orchestrators of workspaces that are gone are
    // retired first: an orchestrator never runs `horch done`.
    let ledger = open_ledger(&ctx, &herdr)?;
    horch_core::sweep::run_due(&ctx);
    let start = orchestrator_start(&ctx, &cwd, flavor, options)?;
    let record_id = start.record.record_id.clone();

    if let Some(respawn) = &options.respawn {
        // The dead orchestrator's record of this workspace ends first.
        ledger.supersede_orchestrators(|ws| ws != respawn.workspace_id)?;
        let ws = respawn_pane(&herdr, respawn)?;
        if let Err(e) = start_orchestrator(&mut ctx, &herdr, &ws, &ledger, start) {
            // The workspace holds the run's workers: it stays open.
            abandon_record(&ledger, &record_id, &e);
            return Err(e);
        }
        println!(
            "Respawned the orchestrator of run {} in workspace {}, pane {} (respawn {} of {}).",
            respawn.slug,
            ws.workspace_id,
            ws.root_pane_id,
            respawn.number,
            orchestrator::RESPAWNS_PER_RUN
        );
        return Ok(());
    }

    println!("Creating fleet workspace rooted at {cwd}...");
    let label = workspace_label(Path::new(&cwd), ctx.paths.cwd.as_deref());
    let ws = herdr.workspace_create(&label, Some(&cwd), true)?;
    // From here a failure closes the workspace and ends the record (#217).
    if let Err(e) = start_orchestrator(&mut ctx, &herdr, &ws, &ledger, start) {
        abandon_start(&herdr, &ws.workspace_id, Some((&ledger, &record_id)), &e);
        return Err(e);
    }
    let ctx = &ctx;

    super::delivery_probe::fleet(ctx, &ws.workspace_id, &ws.root_pane_id);

    // Parked worktrees get a steward pane beside the orchestrator (#308).
    start_worktree_steward(ctx, &ws);

    // The telemetry space (SPC-06). Never a reason for the fleet to fail.
    if let Err(e) = super::telemetry::ensure(ctx, &herdr, true) {
        println!("warning: telemetry space not started: {e:#}");
    }

    print_fleet_ready(&ws.workspace_id, &ledger);
    Ok(())
}

/// When the project has parked worktrees (`horch worktree triage`), spawn
/// the `worktree-steward` teammate beside the orchestrator: it runs the
/// triage with the operator. With none, nothing is spawned. Never a reason
/// for the fleet to fail.
fn start_worktree_steward(ctx: &RuntimeContext, ws: &NewWorkspace) {
    let rows = match super::worktree::triage_rows(ctx, horch_core::taskledger::triage::Measure::Ids)
    {
        Ok(rows) => rows,
        Err(e) => {
            println!("warning: the worktree triage did not run: {e:#}");
            return;
        }
    };
    if rows.is_empty() {
        return;
    }
    println!(
        "{} parked worktrees: starting the worktree steward...",
        rows.len()
    );
    let args = super::spawn::SpawnArgs {
        teammate: Some("worktree-steward".to_string()),
        // A fact, not a brief: the steward's file holds its instructions.
        task: format!("parked worktrees: {}", rows.len()),
        resume: None,
        phase: None,
        effort: None,
        role: None,
        from_pane: Some(ws.root_pane_id.clone()),
        direction: Direction::Right,
        tiling: horch_core::execution::TilingMode::Automatic,
        exact: false,
        force: false,
        replaces: None,
        run: None,
    };
    if let Err(e) = super::spawn::spawn(ctx, args) {
        println!("warning: the worktree steward did not start: {e:#}");
    }
}

/// `FleetFlavor::Auto` becomes the flavor whose pool can serve it; any other
/// flavor is kept.
fn choose_flavor(ctx: &RuntimeContext, flavor: FleetFlavor) -> Result<FleetFlavor> {
    if flavor != FleetFlavor::Auto {
        return Ok(flavor);
    }
    let state_root = ctx.paths.state_root.clone();
    let policy = super::quotacmd::load_policy(ctx, &state_root)?;
    let view = horch_core::routing::snapshot::obtain(
        &state_root,
        horch_core::clock::now(),
        &policy,
        true,
        &super::quotacmd::quota_env(ctx),
    )?;
    let (chosen, why) = auto_flavor(&view);
    println!("fleet: auto chose {chosen} - {why}.");
    Ok(chosen)
}

/// The session ledger. Orchestrators of workspaces that are gone are retired
/// first: an orchestrator never runs `horch done`. Without a herdr workspace
/// list, none is.
fn open_ledger(ctx: &RuntimeContext, herdr: &Herdr) -> Result<SessionLedger> {
    let ledger = SessionLedger::open_in(ctx)?;
    if let Ok(open) = herdr.workspace_list() {
        let open: Vec<String> = open.into_iter().map(|w| w.workspace_id).collect();
        ledger.supersede_orchestrators(|ws| open.iter().any(|o| o == ws))?;
    }
    Ok(ledger)
}

/// The closing text of `horch fleet`.
fn print_fleet_ready(workspace_id: &str, ledger: &SessionLedger) {
    println!("\nDone. Fleet workspace {workspace_id} is live with one orchestrator pane.");
    println!("Session ledger: {}", ledger.path().display());
    println!("Orchestrator commands: horch sessions | horch assign | horch spawn [--resume]");
    println!("Workers are spawned on demand: horch spawn <teammate> \"task\"");
}

/// What `fleet_with` resolved before its workspace exists: the orchestrator
/// record without its workspace, and how to launch its pane.
struct OrchestratorStart<'a> {
    record: Record,
    kind: PaneKind,
    flavor: FleetFlavor,
    options: &'a FleetOptions,
}

/// Resolve the orchestrator of a fleet in `cwd` before its workspace
/// exists: its teammate, session id, skills and ledger record. Each step
/// can fail, and none of them touches herdr (#217).
fn orchestrator_start<'a>(
    ctx: &RuntimeContext,
    cwd: &str,
    flavor: FleetFlavor,
    options: &'a FleetOptions,
) -> Result<OrchestratorStart<'a>> {
    let kind = flavor.pane_kind();
    // The project's facts add its `skills_when` skills, here and at the
    // pane launch, so the record and the launch agree.
    let roster = super::load_roster(ctx, None)?.with_project_facts(project_facts(Path::new(cwd)));
    let teammate = &roster.for_launch(roster.require(kind.teammate_file())?.clone());
    // A resumed session's id is known up front for every agent, so the new
    // record carries it and no discovery is needed.
    let session_id = match &options.resume {
        Some(id) => Some(SessionId::new(id.clone())?.as_str().to_string()),
        None => teammate
            .agent
            .capabilities()
            .caller_minted_session
            .then(horch_core::mint_uuid),
    };
    // The record lists the skills the launch will activate, as a worker's
    // does (SKL-04): the launch fails on a record whose skills differ.
    let home = ctx.inherited.home_var.as_deref().map(Path::new);
    let skills = SkillResolution::read(
        roster.skill_catalog()?,
        Some(teammate),
        home,
        harness_support,
    )?
    .plan(teammate)?;
    Ok(OrchestratorStart {
        record: Record {
            record_id: horch_core::mint_uuid(),
            session_id,
            agent: teammate.agent.as_str().to_string(),
            tier: teammate.name.clone(),
            model: flavor.model().to_string(),
            effort: teammate.effort.clone(),
            phase: teammate.phase,
            role: "orchestrator".to_string(),
            kind: KIND_ORCHESTRATOR.to_string(),
            task: ORCHESTRATING_TASK.to_string(),
            project: Some(cwd.to_string()),
            skills: skills.activated,
            ..Record::default()
        },
        kind,
        flavor,
        options,
    })
}

/// Insert the orchestrator record of the new workspace `ws` and run its
/// pane. `ctx` then works in `ws`.
fn start_orchestrator(
    ctx: &mut RuntimeContext,
    herdr: &Herdr,
    ws: &NewWorkspace,
    ledger: &SessionLedger,
    start: OrchestratorStart<'_>,
) -> Result<()> {
    ctx.herdr.workspace = Some(horch_core::ids::WorkspaceId::new(ws.workspace_id.as_str())?);
    let OrchestratorStart {
        mut record,
        kind,
        flavor,
        options,
    } = start;
    record.workspace_id = Some(ws.workspace_id.clone());
    let (record_id, session_id) = (record.record_id.clone(), record.session_id.clone());
    ledger.insert(record)?;

    match &options.resume {
        Some(id) => println!(
            "Resuming orchestrator session {id} ({}). No briefing is sent; \
             type /orchestrator ($orchestrator in Codex) to re-brief it.",
            flavor.label()
        ),
        None => println!("Launching orchestrator ({})...", flavor.label()),
    }
    herdr.pane_run(
        &ws.root_pane_id,
        &pane_command_for(
            ctx,
            "orchestrator",
            kind,
            Some(flavor.model()),
            Some(PaneRecord {
                record_id: &record_id,
                session_id: session_id.as_deref(),
                resume: options.resume.is_some(),
                respawn: options.respawn.as_ref().map(Respawn::env_value),
            }),
            Some(options.verify()),
        )?,
    )
}

/// Undo a start that failed after its workspace was created (#217): end
/// the orchestrator `record` (ledger and key) as `LaunchFailed` when the
/// start inserted it, so no live record names the workspace, then close
/// the workspace. A cleanup error is printed; `error`, the start's own, is
/// the one the caller returns. The stage `Run` covers the pane command
/// and the pane run.
fn abandon_start(
    herdr: &Herdr,
    workspace_id: &str,
    record: Option<(&SessionLedger, &str)>,
    error: &anyhow::Error,
) {
    if let Some((ledger, key)) = record {
        abandon_record(ledger, key, error);
    }
    if let Err(e) = herdr.workspace_close(workspace_id) {
        eprintln!("warning: closing workspace {workspace_id} failed: {e:#}");
    }
}

/// End the orchestrator record `key` as `LaunchFailed`, when the start
/// inserted it.
fn abandon_record(ledger: &SessionLedger, key: &str, error: &anyhow::Error) {
    if ledger.get(key).is_err() {
        return;
    }
    let state = ExecutionStatus::LaunchFailed {
        stage: LaunchStage::Run,
        reason: format!("{error:#}"),
    };
    if let Err(e) = ledger.store().end_live(key, state) {
        eprintln!("warning: ending the orchestrator record {key} failed: {e:#}");
    }
}

/// The pane a respawned orchestrator runs in: the dead orchestrator's pane
/// when herdr still has it, else a new pane beside the first pane of the
/// workspace. The workspace must still exist.
fn respawn_pane(herdr: &Herdr, respawn: &Respawn) -> Result<NewWorkspace> {
    let ws = &respawn.workspace_id;
    let pane = match respawn
        .old_pane_id
        .as_deref()
        .filter(|p| herdr.pane_exists(p))
    {
        Some(p) => p.to_string(),
        None => {
            let panes = herdr.pane_list(ws)?;
            let first = panes
                .first()
                .with_context(|| format!("workspace {ws} has no pane to split"))?;
            herdr.pane_split(&first.pane_id, Direction::Right)?
        }
    };
    Ok(NewWorkspace {
        workspace_id: ws.clone(),
        root_pane_id: pane,
        tab_id: None,
    })
}

/// Start the orchestrator that `respawn` decided, through [`fleet_with`]:
/// the flavor and the verification profile of the dead one, no updates.
pub fn respawn_orchestrator(ctx: &RuntimeContext, respawn: &Respawn) -> Result<()> {
    let mut project = ctx.clone();
    project.paths.project_dir = Some(PathBuf::from(&respawn.project_root));
    let flavor = SessionLedger::open_in(&project)
        .ok()
        .and_then(|l| {
            l.live_for_role("orchestrator", Some(&respawn.workspace_id))
                .ok()
                .flatten()
        })
        .map(|r| flavor_of_model(&r.model))
        .unwrap_or_default();
    let steps: Vec<_> = respawn
        .finish_steps
        .iter()
        .filter_map(|s| horch_core::taskledger::model::StepName::parse(s).ok())
        .collect();
    let options = FleetOptions {
        no_update: true,
        resume: None,
        verify: VerifyProfile::of_steps(&steps),
        respawn: Some(respawn.clone()),
    };
    fleet_with(ctx, Some(&respawn.project_root), flavor, &options)
}

/// The flavor whose orchestrator runs `model`; Opus, the default, for a
/// model no flavor names.
fn flavor_of_model(model: &str) -> FleetFlavor {
    [
        FleetFlavor::Opus,
        FleetFlavor::Fable,
        FleetFlavor::Astra,
        FleetFlavor::Sol,
    ]
    .into_iter()
    .find(|f| f.model() == model)
    .unwrap_or_default()
}

/// `horch fleet stop`: record a deliberate stop of the orchestrator of each
/// active run of the project, so the watchdog never respawns it (C7). It
/// does not end the agent.
pub fn fleet_stop(ctx: &RuntimeContext, cwd: Option<&str>) -> Result<ExitCode> {
    let root = resolve_cwd(ctx, cwd)?;
    let Some(mut store) = horch_core::taskledger::open_existing(ctx)? else {
        println!("No task ledger. Nothing to stop.");
        return Ok(ExitCode::SUCCESS);
    };
    let active = horch_core::taskledger::model::RunStatus::Active.as_str();
    let runs: Vec<_> = horch_core::taskledger::runs::of_project(store.conn(), Path::new(&root))?
        .into_iter()
        .filter(|r| r.status == active)
        .collect();
    if runs.is_empty() {
        println!("No active run in {root}. Nothing to stop.");
    }
    for run in runs {
        let recorded = store.immediate(|tx| orchestrator::record_stop(tx, &run.run_id))?;
        if recorded {
            println!(
                "Run {}: the orchestrator stop is recorded. horch does not respawn it.",
                run.slug
            );
        } else {
            println!(
                "Run {}: the orchestrator stop was recorded before.",
                run.slug
            );
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// `claude 7d 100% (resets Fri 14:00Z)`: a pool in a few words.
fn pool_words(view: &QuotaView, pool: &str, scope: &str) -> String {
    let a = view.assess_pool(pool, Some(scope), None);
    match &a.worst {
        Some(w) => {
            let mut s = format!("{pool} {} {}", w.label(), quota::pct(w.used_at(view.now)));
            if a.state.blocks() {
                if let Some(r) = &w.resets_at {
                    s.push_str(&format!(" (resets {})", quota::short_time(r)));
                }
            }
            s
        }
        None => format!("{pool} {}", a.state),
    }
}

/// `horch fleet auto` (BAL-08): Opus if the claude pool can serve it, else
/// Sol if the codex pool can, else whichever pool resets first.
fn auto_flavor(view: &QuotaView) -> (FleetFlavor, String) {
    let claude = view.assess_pool(quota::POOL_CLAUDE, Some("opus"), None);
    let codex = view.assess_pool(quota::POOL_CODEX, Some("sol"), None);
    let why = format!(
        "{}, {}",
        pool_words(view, quota::POOL_CLAUDE, "opus"),
        pool_words(view, quota::POOL_CODEX, "sol")
    );
    let serves = |s: State| matches!(s, State::Ok | State::Tight);
    let chosen = if serves(claude.state) {
        FleetFlavor::Opus
    } else if serves(codex.state) {
        FleetFlavor::Sol
    } else {
        let reset = |a: &quota::Assessment| {
            a.worst
                .as_ref()
                .and_then(|w| w.resets_at.clone())
                .unwrap_or_else(|| "9999".into())
        };
        if reset(&codex) < reset(&claude) {
            FleetFlavor::Sol
        } else {
            FleetFlavor::Opus
        }
    };
    (chosen, why)
}

/// The fleet workspace is named after the project folder; a path with no final
/// component (such as `/`) keeps the old fixed label. The path is resolved
/// first, because `.` and `..` have no final component of their own.
/// `current` is the process's current directory, which a relative path is
/// resolved against.
fn workspace_label(cwd: &Path, current: Option<&Path>) -> String {
    resolved_path(cwd, current)
        .as_deref()
        .and_then(Path::file_name)
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "Herdr Fleet".to_string())
}

/// `path` as an absolute path with no `.` or `..` in it: `canonicalize` when the
/// path exists, otherwise the current directory joined with it and normalised by
/// hand. An empty path resolves to nothing rather than to the current directory.
fn resolved_path(path: &Path, current: Option<&Path>) -> Option<PathBuf> {
    if path.as_os_str().is_empty() {
        None
    } else {
        std::fs::canonicalize(path)
            .ok()
            .or_else(|| current.map(|current| normalised(current, path)))
    }
}

/// `current` joined with `path`, with each `.` and `..` resolved by hand.
fn normalised(current: &Path, path: &Path) -> PathBuf {
    let mut resolved = PathBuf::new();
    for component in current.join(path).components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                resolved.pop();
            }
            other => resolved.push(other.as_os_str()),
        }
    }
    resolved
}

/// Native herdr agent-status reporting is optional but nice: it lets the
/// orchestrator watch workers with `herdr agent list` and hands codex session ids
/// to the ledger without the rollout-file fallback.
fn warn_about_missing_integrations(herdr: &Herdr) {
    let Some(status) = herdr.integration_status() else {
        return;
    };
    let missing = status
        .lines()
        .filter(|l| l.starts_with("claude:") || l.starts_with("codex:"))
        .any(|l| l.contains("not installed"));
    if missing {
        println!("note: herdr claude/codex integrations are not installed; the fleet still works,");
        println!("      but 'herdr integration install claude codex' improves live status + codex");
        println!("      session-id capture.");
    }
}

/// Launch the fixed 5-pane orchestration workspace.
///
/// 1 orchestrator (Claude, Fable) + 4 workers (2x Claude Sonnet medium, 1x Claude
/// Opus medium, 1x Codex CLI), laid out as orchestrator-left with a 2x2 worker grid
/// on the right, wired for two-way messaging via `horch tell`.
///
/// Unless `no_update`, every agent CLI updates first (ARC-32).
pub fn orchestration(ctx: &RuntimeContext, cwd: Option<&str>, no_update: bool) -> Result<()> {
    super::update::before_fleet(ctx, no_update);
    doctor::check(ctx)?;
    let herdr = Herdr::with_bin(&ctx.bins.harness.herdr);
    let cwd = resolve_cwd(ctx, cwd)?;

    println!("Creating workspace rooted at {cwd}...");
    let ws = herdr.workspace_create("Herdr Orchestration", Some(&cwd), true)?;

    // A failed split or pane run closes the workspace again (#217).
    if let Err(e) = lay_out_orchestration(ctx, &herdr, &ws) {
        abandon_start(&herdr, &ws.workspace_id, None, &e);
        return Err(e);
    }

    println!(
        "\nDone. Workspace {} is live with 5 panes.",
        ws.workspace_id
    );
    println!(
        "From the orchestrator pane: horch tell <role> \"message\"  \
         (roles: sonnet-1, sonnet-2, opus-1, codex-1)"
    );
    println!("From any worker pane:       horch tell orchestrator \"message\"");
    Ok(())
}

/// The 5 panes of `horch orchestration` in the new workspace `ws`: the
/// layout, then each pane's agent.
fn lay_out_orchestration(ctx: &RuntimeContext, herdr: &Herdr, ws: &NewWorkspace) -> Result<()> {
    println!("Building layout: orchestrator | 2x2 worker grid...");
    let top_left = herdr.pane_split(&ws.root_pane_id, Direction::Right)?;
    let bottom_left = herdr.pane_split(&top_left, Direction::Down)?;
    let top_right = herdr.pane_split(&top_left, Direction::Right)?;
    let bottom_right = herdr.pane_split(&bottom_left, Direction::Right)?;

    // (pane, role, kind, model)
    let panes = [
        (
            &ws.root_pane_id,
            "orchestrator",
            PaneKind::OrchestrationOrchestrator,
            None,
        ),
        (
            &top_left,
            "sonnet-1",
            PaneKind::OrchestrationClaude,
            Some("sonnet"),
        ),
        (
            &top_right,
            "sonnet-2",
            PaneKind::OrchestrationClaude,
            Some("sonnet"),
        ),
        (
            &bottom_left,
            "opus-1",
            PaneKind::OrchestrationClaude,
            Some("opus"),
        ),
        (&bottom_right, "codex-1", PaneKind::OrchestrationCodex, None),
    ];
    for (pane, role, kind, model) in panes {
        println!("Launching {role}...");
        herdr.pane_run(pane, &pane_command(ctx, role, kind, model)?)?;
    }
    Ok(())
}

/// `horch pane-launch` - register this pane under a role and start its agent.
///
/// Replaces `scripts/herdr-fleet/orchestrator.sh`, `scripts/lib/herdr-worker.sh`,
/// and the five `scripts/herdr-orchestration/*.sh` launchers.
/// The ledger identity `horch fleet` gave an orchestrator pane.
#[derive(Debug, Clone, Default)]
pub struct PaneIdentity {
    pub record_id: Option<String>,
    pub session_id: Option<String>,
    /// Resume `session_id` rather than start it fresh (`horch fleet --resume`).
    pub resume: bool,
    pub state_dir: Option<String>,
}

pub fn pane_launch(
    ctx: &mut RuntimeContext,
    role: &str,
    kind: PaneKind,
    model: Option<&str>,
    teammates_dir: Option<&str>,
    identity: PaneIdentity,
) -> Result<ExitCode> {
    require_model(kind, model)?;
    let mailbox = register_pane(ctx, role, &identity)?;
    // Held until the agent exits, then released on drop.
    let _orchestrator_lock = kind
        .is_fleet()
        .then(|| hold_orchestrator_lock(ctx, mailbox.workspace_id()))
        .transpose()?;
    let child_env = child_env(ctx, &mailbox, &identity, teammates_dir);
    let ctx: &RuntimeContext = ctx;
    let roster = launch_roster(ctx, kind, teammates_dir)?;
    let teammate = launch_teammate(&roster, kind, model)?;
    let prompt = briefing_for(kind, &roster, &teammate, role)?;
    let exec_rules = exec_rules_for(kind, &roster);
    let (session, workdir) = session_for(&identity, ctx)?;
    // A fleet orchestrator always gets its briefing as `/orchestrator`. A
    // resumed one already has it in context, so it is not sent again.
    let first_message = if identity.resume { "" } else { prompt.as_str() };
    launch::run_flow(
        ctx,
        LaunchRequest {
            role,
            teammate: &teammate,
            session: &session,
            prompt: first_message,
            model_override: model,
            exec_rules,
            child_env,
            record: identity.record_id.as_deref().zip(workdir.as_deref()).map(
                |(record_id, workdir)| DiscoveryTarget {
                    mailbox: &mailbox,
                    record_id,
                    workdir,
                },
            ),
            fleet_window: roster.fleet_window(
                &teammate,
                model.or(teammate.model.as_deref()).unwrap_or_default(),
            ),
            briefing_skill: kind.is_fleet().then_some(prompt.as_str()),
        },
    )
}

/// Registers the pane's mailbox. The state folder is set first: the mailbox
/// lives in it.
fn register_pane(ctx: &mut RuntimeContext, role: &str, identity: &PaneIdentity) -> Result<Mailbox> {
    let herdr = Herdr::with_bin(&ctx.bins.harness.herdr);
    if let Some(dir) = &identity.state_dir {
        ctx.paths.set_state_dir(dir);
    }
    let (mailbox, _pane) = Mailbox::register_in(&herdr, ctx, role)?;
    Ok(mailbox)
}

/// The briefing of a pane.
fn briefing_for(
    kind: PaneKind,
    roster: &Roster,
    teammate: &Teammate,
    role: &str,
) -> Result<String> {
    if !kind.is_fleet() {
        return prompts::agent_prompt(roster, teammate, role);
    }
    let mut briefing = prompts::agent_prompt(roster, teammate, role)?;
    if let Some(profile) = verify_from_env()? {
        briefing = format!("{}\n\n{}", briefing.trim_end(), verify_briefing(profile));
    }
    Ok(match respawn_from_env() {
        Some((slug, number)) => format!(
            "{}\n\n{}",
            briefing.trim_end(),
            orchestrator::first_instruction(&slug, number)
        ),
        None => briefing,
    })
}

/// The run slug and the respawn number `fleet_with` put in this pane's
/// environment for a respawned orchestrator (C7).
fn respawn_from_env() -> Option<(String, i64)> {
    std::env::var(orchestrator::RESPAWN_ENV)
        .ok()
        .and_then(|v| orchestrator::parse_respawn_env(&v))
}

/// Hold the orchestrator lock of `workspace` while the agent runs: 2 fleet
/// orchestrators never run in 1 workspace (C7). The watchdog reads the
/// holder as the orchestrator's agent PID.
fn hold_orchestrator_lock(
    ctx: &RuntimeContext,
    workspace: &str,
) -> Result<horch_core::fsx::DirLockGuard> {
    let dir = orchestrator::lock_dir(&ctx.paths.ledger_db);
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("creating the lock folder {}", dir.display()))?;
    match orchestrator::lock(&dir, workspace)? {
        Some(guard) => Ok(guard),
        None => {
            let pid = orchestrator::holder(&dir, workspace)
                .map(|h| format!(" (PID {})", h.pid))
                .unwrap_or_default();
            anyhow::bail!(
                "an orchestrator already runs in workspace {workspace}{pid}; \
                 1 workspace has 1 orchestrator"
            )
        }
    }
}

/// The profile `horch fleet` put in this pane's environment, if any.
fn verify_from_env() -> Result<Option<VerifyProfile>> {
    match std::env::var(VERIFY_ENV) {
        Ok(v) if !v.trim().is_empty() => v
            .parse()
            .map(Some)
            .with_context(|| format!("{VERIFY_ENV}='{v}' is not a verification profile")),
        _ => Ok(None),
    }
}

/// An orchestrator runs a different set of commands than a worker, and codex
/// refuses anything its execpolicy does not name. Install the set this pane
/// actually needs, not both.
fn exec_rules_for(kind: PaneKind, roster: &Roster) -> &[ExecRule] {
    match kind {
        PaneKind::FleetCodexOrchestrator => roster.orchestrator_exec_rules(),
        _ => roster.exec_rules(),
    }
}

/// The fixed 5-pane recipe assigns the model per worker pane; a worker pane
/// without one fails before it has any side effect.
fn require_model(kind: PaneKind, model: Option<&str>) -> Result<()> {
    let worker = matches!(
        kind,
        PaneKind::OrchestrationClaude | PaneKind::OrchestrationCodex
    );
    if worker && model.is_none() {
        anyhow::bail!("--model is required for an orchestration worker pane");
    }
    Ok(())
}

/// What the agent inherits on top of this process's environment. A pane is a
/// fresh shell, so everything `horch fleet` knew travels here. The state
/// folder is the effective one, whether `--state-dir`, the environment or
/// `XDG_STATE_HOME` chose it, so the agent's `horch` finds this pane's mailbox
/// even when its harness strips ambient variables.
///
/// Also moves this process into `HORCH_PROJECT_DIR` when it names another
/// directory: the process's working directory, not an environment variable,
/// so the agent and a relative roster path both see it.
fn child_env(
    ctx: &mut RuntimeContext,
    mailbox: &Mailbox,
    identity: &PaneIdentity,
    teammates_dir: Option<&str>,
) -> Vec<(String, String)> {
    let mut env: Vec<(String, String)> = vec![
        (
            "HORCH_WORKSPACE_ID".into(),
            mailbox.workspace_id().to_string(),
        ),
        (
            "HORCH_STATE_DIR".into(),
            ctx.paths.state_root.to_string_lossy().into_owned(),
        ),
    ];
    // Like a worker, the orchestrator knows its own record.
    if let Some(record_id) = &identity.record_id {
        env.push(("HORCH_RECORD_ID".into(), record_id.clone()));
    }
    // Set explicitly, not inherited: a Codex shell policy can drop it, and
    // the orchestrator's `horch ledger run new` reads it.
    if let Ok(verify) = std::env::var(VERIFY_ENV) {
        env.push((VERIFY_ENV.into(), verify));
    }
    // Re-export it: the orchestrator runs `horch spawn` from this pane, and
    // those spawns must resolve the same roster this pane was launched with.
    if let Some(dir) = teammates_dir {
        env.push(("HORCH_TEAMMATES_DIR".into(), dir.to_string()));
    }
    if let Some(project) = ctx
        .paths
        .project_dir
        .clone()
        .filter(|p| Some(p) != ctx.paths.cwd.as_ref())
    {
        if std::env::set_current_dir(&project).is_ok() {
            ctx.paths.cwd = Some(project);
        }
    }
    // Both orchestrator and worker briefings name `horch` as an on-PATH command.
    if let Some(path) = ctx.prepend_own_dir_to_path() {
        env.push(("PATH".into(), path.to_string_lossy().into_owned()));
    }
    env
}

/// The roster of a pane. A fleet orchestrator is offered only the teammates
/// this project needs (`offer_when`); the fixed recipe briefs no roster
/// choice, so it keeps the full list.
fn launch_roster(
    ctx: &RuntimeContext,
    kind: PaneKind,
    teammates_dir: Option<&str>,
) -> Result<Roster> {
    let mut roster = super::load_roster(ctx, teammates_dir)?;
    if kind.is_fleet() {
        if let Ok(project) = ctx.paths.project() {
            roster = roster.with_project_facts(project_facts(&project));
        }
    }
    Ok(roster)
}

/// The teammate file of `kind`, as launched. A fleet flavor passes the
/// orchestrator's model; it is carried on the teammate too, so everything that
/// reads the resolved teammate sees the model actually launched, not the
/// file's fallback. The fixed recipe assigns the agent per pane, not per file.
fn launch_teammate(roster: &Roster, kind: PaneKind, model: Option<&str>) -> Result<Teammate> {
    let mut teammate = roster.for_launch(roster.require(kind.teammate_file())?.clone());
    if let Some(model) = model.filter(|_| kind.is_fleet()) {
        teammate.model = Some(model.to_string());
    }
    if kind == PaneKind::OrchestrationCodex {
        teammate.agent = HarnessKind::Codex;
        teammate.effort = None;
    }
    Ok(teammate)
}

/// How the pane starts its session, and the directory of its ledger record.
/// A managed session when `horch fleet` minted an id (claude); a codex
/// orchestrator's id is discovered after launch, as a worker's is.
/// `horch fleet --resume` passes the id to continue, for either agent.
fn session_for(
    identity: &PaneIdentity,
    ctx: &RuntimeContext,
) -> Result<(SessionMode, Option<String>)> {
    let session_id = identity
        .session_id
        .clone()
        .map(SessionId::new)
        .transpose()?;
    let session = match session_id {
        Some(id) if identity.resume => SessionMode::Resume(id),
        id => SessionMode::Fresh(id),
    };
    let workdir = match &identity.record_id {
        Some(_) => Some(ctx.paths.project()?.to_string_lossy().into_owned()),
        None => None,
    };
    Ok((session, workdir))
}

#[cfg(test)]
mod tests {
    use super::*;
    use horch_core::roster::ProjectFacts;

    /// A context with a known binary path and nothing else.
    fn test_ctx() -> RuntimeContext {
        RuntimeContext::from_env(
            &horch_core::runtime::MapEnv::new("/").with_exe("/opt/horch/bin/horch"),
        )
        .unwrap()
    }

    /// [`workspace_label`] against the test process's current directory.
    fn label(path: &Path) -> String {
        workspace_label(path, std::env::current_dir().ok().as_deref())
    }

    #[test]
    fn pane_kinds_round_trip() {
        for kind in [
            PaneKind::FleetOrchestrator,
            PaneKind::FleetCodexOrchestrator,
            PaneKind::OrchestrationOrchestrator,
            PaneKind::OrchestrationClaude,
            PaneKind::OrchestrationCodex,
        ] {
            assert_eq!(kind.as_str().parse::<PaneKind>().unwrap(), kind);
        }
        assert!("nonsense".parse::<PaneKind>().is_err());
    }

    /// `herdr-fleet` takes the model name or the CLI name, and a typo is
    /// refused rather than quietly defaulted.
    #[test]
    fn fleet_flavors_parse_from_what_a_user_types() {
        for (words, flavor) in [
            (
                &["opus", "cc", "claude", "CC", "Opus"][..],
                FleetFlavor::Opus,
            ),
            (&["fable", "FABLE"][..], FleetFlavor::Fable),
            (&["astra", "codex", "CODEX"][..], FleetFlavor::Astra),
            (&["sol", "Sol"][..], FleetFlavor::Sol),
        ] {
            for word in words {
                assert_eq!(word.parse::<FleetFlavor>().unwrap(), flavor, "{word}");
            }
        }
        assert_eq!(
            FleetFlavor::default(),
            FleetFlavor::Opus,
            "a bare `fleet` is Opus"
        );
        let err = "sonnet".parse::<FleetFlavor>().unwrap_err();
        assert!(err.contains("unknown fleet flavor 'sonnet'"), "{err}");
        // Display round-trips, because clap prints the default with it.
        for flavor in [
            FleetFlavor::Opus,
            FleetFlavor::Fable,
            FleetFlavor::Astra,
            FleetFlavor::Sol,
        ] {
            assert_eq!(flavor.to_string().parse::<FleetFlavor>().unwrap(), flavor);
        }
    }

    /// Each flavor must reach its own teammate file with its own model, or
    /// `horch fleet sol` silently launches Astra.
    #[test]
    fn each_flavor_launches_its_own_orchestrator_pane_and_model() {
        for (flavor, kind, model) in [
            (FleetFlavor::Opus, PaneKind::FleetOrchestrator, "opus"),
            (FleetFlavor::Fable, PaneKind::FleetOrchestrator, "fable"),
            (
                FleetFlavor::Astra,
                PaneKind::FleetCodexOrchestrator,
                "gpt-6-astra",
            ),
            (
                FleetFlavor::Sol,
                PaneKind::FleetCodexOrchestrator,
                "gpt-5.6-sol",
            ),
        ] {
            assert_eq!(flavor.pane_kind(), kind, "{flavor}");
            assert_eq!(flavor.model(), model, "{flavor}");
            let cmd = pane_command(
                &test_ctx(),
                "orchestrator",
                flavor.pane_kind(),
                Some(flavor.model()),
            )
            .unwrap();
            assert!(cmd.contains(kind.as_str()), "{cmd}");
            assert!(cmd.contains(model), "{cmd}");
        }
    }

    /// Opus and Sol are not reserved tiers, so an orchestrator on either can
    /// still hand work to `opus` / `codex-sol` workers; Fable and Astra stay
    /// out of every worker's reach whichever flavor runs.
    #[test]
    fn only_fable_and_astra_orchestrators_hold_a_reserved_tier() {
        use horch_core::roster::reserved_tier;
        assert!(reserved_tier(FleetFlavor::Fable.model()).is_some());
        assert!(reserved_tier(FleetFlavor::Astra.model()).is_some());
        assert!(reserved_tier(FleetFlavor::Opus.model()).is_none());
        assert!(reserved_tier(FleetFlavor::Sol.model()).is_none());
    }

    /// The teammate each pane kind names must actually exist, or the pane comes
    /// up, fails to resolve its briefing, and dies where nobody is watching.
    #[test]
    fn every_pane_kind_names_a_teammate_that_exists() {
        let roster = horch_core::roster::Roster::builtin().unwrap();
        for kind in [
            PaneKind::FleetOrchestrator,
            PaneKind::FleetCodexOrchestrator,
            PaneKind::OrchestrationOrchestrator,
            PaneKind::OrchestrationClaude,
            PaneKind::OrchestrationCodex,
        ] {
            let name = kind.teammate_file();
            assert!(
                roster.get(name).is_some(),
                "{kind:?} names a missing '{name}'"
            );
        }
    }

    #[test]
    fn pane_command_includes_the_model_only_when_given() {
        let with = pane_command(
            &test_ctx(),
            "sonnet-1",
            PaneKind::OrchestrationClaude,
            Some("sonnet"),
        )
        .unwrap();
        assert!(with.contains("--model"), "{with}");
        assert!(with.contains("sonnet-1"), "{with}");

        let without =
            pane_command(&test_ctx(), "codex-1", PaneKind::OrchestrationCodex, None).unwrap();
        assert!(!without.contains("--model"), "{without}");
    }

    /// G7: a pane does not inherit `$XDG_DATA_HOME`, so the pane command
    /// names the spawner's skill store.
    #[test]
    fn pane_command_sets_the_spawners_data_dir() {
        let ctx = RuntimeContext::from_env(
            &horch_core::runtime::MapEnv::new("/")
                .with_exe("/opt/horch/bin/horch")
                .with("XDG_DATA_HOME", "/xdg-data"),
        )
        .unwrap();
        let cmd = pane_command(
            &ctx,
            "orchestrator",
            PaneKind::FleetOrchestrator,
            Some("opus"),
        )
        .unwrap();
        let data = std::path::Path::new("/xdg-data").join("horch");
        let set = if cfg!(windows) {
            format!("$env:HORCH_DATA_DIR = '{}'; ", data.display())
        } else {
            format!(" 'HORCH_DATA_DIR={}' ", data.display())
        };
        assert!(cmd.contains(&set), "{cmd}");
    }

    /// A pane does not inherit `$HORCH_STATE_DIR`, and the mailbox lives
    /// there, so every pane command names the state folder, however the
    /// parent chose it.
    #[test]
    fn pane_command_sets_the_state_dir() {
        for (variable, root, expected) in [
            ("HORCH_STATE_DIR", "/custom-state", "/custom-state"),
            ("XDG_STATE_HOME", "/xdg-state", "/xdg-state/horch"),
        ] {
            let ctx = RuntimeContext::from_env(
                &horch_core::runtime::MapEnv::new("/")
                    .with_exe("/opt/horch/bin/horch")
                    .with(variable, root),
            )
            .unwrap();
            let cmd =
                pane_command(&ctx, "orchestrator", PaneKind::FleetOrchestrator, None).unwrap();
            let expected = std::path::Path::new(expected);
            let set = if cfg!(windows) {
                format!("$env:HORCH_STATE_DIR = '{}'; ", expected.display())
            } else {
                format!(" 'HORCH_STATE_DIR={}' ", expected.display())
            };
            assert!(cmd.contains(&set), "{variable}: {cmd}");
        }
    }

    /// #199: the orchestrator pane uses the task ledger of the `horch` that
    /// opened it. With `HORCH_LEDGER_DB` set, the pane line sets it; without
    /// it, the line does not name it, and the pane uses the default ledger.
    #[test]
    fn pane_command_carries_the_ledger_override_only_when_set() {
        let ledger = Path::new("/scratch/try.sqlite3");
        let set = if cfg!(windows) {
            format!("$env:HORCH_LEDGER_DB = '{}'; ", ledger.display())
        } else {
            format!(" 'HORCH_LEDGER_DB={}' ", ledger.display())
        };
        let with = RuntimeContext::from_env(
            &horch_core::runtime::MapEnv::new("/")
                .with_exe("/opt/horch/bin/horch")
                .with("HORCH_LEDGER_DB", "/scratch/try.sqlite3"),
        )
        .unwrap();
        for kind in [PaneKind::FleetOrchestrator, PaneKind::OrchestrationClaude] {
            let cmd = pane_command(&with, "orchestrator", kind, None).unwrap();
            assert!(cmd.contains(&set), "{kind:?}: {cmd}");
        }
        let cmd = pane_command(
            &test_ctx(),
            "orchestrator",
            PaneKind::FleetOrchestrator,
            None,
        )
        .unwrap();
        assert!(!cmd.contains("HORCH_LEDGER_DB"), "{cmd}");
    }

    #[test]
    fn pane_command_carries_the_verify_profile_only_when_given() {
        let set = if cfg!(windows) {
            "$env:HORCH_VERIFY = 'minimal'; ".to_string()
        } else {
            " 'HORCH_VERIFY=minimal' ".to_string()
        };
        let ctx = test_ctx();
        let kind = PaneKind::FleetOrchestrator;
        let with = pane_command_for(
            &ctx,
            "orchestrator",
            kind,
            None,
            None,
            Some(VerifyProfile::Minimal),
        )
        .unwrap();
        assert!(with.contains(&set), "{with}");
        let without = pane_command(&ctx, "orchestrator", kind, None).unwrap();
        assert!(!without.contains("HORCH_VERIFY"), "{without}");
    }

    #[test]
    fn fleet_defaults_to_light_and_names_it_at_startup() {
        assert_eq!(FleetOptions::default().verify(), VerifyProfile::Light);
        let line = verify_line(FleetOptions::default().verify());
        assert!(
            line.starts_with("Verification: light (check and complexity-sweep;"),
            "{line}"
        );
        assert!(line.contains("--verify full|light|minimal"), "{line}");
    }

    #[test]
    fn verify_briefing_names_the_profile_and_its_steps() {
        let text = verify_briefing(VerifyProfile::Light);
        assert!(text.starts_with("== Verification ==\n"), "{text}");
        assert!(text.contains("profile `light`"), "{text}");
        assert!(
            text.contains("(check, complexity-sweep, land, pr, cleanup)"),
            "{text}"
        );
        assert!(!text.contains("code-sweep,"), "{text}");
        let full = verify_briefing(VerifyProfile::Full);
        assert!(full.contains("check, code-sweep, complexity-sweep, recheck, land, pr, cleanup"));
    }

    #[test]
    fn workspace_label_is_the_project_folder_name() {
        assert_eq!(
            label(Path::new("/Users/me/projects/multi-herdr")),
            "multi-herdr"
        );
    }

    #[test]
    fn workspace_label_ignores_a_trailing_slash() {
        assert_eq!(
            label(Path::new("/Users/me/projects/multi-herdr/")),
            "multi-herdr"
        );
    }

    #[test]
    fn workspace_label_falls_back_when_the_path_has_no_folder_name() {
        assert_eq!(label(Path::new("/")), "Herdr Fleet");
        assert_eq!(label(Path::new("")), "Herdr Fleet");
    }

    fn folder_name(path: &Path) -> String {
        path.file_name().unwrap().to_string_lossy().into_owned()
    }

    fn current_dir() -> PathBuf {
        std::env::current_dir().unwrap().canonicalize().unwrap()
    }

    /// `horch fleet --cwd .` is how people start a fleet, and `.` has no final
    /// component until it is resolved against the current directory.
    #[test]
    fn workspace_label_resolves_dot_to_the_current_directory_name() {
        assert_eq!(label(Path::new(".")), folder_name(&current_dir()));
    }

    #[test]
    fn workspace_label_resolves_dot_dot_to_the_parent_directory_name() {
        let parent = current_dir().parent().unwrap().to_path_buf();
        assert_eq!(label(Path::new("..")), folder_name(&parent));
    }

    /// These paths do not exist, so `canonicalize` fails and the label helper
    /// has to normalise `.` and `..` itself.
    #[test]
    fn workspace_label_normalises_a_path_that_does_not_exist() {
        assert_eq!(label(Path::new("some/dir/.")), "dir");
        assert_eq!(
            label(Path::new("no-such-dir/..")),
            folder_name(&current_dir())
        );
        assert_eq!(
            label(Path::new("/Users/me/projects/multi-herdr/../other")),
            "other"
        );
    }

    #[test]
    fn workspace_label_names_a_real_directory_however_it_is_spelled() {
        let tmp = tempfile::tempdir().unwrap();
        let project = tmp.path().join("my-project");
        std::fs::create_dir_all(project.join("sub")).unwrap();

        assert_eq!(label(&project), "my-project");
        assert_eq!(label(&project.join(".")), "my-project");
        assert_eq!(label(&project.join("sub").join("..")), "my-project");
    }
    #[test]
    fn only_the_two_orchestrator_kinds_are_fleet_kinds() {
        let fleet: Vec<&str> = [
            PaneKind::FleetOrchestrator,
            PaneKind::FleetCodexOrchestrator,
            PaneKind::OrchestrationOrchestrator,
            PaneKind::OrchestrationClaude,
            PaneKind::OrchestrationCodex,
        ]
        .into_iter()
        .filter(|k| k.is_fleet())
        .map(PaneKind::as_str)
        .collect();
        assert_eq!(fleet, ["fleet-orchestrator", "fleet-codex-orchestrator"]);
    }

    #[test]
    fn each_pane_kind_names_the_teammate_file_that_backs_it() {
        assert_eq!(PaneKind::FleetOrchestrator.teammate_file(), "orchestrator");
        assert_eq!(
            PaneKind::FleetCodexOrchestrator.teammate_file(),
            "orchestrator-codex"
        );
        assert_eq!(
            PaneKind::OrchestrationOrchestrator.teammate_file(),
            "orchestration-orchestrator"
        );
        assert_eq!(
            PaneKind::OrchestrationClaude.teammate_file(),
            "orchestration-worker"
        );
        assert_eq!(
            PaneKind::OrchestrationCodex.teammate_file(),
            "orchestration-worker"
        );
    }

    #[test]
    fn a_worker_pane_needs_a_model_and_the_others_do_not() {
        for kind in [PaneKind::OrchestrationClaude, PaneKind::OrchestrationCodex] {
            let err = require_model(kind, None).unwrap_err();
            assert!(
                err.to_string()
                    .contains("--model is required for an orchestration worker pane"),
                "{err}"
            );
            assert!(require_model(kind, Some("sonnet")).is_ok());
        }
        assert!(require_model(PaneKind::FleetOrchestrator, None).is_ok());
        assert!(require_model(PaneKind::OrchestrationOrchestrator, None).is_ok());
    }

    /// A fleet flavor's model lands on the teammate; the Codex worker pane
    /// switches agent and drops the effort; a worker pane keeps its file's model.
    #[test]
    fn launch_teammate_patches_the_teammate_per_kind() {
        let roster = Roster::builtin().unwrap();
        let file_model = |name: &str| roster.require(name).unwrap().model.clone();

        let fleet = launch_teammate(&roster, PaneKind::FleetOrchestrator, Some("opus")).unwrap();
        assert_eq!(fleet.name, "orchestrator");
        assert_eq!(fleet.model.as_deref(), Some("opus"));

        let claude = launch_teammate(&roster, PaneKind::OrchestrationClaude, Some("opus")).unwrap();
        assert_eq!(claude.name, "orchestration-worker");
        assert_eq!(claude.model, file_model("orchestration-worker"));
        assert_ne!(claude.agent, HarnessKind::Codex);

        let codex = launch_teammate(&roster, PaneKind::OrchestrationCodex, Some("x")).unwrap();
        assert_eq!(codex.agent, HarnessKind::Codex);
        assert_eq!(codex.effort, None);
    }

    #[test]
    fn session_for_resumes_only_when_asked_and_has_no_workdir_without_a_record() {
        let ctx = test_ctx();
        let id = "11111111-1111-4111-8111-111111111111";
        let identity = PaneIdentity {
            session_id: Some(id.into()),
            ..PaneIdentity::default()
        };
        let (session, workdir) = session_for(&identity, &ctx).unwrap();
        assert_eq!(session, SessionMode::Fresh(Some(id.parse().unwrap())));
        assert_eq!(workdir, None);

        let resume = PaneIdentity {
            resume: true,
            ..identity.clone()
        };
        assert_eq!(
            session_for(&resume, &ctx).unwrap().0,
            SessionMode::Resume(id.parse().unwrap())
        );
        let none = PaneIdentity::default();
        assert_eq!(
            session_for(&none, &ctx).unwrap().0,
            SessionMode::Fresh(None)
        );
    }

    #[test]
    fn resolved_path_is_nothing_for_an_empty_path_or_an_unknown_current_directory() {
        let cwd = Path::new("/base");
        assert_eq!(resolved_path(Path::new(""), Some(cwd)), None);
        assert_eq!(resolved_path(Path::new("no-such-dir/x"), None), None);
    }

    #[test]
    fn resolved_path_joins_a_missing_relative_path_to_the_current_directory() {
        let cwd = Path::new("/base/here");
        assert_eq!(
            resolved_path(Path::new("no-such-dir/./a/../b"), Some(cwd)),
            Some(PathBuf::from("/base/here/no-such-dir/b"))
        );
    }

    /// `offer_when` sees the top level and one level down, and nothing deeper
    /// or inside a dot-directory.
    #[test]
    fn project_facts_reach_one_level_down() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::write(root.join("Game.uproject"), "").unwrap();
        std::fs::create_dir_all(root.join("ios/App.xcodeproj")).unwrap();
        std::fs::create_dir_all(root.join("a/b")).unwrap();
        std::fs::write(root.join("a/b/Deep.uplugin"), "").unwrap();
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::write(root.join(".git/Hidden.uasset"), "").unwrap();

        let facts = project_facts(root);
        assert!(facts.has_match("*.uproject"), "top level");
        assert!(
            facts.has_match("*.xcodeproj"),
            "one level down, a directory"
        );
        assert!(facts.has_match("b"), "a directory one level down is a name");
        assert!(!facts.has_match("*.uplugin"), "two levels down");
        assert!(!facts.has_match("*.uasset"), "inside a dot-directory");
        assert_eq!(
            project_facts(&root.join("missing")),
            ProjectFacts::default()
        );
    }

    /// A context whose ledger lives in `dir`.
    fn ledger_ctx(dir: &Path) -> RuntimeContext {
        RuntimeContext::from_env(
            &horch_core::runtime::MapEnv::new("/")
                .with_exe("/opt/horch/bin/horch")
                .with("HORCH_STATE_DIR", dir.join("state").to_str().unwrap())
                .with(
                    "HORCH_LEDGER_DB",
                    dir.join("ledger.sqlite3").to_str().unwrap(),
                ),
        )
        .unwrap()
    }

    /// A herdr where pane `p1` exists, every other pane is gone, and
    /// workspace `w1` has pane `p9`, which splits into `p10`.
    fn respawn_herdr(dir: &Path) -> Herdr {
        let bin = dir.join("herdr");
        let warm = crate::test_support::write_warmed(
            &bin,
            "#!/bin/sh\n\
             case \"$*\" in\n\
             'pane get p1') echo '{\"result\":{\"pane\":{\"pane_id\":\"p1\"}}}' ;;\n\
             'pane list --workspace w1') echo '{\"result\":{\"panes\":[{\"pane_id\":\"p9\"}]}}' ;;\n\
             'pane split p9 --direction right --no-focus') echo '{\"result\":{\"pane\":{\"pane_id\":\"p10\"}}}' ;;\n\
             warm*) ;;\n\
             *) exit 1 ;;\n\
             esac\n",
            &["warm"],
        );
        assert!(warm.status.success());
        Herdr::with_bin(bin)
    }

    fn respawn(old_pane: Option<&str>) -> Respawn {
        Respawn {
            run_id: "run-1".into(),
            slug: "r1".into(),
            project_root: "/repo".into(),
            workspace_id: "w1".into(),
            old_pane_id: old_pane.map(str::to_string),
            old_worker_id: "w-old".into(),
            new_worker_id: "w-new".into(),
            number: 2,
            finish_steps: Vec::new(),
        }
    }

    #[test]
    fn trf_38_a_respawn_runs_in_the_old_pane_or_beside_the_first_pane() {
        let dir = tempfile::tempdir().unwrap();
        let herdr = respawn_herdr(dir.path());
        let ws = respawn_pane(&herdr, &respawn(Some("p1"))).unwrap();
        assert_eq!(
            (ws.workspace_id.as_str(), ws.root_pane_id.as_str()),
            ("w1", "p1")
        );
        let ws = respawn_pane(&herdr, &respawn(Some("p-gone"))).unwrap();
        assert_eq!(
            ws.root_pane_id, "p10",
            "a gone pane: a new pane in the same workspace"
        );
        let ws = respawn_pane(&herdr, &respawn(None)).unwrap();
        assert_eq!(
            (ws.workspace_id.as_str(), ws.root_pane_id.as_str()),
            ("w1", "p10")
        );
    }

    #[test]
    fn trf_38_the_respawn_pane_carries_its_first_instruction() {
        let ctx = test_ctx();
        let record = |respawn: Option<String>| PaneRecord {
            record_id: "rec-1",
            session_id: None,
            resume: false,
            respawn,
        };
        let kind = PaneKind::FleetOrchestrator;
        let r = respawn(None);
        let with = pane_command_for(
            &ctx,
            "orchestrator",
            kind,
            None,
            Some(record(Some(r.env_value()))),
            None,
        )
        .unwrap();
        assert!(with.contains("HORCH_ORCHESTRATOR_RESPAWN=r1 2"), "{with}");
        let without =
            pane_command_for(&ctx, "orchestrator", kind, None, Some(record(None)), None).unwrap();
        assert!(!without.contains("HORCH_ORCHESTRATOR_RESPAWN"), "{without}");

        let (slug, number) = orchestrator::parse_respawn_env(&r.env_value()).unwrap();
        let first = orchestrator::first_instruction(&slug, number);
        for item in [
            orchestrator::HANDOFF_FILE,
            "ai_docs/plans/r1/decisions.md",
            "horch ledger status",
        ] {
            assert!(first.contains(item), "{first}");
        }
    }

    #[test]
    fn trf_38_a_respawn_keeps_the_flavor_of_the_dead_orchestrator() {
        for f in [
            FleetFlavor::Opus,
            FleetFlavor::Fable,
            FleetFlavor::Astra,
            FleetFlavor::Sol,
        ] {
            assert_eq!(flavor_of_model(f.model()), f);
        }
        assert_eq!(flavor_of_model("some-model"), FleetFlavor::Opus);
    }

    #[test]
    fn trf_39_fleet_stop_records_the_stop_of_each_active_run() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ledger_ctx(dir.path());
        let root = dir.path().join("repo");
        std::fs::create_dir_all(&root).unwrap();
        let mut store = horch_core::taskledger::open(&ctx).unwrap();
        let run = store
            .immediate(|tx| {
                horch_core::taskledger::runs::create(
                    tx,
                    &root,
                    &horch_core::taskledger::runs::NewRun {
                        slug: "r1".into(),
                        ..Default::default()
                    },
                )
            })
            .unwrap();
        let stopped = |store: &horch_core::taskledger::Store| {
            orchestrator::guard(store.conn(), &run.run_id)
                .unwrap()
                .stopped_at
        };
        assert_eq!(stopped(&store), None);
        assert_eq!(fleet_stop(&ctx, root.to_str()).unwrap(), ExitCode::SUCCESS);
        let first = stopped(&store).expect("the stop is recorded");
        assert_eq!(fleet_stop(&ctx, root.to_str()).unwrap(), ExitCode::SUCCESS);
        assert_eq!(stopped(&store), Some(first), "the first stop stays");
    }

    #[test]
    fn trf_40_a_second_fleet_orchestrator_in_a_workspace_refuses() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ledger_ctx(dir.path());
        let held = hold_orchestrator_lock(&ctx, "w1").unwrap();
        let err = hold_orchestrator_lock(&ctx, "w1").unwrap_err().to_string();
        assert!(
            err.contains(&format!(
                "an orchestrator already runs in workspace w1 (PID {})",
                std::process::id()
            )),
            "{err}"
        );
        assert!(
            hold_orchestrator_lock(&ctx, "w2").is_ok(),
            "1 lock per workspace"
        );
        drop(held);
        assert!(
            hold_orchestrator_lock(&ctx, "w1").is_ok(),
            "free after the agent ends"
        );
    }
}
