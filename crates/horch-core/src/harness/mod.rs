//! The agent CLIs a worker can run, and what each one can do.
//!
//! [`HarnessKind`] is the enum `teammates::Agent` used to be;
//! `teammates::Agent` stays as an alias so existing callers compile. What
//! each harness can do is [`Capabilities`] data, read through
//! [`HarnessKind::capabilities`].

pub mod capabilities;
pub mod claude;
pub mod claude_plugins;
pub mod codex;
pub mod launch;
pub mod none;
pub mod opencode;
pub mod pi;
pub mod prime;

use std::ffi::OsString;
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::str::FromStr;
use std::time::SystemTime;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::runtime::RuntimeContext;
use crate::teammates::{ExecRule, Teammate};

pub use capabilities::{Capabilities, SkillExposure};
pub use launch::{LaunchEnv, Session};

/// What one launch needs from its pane before the CLI starts.
pub struct PrepareRequest<'a> {
    /// The pane's role, which names whatever the launch leaves on disk.
    pub role: &'a str,
    /// The execpolicy rules this pane needs (codex).
    pub exec_rules: &'a [ExecRule],
    /// The skills bundle the launch exposes, when the teammate has one.
    pub skills: Option<&'a crate::skills::Bundle>,
}

/// What [`Harness::prepare`] set up for one launch. Hold it until the CLI
/// has exited, then call [`Prepared::finish`]; dropping it instead cleans up
/// what has to be cleaned up on an error path.
#[derive(Default)]
pub struct Prepared {
    /// Added to the teammate's `args`, so they land before the prompt and
    /// before any `--` delimiter.
    pub extra_args: Vec<String>,
    /// Set on the child after every other environment layer, so these win.
    pub env: Vec<(String, OsString)>,
    /// A directory only this launch records sessions in (Prime Agent).
    pub sessions_dir: Option<PathBuf>,
    finish: Vec<Box<dyn FnOnce()>>,
}

impl Prepared {
    /// Run `step` after the CLI has exited, in the order the steps were added.
    pub fn on_finish(&mut self, step: impl FnOnce() + 'static) {
        self.finish.push(Box::new(step));
    }

    /// Clean up after the CLI has exited.
    pub fn finish(self) {
        for step in self.finish {
            step();
        }
    }
}

/// The parts of a launch the command line is built from.
#[derive(Debug, Clone, Copy)]
pub struct CommandSpec<'a> {
    pub teammate: &'a Teammate,
    pub session: Session<'a>,
    pub prompt: &'a str,
    /// For the fixed `orchestration` recipe, where the model belongs to the
    /// pane rather than to the teammate file.
    pub model_override: Option<&'a str>,
}

/// Everything harness-specific about launching one CLI. Callers dispatch
/// through [`HarnessKind::adapter`] and read [`Capabilities`]; they never
/// match on the kind.
pub trait Harness: Sync {
    fn kind(&self) -> HarnessKind;

    fn capabilities(&self) -> &'static Capabilities {
        self.kind().capabilities()
    }

    /// Claude-shaped teammate fields this harness has no way to express.
    ///
    /// Setting one of these is asking for isolation or capability that would
    /// silently not happen, so the roster check rejects it by name rather than
    /// launching a worker whose author believes it is constrained.
    fn validate(&self, t: &Teammate) -> Vec<&'static str> {
        generic_validate(self, t)
    }

    /// Whether `model` has any effort setting here. A harness whose models
    /// differ overrides this.
    fn model_takes_effort(&self, _model: &str) -> bool {
        !self.capabilities().effort.is_empty()
    }

    /// Set up what the launch needs on disk before the CLI starts: codex's
    /// private home and rules, Prime's daemon socket.
    fn prepare(&self, _ctx: &RuntimeContext, _req: &PrepareRequest<'_>) -> Result<Prepared> {
        Ok(Prepared::default())
    }

    /// The harness's own command line. Call [`Harness::build_command`]
    /// instead, which removes the forbidden environment.
    fn command(&self, env: &LaunchEnv, spec: &CommandSpec<'_>) -> Result<Command>;

    /// The command for `spec`, with every [`launch::FORBIDDEN_ENV`] key
    /// removed. Not overridden by any harness.
    fn build_command(&self, env: &LaunchEnv, spec: &CommandSpec<'_>) -> Result<Command> {
        let mut cmd = self.command(env, spec)?;
        crate::runtime::process::strip_forbidden(&mut cmd);
        Ok(cmd)
    }

    /// Session ids this CLI recorded at or after `since` for `workdir`,
    /// newest first. Paths compare canonically, so `/var/...` and
    /// `/private/var/...` are one directory. `sessions_dir` is
    /// [`Prepared::sessions_dir`].
    fn discover_sessions(
        &self,
        _ctx: &RuntimeContext,
        _workdir: &Path,
        _since: SystemTime,
        _sessions_dir: Option<&Path>,
    ) -> Vec<String> {
        Vec::new()
    }
}

