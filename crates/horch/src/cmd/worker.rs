//! `horch worker` - the port of `scripts/herdr-fleet/worker.sh`.
//!
//! `horch spawn` writes a brief (the resolved teammate, model, task, session and
//! record ids) into
//! the workspace mailbox and then runs this command in the fresh pane. This
//! registers the role, renders that teammate's briefing, and launches the right
//! agent CLI - fresh or resuming a previous session id.
//!
//! The bash version `exec`ed the agent. There is no `exec` on Windows, so the
//! agent runs as a child and its exit status is propagated. That also turns the
//! backgrounded codex session-id harvest into an ordinary thread of this process.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, ExitCode};
use std::time::{Duration, SystemTime};

use anyhow::{bail, Context, Result};
use horch_core::agent;
use horch_core::codex;
use horch_core::herdr::Herdr;
use horch_core::ledger::Ledger;
use horch_core::launch::{self, Session};
use horch_core::mailbox::{Brief, Mailbox};
use horch_core::prompts;
use horch_core::teammates::{Agent, Roster, Teammate};

pub fn worker(role: &str) -> Result<ExitCode> {
    let herdr = Herdr::new();
    let (mailbox, _pane_id) = Mailbox::register(&herdr, role)?;
    let brief = mailbox.read_brief(role)?;

    let project = PathBuf::from(&brief.project_dir);
    if !project.is_dir() {
        bail!("project dir '{}' is missing", brief.project_dir);
    }
    std::env::set_current_dir(&project)
        .with_context(|| format!("entering project dir {}", brief.project_dir))?;

    // Publish the brief into this process's environment. The agent inherits it,
    // which is how `horch note` and `horch done` run from inside the agent know
    // which record they belong to.
    export_brief(&brief);

    // The briefing tells the agent to run `horch tell` / `horch note` / `horch done`
    // by bare name, so this binary's directory has to be reachable. Without it a
    // worker launched from a build directory would have no channel at all.
    if let Err(e) = agent::prepend_own_dir_to_path() {
        eprintln!("horch worker[{role}]: could not add horch to PATH: {e}");
    }

    // The teammate resolved at spawn time travels in the brief. Falling back to
    // the roster keeps briefs written by an older horch loadable.
    let teammate = match &brief.resolved {
        Some(t) => t.clone(),
        None => Roster::load_with(brief.teammates_dir.as_deref())?
            .require(&brief.teammate)?
            .clone(),
    };
    let roster = Roster::load_with(brief.teammates_dir.as_deref())?;
    let prompt = prompts::worker_prompt(&roster, &teammate, role, &brief.task, brief.resume)?;

    match teammate.agent {
        Agent::None => run_smoke(),
        _ => launch_agent(&mailbox, &brief, &teammate, &roster, &prompt),
    }
}

fn export_brief(brief: &Brief) {
    std::env::set_var("HORCH_ROLE", &brief.role);
    std::env::set_var("HORCH_TEAMMATE", &brief.teammate);
    std::env::set_var("HORCH_AGENT", &brief.agent);
    std::env::set_var("HORCH_MODEL", &brief.model);
    std::env::set_var("HORCH_RECORD_ID", &brief.record_id);
    std::env::set_var("HORCH_SESSION_ID", &brief.session_id);
    std::env::set_var("HORCH_RESUME", if brief.resume { "1" } else { "0" });
    std::env::set_var("HORCH_TASK", &brief.task);
    std::env::set_var("HORCH_PROJECT_DIR", &brief.project_dir);
    if let Some(state_dir) = &brief.state_dir {
        std::env::set_var("HORCH_STATE_DIR", state_dir);
    }
    // Republish the agent-CLI overrides the spawner was run with, so
    // `agent::claude_bin()` / `codex_bin()` below see them in this fresh shell.
    if let Some(bin) = &brief.claude_bin {
        std::env::set_var("HORCH_CLAUDE_BIN", bin);
    }
    if let Some(bin) = &brief.codex_bin {
        std::env::set_var("HORCH_CODEX_BIN", bin);
    }
    // The agent runs `horch spawn` and `horch done` from inside this pane; they
    // must resolve the same roster this worker was briefed from.
    if let Some(dir) = &brief.teammates_dir {
        std::env::set_var("HORCH_TEAMMATES_DIR", dir);
    }
}

