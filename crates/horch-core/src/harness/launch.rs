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

use crate::compaction::window::{self, WindowDecision, WindowSource};
use crate::execution::legacy::LedgerWindow;
use crate::execution::records::Ledger;
use crate::execution::SessionMode;
use crate::messaging::delivery::{self, Readiness, Timing};
use crate::messaging::mailbox::Mailbox;
use crate::roster::{ExecRule, HarnessDefault, Teammate};
use crate::runtime::bins::{host_tool_bin, BLENDER_APP};
use crate::runtime::context::DEFAULT_ENV_YIELD;
#[cfg(test)]
use crate::runtime::BinOverrides;
use crate::runtime::{HarnessBins, RuntimeContext};
use crate::workspace::client::WorkspaceClient;
use crate::workspace::herdr::Herdr;

use super::{Capabilities, CommandSpec, HarnessKind, PrepareRequest, WindowInputs};

/// What a launch reads from its environment: the programs to run, the home
/// the operator's Claude settings live under, the inherited OpenCode config,
/// and where the host tools are. Built from a [`RuntimeContext`].
#[derive(Debug, Clone)]
pub struct LaunchEnv {
    pub bins: HarnessBins,
    /// `$HOME` exactly as set (`Inherited::home_var`).
    pub home: Option<PathBuf>,
    /// `$OPENCODE_CONFIG_CONTENT`.
    pub opencode_config_content: Option<String>,
    /// `$PATH`, where a sandboxed launch looks for its helper programs.
    pub path: Option<std::ffi::OsString>,
    /// `$PATHEXT` (Windows).
    pub pathext: Option<String>,
    /// The operator's `$BLENDER_PATH`.
    pub blender_path: Option<std::ffi::OsString>,
    /// Blender's app bundle executable ([`BLENDER_APP`]), the last place a
    /// `{path_of:blender}` lookup tries. A test leaves it `None`, so it never
    /// finds the machine's real Blender.
    pub blender_app: Option<PathBuf>,
    /// The operator's values for the `DEFAULT_ENV_YIELD` names
    /// (`Inherited::operator_env`): a non-Claude harness default `env` key
    /// yields to them. An allow-list, so no other variable is carried.
    pub operator_env: BTreeMap<String, String>,
    /// `$CODEX_HOME`: the codex home whose `config.toml` a codex default
    /// `-c` pair yields to (`codex::codex_home`).
    pub codex_home: Option<PathBuf>,
    /// `$CLAUDE_CONFIG_DIR`: where the operator's Claude user settings live.
    pub claude_config_dir: Option<PathBuf>,
    /// `RuntimeContext.paths.claude_managed_settings`. `None` reads no
    /// managed settings.
    pub claude_managed_settings: Option<PathBuf>,
    /// The directory the agent starts in (the bootstrap cwd), whose
    /// `.claude/` project settings a Claude settings default yields to.
    pub workdir: Option<PathBuf>,
    /// The launch's native-window decision (CTX-05). A builder applies the
    /// window only when it is `applied`. Only `run_flow_code` sets it, so a
    /// command built from [`LaunchEnv::from_context`] alone carries none.
    pub compact_window: Option<WindowDecision>,
}

impl LaunchEnv {
    pub fn from_context(ctx: &RuntimeContext) -> LaunchEnv {
        LaunchEnv {
            bins: ctx.bins.harness.clone(),
            home: ctx.inherited.home_var.as_ref().map(PathBuf::from),
            opencode_config_content: ctx.inherited.opencode_config_content.clone(),
            path: ctx.inherited.path.clone(),
            pathext: ctx.inherited.pathext.clone(),
            blender_path: ctx.inherited.blender_path.clone(),
            blender_app: BLENDER_APP.map(PathBuf::from),
            operator_env: DEFAULT_ENV_YIELD
                .iter()
                .filter_map(|k| Some((k.to_string(), ctx.inherited.operator_env(k)?.to_string())))
                .collect(),
            codex_home: ctx.inherited.codex_home.clone(),
            claude_config_dir: ctx.inherited.claude_config_dir.clone(),
            claude_managed_settings: Some(ctx.paths.claude_managed_settings.clone()),
            workdir: ctx.paths.cwd.clone(),
            compact_window: None,
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
            path: None,
            pathext: None,
            blender_path: None,
            blender_app: None,
            operator_env: BTreeMap::new(),
            codex_home: None,
            claude_config_dir: None,
            claude_managed_settings: None,
            workdir: None,
            compact_window: None,
        }
    }

    pub fn home(&self) -> Option<&Path> {
        self.home.as_deref()
    }

    /// `~/` in a teammate path, expanded against [`LaunchEnv::home`].
    pub(crate) fn expand_home(&self, path: &str) -> PathBuf {
        crate::roster::expand_home(path, self.home())
    }

