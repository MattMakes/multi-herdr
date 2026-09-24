//! `horch fleet` and `horch orchestration` - the port of the justfile recipes and
//! the per-pane launcher scripts they ran.
//!
//! Both build a herdr workspace, lay out panes, and start an agent in each. The
//! difference is lifecycle: `orchestration` is a fixed 5-pane workspace, while
//! `fleet` starts one orchestrator alone, which then grows and shrinks the fleet
//! itself through the session ledger.

use std::path::{Component, Path, PathBuf};
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
}

impl FleetFlavor {
    pub fn as_str(self) -> &'static str {
        match self {
            FleetFlavor::Opus => "opus",
            FleetFlavor::Fable => "fable",
            FleetFlavor::Astra => "astra",
            FleetFlavor::Sol => "sol",
        }
    }

    /// What to print while the pane comes up.
    fn label(self) -> &'static str {
        match self {
            FleetFlavor::Opus => "Claude Code on Opus",
            FleetFlavor::Fable => "Claude Code on Fable",
            FleetFlavor::Astra => "Codex on Astra",
            FleetFlavor::Sol => "Codex on Sol",
        }
    }

    /// The model the orchestrator pane runs on. A Claude alias tracks the
    /// latest release; codex has no alias mechanism, so its slugs are literal.
    pub fn model(self) -> &'static str {
        match self {
            FleetFlavor::Opus => "opus",
            FleetFlavor::Fable => "fable",
            FleetFlavor::Astra => "gpt-6-astra",
            FleetFlavor::Sol => "gpt-5.6-sol",
        }
    }

    /// The teammate file this flavor's orchestrator pane is briefed from.
    fn pane_kind(self) -> PaneKind {
        match self {
            FleetFlavor::Opus | FleetFlavor::Fable => PaneKind::FleetOrchestrator,
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
            other => Err(format!(
                "unknown fleet flavor '{other}' (expected opus, fable, astra or sol; \
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
    let ws = herdr.workspace_create(&workspace_label(Path::new(&cwd)), Some(&cwd), true)?;
    std::env::set_var("HORCH_WORKSPACE_ID", &ws.workspace_id);
    std::env::set_var("HORCH_PROJECT_DIR", &cwd);

    let kind = flavor.pane_kind();
    println!("Launching orchestrator ({})...", flavor.label());
    herdr.pane_run(
        &ws.root_pane_id,
        &pane_command("orchestrator", kind, Some(flavor.model()))?,
    )?;

    println!(
        "\nDone. Fleet workspace {} is live with one orchestrator pane.",
        ws.workspace_id
    );
    println!("Session ledger: {}", Ledger::open()?.path().display());
    println!("Orchestrator commands: horch sessions | horch assign | horch spawn [--resume]");
    println!("Workers are spawned on demand: horch spawn <teammate> \"task\"");
    Ok(())
}

/// The fleet workspace is named after the project folder; a path with no final
/// component (such as `/`) keeps the old fixed label. The path is resolved
/// first, because `.` and `..` have no final component of their own.
fn workspace_label(cwd: &Path) -> String {
    resolved_path(cwd)
        .as_deref()
        .and_then(Path::file_name)
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "Herdr Fleet".to_string())
}

/// `path` as an absolute path with no `.` or `..` in it: `canonicalize` when the
/// path exists, otherwise the current directory joined with it and normalised by
/// hand. An empty path resolves to nothing rather than to the current directory.
fn resolved_path(path: &Path) -> Option<PathBuf> {
    if path.as_os_str().is_empty() {
        return None;
    }
    if let Ok(resolved) = std::fs::canonicalize(path) {
        return Some(resolved);
    }
    let absolute = std::env::current_dir().ok()?.join(path);
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

    /// `herdr-fleet` takes the model name or the CLI name, and a typo is
    /// refused rather than quietly defaulted.
    #[test]
    fn fleet_flavors_parse_from_what_a_user_types() {
        for (words, flavor) in [
            (&["opus", "cc", "claude", "CC", "Opus"][..], FleetFlavor::Opus),
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
            (FleetFlavor::Astra, PaneKind::FleetCodexOrchestrator, "gpt-6-astra"),
            (FleetFlavor::Sol, PaneKind::FleetCodexOrchestrator, "gpt-5.6-sol"),
        ] {
            assert_eq!(flavor.pane_kind(), kind, "{flavor}");
            assert_eq!(flavor.model(), model, "{flavor}");
            let cmd =
                pane_command("orchestrator", flavor.pane_kind(), Some(flavor.model())).unwrap();
            assert!(cmd.contains(kind.as_str()), "{cmd}");
            assert!(cmd.contains(model), "{cmd}");
        }
    }

    /// Opus and Sol are not reserved tiers, so an orchestrator on either can
    /// still hand work to `opus` / `codex-sol` workers; Fable and Astra stay
    /// out of every worker's reach whichever flavor runs.
    #[test]
    fn only_fable_and_astra_orchestrators_hold_a_reserved_tier() {
        use horch_core::teammates::reserved_tier;
        assert!(reserved_tier(FleetFlavor::Fable.model()).is_some());
        assert!(reserved_tier(FleetFlavor::Astra.model()).is_some());
        assert!(reserved_tier(FleetFlavor::Opus.model()).is_none());
        assert!(reserved_tier(FleetFlavor::Sol.model()).is_none());
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

    #[test]
    fn workspace_label_is_the_project_folder_name() {
        assert_eq!(
            workspace_label(Path::new("/Users/me/projects/multi-herdr")),
            "multi-herdr"
        );
    }

    #[test]
    fn workspace_label_ignores_a_trailing_slash() {
        assert_eq!(
            workspace_label(Path::new("/Users/me/projects/multi-herdr/")),
            "multi-herdr"
        );
    }

    #[test]
    fn workspace_label_falls_back_when_the_path_has_no_folder_name() {
        assert_eq!(workspace_label(Path::new("/")), "Herdr Fleet");
        assert_eq!(workspace_label(Path::new("")), "Herdr Fleet");
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
        assert_eq!(workspace_label(Path::new(".")), folder_name(&current_dir()));
    }

    #[test]
    fn workspace_label_resolves_dot_dot_to_the_parent_directory_name() {
        let parent = current_dir().parent().unwrap().to_path_buf();
        assert_eq!(workspace_label(Path::new("..")), folder_name(&parent));
    }

    /// These paths do not exist, so `canonicalize` fails and the label helper
    /// has to normalise `.` and `..` itself.
    #[test]
    fn workspace_label_normalises_a_path_that_does_not_exist() {
        assert_eq!(workspace_label(Path::new("some/dir/.")), "dir");
        assert_eq!(
            workspace_label(Path::new("no-such-dir/..")),
            folder_name(&current_dir())
        );
        assert_eq!(
            workspace_label(Path::new("/Users/me/projects/multi-herdr/../other")),
            "other"
        );
    }

    #[test]
    fn workspace_label_names_a_real_directory_however_it_is_spelled() {
        let tmp = tempfile::tempdir().unwrap();
        let project = tmp.path().join("my-project");
        std::fs::create_dir_all(project.join("sub")).unwrap();

        assert_eq!(workspace_label(&project), "my-project");
        assert_eq!(workspace_label(&project.join(".")), "my-project");
        assert_eq!(
            workspace_label(&project.join("sub").join("..")),
            "my-project"
        );
    }
}
