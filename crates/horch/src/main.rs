//! `horch` - multi-agent orchestration layouts for herdr (https://herdr.dev).
//!
//! One binary replacing the justfile recipes, the `bin/horch-*` and `bin/herdr-*`
//! scripts, the launcher shell scripts, and the Node installer. Needs no bash,
//! jq, node, or just: the only external process is `herdr` itself, plus whichever
//! agent CLI a worker pane launches.

mod cmd;
mod output;

use anyhow::Result;
use clap::{Parser, Subcommand};
use horch_core::herdr::Direction;

#[derive(Parser)]
#[command(
    name = "horch",
    version,
    about = "Multi-agent orchestration layouts for herdr",
    long_about = "Multi-agent orchestration layouts for herdr (https://herdr.dev).\n\n\
                  Every recipe needs a running herdr server (launch the herdr app, or\n\
                  `herdr server` headless) and works from any terminal.",
    subcommand_required = true,
    arg_required_else_help = true
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Launch the herdr-fleet workspace: ONE orchestrator pane, which spawns
    /// exactly the workers the work needs, with a per-project session ledger.
    Fleet {
        /// Who orchestrates: `cc` for Claude Code on Fable (the default), or
        /// `codex` for Codex on Astra. Only the orchestrator pane changes; both
        /// spawn workers from the same roster.
        #[arg(value_name = "FLAVOR", default_value = "cc")]
        flavor: cmd::recipes::FleetFlavor,
        /// Project directory the fleet works in. Defaults to the current directory.
        #[arg(long)]
        cwd: Option<String>,
    },

    /// Launch the fixed 5-pane orchestration workspace: 1 orchestrator (Fable) +
    /// 2x Sonnet, 1x Opus, 1x Codex, wired for two-way messaging.
    Orchestration {
        /// Project directory. Defaults to the current directory.
        #[arg(long)]
        cwd: Option<String>,
    },

    /// Send a message into another pane's terminal.
    Tell {
        /// Registered role to deliver to, e.g. orchestrator or sonnet-1.
        role: String,
        /// The message. Joined with spaces.
        #[arg(required = true, trailing_var_arg = true)]
        message: Vec<String>,
    },

    /// List the roles registered in this workspace.
    Inbox,

    /// Give a task to a live worker: record it on the ledger, then deliver it.
    Assign {
        role: String,
        #[arg(required = true, trailing_var_arg = true)]
        task: Vec<String>,
    },

    /// Append a progress note to this worker's own ledger record.
    Note {
        #[arg(required = true, trailing_var_arg = true)]
        text: Vec<String>,
    },

    /// Finish this worker: record a summary, report DONE, and close its pane.
    Done {
        #[arg(required = true, trailing_var_arg = true)]
        summary: Vec<String>,
    },

    /// Read the project session ledger. Do this before spawning.
    Sessions {
        /// Print the raw ledger JSON instead of the readable summary.
        #[arg(long)]
        json: bool,
    },

    /// Inspect, validate, or scaffold the roster in `teammates/`.
    Teammates {
        /// Machine-readable dump of every loaded teammate.
        #[arg(long)]
        json: bool,
        /// Validate instead of listing. Exits non-zero when something is wrong.
        #[arg(long)]
        check: bool,
        /// Scaffold `<NAME>.md` from `_template.md`.
        #[arg(long, value_name = "NAME")]
        new: Option<String>,
        /// Directory for --new. Defaults to $HORCH_TEAMMATES_DIR.
        #[arg(long, value_name = "DIR")]
        dir: Option<String>,
    },

    /// Create a pane running a worker agent, fresh or resuming a ledger session.
    ///
    /// Prints the new pane id on stdout so callers can chain splits.
    #[command(override_usage = "horch spawn <TEAMMATE> [TASK]\n       \
                                horch spawn --resume <ID> [TASK]")]
    Spawn {
        /// `<teammate> [task]`, or just `[task]` when using --resume.
        #[arg(value_name = "TEAMMATE|TASK")]
        args: Vec<String>,
        /// Resume a previous session by session or record id.
        #[arg(long, value_name = "ID")]
        resume: Option<String>,
        /// Select research, plan, implementation, or validation skills.
        #[arg(long)]
        phase: Option<horch_core::teammates::Phase>,
        /// Override the auto role name (<teammate>-<n>).
        #[arg(long, value_name = "NAME")]
        role: Option<String>,
        /// Pane to split. Defaults to the calling pane.
        #[arg(long, value_name = "PANE")]
        from_pane: Option<String>,
        /// Which way to split.
        #[arg(long, default_value = "right")]
        direction: Direction,
    },

    /// Report the worker grid and the next split that keeps it 2 rows by N.
    Layout {
        #[arg(long, value_name = "ID")]
        pane: Option<String>,
        #[arg(long, value_name = "ID")]
        workspace: Option<String>,
    },

    /// Make every worker column the same width.
    ///
    /// `horch layout` compares the two rows' exact pane edges, so drifted columns
    /// read as ragged and it advises a split that makes the grid worse. Spawning
    /// and `horch done` do this automatically; run it by hand to repair a grid
    /// that was resized or dragged.
    Balance {
        #[arg(long, value_name = "ID")]
        pane: Option<String>,
        #[arg(long, value_name = "ID")]
        workspace: Option<String>,
        /// Print the resizes that would run, without touching the layout.
        #[arg(long)]
        dry_run: bool,
        /// Wait this long before reading the layout. `horch done` uses it to let
        /// its own pane finish closing before the grid is measured.
        #[arg(long, value_name = "MS", default_value_t = 0, hide = true)]
        settle_ms: u64,
    },

    /// Low-level session ledger access.
    Ledger {
        #[command(subcommand)]
        command: cmd::ledgercmd::LedgerCommand,
    },

    /// Inspect the integrated skill catalog and estimated context cost.
    Skills {
        #[arg(long)]
        phase: Option<horch_core::teammates::Phase>,
        /// Emit machine-readable JSON (also the default catalog format).
        #[arg(long)]
        json: bool,
    },

    /// Check that herdr is installed and its server is reachable.
    Doctor,

    /// Copy this binary somewhere on your PATH.
    Install {
        /// Install directory. Defaults to ~/.local/bin (%LOCALAPPDATA%\Programs\horch on Windows).
        #[arg(long, value_name = "DIR")]
        dir: Option<String>,
    },

    /// Self-verifying checks of the machinery. Run these after installing.
    Smoke {
        #[command(subcommand)]
        command: cmd::smoke::SmokeCommand,
    },

    /// Run a worker agent in the current pane. Used by `horch spawn`.
    #[command(hide = true)]
    Worker { role: String },

    /// Turn the current pane into an orchestrator or a fixed-recipe worker.
    /// Used by `horch fleet` and `horch orchestration`.
    #[command(hide = true)]
    PaneLaunch {
        #[arg(long)]
        role: String,
        #[arg(long)]
        kind: cmd::recipes::PaneKind,
        /// Model alias, required for claude panes.
        #[arg(long)]
        model: Option<String>,
        /// Roster directory. Passed explicitly because a herdr pane is a fresh
        /// shell and does not inherit $HORCH_TEAMMATES_DIR.
        #[arg(long, value_name = "DIR")]
        teammates_dir: Option<String>,
    },

    /// Register the current pane under a role. Used by the smoke checks.
    #[command(hide = true)]
    Register { role: String },
}