    /// The executable of the host tool `name` names, by the lookup `horch
    /// doctor` checks ([`host_tool_bin`]). `None` when it is not found, or
    /// when `name` is not a tool horch can look up: only `blender`.
    pub(crate) fn host_tool(&self, name: &str) -> Option<PathBuf> {
        match name {
            "blender" => host_tool_bin(
                self.blender_path.as_deref(),
                self.path.as_deref(),
                self.pathext.as_deref(),
                "blender",
                self.blender_app.as_deref(),
            ),
            _ => None,
        }
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
/// `env` block, the latter winning, then the `env` of its harness defaults
/// for the keys it does not set. The caller gives it to the child command
/// with `runtime::process::inherit_env`, so a value the builder set
/// on the command still wins, as it did when this was exported into the
/// parent's environment.
///
/// An `env` value that starts with `~/` expands against the launch's home,
/// like a teammate path. The CLI starts without a shell, so nothing else
/// expands it, and a committed file cannot carry an absolute home path.
///
/// An `env` value that is exactly `{path_of:<tool>}` becomes that host tool's
/// executable (`LaunchEnv::host_tool`). When the tool is not found, the
/// variable is left out, so the launch still works and the CLI keeps what
/// it inherits.
pub fn teammate_env(teammate: &Teammate, env: &LaunchEnv) -> Vec<(String, String)> {
    let mut out = BTreeMap::new();
    if let Some(sub) = &teammate.subagent_model {
        out.insert("CLAUDE_CODE_SUBAGENT_MODEL".to_string(), sub.clone());
    }
    for (key, value) in &teammate.env {
        let value = if let Some(tool) = path_of(value) {
            match env.host_tool(tool) {
                Some(bin) => bin.to_string_lossy().into_owned(),
                None => continue,
            }
        } else if value.starts_with("~/") {
            env.expand_home(value).to_string_lossy().into_owned()
        } else {
            value.clone()
        };
        out.insert(key.clone(), value);
    }
    // Harness defaults of the final harness, under the teammate's own keys.
    // A later entry wins over an earlier one. A non-Claude default yields to
    // the operator's own value (Claude's settings `env` already beats the
    // process env).
    let mut defaults = BTreeMap::new();
    for entry in HarnessDefault::for_harness(&teammate.harness_defaults, teammate.agent) {
        for (key, value) in &entry.env {
            if out.contains_key(key)
                || (teammate.agent != HarnessKind::Claude && env.operator_env.contains_key(key))
            {
                continue;
            }
            defaults.insert(key.clone(), value.clone());
        }
    }
    out.extend(defaults);
    out.into_iter().collect()
}

/// The tool a `{path_of:<tool>}` env value names.
fn path_of(value: &str) -> Option<&str> {
    value.strip_prefix("{path_of:")?.strip_suffix('}')
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
        return command_in(
            &plain_env(env, teammate),
            teammate,
            session,
            prompt,
            model_override,
        );
    };
    let adapter = teammate.agent.adapter();
    let prompt = format!("{}\n{prompt}", bundle.briefing_in(teammate, env.home()));
    // A bundle with an empty plan carries only the briefing's `Skipped:`
    // notes: there is no skill directory to expose.
    if bundle.plan().activated.is_empty() {
        return command_in(
            &plain_env(env, teammate),
            teammate,
            session,
            &prompt,
            model_override,
        );
    }
    let adjusted = adapter.expose_skills(teammate, bundle, env.home())?;
    let mut cmd = command_in(env, &adjusted, session, &prompt, model_override)?;
    adapter.expose_skills_env(
        &mut cmd,
        teammate,
        bundle,
        env.opencode_config_content.as_deref(),
    )?;
    Ok(cmd)
}

/// The launch env of a launch without exposed skills. A teammate's own
/// `settings:` (Claude) then owns the whole `--settings` overlay, so horch
/// adds no window to it; the skills overlay is horch's, and gets one.
fn plain_env<'a>(env: &'a LaunchEnv, teammate: &Teammate) -> std::borrow::Cow<'a, LaunchEnv> {
    if teammate.settings.is_some() && env.compact_window.is_some() {
        std::borrow::Cow::Owned(LaunchEnv {
            compact_window: None,
            ..env.clone()
        })
    } else {
        std::borrow::Cow::Borrowed(env)
    }
}

/// What this launch does about the harness's native auto-compact window
/// (CTX-05, design §6.5): the operator's value, else the fleet default
/// (applied), else the harness's own. A Claude teammate that names a
/// settings file cannot take a merged key, so a fleet window is not
/// applied. Used by `run_flow_code`, and by `horch context` for a record
/// without a recorded decision.
pub fn window_decision(
    ctx: &RuntimeContext,
    teammate: &Teammate,
    model: &str,
    workdir: &Path,
    fleet: Option<(u64, String)>,
) -> WindowDecision {
    let home = &ctx.paths.home;
    let codex_home = super::codex::codex_home(home, ctx.inherited.codex_home.as_deref());
    let prime_agent_dir = super::prime::source_agent_dir(ctx, teammate);
    let inputs = WindowInputs {
        home,
        claude_config_dir: ctx.inherited.claude_config_dir.as_deref(),
        claude_managed_settings: &ctx.paths.claude_managed_settings,
        codex_home: &codex_home,
        prime_agent_dir: &prime_agent_dir,
        workdir,
        process_window: ctx.inherited.claude_code_auto_compact_window.as_deref(),
        teammate,
        model,
    };
    let mut decision = window::decide(teammate.agent.adapter().operator_window(&inputs), fleet);
    if decision.source == WindowSource::Fleet && settings_file(teammate) {
        decision.applied = false;
        decision.detail = "not applied: teammate settings file".into();
    }
    decision
}

/// Whether a Claude teammate names a settings file (not inline JSON).
fn settings_file(teammate: &Teammate) -> bool {
    teammate.agent == HarnessKind::Claude
        && teammate
            .settings
            .as_deref()
            .is_some_and(|s| !s.trim_start().starts_with('{'))
}

