//! `horch fleet` and `horch orchestration` - the port of the justfile recipes and
//! the per-pane launcher scripts they ran.
//!
//! Both build a herdr workspace, lay out panes, and start an agent in each. The
//! difference is lifecycle: `orchestration` is a fixed 5-pane workspace, while
//! `fleet` starts one orchestrator alone, which then grows and shrinks the fleet
//! itself through the session ledger.

use std::path::PathBuf;
use std::process::{Command, ExitCode};

use anyhow::{Context, Result};
use horch_core::agent;
use horch_core::codex;
use horch_core::herdr::{Direction, Herdr};
use horch_core::launch::{self, Session};
use horch_core::ledger::Ledger;
use horch_core::mailbox::Mailbox;
use horch_core::paneshell::PaneShell;
use horch_core::prompts;
use horch_core::teammates::{Agent, Roster};

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

/// Which agent orchestrates a fleet.
///
/// Only the ORCHESTRATOR pane differs. The roster it spawns workers from is the
/// same either way, so a Codex orchestrator still reaches for `opus` when a task
/// wants Claude, and a Claude one still reaches for `codex-sol`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FleetFlavor {
    /// Claude Code, on Fable.
    #[default]
    Claude,
    /// Codex, on Astra.
    Codex,
}

impl FleetFlavor {
    pub fn as_str(self) -> &'static str {
        match self {
            FleetFlavor::Claude => "cc",
            FleetFlavor::Codex => "codex",
        }
    }

    /// What to print while the pane comes up. The tier is the useful half: it
    /// is what the fleet has exactly one of.
    fn label(self) -> &'static str {
        match self {
            FleetFlavor::Claude => "Claude Code on Fable",
            FleetFlavor::Codex => "Codex on Astra",
        }
    }

    /// The teammate file this flavor's orchestrator pane is briefed from.
    fn pane_kind(self) -> PaneKind {
        match self {
            FleetFlavor::Claude => PaneKind::FleetOrchestrator,
            FleetFlavor::Codex => PaneKind::FleetCodexOrchestrator,
        }
    }
}

impl std::str::FromStr for FleetFlavor {
    type Err = String;

