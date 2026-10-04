//! Turning a [`Teammate`] into an agent-CLI command line.
//!
//! One builder per agent, shared by every caller. `horch worker` and
//! `horch pane-launch` used to construct their own argv, which is how the
//! orchestrator ended up with an effort level and a model that appeared
//! nowhere in any file. Everything below is derived from the teammate's
//! frontmatter; nothing is hard-coded per call site.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use anyhow::{bail, Context, Result};

use crate::execution::records::Ledger;
use crate::execution::SessionMode;
use crate::messaging::mailbox::Mailbox;
use crate::roster::{ExecRule, Teammate};
#[cfg(test)]
use crate::runtime::BinOverrides;
use crate::runtime::{HarnessBins, RuntimeContext};
use crate::workspace::herdr::Herdr;

use super::{Capabilities, CommandSpec, HarnessKind, PrepareRequest};

/// What a launch reads from its environment: the programs to run, the home
/// the operator's Claude settings live under, and the inherited OpenCode
/// config. Built from a [`RuntimeContext`].
#[derive(Debug, Clone)]
pub struct LaunchEnv {
    pub bins: HarnessBins,
    /// `$HOME` exactly as set (`Inherited::home_var`).
    pub home: Option<PathBuf>,
    /// `$OPENCODE_CONFIG_CONTENT`.
    pub opencode_config_content: Option<String>,
}

impl LaunchEnv {
    pub fn from_context(ctx: &RuntimeContext) -> LaunchEnv {
        LaunchEnv {
            bins: ctx.bins.harness.clone(),
            home: ctx.inherited.home_var.as_ref().map(PathBuf::from),
            opencode_config_content: ctx.inherited.opencode_config_content.clone(),
        }
    }

    /// The bare program names, no home and no inherited config: what a unit
    /// test launches with, whatever the process environment holds.
    #[cfg(test)]
    pub(crate) fn for_test() -> LaunchEnv {
        LaunchEnv {
            bins: HarnessBins::resolve(&BinOverrides::default(), None, None),
            home: None,
            opencode_config_content: None,
        }
    }

    pub fn home(&self) -> Option<&Path> {
        self.home.as_deref()
    }

    /// `~/` in a teammate path, expanded against [`LaunchEnv::home`].
    pub(crate) fn expand_home(&self, path: &str) -> PathBuf {
        crate::roster::expand_home(path, self.home())
    }
}

/// How a launch relates to an agent session.
#[derive(Debug, Clone, Copy)]
pub enum Session<'a> {
    /// Caller-minted id, passed on a fresh start (Claude).
    Fresh(&'a str),
    /// Resume an existing id.
    Resume(&'a str),
    /// The agent mints its own id after launch (Codex), or there is no ledger
    /// record at all (an orchestrator pane).
    Unmanaged,
}

/// Environment a teammate's CLI starts with: its `subagent_model` and its
/// `env` block, the latter winning. The caller gives it to the child command
/// with [`crate::runtime::process::inherit_env`], so a value the builder set
/// on the command still wins, as it did when this was exported into the
/// parent's environment.
pub(crate) fn teammate_env(teammate: &Teammate) -> Vec<(String, String)> {
    let mut out = BTreeMap::new();
    if let Some(sub) = &teammate.subagent_model {
        out.insert("CLAUDE_CODE_SUBAGENT_MODEL".to_string(), sub.clone());
    }
    for (key, value) in &teammate.env {
        out.insert(key.clone(), value.clone());
    }
    out.into_iter().collect()
}

/// Environment variables no agent CLI ever receives. The operator's rule
/// (CLAUDE.md): nothing in a fleet uses `ANTHROPIC_API_KEY`. Claude panes
/// sign in with the operator's claude.ai login. The shell that starts a
/// fleet can export the key, so every launch removes it.
pub const FORBIDDEN_ENV: [&str; 1] = ["ANTHROPIC_API_KEY"];

/// Build the command for a teammate, whichever CLI it names, in the launch
/// environment `env`.
///
/// `model_override` exists for the fixed `orchestration` recipe, where the
/// model belongs to the pane rather than to the teammate file.
pub(crate) fn command_in(
    env: &LaunchEnv,
    teammate: &Teammate,
    session: Session<'_>,
    prompt: &str,
    model_override: Option<&str>,
) -> Result<Command> {
    teammate.agent.adapter().build_command(
        env,
        &CommandSpec {
            teammate,
            session,
            prompt,
            model_override,
        },
    )
}

/// Native skill discovery plus a short routing instruction, in the launch
/// environment `env`. The caller holds the bundle until the child exits so
/// lazy reads remain valid throughout.
pub fn command_with_skills_in(
    env: &LaunchEnv,
    teammate: &Teammate,
    session: Session<'_>,
    prompt: &str,
    model_override: Option<&str>,
    bundle: Option<&crate::skills::Bundle>,
) -> Result<Command> {
    let Some(bundle) = bundle else {
        return command_in(env, teammate, session, prompt, model_override);
    };
    let adapter = teammate.agent.adapter();
    let adjusted = adapter.expose_skills(teammate, bundle, env.home())?;
    let prompt = format!("{}\n{prompt}", bundle.briefing_in(teammate, env.home()));
    let mut cmd = command_in(env, &adjusted, session, &prompt, model_override)?;
    adapter.expose_skills_env(
        &mut cmd,
        teammate,
        bundle,
        env.opencode_config_content.as_deref(),
    )?;
    Ok(cmd)
}

pub(super) fn model_for<'a>(teammate: &'a Teammate, override_: Option<&'a str>) -> Result<&'a str> {
    match override_.or(teammate.model.as_deref()) {
        Some(m) => Ok(m),
        None => bail!(
            "teammate '{}' has no model and none was supplied",
            teammate.name
        ),
    }
}

// ─── the launch flow ────────────────────────────────────────────────────────

/// One agent launch in a pane, as `horch worker` and `horch pane-launch`
/// both describe it.
pub struct LaunchRequest<'a> {
    /// The pane's role. It names the codex home, the Prime daemon and the
    /// launch marker this launch leaves on disk.
    pub role: &'a str,
    pub teammate: &'a Teammate,
    pub session: &'a SessionMode,
    pub prompt: &'a str,
    /// The pane's model, when it is not the teammate file's.
    pub model_override: Option<&'a str>,
    /// The execpolicy rules this pane needs; read only by a harness with
    /// [`Capabilities::exec_policy`](super::Capabilities::exec_policy).
    pub exec_rules: &'a [ExecRule],
    /// What the agent inherits on top of this process's environment.
    pub child_env: Vec<(String, String)>,
    /// The ledger record a discovered session id is written to. `None`:
    /// nothing is recorded, so nothing is discovered.
    pub record: Option<DiscoveryTarget<'a>>,
}

