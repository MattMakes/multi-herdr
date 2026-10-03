//! The agent CLIs a worker can run, and what each one can do.
//!
//! [`HarnessKind`] is the enum `teammates::Agent` used to be;
//! `teammates::Agent` stays as an alias so existing callers compile. What
//! each harness can do is [`Capabilities`] data, read through
//! [`HarnessKind::capabilities`].

pub mod capabilities;

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::teammates::Teammate;

pub use capabilities::{Capabilities, SkillExposure};

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
        match self {
            HarnessKind::Claude => !model
                .split(|c: char| !c.is_ascii_alphanumeric())
                .any(|seg| seg.eq_ignore_ascii_case("haiku")),
            HarnessKind::OpenCode => {
                !(model.starts_with("opencode/")
                    && (model.ends_with("-free") || model == "opencode/big-pickle"))
            }
            HarnessKind::None => false,
            HarnessKind::Codex | HarnessKind::Pi | HarnessKind::Prime => true,
        }
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

    /// Claude-shaped teammate fields this agent has no way to express.
    ///
    /// Setting one of these is asking for isolation or capability that would
    /// silently not happen, so the roster check rejects it by name rather than
    /// launching a worker whose author believes it is constrained.
    pub fn unsupported_fields(self, t: &Teammate) -> Vec<&'static str> {
        if self == HarnessKind::Claude || self == HarnessKind::None {
            return Vec::new();
        }
        let mut out = Vec::new();
        if t.tools.is_some() && !self.takes_tool_lists() {
            out.push("tools");
        }
        if !t.disallowed_tools.is_empty() && !self.takes_tool_denylist() {
            out.push("disallowed_tools");
        }
        // Claude's "permit this without asking" list. pi has an allowlist and a
        // denylist but nothing that grants a tool permission, so there is
        // nowhere for this to go on any other agent.
        if !t.allowed_tools.is_empty() {
            out.push("allowed_tools");
        }
        // Codex skills are installed into its private home; it cannot disable
        // all other operator extensions with a CLI switch.
        if self == HarnessKind::Codex && !t.inherit_plugins {
            out.push("inherit_plugins");
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
