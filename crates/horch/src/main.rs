//! `horch` - multi-agent orchestration layouts for herdr (https://herdr.dev).
//!
//! One binary replacing the justfile recipes, the `bin/horch-*` and `bin/herdr-*`
//! scripts, the launcher shell scripts, and the Node installer. Needs no bash,
//! jq, node, or just: the only external process is `herdr` itself, plus whichever
//! agent CLI a worker pane launches.

mod cmd;

use std::process::ExitCode;

use anyhow::Result;
use clap::{Parser, Subcommand};
use horch::{bootstrap, exit, output};
use horch_core::execution::service::SpawnError;
use horch_core::execution::TilingMode;
use horch_core::runtime::{EnvSource, ProcessEnv};
use horch_core::workspace::herdr::HerdrFailed;
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
    #[command(flatten)]
    Launch(LaunchCommand),

    #[command(flatten)]
    Messaging(MessagingCommand),

    #[command(flatten)]
    Session(SessionCommand),

    #[command(flatten)]
    Project(ProjectCommand),

    #[command(flatten)]
    Measure(MeasureCommand),

    #[command(flatten)]
    Grid(GridCommand),

    #[command(flatten)]
    Tool(ToolCommand),
}

#[derive(Subcommand)]
enum LaunchCommand {
    /// Run the worker-report channel relay.
    #[command(hide = true)]
    Channel {
        #[command(subcommand)]
        command: ChannelSubcommand,
    },

    /// Deliver durable worker mail.
    #[command(hide = true)]
    Mail {
        #[command(subcommand)]
        command: MailSubcommand,
    },

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
        /// Do not install a newer multi-herdr release or update the agent
        /// CLIs before the first pane starts. `HORCH_NO_UPDATE=1` does the same.
        #[arg(long)]
        no_update: bool,
        /// Resume an earlier orchestrator session by its session id instead of
        /// starting a new one. Give the flavor it ran as (e.g. `sol` for Codex).
        #[arg(long, value_name = "SESSION_ID")]
        resume: Option<String>,
    },

    /// Launch the fixed 5-pane orchestration workspace: 1 orchestrator (Fable) +
    /// 2x Sonnet, 1x Opus, 1x Codex, wired for two-way messaging.
    Orchestration {
        /// Project directory. Defaults to the current directory.
        #[arg(long)]
        cwd: Option<String>,
        /// Do not install a newer multi-herdr release or update the agent
        /// CLIs before the first pane starts. `HORCH_NO_UPDATE=1` does the same.
        #[arg(long)]
        no_update: bool,
    },

    /// Run the update command of each installed agent CLI. `horch fleet`
    /// does this before its first pane starts.
    #[command(long_about = "Run the update command of each installed agent CLI: \
                            claude update, codex update, opencode upgrade, agy update, \
                            pi update --self, prime-agent update. With no NAME, all 6 \
                            agent CLIs update at the same time. Exits 1 when 1 updater \
                            fails or times out (HORCH_UPDATE_TIMEOUT_MS, default 180000).\n\n\
                            herdr is never in the default list. `horch update herdr` \
                            runs `herdr update` attached to this terminal: it can stop \
                            the herdr server, and that closes every pane.")]
    Update {
        /// What to update: claude, codex, opencode, antigravity, pi, prime
        /// or herdr. Defaults to the 6 agent CLIs.
        #[arg(value_name = "NAME", value_parser = clap::builder::PossibleValuesParser::new(cmd::update::NAMES))]
        names: Vec<String>,
    },
}

#[derive(Subcommand)]
enum MessagingCommand {
    /// Send a message into another pane's terminal.
    Tell {
        /// Registered role to deliver to, e.g. orchestrator or sonnet-1.
        role: String,
        /// The message. Joined with spaces.
        #[arg(required = true, trailing_var_arg = true)]
        message: Vec<String>,
    },

    /// List the roles registered in this workspace. The orchestrator also gets its pending mail, printed last and acknowledged; workers get only the role list.
    Inbox {
        /// Print only the role list; leave mail pending.
        #[arg(long, conflicts_with = "peek")]
        roles: bool,
        /// Orchestrator only: print pending mail without acknowledging it. Workers get only the role list.
        #[arg(long)]
        peek: bool,
    },

    /// Read the last lines of a registered role's pane without typing.
    Peek {
        role: String,
        #[arg(long, default_value_t = 20)]
        lines: usize,
    },

    /// Give a task to a live worker: record it on the ledger, then deliver it.
    Assign {
        role: String,
        #[arg(required = true, trailing_var_arg = true)]
        task: Vec<String>,
    },

    /// Append a progress note to this worker's own ledger record.
    Note {
        /// Share a project fact with workers launched after this command.
        #[arg(long)]
        fact: bool,
        #[arg(required = true, trailing_var_arg = true)]
        text: Vec<String>,
    },
}

