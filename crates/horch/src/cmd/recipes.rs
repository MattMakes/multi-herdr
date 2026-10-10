//! `horch fleet` and `horch orchestration` - the port of the justfile recipes and
//! the per-pane launcher scripts they ran.
//!
//! Both build a herdr workspace, lay out panes, and start an agent in each. The
//! difference is lifecycle: `orchestration` is a fixed 5-pane workspace, while
//! `fleet` starts one orchestrator alone, which then grows and shrinks the fleet
//! itself through the session ledger.

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result};
use horch::project::project_facts;
use horch_core::execution::legacy::{Record, KIND_ORCHESTRATOR};
use horch_core::execution::records::{Ledger, ORCHESTRATING_TASK};
use horch_core::execution::SessionMode;
use horch_core::harness::launch::{self, DiscoveryTarget, LaunchRequest};
use horch_core::harness::HarnessKind;
use horch_core::ids::SessionId;
use horch_core::messaging::mailbox::Mailbox;
use horch_core::prompts;
use horch_core::roster::{Roster, Teammate};
use horch_core::routing::quota::{self, QuotaView, State};
use horch_core::runtime::RuntimeContext;
use horch_core::skills::{harness_support, SkillResolution};
use horch_core::workspace::herdr::Herdr;
use horch_core::workspace::model::Direction;
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
}

