//! `horch fleet` and `horch orchestration` - the port of the justfile recipes and
//! the per-pane launcher scripts they ran.
//!
//! `orchestration` builds a fixed 5-pane workspace and starts an agent in each
//! pane. `fleet` starts fleetd and 1 herdr workspace with no orchestrator:
//! there is no long-lived orchestrator conversation, and fleetd starts 1
//! sub-orchestrator for each feature (FM-16). You add work with `horch idea
//! add`.

use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result};
use horch::project::project_facts;
use horch_core::execution::records::SessionLedger;
use horch_core::execution::SessionMode;
use horch_core::fleetd::fleet_runtime::domain::requests::retired;
use horch_core::fleetd::task_execution::application::launch::PushLaunch;
use horch_core::fleetd::worker_profiles::application::ports::{EnvRule, LaunchSpec};
use horch_core::harness::launch::{self, DiscoveryTarget, LaunchRequest};
use horch_core::harness::HarnessKind;
use horch_core::ids::SessionId;
use horch_core::messaging::mailbox::Mailbox;
use horch_core::prompts;
use horch_core::roster::{ExecRule, Roster, Teammate};
use horch_core::runtime::RuntimeContext;
use horch_core::taskledger::model::VERIFY_ENV;
use horch_core::taskledger::orchestrator;
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

/// The FLAVOR word of `horch fleet`: the agent and model of the orchestrator
/// before Phase 6. The fleet starts no orchestrator now (FM-16), so the word
/// changes nothing; it is still parsed, so an old command line runs.
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
    Ok(PaneShell::host().command_line_with_env(&exe, &env, &args))
}

/// Options of `horch fleet`.
#[derive(Debug, Clone, Default)]
pub struct FleetOptions {
    /// `--no-update`: skip the agent CLI update step (ARC-34).
    pub no_update: bool,
    /// `--resume`: an orchestrator session to resume. Refused: the fleet
    /// starts no orchestrator (FM-16).
    pub resume: Option<String>,
}

/// `horch fleet`: start fleetd and 1 herdr workspace in `cwd`, with no
/// orchestrator pane (FM-16). fleetd starts 1 sub-orchestrator for each
/// feature; you add work with `horch idea add`. The fleetd check runs
/// before this command (`dispatch_checked` in `main.rs`). Unless
/// `options.no_update`, every agent CLI updates first (ARC-32).
pub fn fleet_with(ctx: &RuntimeContext, cwd: Option<&str>, options: &FleetOptions) -> Result<()> {
    if options.resume.is_some() {
        anyhow::bail!(orchestrator::RETIRED);
    }
    println!("horch {}", super::selfupdate::CURRENT);
    super::update::before_fleet(ctx, options.no_update);
    doctor::check(ctx)?;
    let herdr = Herdr::with_bin(&ctx.bins.harness.herdr);
    let cwd = resolve_cwd(ctx, cwd)?;
    let mut ctx = ctx.clone();
    ctx.paths.project_dir = Some(PathBuf::from(&cwd));
    super::spawn::fleet_warnings(&ctx, false);

    warn_about_missing_integrations(&herdr);
    super::restore::warn_if_resume_on(&ctx);
    // Orchestrator records of workspaces that are gone are retired: an
    // orchestrator of a run in flight never runs `horch done`.
    retire_gone_orchestrators(&ctx, &herdr)?;
    horch_core::sweep::run_due(&ctx);

    println!("Creating fleet workspace rooted at {cwd}...");
    let label = workspace_label(Path::new(&cwd), ctx.paths.cwd.as_deref());
    let ws = herdr.workspace_create(&label, Some(&cwd), true)?;
    ctx.herdr.workspace = Some(horch_core::ids::WorkspaceId::new(ws.workspace_id.as_str())?);
    let ctx = &ctx;

    // Parked worktrees get a steward pane beside the first pane (#308).
    start_worktree_steward(ctx, &ws);

    // The telemetry space (SPC-06). Never a reason for the fleet to fail.
    if let Err(e) = super::telemetry::ensure(ctx, &herdr, true) {
        println!("warning: telemetry space not started: {e:#}");
    }

    print_fleet_ready(&ws.workspace_id);
    Ok(())
}