#[derive(Subcommand)]
enum SessionCommand {
    /// Show the context of each live session of this project, with its
    /// threshold and its state. The orchestrator is first.
    Context {
        /// Show only the sessions over their threshold, asked to prepare, or
        /// with a lost compaction job.
        #[arg(long)]
        over: bool,
        /// Show the native window that each teammate gets when it starts here.
        #[arg(long)]
        windows: bool,
        /// Print every row as JSON: 1 object per session, unknown values
        /// null. Ignores --over; --windows wins.
        #[arg(long)]
        json: bool,
        /// Include sessions from every workspace of this project.
        #[arg(long)]
        all_workspaces: bool,
        /// Include only live sessions.
        #[arg(long)]
        live: bool,
        /// Include only this role.
        #[arg(long)]
        role: Option<String>,
    },

    /// Compact the context of a session in place. First ask it to write its
    /// handoff with --request. When it reports COMPACT-READY, run this
    /// command without --request.
    Compact {
        /// The registered role, for example sonnet-1 or orchestrator.
        role: String,
        /// Ask the session to write its handoff file. Nothing is compacted.
        #[arg(long, conflicts_with_all = ["force", "foreground", "timeout"])]
        request: bool,
        /// Skip the route check and the handoff check.
        #[arg(long)]
        force: bool,
        /// Run the compaction job in this process. horch compact starts it so.
        #[arg(long, hide = true)]
        foreground: bool,
        /// The maximum wait, in seconds, for the session to become idle.
        #[arg(long, value_name = "SECS")]
        timeout: Option<u64>,
    },

    /// Switch a read-only Codex pane (teammate permission_mode plan) to the
    /// fleet write profile and back. The orchestrator runs it. horch compact
    /// --request switches to write by itself, and the job switches back.
    Mode {
        /// The registered role of the Codex pane.
        role: String,
        /// write: the fleet_write profile (the state root and the project
        /// are writable). plan: Read Only, as the pane launched.
        #[arg(value_parser = ["write", "plan"])]
        mode: String,
        /// The maximum wait, in seconds, for the pane to be at its prompt.
        #[arg(long, value_name = "SECS")]
        timeout: Option<u64>,
    },

    /// Finish this worker: record a summary, report DONE, and close its pane.
    Done {
        /// Finish even when this record owns an open worktree claim.
        #[arg(long, value_name = "REASON")]
        unlanded: Option<String>,
        #[arg(required = true, trailing_var_arg = true)]
        summary: Vec<String>,
    },
}

#[derive(Subcommand)]
enum ProjectCommand {
    /// Create, land, or inspect worker worktrees.
    Worktree {
        #[command(subcommand)]
        command: WorktreeSubcommand,
    },

    /// Read the project session ledger. Do this before spawning.
    Sessions {
        /// Print the raw ledger JSON instead of the readable summary.
        #[arg(long)]
        json: bool,
        /// Also list competition candidates and judges (multi-herdr-dataset).
        #[arg(long)]
        all: bool,
        /// Include sessions from every workspace of this project.
        #[arg(long)]
        all_workspaces: bool,
        /// Include only live sessions.
        #[arg(long)]
        live: bool,
        /// Include only this role.
        #[arg(long)]
        role: Option<String>,
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

    /// Spend telemetry: what this project's fleet cost, per worker, read
    /// from each harness's transcripts; plus which expected skills each
    /// worker never loaded.
    ///
    /// Informational: spend telemetry to see where tokens go; not an input to
    /// routing, model or teammate choice.
    ///
    /// With no filter, writes 3 reports for the windows that end now (30d,
    /// 7d, 24h) to `<dir>/<YYYY_MM_DD_HHMMSS>_30d.json`, `_7d.json` and
    /// `_24h.json`, and prints a JSON summary. A window counts each call by
    /// its own time, not by when its session started. With --since, --record
    /// or --session: 1 report on stdout, and a file only with --dir. Sessions
    /// that cannot be priced are listed, never zeroed.
    Cost {
        /// Accepted for old callers; JSON is the default.
        #[arg(long, conflicts_with = "text")]
        json: bool,
        /// Print the human tables instead of JSON. The files are still written.
        #[arg(long)]
        text: bool,
        /// Where the report files go. Default: <project>/ai_docs/reports/cost.
        #[arg(long, value_name = "PATH")]
        dir: Option<String>,
        /// Add a what-if column: the same tokens at this model's prices.
        #[arg(long, value_name = "MODEL")]
        reprice: Option<String>,
        /// JSON file of per-MTok prices to use over the built-in table:
        /// {"<model>": {"input": 4, "output": 20, "cache_read": 0.2}}.
        #[arg(long, value_name = "FILE")]
        pricing: Option<String>,
        /// Only calls at or after this time (RFC 3339 or a date such as
        /// 2026-09-24, UTC), as `horch usage --since` counts them.
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
        phase: Option<horch_core::roster::Phase>,
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
        /// The dead worker (task ledger id) this one replaces: its claim
        /// takes that worker's task and gets its transcript and worktree.
        #[arg(long, value_name = "WORKER")]
        replaces: Option<String>,
        /// The task-ledger run (slug or id) this worker joins. Without it, a
        /// spawn with task text runs outside any run.
        #[arg(long = "run", value_name = "RUN")]
        run: Option<String>,
    },
}

#[derive(Subcommand)]
enum MeasureCommand {
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

