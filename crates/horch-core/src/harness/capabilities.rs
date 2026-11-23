#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillExposure {
    None,
    PluginDir,
    SkillFlag,
    ConfigPaths,
    CodexHome,
}

impl SkillExposure {
    pub fn as_str(self) -> &'static str {
        match self {
            SkillExposure::None => "none",
            SkillExposure::PluginDir => "plugin-dir",
            SkillExposure::SkillFlag => "skill-flag",
            SkillExposure::ConfigPaths => "config-paths",
            SkillExposure::CodexHome => "codex-home",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompactCommand {
    WithInstructions,
    Bare,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriggerRule {
    WindowMinusReserve { reserve: u64 },
    CodexLimit,
    PerModel,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompactionCaps {
    pub command: CompactCommand,
    pub max_sends: u8,
    pub trigger: TriggerRule,
    pub failure_lines: &'static [&'static str],
}

impl CompactionCaps {
    pub fn has_command(self) -> bool {
        self.command != CompactCommand::Unknown && self.max_sends > 0
    }

    pub fn takes_instructions(self) -> bool {
        self.command == CompactCommand::WithInstructions
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capabilities {
    pub caller_minted_session: bool,
    pub resumes: bool,
    pub effort: &'static [&'static str],
    pub daemon: bool,
    pub exec_policy: bool,
    pub skill_exposure: SkillExposure,
    pub tool_lists: bool,
    pub tool_denylist: bool,
    pub extensions_flag: bool,
    pub tool_selection: bool,
    pub headless: bool,
    pub local_model: bool,
    pub compaction: CompactionCaps,
}

impl Capabilities {
    pub(crate) fn discovers_session(&self) -> bool {
        self.resumes && !self.caller_minted_session
    }
}

const CLAUDE_FAILURES: &[&str] = &[
    "Error during compaction: ",
    "Not enough messages to compact.",
    "Compaction canceled.",
];

const CODEX_FAILURES: &[&str] = &["thread/compact/start failed in TUI"];

const OPENCODE_FAILURES: &[&str] = &[
    "Conversation history too large to compact",
    "Session too large to compact",
];

const PI_FAILURES: &[&str] = &["Error: Compaction failed: ", "Error: Compaction cancelled"];

const PRIME_FAILURES: &[&str] = &[
    "Error: Compaction failed: ",
    "Error: Compaction cancelled",
    "Already compacted",
    "Session is too short to compact",
];

pub(crate) const CLAUDE: Capabilities = Capabilities {
    caller_minted_session: true,
    resumes: true,
    effort: &["low", "medium", "high", "xhigh", "max"],
    daemon: false,
    exec_policy: false,
    skill_exposure: SkillExposure::PluginDir,
    tool_lists: true,
    tool_denylist: true,
    extensions_flag: false,
    tool_selection: false,
    headless: true,
    local_model: false,
    compaction: CompactionCaps {
        command: CompactCommand::WithInstructions,
        max_sends: 1,
        trigger: TriggerRule::WindowMinusReserve { reserve: 33_000 },
        failure_lines: CLAUDE_FAILURES,
    },
};

pub(crate) const CODEX: Capabilities = Capabilities {
    caller_minted_session: false,
    resumes: true,
    effort: &["none", "low", "medium", "high", "xhigh", "max"],
    daemon: false,
    exec_policy: true,
    skill_exposure: SkillExposure::CodexHome,
    tool_lists: false,
    tool_denylist: false,
    extensions_flag: false,
    tool_selection: false,
    headless: false,
    local_model: false,
    compaction: CompactionCaps {
        command: CompactCommand::Bare,
        max_sends: 3,
        trigger: TriggerRule::CodexLimit,
        failure_lines: CODEX_FAILURES,
    },
};

pub(crate) const OPENCODE: Capabilities = Capabilities {
    caller_minted_session: false,
    resumes: true,
    effort: &["none", "minimal", "low", "medium", "high", "xhigh", "max"],
    daemon: false,
    exec_policy: false,
    skill_exposure: SkillExposure::ConfigPaths,
    tool_lists: false,
    tool_denylist: false,
    extensions_flag: false,
    tool_selection: false,
    headless: false,
    local_model: false,
    compaction: CompactionCaps {
        command: CompactCommand::Bare,
        max_sends: 3,
        trigger: TriggerRule::PerModel,
        failure_lines: OPENCODE_FAILURES,
    },
};

const PI_FAMILY_EFFORT: &[&str] = &["off", "minimal", "low", "medium", "high", "xhigh", "max"];

pub(crate) const PI: Capabilities = Capabilities {
    caller_minted_session: true,
    resumes: true,
    effort: PI_FAMILY_EFFORT,
    daemon: false,
    exec_policy: false,
    skill_exposure: SkillExposure::SkillFlag,
    tool_lists: true,
    tool_denylist: true,
    extensions_flag: true,
    tool_selection: true,
    headless: false,
    local_model: true,
    compaction: CompactionCaps {
        command: CompactCommand::WithInstructions,
        max_sends: 1,
        trigger: TriggerRule::WindowMinusReserve { reserve: 16_384 },
        failure_lines: PI_FAILURES,
    },
};

pub(crate) const PRIME: Capabilities = Capabilities {
    caller_minted_session: false,
    resumes: true,
    effort: PI_FAMILY_EFFORT,
    daemon: true,
    exec_policy: false,
    skill_exposure: SkillExposure::SkillFlag,
    tool_lists: true,
    tool_denylist: false,
    extensions_flag: false,
    tool_selection: false,
    headless: false,
    local_model: false,
    compaction: CompactionCaps {
        command: CompactCommand::WithInstructions,
        max_sends: 1,
        trigger: TriggerRule::WindowMinusReserve { reserve: 16_384 },
        failure_lines: PRIME_FAILURES,
    },
};

pub(crate) const ANTIGRAVITY: Capabilities = Capabilities {
    caller_minted_session: false,
    resumes: true,
    effort: &["low", "medium", "high"],
    daemon: false,
    exec_policy: false,
    skill_exposure: SkillExposure::None,
    tool_lists: false,
    tool_denylist: false,
    extensions_flag: false,
    tool_selection: false,
    headless: false,
    local_model: false,
    compaction: CompactionCaps {
        command: CompactCommand::Unknown,
        max_sends: 0,
        trigger: TriggerRule::Unknown,
        failure_lines: &[],
    },
};

pub(crate) const NONE: Capabilities = Capabilities {
    caller_minted_session: false,
    resumes: false,
    effort: &[],
    daemon: false,
    exec_policy: false,
    skill_exposure: SkillExposure::None,
    tool_lists: false,
    tool_denylist: false,
    extensions_flag: false,
    tool_selection: false,
    headless: false,
    local_model: false,
    compaction: CompactionCaps {
        command: CompactCommand::Unknown,
        max_sends: 0,
        trigger: TriggerRule::Unknown,
        failure_lines: &[],
    },
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skill_exposure_names_are_kebab_case() {
        let names = [
            (SkillExposure::None, "none"),
            (SkillExposure::PluginDir, "plugin-dir"),
            (SkillExposure::SkillFlag, "skill-flag"),
            (SkillExposure::ConfigPaths, "config-paths"),
            (SkillExposure::CodexHome, "codex-home"),
        ];
        for (exposure, name) in names {
            assert_eq!(exposure.as_str(), name);
        }
    }

    #[test]
    fn arc_10_extensions_flag_and_tool_selection_only_for_pi() {
        for (name, caps) in [
            ("claude", CLAUDE),
            ("codex", CODEX),
            ("opencode", OPENCODE),
            ("pi", PI),
            ("prime", PRIME),
            ("antigravity", ANTIGRAVITY),
            ("none", NONE),
        ] {
            assert_eq!(caps.extensions_flag, name == "pi", "{name}");
            assert_eq!(caps.tool_selection, name == "pi", "{name}");
        }
    }
}
