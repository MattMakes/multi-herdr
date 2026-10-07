//! What each harness can do, as data.
//!
//! The launch flow, the roster check and the competition preflight read these
//! values instead of matching on [`HarnessKind`](super::HarnessKind), so a new
//! harness is one new module plus one row here.

const MIB: u64 = 1024 * 1024;

/// Resident memory of one harness CLI process, before any local model.
pub const HARNESS_FOOTPRINT_BYTES: u64 = 600 * MIB;
/// Resident memory of the smoke teammate, which runs no agent CLI.
pub(crate) const NONE_FOOTPRINT_BYTES: u64 = 64 * MIB;

/// How a harness discovers the skills a launch activates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillExposure {
    None,
    /// `--plugin-dir <bundle>` plus a `--settings` overlay (claude).
    PluginDir,
    /// `--skill <dir>` (pi, Prime Agent).
    SkillFlag,
    /// `skills.paths` in `OPENCODE_CONFIG_CONTENT` (opencode).
    ConfigPaths,
    /// A `skills` link in the private `CODEX_HOME` (codex).
    CodexHome,
}

impl SkillExposure {
    /// The kebab-case name `horch agent-list` prints.
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

/// How horch asks a harness to compact, as data (CTX-13, CTX-20).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompactCommand {
    /// `/compact <instructions>` on 1 line (claude, pi, prime).
    WithInstructions,
    /// `/compact` alone: text after it becomes a prompt (codex, opencode).
    Bare,
    /// No known command (antigravity, none).
    Unknown,
}

/// How the harness computes its own auto-compact trigger (CTX-07).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriggerRule {
    /// min(setting, model window) - reserve. claude: 33_000; pi, prime: 16_384.
    WindowMinusReserve {
        reserve: u64,
    },
    /// codex: min(limit, floor(transcript window x 18 / 19)).
    CodexLimit,
    /// opencode: a fixed trigger per model
    /// (`compaction::window::OPENCODE_TRIGGERS`).
    PerModel,
    Unknown,
}

/// How a harness compacts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompactionCaps {
    pub command: CompactCommand,
    /// Sends allowed per job: 1 where a busy harness queues the command.
    pub max_sends: u8,
    pub trigger: TriggerRule,
}

impl CompactionCaps {
    /// horch can type a compact command into the pane.
    pub fn has_command(self) -> bool {
        self.command != CompactCommand::Unknown && self.max_sends > 0
    }

    /// The command takes the compaction instructions on its own line.
    pub fn takes_instructions(self) -> bool {
        self.command == CompactCommand::WithInstructions
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capabilities {
    /// horch chooses the session id before launch (`--session-id`).
    pub caller_minted_session: bool,
    /// A previous session can be resumed.
    pub resumes: bool,
    /// The effort levels the CLI accepts, by name. Empty: no effort setting.
    pub effort: &'static [&'static str],
    /// The CLI supervises its sessions in a background daemon, so each pane
    /// gets its own socket.
    pub daemon: bool,
    /// Capability comes from an execpolicy allowlist in a private home.
    pub exec_policy: bool,
    pub skill_exposure: SkillExposure,
    /// Takes `tools` (and, on claude, the `--*allowedTools` lists).
    pub tool_lists: bool,
    /// Takes a denylist of tool names.
    pub tool_denylist: bool,
    /// Can run one-shot without a terminal (the B4 headless runner).
    pub headless: bool,
    /// Resident memory of one candidate, before a local model.
    pub footprint_bytes: u64,
    /// The model runs on this machine, so its weights count against memory.
    pub local_model: bool,
    /// How horch asks it to compact, and how it triggers its own compaction.
    /// Whether it may be compacted in place is data, not a capability
    /// (`in_place:` in `teammates/_base/context-windows.md`, CTX-17).
    pub compaction: CompactionCaps,
}

impl Capabilities {
    /// The session id has to be found after launch, because the CLI mints
    /// it and only reveals it afterwards.
    pub(crate) fn discovers_session(&self) -> bool {
        self.resumes && !self.caller_minted_session
    }

    /// Resident memory of one candidate whose local model takes
    /// `local_model_bytes`.
    pub(crate) fn footprint(&self, local_model_bytes: u64) -> u64 {
        if self.local_model {
            self.footprint_bytes.saturating_add(local_model_bytes)
        } else {
            self.footprint_bytes
        }
    }
}

pub(crate) const CLAUDE: Capabilities = Capabilities {
    caller_minted_session: true,
    resumes: true,
    effort: &["low", "medium", "high", "xhigh", "max"],
    daemon: false,
    exec_policy: false,
    skill_exposure: SkillExposure::PluginDir,
    tool_lists: true,
    tool_denylist: true,
    headless: true,
    footprint_bytes: HARNESS_FOOTPRINT_BYTES,
    local_model: false,
    compaction: CompactionCaps {
        command: CompactCommand::WithInstructions,
        max_sends: 1,
        trigger: TriggerRule::WindowMinusReserve { reserve: 33_000 },
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
    headless: false,
    footprint_bytes: HARNESS_FOOTPRINT_BYTES,
    local_model: false,
    compaction: CompactionCaps {
        command: CompactCommand::Bare,
        max_sends: 3,
        trigger: TriggerRule::CodexLimit,
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
    headless: false,
    footprint_bytes: HARNESS_FOOTPRINT_BYTES,
    local_model: false,
    compaction: CompactionCaps {
        command: CompactCommand::Bare,
        max_sends: 3,
        trigger: TriggerRule::PerModel,
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
    headless: false,
    footprint_bytes: HARNESS_FOOTPRINT_BYTES,
    local_model: true,
    compaction: CompactionCaps {
        command: CompactCommand::WithInstructions,
        max_sends: 1,
        trigger: TriggerRule::WindowMinusReserve { reserve: 16_384 },
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
    headless: false,
    footprint_bytes: HARNESS_FOOTPRINT_BYTES,
    local_model: false,
    compaction: CompactionCaps {
        command: CompactCommand::WithInstructions,
        max_sends: 1,
        trigger: TriggerRule::WindowMinusReserve { reserve: 16_384 },
    },
};

/// Antigravity CLI (`agy`). It mints its own conversation ids and has no
/// flag or environment variable that points it at a skills directory: it
/// reads only the operator's global skills and the workspace `.agents/`, both
/// shared state. So it exposes no bundled skills.
pub(crate) const ANTIGRAVITY: Capabilities = Capabilities {
    caller_minted_session: false,
    resumes: true,
    effort: &["low", "medium", "high"],
    daemon: false,
    exec_policy: false,
    skill_exposure: SkillExposure::None,
    tool_lists: false,
    tool_denylist: false,
    headless: false,
    footprint_bytes: HARNESS_FOOTPRINT_BYTES,
    local_model: false,
    compaction: CompactionCaps {
        command: CompactCommand::Unknown,
        max_sends: 0,
        trigger: TriggerRule::Unknown,
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
    headless: false,
    footprint_bytes: NONE_FOOTPRINT_BYTES,
    local_model: false,
    compaction: CompactionCaps {
        command: CompactCommand::Unknown,
        max_sends: 0,
        trigger: TriggerRule::Unknown,
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
}