/// When the project has parked worktrees (`horch worktree triage`), queue a
/// one-off task for the `worktree-steward` teammate and spawn it beside the
/// first pane: it claims the task and runs the triage with the operator.
/// With none, nothing is queued or spawned. Never a reason for the fleet to
/// fail.
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
    // No code path types a task into a pane (TRF-43): the steward claims
    // this task from the ledger. The fleet spawns the steward itself, so
    // it prints no `Run next` line.
    let queued =
        steward_task(rows.len()).and_then(|args| super::taskledger::queue_one_off(ctx, args));
    let run = match queued {
        Ok(run) => run,
        Err(e) => {
            println!("warning: the worktree steward task was not queued: {e:#}");
            return;
        }
    };
    if let Err(e) = super::spawn::spawn(ctx, steward_spawn_args(&ws.root_pane_id, &run.slug)) {
        println!("warning: the worktree steward did not start: {e:#}");
    }
}

/// The teammate that runs the worktree triage with the operator.
const WORKTREE_STEWARD: &str = "worktree-steward";

/// What `horch ledger task 'parked worktrees: <parked>' --persona
/// worktree-steward --check none` runs. The title is a fact, not a brief:
/// the steward's file holds its instructions.
fn steward_task(parked: usize) -> Result<super::taskledger::TaskArgs> {
    #[derive(clap::Parser)]
    struct Task {
        #[command(flatten)]
        args: super::taskledger::TaskArgs,
    }
    let title = format!("parked worktrees: {parked}");
    let argv = [
        "task",
        title.as_str(),
        "--persona",
        WORKTREE_STEWARD,
        "--check",
        "none",
    ];
    Ok(<Task as clap::Parser>::try_parse_from(argv)?.args)
}

/// The steward's spawn beside the first pane. It names `run`, the solo run
/// that holds the task [`steward_task`] queued, so a planned run in the
/// workspace never changes which run it joins. It claims that task.
fn steward_spawn_args(root_pane: &str, run: &str) -> super::spawn::SpawnArgs {
    super::spawn::SpawnArgs {
        teammate: Some(WORKTREE_STEWARD.to_string()),
        task: String::new(),
        resume: None,
        phase: None,
        effort: None,
        model: None,
        role: None,
        from_pane: Some(root_pane.to_string()),
        direction: Direction::Right,
        tiling: horch_core::execution::TilingMode::Automatic,
        exact: false,
        force: false,
        replaces: None,
        run: Some(run.to_string()),
        attempt: None,
    }
}

/// End the orchestrator records of workspaces that are gone: an
/// orchestrator never runs `horch done`. Without a herdr workspace list,
/// none is ended.
fn retire_gone_orchestrators(ctx: &RuntimeContext, herdr: &Herdr) -> Result<()> {
    let ledger = SessionLedger::open_in(ctx)?;
    if let Ok(open) = herdr.workspace_list() {
        let open: Vec<String> = open.into_iter().map(|w| w.workspace_id).collect();
        ledger.supersede_orchestrators(|ws| open.iter().any(|o| o == ws))?;
    }
    Ok(())
}

/// The closing text of `horch fleet`.
fn print_fleet_ready(workspace_id: &str) {
    println!("\nDone. Fleet workspace {workspace_id} is live. fleetd starts 1 sub-orchestrator for each feature.");
    println!("Add an idea: horch idea add \"...\"");
    println!("See the ideas and what waits on you: horch ideas");
}

/// Close the workspace of a start that failed after it was created
/// (#217). A cleanup error is printed; the start's own error is the one the
/// caller returns.
fn close_workspace(herdr: &Herdr, workspace_id: &str) {
    if let Err(e) = herdr.workspace_close(workspace_id) {
        eprintln!("warning: closing workspace {workspace_id} failed: {e:#}");
    }
}