/// [`pane_command`], carrying a ledger record for the pane.
fn pane_command_for(
    ctx: &RuntimeContext,
    role: &str,
    kind: PaneKind,
    model: Option<&str>,
    record: Option<PaneRecord<'_>>,
    compete: Option<&CompeteSettings>,
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
    if let Some(record) = record {
        args.push("--record-id".to_string());
        args.push(record.record_id.to_string());
        if let Some(sid) = record.session_id {
            args.push("--session-id".to_string());
            args.push(sid.to_string());
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
    let rounds = compete.map(|c| c.rounds.to_string());
    let mut env: Vec<(&str, &str)> = vec![("HORCH_DATA_DIR", data_root.as_ref())];
    // Competition mode (FDS-13) reaches the briefing the same way.
    if let (Some(c), Some(rounds)) = (compete, rounds.as_deref()) {
        env.push((COMPETE_ROUNDS_ENV, rounds));
        env.push((COMPETE_BUDGET_ENV, c.budget_usd.as_str()));
    }
    Ok(PaneShell::host().command_line_with_env(&exe, &env, &args))
}

/// Environment variables that carry the competition settings into the
/// orchestrator pane, where `pane_launch` renders the briefing.
const COMPETE_ROUNDS_ENV: &str = "HORCH_COMPETE_ROUNDS";
const COMPETE_BUDGET_ENV: &str = "HORCH_COMPETE_BUDGET_USD";

/// Competition mode (FDS-13): what the briefing tells the orchestrator about
/// rounds. The rules live in the base `fleet-compete`; these are its 2 values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompeteSettings {
    /// At most this many rounds in 1 fleet session.
    pub rounds: u32,
    /// The hard ceiling of 1 round in US dollars, as the dataset CLI takes it
    /// for `--budget-usd`: a decimal string.
    pub budget_usd: String,
}

impl Default for CompeteSettings {
    fn default() -> Self {
        Self {
            rounds: 3,
            budget_usd: "10".to_string(),
        }
    }
}

impl CompeteSettings {
    /// The settings a pane inherits from [`COMPETE_ROUNDS_ENV`] and
    /// [`COMPETE_BUDGET_ENV`]. None when competition mode is off.
    fn from_env() -> Result<Option<Self>> {
        let (Ok(rounds), Ok(budget)) = (
            std::env::var(COMPETE_ROUNDS_ENV),
            std::env::var(COMPETE_BUDGET_ENV),
        ) else {
            return Ok(None);
        };
        let rounds = rounds
            .parse()
            .with_context(|| format!("{COMPETE_ROUNDS_ENV}='{rounds}' is not a number"))?;
        Ok(Some(Self {
            rounds,
            budget_usd: budget,
        }))
    }
}

/// Options of `horch fleet` beyond the flavor.
#[derive(Debug, Clone, Default)]
pub struct FleetOptions {
    /// Some: append the competition rules to the orchestrator's briefing.
    pub compete: Option<CompeteSettings>,
}

/// The briefing of a fleet orchestrator: the base briefing, then with
/// `compete` the rendered base `fleet-compete` after 1 blank line (FDS-13).
/// Without `compete` it is [`prompts::agent_prompt`] unchanged.
pub(crate) fn orchestrator_briefing(
    roster: &Roster,
    teammate: &Teammate,
    role: &str,
    compete: Option<&CompeteSettings>,
) -> Result<String> {
    let briefing = prompts::agent_prompt(roster, teammate, role)?;
    let Some(compete) = compete else {
        return Ok(briefing);
    };
    let base = roster
        .base("fleet-compete")
        .context("base 'fleet-compete' does not exist in _base/")?;
    let rounds = compete.rounds.to_string();
    let vars = BTreeMap::from([
        ("compete_rounds", rounds.as_str()),
        ("compete_budget_usd", compete.budget_usd.as_str()),
    ]);
    let rules = prompts::render(&base.body, &vars).context("base 'fleet-compete'")?;
    Ok(format!("{}\n\n{}", briefing.trim_end(), rules.trim_start()))
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
pub fn fleet(ctx: &RuntimeContext, cwd: Option<&str>, flavor: FleetFlavor) -> Result<()> {
    fleet_with(ctx, cwd, flavor, &FleetOptions::default())
}

/// [`fleet`] with `options`: the same launch, with competition mode when
/// `options.compete` is set.
pub fn fleet_with(
    ctx: &RuntimeContext,
    cwd: Option<&str>,
    flavor: FleetFlavor,
    options: &FleetOptions,
) -> Result<()> {
    doctor::check(ctx)?;
    let herdr = Herdr::with_bin(&ctx.bins.harness.herdr);
    let cwd = resolve_cwd(ctx, cwd)?;

    warn_about_missing_integrations(&herdr);

    let flavor = if flavor == FleetFlavor::Auto {
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
        chosen
    } else {
        flavor
    };

    println!("Creating fleet workspace rooted at {cwd}...");
    let label = workspace_label(Path::new(&cwd), ctx.paths.cwd.as_deref());
    let ws = herdr.workspace_create(&label, Some(&cwd), true)?;
    // The rest of this command works in the new workspace and its project.
    let mut ctx = ctx.clone();
    ctx.herdr.workspace = Some(horch_core::ids::WorkspaceId::new(ws.workspace_id.as_str())?);
    ctx.paths.project_dir = Some(PathBuf::from(&cwd));
    let ctx = &ctx;

    // The orchestrator is a ledger record like any worker (TEL-02), so its
    // spend is counted. Orchestrators of workspaces that are gone are
    // retired first: an orchestrator never runs `horch done`.
    let ledger = Ledger::open_in(ctx)?;
    if let Ok(open) = herdr.workspace_list() {
        let open: Vec<String> = open.into_iter().map(|w| w.workspace_id).collect();
        ledger.supersede_orchestrators(|ws| open.iter().any(|o| o == ws))?;
    }
    let kind = flavor.pane_kind();
    // The project's facts add its `skills_when` skills, here and at the
    // pane launch, so the record and the launch agree.
    let roster = super::load_roster(ctx, None)?.with_project_facts(project_facts(Path::new(&cwd)));
    let teammate = &roster.for_launch(roster.require(orchestrator_teammate(kind))?.clone());
    let record_id = horch_core::mint_uuid();
    let session_id = teammate
        .agent
        .capabilities()
        .caller_minted_session
        .then(horch_core::mint_uuid);
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
    ledger.insert(Record {
        record_id: record_id.clone(),
        session_id: session_id.clone(),
        agent: teammate.agent.as_str().to_string(),
        tier: teammate.name.clone(),
        model: flavor.model().to_string(),
        effort: teammate.effort.clone(),
        phase: teammate.phase,
        role: "orchestrator".to_string(),
        kind: KIND_ORCHESTRATOR.to_string(),
        task: ORCHESTRATING_TASK.to_string(),
        project: Some(cwd.clone()),
        workspace_id: Some(ws.workspace_id.clone()),
        skills: skills.activated,
        ..Record::default()
    })?;

    println!("Launching orchestrator ({})...", flavor.label());
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
            }),
            options.compete.as_ref(),
        )?,
    )?;

    // The telemetry space (SPC-06). Never a reason for the fleet to fail.
    if let Err(e) = super::telemetry::ensure(ctx, &herdr, true) {
        println!("warning: telemetry space not started: {e:#}");
    }

    println!(
        "\nDone. Fleet workspace {} is live with one orchestrator pane.",
        ws.workspace_id
    );
    println!("Session ledger: {}", Ledger::open_in(ctx)?.path().display());
    println!("Orchestrator commands: horch sessions | horch assign | horch spawn [--resume]");
    println!("Workers are spawned on demand: horch spawn <teammate> \"task\"");
    Ok(())
}

