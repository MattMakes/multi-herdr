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
use horch_core::codex;
use horch_core::execution::SessionMode;
use horch_core::herdr::Herdr;
use horch_core::launch::{self, Session};
use horch_core::ledger::Ledger;
use horch_core::mailbox::{Brief, Mailbox};
use horch_core::opencode;
use horch_core::prompts;
use horch_core::runtime::{process, RuntimeContext};
use horch_core::teammates::{Agent, Roster, Teammate};

pub fn worker(ctx: &mut RuntimeContext, role: &str) -> Result<ExitCode> {
    let herdr = Herdr::new();
    let (mailbox, _pane_id) = Mailbox::register_in(&herdr, ctx, role)?;
    let brief = mailbox.read_brief(role)?;

    let project = PathBuf::from(&brief.project_dir);
    if !project.is_dir() {
        bail!("project dir '{}' is missing", brief.project_dir);
    }
    // The process's working directory, not an environment variable: the
    // agent starts here, and a relative roster path resolves from here.
    std::env::set_current_dir(&project)
        .with_context(|| format!("entering project dir {}", brief.project_dir))?;
    ctx.paths.cwd = Some(project.clone());

    // The brief is this worker's context: its project, its ledger and the
    // binary overrides the spawner ran with. A pane is a fresh shell, so none
    // of them are in this process's environment.
    ctx.paths.project_dir = Some(project);
    if let Some(state_dir) = &brief.state_dir {
        ctx.paths.set_state_dir(state_dir);
    }
    ctx.apply_overrides(&brief.overrides());

    // The briefing tells the agent to run `horch tell` / `horch note` / `horch done`
    // by bare name, so this binary's directory has to be reachable. Without it a
    // worker launched from a build directory would have no channel at all.
    let path = ctx.prepend_own_dir_to_path();
    let ctx: &RuntimeContext = ctx;

    // The teammate resolved at spawn time travels in the brief. Falling back to
    // the roster keeps briefs written by an older horch loadable.
    let roster = super::load_roster(ctx, brief.teammates_dir.as_deref())?;
    let teammate = match &brief.resolved {
        Some(t) => t.clone(),
        None => roster.require(&brief.teammate)?.clone(),
    };
    let prompt = prompts::worker_prompt(&roster, &teammate, role, &brief.task, &brief.session)?;

    match teammate.agent {
        Agent::None => run_smoke(&child_env(&mailbox, &brief, path)),
        _ => {
            let env = child_env(&mailbox, &brief, path);
            launch_agent(ctx, &mailbox, &brief, &teammate, &roster, &prompt, env)
        }
    }
}

/// What the agent inherits on top of this process's environment: the brief's
/// transport variables (how `horch note` and `horch done` run from inside the
/// agent know which record they belong to), the workspace this pane
/// registered in, and the PATH that reaches this binary. Applied to the child
/// only; this process's environment is never changed.
fn child_env(
    mailbox: &Mailbox,
    brief: &Brief,
    path: Option<std::ffi::OsString>,
) -> Vec<(String, String)> {
    let mut env = brief.transport_env();
    env.push((
        "HORCH_WORKSPACE_ID".into(),
        mailbox.workspace_id().to_string(),
    ));
    if let Some(path) = path {
        env.push(("PATH".into(), path.to_string_lossy().into_owned()));
    }
    env
}

