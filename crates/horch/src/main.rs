//! `horch` - multi-agent orchestration layouts for herdr (https://herdr.dev).
//!
//! One binary replacing the justfile recipes, the `bin/horch-*` and `bin/herdr-*`
//! scripts, the launcher shell scripts, and the Node installer. Needs no bash,
//! jq, node, or just: the only external process is `herdr` itself, plus whichever
//! agent CLI a worker pane launches.

mod cmd;

use anyhow::Result;
use clap::{Parser, Subcommand};
use horch::{bootstrap, exit, output};
use horch_core::execution::service::SpawnError;
use horch_core::execution::TilingMode;
use horch_core::workspace::model::Direction;

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
        /// Who orchestrates, by model: `opus` (the default; also `cc`/`claude`)
        /// or `fable` for Claude Code, `astra` (also `codex`) or `sol` for
        /// Codex, or `auto`: Opus or Sol, whichever usage pool can serve it.
        /// Only the orchestrator pane changes; all of them spawn workers from
        /// the same roster.
        #[arg(value_name = "FLAVOR", default_value = "opus")]
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
        /// Also list competition candidates and judges (multi-herdr-dataset).
        #[arg(long)]
        all: bool,
    },

    /// Inspect, validate, or scaffold the roster in `teammates/`.
    Teammates {
        /// Machine-readable dump of every loaded teammate.
        #[arg(long)]
        json: bool,
        /// Validate instead of listing. Exits non-zero when something is wrong.
        #[arg(long)]
        check: bool,
        /// One row per teammate: harness, model, effort, phase, expected
        /// skills, price. The "what is configured" half of retuning; `horch
        /// cost` is the "what it cost" half. Combine with --json.
        #[arg(long)]
        matrix: bool,
        /// Scaffold `<NAME>.md` from `_template.md`.
        #[arg(long, value_name = "NAME")]
        new: Option<String>,
        /// Directory for --new. Defaults to $HORCH_TEAMMATES_DIR.
        #[arg(long, value_name = "DIR")]
        dir: Option<String>,
    },

    /// What this project's fleet cost, per worker, read from each harness's
    /// transcripts; plus which expected skills each worker never loaded.
    ///
    /// Prices every ledger session (claude, codex, pi, prime) at the built-in
    /// per-MTok table. Sessions that cannot be priced are listed, never zeroed.
    Cost {
        /// Machine-readable report.
        #[arg(long)]
        json: bool,
        /// Add a what-if column: the same tokens at this model's prices.
        #[arg(long, value_name = "MODEL")]
        reprice: Option<String>,
        /// JSON file of per-MTok prices to use over the built-in table:
        /// {"<model>": {"input": 4, "output": 20, "cache_read": 0.2}}.
        #[arg(long, value_name = "FILE")]
        pricing: Option<String>,
        /// Only sessions created at or after this UTC time (e.g. 2026-09-24).
        #[arg(long, value_name = "TIMESTAMP")]
        since: Option<String>,
        /// Only this record or session id. Repeatable.
        #[arg(long = "record", value_name = "ID")]
        records: Vec<String>,
        /// Also price a session outside the ledger, e.g. the orchestrator's:
        /// `claude:<session-id>`. Repeatable.
        #[arg(long = "session", value_name = "AGENT:ID")]
        sessions: Vec<String>,
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
        /// Override the teammate's effort for this one spawn (claude: low..max;
        /// codex: none..max; pi/prime: off..max). Validated per agent. A resume
        /// keeps the level it ran at unless this is given.
        #[arg(long, value_name = "LEVEL")]
        effort: Option<String>,
        /// Override the auto role name (<teammate>-<n>).
        #[arg(long, value_name = "NAME")]
        role: Option<String>,
        /// Pane to split. Defaults to the calling pane.
        #[arg(long, value_name = "PANE")]
        from_pane: Option<String>,
        /// Which way to split.
        #[arg(long, default_value = "right")]
        direction: Direction,
        /// Leave the grid alone after spawning. Same as HORCH_TILE=0.
        #[arg(long = "no-tile")]
        untiled: bool,
        /// Never run a fallback teammate when this one's usage pool is short.
        #[arg(long)]
        exact: bool,
        /// Spawn even when every usage pool that could serve it is exhausted.
        #[arg(long)]
        force: bool,
    },

    /// Where the tokens went: every pane on this machine, from the telemetry
    /// event store. Runs one collector tick first when no collector is live.
    Usage {
        #[arg(long)]
        json: bool,
        /// Only events at or after this time (RFC 3339, or a date).
        #[arg(long, value_name = "TIME")]
        since: Option<String>,
        /// Only this project (its absolute path).
        #[arg(long, value_name = "PATH")]
        project: Option<String>,
        /// Group by teammate, phase, agent, project, plan or kind.
        #[arg(long, default_value = "teammate")]
        by: String,
        /// Only the last 5h, today, or the last 7d.
        #[arg(long)]
        window: Option<String>,
    },

    /// The usage pools (claude, codex, opencode-zen, local) and their state.
    Quota {
        #[arg(long)]
        json: bool,
        /// Probe now, when no collector is live and the reading is stale.
        #[arg(long)]
        refresh: bool,
    },

    /// What `horch spawn <teammate>` would do about usage limits right now:
    /// spawn it, substitute a fallback, or refuse. Spawns nothing.
    Route {
        teammate: String,
        #[arg(long)]
        json: bool,
        #[arg(long)]
        exact: bool,
        #[arg(long)]
        force: bool,
    },

    /// The fleet telemetry space: the collector and its screen, or a viewer
    /// when a collector is already live.
    Telemetry {
        #[command(subcommand)]
        command: Option<TelemetrySub>,
        /// State root for a pane that does not inherit $HORCH_STATE_DIR.
        #[arg(long, value_name = "DIR", hide = true, global = true)]
        state_dir: Option<String>,
    },

    /// Report the worker grid of every tab in the workspace.
    Layout {
        #[arg(long, value_name = "ID")]
        pane: Option<String>,
        #[arg(long, value_name = "ID")]
        workspace: Option<String>,
    },

    /// Rearrange every pane into the fleet grid: the orchestrator full height on
    /// the left of tab 1, 4 workers beside it in 2x2, 6 per overflow tab in 2x3.
    ///
    /// `horch spawn` and `horch done` do this themselves, so run it by hand only
    /// after you have moved panes around or if the grid looks wrong.
    Tile {
        /// Print the moves it would make and the grid they produce, and change
        /// nothing.
        #[arg(long)]
        plan: bool,
        #[arg(long, value_name = "ID")]
        pane: Option<String>,
        #[arg(long, value_name = "ID")]
        workspace: Option<String>,
        /// Wait this long before reading the workspace. `horch done` uses it to
        /// let its own pane finish closing before the grid is measured.
        #[arg(long, value_name = "MS", default_value_t = 0, hide = true)]
        settle_ms: u64,
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
        #[command(subcommand)]
        command: Option<cmd::skillscmd::SkillsCommand>,
        #[arg(long)]
        phase: Option<horch_core::teammates::Phase>,
        /// Emit machine-readable JSON (also the default catalog format).
        #[arg(long)]
        json: bool,
    },

    /// The installed marketplace skills under
    /// `${XDG_DATA_HOME:-~/.local/share}/horch/`.
    Marketplace {
        #[command(subcommand)]
        command: cmd::marketplacecmd::MarketplaceCommand,
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
        /// The orchestrator's ledger record, written by `horch fleet`.
        #[arg(long, value_name = "ID")]
        record_id: Option<String>,
        /// The session id `horch fleet` minted for a claude orchestrator.
        #[arg(long, value_name = "ID")]
        session_id: Option<String>,
        /// The state root, for a pane that does not inherit $HORCH_STATE_DIR.
        #[arg(long, value_name = "DIR")]
        state_dir: Option<String>,
    },

    /// Register the current pane under a role. Used by the smoke checks.
    #[command(hide = true)]
    Register { role: String },
}

