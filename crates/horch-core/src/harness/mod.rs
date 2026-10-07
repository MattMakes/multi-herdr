//! The agent CLIs a worker can run, and what each one can do.
//!
//! [`HarnessKind`] names the harness; it replaced `teammates::Agent`, and
//! no alias remains (`arc_25_no_shim_modules`). What each harness can do is
//! [`Capabilities`] data, read through [`HarnessKind::capabilities`].

pub mod antigravity;
pub mod capabilities;
pub(crate) mod claude;
pub(crate) mod claude_plugins;
pub mod codex;
pub mod headless;
pub mod inventory;
pub mod launch;
pub(crate) mod none;
pub mod opencode;
pub(crate) mod pi;
pub mod prime;
pub mod trust;

use std::ffi::OsString;
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::str::FromStr;
use std::time::SystemTime;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::compaction::window::{OperatorWindow, WindowDecision};
use crate::roster::{ExecRule, Teammate};
use crate::runtime::{HarnessBins, RuntimeContext};
use crate::skills::Bundle;

pub use capabilities::{Capabilities, SkillExposure};
pub use launch::{LaunchEnv, Session};

/// What one launch needs from its pane before the CLI starts.
pub struct PrepareRequest<'a> {
    /// The pane's role, which names whatever the launch leaves on disk.
    pub role: &'a str,
    /// The execpolicy rules this pane needs (codex).
    pub exec_rules: &'a [ExecRule],
    /// The skills bundle the launch exposes, when the teammate has one.
    pub skills: Option<&'a Bundle>,
    /// The launch's native-window decision (CTX-05). Prime's `prepare`
    /// reads it (slice 2).
    pub compact_window: Option<&'a WindowDecision>,
    /// The teammate the launch runs, as loaded (its harness defaults).
    pub teammate: &'a Teammate,
    /// The pane's model: the override, else the teammate's.
    pub model: &'a str,
    /// The directory the agent starts in.
    pub workdir: &'a Path,
}

