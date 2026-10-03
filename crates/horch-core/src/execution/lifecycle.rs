//! A worker's life: [`run_worker`] starts its agent and records how it ended;
//! [`done`] is the worker's own shutdown.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::execution::model::ExecutionStatus;
use crate::execution::store::ExecutionStore;
use crate::harness::launch::{self, DiscoveryTarget, LaunchRequest};
use crate::messaging::brief::Brief;
use crate::messaging::mailbox::Mailbox;
use crate::messaging::message::strip_done_prefix;
use crate::runtime::{process, RuntimeContext};
use crate::teammates::{Agent, Roster};
use crate::workspace::client::WorkspaceClient;
use crate::workspace::herdr::Herdr;

/// Who hears about an execution when it ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReportTarget {
    Orchestrator,
    None,
}

/// What `horch done` knows about the worker that runs it.
#[derive(Debug, Clone)]
pub struct DoneRequest<'a> {
    pub record_id: &'a str,
    pub role: &'a str,
    /// This pane's `HERDR_PANE_ID`, which `pane get` upgrades to the public id.
    pub pane: &'a str,
    /// `HORCH_WORKSPACE_ID` when it is set; else the pane's own workspace.
    pub workspace: Option<&'a str>,
    pub summary: &'a str,
    pub report_to: ReportTarget,
}

/// The steps of [`done`] that are not workspace calls. The CLI runs them
/// against the ledger, `horch tell`, the mailbox and the tiler.
pub trait DoneSteps {
    fn mark_done(&self, record_id: &str, summary: &str) -> Result<()>;
    fn report(&self, line: &str) -> Result<()>;
    fn unregister(&self, workspace: &str, role: &str);
    fn settle(&self, workspace: &str);
}

/// Worker-facing self-shutdown, run only when the work is truly complete (not
/// while waiting on a question).
///
/// The order is fixed: mark done in the ledger, report to the orchestrator,
/// unregister the mailbox entry, settle the grid, close the pane. Everything
/// happens BEFORE the pane close, because closing the pane kills this very
/// process tree. The underlying agent session stays on disk and resumable.
///
/// A failed report is logged and the shutdown goes on; the ledger already
/// holds the summary.
pub fn done(ws: &dyn WorkspaceClient, steps: &dyn DoneSteps, req: &DoneRequest) -> Result<()> {
    let role = req.role;
    // `done` adds the tag and keyword itself; a summary that repeats them would
    // arrive as `[r] DONE: [r] DONE: ...`.
    let summary = strip_done_prefix(req.summary, role);
    steps.mark_done(req.record_id, summary)?;

    if req.report_to == ReportTarget::Orchestrator {
        if let Err(e) = steps.report(&format!("[{role}] DONE: {summary}")) {
            eprintln!(
                "horch done: could not reach orchestrator ({e:#}); ledger is updated, \
                 shutting down anyway"
            );
        }
    }

    let pane = ws.pane_get(req.pane)?;
    let workspace = req
        .workspace
        .map(str::to_owned)
        .or_else(|| pane.workspace_id.clone())
        .context("could not resolve this pane's workspace")?;

    steps.unregister(&workspace, role);

    // A departing worker leaves a hole and hands its width to whichever neighbour
    // happens to be its split sibling, which lumps the grid rather than spreading
    // it. Hand the tidy to a detached child: the close below kills this process
    // tree, so by the time the hole exists, this process is gone. The child pulls
    // the last worker into the free slot, and an overflow tab that lost its last
    // worker closes itself.
    steps.settle(&workspace);

    ws.pane_close(&pane.pane_id)
}

