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

use std::path::PathBuf;
use std::process::{Command, ExitCode};
use std::time::Duration;

use anyhow::{bail, Context, Result};
use horch_core::harness::launch::{self, DiscoveryTarget, LaunchRequest};
use horch_core::herdr::Herdr;
use horch_core::mailbox::{Brief, Mailbox};
use horch_core::prompts;
use horch_core::runtime::{process, RuntimeContext};
use horch_core::teammates::{Agent, Roster, Teammate};

pub fn worker(ctx: &mut RuntimeContext, role: &str) -> Result<ExitCode> {
    let herdr = Herdr::with_bin(&ctx.bins.harness.herdr);
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

    let env = child_env(&mailbox, &brief, path);
    if teammate.agent == Agent::None {
        return run_smoke(&env);
    }
    launch_agent(ctx, &mailbox, &brief, &teammate, &roster, &prompt, env)
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
/// Every flag comes from the teammate file, and every harness difference
/// (codex rules, the Prime daemon, session discovery) from its harness
/// module: this function only says which pane, record and rules apply.
fn launch_agent(
    ctx: &RuntimeContext,
    mailbox: &Mailbox,
    brief: &Brief,
    teammate: &Teammate,
    roster: &Roster,
    prompt: &str,
    child_env: Vec<(String, String)>,
) -> Result<ExitCode> {
    launch::run_flow(
        ctx,
        LaunchRequest {
            role: &brief.role,
            teammate,
            session: &brief.session,
            prompt,
            model_override: None,
            exec_rules: roster.exec_rules(),
            child_env,
            record: Some(DiscoveryTarget {
                mailbox,
                record_id: &brief.record_id,
                workdir: brief.workdir_or_project(),
            }),
        },
    )
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

#[cfg(test)]
mod tests {
    use super::*;
    use horch_core::execution::SessionMode;
    use horch_core::harness::launch::{agent_command, Session};
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