/// Launch this worker's agent CLI.
///
/// Every flag comes from the teammate file via [`launch::command`]; this
/// function only decides which session shape applies and whether a codex
/// session id needs harvesting.
fn launch_agent(
    mailbox: &Mailbox,
    brief: &Brief,
    teammate: &Teammate,
    roster: &Roster,
    prompt: &str,
) -> Result<ExitCode> {
    launch::apply_env(teammate);
    if teammate.agent == Agent::Codex {
        codex::ensure_rules(&agent::home_dir(), roster)?;
    }

    let session = if brief.resume {
        Session::Resume(&brief.session_id)
    } else if teammate.agent == Agent::Claude {
        Session::Fresh(&brief.session_id)
    } else {
        // A fresh codex session mints its id itself.
        Session::Unmanaged
    };

    let cmd = launch::command(teammate, session, prompt, None)?;
    let name = format!("{:?}", cmd.get_program());

    // Harvest runs only for a fresh codex session, in the background, while
    // codex holds the foreground.
    let harvest = if teammate.agent == Agent::Codex && !brief.resume {
        Some(start_harvest(mailbox, brief)?)
    } else {
        None
    };
    let code = run_agent(cmd, name.trim_matches('"'));
    // The harvest is best-effort: a finished agent needs no session id captured.
    if let Some(h) = harvest {
        h.stop();
    }
    code
}

/// Handle to the background session-id harvest.
struct Harvest {
    done: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl Harvest {
    fn stop(&self) {
        self.done.store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Poll for the codex session id and record it against this worker's ledger entry.
///
/// Prefers herdr's native `agent_session` (available when the codex integration is
/// installed), falling back to the newest rollout file for this project dir.
fn start_harvest(mailbox: &Mailbox, brief: &Brief) -> Result<Harvest> {
    let marker = mailbox.launch_marker(&brief.role);
    std::fs::write(&marker, b"")
        .with_context(|| format!("writing launch marker {}", marker.display()))?;
    let since = std::fs::metadata(&marker)
        .and_then(|m| m.modified())
        .unwrap_or_else(|_| SystemTime::now());

    let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = done.clone();
    let pane_env = std::env::var("HERDR_PANE_ID").unwrap_or_default();
    let sessions_dir = codex::sessions_dir(&agent::home_dir());
    let log_path = mailbox.harvest_log(&brief.role);
    let role = brief.role.clone();
    let record_id = brief.record_id.clone();
    let project_dir = brief.project_dir.clone();
    let ledger = Ledger::open()?;
    let herdr = Herdr::new();

    std::thread::spawn(move || {
        for _ in 0..60 {
            std::thread::sleep(Duration::from_secs(3));
            if flag.load(std::sync::atomic::Ordering::Relaxed) {
                return;
            }

            let mut session_id = (!pane_env.is_empty())
                .then(|| herdr.pane_get(&pane_env).ok())
                .flatten()
                .and_then(|p| p.agent_session_id());

            if session_id.is_none() {
                session_id = codex::find_rollouts(&sessions_dir, &project_dir, since)
                    .into_iter()
                    // Skip ids already claimed by a concurrently spawned worker.
                    .find(|c| !ledger.has_session(&c.session_id).unwrap_or(false))
                    .map(|c| c.session_id);
            }

            if let Some(session_id) = session_id {
                if let Err(e) = ledger.set_session(&record_id, &session_id) {
                    eprintln!("horch worker[{role}]: recording session id failed: {e:#}");
                    return;
                }
                let _ = std::fs::remove_file(&marker);
                return;
            }
        }
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
        {
            let _ = writeln!(
                file,
                "horch worker[{role}]: could not capture codex session id \
                 (resume disabled for this session)"
            );
        }
    });

    Ok(Harvest { done })
}

/// The smoke teammate: a fake agent that verifies the machinery (spawn, register,
/// ledger, tell, self-close) without spending any tokens.
///
/// These run as subprocesses invoked by bare name, deliberately: that is exactly
/// how a real agent calls them from its shell tool, so the check also proves
/// `horch` is reachable on the PATH the briefings promise. Calling the library
/// functions directly would pass even when no agent could find the binary.
fn run_smoke() -> Result<ExitCode> {
    run_horch(&["note", "smoke: worker launched, brief read, ledger reachable"])?;
    std::thread::sleep(Duration::from_secs(1));
    // `done` closes this pane, which kills this process tree; nothing after it runs.
    run_horch(&["done", "smoke: machinery verified end to end"])?;
    Ok(ExitCode::SUCCESS)
}

/// Invoke `horch` the way an agent would: by name, off PATH.
fn run_horch(args: &[&str]) -> Result<()> {
    let status = Command::new("horch").args(args).status().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            anyhow::anyhow!(
                "`horch` is not on PATH, so an agent in this pane could not reach the \
                 orchestrator either. Run `horch install` to put it on PATH."
            )
        } else {
            anyhow::Error::new(e).context("running horch")
        }
    })?;
    if !status.success() {
        bail!("horch {} failed: {status}", args.join(" "));
    }
    Ok(())
}

/// Run the agent as a child process and propagate its exit status.
fn run_agent(mut cmd: Command, name: &str) -> Result<ExitCode> {
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