/// The steps of [`run_worker`]. [`PaneWorker`] runs them for real; a test
/// records their order.
pub trait WorkerSteps {
    /// Find this pane's mailbox and read the brief `horch spawn` wrote.
    fn load_brief(&mut self) -> Result<Brief>;
    /// Take the brief's project, ledger and binary overrides as this
    /// process's context.
    fn enter_context(&mut self, brief: &Brief) -> Result<()>;
    /// Register the role in the mailbox, so `horch tell` reaches this pane.
    fn register(&mut self, brief: &Brief) -> Result<()>;
    /// The record is `Running`.
    fn set_running(&mut self, brief: &Brief) -> Result<()>;
    /// Run the agent through the harness flow and wait for it. `None`: a
    /// signal ended it.
    fn launch(&mut self, brief: &Brief) -> Result<Option<i32>>;
    /// Record how the agent ended ([`ExecutionStore::record_exit`]).
    fn agent_exited(&mut self, brief: &Brief, code: Option<i32>) -> Result<()>;
}

/// Run a worker: load the brief, build the context, register the mailbox,
/// set `Running`, launch the agent through the harness flow, wait, and
/// record its exit. Returns the agent's exit code (1 when a signal ended
/// it), which `horch worker` exits with.
///
/// `SPEC-TODO(Spec A §8)`: the startup order verbatim. The worker never
/// calls `done`: the agent runs `horch done`, which closes this pane.
///
/// A ledger write that fails is logged and the worker goes on: the agent
/// must start even when its bookkeeping cannot be written.
pub fn run_worker(steps: &mut dyn WorkerSteps) -> Result<i32> {
    let brief = steps.load_brief()?;
    steps.enter_context(&brief)?;
    steps.register(&brief)?;
    if let Err(e) = steps.set_running(&brief) {
        eprintln!(
            "horch worker[{}]: recording the start failed: {e:#}",
            brief.role
        );
    }
    let code = match steps.launch(&brief) {
        Ok(code) => code,
        Err(e) => {
            if let Err(rec) = steps.agent_exited(&brief, None) {
                eprintln!(
                    "horch worker[{}]: recording the exit failed: {rec:#}",
                    brief.role
                );
            }
            return Err(e);
        }
    };
    if let Err(e) = steps.agent_exited(&brief, code) {
        eprintln!(
            "horch worker[{}]: recording the exit failed: {e:#}",
            brief.role
        );
    }
    Ok(code.unwrap_or(1))
}

/// The worker in a real herdr pane.
pub struct PaneWorker<'a> {
    pub ctx: &'a mut RuntimeContext,
    pub role: &'a str,
    /// Make the project the process's working directory. The CLI does it:
    /// the agent starts there, and a relative roster path resolves from it.
    pub enter_dir: &'a dyn Fn(&Path) -> Result<()>,
    mailbox: Option<Mailbox>,
    path: Option<std::ffi::OsString>,
}

impl<'a> PaneWorker<'a> {
    pub fn new(
        ctx: &'a mut RuntimeContext,
        role: &'a str,
        enter_dir: &'a dyn Fn(&Path) -> Result<()>,
    ) -> Self {
        Self {
            ctx,
            role,
            enter_dir,
            mailbox: None,
            path: None,
        }
    }

    fn herdr(&self) -> Herdr {
        Herdr::with_bin(&self.ctx.bins.harness.herdr)
    }

    fn mailbox(&self) -> Result<&Mailbox> {
        self.mailbox.as_ref().context("the brief is not loaded")
    }

    fn store(&self) -> Result<ExecutionStore> {
        ExecutionStore::open_in(self.ctx)
    }
}