/// Launch this worker's agent CLI.
///
/// Every flag comes from the teammate file via [`launch::command`]; this
/// function only decides which session shape applies and whether a codex
/// session id needs harvesting.
fn launch_agent(
    ctx: &RuntimeContext,
    mailbox: &Mailbox,
    brief: &Brief,
    teammate: &Teammate,
    roster: &Roster,
    prompt: &str,
    child_env: Vec<(String, String)>,
) -> Result<ExitCode> {
    let skills = horch_core::skills::Bundle::install(&ctx.paths.state_root, teammate)?;
    // Held across the launch: it points codex at a private CODEX_HOME holding
    // only this worker's rules, and is finished once the CLI has exited.
    let rules = if teammate.agent.uses_execpolicy() {
        Some(codex::Rules::install(
            &ctx.paths.home,
            &codex::codex_home(&ctx.paths.home, ctx.inherited.codex_home.as_deref()),
            &ctx.paths.state_root,
            &brief.role,
            roster.exec_rules(),
        )?)
    } else {
        None
    };

    let session = match &brief.session {
        SessionMode::Resume(id) => Session::Resume(id.as_str()),
        SessionMode::Fresh(id) if teammate.agent.mints_session_id() => {
            Session::Fresh(id.as_ref().map(|id| id.as_str()).unwrap_or_default())
        }
        // A fresh codex or opencode session mints its id itself.
        SessionMode::Fresh(_) => Session::Unmanaged,
    };

    // Prime Agent supervises its own sessions, so it gets a socket and a session
    // directory that belong to this pane alone - otherwise `horch done` would
    // leave its daemon running and the fleet would accumulate one per spawn.
    let daemon = if teammate.agent.runs_a_daemon() {
        Some(horch_core::prime::Daemon::install(
            &ctx.paths.state_root,
            &brief.role,
        )?)
    } else {
        None
    };

    let mut launch_teammate = teammate.clone();
    if let Some(daemon) = &daemon {
        // pi-family builders append `--` before the prompt. Daemon flags must
        // be added before that delimiter or Prime treats them as prompt text.
        launch_teammate.args.extend([
            "--daemon-socket".into(),
            daemon.socket().to_string_lossy().into_owned(),
            "--session-dir".into(),
            daemon.sessions_dir().to_string_lossy().into_owned(),
        ]);
    }
    let mut cmd = agent_command(
        ctx,
        &launch_teammate,
        session,
        prompt,
        skills.as_ref(),
        child_env,
    )?;
    if let Some(rules) = &rules {
        if let Some(skills) = &skills {
            rules.attach_skills(&skills.skills_dir())?;
        }
        rules.apply(&mut cmd);
    }
    let name = format!("{:?}", cmd.get_program());

    // Harvest runs only for a fresh codex session, in the background, while
    // codex holds the foreground.
    let harvest = if teammate.agent.harvests_session_id() && !brief.session.is_resume() {
        Some(start_harvest(
            ctx,
            mailbox,
            brief,
            teammate.agent,
            daemon.clone(),
        )?)
    } else {
        None
    };
    let code = run_agent(cmd, name.trim_matches('"'));
    // The harvest is best-effort: a finished agent needs no session id captured.
    if let Some(h) = harvest {
        h.stop();
    }
    if let Some(rules) = rules {
        rules.finish();
    }
    // After the CLI has exited, not before: stopping the daemon early would take
    // the session it is still writing with it.
    if let Some(daemon) = daemon {
        daemon.finish();
    }
    code
}

/// The agent's command: the teammate's launch line, its own environment, then
/// the child environment ([`child_env`]). Nothing here touches this process's
/// environment, and `FORBIDDEN_ENV` stays removed.
fn agent_command(
    ctx: &RuntimeContext,
    teammate: &Teammate,
    session: Session<'_>,
    prompt: &str,
    skills: Option<&horch_core::skills::Bundle>,
    child_env: Vec<(String, String)>,
) -> Result<Command> {
    let mut cmd = launch::command_with_skills_in(
        &launch::LaunchEnv::from_context(ctx),
        teammate,
        session,
        prompt,
        None,
        skills,
    )?;
    process::inherit_env(&mut cmd, launch::teammate_env(teammate));
    process::inherit_env(&mut cmd, child_env);
    process::strip_forbidden(&mut cmd);
    Ok(cmd)
}