    /// Every agent harness horch can drive: binary, version, efforts,
    /// capabilities, the models the roster runs on it, and its usage pool.
    AgentList {
        #[arg(long)]
        json: bool,
        /// Skip the `--version` probe: run no harness binary.
        #[arg(long)]
        no_probe: bool,
    },

    /// The usage pools (claude, codex, opencode-zen, google, local) and their state.
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
}

#[derive(Subcommand)]
// Parsed once per process, so the size of the ledger variant costs nothing.
#[allow(clippy::large_enum_variant)]
enum GridCommand {
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

    /// The task ledger: queue tasks, claim them, finish them, and read the
    /// board. `horch ledger legacy` holds the old session-ledger plumbing.
    Ledger {
        #[command(subcommand)]
        command: cmd::taskledger::LedgerCommand,
    },

    /// Inspect the integrated skill catalog and estimated context cost.
    Skills {
        #[command(subcommand)]
        command: Option<cmd::skillscmd::SkillsCommand>,
        #[arg(long)]
        phase: Option<horch_core::roster::Phase>,
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
}

#[derive(Subcommand)]
enum ToolCommand {
    /// Check that herdr is installed and its server is reachable.
    Doctor {
        /// Remove the stale state that doctor counts: spool files, sockets,
        /// registries and mail of gone workspaces, finished bundles and codex
        /// homes, dead temp files, and working records of closed panes.
        #[arg(long)]
        clean: bool,
    },

    /// Copy this binary somewhere on your PATH.
    Install {
        /// Install directory. Defaults to ~/.local/bin (%LOCALAPPDATA%\Programs\horch on Windows).
        #[arg(long, value_name = "DIR")]
        dir: Option<String>,
    },

    /// Install the latest multi-herdr release over this horch.
    #[command(
        long_about = "Look up the latest multi-herdr release on GitHub. When it is \
                            newer than this horch, download the archive for this machine, \
                            verify it against the release's SHA256SUMS, and run its \
                            `horch install --dir <dir of this horch>`. A source build \
                            (under a cargo target dir) is refused: update it with \
                            `git pull` and `just install`."
    )]
    SelfUpdate {
        /// Only say whether a newer release is out. Exits 0 either way.
        #[arg(long)]
        check: bool,
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
        /// Resume `--session-id` rather than start it fresh.
        #[arg(long, requires = "session_id")]
        resume: bool,
        /// The state root, for a pane that does not inherit $HORCH_STATE_DIR.
        #[arg(long, value_name = "DIR")]
        state_dir: Option<String>,
    },

    /// Register the current pane under a role. Used by the smoke checks.
    #[command(hide = true)]
    Register { role: String },
}

#[derive(Subcommand)]
enum ChannelSubcommand {
    /// Serve the relay as an MCP stdio server.
    Serve {
        #[arg(long, value_parser = |s: &str| s.parse::<horch_core::harness::HarnessKind>())]
        agent: horch_core::harness::HarnessKind,
    },
}

#[derive(Subcommand)]
enum MailSubcommand {
    /// Redeliver pending mail until it is acknowledged.
    Courier {
        #[arg(long)]
        workspace: String,
    },
}

#[derive(Subcommand)]
enum WorktreeSubcommand {
    /// Create and claim a worker worktree.
    New { name: String },
    /// Rebase, check, and land one owned worktree.
    Land {
        name: Option<String>,
        #[arg(long, value_name = "CMD")]
        check: Option<String>,
    },
    /// Show registered claims and changed unregistered worktrees.
    List,
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
            let argv: Vec<String> = std::env::args().collect();
            let sandbox = ProcessEnv.var("CODEX_SANDBOX");
            if let Some(hint) = codex_sandbox_hint(&e, sandbox.as_deref(), &plain_command(&argv)) {
                eprintln!("horch: {hint}");
            }
            std::process::ExitCode::FAILURE
        }
    }
}