/// The checks every harness shares, from its [`Capabilities`]. A harness
/// that adds a check calls this first.
pub fn generic_validate<H: Harness + ?Sized>(harness: &H, t: &Teammate) -> Vec<&'static str> {
    let caps = harness.capabilities();
    let mut out = Vec::new();
    if t.tools.is_some() && !caps.tool_lists {
        out.push("tools");
    }
    if !t.disallowed_tools.is_empty() && !caps.tool_denylist {
        out.push("disallowed_tools");
    }
    // Claude's "permit this without asking" list. pi has an allowlist and a
    // denylist but nothing that grants a tool permission, so there is
    // nowhere for this to go on any other agent.
    if !t.allowed_tools.is_empty() {
        out.push("allowed_tools");
    }
    // Claude-shaped configuration with no counterpart anywhere else.
    for (field, set) in [
        ("plugin_dirs", !t.plugin_dirs.is_empty()),
        ("settings", t.settings.is_some()),
        ("subagent_model", t.subagent_model.is_some()),
        ("setting_sources", t.setting_sources.is_some()),
        ("disable_skills", t.disable_skills),
        ("inherit_claudeai_skills", t.inherit_claudeai_skills),
        ("disabled_skills", !t.disabled_skills.is_empty()),
        ("plugin_skills", !t.plugin_skills.is_empty()),
        ("remote_control", t.remote_control),
        ("mcp_servers", t.mcp_servers.is_some()),
        ("mcp_config_files", !t.mcp_config_files.is_empty()),
    ] {
        if set {
            out.push(field);
        }
    }
    out.sort_unstable();
    out.dedup();
    out
}

/// A workdir as session discovery compares it: equal as written, or equal
/// after resolving symlinks. A herdr pane's cwd and the path an agent
/// records can differ by `/var` versus `/private/var` (or `/tmp` versus
/// `/private/tmp`) on macOS, and a string compare would silently find
/// nothing.
#[derive(Debug, Clone)]
pub struct Workdir {
    raw: PathBuf,
    canonical: Option<PathBuf>,
}

impl Workdir {
    pub fn new(path: impl Into<PathBuf>) -> Workdir {
        let raw = path.into();
        let canonical = std::fs::canonicalize(&raw).ok();
        Workdir { raw, canonical }
    }

    /// Whether `recorded`, a directory an agent wrote down, is this one.
    pub fn matches(&self, recorded: &Path) -> bool {
        if recorded == self.raw {
            return true;
        }
        match (&self.canonical, std::fs::canonicalize(recorded).ok()) {
            (Some(a), Some(b)) => *a == b,
            _ => false,
        }
    }
}

/// Which CLI a teammate launches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HarnessKind {
    Claude,
    Codex,
    #[serde(rename = "opencode")]
    OpenCode,
    Pi,
    Prime,
    /// No real agent: the smoke teammate exercises the machinery only.
    None,
}