    /// The model names are accepted alongside the CLI names, because that is
    /// how the choice is actually discussed: "the Fable one" or "the Astra one".
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "cc" | "claude" | "fable" => Ok(FleetFlavor::Claude),
            "codex" | "astra" => Ok(FleetFlavor::Codex),
            other => Err(format!(
                "unknown fleet flavor '{other}' (expected cc for Claude Code on Fable, \
                 or codex for Codex on Astra)"
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
fn resolve_cwd(cwd: Option<&str>) -> Result<String> {
    let dir = match cwd {
        Some(c) => PathBuf::from(c),
        None => std::env::current_dir().context("resolving the current directory")?,
    };
    Ok(dir.to_string_lossy().into_owned())
}

/// The command line that turns a pane into `kind`.
fn pane_command(role: &str, kind: PaneKind, model: Option<&str>) -> Result<String> {
    let exe = std::env::current_exe().context("locating the horch binary")?;
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
    if let Ok(dir) = std::env::var("HORCH_TEAMMATES_DIR") {
        if !dir.is_empty() {
            args.push("--teammates-dir".to_string());
            args.push(dir);
        }
    }
    Ok(PaneShell::host().command_line(&exe, &args))
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
pub fn fleet(cwd: Option<&str>, flavor: FleetFlavor) -> Result<()> {
    doctor::check()?;
    let herdr = Herdr::new();
    let cwd = resolve_cwd(cwd)?;

    warn_about_missing_integrations(&herdr);

    println!("Creating fleet workspace rooted at {cwd}...");
    let ws = herdr.workspace_create("Herdr Fleet", Some(&cwd), true)?;
    std::env::set_var("HORCH_WORKSPACE_ID", &ws.workspace_id);
    std::env::set_var("HORCH_PROJECT_DIR", &cwd);

    let kind = flavor.pane_kind();
    println!("Launching orchestrator ({})...", flavor.label());
    herdr.pane_run(&ws.root_pane_id, &pane_command("orchestrator", kind, None)?)?;

    println!(
        "\nDone. Fleet workspace {} is live with one orchestrator pane.",
        ws.workspace_id
    );
    println!("Session ledger: {}", Ledger::open()?.path().display());
    println!("Orchestrator commands: horch sessions | horch assign | horch spawn [--resume]");
    println!("Workers are spawned on demand: horch spawn <teammate> \"task\"");
    Ok(())
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
pub fn orchestration(cwd: Option<&str>) -> Result<()> {
    doctor::check()?;
    let herdr = Herdr::new();
    let cwd = resolve_cwd(cwd)?;

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
        herdr.pane_run(pane, &pane_command(role, kind, model)?)?;
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
pub fn pane_launch(
    role: &str,
    kind: PaneKind,
    model: Option<&str>,
    teammates_dir: Option<&str>,
) -> Result<ExitCode> {
    let herdr = Herdr::new();
    Mailbox::register(&herdr, role)?;

    // Re-export it: the orchestrator runs `horch spawn` from this pane, and
    // those spawns must resolve the same roster this pane was launched with.
    if let Some(dir) = teammates_dir {
        std::env::set_var("HORCH_TEAMMATES_DIR", dir);
    }
    if let Ok(project) = std::env::var("HORCH_PROJECT_DIR") {
        if !project.is_empty() {
            let _ = std::env::set_current_dir(&project);
        }
    }
    // Both orchestrator and worker briefings name `horch` as an on-PATH command.
    if let Err(e) = agent::prepend_own_dir_to_path() {
        eprintln!("horch pane-launch[{role}]: could not add horch to PATH: {e}");
    }
    // Which teammate file backs this pane. The model, effort level and
    // permission mode all come from that file - a pane kind selects a file, it
    // does not carry launch settings of its own.
    let roster = Roster::load_with(teammates_dir)?;
    let name = match kind {
        PaneKind::FleetOrchestrator => "orchestrator",
        PaneKind::FleetCodexOrchestrator => "orchestrator-codex",
        PaneKind::OrchestrationOrchestrator => "orchestration-orchestrator",
        PaneKind::OrchestrationClaude | PaneKind::OrchestrationCodex => "orchestration-worker",
    };
    let mut teammate = roster.require(name)?.clone();
    // The fixed 5-pane recipe assigns agent and model per pane, not per file.
    if kind == PaneKind::OrchestrationCodex {
        teammate.agent = Agent::Codex;
        teammate.effort = None;
    }
    if matches!(
        kind,
        PaneKind::OrchestrationClaude | PaneKind::OrchestrationCodex
    ) && model.is_none()
    {
        anyhow::bail!("--model is required for an orchestration worker pane");
    }

    let prompt = prompts::agent_prompt(&roster, &teammate, role)?;
    launch::apply_env(&teammate);
    let skills = horch_core::skills::Bundle::install(&horch_core::ledger::state_root(), &teammate)?;
    let rules = if teammate.agent.uses_execpolicy() {
        // An orchestrator runs a different set of commands than a worker, and
        // codex refuses anything its execpolicy does not name. Install the set
        // this pane actually needs, not both.
        let needed = match kind {
            PaneKind::FleetCodexOrchestrator => roster.orchestrator_exec_rules(),
            _ => roster.exec_rules(),
        };
        Some(codex::Rules::install(&agent::home_dir(), role, needed)?)
    } else {
        None
    };

    let mut cmd = launch::command_with_skills(
        &teammate,
        Session::Unmanaged,
        &prompt,
        model,
        skills.as_ref(),
    )?;
    if let Some(rules) = &rules {
        if let Some(skills) = &skills {
            rules.attach_skills(&skills.skills_dir())?;
        }
        rules.apply(&mut cmd);
    }
    let name = format!("{:?}", cmd.get_program());
    let code = run(cmd, name.trim_matches('"'));
    if let Some(rules) = rules {
        rules.finish();
    }
    code
}

fn run(mut cmd: Command, name: &str) -> Result<ExitCode> {
    let status = cmd.status().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            anyhow::anyhow!(
                "agent CLI '{name}' not found on PATH. Set HORCH_CLAUDE_BIN or \
                 HORCH_CODEX_BIN to point at it."
            )
        } else {
            anyhow::Error::new(e).context(format!("launching {name}"))
        }
    })?;
    Ok(match status.code() {
        Some(0) => ExitCode::SUCCESS,
        Some(code) => ExitCode::from(code.clamp(1, 255) as u8),
        None => ExitCode::FAILURE,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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

    /// `herdr-fleet` picks the Claude/Fable orchestrator, `herdr-fleet codex`
    /// the Codex/Astra one, and a typo is refused rather than quietly defaulted.
    #[test]
    fn fleet_flavors_parse_from_what_a_user_types() {
        for word in ["cc", "claude", "fable", "CC", "Claude"] {
            assert_eq!(
                word.parse::<FleetFlavor>().unwrap(),
                FleetFlavor::Claude,
                "{word}"
            );
        }
        for word in ["codex", "astra", "CODEX"] {
            assert_eq!(
                word.parse::<FleetFlavor>().unwrap(),
                FleetFlavor::Codex,
                "{word}"
            );
        }
        assert_eq!(
            FleetFlavor::default(),
            FleetFlavor::Claude,
            "a bare `fleet` is Fable"
        );
        let err = "opus".parse::<FleetFlavor>().unwrap_err();
        assert!(err.contains("unknown fleet flavor 'opus'"), "{err}");
    }

    /// Each flavor must reach its own teammate file, or `horch fleet codex`
    /// silently launches the Claude orchestrator.
    #[test]
    fn each_flavor_launches_its_own_orchestrator_pane() {
        assert_eq!(FleetFlavor::Claude.pane_kind(), PaneKind::FleetOrchestrator);
        assert_eq!(
            FleetFlavor::Codex.pane_kind(),
            PaneKind::FleetCodexOrchestrator
        );

        let cmd = pane_command("orchestrator", FleetFlavor::Codex.pane_kind(), None).unwrap();
        assert!(cmd.contains("fleet-codex-orchestrator"), "{cmd}");
    }

    /// The teammate each pane kind names must actually exist, or the pane comes
    /// up, fails to resolve its briefing, and dies where nobody is watching.
    #[test]
    fn every_pane_kind_names_a_teammate_that_exists() {
        let roster = Roster::builtin().unwrap();
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
        let with = pane_command("sonnet-1", PaneKind::OrchestrationClaude, Some("sonnet")).unwrap();
        assert!(with.contains("--model"), "{with}");
        assert!(with.contains("sonnet-1"), "{with}");

        let without = pane_command("codex-1", PaneKind::OrchestrationCodex, None).unwrap();
        assert!(!without.contains("--model"), "{without}");
    }
}