/// Where a discovered session id goes, and which sessions are candidates.
pub struct DiscoveryTarget<'a> {
    pub mailbox: &'a Mailbox,
    pub record_id: &'a str,
    /// The directory the agent works in. Only sessions recorded for this
    /// directory, compared canonically, are candidates.
    pub workdir: &'a str,
}

/// The [`Session`] a launch passes for `mode` on a harness with `caps`.
///
/// A fresh session carries its id only where horch mints it; a harness
/// that mints its own starts with nothing and is discovered afterwards.
pub fn session_for<'a>(caps: &Capabilities, mode: &'a SessionMode) -> Session<'a> {
    match mode {
        SessionMode::Resume(id) => Session::Resume(id.as_str()),
        SessionMode::Fresh(Some(id)) if caps.caller_minted_session => Session::Fresh(id.as_str()),
        SessionMode::Fresh(_) => Session::Unmanaged,
    }
}

/// Run one agent CLI in this pane and propagate its exit status.
///
/// prepare (codex rules, Prime daemon, skills bundle) → build → a session
/// discovery thread unless horch minted the id → wait → clean up.
pub fn run_flow(ctx: &RuntimeContext, req: LaunchRequest<'_>) -> Result<ExitCode> {
    Ok(exit_code(run_flow_code(ctx, req)?))
}

/// [`run_flow`], returning the agent's exit status as a number: `None` when
/// a signal ended it. The worker records it (ARC-18).
pub(crate) fn run_flow_code(ctx: &RuntimeContext, req: LaunchRequest<'_>) -> Result<Option<i32>> {
    let adapter = req.teammate.agent.adapter();
    let caps = adapter.capabilities();
    // Held across the launch, so lazy reads of the bundle remain valid.
    let skills = install_skills(ctx, &req)?;
    let prepared = adapter.prepare(
        ctx,
        &PrepareRequest {
            role: req.role,
            exec_rules: req.exec_rules,
            skills: skills.as_ref(),
        },
    )?;

    let mut teammate = req.teammate.clone();
    // The pane's model goes on the teammate: the builders read an override
    // first and the teammate's model second, so the argv is the same.
    if let Some(model) = req.model_override {
        teammate.model = Some(model.to_string());
    }
    teammate.args.extend(prepared.extra_args.iter().cloned());
    let session = session_for(caps, req.session);
    let mut cmd = agent_command(
        ctx,
        &teammate,
        session,
        req.prompt,
        skills.as_ref(),
        req.child_env,
    )?;
    // Last, so a value the harness prepared (codex's private home) wins.
    for (key, value) in &prepared.env {
        cmd.env(key, value);
    }
    crate::runtime::process::strip_forbidden(&mut cmd);
    let name = format!("{:?}", cmd.get_program());

    // Discovery runs only for a fresh session the agent mints itself, in the
    // background, while the agent holds the foreground.
    let discovery = match &req.record {
        Some(target) if caps.discovers_session() && !req.session.is_resume() => {
            match start_discovery(
                ctx,
                req.role,
                req.teammate.agent,
                target,
                prepared.sessions_dir.clone(),
            ) {
                Ok(d) => Some(d),
                Err(e) => {
                    eprintln!(
                        "horch[{}]: session discovery did not start: {e:#}",
                        req.role
                    );
                    None
                }
            }
        }
        _ => None,
    };
    let code = run_agent_code(cmd, name.trim_matches('"'));
    // Discovery is best-effort: a finished agent needs no session id captured.
    if let Some(d) = discovery {
        d.stop();
    }
    prepared.finish();
    code
}

/// The activated skills of this launch, from the bundled catalog plus the
/// installed marketplace skills. Nothing here touches the network: a
/// marketplace skill is copied from the local store. The directory is named
/// for the execution (the ledger record) when the launch records one and no
/// earlier launch of it left its directory behind; otherwise for a fresh id.
fn install_skills(
    ctx: &RuntimeContext,
    req: &LaunchRequest<'_>,
) -> Result<Option<crate::skills::Bundle>> {
    let catalog = crate::skills::SkillCatalog::installed(&ctx.paths.data_root)?;
    let name = bundle_name(
        req.record.as_ref().map(|r| r.record_id),
        &ctx.paths.state_root.join("skill-bundles"),
    );
    crate::skills::Bundle::install_from(&ctx.paths.state_root, req.teammate, catalog, &name)
}

/// The record id when it is one plain path component that no directory in
/// `bundles` has yet; otherwise a fresh execution id. A legacy record id can
/// hold `/` or `..`, which must never name a directory.
fn bundle_name(record_id: Option<&str>, bundles: &Path) -> String {
    record_id
        .and_then(|id| crate::ids::ExecutionId::new(id).ok())
        .map(|id| id.to_string())
        .filter(|id| !matches!(id.as_str(), "." | "..") && !id.contains(['/', '\\', ':', '\0']))
        .filter(|id| std::fs::symlink_metadata(bundles.join(id)).is_err())
        .unwrap_or_else(|| crate::ids::ExecutionId::mint(crate::clock::now()).to_string())
}

/// The agent's command: the teammate's launch line, its own environment, then
/// `child_env`. Nothing here touches this process's environment, and
/// `FORBIDDEN_ENV` stays removed.
pub(crate) fn agent_command(
    ctx: &RuntimeContext,
    teammate: &Teammate,
    session: Session<'_>,
    prompt: &str,
    skills: Option<&crate::skills::Bundle>,
    child_env: Vec<(String, String)>,
) -> Result<Command> {
    let mut cmd = command_with_skills_in(
        &LaunchEnv::from_context(ctx),
        teammate,
        session,
        prompt,
        None,
        skills,
    )?;
    crate::runtime::process::inherit_env(&mut cmd, teammate_env(teammate));
    crate::runtime::process::inherit_env(&mut cmd, child_env);
    crate::runtime::process::strip_forbidden(&mut cmd);
    Ok(cmd)
}

/// Handle to the background session discovery.
pub(crate) struct Discovery {
    done: Arc<AtomicBool>,
}

impl Discovery {
    pub(crate) fn stop(&self) {
        self.done.store(true, Ordering::Relaxed);
    }
}

/// The waits before each discovery poll: 0.5 s, 1 s, 2 s, then 3 s up to a
/// total of about 3 minutes. A short agent run can end within 3 s.
const POLL_SCHEDULE: [Duration; 62] = {
    let mut schedule = [Duration::from_secs(3); 62];
    schedule[0] = Duration::from_millis(500);
    schedule[1] = Duration::from_secs(1);
    schedule[2] = Duration::from_secs(2);
    schedule
};