/// What to do when herdr failed inside Codex's sandbox. `codex_sandbox` is
/// `CODEX_SANDBOX`, which Codex sets in every command it runs sandboxed.
/// The sandbox has no network, so herdr's socket is never reachable from it.
/// An execpolicy `allow` lifts the sandbox only for a command that matches
/// the rule's prefix on its own, so `horch spawn` at the end of a script with
/// a heredoc ran sandboxed, and a Codex orchestrator reported the fleet
/// BLOCKED. A worker that set the fleet variables inline lost its report
/// the same way (W2 C1), so the hint ends with `command`, the exact command
/// to run.
fn codex_sandbox_hint(
    e: &anyhow::Error,
    codex_sandbox: Option<&str>,
    command: &str,
) -> Option<String> {
    fn herdr_failed(e: &anyhow::Error) -> bool {
        // `SpawnError` carries its cause in `Display`, not `source()`.
        let inner = match e.downcast_ref::<SpawnError>() {
            Some(SpawnError::LaunchFailed { source, .. } | SpawnError::Store(source)) => {
                herdr_failed(source)
            }
            _ => false,
        };
        inner || e.chain().any(|c| c.is::<HerdrFailed>())
    }
    (codex_sandbox.is_some() && herdr_failed(e)).then(|| {
        format!(
            "this ran inside the Codex sandbox, which blocks herdr's socket and the \
             channel relay. It is not a fleet failure. Write any file in a separate \
             command first. Then run exactly this, as the whole command, with no \
             variable prefix, `env`, `export`, `cd`, `;`, `&&`, `|`, redirect, heredoc \
             or script:\n  {command}"
        )
    })
}

/// `argv` as one shell command, with `horch` as the program and each other
/// word quoted when it needs it.
fn plain_command(argv: &[String]) -> String {
    let words = argv.iter().skip(1).map(|word| {
        let plain = !word.is_empty()
            && word
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_./:=@%+,".contains(&b));
        if plain {
            word.clone()
        } else {
            format!("'{}'", word.replace('\'', r"'\''"))
        }
    });
    std::iter::once("horch".to_string())
        .chain(words)
        .collect::<Vec<_>>()
        .join(" ")
}

fn run() -> Result<std::process::ExitCode> {
    let cli = Cli::parse();
    // The one read of the process environment. Every command takes its
    // values from this context.
    let mut ctx = bootstrap::context()?;
    cli.command.warn_stale(&ctx);
    // A pane that herdr resumed after a restart has no HORCH_ROLE (#125).
    if cli.command.adopts_identity() {
        cmd::restore::adopt_restored_identity(&mut ctx);
    }
    let remind = cli.command.reminds();
    let watched = cli.command.watched();
    let result = cli.command.dispatch(&mut ctx);
    if remind {
        cmd::messaging::remind_pending(&ctx);
    }
    // No dead watchdog hides dead workers: any horch command restarts it.
    if watched {
        cmd::taskledger_watch::ensure(&ctx);
    }
    result
}

type Ctx = horch_core::runtime::RuntimeContext;

/// A handler that returns no exit code succeeds when it returns.
fn done(r: Result<()>) -> Result<ExitCode> {
    r.map(|()| ExitCode::SUCCESS)
}

impl Command {
    fn dispatch(self, ctx: &mut Ctx) -> Result<ExitCode> {
        match self {
            Command::Launch(c) => c.dispatch(ctx),
            Command::Messaging(c) => c.dispatch(ctx),
            Command::Session(c) => c.dispatch(ctx),
            Command::Project(c) => c.dispatch(ctx),
            Command::Measure(c) => c.dispatch(ctx),
            Command::Grid(c) => c.dispatch(ctx),
            Command::Tool(c) => c.dispatch(ctx),
        }
    }

    /// `doctor` and `spawn` warn when the installed horch is stale.
    fn warn_stale(&self, ctx: &Ctx) {
        match self {
            Command::Tool(ToolCommand::Doctor { .. }) => cmd::selfupdate::doctor_warning(ctx),
            Command::Project(ProjectCommand::Spawn { .. })
            | Command::Messaging(MessagingCommand::Inbox { .. }) => {
                cmd::selfupdate::stale_warning(ctx)
            }
            _ => {}
        }
    }

    fn adopts_identity(&self) -> bool {
        matches!(
            self,
            Command::Messaging(
                MessagingCommand::Tell { .. }
                    | MessagingCommand::Inbox { .. }
                    | MessagingCommand::Note { .. }
            ) | Command::Session(SessionCommand::Done { .. })
                | Command::Grid(GridCommand::Ledger { .. })
        )
    }

    fn reminds(&self) -> bool {
        !matches!(
            self,
            Command::Launch(LaunchCommand::Channel { .. } | LaunchCommand::Mail { .. })
                | Command::Tool(ToolCommand::Worker { .. })
                | Command::Grid(GridCommand::Tile { .. } | GridCommand::Balance { .. })
                | Command::Messaging(MessagingCommand::Inbox { roles: false, .. })
        )
    }

    /// `install` restarts a watchdog on the binary it installed; `ensure`
    /// would move it back onto the binary that ran `install` (#216).
    fn watched(&self) -> bool {
        !matches!(
            self,
            Command::Tool(ToolCommand::Worker { .. } | ToolCommand::Install { .. })
                | Command::Grid(GridCommand::Ledger {
                    command: cmd::taskledger::LedgerCommand::Watch { .. }
                })
        )
    }
}