impl HarnessKind {
    pub fn as_str(self) -> &'static str {
        match self {
            HarnessKind::Claude => "claude",
            HarnessKind::Codex => "codex",
            HarnessKind::OpenCode => "opencode",
            HarnessKind::Pi => "pi",
            HarnessKind::Prime => "prime",
            HarnessKind::None => "none",
        }
    }

    /// The module that owns this harness's behavior.
    pub fn adapter(self) -> &'static dyn Harness {
        match self {
            HarnessKind::Claude => &claude::Claude,
            HarnessKind::Codex => &codex::Codex,
            HarnessKind::OpenCode => &opencode::OpenCode,
            HarnessKind::Pi => &pi::Pi,
            HarnessKind::Prime => &prime::Prime,
            HarnessKind::None => &none::NoAgent,
        }
    }

    /// What this harness can do.
    pub fn capabilities(self) -> &'static Capabilities {
        match self {
            HarnessKind::Claude => &capabilities::CLAUDE,
            HarnessKind::Codex => &capabilities::CODEX,
            HarnessKind::OpenCode => &capabilities::OPENCODE,
            HarnessKind::Pi => &capabilities::PI,
            HarnessKind::Prime => &capabilities::PRIME,
            HarnessKind::None => &capabilities::NONE,
        }
    }

    /// Whether `model` has any effort setting on this harness. See
    /// [`crate::roster::model_takes_effort`].
    pub fn model_takes_effort(self, model: &str) -> bool {
        self.adapter().model_takes_effort(model)
    }

    /// Whether horch can choose this agent's session id before it launches.
    ///
    /// Claude takes `--session-id`, and pi takes `--session-id` with "create it
    /// if missing" semantics, so the ledger knows the resume handle before the
    /// pane even starts. Codex, OpenCode and Prime Agent mint their own and only
    /// reveal it afterwards - verified against `prime-agent --help` 0.9.4, which
    /// has `--session-dir` but no `--session-id`.
    ///
    /// Kept until A12; read [`Capabilities::caller_minted_session`].
    pub fn mints_session_id(self) -> bool {
        self.capabilities().caller_minted_session
    }

    /// Whether the session id has to be recovered after launch, by watching
    /// wherever this agent records its sessions.
    ///
    /// Kept until A12; read [`Capabilities::discovers_session`].
    pub fn harvests_session_id(self) -> bool {
        self.capabilities().discovers_session()
    }

    /// Whether this agent supervises its own sessions in a background service.
    ///
    /// Only Prime Agent does, and it is why a Prime pane gets its own daemon
    /// socket: herdr already treats a pane as an agent's lifetime, so an
    /// unscoped daemon would outlive `horch done` and accumulate one per spawn.
    ///
    /// Kept until A12; read [`Capabilities::daemon`].
    pub fn runs_a_daemon(self) -> bool {
        self.capabilities().daemon
    }

    /// Whether this agent's capability comes from an execpolicy allowlist rather
    /// than from flags. Only codex works that way, and it is why a codex pane
    /// gets a private `CODEX_HOME`.
    ///
    /// Kept until A12; read [`Capabilities::exec_policy`].
    pub fn uses_execpolicy(self) -> bool {
        self.capabilities().exec_policy
    }

    /// Whether this agent takes `tools` / `allowed_tools` / `disallowed_tools`.
    ///
    /// Claude has `--tools` and the two `--*allowedTools` lists; pi and Prime
    /// Agent have `--tools`, and pi alone adds `--exclude-tools`. Codex and
    /// OpenCode have neither.
    pub fn takes_tool_lists(self) -> bool {
        self.capabilities().tool_lists
    }

    /// Whether a denylist of tool names has anywhere to go. Prime Agent 0.9.4
    /// has `--tools` and `--no-tools` but no `--exclude-tools`.
    pub fn takes_tool_denylist(self) -> bool {
        self.capabilities().tool_denylist
    }

    /// Claude-shaped teammate fields this agent has no way to express. See
    /// [`Harness::validate`].
    pub fn unsupported_fields(self, t: &Teammate) -> Vec<&'static str> {
        self.adapter().validate(t)
    }
}