/// One discovery attempt: herdr's native `agent_session_id` for the pane,
/// else the harness's own records for `workdir`, newest first, skipping ids
/// already claimed by a concurrently spawned worker.
#[allow(clippy::too_many_arguments)]
fn discover_once(
    ctx: &RuntimeContext,
    agent: HarnessKind,
    herdr: &Herdr,
    pane: &str,
    ledger: &Ledger,
    workdir: &Path,
    since: SystemTime,
    sessions_dir: Option<&Path>,
) -> Option<String> {
    (!pane.is_empty())
        .then(|| herdr.pane_get(pane).ok())
        .flatten()
        .and_then(|p| p.agent_session_id())
        .or_else(|| {
            agent
                .adapter()
                .discover_sessions(ctx, workdir, since, sessions_dir)
                .into_iter()
                .find(|id| !ledger.has_session(id).unwrap_or(false))
        })
}

/// The last discovery attempt, for `horch done` to run before it closes the
/// pane (which kills the discovery thread). Does nothing when the record
/// already has a session id, the harness mints its id at launch, the launch
/// was a resume, or the launch marker is gone. The marker holds the launch's
/// `sessions_dir` (Prime's), so this attempt searches where the thread does.
pub fn discover_now(
    ctx: &RuntimeContext,
    mailbox: &Mailbox,
    role: &str,
    record_id: &str,
) -> Result<()> {
    let brief = mailbox.read_brief(role)?;
    let agent = brief.agent()?;
    if !agent.adapter().capabilities().discovers_session() || brief.session.is_resume() {
        return Ok(());
    }
    let ledger = Ledger::open_in(ctx)?;
    if ledger.get(record_id)?.session_id.is_some() {
        return Ok(());
    }
    let marker = mailbox.launch_marker(role);
    let since = std::fs::metadata(&marker).and_then(|m| m.modified())?;
    let sessions_dir = marker_sessions_dir(&marker);
    let herdr = Herdr::with_bin(&ctx.bins.harness.herdr);
    let pane = ctx
        .herdr
        .pane
        .as_ref()
        .map(|p| p.to_string())
        .unwrap_or_default();
    let workdir = PathBuf::from(brief.workdir_or_project());
    if let Some(session_id) = discover_once(
        ctx,
        agent,
        &herdr,
        &pane,
        &ledger,
        &workdir,
        since,
        sessions_dir.as_deref(),
    ) {
        ledger.set_session(record_id, &session_id)?;
        let _ = std::fs::remove_file(&marker);
    }
    Ok(())
}