impl WorkerSteps for PaneWorker<'_> {
    fn load_brief(&mut self) -> Result<Brief> {
        let mailbox = Mailbox::resolve_in(&self.herdr(), self.ctx)?;
        let brief = mailbox.read_brief(self.role)?;
        self.mailbox = Some(mailbox);
        Ok(brief)
    }

    fn enter_context(&mut self, brief: &Brief) -> Result<()> {
        let project = PathBuf::from(&brief.project_dir);
        if !project.is_dir() {
            bail!("project dir '{}' is missing", brief.project_dir);
        }
        (self.enter_dir)(&project)
            .with_context(|| format!("entering project dir {}", brief.project_dir))?;
        self.ctx.paths.cwd = Some(project.clone());
        // The brief is this worker's context: its project, its ledger and
        // the binary overrides the spawner ran with. A pane is a fresh
        // shell, so none of them are in this process's environment.
        self.ctx.paths.project_dir = Some(project);
        if let Some(state_dir) = &brief.state_dir {
            self.ctx.paths.set_state_dir(state_dir);
        }
        self.ctx.apply_overrides(&brief.overrides());
        // The briefing tells the agent to run `horch tell` / `horch note` /
        // `horch done` by bare name, so this binary's directory has to be
        // reachable. Without it a worker launched from a build directory
        // would have no channel at all.
        self.path = self.ctx.prepend_own_dir_to_path();
        Ok(())
    }

    fn register(&mut self, _brief: &Brief) -> Result<()> {
        let herdr = self.herdr();
        let (mailbox, _pane) = Mailbox::register_in(&herdr, self.ctx, self.role)?;
        self.mailbox = Some(mailbox);
        Ok(())
    }

    fn set_running(&mut self, brief: &Brief) -> Result<()> {
        self.store()?
            .set_state(&brief.record_id, ExecutionStatus::Running)
    }

    fn launch(&mut self, brief: &Brief) -> Result<Option<i32>> {
        let ctx: &RuntimeContext = self.ctx;
        let mailbox = self.mailbox()?;
        // The teammate resolved at spawn time travels in the brief. Falling
        // back to the roster keeps briefs written by an older horch loadable.
        let roster = Roster::load_layered(
            ctx.inherited.home_var.as_deref().map(Path::new),
            ctx.bins.roster_override.as_deref(),
            brief.teammates_dir.as_deref(),
        )?;
        let teammate = match &brief.resolved {
            Some(t) => t.clone(),
            None => roster.require(&brief.teammate)?.clone(),
        };
        let prompt = crate::prompts::worker_prompt(
            &roster,
            &teammate,
            self.role,
            &brief.task,
            &brief.session,
        )?;
        let env = child_env(mailbox, brief, self.path.clone());
        if teammate.agent == Agent::None {
            return run_smoke(&env).map(|()| Some(0));
        }
        // Every flag comes from the teammate file, and every harness
        // difference (codex rules, the Prime daemon, session discovery) from
        // its harness module: this only says which pane, record and rules
        // apply.
        launch::run_flow_code(
            ctx,
            LaunchRequest {
                role: &brief.role,
                teammate: &teammate,
                session: &brief.session,
                prompt: &prompt,
                model_override: None,
                exec_rules: roster.exec_rules(),
                child_env: env,
                record: Some(DiscoveryTarget {
                    mailbox,
                    record_id: &brief.record_id,
                    workdir: brief.workdir_or_project(),
                }),
            },
        )
    }

    fn agent_exited(&mut self, brief: &Brief, code: Option<i32>) -> Result<()> {
        self.store()?.record_exit(&brief.record_id, code)
    }
}

/// What the agent inherits on top of this process's environment: the brief's
/// transport variables (how `horch note` and `horch done` run from inside the
/// agent know which record they belong to), the workspace this pane
/// registered in, and the PATH that reaches this binary. Applied to the child
/// only; this process's environment is never changed.
pub fn child_env(
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

/// The smoke teammate: a fake agent that verifies the machinery (spawn,
/// register, ledger, tell, self-close) without spending any tokens.
///
/// These run as subprocesses invoked by bare name, deliberately: that is
/// exactly how a real agent calls them from its shell tool, so the check also
/// proves `horch` is reachable on the PATH the briefings promise.
fn run_smoke(env: &[(String, String)]) -> Result<()> {
    run_horch(
        env,
        &[
            "note",
            "smoke: worker launched, brief read, ledger reachable",
        ],
    )?;
    std::thread::sleep(Duration::from_secs(1));
    // `done` closes this pane, which kills this process tree; nothing after
    // it runs.
    run_horch(env, &["done", "smoke: machinery verified end to end"])
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
    use crate::execution::SessionMode;
    use crate::harness::launch::{agent_command, Session};
    use crate::messaging::brief::SCHEMA;
    use crate::runtime::{BinOverrides, MapEnv};

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
