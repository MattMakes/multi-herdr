use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::{PermissionMode, Phase, Requirement};
use crate::harness::HarnessKind;

pub(crate) const ORCHESTRATOR_TIERS: [(&str, &str); 2] =
    [("fable", "opus"), ("astra", "codex-sol")];

pub(crate) const TOP_TIER_SEATS: [(&str, &str); 3] = [
    ("fable-creative", "fable"),
    ("astra-creative", "astra"),
    ("game-design-juicer", "fable"),
];

pub(crate) const SEATS_WITH_FALLBACKS: [&str; 1] = ["game-design-juicer"];

pub(crate) const FLEET_ORCHESTRATORS: [&str; 2] = ["orchestrator", "orchestrator-codex"];

pub(crate) const ORCHESTRATOR_ONLY_SKILLS: [&str; 2] = ["orchestrate", "skill-creator"];

pub(crate) const ORCHESTRATOR_DENIED_TOOLS: [&str; 2] = ["Agent", "RemoteTrigger"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelSource {
    Teammate,
    Operator,
}

pub fn reserved_tier(model: &str) -> Option<(&'static str, &'static str)> {
    model
        .split(|c: char| !c.is_ascii_alphanumeric())
        .find_map(|segment| {
            ORCHESTRATOR_TIERS
                .into_iter()
                .find(|(tier, _)| segment.eq_ignore_ascii_case(tier))
        })
}

pub(crate) const BRIEF_DESCRIPTION_MAX: usize = 120;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Teammate {
    pub name: String,
    pub brief_description: String,
    #[serde(default)]
    pub generic: bool,
    #[serde(default)]
    pub hidden: bool,
    #[serde(default)]
    pub base: Option<String>,
    pub agent: HarnessKind,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub effort: Option<String>,
    #[serde(default)]
    pub subagent_model: Option<String>,
    #[serde(default = "yes")]
    pub inherit_plugins: bool,
    #[serde(default)]
    pub plugin_dirs: Vec<String>,
    #[serde(default)]
    pub skills: Vec<String>,
    #[serde(default)]
    pub available_skills: Vec<String>,
    #[serde(default)]
    pub operator_skills: Option<OperatorSkills>,
    #[serde(default)]
    pub phase: Option<Phase>,
    #[serde(default)]
    pub permission_mode: Option<PermissionMode>,
    #[serde(default)]
    pub tools: Option<Vec<String>>,
    #[serde(default)]
    pub allowed_tools: Vec<String>,
    #[serde(default)]
    pub disallowed_tools: Vec<String>,
    #[serde(default)]
    pub allow_subagents: bool,
    #[serde(default)]
    pub setting_sources: Option<Vec<String>>,
    #[serde(default)]
    pub disable_skills: bool,
    #[serde(default)]
    pub inherit_claudeai_skills: bool,
    #[serde(default)]
    pub disabled_skills: Vec<String>,
    #[serde(default)]
    pub plugin_skills: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    pub remote_control: bool,
    #[serde(default)]
    pub worker_channel: bool,
    #[serde(default)]
    pub settings: Option<String>,
    #[serde(default)]
    pub sandbox: Option<serde_json::Map<String, serde_json::Value>>,
    #[serde(default)]
    pub horch_access: HorchAccess,
    #[serde(default)]
    pub spawn_only: Vec<String>,
    #[serde(default)]
    pub mcp_servers: Option<BTreeMap<String, serde_json::Value>>,
    #[serde(default)]
    pub mcp_config_files: Vec<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub trains_on_input: bool,
    #[serde(default)]
    pub first_instruction: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_prompt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_prompt_text: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fallbacks: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub offer_when: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub skills_when: BTreeMap<String, Vec<String>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub requires: Vec<Requirement>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub harness_defaults: Vec<HarnessDefault>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compact_window: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compact_at: Option<u64>,
    #[serde(skip)]
    pub persona: String,
}

impl Default for Teammate {
    fn default() -> Self {
        Teammate {
            name: String::new(),
            brief_description: String::new(),
            generic: false,
            hidden: false,
            base: None,
            agent: HarnessKind::Claude,
            model: None,
            effort: None,
            subagent_model: None,
            inherit_plugins: true,
            plugin_dirs: Vec::new(),
            skills: Vec::new(),
            available_skills: Vec::new(),
            operator_skills: None,
            phase: None,
            permission_mode: None,
            tools: None,
            allowed_tools: Vec::new(),
            disallowed_tools: Vec::new(),
            allow_subagents: false,
            setting_sources: None,
            disable_skills: false,
            inherit_claudeai_skills: false,
            disabled_skills: Vec::new(),
            plugin_skills: BTreeMap::new(),
            remote_control: false,
            worker_channel: false,
            settings: None,
            sandbox: None,
            horch_access: HorchAccess::Full,
            spawn_only: Vec::new(),
            mcp_servers: None,
            mcp_config_files: Vec::new(),
            args: Vec::new(),
            env: BTreeMap::new(),
            trains_on_input: false,
            first_instruction: None,
            system_prompt: None,
            system_prompt_text: None,
            fallbacks: Vec::new(),
            offer_when: Vec::new(),
            skills_when: BTreeMap::new(),
            requires: Vec::new(),
            harness_defaults: Vec::new(),
            compact_window: None,
            compact_at: None,
            persona: String::new(),
        }
    }
}

impl Teammate {
    pub fn may_spawn(&self, name: &str) -> bool {
        self.spawn_only.is_empty()
            || self
                .spawn_only
                .iter()
                .any(|pattern| super::offer::glob_match(pattern, name))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperatorSkills {
    pub dir: String,
    pub names: Vec<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HorchAccess {
    #[default]
    Full,
    Report,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExecRule {
    pub pattern: String,
    pub justification: String,
    #[serde(default)]
    pub decision: ExecDecision,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExecDecision {
    #[default]
    Allow,
    Forbidden,
}

impl ExecDecision {
    pub fn as_str(self) -> &'static str {
        match self {
            ExecDecision::Allow => "allow",
            ExecDecision::Forbidden => "forbidden",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Base {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub skills_instruction: String,
    #[serde(default)]
    pub trains_on_input: String,
    #[serde(default)]
    pub task_fresh: String,
    #[serde(default)]
    pub task_resume: String,
    #[serde(default)]
    pub task_idle: String,
    #[serde(default)]
    pub rules: Vec<ExecRule>,
    #[serde(default)]
    pub defaults: Vec<HarnessDefault>,
    #[serde(default)]
    pub windows: Option<BTreeMap<String, u64>>,
    #[serde(default)]
    pub in_place: Option<Vec<String>>,
    #[serde(default)]
    pub messages: BTreeMap<String, String>,
    #[serde(skip)]
    pub body: String,
}

pub(crate) const HARNESS_DEFAULTS_BASE: &str = "harness-defaults";

pub(crate) const CONTEXT_WINDOWS_BASE: &str = "context-windows";

pub(crate) const CONTEXT_MESSAGES_BASE: &str = "context-messages";

pub(crate) const CONTEXT_MESSAGE_KEYS: [&str; 12] = [
    "request",
    "warning",
    "warning-orchestrator",
    "instructions",
    "instructions-orchestrator",
    "resume",
    "resume-orchestrator",
    "hydrate",
    "hydrate-orchestrator",
    "hydrate-missing",
    "reported",
    "failed",
];

pub(crate) const CONTEXT_MESSAGE_PLACEHOLDERS: [&str; 9] = [
    "role",
    "tokens",
    "threshold",
    "handoff",
    "pre",
    "post",
    "step",
    "reason",
    "log",
];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HarnessDefault {
    pub harness: HarnessKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settings: Option<serde_json::Map<String, serde_json::Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config: Option<serde_json::Map<String, serde_json::Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_settings: Option<serde_json::Map<String, serde_json::Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub force: Option<String>,
}

impl HarnessDefault {
    pub fn for_harness(
        all: &[HarnessDefault],
        kind: HarnessKind,
    ) -> impl Iterator<Item = &HarnessDefault> {
        all.iter().filter(move |d| d.harness == kind)
    }

    pub(crate) fn matches_base(&self, base: Option<&str>) -> bool {
        self.base.is_none() || self.base.as_deref() == base
    }

    pub fn config_keys(args: &[String]) -> Vec<&str> {
        let mut keys = Vec::new();
        let mut it = args.iter();
        while let Some(arg) = it.next() {
            if arg == "-c" {
                if let Some(key) = it.next().and_then(|pair| pair.split_once('=')) {
                    keys.push(key.0.trim());
                }
            }
        }
        keys
    }
}

impl Default for HarnessDefault {
    fn default() -> Self {
        HarnessDefault {
            harness: HarnessKind::Claude,
            base: None,
            env: BTreeMap::new(),
            args: Vec::new(),
            settings: None,
            config: None,
            agent_settings: None,
            force: None,
        }
    }
}
