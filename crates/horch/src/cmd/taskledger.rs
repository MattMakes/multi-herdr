//! `horch ledger`: the task ledger CLI (docs/specs/task-ledger.md, #28).
//!
//! Orchestrators queue work (`run new`, `add`, `add-map`, `plan-workers`),
//! workers claim and report it (`claim`, `progress`, `note`), and anyone reads
//! the board (`status`, `show`). The old session-ledger plumbing lives under
//! `horch ledger legacy`.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{bail, Context, Result};
use clap::{Args, Subcommand};
use horch_core::runtime::RuntimeContext;
use horch_core::taskledger::git::Git;
use horch_core::taskledger::map;
use horch_core::taskledger::model::{
    Actor, EventKind, ProgressSource, StepName, TaskKind, WorkerKind, WorktreeMode,
};
use horch_core::taskledger::runs::{self, NewRun, Run};
use horch_core::taskledger::status;
use horch_core::taskledger::tasks::{self, ClaimOutcome, Claimed, NewTask};
use horch_core::taskledger::workers::{self, NewWorker};
use horch_core::taskledger::{self, Store};

use super::ledgercmd::LegacyCommand;
use crate::output;

/// Exit code of `status` when work remains.
pub const EXIT_NOT_COMPLETE: u8 = 1;

#[derive(Subcommand)]
pub enum LedgerCommand {
    /// The board of a run. Exits 0 only when every task is done or cancelled
    /// and every worktree is cleaned or unlanded; 1 while work remains.
    Status {
        #[command(flatten)]
        run: RunArg,
        #[arg(long)]
        json: bool,
    },
    /// 1 task (`T-<n>`) or worker (id or role) with its history.
    Show {
        key: String,
        #[command(flatten)]
        run: RunArg,
        #[arg(long)]
        json: bool,
    },
    /// Runs: `run new <slug>` creates a run and its run branch.
    Run {
        #[command(subcommand)]
        command: RunCommand,
    },
    /// Queue 1 task from a ticket file.
    Add(AddArgs),
    /// Queue every ticket of a ticket map (MAP.md) in 1 transaction.
    AddMap {
        map: PathBuf,
        #[command(flatten)]
        run: RunArg,
    },
    /// Per persona: how many workers to spawn now.
    PlanWorkers {
        #[command(flatten)]
        run: RunArg,
        #[arg(long)]
        json: bool,
    },
    /// Let a queued task's fallback persona claim it.
    Fallback {
        task: String,
        reason: String,
        #[command(flatten)]
        run: RunArg,
    },
    /// Drop a task.
    Cancel {
        task: String,
        reason: String,
        #[command(flatten)]
        run: RunArg,
    },
    /// (worker) Take the next task meant for this worker's persona.
    Claim {
        #[arg(long)]
        json: bool,
    },
    /// (worker) Record a milestone; it counts as progress.
    Progress { text: String },
    /// (worker) Record a note; it counts as progress.
    Note { text: String },
    /// Worker rows (plumbing for tests and debugging).
    #[command(hide = true)]
    Worker {
        #[command(subcommand)]
        command: WorkerCommand,
    },
    /// The old session-ledger plumbing.
    Legacy {
        #[command(subcommand)]
        command: LegacyCommand,
    },
    /// Print the task ledger's file path.
    Path,
}