/// The launch of the push attempt `l` that fleetd bound (ticket 10 of
/// `docs/tickets/fleetd-p4/`): `horch spawn <persona> --attempt <id>` in
/// the run of the task, beside the pane of its orchestrator, in the
/// workspace of the run. The spawn binds nothing: it opens the pane of the
/// attempt, whose environment names `HORCH_TASK_YAML`. fleetd starts it
/// through its launcher (`task_execution::application::launch::launch`),
/// which removes `ANTHROPIC_API_KEY` (SR-7). `--exact` keeps the persona of
/// the bind.
// The pass of Phase 4 (ticket 46 of `docs/tickets/fleetd-p4/`) calls it
// from the composition root of `horch fleetd`.
#[cfg_attr(not(test), allow(dead_code))]
pub fn push_launch_spec(ctx: &RuntimeContext, l: &PushLaunch) -> Result<LaunchSpec> {
    let Some(pane) = l.from_pane.as_deref() else {
        anyhow::bail!(
            "the launch of attempt {} has no pane to split: the run has no orchestrator pane",
            l.attempt
        );
    };
    // A detached launch removes the pane identity of fleetd, so the spawn
    // finds its mailbox only through this variable.
    let Some(workspace) = l.workspace_id.as_deref() else {
        anyhow::bail!(
            "the launch of attempt {} has no workspace: the run and the worker row name none",
            l.attempt
        );
    };
    let args = [
        "spawn",
        l.persona.as_str(),
        "--run",
        l.run_slug.as_str(),
        "--exact",
        "--attempt",
        l.attempt.as_str(),
        "--from-pane",
        pane,
    ];
    Ok(LaunchSpec {
        program: ctx.bins.exe()?,
        args: args.iter().map(OsString::from).collect(),
        cwd: Some(PathBuf::from(&l.project_root)),
        // The fleet of fleetd goes explicitly to the spawn (T-890), so it
        // never guesses its ledger or fleet root from `HORCH_STATE_DIR`.
        env_rule: EnvRule {
            vars: std::iter::once(("HORCH_WORKSPACE_ID".into(), workspace.into()))
                .chain(
                    ctx.paths
                        .fleet_env()
                        .map(|(k, v)| (k.into(), v.as_os_str().to_os_string())),
                )
                .collect(),
        },
        detach: true,
        output: Some(ctx.paths.fleet_root.join("run").join("launch.log")),
    })
}