/// The launch marker's contents: the launch's `sessions_dir`, or nothing. Its
/// mtime is the launch time; its contents tell [`discover_now`] where the
/// thread searches.
fn marker_contents(sessions_dir: Option<&Path>) -> String {
    sessions_dir
        .map(|d| d.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// The `sessions_dir` a launch marker holds. An empty marker (none, or one
/// an older horch wrote) holds none.
fn marker_sessions_dir(marker: &Path) -> Option<PathBuf> {
    std::fs::read_to_string(marker)
        .ok()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
}

/// Poll for the session id the agent minted and record it against the
/// target's ledger record.
///
/// Prefers herdr's native `agent_session` (available when the agent's herdr
/// integration is installed), falling back to the harness's own records for
/// the target's workdir.
fn start_discovery(
    ctx: &RuntimeContext,
    role: &str,
    agent: HarnessKind,
    target: &DiscoveryTarget<'_>,
    sessions_dir: Option<PathBuf>,
) -> Result<Discovery> {
    let marker = target.mailbox.launch_marker(role);
    std::fs::write(&marker, marker_contents(sessions_dir.as_deref()))
        .with_context(|| format!("writing launch marker {}", marker.display()))?;
    let since = std::fs::metadata(&marker)
        .and_then(|m| m.modified())
        .unwrap_or_else(|_| SystemTime::now());

    let done = Arc::new(AtomicBool::new(false));
    let flag = done.clone();
    let pane = ctx
        .herdr
        .pane
        .as_ref()
        .map(|p| p.to_string())
        .unwrap_or_default();
    let log_path = target.mailbox.harvest_log(role);
    let role = role.to_string();
    let record_id = target.record_id.to_string();
    let workdir = PathBuf::from(target.workdir);
    let ledger = Ledger::open_in(ctx)?;
    let herdr = Herdr::with_bin(&ctx.bins.harness.herdr);
    let ctx = ctx.clone();

    std::thread::spawn(move || {
        for delay in POLL_SCHEDULE {
            std::thread::sleep(delay);
            if flag.load(Ordering::Relaxed) {
                return;
            }

            let found = discover_once(
                &ctx,
                agent,
                &herdr,
                &pane,
                &ledger,
                &workdir,
                since,
                sessions_dir.as_deref(),
            );
            if let Some(session_id) = found {
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

    Ok(Discovery { done })
}

/// The process exit code for an agent's exit status.
fn exit_code(code: Option<i32>) -> ExitCode {
    match code {
        Some(0) => ExitCode::SUCCESS,
        Some(code) => ExitCode::from(code.clamp(1, 255) as u8),
        None => ExitCode::FAILURE,
    }
}

/// Run the agent as a child process and return its exit status.
fn run_agent_code(mut cmd: Command, name: &str) -> Result<Option<i32>> {
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
    Ok(status.code())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::HarnessKind;
    use crate::roster::Roster;

    fn argv(cmd: &Command) -> Vec<String> {
        cmd.get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn phase_skills_are_native_on_all_harnesses_and_resume_keeps_prompt_last() {
        use crate::roster::Phase;
        let r = Roster::builtin().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        for name in ["sonnet", "codex-sol", "opencode-pickle", "pi", "prime"] {
            let mut t = r.require(name).unwrap().clone();
            t.phase = Some(Phase::Research);
            if cfg!(windows) && t.agent == HarnessKind::Codex {
                assert!(crate::skills::Bundle::install(tmp.path(), &t).is_err());
                continue;
            }
            let bundle = crate::skills::Bundle::install(tmp.path(), &t)
                .unwrap()
                .unwrap();
            for session in [Session::Unmanaged, Session::Resume("sid")] {
                let cmd = command_with_skills_in(
                    &LaunchEnv::for_test(),
                    &t,
                    session,
                    "BRIEFING",
                    None,
                    Some(&bundle),
                )
                .unwrap();
                let args = argv(&cmd);
                assert!(
                    args.last().unwrap().ends_with("BRIEFING"),
                    "{name}: {args:?}"
                );
                assert!(args.last().unwrap().contains("Fleet skill phase: research"));
                match t.agent {
                    HarnessKind::Claude => assert!(args.contains(&"--plugin-dir".into())),
                    HarnessKind::Pi | HarnessKind::Prime => {
                        let flag = args.iter().position(|s| s == "--skill").unwrap();
                        assert!(std::path::Path::new(&args[flag + 1])
                            .join("brainstorm/SKILL.md")
                            .exists());
                        assert!(flag < args.iter().position(|s| s == "--").unwrap());
                    }
                    HarnessKind::OpenCode => {
                        let (_, config) = cmd
                            .get_envs()
                            .find(|(k, _)| *k == "OPENCODE_CONFIG_CONTENT")
                            .unwrap();
                        let value: serde_json::Value =
                            serde_json::from_str(config.unwrap().to_str().unwrap()).unwrap();
                        assert!(value["skills"]["paths"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .any(|p| p == &serde_json::json!(bundle.skills_dir())));
                    }
                    HarnessKind::Codex => assert!(!args.contains(&"--skill".into())),
                    HarnessKind::Antigravity | HarnessKind::None => unreachable!(),
                }
            }
        }
    }

    /// No agent CLI receives ANTHROPIC_API_KEY, whatever the shell exports
    /// and whatever a teammate's `env:` says.
    /// The bundle is named for its execution, unless that name is not one
    /// plain path component or is taken; then it gets a fresh id.
    #[test]
    fn bundle_name_is_the_record_id_when_it_is_safe_and_free() {
        let tmp = tempfile::tempdir().unwrap();
        let rec = "0190f2c4-0000-7000-8000-000000000001";
        assert_eq!(bundle_name(Some(rec), tmp.path()), rec);
        std::fs::create_dir(tmp.path().join(rec)).unwrap();
        let fresh = bundle_name(Some(rec), tmp.path());
        assert_ne!(fresh, rec);
        assert!(crate::ids::ExecutionId::new(&fresh).is_ok());
        for bad in ["../escape", "a/b", "a\\b", "..", ".", "C:x", ""] {
            let name = bundle_name(Some(bad), tmp.path());
            assert_ne!(name, bad);
            assert!(!name.contains(['/', '\\', ':']), "{name}");
        }
        assert!(!bundle_name(None, tmp.path()).is_empty());
    }

    #[test]
    fn no_launch_carries_the_anthropic_api_key() {
        let r = Roster::builtin().unwrap();
        for name in r.names() {
            let t = r.require(name).unwrap();
            if t.agent == HarnessKind::None {
                continue;
            }
            let cmd = command_in(
                &LaunchEnv::for_test(),
                t,
                Session::Unmanaged,
                "p",
                Some("m"),
            )
            .unwrap();
            assert!(
                cmd.get_envs()
                    .any(|(k, v)| k == "ANTHROPIC_API_KEY" && v.is_none()),
                "{} does not remove ANTHROPIC_API_KEY",
                t.name
            );
        }
    }

    #[test]
    fn claude_fresh_carries_session_and_mode() {
        let (_home, env) = fake_home("{}");
        let r = Roster::builtin().unwrap();
        let cmd = command_in(
            &env,
            r.require("opus").unwrap(),
            Session::Fresh("sid"),
            "p",
            None,
        )
        .unwrap();
        let a = argv(&cmd);
        assert_eq!(
            a,
            vec![
                // The fleet rule: opus, like every claude fleet pane, denies
                // the subagent tool. See `ai_docs/reports/no-subagents.md`.
                "--disallowedTools",
                "Agent",
                "--model",
                "opus",
                "--effort",
                "medium",
                "--permission-mode",
                "auto",
                "--settings",
                r#"{"enabledPlugins":{"skill-creator@claude-plugins-official":false},"remoteControlAtStartup":false,"skillOverrides":{"anthropic-skills:skill-creator":"off","herdr-orchestrator":"off","herdr-worker":"off","herdr:herdr-orchestrator":"off","herdr:herdr-worker":"off","skill-creator":"off"},"syncClaudeAiSkills":false}"#,
                "--session-id",
                "sid",
                "p"
            ]
        );
    }

    #[test]
    fn claude_resume_swaps_the_session_flag() {
        let r = Roster::builtin().unwrap();
        let cmd = command_in(
            &LaunchEnv::for_test(),
            r.require("sonnet").unwrap(),
            Session::Resume("old"),
            "p",
            None,
        )
        .unwrap();
        assert!(argv(&cmd).contains(&"--resume".to_string()));
        assert!(!argv(&cmd).contains(&"--session-id".to_string()));
    }

    #[test]
    fn codex_resume_leads_with_the_subcommand() {
        let r = Roster::builtin().unwrap();
        let cmd = command_in(
            &LaunchEnv::for_test(),
            r.require("codex-sol").unwrap(),
            Session::Resume("rid"),
            "p",
            None,
        )
        .unwrap();
        let a = argv(&cmd);
        assert_eq!(a[0], "resume");
        assert_eq!(a[1], "-c");
        assert_eq!(a[2], r#"model="gpt-5.6-sol""#);
        assert_eq!(a.last().unwrap(), "p");
        assert!(a.contains(&"rid".to_string()));
    }

    #[test]
    fn codex_permission_mode_becomes_sandbox_and_approval() {
        let r = Roster::builtin().unwrap();
        let cmd = command_in(
            &LaunchEnv::for_test(),
            r.require("codex-terra").unwrap(),
            Session::Unmanaged,
            "p",
            None,
        )
        .unwrap();
        let a = argv(&cmd);
        assert!(
            a.windows(2).any(|w| w == ["-s", "workspace-write"]),
            "{a:?}"
        );
        assert!(a.windows(2).any(|w| w == ["-a", "never"]), "{a:?}");
    }

    #[test]
    fn fixed_recipe_codex_worker_keeps_noninteractive_permissions() {
        let r = Roster::builtin().unwrap();
        let mut t = r.require("orchestration-worker").unwrap().clone();
        t.agent = HarnessKind::Codex;
        t.effort = None;
        let a = argv(
            &command_in(
                &LaunchEnv::for_test(),
                &t,
                Session::Unmanaged,
                "p",
                Some("gpt-5.6-sol"),
            )
            .unwrap(),
        );
        assert!(a.windows(2).any(|w| w == ["-s", "workspace-write"]));
        assert!(a.windows(2).any(|w| w == ["-a", "never"]));
    }

    #[test]
    fn prime_daemon_flags_and_skills_precede_the_prompt_delimiter() {
        let r = Roster::builtin().unwrap();
        let mut t = r.require("prime").unwrap().clone();
        t.args.extend([
            "--daemon-socket".into(),
            "/tmp/socket".into(),
            "--session-dir".into(),
            "/tmp/sessions".into(),
        ]);
        let tmp = tempfile::tempdir().unwrap();
        let bundle = crate::skills::Bundle::install(tmp.path(), &t)
            .unwrap()
            .unwrap();
        let a = argv(
            &command_with_skills_in(
                &LaunchEnv::for_test(),
                &t,
                Session::Unmanaged,
                "p",
                None,
                Some(&bundle),
            )
            .unwrap(),
        );
        let delimiter = a.iter().position(|s| s == "--").unwrap();
        for flag in ["--daemon-socket", "--session-dir", "--skill"] {
            assert!(a.iter().position(|s| s == flag).unwrap() < delimiter);
        }
    }

    #[test]
    fn codex_panes_start_and_resume_without_interactive_setup() {
        let r = Roster::builtin().unwrap();
        for name in ["codex-sol", "codex-terra", "orchestrator-codex"] {
            for session in [Session::Unmanaged, Session::Resume("rid")] {
                let a = argv(
                    &command_in(
                        &LaunchEnv::for_test(),
                        r.require(name).unwrap(),
                        session,
                        "BRIEFING",
                        None,
                    )
                    .unwrap(),
                );
                assert!(
                    a.windows(2)
                        .any(|w| w == ["-c", "check_for_update_on_startup=false"]),
                    "{name}: {a:?}"
                );
                assert!(
                    a.windows(2)
                        .any(|w| w == ["-c", "tui.resume_cwd=\"current\""]),
                    "{name}: {a:?}"
                );
                assert_eq!(
                    a.iter()
                        .filter(|arg| *arg == "--dangerously-bypass-hook-trust")
                        .count(),
                    1,
                    "{name}: {a:?}"
                );
                assert!(
                    a.windows(2).any(|w| w == ["-s", "workspace-write"]),
                    "{name}: {a:?}"
                );
                assert!(a.windows(2).any(|w| w == ["-a", "never"]), "{name}: {a:?}");
                assert!(!a.iter().any(|arg| {
                    arg == "--dangerously-bypass-approvals-and-sandbox"
                        || arg == "danger-full-access"
                }));
                assert_eq!(a.last().unwrap(), "BRIEFING");
            }
        }
    }

    /// The codex orchestrator is the one pane that launches codex with a model,
    /// an effort and a sandbox all at once. The prompt must still land last: a
    /// briefing swallowed by an earlier flag is an orchestrator that comes up
    /// with no instructions and no error.
    #[test]
    fn the_codex_orchestrator_launches_with_astra_and_keeps_its_prompt_last() {
        let r = Roster::builtin().unwrap();
        let t = r.require("orchestrator-codex").unwrap();
        let cmd = command_in(
            &LaunchEnv::for_test(),
            t,
            Session::Unmanaged,
            "BRIEFING",
            None,
        )
        .unwrap();
        let a = argv(&cmd);
        assert!(a.contains(&r#"model="gpt-6-astra""#.to_string()), "{a:?}");
        assert!(
            a.contains(&r#"model_reasoning_effort="xhigh""#.to_string()),
            "{a:?}"
        );
        assert!(
            a.windows(2).any(|w| w == ["-s", "workspace-write"]),
            "{a:?}"
        );
        assert_eq!(a.last().unwrap(), "BRIEFING");
        assert_ne!(a[0], "resume", "a fresh orchestrator is not a resume");
    }

    /// OpenCode takes its prompt as a FLAG, and its reasoning effort is a
    /// provider "variant". A worker also needs `--auto`, or it stops on the
    /// first permission prompt in a pane nobody is watching.
    #[test]
    fn opencode_passes_the_prompt_as_a_flag_and_auto_approves() {
        let r = Roster::builtin().unwrap();
        let cmd = command_in(
            &LaunchEnv::for_test(),
            r.require("opencode-pickle").unwrap(),
            Session::Unmanaged,
            "BRIEFING",
            None,
        )
        .unwrap();
        let a = argv(&cmd);
        assert!(
            a.windows(2)
                .any(|w| w == ["--model", "opencode/big-pickle"]),
            "{a:?}"
        );
        assert!(!a.contains(&"--variant".to_string()), "{a:?}");
        assert!(a.contains(&"--auto".to_string()), "{a:?}");
        assert!(
            a.contains(&"--pure".to_string()),
            "inherit_plugins: false: {a:?}"
        );
        assert_eq!(a[a.len() - 2], "--prompt");
        assert_eq!(a.last().unwrap(), "BRIEFING");
    }

    /// `plugin_skills` narrows a plugin to the skills a teammate names: the
    /// others go off by their plugin-qualified name, the named ones stay on,
    /// and a `--check` catches a name the plugin does not ship.
    #[test]
    fn plugin_skills_switch_off_the_rest_of_the_plugin() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("code");
        for skill in ["review", "lint", "format"] {
            let dir = root.join("skills").join(skill);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(
                dir.join("SKILL.md"),
                format!("---\nname: {skill}\ndescription: Does {skill}.\n---\n"),
            )
            .unwrap();
        }
        let r = Roster::builtin().unwrap();
        let mut t = r.require("opus").unwrap().clone();
        t.plugin_dirs = vec![root.to_string_lossy().into_owned()];
        t.plugin_skills.insert("code".into(), vec!["review".into()]);

        let a =
            argv(&command_in(&LaunchEnv::for_test(), &t, Session::Unmanaged, "p", None).unwrap());
        let at = a.iter().position(|x| x == "--settings").unwrap();
        let overlay: serde_json::Value = serde_json::from_str(&a[at + 1]).unwrap();
        let overrides = &overlay["skillOverrides"];
        assert_eq!(overrides["code:lint"], "off", "{overlay}");
        assert_eq!(overrides["code:format"], "off", "{overlay}");
        assert!(overrides.get("code:review").is_none(), "{overlay}");

        // The briefing names the reinforced one with its description.
        let bundle = crate::skills::Bundle::install(tmp.path(), &t)
            .unwrap()
            .unwrap();
        let brief = bundle.briefing_in(&t, None);
        assert!(brief.contains("- code:review: Does review."), "{brief}");
        assert!(!brief.contains("code:lint"), "{brief}");

        // A skill the plugin does not ship fails the launch and the check.
        t.plugin_skills.insert("code".into(), vec!["deploy".into()]);
        let err = command_in(&LaunchEnv::for_test(), &t, Session::Unmanaged, "p", None)
            .unwrap_err()
            .to_string();
        assert!(err.contains("no skill 'deploy'"), "{err}");
    }

    /// Haiku has no effort setting: an override onto it drops `--effort`
    /// instead of launching a claude that refuses the flag.
    #[test]
    fn a_haiku_override_launches_without_effort() {
        let r = Roster::builtin().unwrap();
        let t = r.require("opus").unwrap();
        assert!(t.effort.is_some());
        let a = argv(
            &command_in(
                &LaunchEnv::for_test(),
                t,
                Session::Unmanaged,
                "p",
                Some("haiku"),
            )
            .unwrap(),
        );
        assert!(a.windows(2).any(|w| w == ["--model", "haiku"]), "{a:?}");
        assert!(!a.contains(&"--effort".to_string()), "{a:?}");
        let a =
            argv(&command_in(&LaunchEnv::for_test(), t, Session::Unmanaged, "p", None).unwrap());
        assert!(a.contains(&"--effort".to_string()), "{a:?}");
    }

    /// A paid OpenCode model's effort reaches it as the build agent's
    /// `variant` in the config overlay - never as `--variant`, which the TUI
    /// swallows - and survives the skill bundle adding its own paths.
    #[test]
    fn opencode_effort_rides_the_config_overlay_alongside_skills() {
        fn overlay(cmd: &Command) -> serde_json::Value {
            let raw = cmd
                .get_envs()
                .find(|(k, _)| *k == "OPENCODE_CONFIG_CONTENT")
                .and_then(|(_, v)| v)
                .expect("OPENCODE_CONFIG_CONTENT is set");
            serde_json::from_str(&raw.to_string_lossy()).unwrap()
        }
        let r = Roster::builtin().unwrap();
        let mut t = r.require("opencode-pickle").unwrap().clone();
        t.model = Some("anthropic/claude-sonnet-5".into());
        t.effort = Some("high".into());
        t.env.insert(
            "OPENCODE_CONFIG_CONTENT".into(),
            r#"{"mcp":{"playwright":{"enabled":false}}}"#.into(),
        );

        let cmd = command_in(&LaunchEnv::for_test(), &t, Session::Unmanaged, "p", None).unwrap();
        assert!(!argv(&cmd).contains(&"--variant".to_string()));
        let v = overlay(&cmd);
        assert_eq!(v["agent"]["build"]["variant"], "high", "{v}");
        assert_eq!(
            v["mcp"]["playwright"]["enabled"], false,
            "the teammate's own overlay stays: {v}"
        );

        let tmp = tempfile::tempdir().unwrap();
        let bundle = crate::skills::Bundle::install(tmp.path(), &t)
            .unwrap()
            .unwrap();
        let cmd = command_with_skills_in(
            &LaunchEnv::for_test(),
            &t,
            Session::Unmanaged,
            "p",
            None,
            Some(&bundle),
        )
        .unwrap();
        let v = overlay(&cmd);
        assert_eq!(v["agent"]["build"]["variant"], "high", "{v}");
        assert!(v["skills"]["paths"].as_array().unwrap().len() == 1, "{v}");

        // No effort, no overlay from the builder.
        t.effort = None;
        t.env.clear();
        let cmd = command_in(&LaunchEnv::for_test(), &t, Session::Unmanaged, "p", None).unwrap();
        assert!(cmd.get_envs().all(|(k, _)| k != "OPENCODE_CONFIG_CONTENT"));
    }

    /// OpenCode mints its own `ses_...` ids, so a fresh launch says nothing
    /// about sessions and a resume passes the harvested one back.
    #[test]
    fn opencode_only_names_a_session_when_resuming() {
        let r = Roster::builtin().unwrap();
        let t = r.require("opencode-ultra").unwrap();
        let fresh = argv(
            &command_in(
                &LaunchEnv::for_test(),
                t,
                Session::Fresh("ignored"),
                "p",
                None,
            )
            .unwrap(),
        );
        assert!(!fresh.contains(&"--session".to_string()), "{fresh:?}");
        let resumed = argv(
            &command_in(
                &LaunchEnv::for_test(),
                t,
                Session::Resume("ses_abc"),
                "p",
                None,
            )
            .unwrap(),
        );
        assert!(
            resumed.windows(2).any(|w| w == ["--session", "ses_abc"]),
            "{resumed:?}"
        );
    }

    /// pi CAN be told its session id, so the ledger knows the resume handle
    /// before the pane starts - the same deal claude gives. `--` fences the
    /// prompt off from option parsing.
    #[test]
    fn pi_takes_a_caller_minted_session_and_fences_its_prompt() {
        let r = Roster::builtin().unwrap();
        let cmd = command_in(
            &LaunchEnv::for_test(),
            r.require("pi").unwrap(),
            Session::Fresh("sid-1"),
            "-x BRIEF",
            None,
        )
        .unwrap();
        let a = argv(&cmd);
        assert!(
            a.windows(2).any(|w| w == ["--model", "ollama/qwen3.8"]),
            "{a:?}"
        );
        assert!(a.windows(2).any(|w| w == ["--thinking", "low"]), "{a:?}");
        assert!(
            a.windows(2).any(|w| w == ["--session-id", "sid-1"]),
            "{a:?}"
        );
        assert!(a.contains(&"--no-extensions".to_string()), "{a:?}");
        assert_eq!(
            a[a.len() - 2],
            "--",
            "a prompt starting with - must not parse as a flag"
        );
        assert_eq!(a.last().unwrap(), "-x BRIEF");
    }

    /// Prime has no `--session-id` (verified against 0.9.4), so a fresh launch
    /// must not invent one - `horch worker` owns its session directory instead.
    #[test]
    fn prime_never_claims_to_set_a_session_id() {
        let r = Roster::builtin().unwrap();
        let t = r.require("prime").unwrap();
        let fresh = argv(
            &command_in(
                &LaunchEnv::for_test(),
                t,
                Session::Fresh("sid-1"),
                "p",
                None,
            )
            .unwrap(),
        );
        assert!(!fresh.contains(&"--session-id".to_string()), "{fresh:?}");
        assert!(!fresh.contains(&"sid-1".to_string()), "{fresh:?}");
        let resumed = argv(
            &command_in(
                &LaunchEnv::for_test(),
                t,
                Session::Resume("/state/s.jsonl"),
                "p",
                None,
            )
            .unwrap(),
        );
        assert!(
            resumed
                .windows(2)
                .any(|w| w == ["--resume", "/state/s.jsonl"]),
            "{resumed:?}"
        );
    }

    /// A permission_mode with no counterpart must stop the launch rather than
    /// quietly running more permissively than the file asked for.
    #[test]
    fn a_mode_the_agent_cannot_express_is_refused() {
        let r = Roster::builtin().unwrap();
        let mut t = r.require("opencode-pickle").unwrap().clone();
        t.permission_mode = Some(crate::roster::PermissionMode::Plan);
        let err = command_in(&LaunchEnv::for_test(), &t, Session::Unmanaged, "p", None)
            .unwrap_err()
            .to_string();
        assert!(err.contains("no opencode equivalent"), "{err}");
    }

    /// A home holding `settings_json` as the operator's Claude settings, and
    /// the launch environment that points at it.
    fn fake_home(settings_json: &str) -> (tempfile::TempDir, LaunchEnv) {
        let home = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(home.path().join(".claude")).unwrap();
        std::fs::write(home.path().join(".claude/settings.json"), settings_json).unwrap();
        let env = LaunchEnv {
            home: Some(home.path().to_path_buf()),
            ..LaunchEnv::for_test()
        };
        (home, env)
    }

    const OPERATOR: &str = r#"{
        "statusLine": {"type":"command","command":"bash sl.sh"},
        "enabledPlugins": {"herdr@m": true, "ddd@m": true, "old@m": false},
        "skillOverrides": {"explain-diff-notion": "off"},
        "disableWorkflows": true
    }"#;

    fn settings_overlay(a: &[String]) -> Option<serde_json::Value> {
        let i = a.iter().position(|x| x == "--settings")?;
        serde_json::from_str(&a[i + 1]).ok()
    }

    /// The orchestrator keeps the operator's settings - they are already tuned
    /// for token economy - and sheds only plugins and MCP servers. Not skills:
    /// `--disable-slash-commands` also removes the built-in `/context` and
    /// `/config`, and there is no way to keep just those two.
    #[test]
    fn the_orchestrator_sheds_plugins_and_mcp_but_keeps_settings_and_builtins() {
        let (_home, env) = fake_home(OPERATOR);
        let r = Roster::builtin().unwrap();
        let cmd = command_in(
            &env,
            r.require("orchestrator").unwrap(),
            Session::Unmanaged,
            "p",
            None,
        )
        .unwrap();
        let a = argv(&cmd);
        assert!(
            !a.contains(&"--setting-sources".to_string()),
            "settings must be inherited: {a:?}"
        );
        assert!(
            !a.contains(&"--disable-slash-commands".to_string()),
            "would remove the built-in /context and /config: {a:?}"
        );
        assert!(a.contains(&"--strict-mcp-config".to_string()), "{a:?}");
        assert!(a.contains(&r#"{"mcpServers":{}}"#.to_string()), "{a:?}");
        assert!(!a.contains(&"--plugin-dir".to_string()), "{a:?}");

        // Only the operator's ENABLED plugins are switched off, by name.
        let overlay = settings_overlay(&a).expect("a --settings overlay");
        assert_eq!(overlay["enabledPlugins"]["herdr@m"], false);
        assert_eq!(overlay["enabledPlugins"]["ddd@m"], false);
        assert!(
            overlay["enabledPlugins"].get("old@m").is_none(),
            "{overlay}"
        );
        // The claude.ai-synced skills and plugins go off too.
        assert_eq!(overlay["syncClaudeAiSkills"], false, "{overlay}");
        assert_eq!(overlay["syncClaudeAiPlugins"], false, "{overlay}");
        // And nothing else rides along - no statusLine override and no tuning.
        assert!(overlay.get("statusLine").is_none(), "{overlay}");
        assert!(overlay.get("disableWorkflows").is_none(), "{overlay}");
        // skillOverrides carries this teammate's own two entries and the
        // ambient skill-creator copies, nothing else. The operator's
        // `explain-diff-notion` override is not copied in: Claude merges
        // settings per key, so it still applies by itself.
        assert_eq!(
            overlay["skillOverrides"],
            serde_json::json!({
                "herdr-orchestrator": "off",
                "herdr-worker": "off",
                "skill-creator": "off",
                "anthropic-skills:skill-creator": "off"
            }),
            "{overlay}"
        );
        // Remote Control is the orchestrator's alone.
        assert_eq!(overlay["remoteControlAtStartup"], true, "{overlay}");
        // Its skill-creator is the bundled `horch:skill-creator`, never the
        // official plugin: skillOverrides cannot hide a plugin skill.
        assert_eq!(
            overlay["enabledPlugins"]["skill-creator@claude-plugins-official"], false,
            "{overlay}"
        );
        assert!(
            a.windows(2).any(|w| w == ["--permission-mode", "auto"]),
            "{a:?}"
        );
        assert!(
            a.windows(2)
                .any(|w| w == ["--disallowedTools", "Agent,RemoteTrigger"]),
            "{a:?}"
        );
    }

    /// `--strict-mcp-config` is what makes an `--mcp-config` subtractive. Without
    /// it the operator's servers load too and the isolation is a no-op.
    #[test]
    fn declared_mcp_servers_are_strict_and_complete() {
        let r = Roster::builtin().unwrap();
        let cmd = command_in(
            &LaunchEnv::for_test(),
            r.require("frontend-developer").unwrap(),
            Session::Unmanaged,
            "p",
            None,
        )
        .unwrap();
        let a = argv(&cmd);
        let json = a
            .iter()
            .find(|x| x.starts_with(r#"{"mcpServers""#))
            .expect("an --mcp-config payload");
        for server in ["playwright", "chrome-devtools", "context7"] {
            assert!(json.contains(server), "{server} missing from {json}");
        }
        assert!(a.contains(&"--strict-mcp-config".to_string()), "{a:?}");
        assert!(
            !a.iter().any(|x| x == "--plugin-dir"),
            "legacy local plugins: {a:?}"
        );
    }

    /// `--mcp-config`, `--tools`, `--allowedTools`, `--disallowedTools` are
    /// variadic. The prompt is the last positional. A variadic flag anywhere
    /// after `--model` could reach it; a live probe lost the prompt this way.
    #[test]
    fn variadic_flags_are_fenced_off_from_the_prompt_by_model() {
        let (_home, env) = fake_home(OPERATOR);
        let r = Roster::builtin().unwrap();
        let mut t = r.require("frontend-developer").unwrap().clone();
        t.tools = Some(vec!["Read".into(), "Bash".into()]);
        t.allowed_tools = vec!["Bash(git *)".into()];
        t.disallowed_tools = vec!["Write".into()];
        t.mcp_config_files = vec!["/tmp/extra.json".into()];
        t.effort = None;
        t.permission_mode = None;
        let cmd = command_in(&env, &t, Session::Unmanaged, "PROMPT", None).unwrap();
        let a = argv(&cmd);
        let model_at = a.iter().position(|x| x == "--model").unwrap();
        for flag in [
            "--mcp-config",
            "--tools",
            "--allowedTools",
            "--disallowedTools",
        ] {
            for (i, x) in a.iter().enumerate() {
                if x == flag {
                    assert!(
                        i < model_at,
                        "{flag} at {i} is after --model at {model_at}: {a:?}"
                    );
                }
            }
        }
        assert_eq!(a.last().unwrap(), "PROMPT");
    }

    /// Plugin paths are written with `~` so a committed file is not bound to one
    /// home directory; they must be expanded before the CLI sees them.
    #[test]
    fn plugin_paths_are_expanded_before_launch() {
        let r = Roster::builtin().unwrap();
        let cmd = command_in(
            &LaunchEnv::for_test(),
            r.require("staff-engineer").unwrap(),
            Session::Unmanaged,
            "p",
            None,
        )
        .unwrap();
        assert!(
            argv(&cmd).iter().all(|a| !a.starts_with('~')),
            "a literal ~ reached the command line"
        );
    }

    /// A teammate that leaves these unset must not gain flags it never asked
    /// for - the generics deliberately inherit the operator's environment. Two
    /// things still ride in the overlay: the claude.ai-synced skills, off in
    /// every pane whether plugins are inherited or not, and this teammate's own
    /// `disabled_skills`, which name the stale external herdr briefings.
    #[test]
    fn unset_isolation_fields_add_no_flags() {
        let (_home, env) = fake_home(OPERATOR);
        let r = Roster::builtin().unwrap();
        let sonnet = r.require("sonnet").unwrap();
        assert!(sonnet.inherit_plugins);
        let cmd = command_in(&env, sonnet, Session::Unmanaged, "p", None).unwrap();
        let a = argv(&cmd);
        for flag in [
            "--setting-sources",
            "--disable-slash-commands",
            "--mcp-config",
            "--strict-mcp-config",
            "--plugin-dir",
        ] {
            assert!(!a.contains(&flag.to_string()), "{flag} appeared: {a:?}");
        }
        let overlay = settings_overlay(&a).expect("a --settings overlay");
        assert_eq!(
            overlay,
            serde_json::json!({
                "syncClaudeAiSkills": false,
                "remoteControlAtStartup": false,
                "enabledPlugins": {"skill-creator@claude-plugins-official": false},
                "skillOverrides": {
                    "herdr-orchestrator": "off",
                    "herdr-worker": "off",
                    "herdr:herdr-orchestrator": "off",
                    "herdr:herdr-worker": "off",
                    "skill-creator": "off",
                    "anthropic-skills:skill-creator": "off"
                }
            })
        );
    }

    /// `inherit_claudeai_skills: true` leaves the switch out. `sonnet` still
    /// gets a `--settings` overlay, because its `disabled_skills` go in the same
    /// place; what must be absent is the `syncClaudeAiSkills` key itself.
    #[test]
    fn opting_in_to_claudeai_skills_leaves_the_switch_out() {
        let (_home, env) = fake_home(OPERATOR);
        let r = Roster::builtin().unwrap();
        let mut sonnet = r.require("sonnet").unwrap().clone();
        sonnet.inherit_claudeai_skills = true;
        let a = argv(&command_in(&env, &sonnet, Session::Unmanaged, "p", None).unwrap());
        let overlay = settings_overlay(&a).expect("a --settings overlay");
        assert_eq!(
            overlay,
            serde_json::json!({"remoteControlAtStartup": false,
                "enabledPlugins": {"skill-creator@claude-plugins-official": false},
                "skillOverrides": {
                "herdr-orchestrator": "off",
                "herdr-worker": "off",
                "herdr:herdr-orchestrator": "off",
                "herdr:herdr-worker": "off",
                "skill-creator": "off",
                "anthropic-skills:skill-creator": "off"
            }}),
            "the synced-skills switch must be the only thing opting out removes"
        );

        // Opting in touches only this switch: the plugin off-map stays.
        let mut orch = r.require("orchestrator").unwrap().clone();
        orch.inherit_claudeai_skills = true;
        let a = argv(&command_in(&env, &orch, Session::Unmanaged, "p", None).unwrap());
        let overlay = settings_overlay(&a).expect("a --settings overlay");
        assert!(overlay.get("syncClaudeAiSkills").is_none(), "{overlay}");
        assert_eq!(overlay["enabledPlugins"]["herdr@m"], false);
    }

    /// Each `disabled_skills` entry is switched off by name, next to the
    /// synced-skills switch. The operator's own overrides are not restated:
    /// Claude merges settings per key, so they still apply.
    #[test]
    fn disabled_skills_become_skill_overrides() {
        let (_home, env) = fake_home(OPERATOR);
        let r = Roster::builtin().unwrap();
        let mut t = r.require("sonnet").unwrap().clone();
        t.disabled_skills = vec!["dev-prime".into(), "code:core".into()];
        let a = argv(&command_in(&env, &t, Session::Unmanaged, "p", None).unwrap());
        let overlay = settings_overlay(&a).expect("a --settings overlay");
        assert_eq!(
            overlay,
            serde_json::json!({
                "syncClaudeAiSkills": false,
                "remoteControlAtStartup": false,
                "enabledPlugins": {"skill-creator@claude-plugins-official": false},
                "skillOverrides": {
                    "dev-prime": "off",
                    "code:core": "off",
                    "skill-creator": "off",
                    "anthropic-skills:skill-creator": "off"
                }
            })
        );
    }

    /// A teammate's own `settings:` file replaces the plain overlay whole, so
    /// that file has to carry the skill switches itself. (The skill-bundle
    /// path in `skills.rs` merges into the file instead.)
    #[test]
    fn a_teammate_settings_file_replaces_the_plain_overlay() {
        let (_home, env) = fake_home(OPERATOR);
        let r = Roster::builtin().unwrap();
        let mut t = r.require("sonnet").unwrap().clone();
        t.settings = Some("/tmp/worker-settings.json".into());
        t.disabled_skills = vec!["dev-prime".into()];
        let a = argv(&command_in(&env, &t, Session::Unmanaged, "p", None).unwrap());
        assert!(
            a.windows(2)
                .any(|w| w == ["--settings", "/tmp/worker-settings.json"]),
            "{a:?}"
        );
        assert!(settings_overlay(&a).is_none(), "{a:?}");
    }

    /// The blunt instrument still keeps the status line: every pane in the
    /// fleet shows the operator's, or the orchestrator runs blind on context
    /// and cost, exactly where that information matters most.
    #[test]
    fn restricted_settings_keep_the_operator_status_line() {
        let (_home, env) = fake_home(OPERATOR);
        let r = Roster::builtin().unwrap();
        let mut t = r.require("sonnet").unwrap().clone();
        t.setting_sources = Some(vec![]);
        t.disable_skills = true;
        let cmd = command_in(&env, &t, Session::Unmanaged, "p", None).unwrap();
        let a = argv(&cmd);
        assert!(
            a.windows(2).any(|w| w == ["--setting-sources", ""]),
            "{a:?}"
        );
        let overlay = settings_overlay(&a).expect("a --settings overlay");
        assert_eq!(overlay["statusLine"]["command"], "bash sl.sh");
    }

    #[test]
    fn a_none_agent_cannot_be_launched() {
        let r = Roster::builtin().unwrap();
        let err = command_in(
            &LaunchEnv::for_test(),
            r.require("smoke").unwrap(),
            Session::Unmanaged,
            "p",
            None,
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("agent: none"), "{err}");
    }

    /// The marker carries the launch's `sessions_dir` to `discover_now`. An
    /// empty marker, as an older horch wrote it, carries none.
    #[test]
    fn launch_marker_carries_the_sessions_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let marker = tmp.path().join(".prime-1.launch-marker");
        let sessions = tmp.path().join("prime/prime-1-x/sessions");
        std::fs::write(&marker, marker_contents(Some(&sessions))).unwrap();
        assert_eq!(marker_sessions_dir(&marker), Some(sessions));
        std::fs::write(&marker, marker_contents(None)).unwrap();
        assert_eq!(marker_sessions_dir(&marker), None);
        assert_eq!(marker_sessions_dir(&tmp.path().join("gone")), None);
    }
}