/// What an operator window resolver reads for one launch (design §6.4).
pub struct WindowInputs<'a> {
    /// `RuntimeContext.paths.home`.
    pub home: &'a Path,
    /// `$CLAUDE_CONFIG_DIR`.
    pub claude_config_dir: Option<&'a Path>,
    /// `RuntimeContext.paths.claude_managed_settings` (review finding 5).
    pub claude_managed_settings: &'a Path,
    /// The inherited codex home (`codex::codex_home`).
    pub codex_home: &'a Path,
    /// The Prime agent dir the launch links from: the teammate's
    /// `PRIME_AGENT_CODING_AGENT_DIR`, else the inherited one, else
    /// `~/.prime/agent` (`prime::source_agent_dir`).
    pub prime_agent_dir: &'a Path,
    /// The directory the agent starts in.
    pub workdir: &'a Path,
    /// The inherited `CLAUDE_CODE_AUTO_COMPACT_WINDOW`.
    pub process_window: Option<&'a str>,
    pub teammate: &'a Teammate,
    pub model: &'a str,
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
    pub(crate) fn on_finish(&mut self, step: impl FnOnce() + 'static) {
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
    /// private home with the skills bundle linked in, Prime's daemon socket.
    /// The launch then calls [`Harness::expose_skills`] and
    /// [`Harness::expose_skills_env`] around the build.
    fn prepare(&self, _ctx: &RuntimeContext, _req: &PrepareRequest<'_>) -> Result<Prepared> {
        Ok(Prepared::default())
    }

    /// The native auto-compact setting the operator's own files set for
    /// this launch, if any (CTX-05). Default: none (no known lever).
    fn operator_window(&self, _inputs: &WindowInputs<'_>) -> Option<OperatorWindow> {
        None
    }

    /// The window value the built command really passes to the harness,
    /// read back from the command, with the environment `prepare` set
    /// (review finding 12). Default: none.
    fn window_in_command(&self, _cmd: &Command) -> Option<u64> {
        None
    }

    /// The CLI drops a prompt given on the command line of a resumed
    /// session, so the launch also types the prompt into the pane once the
    /// agent is idle
    /// ([`crate::messaging::delivery::deliver_when_idle`]).
    fn resume_prompt_typed(&self) -> bool {
        false
    }

    /// The harness's own command line. Call [`Harness::build_command`]
    /// instead, which removes the forbidden environment.
    fn command(&self, env: &LaunchEnv, spec: &CommandSpec<'_>) -> Result<Command>;

    /// Fail when this CLI cannot expose skills here, before a pane is
    /// allocated or shared rules are written.
    fn ensure_skills_supported(&self) -> Result<()> {
        Ok(())
    }

    /// The prefix this CLI shows before a bundled skill's name (`horch:`
    /// for a plugin), or `None` when it names skills bare.
    fn skill_namespace(&self) -> Option<&'static str> {
        None
    }

    /// Expose the bundle's activated skills the way this CLI discovers
    /// skills, by changing the teammate the command is built from. `home`
    /// is `$HOME`. The bundle holds exactly the activated skills, one
    /// directory each under [`Bundle::skills_dir`].
    fn expose_skills(
        &self,
        teammate: &Teammate,
        _skills: &Bundle,
        _home: Option<&Path>,
    ) -> Result<Teammate> {
        Ok(teammate.clone())
    }

    /// Expose the bundle's skills on the built command, for a CLI that reads
    /// them from an environment overlay the builder may already have set.
    /// `inherited` is the value the launch would otherwise pass through.
    fn expose_skills_env(
        &self,
        _cmd: &mut Command,
        _teammate: &Teammate,
        _skills: &Bundle,
        _inherited: Option<&str>,
    ) -> Result<()> {
        Ok(())
    }

    /// The command for `spec`, with every [`launch::FORBIDDEN_ENV`] key and
    /// every git repository variable removed. Not overridden by any harness.
    fn build_command(&self, env: &LaunchEnv, spec: &CommandSpec<'_>) -> Result<Command> {
        let mut cmd = self.command(env, spec)?;
        crate::runtime::process::scrub_child_env(&mut cmd);
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
pub(crate) fn generic_validate<H: Harness + ?Sized>(
    harness: &H,
    t: &Teammate,
) -> Vec<&'static str> {
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
    Antigravity,
    /// No real agent: the smoke teammate exercises the machinery only.
    None,
}

impl HarnessKind {
    /// Every kind, `None` included. A new harness adds itself here.
    pub const ALL: &'static [HarnessKind] = &[
        HarnessKind::Claude,
        HarnessKind::Codex,
        HarnessKind::OpenCode,
        HarnessKind::Pi,
        HarnessKind::Prime,
        HarnessKind::Antigravity,
        HarnessKind::None,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            HarnessKind::Claude => "claude",
            HarnessKind::Codex => "codex",
            HarnessKind::OpenCode => "opencode",
            HarnessKind::Pi => "pi",
            HarnessKind::Prime => "prime",
            HarnessKind::Antigravity => "antigravity",
            HarnessKind::None => "none",
        }
    }

    /// The CLI this harness runs, from `bins`. `None` for the agentless
    /// smoke harness.
    pub fn binary(self, bins: &HarnessBins) -> Option<PathBuf> {
        match self {
            HarnessKind::Claude => Some(bins.claude.clone()),
            HarnessKind::Codex => Some(bins.codex.clone()),
            HarnessKind::OpenCode => Some(bins.opencode.clone()),
            HarnessKind::Pi => Some(bins.pi.clone()),
            HarnessKind::Prime => Some(bins.prime.clone()),
            HarnessKind::Antigravity => Some(bins.antigravity.clone()),
            HarnessKind::None => None,
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
            HarnessKind::Antigravity => &antigravity::Antigravity,
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
            HarnessKind::Antigravity => &capabilities::ANTIGRAVITY,
            HarnessKind::None => &capabilities::NONE,
        }
    }

    /// Whether `model` has any effort setting on this harness. See
    /// [`crate::roster::model_takes_effort`].
    pub fn model_takes_effort(self, model: &str) -> bool {
        self.adapter().model_takes_effort(model)
    }

    /// Claude-shaped teammate fields this agent has no way to express. See
    /// [`Harness::validate`].
    pub(crate) fn unsupported_fields(self, t: &Teammate) -> Vec<&'static str> {
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
            "antigravity" => Ok(HarnessKind::Antigravity),
            "none" => Ok(HarnessKind::None),
            other => Err(format!(
                "unknown agent '{other}' (claude, codex, opencode, pi, prime, antigravity, none)"
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::HarnessKind;
    use crate::roster::Roster;

    const ALL: [HarnessKind; 7] = [
        HarnessKind::Claude,
        HarnessKind::Codex,
        HarnessKind::OpenCode,
        HarnessKind::Pi,
        HarnessKind::Prime,
        HarnessKind::Antigravity,
        HarnessKind::None,
    ];

    /// `HarnessKind::ALL` (what `horch agent-list` iterates) names every kind.
    #[test]
    fn all_names_every_kind() {
        assert_eq!(HarnessKind::ALL, &ALL[..]);
    }

    #[test]
    fn binary_names_each_harness_cli() {
        let bins = HarnessBins::resolve(&crate::runtime::BinOverrides::default(), None, None);
        let got: Vec<Option<PathBuf>> = ALL.iter().map(|k| k.binary(&bins)).collect();
        assert_eq!(
            got,
            [
                Some(bins.claude.clone()),
                Some(bins.codex.clone()),
                Some(bins.opencode.clone()),
                Some(bins.pi.clone()),
                Some(bins.prime.clone()),
                Some(bins.antigravity.clone()),
                None,
            ]
        );
    }

    /// The predicates and effort table as they were before A4, copied
    /// verbatim: (mints_session_id, harvests_session_id, runs_a_daemon,
    /// uses_execpolicy, takes_tool_lists, takes_tool_denylist, valid_efforts).
    fn legacy(kind: HarnessKind) -> (bool, bool, bool, bool, bool, bool, &'static [&'static str]) {
        use HarnessKind as A;
        (
            matches!(kind, A::Claude | A::Pi),
            matches!(kind, A::Codex | A::OpenCode | A::Prime | A::Antigravity),
            kind == A::Prime,
            kind == A::Codex,
            matches!(kind, A::Claude | A::Pi | A::Prime),
            matches!(kind, A::Claude | A::Pi),
            match kind {
                A::Claude => &["low", "medium", "high", "xhigh", "max"],
                A::Codex => &["none", "low", "medium", "high", "xhigh", "max"],
                A::OpenCode => &["none", "minimal", "low", "medium", "high", "xhigh", "max"],
                A::Pi | A::Prime => &["off", "minimal", "low", "medium", "high", "xhigh", "max"],
                A::Antigravity => &["low", "medium", "high"],
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
                HarnessKind::Antigravity | HarnessKind::None => SkillExposure::None,
            };
            assert_eq!(c.skill_exposure, exposure, "{kind}");
            assert_eq!(
                crate::roster::effort::valid_efforts(kind),
                efforts,
                "{kind}"
            );
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
            (HarnessKind::Antigravity, "antigravity"),
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
    }
}