/// Handle to the background session-id harvest.
pub(crate) struct Harvest {
    done: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl Harvest {
    pub(crate) fn stop(&self) {
        self.done.store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Poll for the codex session id and record it against this worker's ledger entry.
///
/// Prefers herdr's native `agent_session` (available when the codex integration is
/// installed), falling back to the newest rollout file for this project dir.
pub(crate) fn start_harvest(
    ctx: &RuntimeContext,
    mailbox: &Mailbox,
    brief: &Brief,
    agent: Agent,
    daemon: Option<horch_core::prime::Daemon>,
) -> Result<Harvest> {
    let marker = mailbox.launch_marker(&brief.role);
    std::fs::write(&marker, b"")
        .with_context(|| format!("writing launch marker {}", marker.display()))?;
    let since = std::fs::metadata(&marker)
        .and_then(|m| m.modified())
        .unwrap_or_else(|_| SystemTime::now());

    let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = done.clone();
    let pane_env = ctx
        .herdr
        .pane
        .as_ref()
        .map(|p| p.to_string())
        .unwrap_or_default();
    let sessions_dir = codex::sessions_dir(&ctx.paths.home);
    let log_path = mailbox.harvest_log(&brief.role);
    let role = brief.role.clone();
    let record_id = brief.record_id.clone();
    let project_dir = brief.project_dir.clone();
    let ledger = Ledger::open_in(ctx)?;
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
                // Newest first, and skipping ids already claimed by a
                // concurrently spawned worker. Codex records sessions as
                // rollout files; OpenCode answers `session list`.
                let candidates: Vec<String> = match agent {
                    Agent::OpenCode => opencode::find_sessions(&project_dir, since)
                        .into_iter()
                        .map(|c| c.session_id)
                        .collect(),
                    // Prime writes into a directory this pane owns, so the
                    // session there is unambiguously this worker's. The resume
                    // handle is the file path, which is what `--resume` takes.
                    Agent::Prime => daemon
                        .as_ref()
                        .and_then(|d| horch_core::prime::find_session(d.sessions_dir()))
                        .map(|p| p.to_string_lossy().into_owned())
                        .into_iter()
                        .collect(),
                    _ => codex::find_rollouts(&sessions_dir, &project_dir, since)
                        .into_iter()
                        .map(|c| c.session_id)
                        .collect(),
                };
                session_id = candidates
                    .into_iter()
                    .find(|id| !ledger.has_session(id).unwrap_or(false));
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
                "horch worker[{role}]: could not capture the {agent} session id \
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
fn run_smoke(env: &[(String, String)]) -> Result<ExitCode> {
    run_horch(
        env,
        &[
            "note",
            "smoke: worker launched, brief read, ledger reachable",
        ],
    )?;
    std::thread::sleep(Duration::from_secs(1));
    // `done` closes this pane, which kills this process tree; nothing after it runs.
    run_horch(env, &["done", "smoke: machinery verified end to end"])?;
    Ok(ExitCode::SUCCESS)
}

/// Invoke `horch` the way an agent would: by name, off PATH, with the
/// environment an agent in this pane would have.
fn run_horch(env: &[(String, String)], args: &[&str]) -> Result<()> {
    // A PATH given to the child is also the one `Command` searches.
    let mut cmd = Command::new("horch");
    process::inherit_env(&mut cmd, env.iter().map(|(k, v)| (k, v)));
    let status = cmd.args(args).status().map_err(|e| {
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

#[cfg(test)]
mod tests {
    use super::*;
    use horch_core::execution::SessionMode;
    use horch_core::messaging::brief::SCHEMA;
    use horch_core::runtime::{BinOverrides, MapEnv};

    fn process_env() -> Vec<(std::ffi::OsString, std::ffi::OsString)> {
        let mut vars: Vec<_> = std::env::vars_os().collect();
        vars.sort();
        vars
    }

    /// The brief's transport variables, the workspace and the PATH reach the
    /// agent's command and nothing else: the test process's own environment is
    /// the same afterwards, and `ANTHROPIC_API_KEY` is removed from the child
    /// even when the teammate's `env` block names it.
    #[test]
    fn arc_06_transport_env_applied_to_child() {
        let before = process_env();
        let tmp = tempfile::tempdir().unwrap();
        let mailbox = Mailbox::under(tmp.path(), "w9");
        let mut brief = Brief {
            schema: SCHEMA,
            role: "sonnet-1".into(),
            teammate: "sonnet".into(),
            agent: "claude".into(),
            model: "sonnet".into(),
            record_id: "r1".into(),
            session: SessionMode::Fresh(Some("s1".parse().unwrap())),
            task: "t".into(),
            project_dir: "/p".into(),
            state_dir: Some("/state".into()),
            claude_bin: None,
            codex_bin: None,
            resolved: None,
            teammates_dir: Some("/roster".into()),
            workdir: None,
            bin_overrides: BinOverrides::default(),
        };
        let mut env = MapEnv::new("/");
        for key in BinOverrides::VARS {
            env = env.with(key, &format!("/fake/{key}"));
        }
        brief.set_overrides(BinOverrides::from_env(&env));

        let mut ctx = RuntimeContext::from_env(
            &MapEnv::new("/")
                .with_exe("/opt/horch/bin/horch")
                .with("PATH", "/usr/bin")
                .with("ANTHROPIC_API_KEY", "must-not-leak"),
        )
        .unwrap();
        ctx.apply_overrides(&brief.overrides());
        let path = ctx.prepend_own_dir_to_path();

        let roster = Roster::builtin().unwrap();
        let mut teammate = roster.require("sonnet").unwrap().clone();
        teammate
            .env
            .insert("ANTHROPIC_API_KEY".into(), "from-the-teammate".into());
        let cmd = agent_command(
            &ctx,
            &teammate,
            Session::Fresh("s1"),
            "p",
            None,
            child_env(&mailbox, &brief, path),
        )
        .unwrap();

        let envs: std::collections::BTreeMap<String, Option<String>> = cmd
            .get_envs()
            .map(|(k, v)| {
                (
                    k.to_string_lossy().into_owned(),
                    v.map(|v| v.to_string_lossy().into_owned()),
                )
            })
            .collect();
        for (key, value) in brief.transport_env() {
            assert_eq!(envs.get(&key), Some(&Some(value)), "{key}");
        }
        assert_eq!(envs["HORCH_WORKSPACE_ID"].as_deref(), Some("w9"));
        assert!(envs["PATH"]
            .as_deref()
            .unwrap()
            .starts_with("/opt/horch/bin"));
        assert_eq!(envs.get("ANTHROPIC_API_KEY"), Some(&None), "removed");
        // The resolved claude override is the program itself.
        assert_eq!(
            cmd.get_program(),
            std::ffi::OsStr::new("/fake/HORCH_CLAUDE_BIN")
        );
        assert_eq!(process_env(), before, "the test process env did not change");
    }
}