/// The decision as the built command carries it (review finding 12): an
/// applied decision stays applied only when the command passes its value.
/// Otherwise the detail names the first reason that holds.
pub fn window_in_effect(
    mut decision: WindowDecision,
    teammate: &Teammate,
    cmd: &Command,
) -> WindowDecision {
    if !decision.applied || teammate.agent.adapter().window_in_command(cmd) == decision.tokens {
        return decision;
    }
    decision.applied = false;
    let reason = if settings_file(teammate) {
        "teammate settings file"
    } else if teammate.agent == HarnessKind::Claude && teammate.settings.is_some() {
        "teammate inline settings own the overlay"
    } else {
        "not in the command"
    };
    decision.detail = format!("not applied: {reason}");
    decision
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
    /// The fleet's native window for this teammate and model, with its
    /// source (`Roster::fleet_window`). None: no fleet default.
    pub fleet_window: Option<(u64, String)>,
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
    // The intended window decision, before `prepare` (Prime's reads it).
    let model = req
        .model_override
        .or(req.teammate.model.as_deref())
        .unwrap_or_default();
    let workdir = match &req.record {
        Some(target) => PathBuf::from(target.workdir),
        None => ctx.paths.cwd.clone().unwrap_or_default(),
    };
    let decision = window_decision(ctx, req.teammate, model, &workdir, req.fleet_window.clone());
    let prepared = adapter.prepare(
        ctx,
        &PrepareRequest {
            role: req.role,
            exec_rules: req.exec_rules,
            // An empty-plan bundle has no directory for the adapter to link.
            skills: skills.as_ref().filter(|b| !b.plan().activated.is_empty()),
            compact_window: Some(&decision),
            teammate: req.teammate,
            model,
            workdir: &workdir,
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
    let mut env = LaunchEnv::from_context(ctx);
    env.compact_window = Some(decision.clone());
    let mut cmd = agent_command_in(
        &env,
        &teammate,
        session,
        req.prompt,
        skills.as_ref(),
        req.child_env,
    )?;
    // Last, so a value the harness prepared (codex's private home) wins.
    // Before the read-back: Prime's window is in the agent dir its
    // `prepare` names in the environment.
    for (key, value) in &prepared.env {
        cmd.env(key, value);
    }
    // What the command really carries is what the record says.
    let decision = window_in_effect(decision, &teammate, &cmd);
    if let Some(target) = &req.record {
        let recorded = crate::execution::store::ExecutionStore::open_in(ctx).and_then(|store| {
            store.set_compact_window(target.record_id, &LedgerWindow::from(&decision))
        });
        if let Err(e) = recorded {
            eprintln!("horch[{}]: window decision not recorded: {e:#}", req.role);
        }
    }
    crate::runtime::process::scrub_child_env(&mut cmd);
    let name = format!("{:?}", cmd.get_program());

    // A resume on a CLI that drops its command-line prompt gets the prompt
    // typed in, in the background, once the agent is idle.
    if req.session.is_resume() && adapter.resume_prompt_typed() {
        start_typed_prompt(ctx, req.role, req.prompt, req.record.as_ref());
    }

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
    let home = ctx.inherited.home_var.as_deref().map(Path::new);
    let bundle = crate::skills::Bundle::install_from(
        &ctx.paths.state_root,
        req.teammate,
        catalog,
        &name,
        home,
    )?;
    // The worker must run the skills the ledger recorded at spawn (SKL-04).
    // The catalog, the operator directory and the `plugin_skills` plugins
    // are read again here, so a skill that changed, vanished or appeared
    // since the spawn fails the launch. Each copy (the bundle's, and the
    // filtered plugin copy) is digest-checked against this plan.
    if let Some(target) = &req.record {
        let record = crate::execution::store::ExecutionStore::open_in(ctx)?
            .get(target.record_id)
            .with_context(|| format!("reading the skills recorded for {}", target.record_id))?;
        let launched = bundle
            .as_ref()
            .map(|b| b.plan().activated.as_slice())
            .unwrap_or_default();
        if let Some(why) = skill_drift(&record.skills, launched) {
            bail!(
                "the skills changed since {} was spawned: {why}. \
                 Spawn the worker again to record the current skills",
                target.record_id
            );
        }
    }
    Ok(bundle)
}

/// How the skills this launch resolved differ from the ones the ledger
/// recorded at spawn, by id, version and digest; `None` when they match.
/// The invocation policy and the source label do not count: the bytes do.
fn skill_drift(
    recorded: &[crate::skills::activation::ResolvedSkillRef],
    launched: &[crate::skills::activation::ResolvedSkillRef],
) -> Option<String> {
    let key = |r: &crate::skills::activation::ResolvedSkillRef| {
        (r.id.as_str().to_owned(), (r.version.0.clone(), r.digest))
    };
    let recorded: BTreeMap<_, _> = recorded.iter().map(key).collect();
    let launched: BTreeMap<_, _> = launched.iter().map(key).collect();
    let mut problems = Vec::new();
    for (id, (version, digest)) in &recorded {
        match launched.get(id) {
            None => problems.push(format!("'{id}' ({version}) is missing now")),
            Some((v, d)) if (v, d) != (version, digest) => problems.push(format!(
                "'{id}' was {version} (digest {}) and is {v} (digest {}) now",
                digest.short12(),
                d.short12()
            )),
            Some(_) => {}
        }
    }
    for (id, (version, _)) in &launched {
        if !recorded.contains_key(id) {
            problems.push(format!("'{id}' ({version}) was not recorded"));
        }
    }
    (!problems.is_empty()).then(|| problems.join("; "))
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

/// [`agent_command_in`] in the context's launch environment, with no
/// window decision.
#[cfg(test)]
pub(crate) fn agent_command(
    ctx: &RuntimeContext,
    teammate: &Teammate,
    session: Session<'_>,
    prompt: &str,
    skills: Option<&crate::skills::Bundle>,
    child_env: Vec<(String, String)>,
) -> Result<Command> {
    agent_command_in(
        &LaunchEnv::from_context(ctx),
        teammate,
        session,
        prompt,
        skills,
        child_env,
    )
}

/// The agent's command: the teammate's launch line, its own environment, then
/// `child_env`. Nothing here touches this process's environment, and
/// `FORBIDDEN_ENV` and the git repository variables stay removed.
fn agent_command_in(
    env: &LaunchEnv,
    teammate: &Teammate,
    session: Session<'_>,
    prompt: &str,
    skills: Option<&crate::skills::Bundle>,
    child_env: Vec<(String, String)>,
) -> Result<Command> {
    let mut cmd = command_with_skills_in(env, teammate, session, prompt, None, skills)?;
    crate::runtime::process::inherit_env(&mut cmd, teammate_env(teammate, env));
    crate::runtime::process::inherit_env(&mut cmd, child_env);
    crate::runtime::process::scrub_child_env(&mut cmd);
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
/// Type a resumed session's prompt into this pane from a background
/// thread ([`type_resumed_prompt`]). Without a pane id nothing can be typed,
/// and the record says so at once.
fn start_typed_prompt(
    ctx: &RuntimeContext,
    role: &str,
    prompt: &str,
    target: Option<&DiscoveryTarget<'_>>,
) {
    let ledger = target.and_then(|t| {
        Ledger::open_in(ctx)
            .ok()
            .map(|l| (l, t.record_id.to_string()))
    });
    let orchestrator = target.and_then(|t| t.mailbox.pane_for("orchestrator"));
    let herdr = Herdr::with_bin(&ctx.bins.harness.herdr);
    let role = role.to_string();
    let prompt = prompt.to_string();
    let pane = ctx.herdr.pane.as_ref().map(|p| p.to_string());
    std::thread::spawn(move || {
        let ledger = ledger.as_ref().map(|(l, id)| (l, id.as_str()));
        type_resumed_prompt(
            &herdr,
            pane.as_deref(),
            &prompt,
            &role,
            ledger,
            orchestrator.as_deref(),
            &Readiness::DEFAULT,
            &Timing::DEFAULT,
        );
    });
}

/// Type `prompt` into `pane` once its agent is idle. When that fails, the
/// worker's record gets a note that says why, and the orchestrator pane, if
/// one is registered, gets a `BLOCKED` line: a resumed pane never sits idle
/// with no task and no word. True when the prompt was typed.
#[allow(clippy::too_many_arguments)]
pub(crate) fn type_resumed_prompt(
    ws: &dyn WorkspaceClient,
    pane: Option<&str>,
    prompt: &str,
    role: &str,
    ledger: Option<(&Ledger, &str)>,
    orchestrator: Option<&str>,
    wait: &Readiness,
    timing: &Timing,
) -> bool {
    let result = match pane {
        Some(pane) => delivery::deliver_when_idle(ws, pane, prompt, wait, timing),
        None => Err(anyhow::anyhow!(
            "HERDR_PANE_ID is not set, so the resumed task cannot be typed"
        )),
    };
    let Err(e) = result else {
        return true;
    };
    let why = format!("resumed task not delivered: {e:#}");
    eprintln!("horch worker[{role}]: {why}");
    if let Some((ledger, record_id)) = ledger {
        if let Err(e) = ledger.note(record_id, &why) {
            eprintln!("horch worker[{role}]: recording the failure failed: {e:#}");
        }
    }
    if let Some(orchestrator) = orchestrator {
        let line = format!(
            "[{role}] BLOCKED: My resumed task did not reach my pane. Reason: {e:#}. \
             Send it again with horch assign {role} \"<task>\"."
        );
        if let Err(e) = delivery::send_line_with(ws, orchestrator, &line, timing) {
            eprintln!("horch worker[{role}]: telling the orchestrator failed: {e:#}");
        }
    }
    false
}

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

    fn skill_ref(
        id: &str,
        version: &str,
        digest: char,
    ) -> crate::skills::activation::ResolvedSkillRef {
        crate::skills::activation::ResolvedSkillRef {
            id: crate::ids::SkillId::new(id).unwrap(),
            version: horch_marketplace::SkillVersion(version.into()),
            digest: format!("sha256:{}", digest.to_string().repeat(64))
                .parse()
                .unwrap(),
            source: "bundled".into(),
            policy: crate::skills::activation::InvocationPolicy::Explicit,
        }
    }

    /// The launch compares id, version and digest with the ledger record.
    #[test]
    fn skill_drift_names_missing_changed_and_unrecorded_skills() {
        let tdd = skill_ref("tdd", "bundled+aaaa", 'a');
        let op = skill_ref("test-modernizer", "operator+bbbb", 'b');
        assert_eq!(
            skill_drift(&[tdd.clone(), op.clone()], &[op.clone(), tdd.clone()]),
            None
        );
        assert_eq!(skill_drift(&[], &[]), None);
        // The policy and the source do not count.
        let offered = crate::skills::activation::ResolvedSkillRef {
            policy: crate::skills::activation::InvocationPolicy::Offered,
            source: "elsewhere".into(),
            ..tdd.clone()
        };
        assert_eq!(skill_drift(std::slice::from_ref(&tdd), &[offered]), None);

        assert_eq!(
            skill_drift(&[tdd.clone(), op.clone()], std::slice::from_ref(&tdd)).unwrap(),
            "'test-modernizer' (operator+bbbb) is missing now"
        );
        let edited = skill_ref("test-modernizer", "operator+cccc", 'c');
        assert_eq!(
            skill_drift(std::slice::from_ref(&op), &[edited]).unwrap(),
            "'test-modernizer' was operator+bbbb (digest bbbbbbbbbbbb) and is \
             operator+cccc (digest cccccccccccc) now"
        );
        // Same version label, other bytes: still a change.
        let same_label = skill_ref("test-modernizer", "operator+bbbb", 'd');
        assert!(skill_drift(std::slice::from_ref(&op), &[same_label]).is_some());
        assert_eq!(
            skill_drift(std::slice::from_ref(&tdd), &[tdd.clone(), op]).unwrap(),
            "'test-modernizer' (operator+bbbb) was not recorded"
        );
    }
    use crate::harness::HarnessKind;
    use crate::roster::Roster;

    fn argv(cmd: &Command) -> Vec<String> {
        cmd.get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    }

    /// `~/` in an `env` value expands against the launch's home: the CLI gets
    /// no shell, and asc refuses a relative `ASC_CONFIG_PATH`. Only a leading
    /// `~/` expands; with no home the value stays as written.
    #[test]
    fn teammate_env_expands_a_leading_tilde_against_home() {
        let mut t = Teammate::default();
        t.env.insert("A".into(), "~/.config/x.json".into());
        t.env.insert("B".into(), "a~/b".into());
        t.env.insert("C".into(), "~".into());
        let home = LaunchEnv {
            home: Some(PathBuf::from("/home/op")),
            ..LaunchEnv::for_test()
        };
        assert_eq!(
            teammate_env(&t, &home),
            vec![
                ("A".to_string(), "/home/op/.config/x.json".to_string()),
                ("B".to_string(), "a~/b".to_string()),
                ("C".to_string(), "~".to_string()),
            ]
        );
        assert_eq!(
            teammate_env(&t, &LaunchEnv::for_test())[0].1,
            "~/.config/x.json"
        );

        let r = Roster::builtin().unwrap();
        let env = teammate_env(r.require("app-release-preparer").unwrap(), &home);
        let config = env.iter().find(|(k, _)| k == "ASC_CONFIG_PATH").unwrap();
        assert_eq!(config.1, "/home/op/.config/horch/asc/config.json");
    }

    /// The blender-artist's `BLENDER_PATH: "{path_of:blender}"` becomes the
    /// Blender the doctor's lookup finds: the operator's `BLENDER_PATH`, else
    /// `blender` on PATH, else the app. The Blender MCP server inherits it.
    /// Not found, the variable is left out and the launch still builds. The
    /// app is a temp file, so no test sees the machine's real Blender.
    #[cfg(unix)]
    #[test]
    fn path_of_blender_fills_blender_path_or_leaves_it_out() {
        let tmp = tempfile::tempdir().unwrap();
        let bin_dir = tmp.path().join("bin");
        std::fs::create_dir_all(&bin_dir).unwrap();
        let on_path = bin_dir.join("blender");
        std::fs::write(&on_path, "#!/bin/sh\n").unwrap();
        crate::runtime::process::make_executable(&on_path).unwrap();
        let app = tmp.path().join("Blender");
        std::fs::write(&app, "#!/bin/sh\n").unwrap();
        let none = std::env::join_paths([tmp.path().join("none")]).unwrap();

        let r = Roster::builtin().unwrap();
        let artist = r.require("blender-artist").unwrap();
        let blender_path = |env: &LaunchEnv| {
            teammate_env(artist, env)
                .into_iter()
                .find(|(k, _)| k == "BLENDER_PATH")
                .map(|(_, v)| v)
        };
        let lookup = |var: Option<&str>, path, app: Option<&Path>| LaunchEnv {
            blender_path: var.map(Into::into),
            path: Some(path),
            blender_app: app.map(Path::to_path_buf),
            ..LaunchEnv::for_test()
        };
        let path = std::env::join_paths([&bin_dir]).unwrap();

        // The operator's BLENDER_PATH wins.
        let env = lookup(Some("/operator/Blender"), path.clone(), Some(&app));
        assert_eq!(blender_path(&env).as_deref(), Some("/operator/Blender"));
        // Then `blender` on PATH.
        let env = lookup(None, path, Some(&app));
        assert_eq!(blender_path(&env), Some(on_path.to_string_lossy().into()));
        // Then the app.
        let env = lookup(None, none.clone(), Some(&app));
        assert_eq!(blender_path(&env), Some(app.to_string_lossy().into()));
        // Not found: no BLENDER_PATH, and the launch command still builds.
        let env = lookup(None, none, None);
        assert_eq!(blender_path(&env), None);
        command_in(&env, artist, Session::Fresh("s"), "p", None).unwrap();
    }

    /// Every `{path_of:<tool>}` in a built-in teammate's `env` names a tool
    /// `LaunchEnv::host_tool` can look up: a typo would leave the variable
    /// out of every launch with no error.
    #[test]
    fn every_path_of_names_a_known_tool() {
        let r = Roster::builtin().unwrap();
        let mut seen = 0;
        for name in r.names() {
            for value in r.require(name).unwrap().env.values() {
                if let Some(tool) = path_of(value) {
                    assert!(["blender"].contains(&tool), "{name}: {value}");
                    seen += 1;
                }
            }
        }
        assert!(seen > 0, "no teammate uses {{path_of:...}}");
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
                // U-64: an opencode resume passes no `--prompt`; the launch
                // types the prompt in instead (`resume_prompt_typed`).
                if t.agent == HarnessKind::OpenCode && matches!(session, Session::Resume(_)) {
                    assert!(!args.iter().any(|a| a == "--prompt"), "{name}: {args:?}");
                } else {
                    assert!(
                        args.last().unwrap().ends_with("BRIEFING"),
                        "{name}: {args:?}"
                    );
                    assert!(args.last().unwrap().contains("Fleet skill phase: research"));
                }
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
        // A sandboxed teammate launches only where the host can sandbox it:
        // on Linux, `bwrap` and `socat` on PATH. Fake ones stand in.
        let tmp = tempfile::tempdir().unwrap();
        let env = LaunchEnv {
            path: Some(super::super::claude::sandbox_tools_path(tmp.path(), true)),
            ..LaunchEnv::for_test()
        };
        for name in r.names() {
            let t = r.require(name).unwrap();
            if t.agent == HarnessKind::None {
                continue;
            }
            let cmd = command_in(&env, t, Session::Unmanaged, "p", Some("m")).unwrap();
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
                // the subagent tool.
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
    /// pane loads a filtered copy from the skills bundle (the named skill,
    /// the plugin's hooks and scripts, no other skill and no command), the
    /// installed original goes off and the copy goes on. A launch without a
    /// bundle fails, and a `--check` catches a name the plugin does not ship.
    #[test]
    fn plugin_skills_load_a_filtered_copy_of_the_plugin() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let fixture = |root: &std::path::Path, name: &str| {
            for skill in ["review", "lint", "format"] {
                let dir = root.join("skills").join(skill);
                std::fs::create_dir_all(dir.join("scripts")).unwrap();
                std::fs::write(
                    dir.join("SKILL.md"),
                    format!("---\nname: {skill}\ndescription: Does {skill}.\n---\n"),
                )
                .unwrap();
                std::fs::write(dir.join("scripts/run.sh"), "echo hi\n").unwrap();
            }
            for (rel, text) in [
                (
                    ".claude-plugin/plugin.json",
                    format!(
                        r#"{{"name":"{name}","version":"1.0.0","skills":["./extra/"],"commands":"./commands/"}}"#
                    ),
                ),
                ("commands/deploy.md", "Deploy.".into()),
                ("hooks/hooks.json", r#"{"hooks":{}}"#.into()),
                ("scripts/shared.sh", "echo shared\n".into()),
                (".in_use/123", "pid".into()),
            ] {
                let path = root.join(rel);
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(path, text).unwrap();
            }
        };
        let check_copy = |copy: &std::path::Path, name: &str| {
            let skills: Vec<String> = std::fs::read_dir(copy.join("skills"))
                .unwrap()
                .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .collect();
            assert_eq!(skills, ["review"], "{}", copy.display());
            assert!(copy.join("skills/review/scripts/run.sh").is_file());
            assert!(copy.join("hooks/hooks.json").is_file());
            assert!(copy.join("scripts/shared.sh").is_file());
            assert!(!copy.join("commands").exists());
            assert!(!copy.join(".in_use").exists());
            let manifest: serde_json::Value = serde_json::from_str(
                &std::fs::read_to_string(copy.join(".claude-plugin/plugin.json")).unwrap(),
            )
            .unwrap();
            assert_eq!(
                manifest,
                serde_json::json!({"name": name, "version": "1.0.0"})
            );
        };
        let install = |t: &Teammate, env: &LaunchEnv| {
            crate::skills::Bundle::install_from(
                &tmp.path().join("state"),
                t,
                crate::skills::SkillCatalog::bundled().unwrap(),
                &crate::mint_uuid(),
                env.home(),
            )
            .unwrap()
            .unwrap()
        };
        let launch = |t: &Teammate, env: &LaunchEnv| {
            let bundle = install(t, env);
            let a = argv(
                &command_with_skills_in(env, t, Session::Unmanaged, "p", None, Some(&bundle))
                    .unwrap(),
            );
            (bundle, a)
        };
        let overlay_of = |a: &[String]| -> serde_json::Value {
            let at = a.iter().position(|x| x == "--settings").unwrap();
            serde_json::from_str(&a[at + 1]).unwrap()
        };
        let r = Roster::builtin().unwrap();

        // A plugin from plugin_dirs: its directory is swapped for the copy.
        let root = tmp.path().join("code");
        fixture(&root, "code");
        let mut t = r.require("opus").unwrap().clone();
        t.plugin_dirs = vec![root.to_string_lossy().into_owned()];
        t.plugin_skills.insert("code".into(), vec!["review".into()]);
        let (bundle, a) = launch(&t, &LaunchEnv::for_test());
        let copy = bundle.root().join("plugins/code");
        let dirs: Vec<&str> = a
            .windows(2)
            .filter(|w| w[0] == "--plugin-dir")
            .map(|w| w[1].as_str())
            .collect();
        assert_eq!(
            dirs,
            [copy.to_str().unwrap(), bundle.root().to_str().unwrap()],
            "{a:?}"
        );
        check_copy(&copy, "code");
        assert_eq!(overlay_of(&a)["enabledPlugins"]["code@inline"], true);
        // The plan (and so the ledger record) pins the named skill only.
        let review = bundle
            .plan()
            .activated
            .iter()
            .find(|r| r.id.as_str() == "code:review")
            .unwrap_or_else(|| panic!("{:?}", bundle.plan().activated));
        assert_eq!(review.source, "plugin:code@inline");
        assert!(review.version.0.starts_with("1.0.0+"), "{review:?}");
        assert_eq!(
            review.digest.to_string(),
            horch_marketplace::integrity::tree_digest(&root.join("skills/review")).unwrap()
        );
        assert!(!bundle.plan().activated_ids().contains(&"code:lint"));
        // The briefing names the reinforced one with its description.
        let brief = bundle.briefing_in(&t, None);
        assert!(brief.contains("- code:review: Does review."), "{brief}");
        assert!(!brief.contains("code:lint"), "{brief}");
        drop(bundle);
        assert!(!copy.exists(), "the copy goes with the bundle");

        // A skill edited after the plan: the copy differs from the pinned
        // digest, and the launch fails.
        let bundle = install(&t, &LaunchEnv::for_test());
        std::fs::write(root.join("skills/review/scripts/run.sh"), "echo edited\n").unwrap();
        let err = command_with_skills_in(
            &LaunchEnv::for_test(),
            &t,
            Session::Unmanaged,
            "p",
            None,
            Some(&bundle),
        )
        .unwrap_err();
        let err = format!("{err:#}");
        assert!(
            err.contains("plugin skill 'code:review'") && err.contains("the plan pins"),
            "{err}"
        );
        drop(bundle);

        // An installed plugin: the copy is appended, every install goes off.
        let installed = tmp.path().join("cache/dev");
        fixture(&installed, "dev");
        std::fs::create_dir_all(home.join(".claude/plugins")).unwrap();
        std::fs::write(
            home.join(".claude/plugins/installed_plugins.json"),
            serde_json::json!({"version": 2, "plugins": {
                "dev@market": [{"installPath": installed}],
                "dev@fork": [{"installPath": installed}]
            }})
            .to_string(),
        )
        .unwrap();
        let env = LaunchEnv {
            home: Some(home.clone()),
            ..LaunchEnv::for_test()
        };
        let mut t = r.require("opus").unwrap().clone();
        t.plugin_skills.insert("dev".into(), vec!["review".into()]);
        let (bundle, a) = launch(&t, &env);
        let copy = bundle.root().join("plugins/dev");
        assert!(
            a.windows(2)
                .any(|w| w[0] == "--plugin-dir" && w[1] == copy.to_str().unwrap()),
            "{a:?}"
        );
        assert!(!a.iter().any(|x| x == installed.to_str().unwrap()), "{a:?}");
        check_copy(&copy, "dev");
        let enabled = &overlay_of(&a)["enabledPlugins"];
        assert_eq!(enabled["dev@market"], false, "{enabled}");
        assert_eq!(enabled["dev@fork"], false, "{enabled}");
        assert_eq!(enabled["dev@inline"], true, "{enabled}");
        drop(bundle);

        // No bundle: nowhere for the copy, so the launch fails.
        let err = command_in(&env, &t, Session::Unmanaged, "p", None)
            .unwrap_err()
            .to_string();
        assert!(err.contains("has no bundle"), "{err}");

        // A skill the plugin does not ship fails the launch and the check.
        t.plugin_skills.insert("dev".into(), vec!["deploy".into()]);
        let err = command_in(&env, &t, Session::Unmanaged, "p", None)
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

        // No effort: the builder sets only the harness default `config`.
        t.effort = None;
        t.env.clear();
        let cmd = command_in(&LaunchEnv::for_test(), &t, Session::Unmanaged, "p", None).unwrap();
        assert_eq!(
            overlay(&cmd),
            serde_json::json!({"agent": {"title": {"disable": true}}})
        );
        // No effort and no defaults: no overlay from the builder.
        t.harness_defaults.clear();
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

    /// LA-3: opencode 1.18.34 ignores `--prompt` beside `--session`, so a
    /// resume gets its prompt typed in. The resume argv drops the flag
    /// (U-64), so the typed prompt is the 1 delivery path.
    #[test]
    fn only_an_opencode_resume_types_its_prompt() {
        assert!(HarnessKind::OpenCode.adapter().resume_prompt_typed());
        for kind in [
            HarnessKind::Claude,
            HarnessKind::Codex,
            HarnessKind::Pi,
            HarnessKind::Prime,
        ] {
            assert!(!kind.adapter().resume_prompt_typed(), "{kind:?}");
        }
    }

    /// A resumed prompt that never reaches an idle agent is not silent: the
    /// record gets a note that says why, and the orchestrator a BLOCKED line.
    #[test]
    fn an_undelivered_resume_prompt_is_recorded_and_reported() {
        use crate::messaging::delivery::{Readiness, Timing};
        use crate::workspace::testing::FakeWorkspace;
        let fast = Timing {
            tail_timeout: Duration::from_millis(5),
            poll_start: Duration::from_millis(1),
            poll_max: Duration::from_millis(1),
            settle_short: Duration::from_millis(1),
            settle_long: Duration::from_millis(1),
            second_enter: Duration::from_millis(1),
        };
        let wait = Readiness {
            timeout: Duration::from_millis(20),
            poll: Duration::from_millis(1),
        };
        let tmp = tempfile::tempdir().unwrap();
        let ledger = Ledger::for_project(tmp.path(), "/proj");
        ledger
            .add(
                "rec-1",
                "opencode",
                "opencode-pickle",
                "m",
                "pickle-1",
                Some("ses_1"),
                "t",
            )
            .unwrap();
        let ws = FakeWorkspace::new();
        let created = ws.workspace_create("fleet", None, false).unwrap();
        let worker = created.root_pane_id;
        let orchestrator = ws
            .pane_split(&worker, crate::workspace::model::Direction::Right)
            .unwrap();
        ws.set_agent_states(&worker, &[(Some("opencode"), Some("unknown"))]);

        let typed = type_resumed_prompt(
            &ws,
            Some(&worker),
            "TASK",
            "pickle-1",
            Some((&ledger, "rec-1")),
            Some(&orchestrator),
            &wait,
            &fast,
        );
        assert!(!typed);
        let history = ledger.get("rec-1").unwrap().history;
        let note = history.iter().find(|h| h.event == "note").unwrap();
        assert!(note.text.contains("resumed task not delivered"), "{note:?}");
        assert!(note.text.contains("was not idle"), "{note:?}");
        let prompts: Vec<_> = ws
            .calls()
            .into_iter()
            .filter(|c| c.method == "agent_prompt")
            .collect();
        assert_eq!(prompts.len(), 1, "{prompts:?}");
        assert_eq!(prompts[0].args[0], orchestrator);
        assert!(prompts[0].args[1].starts_with("[pickle-1] BLOCKED: "));

        // An idle agent gets the prompt and nothing is recorded.
        let ws = FakeWorkspace::new();
        let worker = ws
            .workspace_create("fleet", None, false)
            .unwrap()
            .root_pane_id;
        ws.set_agent_states(&worker, &[(Some("opencode"), Some("idle"))]);
        assert!(type_resumed_prompt(
            &ws,
            Some(&worker),
            "TASK",
            "pickle-1",
            None,
            None,
            &wait,
            &fast
        ));
        assert!(ws
            .calls()
            .iter()
            .any(|c| c.method == "agent_prompt" && c.args == [worker.clone(), "TASK".to_string()]));
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
                "enabledPlugins": {"herdr@m": false, "skill-creator@claude-plugins-official": false},
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
                "enabledPlugins": {"herdr@m": false, "skill-creator@claude-plugins-official": false},
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

    /// U-14: a horch started under `git rebase -x` or a git hook has
    /// `GIT_DIR` and its relatives set. An agent it launches must not see
    /// them, or the agent's `git commit` writes into that other repository.
    #[cfg(unix)]
    #[test]
    fn an_agent_launch_drops_the_git_repository_variables() {
        let out = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "harness::launch::tests::git_env_agent_launch_child",
                "--ignored",
                "--test-threads=1",
            ])
            .env("GIT_DIR", "/nonexistent/decoy/.git")
            .env("GIT_WORK_TREE", "/nonexistent/decoy")
            .env("GIT_INDEX_FILE", "/nonexistent/decoy/.git/index")
            .env("GIT_CONFIG_COUNT", "1")
            .env("GIT_CONFIG_KEY_0", "user.name")
            .env("GIT_CONFIG_VALUE_0", "decoy")
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(
            out.status.success() && stdout.contains("1 passed"),
            "{stdout}\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    #[cfg(unix)]
    #[test]
    #[ignore = "run by an_agent_launch_drops_the_git_repository_variables, with GIT_DIR set"]
    fn git_env_agent_launch_child() {
        let tmp = tempfile::tempdir().unwrap();
        let seen = tmp.path().join("env.txt");
        let fake = tmp.path().join("claude");
        std::fs::write(&fake, format!("#!/bin/sh\nenv > '{}'\n", seen.display())).unwrap();
        crate::runtime::process::make_executable(&fake).unwrap();
        let env = LaunchEnv {
            bins: HarnessBins::resolve(
                &BinOverrides {
                    claude: Some(fake),
                    ..BinOverrides::default()
                },
                None,
                None,
            ),
            ..LaunchEnv::for_test()
        };
        let r = Roster::builtin().unwrap();
        let mut cmd = command_in(
            &env,
            r.require("opus").unwrap(),
            Session::Unmanaged,
            "p",
            None,
        )
        .unwrap();
        let status = cmd
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .status()
            .unwrap();
        assert!(status.success());
        let seen = std::fs::read_to_string(&seen).unwrap();
        // Only the names: the rest of the environment can hold secrets.
        let leaked: Vec<&str> = [
            "GIT_DIR",
            "GIT_WORK_TREE",
            "GIT_INDEX_FILE",
            "GIT_CONFIG_KEY_0",
        ]
        .into_iter()
        .filter(|key| seen.lines().any(|l| l.starts_with(&format!("{key}="))))
        .collect();
        assert!(leaked.is_empty(), "the agent sees {leaked:?}");
    }

    /// U-53: a teammate whose only skills are operator skills, all missing on
    /// this host, has an empty plan and so no skill files. Its briefing still
    /// carries the `Skipped:` note, and the harness exposes nothing.
    #[test]
    fn operator_only_teammate_with_every_skill_missing_still_gets_the_skipped_note() {
        let tmp = tempfile::tempdir().unwrap();
        let mut t = Roster::builtin().unwrap().require("opus").unwrap().clone();
        t.phase = None;
        t.skills.clear();
        t.plugin_skills.clear();
        t.operator_skills = Some(crate::roster::OperatorSkills {
            dir: tmp.path().join("exported").to_string_lossy().into_owned(),
            names: vec!["swiftui-whats-new-27".into()],
        });
        let bundle = crate::skills::Bundle::install_from(
            &tmp.path().join("state"),
            &t,
            crate::skills::SkillCatalog::bundled().unwrap(),
            &crate::mint_uuid(),
            None,
        )
        .unwrap()
        .expect("the skipped note needs a bundle");
        assert!(bundle.plan().activated.is_empty());
        assert!(!bundle.root().exists(), "an empty plan writes no files");
        let cmd = command_with_skills_in(
            &LaunchEnv::for_test(),
            &t,
            Session::Unmanaged,
            "the task",
            None,
            Some(&bundle),
        )
        .unwrap();
        let a = argv(&cmd);
        let prompt = a.last().unwrap();
        assert!(
            prompt.contains("Skipped: operator skill swiftui-whats-new-27 is not installed"),
            "{prompt}"
        );
        assert!(prompt.ends_with("the task"), "{prompt}");
        assert!(!prompt.contains("Available native skills"), "{prompt}");
        assert!(!a.iter().any(|x| x == "--plugin-dir"), "{a:?}");

        // No operator skill at all: no bundle, as before.
        t.operator_skills = None;
        let none = crate::skills::Bundle::install_from(
            &tmp.path().join("state"),
            &t,
            crate::skills::SkillCatalog::bundled().unwrap(),
            &crate::mint_uuid(),
            None,
        )
        .unwrap();
        assert!(none.is_none());
    }
}