impl LaunchCommand {
    fn dispatch(self, ctx: &mut Ctx) -> Result<ExitCode> {
        match self {
            LaunchCommand::Channel { command } => match command {
                ChannelSubcommand::Serve { agent } => done(cmd::channel::serve(ctx, agent)),
            },
            LaunchCommand::Mail { command } => match command {
                MailSubcommand::Courier { workspace } => done(cmd::mail::courier(ctx, &workspace)),
            },
            LaunchCommand::Fleet {
                cwd,
                flavor,
                no_update,
                resume,
            } => done(cmd::recipes::fleet_with(
                ctx,
                cwd.as_deref(),
                flavor,
                &cmd::recipes::FleetOptions {
                    no_update,
                    resume,
                    ..Default::default()
                },
            )),
            LaunchCommand::Orchestration { cwd, no_update } => {
                done(cmd::recipes::orchestration(ctx, cwd.as_deref(), no_update))
            }
            LaunchCommand::Update { names } => cmd::update::update(ctx, &names),
        }
    }
}

impl MessagingCommand {
    fn dispatch(self, ctx: &mut Ctx) -> Result<ExitCode> {
        match self {
            MessagingCommand::Tell { role, message } => {
                done(cmd::messaging::tell(ctx, &role, &joined(&message)))
            }
            MessagingCommand::Inbox { roles, peek } => {
                done(cmd::messaging::inbox(ctx, roles, peek))
            }
            MessagingCommand::Peek { role, lines } => done(cmd::messaging::peek(ctx, &role, lines)),
            MessagingCommand::Assign { role, task } => {
                done(cmd::messaging::assign(ctx, &role, &joined(&task)))
            }
            MessagingCommand::Note { text, fact } => done(note_command(ctx, &joined(&text), fact)),
        }
    }
}

impl SessionCommand {
    fn dispatch(self, ctx: &mut Ctx) -> Result<ExitCode> {
        match self {
            SessionCommand::Context {
                over,
                windows,
                json,
                all_workspaces,
                live,
                role,
            } => cmd::context::context_filtered(
                ctx,
                over,
                windows,
                json,
                all_workspaces,
                live,
                role.as_deref(),
            ),
            SessionCommand::Compact {
                role,
                request,
                force,
                foreground,
                timeout,
            } => cmd::compact::compact(
                ctx,
                &cmd::compact::CompactArgs {
                    role,
                    request,
                    force,
                    foreground,
                    timeout,
                },
            ),
            SessionCommand::Mode {
                role,
                mode,
                timeout,
            } => mode_command(ctx, role, &mode, timeout),
            SessionCommand::Done { summary, unlanded } => {
                done(done_command(ctx, &joined(&summary), unlanded.as_deref()))
            }
        }
    }
}

impl ProjectCommand {
    fn dispatch(self, ctx: &mut Ctx) -> Result<ExitCode> {
        match self {
            ProjectCommand::Worktree { command } => done(worktree_command(ctx, command)),
            ProjectCommand::Sessions {
                json,
                all,
                all_workspaces,
                live,
                role,
            } => done(cmd::ledgercmd::sessions_filtered(
                ctx,
                json,
                all,
                all_workspaces,
                live,
                role.as_deref(),
            )),
            ProjectCommand::Teammates {
                json,
                check,
                matrix,
                new,
                dir,
            } => teammates_command(ctx, json, check, matrix, new.as_deref(), dir.as_deref()),
            ProjectCommand::Cost {
                json: _,
                text,
                dir,
                reprice,
                pricing,
                since,
                records,
                sessions,
            } => done(cmd::cost::cost(
                ctx,
                cmd::cost::CostArgs {
                    text,
                    dir,
                    reprice,
                    pricing,
                    since,
                    records,
                    sessions,
                },
            )),
            ProjectCommand::Spawn {
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
                replaces,
                run,
            } => done(spawn_command(
                ctx,
                &args,
                cmd::spawn::SpawnArgs {
                    teammate: None,
                    task: String::new(),
                    resume,
                    phase,
                    effort,
                    role,
                    from_pane,
                    direction,
                    tiling: TilingMode::from_no_tile(untiled),
                    exact,
                    force,
                    replaces,
                    run,
                },
            )),
        }
    }
}

impl MeasureCommand {
    fn dispatch(self, ctx: &mut Ctx) -> Result<ExitCode> {
        match self {
            MeasureCommand::Usage {
                json,
                since,
                project,
                by,
                window,
            } => done(cmd::usagecmd::usage(
                ctx,
                cmd::usagecmd::UsageArgs {
                    json,
                    since,
                    project,
                    by,
                    window,
                },
            )),
            MeasureCommand::AgentList { json, no_probe } => {
                done(cmd::agentlist::agent_list(ctx, json, no_probe))
            }
            MeasureCommand::Quota { json, refresh } => {
                done(cmd::quotacmd::quota(ctx, json, refresh))
            }
            MeasureCommand::Route {
                teammate,
                json,
                exact,
                force,
            } => cmd::route::route(ctx, &teammate, json, exact, force),
            MeasureCommand::Telemetry { command, state_dir } => {
                telemetry_command(ctx, command, state_dir)
            }
        }
    }
}