#[derive(Subcommand)]
enum TelemetrySub {
    /// Open the collector in its own herdr workspace, unless one is live.
    /// Never focuses, moves or splits an existing pane.
    Ensure,
    /// Run exactly one collector tick, with no screen. Exit 2 when a live
    /// collector holds the lock.
    Collect {
        #[arg(long)]
        once: bool,
    },
    /// Print one frame of the screen as plain text.
    Render {
        /// A snapshot file. Defaults to the state root's.
        #[arg(long, value_name = "FILE")]
        snapshot: Option<String>,
        #[arg(long, default_value = "120x40")]
        size: String,
        #[arg(long, default_value = "teammate")]
        group: String,
        #[arg(long, default_value = "live")]
        window: String,
    },
}

/// Join a trailing var-arg list the way `"$*"` did.
fn joined(parts: &[String]) -> String {
    parts.join(" ")
}

fn main() -> std::process::ExitCode {
    match run() {
        Ok(code) => code,
        // The REFUSED line is already on stdout; exit 3 says why (design 13.4).
        Err(e)
            if matches!(
                e.downcast_ref::<SpawnError>(),
                Some(SpawnError::Refused { .. })
            ) =>
        {
            eprintln!("horch: {e}");
            std::process::ExitCode::from(exit::REFUSED)
        }
        Err(e) => {
            eprintln!("horch: {e:#}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn run() -> Result<std::process::ExitCode> {
    let cli = Cli::parse();
    // The one read of the process environment. Every command takes its
    // values from this context.
    let mut ctx = bootstrap::context()?;
    let ctx = &mut ctx;
    match cli.command {
        Command::Fleet { cwd, flavor } => cmd::recipes::fleet(ctx, cwd.as_deref(), flavor)?,
        Command::Orchestration { cwd } => cmd::recipes::orchestration(ctx, cwd.as_deref())?,
        Command::Tell { role, message } => cmd::messaging::tell(ctx, &role, &joined(&message))?,
        Command::Inbox => cmd::messaging::inbox(ctx)?,
        Command::Assign { role, task } => cmd::messaging::assign(ctx, &role, &joined(&task))?,
        Command::Note { text } => cmd::messaging::note(ctx, &joined(&text))?,
        Command::Done { summary } => cmd::messaging::done(ctx, &joined(&summary))?,
        Command::Sessions { json, all } => cmd::ledgercmd::sessions(ctx, json, all)?,
        Command::Skills {
            command,
            phase,
            json,
        } => return cmd::skillscmd::run(ctx, command, phase, json),
        Command::Spawn {
            args,
            resume,
            phase,
            effort,
            role,
            from_pane,
            direction,
            untiled,
            exact,
            force,
        } => {
            let (teammate, task) = cmd::spawn::resolve_positionals(&args, resume.as_deref())?;
            let pane = cmd::spawn::spawn(
                ctx,
                cmd::spawn::SpawnArgs {
                    teammate,
                    task,
                    resume,
                    phase,
                    effort,
                    role,
                    from_pane,
                    direction,
                    tiling: TilingMode::from_no_tile(untiled),
                    exact,
                    force,
                },
            )?;
            output::println(&pane);
        }
        Command::Usage {
            json,
            since,
            project,
            by,
            window,
        } => cmd::usagecmd::usage(
            ctx,
            cmd::usagecmd::UsageArgs {
                json,
                since,
                project,
                by,
                window,
            },
        )?,
        Command::Quota { json, refresh } => cmd::quotacmd::quota(ctx, json, refresh)?,
        Command::Route {
            teammate,
            json,
            exact,
            force,
        } => return cmd::route::route(ctx, &teammate, json, exact, force),
        Command::Telemetry { command, state_dir } => {
            if let Some(dir) = state_dir {
                ctx.paths.set_state_dir(dir);
            }
            use cmd::telemetry::TelemetryCommand as T;
            let command = match command {
                None => T::Run,
                Some(TelemetrySub::Ensure) => T::Ensure,
                Some(TelemetrySub::Collect { once }) => {
                    if !once {
                        anyhow::bail!(
                            "`horch telemetry collect` needs --once; the long-running \
                             collector is plain `horch telemetry`"
                        );
                    }
                    T::CollectOnce
                }
                Some(TelemetrySub::Render {
                    snapshot,
                    size,
                    group,
                    window,
                }) => T::Render {
                    snapshot,
                    size,
                    group,
                    window,
                },
            };
            return cmd::telemetry::run(ctx, command);
        }
        Command::Cost {
            json,
            reprice,
            pricing,
            since,
            records,
            sessions,
        } => cmd::cost::cost(
            ctx,
            cmd::cost::CostArgs {
                json,
                reprice,
                pricing,
                since,
                records,
                sessions,
            },
        )?,
        Command::Teammates {
            json,
            check,
            matrix,
            new,
            dir,
        } => {
            if let Some(name) = new {
                cmd::teammatescmd::new(ctx, &name, dir.as_deref())?;
            } else if matrix {
                cmd::teammatescmd::matrix(ctx, json)?;
            } else if check {
                return Ok(cmd::teammatescmd::check(ctx)?);
            } else {
                cmd::teammatescmd::list(ctx, json)?;
            }
        }
        Command::Layout { pane, workspace } => {
            cmd::layoutcmd::layout(ctx, pane.as_deref(), workspace.as_deref())?
        }
        Command::Tile {
            plan,
            pane,
            workspace,
            settle_ms,
        } => cmd::tilecmd::tile(ctx, pane.as_deref(), workspace.as_deref(), plan, settle_ms)?,
        Command::Balance {
            pane,
            workspace,
            dry_run,
            settle_ms,
        } => cmd::balancecmd::balance(
            ctx,
            pane.as_deref(),
            workspace.as_deref(),
            dry_run,
            settle_ms,
        )?,
        Command::Ledger { command } => return cmd::ledgercmd::run(ctx, command),
        Command::Marketplace { command } => return cmd::marketplacecmd::run(ctx, command),
        Command::Doctor => cmd::doctor::doctor(ctx)?,
        Command::Install { dir } => cmd::install::install(ctx, dir.as_deref())?,
        Command::Smoke { command } => return cmd::smoke::run(ctx, command),
        Command::Worker { role } => return cmd::worker::worker(ctx, &role),
        Command::PaneLaunch {
            role,
            kind,
            model,
            teammates_dir,
            record_id,
            session_id,
            state_dir,
        } => {
            return cmd::recipes::pane_launch(
                ctx,
                &role,
                kind,
                model.as_deref(),
                teammates_dir.as_deref(),
                cmd::recipes::PaneIdentity {
                    record_id,
                    session_id,
                    state_dir,
                },
            )
        }
        Command::Register { role } => cmd::messaging::register(ctx, &role)?,
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
    fn spawn_takes_the_gate_flags() {
        match Cli::try_parse_from(["horch", "spawn", "opus", "x", "--exact", "--force"])
            .unwrap()
            .command
        {
            Command::Spawn { exact, force, .. } => assert!(exact && force),
            _ => panic!("wrong subcommand"),
        }
    }

    #[test]
    fn the_telemetry_commands_parse() {
        for argv in [
            vec!["horch", "telemetry"],
            vec!["horch", "telemetry", "ensure"],
            vec!["horch", "telemetry", "collect", "--once"],
            vec![
                "horch",
                "telemetry",
                "render",
                "--size",
                "80x24",
                "--group",
                "phase",
                "--window",
                "7d",
            ],
            vec![
                "horch", "usage", "--json", "--by", "plan", "--window", "today",
            ],
            vec!["horch", "quota", "--refresh", "--json"],
            vec!["horch", "route", "researcher", "--json"],
            vec!["horch", "fleet", "auto"],
        ] {
            assert!(Cli::try_parse_from(argv.clone()).is_ok(), "{argv:?}");
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
