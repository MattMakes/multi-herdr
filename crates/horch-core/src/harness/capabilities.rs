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
};