impl GridCommand {
    fn dispatch(self, ctx: &mut Ctx) -> Result<ExitCode> {
        match self {
            GridCommand::Layout { pane, workspace } => done(cmd::layoutcmd::layout(
                ctx,
                pane.as_deref(),
                workspace.as_deref(),
            )),
            GridCommand::Tile {
                plan,
                pane,
                workspace,
                settle_ms,
            } => done(cmd::tilecmd::tile(
                ctx,
                pane.as_deref(),
                workspace.as_deref(),
                plan,
                settle_ms,
            )),
            GridCommand::Balance {
                pane,
                workspace,
                dry_run,
                settle_ms,
            } => done(cmd::balancecmd::balance(
                ctx,
                pane.as_deref(),
                workspace.as_deref(),
                dry_run,
                settle_ms,
            )),
            GridCommand::Ledger { command } => cmd::taskledger::run(ctx, command),
            GridCommand::Skills {
                command,
                phase,
                json,
            } => cmd::skillscmd::run(ctx, command, phase, json),
            GridCommand::Marketplace { command } => cmd::marketplacecmd::run(ctx, command),
        }
    }
}

impl ToolCommand {
    fn dispatch(self, ctx: &mut Ctx) -> Result<ExitCode> {
        match self {
            ToolCommand::Doctor { clean } => done(cmd::doctor::doctor(ctx, clean)),
            ToolCommand::Install { dir } => done(cmd::install::install(ctx, dir.as_deref())),
            ToolCommand::SelfUpdate { check } => cmd::selfupdate::self_update(ctx, check),
            ToolCommand::Smoke { command } => cmd::smoke::run(ctx, command),
            ToolCommand::Worker { role } => cmd::worker::worker(ctx, &role),
            ToolCommand::PaneLaunch {
                role,
                kind,
                model,
                teammates_dir,
                record_id,
                session_id,
                resume,
                state_dir,
            } => cmd::recipes::pane_launch(
                ctx,
                &role,
                kind,
                model.as_deref(),
                teammates_dir.as_deref(),
                cmd::recipes::PaneIdentity {
                    record_id,
                    session_id,
                    resume,
                    state_dir,
                },
            ),
            ToolCommand::Register { role } => done(cmd::messaging::register(ctx, &role)),
        }
    }
}

/// `horch note`: a note on the worker's record, or a project fact.
fn note_command(ctx: &mut Ctx, text: &str, fact: bool) -> Result<()> {
    if !fact {
        return cmd::messaging::note(ctx, text);
    }
    let added =
        horch_core::project_notes::record(&ctx.paths.state_root, &ctx.paths.project()?, text)?;
    output::println(if added {
        "Project fact recorded."
    } else {
        "Project fact already recorded."
    });
    Ok(())
}

fn mode_command(ctx: &mut Ctx, role: String, mode: &str, timeout: Option<u64>) -> Result<ExitCode> {
    let mode = horch_core::harness::codex::PaneMode::parse(mode)
        .ok_or_else(|| anyhow::anyhow!("mode is write or plan"))?;
    cmd::mode::mode(
        ctx,
        &cmd::mode::ModeArgs {
            role,
            mode,
            timeout,
        },
    )
}

/// `horch done`: the task ledger finishes a ledger worker, the mailbox the rest.
fn done_command(ctx: &mut Ctx, summary: &str, unlanded: Option<&str>) -> Result<()> {
    if cmd::taskledger::is_ledger_worker(ctx) {
        let store = horch_core::taskledger::open(ctx)?;
        cmd::taskledger::done_cmd(ctx, store, summary, unlanded)
    } else {
        cmd::messaging::done(ctx, summary, unlanded)
    }
}

fn worktree_command(ctx: &mut Ctx, command: WorktreeSubcommand) -> Result<()> {
    match command {
        WorktreeSubcommand::New { name } => cmd::worktree::new(ctx, &name),
        WorktreeSubcommand::Land { name, check } => {
            cmd::worktree::land(ctx, name.as_deref(), check.as_deref())
        }
        WorktreeSubcommand::List => cmd::worktree::list(ctx),
    }
}

/// `horch spawn`: split the positionals, spawn, and print the pane id.
fn spawn_command(
    ctx: &mut Ctx,
    positionals: &[String],
    mut args: cmd::spawn::SpawnArgs,
) -> Result<()> {
    (args.teammate, args.task) =
        cmd::spawn::resolve_positionals(positionals, args.resume.as_deref())?;
    output::println(&cmd::spawn::spawn(ctx, args)?);
    Ok(())
}