/// `horch fleet stop`, retired by ticket 48 of fleetd-p6: no orchestrator
/// runs, so it has none to stop. It prints the line of the retired kind
/// `orchestrator_stop`, sends nothing and exits 1.
pub fn fleet_stop(_ctx: &RuntimeContext, _cwd: Option<&str>) -> Result<ExitCode> {
    let r = retired("orchestrator_stop").expect("orchestrator_stop is a retired kind");
    eprintln!("{}", r.line());
    Ok(ExitCode::from(1))
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
        close_workspace(&herdr, &ws.workspace_id);
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
    // There is no long-lived orchestrator conversation (FM-16): the pane of
    // a fleet orchestrator starts no agent.
    if kind.is_fleet() {
        orchestrator::launch_run()?;
    }
    let (mailbox, _pane, _lock) = register_pane(ctx, role, &identity, false)?;
    let child_env = child_env(ctx, &mailbox, &identity, teammates_dir);
    let ctx: &RuntimeContext = ctx;
    let roster = launch_roster(ctx, kind, teammates_dir)?;
    let teammate = launch_teammate(&roster, kind, model)?;
    let prompt = briefing_for(&roster, &teammate, role)?;
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
/// The second value is the pane id. A fleet orchestrator (`fleet`) takes
/// the workspace orchestrator lock before it registers its role, so a
/// launch that loses the lock never replaces the live route (TRF-39). The
/// third value is that lock.
pub(super) fn register_pane(
    ctx: &mut RuntimeContext,
    role: &str,
    identity: &PaneIdentity,
    fleet: bool,
) -> Result<(Mailbox, String, Option<horch_core::fsx::DirLockGuard>)> {
    register_pane_then(ctx, role, identity, fleet, |_, _, _| Ok(()))
}

/// [`register_pane`] that runs `before_route` with the workspace and the
/// pane after it takes the lock and before it writes the route. A failed
/// `before_route` writes no route.
fn register_pane_then(
    ctx: &mut RuntimeContext,
    role: &str,
    identity: &PaneIdentity,
    fleet: bool,
    before_route: impl FnOnce(&RuntimeContext, &str, &str) -> Result<()>,
) -> Result<(Mailbox, String, Option<horch_core::fsx::DirLockGuard>)> {
    let herdr = Herdr::with_bin(&ctx.bins.harness.herdr);
    if let Some(dir) = &identity.state_dir {
        ctx.paths.set_state_dir(dir);
    }
    let (mailbox, pane) = Mailbox::locate_in(&herdr, ctx)?;
    let lock = fleet
        .then(|| hold_orchestrator_lock(ctx, mailbox.workspace_id()))
        .transpose()?;
    before_route(ctx, mailbox.workspace_id(), &pane)?;
    mailbox.register_role(role, &pane)?;
    Ok((mailbox, pane, lock))
}

/// The briefing of a pane.
fn briefing_for(roster: &Roster, teammate: &Teammate, role: &str) -> Result<String> {
    prompts::agent_prompt(roster, teammate, role)
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

    fn push(from_pane: Option<&str>, workspace: Option<&str>) -> PushLaunch {
        PushLaunch {
            attempt: "T1/A1".into(),
            task_id: "T1".into(),
            worker_id: "W1".into(),
            persona: "opus".into(),
            run_slug: "r1".into(),
            project_root: "/work/proj".into(),
            from_pane: from_pane.map(str::to_string),
            workspace_id: workspace.map(str::to_string),
        }
    }

    fn arg_strings(spec: &LaunchSpec) -> Vec<String> {
        spec.args
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    }

    /// Ticket 10 of fleetd-p4: fleetd opens the pane of the attempt that it
    /// bound with `horch spawn <persona> --attempt <id>`, beside the pane
    /// of the orchestrator, in the workspace of the run. The spawn gets no
    /// claim target and no row to adopt: the attempt names its task and
    /// its worker.
    #[test]
    fn push_launch_spec_spawns_the_bound_attempt() {
        let ctx = test_ctx();
        let spec = push_launch_spec(&ctx, &push(Some("w0:p0"), Some("w0"))).unwrap();
        assert_eq!(spec.program, PathBuf::from("/opt/horch/bin/horch"));
        assert_eq!(
            arg_strings(&spec),
            [
                "spawn",
                "opus",
                "--run",
                "r1",
                "--exact",
                "--attempt",
                "T1/A1",
                "--from-pane",
                "w0:p0"
            ]
        );
        assert_eq!(spec.cwd.as_deref(), Some(Path::new("/work/proj")));
        let env: Vec<(String, String)> = spec
            .env_rule
            .vars
            .iter()
            .map(|(k, v)| {
                (
                    k.to_string_lossy().into_owned(),
                    v.to_string_lossy().into_owned(),
                )
            })
            .collect();
        let text = |p: &Path| p.to_string_lossy().into_owned();
        assert_eq!(
            env,
            [
                ("HORCH_WORKSPACE_ID".to_string(), "w0".to_string()),
                ("HORCH_LEDGER_DB".to_string(), text(&ctx.paths.ledger_db)),
                ("FLEET_ROOT".to_string(), text(&ctx.paths.fleet_root)),
            ]
        );
        assert!(spec.detach);
        assert_eq!(
            spec.output,
            Some(ctx.paths.fleet_root.join("run").join("launch.log"))
        );
    }

    #[test]
    fn push_launch_with_no_pane_or_no_workspace_is_refused() {
        let ctx = test_ctx();
        let e = push_launch_spec(&ctx, &push(None, Some("w0"))).unwrap_err();
        assert!(format!("{e:#}").contains("no pane to split"), "{e:#}");
        let e = push_launch_spec(&ctx, &push(Some("w0:p0"), None)).unwrap_err();
        assert!(format!("{e:#}").contains("no workspace"), "{e:#}");
    }

    #[cfg(unix)]
    #[test]
    fn detached_push_launch_gives_the_spawn_its_workspace() {
        use horch_core::fleetd::worker_profiles::adapters::launcher::Launcher;
        use horch_core::fleetd::worker_profiles::application::ports::ProcessHost;
        let dir = tempfile::tempdir().unwrap();
        let dump = dir.path().join("env.txt");
        let mut spec = push_launch_spec(&test_ctx(), &push(Some("w0:p0"), Some("w0"))).unwrap();
        spec.program = PathBuf::from("/bin/sh");
        spec.args = vec!["-c".into(), format!("env > '{}'", dump.display()).into()];
        spec.cwd = Some(dir.path().to_path_buf());
        spec.output = None;
        let mut launched = Launcher.launch(&spec).unwrap();
        launched.child.wait().unwrap();
        let text = std::fs::read_to_string(&dump).unwrap();
        assert!(text.lines().any(|l| l == "HORCH_WORKSPACE_ID=w0"), "{text}");
    }

    #[test]
    fn trf_43_the_steward_spawn_has_no_task_text_and_its_queue_is_retired() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("proj");
        std::fs::create_dir_all(&root).unwrap();
        let env = horch_core::runtime::MapEnv::new(&root)
            .with("HORCH_PROJECT_DIR", root.to_str().unwrap())
            .with("HORCH_WORKSPACE_ID", "w7");
        let mut ctx = RuntimeContext::from_env(&env).unwrap();
        ctx.paths.ledger_db = dir.path().join("ledger.sqlite3");

        let task = steward_task(3).unwrap();
        let err = super::super::taskledger::queue_one_off(&ctx, task).unwrap_err();
        assert!(
            format!("{err:#}").starts_with("horch ledger task is retired. Add a ticket that you wrote: horch idea add --direct"),
            "{err:#}"
        );
        assert!(!ctx.paths.ledger_db.exists(), "the refusal wrote a ledger");

        let args = steward_spawn_args("w7:p1", "solo-w7");
        assert_eq!(args.run.as_deref(), Some("solo-w7"));
        assert_eq!(args.teammate.as_deref(), Some(WORKTREE_STEWARD));
        assert_eq!(args.from_pane.as_deref(), Some("w7:p1"));
        assert!(args.task.is_empty(), "{:?}", args.task);
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

    #[cfg(unix)]
    #[test]
    fn trf_39_losing_launch_keeps_the_live_route() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("herdr");
        let warm = crate::test_support::write_warmed(
            &bin,
            "#!/bin/sh\n\
             case \"$*\" in\n\
             'pane get p2') echo '{\"result\":{\"pane\":{\"pane_id\":\"p2\",\"workspace_id\":\"w1\"}}}' ;;\n\
             warm*) ;;\n\
             *) exit 1 ;;\n\
             esac\n",
            &["warm"],
        );
        assert!(warm.status.success());
        let db = dir.path().join("ledger.sqlite3");
        let mut ctx = RuntimeContext::from_env(
            &horch_core::runtime::MapEnv::new("/")
                .with_exe("/opt/horch/bin/horch")
                .with(
                    "HORCH_STATE_DIR",
                    dir.path().join("state").to_str().unwrap(),
                )
                .with("HORCH_LEDGER_DB", db.to_str().unwrap())
                .with("HORCH_HERDR_BIN", bin.to_str().unwrap())
                .with("HERDR_PANE_ID", "p2"),
        )
        .unwrap();
        // Launch A holds the w1 lock and the orchestrator route.
        let lock_dir = orchestrator::lock_dir(&db);
        std::fs::create_dir_all(&lock_dir).unwrap();
        let a = orchestrator::lock(&lock_dir, "w1").unwrap().unwrap();
        let mailbox = Mailbox::under(&ctx.paths.state_root, "w1");
        std::fs::create_dir_all(mailbox.dir()).unwrap();
        mailbox.register_role("orchestrator", "p1").unwrap();

        // Launch B in pane p2 loses the lock: the route stays on A.
        let Err(err) = register_pane(&mut ctx, "orchestrator", &PaneIdentity::default(), true)
        else {
            panic!("a 2nd orchestrator refuses");
        };
        let err = err.to_string();
        assert!(err.contains("already runs in workspace w1"), "{err}");
        assert_eq!(mailbox.pane_for("orchestrator").as_deref(), Some("p1"));

        // After A ends, B takes the lock and then the route.
        drop(a);
        let (_mb, pane, lock) =
            register_pane(&mut ctx, "orchestrator", &PaneIdentity::default(), true).unwrap();
        assert_eq!(pane, "p2");
        assert!(lock.is_some());
        assert_eq!(mailbox.pane_for("orchestrator").as_deref(), Some("p2"));
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