/// Join a trailing var-arg list the way `"$*"` did.
fn joined(parts: &[String]) -> String {
    parts.join(" ")
}

fn main() -> std::process::ExitCode {
    match run() {
        Ok(code) => code,
        Err(e) => {
            eprintln!("horch: {e:#}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn run() -> Result<std::process::ExitCode> {
    let cli = Cli::parse();
    match cli.command {
        Command::Fleet { cwd, flavor } => cmd::recipes::fleet(cwd.as_deref(), flavor)?,
        Command::Orchestration { cwd } => cmd::recipes::orchestration(cwd.as_deref())?,
        Command::Tell { role, message } => cmd::messaging::tell(&role, &joined(&message))?,
        Command::Inbox => cmd::messaging::inbox()?,
        Command::Assign { role, task } => cmd::messaging::assign(&role, &joined(&task))?,
        Command::Note { text } => cmd::messaging::note(&joined(&text))?,
        Command::Done { summary } => cmd::messaging::done(&joined(&summary))?,
        Command::Sessions { json } => cmd::ledgercmd::sessions(json)?,
        Command::Skills { phase, json } => {
            let catalog = horch_core::skills::describe(phase)?;
            if json {
                output::println(&serde_json::to_string_pretty(&catalog)?);
            } else {
                output::println(&format!(
                    "Phase: {}",
                    phase.map(|p| p.to_string()).unwrap_or_else(|| "all".into())
                ));
                for skill in catalog["skills"].as_array().expect("catalog skills array") {
                    output::println(&format!(
                        "  {:<18} {}",
                        skill["name"].as_str().unwrap(),
                        skill["description"].as_str().unwrap()
                    ));
                }
                output::println(&format!(
                    "Metadata: {} bytes (~{} tokens, estimate only); workflows load on demand.",
                    catalog["metadata_bytes"], catalog["metadata_tokens_estimate"]
                ));
            }
        }
        Command::Spawn {
            args,
            resume,
            phase,
            role,
            from_pane,
            direction,
        } => {
            let (teammate, task) = cmd::spawn::resolve_positionals(&args, resume.as_deref())?;
            let pane = cmd::spawn::spawn(cmd::spawn::SpawnArgs {
                teammate,
                task,
                resume,
                phase,
                role,
                from_pane,
                direction,
            })?;
            output::println(&pane);
        }
        Command::Teammates {
            json,
            check,
            new,
            dir,
        } => {
            if let Some(name) = new {
                cmd::teammatescmd::new(&name, dir.as_deref())?;
            } else if check {
                return Ok(cmd::teammatescmd::check()?);
            } else {
                cmd::teammatescmd::list(json)?;
            }
        }
        Command::Layout { pane, workspace } => {
            cmd::layoutcmd::layout(pane.as_deref(), workspace.as_deref())?
        }
        Command::Balance {
            pane,
            workspace,
            dry_run,
            settle_ms,
        } => cmd::balancecmd::balance(pane.as_deref(), workspace.as_deref(), dry_run, settle_ms)?,
        Command::Ledger { command } => return cmd::ledgercmd::run(command),
        Command::Doctor => cmd::doctor::doctor()?,
        Command::Install { dir } => cmd::install::install(dir.as_deref())?,
        Command::Smoke { command } => return cmd::smoke::run(command),
        Command::Worker { role } => return cmd::worker::worker(&role),
        Command::PaneLaunch {
            role,
            kind,
            model,
            teammates_dir,
        } => {
            return cmd::recipes::pane_launch(
                &role,
                kind,
                model.as_deref(),
                teammates_dir.as_deref(),
            )
        }
        Command::Register { role } => cmd::messaging::register(&role)?,
    }
    Ok(std::process::ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_definition_is_valid() {
        Cli::command().debug_assert();
    }

    /// `horch tell sonnet-1 fix the bug` must behave like the old
    #[test]
    fn spawn_accepts_phase_for_fresh_and_resumed_workers() {
        for argv in [
            vec!["horch", "spawn", "codex-sol", "--phase", "research"],
            vec!["horch", "spawn", "--resume", "r1", "--phase", "research"],
        ] {
            match Cli::try_parse_from(argv).unwrap().command {
                Command::Spawn { phase, .. } => {
                    assert_eq!(phase, Some(horch_core::teammates::Phase::Research))
                }
                _ => panic!("wrong command"),
            }
        }
        assert!(Cli::try_parse_from(["horch", "spawn", "opus", "--phase", "invalid"]).is_err());
    }

    /// `herdr-tell sonnet-1 fix the bug`, which joined "$*".
    #[test]
    fn tell_joins_its_trailing_words() {
        let cli = Cli::try_parse_from(["horch", "tell", "sonnet-1", "fix", "the", "bug"]).unwrap();
        match cli.command {
            Command::Tell { role, message } => {
                assert_eq!(role, "sonnet-1");
                assert_eq!(joined(&message), "fix the bug");
            }
            _ => panic!("wrong subcommand"),
        }
    }

    /// A quoted single argument must survive intact.
    #[test]
    fn tell_preserves_a_single_quoted_message() {
        let cli = Cli::try_parse_from(["horch", "tell", "orchestrator", "[a] DONE: x, y"]).unwrap();
        match cli.command {
            Command::Tell { message, .. } => assert_eq!(joined(&message), "[a] DONE: x, y"),
            _ => panic!("wrong subcommand"),
        }
    }

    #[test]
    fn tell_requires_a_message() {
        assert!(Cli::try_parse_from(["horch", "tell", "sonnet-1"]).is_err());
    }

    /// Walk the whole path a user types through to a resolved (teammate, task).
    fn parse_spawn(argv: &[&str]) -> (Option<String>, String, Option<String>, Direction) {
        let cli = Cli::try_parse_from(argv).expect("should parse");
        match cli.command {
            Command::Spawn {
                args,
                resume,
                direction,
                ..
            } => {
                let (teammate, task) = cmd::spawn::resolve_positionals(&args, resume.as_deref())
                    .expect("should resolve");
                (teammate, task, resume, direction)
            }
            _ => panic!("wrong subcommand"),
        }
    }

    #[test]
    fn spawn_parses_a_teammate_and_task() {
        let (teammate, task, resume, direction) =
            parse_spawn(&["horch", "spawn", "codex-sol", "do the thing"]);
        assert_eq!(teammate.as_deref(), Some("codex-sol"));
        assert_eq!(task, "do the thing");
        assert!(resume.is_none());
        assert_eq!(direction, Direction::Right, "right is the default");
    }

    /// `horch spawn --resume <id> "<task>"` is the form the orchestrator briefing
    /// documents; the task must not be mistaken for a teammate.
    #[test]
    fn spawn_parses_a_resume_key_without_a_teammate() {
        let (teammate, task, resume, _) =
            parse_spawn(&["horch", "spawn", "--resume", "s-1", "continue"]);
        assert!(teammate.is_none());
        assert_eq!(resume.as_deref(), Some("s-1"));
        assert_eq!(task, "continue");
    }

    /// Teammate names are no longer a closed enum, so an unknown one cannot be
    /// caught while parsing argv. It is rejected against the loaded roster, and
    /// the error names what is actually available on this machine.
    #[test]
    fn spawn_rejects_an_unknown_teammate_against_the_roster() {
        let cli = Cli::try_parse_from(["horch", "spawn", "haiku"]).unwrap();
        match cli.command {
            Command::Spawn { args, resume, .. } => {
                let (name, _) = cmd::spawn::resolve_positionals(&args, resume.as_deref()).unwrap();
                let roster = horch_core::teammates::Roster::builtin().unwrap();
                let err = roster
                    .require(name.as_deref().unwrap())
                    .unwrap_err()
                    .to_string();
                assert!(err.contains("unknown teammate 'haiku'"), "{err}");
                assert!(
                    err.contains("sonnet"),
                    "error should list what is available: {err}"
                );
            }
            _ => panic!("wrong subcommand"),
        }
    }

    #[test]
    fn spawn_rejects_an_unknown_direction() {
        assert!(Cli::try_parse_from(["horch", "spawn", "sonnet", "--direction", "left"]).is_err());
    }

    #[test]
    fn bare_horch_shows_help_rather_than_doing_something() {
        let err = Cli::try_parse_from(["horch"])
            .err()
            .expect("bare `horch` must not resolve to a command");
        assert_eq!(
            err.kind(),
            clap::error::ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
        );
    }
}
