pub mod antigravity;
pub mod capabilities;
pub mod claude;
pub(crate) mod claude_plugins;
pub mod codex;
pub mod cooldown;
pub mod inventory;
pub mod launch;
pub mod none;
pub mod opencode;
pub mod pi;
pub mod prime;
pub mod trust;
pub mod update;

use horch_platform::start::Command;
use std::ffi::OsString;
use std::fmt;
use std::path::{Path, PathBuf};
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

pub struct PrepareRequest<'a> {
    pub role: &'a str,
    pub exec_rules: &'a [ExecRule],
    pub skills: Option<&'a Bundle>,
    pub compact_window: Option<&'a WindowDecision>,
    pub teammate: &'a Teammate,
    pub model: &'a str,
    pub workdir: &'a Path,
    pub shell_env: &'a [(String, String)],
}

pub struct WindowInputs<'a> {
    pub home: &'a Path,
    pub claude_config_dir: Option<&'a Path>,
    pub claude_managed_settings: &'a Path,
    pub codex_home: &'a Path,
    pub prime_agent_dir: &'a Path,
    pub workdir: &'a Path,
    pub process_window: Option<&'a str>,
    pub teammate: &'a Teammate,
    pub model: &'a str,
}

#[derive(Default)]
pub struct Prepared {
    pub extra_args: Vec<String>,
    pub env: Vec<(String, OsString)>,
    pub sessions_dir: Option<PathBuf>,
    finish: Vec<Box<dyn FnOnce()>>,
}

impl Prepared {
    pub(crate) fn on_finish(&mut self, step: impl FnOnce() + 'static) {
        self.finish.push(Box::new(step));
    }

    pub fn finish(self) {
        for step in self.finish {
            step();
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct CommandSpec<'a> {
    pub teammate: &'a Teammate,
    pub session: Session<'a>,
    pub prompt: &'a str,
    pub model_override: Option<&'a str>,
}

pub trait Harness: Sync {
    fn kind(&self) -> HarnessKind;

    fn capabilities(&self) -> &'static Capabilities {
        self.kind().capabilities()
    }

    fn validate(&self, t: &Teammate) -> Vec<&'static str> {
        generic_validate(self, t)
    }

    fn model_takes_effort(&self, _model: &str) -> bool {
        !self.capabilities().effort.is_empty()
    }

    fn prepare(&self, _ctx: &RuntimeContext, _req: &PrepareRequest<'_>) -> Result<Prepared> {
        Ok(Prepared::default())
    }

    fn operator_window(&self, _inputs: &WindowInputs<'_>) -> Option<OperatorWindow> {
        None
    }

    fn window_in_command(&self, _cmd: &Command) -> Option<u64> {
        None
    }

    fn resume_prompt_typed(&self) -> bool {
        false
    }

    fn command(&self, env: &LaunchEnv, spec: &CommandSpec<'_>) -> Result<Command>;

    fn ensure_skills_supported(&self) -> Result<()> {
        Ok(())
    }

    fn skill_namespace(&self) -> Option<&'static str> {
        None
    }

    fn expose_skills(
        &self,
        teammate: &Teammate,
        _skills: &Bundle,
        _home: Option<&Path>,
    ) -> Result<Teammate> {
        Ok(teammate.clone())
    }

    fn expose_skills_env(
        &self,
        _cmd: &mut Command,
        _teammate: &Teammate,
        _skills: &Bundle,
        _inherited: Option<&str>,
    ) -> Result<()> {
        Ok(())
    }

    fn build_command(&self, env: &LaunchEnv, spec: &CommandSpec<'_>) -> Result<Command> {
        let mut cmd = self.command(env, spec)?;
        crate::runtime::process::scrub_child_env(&mut cmd);
        Ok(cmd)
    }

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
    if !t.allowed_tools.is_empty() {
        out.push("allowed_tools");
    }
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
        ("system_prompt", t.system_prompt.is_some()),
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
    None,
}

impl HarnessKind {
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

    pub fn model_takes_effort(self, model: &str) -> bool {
        self.adapter().model_takes_effort(model)
    }

    pub(crate) fn unsupported_fields(self, t: &Teammate) -> Vec<&'static str> {
        self.adapter().validate(t)
    }
}

impl fmt::Display for HarnessKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl HarnessKind {
    pub fn channel_output(
        self,
        codex_bin: std::path::PathBuf,
        thread: Box<dyn Fn() -> anyhow::Result<String> + Send + Sync>,
    ) -> anyhow::Result<crate::messaging::channel::Output> {
        use crate::messaging::channel::Output;
        match self {
            HarnessKind::Claude => Ok(Output::ClaudeNotification),
            HarnessKind::Codex => Ok(Output::CodexQueue { codex_bin, thread }),
            other => anyhow::bail!("agent '{other}' has no channel relay; use claude or codex"),
        }
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

        let t = Roster::builtin().unwrap();
        let t = t.require("opencode-pickle").unwrap();
        assert_eq!(t.agent, HarnessKind::OpenCode);
    }
}