#[derive(Subcommand)]
pub enum RunCommand {
    /// Create a run and its run branch (from the main branch).
    New {
        slug: String,
        /// The branch tasks land on; default `fleet/<slug>`.
        #[arg(long)]
        branch: Option<String>,
        /// The main branch the run branch starts from and the PR targets;
        /// default the branch checked out in the project.
        #[arg(long)]
        base: Option<String>,
        /// The remote the run branch is pushed to; default `origin` when it
        /// exists.
        #[arg(long)]
        remote: Option<String>,
        /// The finish steps, comma-separated; default all 7.
        #[arg(long, value_delimiter = ',')]
        steps: Vec<String>,
    },
    /// Every run of this project.
    List {
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum WorkerCommand {
    /// Insert a worker row and print its id.
    New {
        #[arg(long)]
        persona: String,
        #[arg(long)]
        role: Option<String>,
        #[arg(long)]
        replaces: Option<String>,
        #[command(flatten)]
        run: RunArg,
    },
    /// Record a PID for a worker row (supervisor or agent).
    Pid {
        worker: String,
        #[arg(long)]
        supervisor: Option<u32>,
        #[arg(long)]
        agent: Option<u32>,
    },
}

#[derive(Args, Clone, Default)]
pub struct RunArg {
    /// The run: an id or a slug. Default: the 1 active run of this project.
    #[arg(long = "run", value_name = "RUN")]
    pub run: Option<String>,
}

#[derive(Args)]
pub struct AddArgs {
    /// The ticket file the worker reads.
    #[arg(long = "file")]
    file: PathBuf,
    /// The persona that claims it.
    #[arg(long)]
    primary: String,
    #[arg(long)]
    fallback: Option<String>,
    /// Default: the ticket's first heading.
    #[arg(long)]
    title: Option<String>,
    /// Run alone: no other task of the run runs beside it.
    #[arg(long, conflicts_with = "parallel")]
    exclusive: bool,
    /// May run beside other tasks (the default).
    #[arg(long)]
    parallel: bool,
    /// Tasks this one waits for: `T-<n>` or ids.
    #[arg(long = "blocked-by", value_delimiter = ',')]
    blocked_by: Vec<String>,
    /// Files this task owns.
    #[arg(long = "own", value_delimiter = ',')]
    own: Vec<String>,
    /// Files this task must not touch.
    #[arg(long = "no-touch", value_delimiter = ',')]
    no_touch: Vec<String>,
    /// The check the finish steps run.
    #[arg(long)]
    check: Option<String>,
    #[arg(long)]
    phase: Option<String>,
    #[arg(long)]
    issue: Option<String>,
    #[arg(long)]
    spec: Option<String>,
    #[arg(long, default_value_t = 100)]
    priority: i64,
    /// Work in the shared checkout, not a worktree.
    #[arg(long)]
    shared: bool,
    #[command(flatten)]
    run: RunArg,
}

/// The project root a command means: the main checkout of the repository
/// the project dir is in, else the project dir itself.
pub fn project_root(ctx: &RuntimeContext) -> Result<PathBuf> {
    let dir = ctx.paths.project()?;
    let git = Git::new(&ctx.bins.harness.git);
    Ok(git
        .main_root(&dir)
        .unwrap_or_else(|| dir.canonicalize().unwrap_or(dir)))
}

/// The run a command means: `--run`, else `HORCH_RUN_ID`, else the worker's
/// run, else the project's 1 active run.
pub fn resolve_run(ctx: &RuntimeContext, store: &Store, arg: &RunArg) -> Result<Run> {
    let explicit = arg
        .run
        .clone()
        .or_else(|| std::env::var("HORCH_RUN_ID").ok().filter(|v| !v.is_empty()));
    if explicit.is_none() {
        if let Some(wid) = worker_id_opt(ctx) {
            if let Some(w) = workers::find(store.conn(), &wid)? {
                if let Some(r) = w.run_id {
                    return runs::get(store.conn(), &r);
                }
            }
        }
    }
    runs::resolve(store.conn(), &project_root(ctx)?, explicit.as_deref())
}

fn worker_id_opt(ctx: &RuntimeContext) -> Option<String> {
    ctx.worker.as_ref().and_then(|w| w.worker_id.clone())
}

/// This worker's id, from `HORCH_WORKER_ID`.
pub fn worker_id(ctx: &RuntimeContext) -> Result<String> {
    worker_id_opt(ctx)
        .context("HORCH_WORKER_ID is not set: run this from a worker pane that horch spawn opened")
}

fn json(v: &impl serde::Serialize) -> Result<()> {
    output::println(&serde_json::to_string_pretty(v)?);
    Ok(())
}

pub fn run(ctx: &RuntimeContext, command: LedgerCommand) -> Result<ExitCode> {
    if let LedgerCommand::Legacy { command } = command {
        return super::ledgercmd::run(ctx, command);
    }
    if let LedgerCommand::Path = command {
        output::println(&ctx.paths.ledger_db.display().to_string());
        return Ok(ExitCode::SUCCESS);
    }
    let mut store = taskledger::open(ctx)?;
    match command {
        LedgerCommand::Legacy { .. } | LedgerCommand::Path => unreachable!("handled above"),
        LedgerCommand::Status { run, json: as_json } => status_cmd(ctx, &store, &run, as_json),
        LedgerCommand::Show {
            key,
            run,
            json: as_json,
        } => show(ctx, &store, &key, &run, as_json),
        LedgerCommand::Run { command } => run_cmd(ctx, &mut store, command),
        LedgerCommand::Add(args) => add(ctx, &mut store, args),
        LedgerCommand::AddMap { map: path, run } => add_map(ctx, &mut store, &path, &run),
        LedgerCommand::PlanWorkers { run, json: as_json } => {
            let r = resolve_run(ctx, &store, &run)?;
            let plan = tasks::plan_workers(store.conn(), &r.run_id)?;
            if as_json {
                json(&plan)?;
            } else if plan.is_empty() {
                output::println("No task can start now. Spawn no worker.");
            } else {
                for p in &plan {
                    let mut line = format!(
                        "{}: spawn {} ({} can start, {} on the way).",
                        p.persona, p.spawn, p.startable, p.on_the_way
                    );
                    if !p.replaces.is_empty() {
                        line.push_str(&format!(
                            " Replaces: {}. Spawn those with --replaces <worker>.",
                            p.replaces.join(", ")
                        ));
                    }
                    output::println(&line);
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        LedgerCommand::Fallback { task, reason, run } => {
            let r = resolve_run(ctx, &store, &run)?;
            let t = store.immediate(|tx| {
                let t = tasks::resolve(tx, &r.run_id, &task)?;
                tasks::fallback(tx, &t, &reason)
            })?;
            output::println(&format!(
                "{} can now be claimed by {}.",
                t.short(),
                t.claimable_by.as_deref().unwrap_or("?")
            ));
            Ok(ExitCode::SUCCESS)
        }
        LedgerCommand::Cancel { task, reason, run } => {
            let r = resolve_run(ctx, &store, &run)?;
            let t = store.immediate(|tx| {
                let t = tasks::resolve(tx, &r.run_id, &task)?;
                tasks::cancel(tx, &t, &reason)
            })?;
            output::println(&format!("{} cancelled.", t.short()));
            if t.current_worker_id.is_some() {
                eprintln!(
                    "horch ledger: a worker still holds {}. Stop it with horch ledger stop <worker>.",
                    t.short()
                );
            }
            Ok(ExitCode::SUCCESS)
        }
        LedgerCommand::Claim { json: as_json } => claim(ctx, &mut store, as_json),
        LedgerCommand::Progress { text } => progress(ctx, &mut store, EventKind::Progress, &text),
        LedgerCommand::Note { text } => progress(ctx, &mut store, EventKind::Note, &text),
        LedgerCommand::Worker { command } => worker_cmd(ctx, &mut store, command),
    }
}

fn status_cmd(
    ctx: &RuntimeContext,
    store: &Store,
    arg: &RunArg,
    as_json: bool,
) -> Result<ExitCode> {
    let root = project_root(ctx)?;
    let explicit = arg.run.is_some() || std::env::var("HORCH_RUN_ID").is_ok_and(|v| !v.is_empty());
    let all = runs::of_project(store.conn(), &root)?;
    if all.is_empty() && !explicit && worker_id_opt(ctx).is_none() {
        if as_json {
            json(&serde_json::json!({ "runs": [] }))?;
        } else {
            output::println("No runs.");
        }
        return Ok(ExitCode::SUCCESS);
    }
    let run = resolve_run(ctx, store, arg)?;
    let report = status::report(store.conn(), &run)?;
    if as_json {
        json(&report)?;
    } else {
        output::print(&status::render(&report));
    }
    Ok(if report.complete {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(EXIT_NOT_COMPLETE)
    })
}

fn show(
    ctx: &RuntimeContext,
    store: &Store,
    key: &str,
    arg: &RunArg,
    as_json: bool,
) -> Result<ExitCode> {
    let run = resolve_run(ctx, store, arg)?;
    let conn = store.conn();
    if let Ok(t) = tasks::resolve(conn, &run.run_id, key) {
        let events = status::events(conn, "task_id", &t.task_id)?;
        let attempts = workers::attempts_at(conn, &t.task_id)?;
        let blockers = tasks::blockers(conn, &t.task_id)?;
        let own = tasks::files(
            conn,
            &t.task_id,
            horch_core::taskledger::model::FileMode::Own,
        )?;
        if as_json {
            json(&serde_json::json!({
                "task": t, "events": events, "attempts": attempts,
                "blocked_by": blockers.iter().map(|b| b.short()).collect::<Vec<_>>(),
                "own": own,
            }))?;
        } else {
            output::println(&format!("{} {}: {}", t.short(), t.status, t.title));
            output::println(&format!(
                "Ticket: {}",
                t.task_file.as_deref().unwrap_or("-")
            ));
            output::println(&format!(
                "Persona: {} (primary {}, fallback {}).",
                t.claimable_by.as_deref().unwrap_or("-"),
                t.primary_persona.as_deref().unwrap_or("-"),
                t.fallback_persona.as_deref().unwrap_or("-")
            ));
            if !blockers.is_empty() {
                output::println(&format!(
                    "Blocked by: {}",
                    blockers
                        .iter()
                        .map(|b| format!("{} ({})", b.short(), b.status))
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
            for a in &attempts {
                output::println(&format!(
                    "Attempt {}: {} {}{}",
                    a.attempt.unwrap_or(0),
                    a.label(),
                    a.status,
                    a.end_reason
                        .as_deref()
                        .map(|r| format!(" ({r})"))
                        .unwrap_or_default()
                ));
            }
            for e in events {
                output::println(&format!(
                    "  {} {:<10} {:<13} {}",
                    e.at, e.actor, e.kind, e.text
                ));
            }
        }
        return Ok(ExitCode::SUCCESS);
    }
    let w = workers::resolve(conn, &run.run_id, key)
        .with_context(|| format!("'{key}' is neither a task nor a worker of run {}", run.slug))?;
    let events = status::events(conn, "worker_id", &w.worker_id)?;
    if as_json {
        json(&serde_json::json!({ "worker": w, "events": events }))?;
    } else {
        output::println(&format!(
            "{} {} ({}), persona {}.",
            w.label(),
            w.status,
            w.worker_id,
            w.persona.as_deref().unwrap_or("-")
        ));
        output::println(&format!(
            "Supervisor PID {} (start {}), agent PID {} (start {}), pane {}, host {}.",
            opt(w.supervisor_pid),
            opt(w.supervisor_started),
            opt(w.agent_pid),
            opt(w.agent_started),
            w.pane_id.as_deref().unwrap_or("-"),
            w.host.as_deref().unwrap_or("-")
        ));
        output::println(&format!(
            "Session {}. Transcript {}.",
            w.session_id.as_deref().unwrap_or("-"),
            w.transcript_path.as_deref().unwrap_or("-")
        ));
        output::println(&format!(
            "Last checkin {}. Last progress {} ({}). Stall rounds {}.",
            w.last_checkin_at.as_deref().unwrap_or("-"),
            w.last_progress_at.as_deref().unwrap_or("-"),
            w.last_progress_source.as_deref().unwrap_or("-"),
            w.stall_rounds
        ));
        for e in events {
            output::println(&format!(
                "  {} {:<10} {:<13} {}",
                e.at, e.actor, e.kind, e.text
            ));
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn opt(v: Option<i64>) -> String {
    v.map(|v| v.to_string()).unwrap_or_else(|| "-".into())
}

fn run_cmd(ctx: &RuntimeContext, store: &mut Store, command: RunCommand) -> Result<ExitCode> {
    let root = project_root(ctx)?;
    match command {
        RunCommand::List { json: as_json } => {
            let all = runs::of_project(store.conn(), &root)?;
            if as_json {
                json(&all)?;
            } else if all.is_empty() {
                output::println("No runs.");
            } else {
                for r in all {
                    output::println(&format!(
                        "{} {} {} -> {} ({})",
                        r.slug,
                        r.status,
                        r.run_branch.as_deref().unwrap_or("-"),
                        r.base_branch.as_deref().unwrap_or("-"),
                        r.run_id
                    ));
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        RunCommand::New {
            slug,
            branch,
            base,
            remote,
            steps,
        } => {
            let steps: Vec<StepName> = steps
                .iter()
                .filter(|s| !s.is_empty())
                .map(|s| StepName::parse(s))
                .collect::<Result<_>>()?;
            let git = Git::new(&ctx.bins.harness.git);
            let is_repo = git.main_root(&root).is_some();
            let run_branch = branch.unwrap_or_else(|| format!("fleet/{slug}"));
            let base = base.or_else(|| is_repo.then(|| git.current_branch(&root)).flatten());
            let remote = remote.or_else(|| {
                (is_repo && git.has_remote(&root, "origin")).then(|| "origin".to_string())
            });
            if is_repo {
                let Some(base) = &base else {
                    bail!(
                        "the project has no branch checked out; name the main branch with --base"
                    );
                };
                if !git.branch_exists(&root, &run_branch) {
                    git.run(&root, &["branch", &run_branch, base])?;
                    eprintln!("horch ledger: created run branch {run_branch} from {base}.");
                }
                if let Some(remote) = &remote {
                    let out =
                        git.try_run(&root, &["push", "--set-upstream", remote, &run_branch])?;
                    if !out.ok {
                        eprintln!(
                            "horch ledger: warning: could not push {run_branch} to {remote}: {}",
                            out.stderr
                        );
                    }
                }
            }
            let new = NewRun {
                slug,
                workspace_id: ctx.herdr.workspace.as_ref().map(|w| w.as_str().to_string()),
                base_branch: base,
                run_branch: is_repo.then_some(run_branch),
                remote,
                finish_steps: steps,
            };
            let r = store.immediate(|tx| runs::create(tx, &root, &new))?;
            output::println(&format!(
                "Run {} created ({}). Run branch: {}. Finish steps: {}.",
                r.slug,
                r.run_id,
                r.run_branch.as_deref().unwrap_or("(no git repository)"),
                r.finish_steps.join(", ")
            ));
            Ok(ExitCode::SUCCESS)
        }
    }
}

/// The ticket's first heading, or its file stem.
fn ticket_title(path: &Path) -> String {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| map::ticket_facts(&t).title)
        .unwrap_or_else(|| {
            path.file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "task".into())
        })
}

/// A ticket path as the worker will read it: relative to the project root
/// when it is inside it.
fn ticket_path(root: &Path, file: &Path, cwd: &Path) -> Result<String> {
    let abs = if file.is_absolute() {
        file.to_path_buf()
    } else {
        cwd.join(file)
    };
    if !abs.is_file() {
        bail!("ticket file {} does not exist", abs.display());
    }
    let canon = abs.canonicalize()?;
    Ok(canon
        .strip_prefix(root)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| canon.to_string_lossy().into_owned()))
}

fn add(ctx: &RuntimeContext, store: &mut Store, a: AddArgs) -> Result<ExitCode> {
    let root = project_root(ctx)?;
    let cwd = ctx.paths.current_dir().unwrap_or_else(|_| root.clone());
    let file = ticket_path(&root, &a.file, &cwd)?;
    let facts = std::fs::read_to_string(root.join(&file))
        .map(|t| map::ticket_facts(&t))
        .unwrap_or_default();
    let run = resolve_run(ctx, store, &a.run)?;
    let new = NewTask {
        title: a.title.unwrap_or_else(|| ticket_title(&root.join(&file))),
        task_file: file,
        issue_url: a.issue,
        spec_ref: a.spec,
        primary: a.primary,
        fallback: a.fallback,
        phase: a.phase,
        parallel: !a.exclusive,
        priority: a.priority,
        wave: None,
        worktree_mode: if a.shared {
            WorktreeMode::Shared
        } else {
            WorktreeMode::Worktree
        },
        land_check: a.check.or(facts.check),
        kind: TaskKind::Work,
        parent_task_id: None,
        blocked_by: a.blocked_by,
        own: if a.own.is_empty() { facts.own } else { a.own },
        no_touch: if a.no_touch.is_empty() {
            facts.no_touch
        } else {
            a.no_touch
        },
    };
    let _ = a.parallel;
    let t = store.immediate(|tx| tasks::add(tx, &run, &new, Actor::Orchestrator))?;
    output::println(&format!(
        "{} queued for {}: {}.",
        t.short(),
        t.claimable_by.as_deref().unwrap_or("?"),
        t.title
    ));
    Ok(ExitCode::SUCCESS)
}

fn add_map(ctx: &RuntimeContext, store: &mut Store, path: &Path, arg: &RunArg) -> Result<ExitCode> {
    let root = project_root(ctx)?;
    let cwd = ctx.paths.current_dir().unwrap_or_else(|_| root.clone());
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    };
    let parsed = map::read(&path, &root)?;
    let run = resolve_run(ctx, store, arg)?;
    let items: Vec<(String, NewTask)> = parsed.items.into_iter().map(|i| (i.key, i.task)).collect();
    let added = store.immediate(|tx| tasks::add_many(tx, &run, &items, Actor::Orchestrator))?;
    for t in &added {
        output::println(&format!(
            "{} queued for {}: {}.",
            t.short(),
            t.claimable_by.as_deref().unwrap_or("?"),
            t.title
        ));
    }
    output::println(&format!(
        "{} tasks queued from {}.",
        added.len(),
        path.display()
    ));
    Ok(ExitCode::SUCCESS)
}

fn claim(ctx: &RuntimeContext, store: &mut Store, as_json: bool) -> Result<ExitCode> {
    let wid = worker_id(ctx)?;
    let outcome = store.immediate(|tx| tasks::claim(tx, &wid))?;
    if as_json {
        json(&outcome)?;
        return Ok(ExitCode::SUCCESS);
    }
    match outcome {
        ClaimOutcome::NoTask { tell, persona, .. } => {
            output::println(&format!(
                "No task for {persona}. Send this line, then end your turn:"
            ));
            output::println(&tell);
        }
        ClaimOutcome::Claimed(c) => output::print(&render_claim(&c)),
    }
    Ok(ExitCode::SUCCESS)
}

/// The text a worker reads after a claim.
pub fn render_claim(c: &Claimed) -> String {
    let t = &c.task;
    let mut out = format!(
        "Claimed {} (attempt {}): {}\nTicket: {}. Read it in full and follow it exactly.\n",
        t.short(),
        c.attempt,
        t.title,
        t.task_file.as_deref().unwrap_or("-")
    );
    if let Some(p) = &t.phase {
        out.push_str(&format!("Phase: {p}.\n"));
    }
    if !c.own.is_empty() {
        out.push_str(&format!("Own: {}\n", c.own.join(", ")));
    }
    if !c.do_not_touch.is_empty() {
        out.push_str(&format!("Do not touch: {}\n", c.do_not_touch.join(", ")));
    }
    if t.is_review() {
        out.push_str("This is a review task. Write your findings file as the ticket says, then run: horch ledger done \"<summary>\"\n");
        return out;
    }
    match (&c.worktree, t.uses_worktree()) {
        (Some(path), _) => out.push_str(&format!(
            "Worktree: {path} (branch {}). It holds the earlier attempt's work. Work there.\n",
            c.worktree_branch.as_deref().unwrap_or("?")
        )),
        (None, true) => out.push_str(&format!(
            "Make your worktree first: horch ledger worktree new {}\n",
            slug(&t.title)
        )),
        (None, false) => out.push_str("Work in the shared checkout.\n"),
    }
    if !c.prior.is_empty() {
        out.push_str("Earlier attempts, oldest first:\n");
        for p in &c.prior {
            out.push_str(&format!(
                "  Attempt {} by {}: {}{}.\n    Summary: {}\n    Transcript: {}\n",
                p.attempt.unwrap_or(0),
                p.role.as_deref().unwrap_or(&p.worker_id),
                p.status,
                p.end_reason
                    .as_deref()
                    .map(|r| format!(" ({r})"))
                    .unwrap_or_default(),
                p.summary.as_deref().unwrap_or("(none)"),
                p.transcript_path.as_deref().unwrap_or("(unknown)")
            ));
        }
        out.push_str(
            "Use what you can of the earlier work. Before you continue, read each earlier transcript in chunks to learn what went wrong.\n",
        );
    }
    out.push_str("Record milestones with: horch ledger progress \"<text>\"\n");
    out.push_str("When the work is complete, run: horch ledger finish\n");
    out
}

/// A short branch-safe name from a title.
pub fn slug(title: &str) -> String {
    let mut s: String = title
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    while s.contains("--") {
        s = s.replace("--", "-");
    }
    let s = s.trim_matches('-');
    s.chars()
        .take(40)
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

fn progress(
    ctx: &RuntimeContext,
    store: &mut Store,
    kind: EventKind,
    text: &str,
) -> Result<ExitCode> {
    let wid = worker_id(ctx)?;
    store.immediate(|tx| workers::progress(tx, &wid, ProgressSource::Agent, kind, text))?;
    Ok(ExitCode::SUCCESS)
}

/// `horch note` and `horch done` also write the ledger when the worker has
/// a row; a failure there never blocks them.
pub fn note_from_worker(ctx: &RuntimeContext, text: &str) {
    let Some(wid) = worker_id_opt(ctx) else {
        return;
    };
    let result = taskledger::open(ctx).and_then(|mut s| {
        if workers::find(s.conn(), &wid)?.is_none() {
            return Ok(());
        }
        s.immediate(|tx| workers::progress(tx, &wid, ProgressSource::Agent, EventKind::Note, text))
    });
    if let Err(e) = result {
        eprintln!("horch: the task ledger did not record the note: {e:#}");
    }
}

fn worker_cmd(ctx: &RuntimeContext, store: &mut Store, command: WorkerCommand) -> Result<ExitCode> {
    match command {
        WorkerCommand::New {
            persona,
            role,
            replaces,
            run,
        } => {
            let r = resolve_run(ctx, store, &run)?;
            let id = store.immediate(|tx| {
                workers::insert(
                    tx,
                    &NewWorker {
                        run_id: r.run_id.clone(),
                        kind: Some(WorkerKind::Worker),
                        persona,
                        role,
                        previous_worker_id: replaces,
                        ..Default::default()
                    },
                )
            })?;
            output::println(&id);
        }
        WorkerCommand::Pid {
            worker,
            supervisor,
            agent,
        } => {
            store.immediate(|tx| {
                if let Some(pid) = supervisor {
                    workers::supervisor_started(
                        tx,
                        &worker,
                        pid,
                        horch_core::procid::start_time(pid),
                    )?;
                }
                if let Some(pid) = agent {
                    workers::agent_started(tx, &worker, pid, horch_core::procid::start_time(pid))?;
                }
                Ok(())
            })?;
        }
    }
    Ok(ExitCode::SUCCESS)
}