impl fmt::Display for HarnessKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for HarnessKind {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "claude" => Ok(HarnessKind::Claude),
            "codex" => Ok(HarnessKind::Codex),
            "opencode" => Ok(HarnessKind::OpenCode),
            "pi" => Ok(HarnessKind::Pi),
            "prime" => Ok(HarnessKind::Prime),
            "none" => Ok(HarnessKind::None),
            other => Err(format!(
                "unknown agent '{other}' (claude, codex, opencode, pi, prime, none)"
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::teammates::{Agent, Roster};

    const ALL: [HarnessKind; 6] = [
        HarnessKind::Claude,
        HarnessKind::Codex,
        HarnessKind::OpenCode,
        HarnessKind::Pi,
        HarnessKind::Prime,
        HarnessKind::None,
    ];

    /// The predicates and effort table as they were before A4, copied
    /// verbatim: (mints_session_id, harvests_session_id, runs_a_daemon,
    /// uses_execpolicy, takes_tool_lists, takes_tool_denylist, valid_efforts).
    fn legacy(kind: HarnessKind) -> (bool, bool, bool, bool, bool, bool, &'static [&'static str]) {
        use HarnessKind as A;
        (
            matches!(kind, A::Claude | A::Pi),
            matches!(kind, A::Codex | A::OpenCode | A::Prime),
            kind == A::Prime,
            kind == A::Codex,
            matches!(kind, A::Claude | A::Pi | A::Prime),
            matches!(kind, A::Claude | A::Pi),
            match kind {
                A::Claude => &["low", "medium", "high", "xhigh", "max"],
                A::Codex => &["none", "low", "medium", "high", "xhigh", "max"],
                A::OpenCode => &["none", "minimal", "low", "medium", "high", "xhigh", "max"],
                A::Pi | A::Prime => &["off", "minimal", "low", "medium", "high", "xhigh", "max"],
                A::None => &[],
            },
        )
    }

    #[test]
    fn arc_10_capabilities_match_legacy_predicates() {
        for kind in ALL {
            let c = kind.capabilities();
            let (mints, harvests, daemon, execpolicy, tools, denylist, efforts) = legacy(kind);
            assert_eq!(c.caller_minted_session, mints, "{kind}");
            assert_eq!(c.discovers_session(), harvests, "{kind}");
            assert_eq!(c.resumes, kind != HarnessKind::None, "{kind}");
            assert_eq!(c.daemon, daemon, "{kind}");
            assert_eq!(c.exec_policy, execpolicy, "{kind}");
            assert_eq!(c.tool_lists, tools, "{kind}");
            assert_eq!(c.tool_denylist, denylist, "{kind}");
            assert_eq!(c.effort, efforts, "{kind}");
            assert_eq!(c.headless, kind == HarnessKind::Claude, "{kind}");
            let exposure = match kind {
                HarnessKind::Claude => SkillExposure::PluginDir,
                HarnessKind::Codex => SkillExposure::CodexHome,
                HarnessKind::OpenCode => SkillExposure::ConfigPaths,
                HarnessKind::Pi | HarnessKind::Prime => SkillExposure::SkillFlag,
                HarnessKind::None => SkillExposure::None,
            };
            assert_eq!(c.skill_exposure, exposure, "{kind}");
            // The legacy methods now delegate; they still answer the same.
            assert_eq!(kind.mints_session_id(), mints, "{kind}");
            assert_eq!(kind.harvests_session_id(), harvests, "{kind}");
            assert_eq!(kind.runs_a_daemon(), daemon, "{kind}");
            assert_eq!(kind.uses_execpolicy(), execpolicy, "{kind}");
            assert_eq!(kind.takes_tool_lists(), tools, "{kind}");
            assert_eq!(kind.takes_tool_denylist(), denylist, "{kind}");
            assert_eq!(crate::roster::valid_efforts(kind), efforts, "{kind}");
            // The pre-A4 preflight footprint: 600 MiB, plus the local model on pi,
            // 64 MiB for none.
            let local = 7 << 30;
            let mib = 1024 * 1024;
            let want = match kind {
                HarnessKind::Pi => 600 * mib + local,
                HarnessKind::None => 64 * mib,
                _ => 600 * mib,
            };
            assert_eq!(c.footprint(local), want, "{kind}");
        }
    }

    #[test]
    fn arc_03_harness_kind_serde_compat() {
        for (kind, old) in [
            (HarnessKind::Claude, "claude"),
            (HarnessKind::Codex, "codex"),
            (HarnessKind::OpenCode, "opencode"),
            (HarnessKind::Pi, "pi"),
            (HarnessKind::Prime, "prime"),
            (HarnessKind::None, "none"),
        ] {
            assert_eq!(kind.as_str(), old);
            assert_eq!(kind.to_string(), old);
            assert_eq!(old.parse::<HarnessKind>().unwrap(), kind);

            let json = serde_json::to_string(&kind).unwrap();
            assert_eq!(json, format!("\"{old}\""));
            assert_eq!(serde_json::from_str::<HarnessKind>(&json).unwrap(), kind);

            let yaml = serde_yaml::to_string(&kind).unwrap();
            assert_eq!(yaml.trim(), old);
            assert_eq!(serde_yaml::from_str::<HarnessKind>(old).unwrap(), kind);
        }
        assert!(serde_json::from_str::<HarnessKind>(r#""open_code""#).is_err());

        // Teammate frontmatter keeps its spelling.
        let t = Roster::builtin().unwrap();
        let t = t.require("opencode-pickle").unwrap();
        assert_eq!(t.agent, HarnessKind::OpenCode);

        // The shim is an alias, not a second type.
        let alias: Agent = HarnessKind::OpenCode;
        let same: HarnessKind = alias;
        assert_eq!(same, Agent::OpenCode);
        assert_eq!(
            std::any::TypeId::of::<Agent>(),
            std::any::TypeId::of::<HarnessKind>()
        );
    }
}