/// Which teammate file backs a fleet orchestrator pane of `kind`.
fn orchestrator_teammate(kind: PaneKind) -> &'static str {
    match kind {
        PaneKind::FleetCodexOrchestrator => "orchestrator-codex",
        _ => "orchestrator",
    }
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
        return None;
    }
    if let Ok(resolved) = std::fs::canonicalize(path) {
        return Some(resolved);
    }
    let absolute = current?.join(path);
    let mut resolved = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                resolved.pop();
            }
            other => resolved.push(other.as_os_str()),
        }
    }
    Some(resolved)
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
/// 1 orchestrator (Claude, Fable) + 4 workers (2x Claude Sonnet xhigh, 1x Claude
/// Opus xhigh, 1x Codex CLI), laid out as orchestrator-left with a 2x2 worker grid
/// on the right, wired for two-way messaging via `horch tell`.
pub fn orchestration(ctx: &RuntimeContext, cwd: Option<&str>) -> Result<()> {
    doctor::check(ctx)?;
    let herdr = Herdr::with_bin(&ctx.bins.harness.herdr);
    let cwd = resolve_cwd(ctx, cwd)?;

    println!("Creating workspace rooted at {cwd}...");
    let ws = herdr.workspace_create("Herdr Orchestration", Some(&cwd), true)?;

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

/// `horch pane-launch` - register this pane under a role and start its agent.
///
/// Replaces `scripts/herdr-fleet/orchestrator.sh`, `scripts/lib/herdr-worker.sh`,
/// and the five `scripts/herdr-orchestration/*.sh` launchers.
/// The ledger identity `horch fleet` gave an orchestrator pane.
#[derive(Debug, Clone, Default)]
pub struct PaneIdentity {
    pub record_id: Option<String>,
    pub session_id: Option<String>,
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
    let herdr = Herdr::with_bin(&ctx.bins.harness.herdr);
    let (mailbox, _pane) = Mailbox::register_in(&herdr, ctx, role)?;
    // What the agent inherits on top of this process's environment. A pane
    // is a fresh shell, so everything `horch fleet` knew travels here.
    let mut child_env: Vec<(String, String)> = vec![(
        "HORCH_WORKSPACE_ID".into(),
        mailbox.workspace_id().to_string(),
    )];
    if let Some(dir) = &identity.state_dir {
        ctx.paths.set_state_dir(dir);
        child_env.push(("HORCH_STATE_DIR".into(), dir.clone()));
    }
    // Like a worker, the orchestrator knows its own record.
    if let Some(record_id) = &identity.record_id {
        child_env.push(("HORCH_RECORD_ID".into(), record_id.clone()));
    }

    // Re-export it: the orchestrator runs `horch spawn` from this pane, and
    // those spawns must resolve the same roster this pane was launched with.
    if let Some(dir) = teammates_dir {
        child_env.push(("HORCH_TEAMMATES_DIR".into(), dir.to_string()));
    }
    // `HORCH_PROJECT_DIR`, when it names another directory, is where this
    // pane works. The process's working directory, not an environment
    // variable, so the agent and a relative roster path both see it.
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
        child_env.push(("PATH".into(), path.to_string_lossy().into_owned()));
    }
    let ctx: &RuntimeContext = ctx;
    // Which teammate file backs this pane. The model, effort level and
    // permission mode all come from that file - a pane kind selects a file, it
    // does not carry launch settings of its own.
    let mut roster = super::load_roster(ctx, teammates_dir)?;
    // A fleet orchestrator is offered only the teammates this project needs
    // (`offer_when`). The fixed recipe briefs no roster choice, so it keeps
    // the full list.
    if matches!(
        kind,
        PaneKind::FleetOrchestrator | PaneKind::FleetCodexOrchestrator
    ) {
        if let Ok(project) = ctx.paths.project() {
            roster = roster.with_project_facts(project_facts(&project));
        }
    }
    let name = match kind {
        PaneKind::FleetOrchestrator => "orchestrator",
        PaneKind::FleetCodexOrchestrator => "orchestrator-codex",
        PaneKind::OrchestrationOrchestrator => "orchestration-orchestrator",
        PaneKind::OrchestrationClaude | PaneKind::OrchestrationCodex => "orchestration-worker",
    };
    let mut teammate = roster.for_launch(roster.require(name)?.clone());
    // A fleet flavor passes the orchestrator's model; carry it on the teammate
    // too, so everything that reads the resolved teammate sees the model
    // actually launched, not the file's fallback.
    if matches!(
        kind,
        PaneKind::FleetOrchestrator | PaneKind::FleetCodexOrchestrator
    ) {
        if let Some(model) = model {
            teammate.model = Some(model.to_string());
        }
    }
    // The fixed 5-pane recipe assigns agent and model per pane, not per file.
    if kind == PaneKind::OrchestrationCodex {
        teammate.agent = HarnessKind::Codex;
        teammate.effort = None;
    }
    if matches!(
        kind,
        PaneKind::OrchestrationClaude | PaneKind::OrchestrationCodex
    ) && model.is_none()
    {
        anyhow::bail!("--model is required for an orchestration worker pane");
    }

    // Competition settings reach a fleet orchestrator only (FDS-13).
    let compete = match kind {
        PaneKind::FleetOrchestrator | PaneKind::FleetCodexOrchestrator => {
            CompeteSettings::from_env()?
        }
        _ => None,
    };
    let prompt = orchestrator_briefing(&roster, &teammate, role, compete.as_ref())?;
    // An orchestrator runs a different set of commands than a worker, and
    // codex refuses anything its execpolicy does not name. Install the set
    // this pane actually needs, not both.
    let exec_rules = match kind {
        PaneKind::FleetCodexOrchestrator => roster.orchestrator_exec_rules(),
        _ => roster.exec_rules(),
    };
    // A managed session when `horch fleet` minted an id (claude); a codex
    // orchestrator's id is discovered after launch, as a worker's is.
    let session = SessionMode::Fresh(identity.session_id.map(SessionId::new).transpose()?);
    let workdir = match &identity.record_id {
        Some(_) => Some(ctx.paths.project()?.to_string_lossy().into_owned()),
        None => None,
    };
    launch::run_flow(
        ctx,
        LaunchRequest {
            role,
            teammate: &teammate,
            session: &session,
            prompt: &prompt,
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
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use horch_core::roster::ProjectFacts;

    /// The orchestrator's briefing for `compete`, from the compiled-in roster.
    fn briefing(compete: Option<&CompeteSettings>) -> String {
        let roster = Roster::builtin().unwrap();
        let t = roster.require("orchestrator").unwrap();
        orchestrator_briefing(&roster, t, "orchestrator", compete).unwrap()
    }

    /// FDS-13: no compete settings, the briefing is the unchanged one.
    #[test]
    fn fds_13_briefing_without_compete_unchanged() {
        let roster = Roster::builtin().unwrap();
        let t = roster.require("orchestrator").unwrap();
        let before = prompts::agent_prompt(&roster, t, "orchestrator").unwrap();
        assert_eq!(briefing(None), before);
        assert!(!before.contains("== Competition mode =="));
        assert!(FleetOptions::default().compete.is_none());
    }

    /// FDS-13: compete settings append the rules, with both values rendered.
    #[test]
    fn fds_13_briefing_with_compete_rendered() {
        let plain = briefing(None);
        let text = briefing(Some(&CompeteSettings::default()));
        assert!(text.starts_with(plain.trim_end()), "base text comes first");
        assert!(text.contains("== Competition mode =="), "{text}");
        assert!(text.contains("--budget-usd 10"), "{text}");
        assert!(text.contains("fewer than 3 rounds"), "{text}");
        assert!(!text.contains("{compete_"), "{text}");
        let rules = text.split("== Competition mode ==").next().unwrap();
        assert!(rules.ends_with("\n\n"), "1 blank line before the rules");
        let custom = briefing(Some(&CompeteSettings {
            rounds: 5,
            budget_usd: "2.50".into(),
        }));
        assert!(custom.contains("--budget-usd 2.50"));
        assert!(custom.contains("fewer than 5 rounds"));
    }

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
        for (kind, name) in [
            (PaneKind::FleetOrchestrator, "orchestrator"),
            (PaneKind::FleetCodexOrchestrator, "orchestrator-codex"),
            (
                PaneKind::OrchestrationOrchestrator,
                "orchestration-orchestrator",
            ),
            (PaneKind::OrchestrationClaude, "orchestration-worker"),
        ] {
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
}