fn teammates_command(
    ctx: &mut Ctx,
    json: bool,
    check: bool,
    matrix: bool,
    new: Option<&str>,
    dir: Option<&str>,
) -> Result<ExitCode> {
    if let Some(name) = new {
        done(cmd::teammatescmd::new(ctx, name, dir))
    } else if matrix {
        done(cmd::teammatescmd::matrix(ctx, json))
    } else if check {
        cmd::teammatescmd::check(ctx)
    } else {
        done(cmd::teammatescmd::list(ctx, json))
    }
}

fn telemetry_command(
    ctx: &mut Ctx,
    command: Option<TelemetrySub>,
    state_dir: Option<String>,
) -> Result<ExitCode> {
    if let Some(dir) = state_dir {
        ctx.paths.set_state_dir(dir);
    }
    use cmd::telemetry::TelemetryCommand as T;
    let command = match command {
        None => T::Run,
        Some(TelemetrySub::Ensure) => T::Ensure,
        Some(TelemetrySub::Collect { once: false }) => anyhow::bail!(
            "`horch telemetry collect` needs --once; the long-running \
             collector is plain `horch telemetry`"
        ),
        Some(TelemetrySub::Collect { once: true }) => T::CollectOnce,
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
    cmd::telemetry::run(ctx, command)
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    /// The failure a Codex orchestrator hit on `horch spawn`, verbatim.
    #[cfg(unix)]
    fn herdr_denied() -> anyhow::Error {
        use std::os::unix::process::ExitStatusExt;
        HerdrFailed {
            command: "pane get wT:p1".into(),
            status: std::process::ExitStatus::from_raw(256),
            stderr: r#"Error: Os { code: 1, kind: PermissionDenied, message: "Operation not permitted" }"#
                .into(),
        }
        .into()
    }

    /// `horch spawn` at the end of a compound command ran in Codex's sandbox,
    /// and the orchestrator reported the fleet BLOCKED. The hint names the cause
    /// and the rerun, through the spawn error that wraps the herdr one.
    #[cfg(unix)]
    #[test]
    fn a_herdr_failure_in_the_codex_sandbox_says_to_run_horch_alone() {
        let spawn: anyhow::Error = SpawnError::LaunchFailed {
            id: horch_core::ids::ExecutionId::new("1a00fce6-6ca6-42ab-9d41-9cff5404221b").unwrap(),
            stage: horch_core::execution::LaunchStage::Split,
            source: herdr_denied(),
        }
        .into();
        for e in [herdr_denied(), herdr_denied().context("horch tell"), spawn] {
            let hint = codex_sandbox_hint(&e, Some("seatbelt"), "horch spawn x").unwrap();
            assert!(hint.contains("as the whole command"), "{hint}");
        }
    }

    /// W2 C1: a worker that set the variables inline, or ran `horch done`
    /// from a script, ran sandboxed and lost its report. The hint gives the
    /// exact plain command to run, quoted for the shell, also through the
    /// error of a `horch done` whose report failed.
    #[cfg(unix)]
    #[test]
    fn w2_the_sandbox_hint_names_the_exact_command() {
        let argv = ["horch", "done", "It's finished; tests pass"].map(String::from);
        let command = plain_command(&argv);
        assert_eq!(command, r#"horch done 'It'\''s finished; tests pass'"#);
        let done = herdr_denied().context("horch done: the report did not reach the orchestrator");
        let hint = codex_sandbox_hint(&done, Some("seatbelt"), &command).unwrap();
        assert!(hint.ends_with(&format!("\n  {command}")), "{hint}");
        assert!(hint.contains("variable prefix"), "{hint}");
        assert_eq!(
            plain_command(&["horch", "note", "x"].map(String::from)),
            "horch note x"
        );
    }

    #[cfg(unix)]
    #[test]
    fn no_sandbox_hint_outside_the_sandbox_or_for_another_failure() {
        assert_eq!(codex_sandbox_hint(&herdr_denied(), None, "horch x"), None);
        let other = anyhow::anyhow!("roster: no teammate named 'x'");
        assert_eq!(
            codex_sandbox_hint(&other, Some("seatbelt"), "horch x"),
            None
        );
    }

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
                Command::Project(ProjectCommand::Spawn { phase, .. }) => {
                    assert_eq!(phase, Some(horch_core::roster::Phase::Research))
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
            Command::Messaging(MessagingCommand::Tell { role, message }) => {
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
            Command::Messaging(MessagingCommand::Tell { message, .. }) => {
                assert_eq!(joined(&message), "[a] DONE: x, y")
            }
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
            Command::Project(ProjectCommand::Spawn {
                args,
                resume,
                direction,
                ..
            }) => {
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
            Command::Project(ProjectCommand::Spawn { args, resume, .. }) => {
                let (name, _) = cmd::spawn::resolve_positionals(&args, resume.as_deref()).unwrap();
                let roster = horch_core::roster::Roster::builtin().unwrap();
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
            Command::Project(ProjectCommand::Spawn { exact, force, .. }) => assert!(exact && force),
            _ => panic!("wrong subcommand"),
        }
    }

    /// Which flattened group a parsed command landed in.
    fn group_of(command: &Command) -> &'static str {
        match command {
            Command::Launch(_) => "launch",
            Command::Messaging(_) => "messaging",
            Command::Session(_) => "session",
            Command::Project(_) => "project",
            Command::Measure(_) => "measure",
            Command::Grid(_) => "grid",
            Command::Tool(_) => "tool",
        }
    }

    #[test]
    fn the_telemetry_and_fleet_commands_parse_into_their_groups() {
        for (argv, group) in [
            (vec!["horch", "telemetry"], "measure"),
            (vec!["horch", "telemetry", "ensure"], "measure"),
            (vec!["horch", "telemetry", "collect", "--once"], "measure"),
            (
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
                "measure",
            ),
            (
                vec![
                    "horch", "usage", "--json", "--by", "plan", "--window", "today",
                ],
                "measure",
            ),
            (vec!["horch", "quota", "--refresh", "--json"], "measure"),
            (vec!["horch", "route", "researcher", "--json"], "measure"),
            (vec!["horch", "fleet", "auto"], "launch"),
            (
                vec![
                    "horch",
                    "fleet",
                    "--resume",
                    "0b6f8c1e-6c0e-4a43-9d7e-3f2a1b4c5d6e",
                ],
                "launch",
            ),
            (
                vec!["horch", "fleet", "sol", "--resume", "thread-1"],
                "launch",
            ),
        ] {
            let cli = Cli::try_parse_from(argv.clone()).unwrap_or_else(|e| panic!("{argv:?}: {e}"));
            assert_eq!(group_of(&cli.command), group, "{argv:?}");
        }
    }

    /// Grouping the variants into flattened sub-enums must not change what
    /// `horch --help` lists: the same commands, in the same order, with the
    /// internal ones still hidden.
    #[test]
    fn help_lists_the_same_commands_in_the_same_order() {
        let cmd = Cli::command();
        let listed: Vec<(&str, bool)> = cmd
            .get_subcommands()
            .map(|c| (c.get_name(), c.is_hide_set()))
            .collect();
        let expected = [
            ("channel", true),
            ("mail", true),
            ("fleet", false),
            ("orchestration", false),
            ("update", false),
            ("tell", false),
            ("inbox", false),
            ("peek", false),
            ("assign", false),
            ("note", false),
            ("context", false),
            ("compact", false),
            ("mode", false),
            ("done", false),
            ("worktree", false),
            ("sessions", false),
            ("teammates", false),
            ("cost", false),
            ("spawn", false),
            ("usage", false),
            ("agent-list", false),
            ("quota", false),
            ("route", false),
            ("telemetry", false),
            ("layout", false),
            ("tile", false),
            ("balance", false),
            ("ledger", false),
            ("skills", false),
            ("marketplace", false),
            ("doctor", false),
            ("install", false),
            ("self-update", false),
            ("smoke", false),
            ("worker", true),
            ("pane-launch", true),
            ("register", true),
        ];
        assert_eq!(listed, expected);
    }

    /// The commands whose reminders, watchdog and identity handling differ.
    #[test]
    fn command_policies_follow_the_command() {
        let parse = |argv: &[&str]| Cli::try_parse_from(argv).unwrap().command;
        let inbox_roles = parse(&["horch", "inbox", "--roles"]);
        let inbox_mail = parse(&["horch", "inbox"]);
        assert!(inbox_roles.reminds() && !inbox_mail.reminds());
        assert!(!parse(&["horch", "tile"]).reminds());
        assert!(!parse(&["horch", "worker", "r"]).watched());
        assert!(!parse(&["horch", "install"]).watched());
        assert!(!parse(&["horch", "ledger", "watch"]).watched());
        assert!(parse(&["horch", "ledger", "path"]).watched());
        assert!(parse(&["horch", "done", "x"]).adopts_identity());
        assert!(parse(&["horch", "ledger", "path"]).adopts_identity());
        assert!(!parse(&["horch", "peek", "r"]).adopts_identity());
    }

    /// `horch fleet --resume` reaches the pane as `--session-id <id> --resume`;
    /// a bare `--resume` has no session to continue.
    #[test]
    fn pane_launch_resume_needs_a_session_id() {
        let base = ["horch", "pane-launch", "--role", "orchestrator", "--kind"];
        let kind = cmd::recipes::PaneKind::FleetOrchestrator.as_str();
        let with_id = [&base[..], &[kind, "--session-id", "s-1", "--resume"]].concat();
        assert!(Cli::try_parse_from(with_id).is_ok());
        let without = [&base[..], &[kind, "--resume"]].concat();
        assert!(Cli::try_parse_from(without).is_err());
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
