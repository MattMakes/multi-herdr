//! The roster: one markdown file per team member, read at run time.
//!
//! Every briefing this fleet sends to an agent originates in `teammates/`. This
//! module reads those files and substitutes placeholders; it never composes,
//! paraphrases, or conditionally rewrites their prose. A prompt that appears in
//! a pane is byte-for-byte what the `.md` file says, with `{name}` spans
//! replaced. If a briefing needs to change, the file changes.
//!
//! Layering, lowest precedence first:
//!
//! | layer                        | why |
//! |------------------------------|-----|
//! | compiled-in (`build.rs`)     | `horch install` is a plain binary copy, so a fleet must spawn with no repo checked out |
//! | `~/.config/horch/teammates`  | user-wide additions |
//! | `$HORCH_TEAMMATES_DIR`       | this repo's folder, exported by the justfile; tests and one-offs |
//!
//! Overlay is by whole entry, keyed on name. `<cwd>/teammates` is deliberately
//! NOT searched: a worker's cwd is the target project, and a stray folder there
//! would silently re-brief the fleet.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

include!(concat!(env!("OUT_DIR"), "/builtin_teammates.rs"));

// `TEMPLATE` comes from the generated file above: the annotated `_template.md`,
// used both to scaffold new teammates and to hold the documented field list to
// the struct in a test.

/// The model tiers reserved for the orchestrator, and the teammate to reach for
/// instead of each.
///
/// A fleet has at most one top-tier session: the orchestrator, when it runs on
/// Fable or Astra. `horch spawn` refuses to start a worker on EITHER tier,
/// whichever flavor is orchestrating - so a Fable orchestrator cannot start an
/// Astra, an Astra cannot start a Fable, and neither can clone itself. The
/// Opus and Sol flavors (`horch fleet opus|sol`) hold no reserved tier, so the
/// fleet then has none; their workers may share the orchestrator's model. The
/// orchestrator writes every brief for an Opus/Codex-Sol reader either way.
pub const ORCHESTRATOR_TIERS: [(&str, &str); 2] = [("fable", "opus"), ("astra", "codex-sol")];

/// The reserved tier a model belongs to, if any.
///
/// Matched on the model's name segments rather than the whole slug, because the
/// two agents name their models differently and both keep moving: `fable`,
/// `claude-fable-5-1` and `gpt-6-astra` are all reserved, while `opus` and
/// `gpt-5.6-sol` are not. A version bump stays reserved without an edit here.
pub fn reserved_tier(model: &str) -> Option<(&'static str, &'static str)> {
    model
        .split(|c: char| !c.is_ascii_alphanumeric())
        .find_map(|segment| {
            ORCHESTRATOR_TIERS
                .into_iter()
                .find(|(tier, _)| segment.eq_ignore_ascii_case(tier))
        })
}

/// The effort levels each agent's CLI accepts, by name.
///
/// Same field, different mechanism per agent (see `_template.md`), and the
/// sets differ: codex has `none` but rejects `minimal`, pi has `off`, claude
/// tops out at `max`. Checked at `--check` and at `horch spawn --effort`, so a
/// typo fails before a pane starts rather than inside one nobody watches.
pub fn valid_efforts(agent: Agent) -> &'static [&'static str] {
    match agent {
        Agent::Claude => &["low", "medium", "high", "xhigh", "max"],
        Agent::Codex => &["none", "low", "medium", "high", "xhigh", "max"],
        Agent::Opencode => &["none", "minimal", "low", "medium", "high", "xhigh", "max"],
        Agent::Pi | Agent::Prime => &["off", "minimal", "low", "medium", "high", "xhigh", "max"],
        Agent::None => &[],
    }
}

/// Whether `model` has any effort setting at all on `agent`.
///
/// Claude's Haiku 4.5 has none, so claude rejects `--effort` for it. The
/// OpenCode free-tier models report `variants: {}` (`opencode models
/// --verbose`, ai_docs/reports/env-research/codex-opencode.md), so an effort
/// there is silently a no-op - worse than an error, because the file then
/// claims a setting that is not happening.
pub fn model_takes_effort(agent: Agent, model: &str) -> bool {
    match agent {
        Agent::Claude => !model
            .split(|c: char| !c.is_ascii_alphanumeric())
            .any(|seg| seg.eq_ignore_ascii_case("haiku")),
        Agent::Opencode => !(model.starts_with("opencode/")
            && (model.ends_with("-free") || model == "opencode/big-pickle")),
        Agent::None => false,
        _ => true,
    }
}

/// Why `effort` cannot be used with this agent and model, or `None` if it can.
pub fn effort_problem(agent: Agent, model: Option<&str>, effort: &str) -> Option<String> {
    let model = model.unwrap_or_default();
    if !model.is_empty() && !model_takes_effort(agent, model) {
        return Some(format!(
            "{model} has no effort setting on {agent}; remove effort (it would be \
             ignored, or refused)"
        ));
    }
    if agent == Agent::Codex {
        // Both are accepted by some codex clients and both are traps.
        match effort {
            "minimal" => {
                return Some(
                    "codex effort 'minimal' is an API error on the gpt-5.6 models; use low"
                        .into(),
                )
            }
            "ultra" => {
                return Some(
                    "codex effort 'ultra' fans out to parallel client-side agents and \
                     multiplies spend; use max"
                        .into(),
                )
            }
            "none" if reserved_tier(model).is_some() => {
                return Some(format!("{model} does not accept effort 'none'; use low"))
            }
            _ => {}
        }
    }
    let valid = valid_efforts(agent);
    if valid.contains(&effort) {
        None
    } else {
        Some(format!(
            "effort '{effort}' is not a {agent} level (expected one of: {})",
            valid.join(", ")
        ))
    }
}

/// Longest a `brief_description` may be. Every non-hidden teammate's
/// description is concatenated into the orchestrator's briefing on every run,
/// so this is a direct, permanent tax on the orchestrator's context.
pub const BRIEF_DESCRIPTION_MAX: usize = 120;

/// Work phase selecting the portable skill catalog for a worker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Research,
    Plan,
    Implementation,
    Validation,
}

impl Phase {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Research => "research",
            Self::Plan => "plan",
            Self::Implementation => "implementation",
            Self::Validation => "validation",
        }
    }
}

impl fmt::Display for Phase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Phase {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "research" => Ok(Self::Research),
            "plan" => Ok(Self::Plan),
            "implementation" => Ok(Self::Implementation),
            "validation" => Ok(Self::Validation),
            _ => Err(format!(
                "unknown phase '{s}' (research, plan, implementation, validation)"
            )),
        }
    }
}

/// Which CLI a teammate launches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Agent {
    Claude,
    Codex,
    Opencode,
    Pi,
    Prime,
    /// No real agent: the smoke teammate exercises the machinery only.
    None,
}

impl Agent {
    pub fn as_str(self) -> &'static str {
        match self {
            Agent::Claude => "claude",
            Agent::Codex => "codex",
            Agent::Opencode => "opencode",
            Agent::Pi => "pi",
            Agent::Prime => "prime",
            Agent::None => "none",
        }
    }

    /// Whether horch can choose this agent's session id before it launches.
    ///
    /// Claude takes `--session-id`, and pi takes `--session-id` with "create it
    /// if missing" semantics, so the ledger knows the resume handle before the
    /// pane even starts. Codex, OpenCode and Prime Agent mint their own and only
    /// reveal it afterwards - verified against `prime-agent --help` 0.9.4, which
    /// has `--session-dir` but no `--session-id`.
    pub fn mints_session_id(self) -> bool {
        matches!(self, Agent::Claude | Agent::Pi)
    }

    /// Whether the session id has to be recovered after launch, by watching
    /// wherever this agent records its sessions.
    pub fn harvests_session_id(self) -> bool {
        matches!(self, Agent::Codex | Agent::Opencode | Agent::Prime)
    }

    /// Whether this agent supervises its own sessions in a background service.
    ///
    /// Only Prime Agent does, and it is why a Prime pane gets its own daemon
    /// socket: herdr already treats a pane as an agent's lifetime, so an
    /// unscoped daemon would outlive `horch done` and accumulate one per spawn.
    pub fn runs_a_daemon(self) -> bool {
        self == Agent::Prime
    }

    /// Whether this agent's capability comes from an execpolicy allowlist rather
    /// than from flags. Only codex works that way, and it is why a codex pane
    /// gets a private `CODEX_HOME`.
    pub fn uses_execpolicy(self) -> bool {
        self == Agent::Codex
    }

    /// Whether this agent takes `tools` / `allowed_tools` / `disallowed_tools`.
    ///
    /// Claude has `--tools` and the two `--*allowedTools` lists; pi and Prime
    /// Agent have `--tools`, and pi alone adds `--exclude-tools`. Codex and
    /// OpenCode have neither.
    pub fn takes_tool_lists(self) -> bool {
        matches!(self, Agent::Claude | Agent::Pi | Agent::Prime)
    }

    /// Whether a denylist of tool names has anywhere to go. Prime Agent 0.9.4
    /// has `--tools` and `--no-tools` but no `--exclude-tools`.
    pub fn takes_tool_denylist(self) -> bool {
        matches!(self, Agent::Claude | Agent::Pi)
    }

    /// Claude-shaped teammate fields this agent has no way to express.
    ///
    /// Setting one of these is asking for isolation or capability that would
    /// silently not happen, so the roster check rejects it by name rather than
    /// launching a worker whose author believes it is constrained.
    pub fn unsupported_fields(self, t: &Teammate) -> Vec<&'static str> {
        if self == Agent::Claude || self == Agent::None {
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
        if self == Agent::Codex && !t.inherit_plugins {
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

impl fmt::Display for Agent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Agent {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "claude" => Ok(Agent::Claude),
            "codex" => Ok(Agent::Codex),
            "opencode" => Ok(Agent::Opencode),
            "pi" => Ok(Agent::Pi),
            "prime" => Ok(Agent::Prime),
            "none" => Ok(Agent::None),
            other => Err(format!(
                "unknown agent '{other}' (claude, codex, opencode, pi, prime, none)"
            )),
        }
    }
}

/// How much a teammate may do without stopping to ask.
///
/// Claude takes these directly. Codex has no single equivalent, so
/// [`PermissionMode::codex_args`] maps them onto its sandbox/approval pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PermissionMode {
    #[serde(rename = "acceptEdits")]
    AcceptEdits,
    #[serde(rename = "auto")]
    Auto,
    #[serde(rename = "bypassPermissions")]
    BypassPermissions,
    #[serde(rename = "manual")]
    Manual,
    #[serde(rename = "dontAsk")]
    DontAsk,
    #[serde(rename = "plan")]
    Plan,
}

impl PermissionMode {
    pub fn as_str(self) -> &'static str {
        match self {
            PermissionMode::AcceptEdits => "acceptEdits",
            PermissionMode::Auto => "auto",
            PermissionMode::BypassPermissions => "bypassPermissions",
            PermissionMode::Manual => "manual",
            PermissionMode::DontAsk => "dontAsk",
            PermissionMode::Plan => "plan",
        }
    }

    /// OpenCode's single approval flag for this mode.
    ///
    /// OpenCode has one lever - `--auto`, "auto-approve permissions that are not
    /// explicitly denied" - so the modes collapse into "pass it" or "do not".
    /// `Plan` and the two ask-shaped modes have no analogue at all: there is no
    /// read-only mode to put it in, and a worker left to prompt in a pane nobody
    /// is watching stalls forever. Returning `None` makes that an error.
    pub fn opencode_args(self) -> Option<Vec<String>> {
        match self {
            PermissionMode::Auto | PermissionMode::BypassPermissions => {
                Some(vec!["--auto".to_string()])
            }
            // OpenCode still asks about anything explicitly denied, which is the
            // closest thing it has to "accept edits, ask for the rest".
            PermissionMode::AcceptEdits => Some(vec!["--auto".to_string()]),
            PermissionMode::Plan | PermissionMode::Manual | PermissionMode::DontAsk => None,
        }
    }

    /// Codex sandbox + approval flags for this mode.
    ///
    /// `Manual` and `DontAsk` have no codex analogue. Returning `None` lets the
    /// caller raise an error rather than silently downgrade a teammate to
    /// something more permissive than its file asked for.
    pub fn codex_args(self) -> Option<Vec<String>> {
        let pair: &[&str] = match self {
            PermissionMode::Plan => &["-s", "read-only", "-a", "on-request"],
            PermissionMode::AcceptEdits => &["-s", "workspace-write", "-a", "on-request"],
            PermissionMode::Auto => &["-s", "workspace-write", "-a", "never"],
            PermissionMode::BypassPermissions => &["--dangerously-bypass-approvals-and-sandbox"],
            PermissionMode::Manual | PermissionMode::DontAsk => return None,
        };
        Some(pair.iter().map(|s| s.to_string()).collect())
    }
}

/// One team member: the frontmatter of a `teammates/*.md` file, plus its body.
///
/// `deny_unknown_fields` is deliberate. A misspelled key in a hand-written file
/// would otherwise be silently ignored, producing a worker launched with
/// permissions or tools its author believed they had set.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Teammate {
    pub name: String,
    pub brief_description: String,
    #[serde(default)]
    pub generic: bool,
    /// Loaded and spawnable by name, but never offered in the roster.
    #[serde(default)]
    pub hidden: bool,
    /// Names a file in `_base/`. When absent, the body IS the whole prompt.
    #[serde(default)]
    pub base: Option<String>,
    pub agent: Agent,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub effort: Option<String>,
    #[serde(default)]
    pub subagent_model: Option<String>,
    /// Whether the operator's globally-enabled plugins load into this agent.
    /// `false` switches each of them off for this session only, by name, via
    /// a `--settings` override - the operator's other settings stay exactly as
    /// tuned. `plugin_dirs` then adds back only what this teammate needs.
    #[serde(default = "yes")]
    pub inherit_plugins: bool,
    #[serde(default)]
    pub plugin_dirs: Vec<String>,
    #[serde(default)]
    pub skills: Vec<String>,
    /// Default work phase; spawn --phase overrides it.
    #[serde(default)]
    pub phase: Option<Phase>,
    #[serde(default)]
    pub permission_mode: Option<PermissionMode>,
    /// `--tools`. `None` omits the flag; `Some([])` passes `""` (no tools).
    #[serde(default)]
    pub tools: Option<Vec<String>>,
    #[serde(default)]
    pub allowed_tools: Vec<String>,
    #[serde(default)]
    pub disallowed_tools: Vec<String>,
    /// Whether this pane may start a subagent of its own.
    ///
    /// A fleet pane must not: a subagent's work never reaches the ledger or the
    /// grid, and deciding to split the work belongs to the orchestrator. A
    /// worker that needs more hands sends `QUESTION:` instead. `check` enforces
    /// this on every claude teammate the orchestrator can spawn, and on the
    /// orchestrator itself, by insisting on `disallowed_tools: [Agent]`.
    ///
    /// `true` waives the rule for one teammate. No shipped teammate sets it.
    #[serde(default)]
    pub allow_subagents: bool,
    /// `--setting-sources`. `None` omits the flag, so the operator's user,
    /// project and local settings apply as usual. `Some([])` renders
    /// `--setting-sources ""`, which loads none of them - no `enabledPlugins`,
    /// no `permissions.deny`. That is the lever for starting an agent clean.
    ///
    /// Beware the double negative: dropping user settings also drops
    /// `disableBundledSkills`, so bundled skills come back. Pair with
    /// `disable_skills` when the intent is genuinely "no skills".
    #[serde(default)]
    pub setting_sources: Option<Vec<String>>,
    /// `--disable-slash-commands`, which disables ALL skills - including ones a
    /// `plugin_dirs` entry brings in. Never set this alongside plugins.
    #[serde(default)]
    pub disable_skills: bool,
    /// Whether the skills the operator synced from claude.ai (listed as
    /// `anthropic-skills:<name>`) load. `false`, the default, puts
    /// `syncClaudeAiSkills: false` in the `--settings` overlay: they are hidden
    /// for this session only, and nothing on disk moves.
    #[serde(default)]
    pub inherit_claudeai_skills: bool,
    /// Further skills to switch off by name, as `skillOverrides` entries set to
    /// `"off"`. Settings merge per key, so the operator's own overrides stay.
    #[serde(default)]
    pub disabled_skills: Vec<String>,
    /// Claude plugins whose named skills this teammate is expected to use.
    /// The named skills are rendered, with descriptions, into the briefing;
    /// the plugin's other skills are switched off for the session. See
    /// `plugins.rs`.
    #[serde(default)]
    pub plugin_skills: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    pub settings: Option<String>,
    /// Inline MCP server definitions. `None` leaves the operator's MCP
    /// configuration alone; `Some` renders `--mcp-config <json>
    /// --strict-mcp-config`, so `Some({})` means exactly zero MCP servers.
    ///
    /// Keep secrets out: these files are committed. A server needing an API key
    /// belongs in `mcp_config_files`, pointing at a file the operator owns.
    #[serde(default)]
    pub mcp_servers: Option<BTreeMap<String, serde_json::Value>>,
    /// Extra `--mcp-config <path>` arguments, for servers defined outside this
    /// repo. Merged under the same `--strict-mcp-config` when `mcp_servers` is
    /// set.
    #[serde(default)]
    pub mcp_config_files: Vec<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// Whether this teammate's provider trains on what it is sent.
    ///
    /// True for the free tiers: the model costs nothing because the prompts are
    /// the payment. It appends the base's `trains_on_input` block to the
    /// briefing, so the constraint reaches the worker as an instruction rather
    /// than living only in a `brief_description` the worker never sees.
    #[serde(default)]
    pub trains_on_input: bool,
    #[serde(default)]
    pub first_instruction: Option<String>,
    /// The file body. Its meaning depends on `base`: a persona when `base` is
    /// set, the entire prompt when it is not.
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
            agent: Agent::Claude,
            model: None,
            effort: None,
            subagent_model: None,
            inherit_plugins: true,
            plugin_dirs: Vec::new(),
            skills: Vec::new(),
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
            settings: None,
            mcp_servers: None,
            mcp_config_files: Vec::new(),
            args: Vec::new(),
            env: BTreeMap::new(),
            trains_on_input: false,
            first_instruction: None,
            persona: String::new(),
        }
    }
}

fn yes() -> bool {
    true
}

/// One execpolicy rule from `_base/codex-execpolicy.md`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExecRule {
    pub pattern: String,
    pub justification: String,
}

/// A file in `_base/`: a shared prompt shell, or shared data like the
/// execpolicy rules. Not a teammate and never in the roster.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Base {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    /// Prepended instruction when a teammate declares `skills`. Prose lives
    /// here rather than in Rust; `{skills}` is the comma-joined list.
    #[serde(default)]
    pub skills_instruction: String,
    /// Appended when a teammate sets `trains_on_input`. One copy, here, rather
    /// than the same paragraph pasted into every free-tier teammate file.
    #[serde(default)]
    pub trains_on_input: String,
    /// Closing paragraph when the spawn carried a task.
    #[serde(default)]
    pub task_fresh: String,
    /// Closing paragraph when resuming an earlier session.
    #[serde(default)]
    pub task_resume: String,
    /// Closing paragraph when the worker starts idle.
    #[serde(default)]
    pub task_idle: String,
    #[serde(default)]
    pub rules: Vec<ExecRule>,
    #[serde(skip)]
    pub body: String,
}

/// Every teammate and base visible to this process.
#[derive(Debug, Clone, Default)]
pub struct Roster {
    teammates: BTreeMap<String, Teammate>,
    bases: BTreeMap<String, Base>,
    /// Directories that were overlaid, lowest precedence first. For `doctor`.
    pub sources: Vec<PathBuf>,
}

impl Roster {
    /// Compiled-in teammates only. Always succeeds, so a broken user directory
    /// can never leave the fleet with no roster at all.
    pub fn builtin() -> Result<Roster> {
        let mut r = Roster::default();
        for (name, text) in BUILTIN_BASES {
            r.bases.insert(
                name.to_string(),
                parse_base(name, text).with_context(|| format!("compiled-in base '{name}'"))?,
            );
        }
        for (name, text) in BUILTIN_TEAMMATES {
            r.teammates.insert(
                name.to_string(),
                parse_teammate(name, text)
                    .with_context(|| format!("compiled-in teammate '{name}'"))?,
            );
        }
        Ok(r)
    }

    /// Built-ins, overlaid by `~/.config/horch/teammates`, then by
    /// `$HORCH_TEAMMATES_DIR`.
    pub fn load() -> Result<Roster> {
        Roster::load_with(None)
    }

    /// As [`Roster::load`], with `explicit` as the highest-precedence overlay.
    ///
    /// A herdr pane is a fresh shell started by the server: it inherits the
    /// user's profile, not the environment `horch` was invoked with. So a pane
    /// cannot see `$HORCH_TEAMMATES_DIR` and has to be told the path outright,
    /// the same way `HORCH_CLAUDE_BIN` travels in the worker's brief.
    pub fn load_with(explicit: Option<&str>) -> Result<Roster> {
        let mut r = Roster::builtin()?;
        let mut dirs = overlay_dirs();
        if let Some(dir) = explicit.filter(|d| !d.is_empty()) {
            dirs.push(PathBuf::from(dir));
        }
        for dir in dirs {
            if dir.is_dir() {
                r.overlay(&dir)
                    .with_context(|| format!("loading teammates from {}", dir.display()))?;
                r.sources.push(dir);
            }
        }
        Ok(r)
    }

    /// Read one directory over the top of what is already loaded.
    pub fn overlay(&mut self, dir: &Path) -> Result<()> {
        let base_dir = dir.join("_base");
        if base_dir.is_dir() {
            for (stem, path) in md_files(&base_dir)? {
                let text = std::fs::read_to_string(&path)
                    .with_context(|| format!("reading {}", path.display()))?;
                let base =
                    parse_base(&stem, &text).with_context(|| format!("in {}", path.display()))?;
                self.bases.insert(stem, base);
            }
        }
        for (stem, path) in md_files(dir)? {
            if stem.starts_with('_') {
                continue;
            }
            let text = std::fs::read_to_string(&path)
                .with_context(|| format!("reading {}", path.display()))?;
            let t =
                parse_teammate(&stem, &text).with_context(|| format!("in {}", path.display()))?;
            self.teammates.insert(stem, t);
        }
        Ok(())
    }

    pub fn get(&self, name: &str) -> Option<&Teammate> {
        self.teammates.get(name)
    }

    /// Like [`Roster::get`], but with an error naming what is available.
    pub fn require(&self, name: &str) -> Result<&Teammate> {
        match self.teammates.get(name) {
            Some(t) => Ok(t),
            None => bail!(
                "unknown teammate '{name}'. Available: {}",
                self.names().join(", ")
            ),
        }
    }

    pub fn base(&self, name: &str) -> Option<&Base> {
        self.bases.get(name)
    }

    pub fn require_base(&self, name: &str) -> Result<&Base> {
        self.bases.get(name).with_context(|| {
            format!("teammate names base '{name}', which does not exist in _base/")
        })
    }

    /// All names, including hidden ones. Spawnable by name.
    pub fn names(&self) -> Vec<&str> {
        self.teammates.keys().map(|s| s.as_str()).collect()
    }

    /// What the orchestrator is offered: specialists first, then generics.
    /// Hidden teammates are excluded.
    pub fn offered(&self) -> Vec<&Teammate> {
        let mut v: Vec<&Teammate> = self.teammates.values().filter(|t| !t.hidden).collect();
        v.sort_by_key(|t| (t.generic, t.name.clone()));
        v
    }

    /// The `{roster}` substitution: `brief_description` lines and nothing else.
    pub fn roster_lines(&self) -> String {
        let offered = self.offered();
        let width = offered.iter().map(|t| t.name.len()).max().unwrap_or(0);
        offered
            .iter()
            .map(|t| {
                format!(
                    "  {:<width$}  {}",
                    t.name,
                    t.brief_description,
                    width = width
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The execpolicy rules a codex WORKER needs installed before launch.
    pub fn exec_rules(&self) -> &[ExecRule] {
        self.rules_of("codex-execpolicy")
    }

    /// The execpolicy rules a codex ORCHESTRATOR needs instead: it never runs
    /// `horch note` or `horch done`, and the worker set allows it nothing it
    /// needs to build a fleet with.
    pub fn orchestrator_exec_rules(&self) -> &[ExecRule] {
        self.rules_of("codex-orchestrator-execpolicy")
    }

    fn rules_of(&self, base: &str) -> &[ExecRule] {
        match self.bases.get(base) {
            Some(b) => &b.rules,
            None => &[],
        }
    }

    /// Whether this teammate may be started by `horch spawn`. Orchestrators
    /// are launched by `pane-launch`, never spawned, so this is where the
    /// one-top-tier-session rule bites.
    pub fn is_spawnable(t: &Teammate) -> Result<()> {
        Roster::model_is_spawnable(t.model.as_deref().unwrap_or_default(), &t.name)
    }

    /// The same rule, applied to the model a spawn is actually about to launch.
    ///
    /// `horch spawn --resume` takes its model from the ledger record rather than
    /// from the teammate file, so checking the file alone would leave a stale or
    /// hand-edited record able to start a second top-tier session behind an
    /// innocent-looking tier name.
    pub fn model_is_spawnable(model: &str, who: &str) -> Result<()> {
        if let Some((tier, instead)) = reserved_tier(model) {
            bail!(
                "'{who}' runs on {model}, and the {tier} tier is reserved for the \
                 orchestrator; the fleet has at most one top-tier session, whichever \
                 agent is orchestrating. Use {instead}."
            );
        }
        Ok(())
    }

    /// Everything wrong with the loaded roster, as human-readable lines.
    /// Empty means healthy.
    pub fn check(&self) -> Vec<String> {
        let mut problems = Vec::new();
        for t in self.teammates.values() {
            let who = &t.name;
            if let Err(error) = crate::skills::selected(t) {
                problems.push(format!("{error:#}"));
            }
            if t.brief_description.trim().is_empty() {
                problems.push(format!("{who}: brief_description is empty"));
            }
            if t.brief_description.len() > BRIEF_DESCRIPTION_MAX {
                problems.push(format!(
                    "{who}: brief_description is {} chars, max {BRIEF_DESCRIPTION_MAX} \
                     (it is carried in the orchestrator's context on every run)",
                    t.brief_description.len()
                ));
            }
            if let Some(base) = &t.base {
                if !self.bases.contains_key(base) {
                    problems.push(format!("{who}: base '{base}' does not exist in _base/"));
                }
            }
            // Anything the orchestrator can pick must be something it may spawn.
            if !t.hidden {
                if let Err(e) = Roster::is_spawnable(t) {
                    problems.push(format!("{e:#}"));
                }
            }
            if t.agent != Agent::None && t.model.is_none() && t.name != "orchestration-worker" {
                problems.push(format!("{who}: agent is {} but no model is set", t.agent));
            }
            if let Some(effort) = &t.effort {
                if let Some(why) = effort_problem(t.agent, t.model.as_deref(), effort) {
                    problems.push(format!("{who}: {why}"));
                }
            }
            // Reinforced plugin skills must exist, or the briefing promises a
            // skill the pane cannot load.
            if t.agent == Agent::Claude && !t.plugin_skills.is_empty() {
                if t.disable_skills {
                    problems.push(format!(
                        "{who}: plugin_skills cannot load with disable_skills: true"
                    ));
                }
                if let Err(e) = crate::plugins::resolve_all(t) {
                    problems.push(format!("{who}: {e:#}"));
                }
            }
            // A codex worker with no effort silently takes whatever the
            // operator's ~/.codex/config.toml says (the pane's private
            // CODEX_HOME links it). That made two workers run at "medium"
            // nobody chose, so every offered codex teammate states its own.
            if t.agent == Agent::Codex && !t.hidden && t.effort.is_none() {
                problems.push(format!(
                    "{who}: a codex teammate must set effort; without it the pane \
                     inherits model_reasoning_effort from ~/.codex/config.toml"
                ));
            }
            // Every claude-shaped field this agent cannot express. Naming the
            // field beats a generic "unsupported": the author set it on purpose.
            for field in t.agent.unsupported_fields(t) {
                problems.push(format!(
                    "{who}: '{field}' is not something {} can express; \
                     use args, or move this work to a claude teammate",
                    t.agent
                ));
            }
            // A fleet pane must not spawn subagents. The prose rule in
            // `_base/fleet-worker.md` was not enough on its own, so the switch
            // carries it: `--disallowedTools Agent` takes the tool out of the
            // session's tool set on claude 2.1.278. The hidden orchestration-*
            // teammates run a fixed recipe, not fleet panes, so only a
            // spawnable teammate and the orchestrator itself are covered.
            // Codex, OpenCode, pi and Prime are elsewhere; see
            // `ai_docs/reports/no-subagents.md`.
            if t.agent == Agent::Claude
                && (!t.hidden || t.name == "orchestrator")
                && !t.allow_subagents
                && !t.disallowed_tools.iter().any(|x| x == "Agent")
            {
                problems.push(format!(
                    "{who}: a fleet pane must not spawn subagents; add \
                     disallowed_tools: [Agent] or set allow_subagents: true"
                ));
            }
            if let Some(mode) = t.permission_mode {
                let ok = match t.agent {
                    Agent::Codex => mode.codex_args().is_some(),
                    Agent::Opencode => mode.opencode_args().is_some(),
                    // pi and Prime run their tools without asking, so there is
                    // no gate for a mode to set. Saying nothing is correct;
                    // saying `acceptEdits` implies a restraint that is absent.
                    Agent::Pi | Agent::Prime => false,
                    Agent::Claude | Agent::None => true,
                };
                if !ok {
                    problems.push(format!(
                        "{who}: permission_mode '{}' has no {} equivalent",
                        mode.as_str(),
                        t.agent
                    ));
                }
            }
            // A plan-mode worker that cannot leave plan mode stalls in a pane
            // nobody is watching. Denies normally beat allows, so naming the
            // tool in allowed_tools is not accepted as proof on its own.
            if t.permission_mode == Some(PermissionMode::Plan) && t.agent == Agent::Claude {
                // `--setting-sources ""` loads no settings file, so a
                // `permissions.deny` written for an attended session cannot
                // reach this worker.
                let escapes_settings = t
                    .setting_sources
                    .as_ref()
                    .map(|v| v.is_empty())
                    .unwrap_or(false)
                    || t.settings.is_some();
                let tool_available = t
                    .tools
                    .as_ref()
                    .map(|v| v.iter().any(|x| x == "ExitPlanMode" || x == "default"))
                    .unwrap_or(true);
                if !escapes_settings {
                    problems.push(format!(
                        "{who}: permission_mode is 'plan' but it inherits the operator's \
                         settings, which may deny ExitPlanMode - set \
                         `setting_sources: []` or point `settings:` at a file written \
                         for workers"
                    ));
                }
                if !tool_available {
                    problems.push(format!(
                        "{who}: permission_mode is 'plan' but 'tools' does not include \
                         ExitPlanMode, so it can never leave plan mode"
                    ));
                }
            }
            // Dropping the operator's settings also drops `disableBundledSkills`,
            // so "clean" can silently mean "more skills than before".
            if t.setting_sources
                .as_ref()
                .map(|v| v.is_empty())
                .unwrap_or(false)
                && !t.disable_skills
                && t.plugin_dirs.is_empty()
                && t.skills.is_empty()
                && t.phase.is_none()
            {
                problems.push(format!(
                    "{who}: setting_sources is empty but disable_skills is false and no \
                     plugin_dirs are set - bundled skills the operator disabled in settings \
                     will load. Set disable_skills: true, or name the plugins you want."
                ));
            }
            if t.disable_skills && !t.plugin_dirs.is_empty() {
                problems.push(format!(
                    "{who}: disable_skills turns off ALL skills, including the ones \
                     plugin_dirs loads - the plugins would be dead weight"
                ));
            }
            if t.disable_skills && !t.disabled_skills.is_empty() {
                problems.push(format!(
                    "{who}: disable_skills already turns off ALL skills, so the \
                     disabled_skills list does nothing"
                ));
            }
            for name in &t.disabled_skills {
                if name.trim().is_empty() {
                    problems.push(format!("{who}: disabled_skills has a blank entry"));
                } else if t
                    .skills
                    .iter()
                    .any(|s| *name == *s || *name == format!("horch:{s}"))
                {
                    problems.push(format!(
                        "{who}: '{name}' is both in skills and in disabled_skills"
                    ));
                }
            }
            if t.trains_on_input {
                let renders = t
                    .base
                    .as_ref()
                    .and_then(|b| self.bases.get(b))
                    .map(|b| !b.trains_on_input.trim().is_empty())
                    .unwrap_or(false);
                if !renders {
                    problems.push(format!(
                        "{who}: trains_on_input is set but its base has no \
                         trains_on_input block to render, so the worker would never \
                         be told"
                    ));
                }
            }
            if (!t.skills.is_empty() || t.phase.is_some()) && t.disable_skills {
                problems.push(format!(
                    "{who}: declares skills or phase but also disable_skills"
                ));
            }
            // `args` is emitted immediately before the prompt. These flags are
            // variadic and would take the prompt as one more value.
            if let Some(last) = t.args.last() {
                if [
                    "--mcp-config",
                    "--tools",
                    "--allowedTools",
                    "--allowed-tools",
                    "--disallowedTools",
                    "--disallowed-tools",
                ]
                .contains(&last.as_str())
                {
                    problems.push(format!(
                        "{who}: args ends with '{last}', a variadic flag that would swallow \
                         the prompt; put it earlier or use the dedicated field"
                    ));
                }
            }
            for dir in &t.plugin_dirs {
                if !expand_home(dir).is_dir() {
                    problems.push(format!("{who}: plugin_dir '{dir}' does not exist"));
                }
            }
            // A teammate that brings its own settings file takes over the status
            // line too; one without a statusLine would be the only pane in the
            // fleet with none.
            if let Some(path) = &t.settings {
                let path = expand_home(path);
                match std::fs::read_to_string(&path)
                    .ok()
                    .and_then(|x| serde_json::from_str::<serde_json::Value>(&x).ok())
                {
                    None => problems.push(format!(
                        "{who}: settings file '{}' is missing or not valid JSON",
                        path.display()
                    )),
                    Some(v) if v.get("statusLine").is_none() => problems.push(format!(
                        "{who}: settings file '{}' defines no statusLine, so this pane \
                         would be the only one in the fleet without one",
                        path.display()
                    )),
                    Some(_) => {}
                }
            }
            for file in &t.mcp_config_files {
                if !expand_home(file).is_file() {
                    problems.push(format!(
                        "{who}: mcp_config_files entry '{file}' does not exist"
                    ));
                }
            }
        }
        // Every command an agent is told to run must be allowed for codex, and
        // nothing more: a rule with no command behind it is standing permission
        // nobody asked for.
        if let Some(worker) = self.bases.get("fleet-worker") {
            let told = format!("{}{}", worker.body, worker.task_idle);
            problems.extend(unused_rules(
                self.exec_rules(),
                &told,
                "worker",
                "fleet-worker",
            ));
        }
        if let Some(orchestrator) = self.bases.get("fleet-orchestrator") {
            problems.extend(unused_rules(
                self.orchestrator_exec_rules(),
                &orchestrator.body,
                "an orchestrator",
                "fleet-orchestrator",
            ));
        }
        problems
    }
}

/// The operator's `statusLine` block, read from `~/.claude/settings.json`.
///
/// Every agent in a fleet - orchestrator and workers alike - shows the same
/// status line as an ordinary session. That is not decoration: a pane with no
/// status line gives no model, no context usage and no session cost, and a
/// fleet is exactly where that information matters most.
///
/// It has to be re-injected explicitly because `--setting-sources ""`, which is
/// how a teammate sheds the operator's globally-enabled plugins, sheds their
/// `statusLine` with everything else.
pub fn operator_status_line() -> Option<serde_json::Value> {
    operator_settings()?.get("statusLine").cloned()
}

/// Names of the plugins the operator has enabled globally, from
/// `enabledPlugins` in `~/.claude/settings.json`.
pub fn operator_enabled_plugins() -> Vec<String> {
    let Some(settings) = operator_settings() else {
        return Vec::new();
    };
    match settings.get("enabledPlugins").and_then(|v| v.as_object()) {
        Some(map) => map
            .iter()
            .filter(|(_, on)| on.as_bool() == Some(true))
            .map(|(name, _)| name.clone())
            .collect(),
        None => Vec::new(),
    }
}

/// Execpolicy rules allowing a command the briefing never asks for.
fn unused_rules(rules: &[ExecRule], told: &str, who: &str, base: &str) -> Vec<String> {
    rules
        .iter()
        .filter_map(|rule| {
            let joined = rule
                .pattern
                .split(',')
                .map(|p| p.trim().trim_matches('"'))
                .collect::<Vec<_>>()
                .join(" ");
            (!told.contains(&joined)).then(|| {
                format!(
                    "codex execpolicy allows '{joined}', which _base/{base}.md never \
                     tells {who} to run"
                )
            })
        })
        .collect()
}

/// Operator settings that silently override a teammate's `effort`, as
/// warning lines for `horch doctor`. Empty when nothing overrides.
pub fn operator_effort_warnings() -> Vec<String> {
    let codex_config = std::fs::read_to_string(
        crate::codex::codex_home(&crate::agent::home_dir()).join("config.toml"),
    )
    .ok();
    effort_override_warnings(
        std::env::var("CLAUDE_CODE_EFFORT_LEVEL").ok().as_deref(),
        operator_settings().as_ref(),
        codex_config.as_deref(),
    )
}

/// The pure half of [`operator_effort_warnings`].
///
/// Claude Code resolves effort as: env `CLAUDE_CODE_EFFORT_LEVEL` > `--effort`
/// > settings > model default (cezaar#41). horch passes `--effort`, so only
/// the env var - set in the shell, or in the `env` block of
/// ~/.claude/settings.json, which Claude Code exports into its own process -
/// beats it, and it beats it for every pane at once. `maxEffortLevel` caps
/// every level above it. A codex pane with no effort takes
/// `model_reasoning_effort` from the operator's config.toml.
pub fn effort_override_warnings(
    env_level: Option<&str>,
    settings: Option<&serde_json::Value>,
    codex_config: Option<&str>,
) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(level) = env_level.filter(|l| !l.is_empty()) {
        out.push(format!(
            "CLAUDE_CODE_EFFORT_LEVEL={level} is set in this shell: it beats --effort, so \
             every claude pane runs at {level} whatever its teammate file says"
        ));
    }
    if let Some(settings) = settings {
        if let Some(level) = settings
            .pointer("/env/CLAUDE_CODE_EFFORT_LEVEL")
            .and_then(|v| v.as_str())
        {
            out.push(format!(
                "~/.claude/settings.json sets env.CLAUDE_CODE_EFFORT_LEVEL={level}: it beats \
                 --effort, so every claude pane runs at {level}"
            ));
        }
        if let Some(cap) = settings.get("maxEffortLevel").and_then(|v| v.as_str()) {
            out.push(format!(
                "~/.claude/settings.json caps effort at maxEffortLevel={cap}; teammates set \
                 above it run at {cap}"
            ));
        }
    }
    if let Some(level) = codex_config.and_then(codex_default_effort) {
        out.push(format!(
            "~/.codex/config.toml sets model_reasoning_effort=\"{level}\": any codex pane \
             with no effort (the orchestration recipe's) runs at {level}"
        ));
    }
    out
}

/// The top-level `model_reasoning_effort` in a codex config.toml, if any.
/// A line scan rather than a TOML parser: only the root table counts, so the
/// scan stops at the first `[section]`.
fn codex_default_effort(toml: &str) -> Option<String> {
    for line in toml.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            return None;
        }
        if let Some((key, value)) = line.split_once('=') {
            if key.trim() == "model_reasoning_effort" {
                let value = value.split('#').next().unwrap_or_default().trim();
                return Some(value.trim_matches(|c| c == '"' || c == '\'').to_string());
            }
        }
    }
    None
}

fn operator_settings() -> Option<serde_json::Value> {
    let home = std::env::var_os("HOME")?;
    let path = PathBuf::from(home).join(".claude/settings.json");
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

/// Expand a leading `~/` against `$HOME`.
///
/// Teammate files are committed, so a plugin path written as an absolute
/// `/Users/<someone>/...` only works on one machine. `~` keeps them portable
/// without inventing a template syntax for paths.
pub fn expand_home(path: &str) -> PathBuf {
    match path.strip_prefix("~/") {
        Some(rest) => match std::env::var_os("HOME") {
            Some(home) => PathBuf::from(home).join(rest),
            None => PathBuf::from(path),
        },
        None => PathBuf::from(path),
    }
}

/// Where a runtime directory may be found, lowest precedence first.
fn overlay_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(home) = std::env::var_os("HOME") {
        dirs.push(PathBuf::from(home).join(".config/horch/teammates"));
    }
    if let Some(dir) = std::env::var_os("HORCH_TEAMMATES_DIR") {
        if !dir.is_empty() {
            dirs.push(PathBuf::from(dir));
        }
    }
    dirs
}

/// `*.md` in one directory as `(stem, path)`, sorted, skipping READMEs.
fn md_files(dir: &Path) -> Result<Vec<(String, PathBuf)>> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir)
        .with_context(|| format!("reading {}", dir.display()))?
        .flatten()
    {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let stem = path.file_stem().unwrap().to_string_lossy().to_string();
        if stem == "README" {
            continue;
        }
        out.push((stem, path));
    }
    out.sort();
    Ok(out)
}

/// Split `---\n<frontmatter>\n---\n<body>`.
fn split_frontmatter(text: &str) -> Result<(&str, &str)> {
    let rest = text
        .strip_prefix("---\n")
        .context("file does not start with a '---' frontmatter fence")?;
    let end = rest
        .find("\n---\n")
        .context("frontmatter is not closed by a '---' line")?;
    Ok((&rest[..end], &rest[end + 5..]))
}

#[cfg(test)]
mod spawnable_tests {
    use super::*;

    #[test]
    fn phases_round_trip_and_legacy_teammates_have_no_phase() {
        for (name, phase) in [
            ("research", Phase::Research),
            ("plan", Phase::Plan),
            ("implementation", Phase::Implementation),
            ("validation", Phase::Validation),
        ] {
            assert_eq!(name.parse::<Phase>().unwrap(), phase);
            assert_eq!(phase.to_string(), name);
            assert_eq!(
                serde_json::to_string(&phase).unwrap(),
                format!("\"{name}\"")
            );
        }
        assert!("unknown".parse::<Phase>().is_err());
        let t = parse_teammate(
            "legacy",
            "---\nname: legacy\nbrief_description: Legacy\nagent: codex\n---\nbody",
        )
        .unwrap();
        assert_eq!(t.phase, None);
    }

    #[test]
    fn builtin_phase_defaults_are_portable_and_disabling_conflicts() {
        let mut r = Roster::builtin().unwrap();
        for t in r.teammates.values() {
            assert!(
                t.plugin_dirs.is_empty(),
                "{} has machine-local plugins",
                t.name
            );
            let expected = match t.name.as_str() {
                "smoke" => None,
                "researcher" | "product-lead" | "designer" => Some(Phase::Research),
                "staff-engineer"
                | "orchestrator"
                | "orchestrator-codex"
                | "orchestration-orchestrator" => Some(Phase::Plan),
                "architect-reviewer" | "qa-engineer" | "codex-reviewer" => {
                    Some(Phase::Validation)
                }
                _ => Some(Phase::Implementation),
            };
            assert_eq!(t.phase, expected, "{}", t.name);
        }
        assert!(r.check().is_empty(), "{:?}", r.check());
        r.teammates.get_mut("opus").unwrap().disable_skills = true;
        assert!(r
            .check()
            .iter()
            .any(|p| p.contains("opus: declares skills or phase")));
    }

    /// Every agent names its effort levels differently, and a level the CLI
    /// does not take fails at `--check`, not in a pane nobody is watching.
    #[test]
    fn effort_is_checked_against_each_agents_own_levels() {
        // Valid on its own agent.
        for (agent, model, effort) in [
            (Agent::Claude, "opus", "medium"),
            (Agent::Claude, "sonnet", "max"),
            (Agent::Codex, "gpt-5.6-sol", "none"),
            (Agent::Codex, "gpt-6-astra", "xhigh"),
            (Agent::Opencode, "anthropic/claude-sonnet-5", "high"),
            (Agent::Pi, "ollama/qwen3.8", "off"),
            (Agent::Prime, "anthropic/claude-opus-5-5", "minimal"),
        ] {
            assert_eq!(effort_problem(agent, Some(model), effort), None, "{agent} {effort}");
        }
        // Refused, each with a reason that names the fix.
        for (agent, model, effort, says) in [
            (Agent::Claude, "opus", "minimal", "expected one of: low, medium"),
            (Agent::Claude, "haiku", "low", "no effort setting"),
            (Agent::Claude, "claude-haiku-4-5-20251001", "low", "no effort setting"),
            (Agent::Codex, "gpt-5.6-sol", "minimal", "API error"),
            (Agent::Codex, "gpt-5.6-sol", "ultra", "multiplies spend"),
            (Agent::Codex, "gpt-6-astra", "none", "use low"),
            (Agent::Opencode, "opencode/big-pickle", "high", "no effort setting"),
            (Agent::Opencode, "opencode/nemotron-3-ultra-free", "high", "no effort setting"),
            (Agent::Pi, "ollama/qwen3.8", "ultra", "not a pi level"),
            (Agent::Prime, "anthropic/claude-opus-5-5", "none", "not a prime level"),
        ] {
            let why = effort_problem(agent, Some(model), effort)
                .unwrap_or_else(|| panic!("{agent} {model} {effort} should be refused"));
            assert!(why.contains(says), "{agent} {model} {effort}: {why}");
        }
    }

    #[test]
    fn roster_check_names_a_bad_effort_and_an_unstated_codex_effort() {
        let mut r = Roster::builtin().unwrap();
        assert!(r.check().is_empty(), "{:?}", r.check());

        r.teammates.get_mut("opus").unwrap().effort = Some("extreme".into());
        assert!(
            r.check()
                .iter()
                .any(|p| p.starts_with("opus: effort 'extreme' is not a claude level")),
            "{:?}",
            r.check()
        );
        r.teammates.get_mut("opus").unwrap().effort = Some("medium".into());

        r.teammates.get_mut("sonnet").unwrap().model = Some("haiku".into());
        assert!(
            r.check()
                .iter()
                .any(|p| p.starts_with("sonnet: haiku has no effort setting")),
            "{:?}",
            r.check()
        );
        r.teammates.get_mut("sonnet").unwrap().model = Some("sonnet".into());

        r.teammates.get_mut("codex-sol").unwrap().effort = None;
        assert!(
            r.check()
                .iter()
                .any(|p| p.starts_with("codex-sol: a codex teammate must set effort")),
            "{:?}",
            r.check()
        );
    }

    #[test]
    fn doctor_warns_about_every_effort_override_and_is_silent_without_one() {
        assert!(effort_override_warnings(None, None, None).is_empty());
        assert!(effort_override_warnings(
            Some(""),
            Some(&serde_json::json!({"effortLevel": "high"})),
            Some("model = \"gpt-5.6-sol\"\n[profiles.x]\nmodel_reasoning_effort = \"low\"\n"),
        )
        .is_empty(), "effortLevel loses to --effort, and a profile's key is not the default");

        let w = effort_override_warnings(
            Some("low"),
            Some(&serde_json::json!({
                "env": {"CLAUDE_CODE_EFFORT_LEVEL": "medium"},
                "maxEffortLevel": "high"
            })),
            Some("# comment\nmodel_reasoning_effort = \"medium\" # why\n[tui]\n"),
        );
        assert_eq!(w.len(), 4, "{w:?}");
        assert!(w[0].starts_with("CLAUDE_CODE_EFFORT_LEVEL=low"), "{w:?}");
        assert!(w[1].contains("env.CLAUDE_CODE_EFFORT_LEVEL=medium"), "{w:?}");
        assert!(w[2].contains("maxEffortLevel=high"), "{w:?}");
        assert!(w[3].contains("model_reasoning_effort=\"medium\""), "{w:?}");
    }

    #[test]
    fn roster_check_rejects_plugin_skills_it_cannot_honour() {
        let mut r = Roster::builtin().unwrap();
        r.teammates
            .get_mut("opus")
            .unwrap()
            .plugin_skills
            .insert("no-such-plugin-anywhere".into(), vec!["x".into()]);
        assert!(
            r.check()
                .iter()
                .any(|p| p.starts_with("opus: plugin 'no-such-plugin-anywhere' is neither")),
            "{:?}",
            r.check()
        );
        r.teammates
            .get_mut("codex-sol")
            .unwrap()
            .plugin_skills
            .insert("code".into(), vec!["review".into()]);
        assert!(
            r.check()
                .iter()
                .any(|p| p.starts_with("codex-sol: 'plugin_skills' is not something codex")),
            "{:?}",
            r.check()
        );
    }

    /// A fleet pane must not spawn subagents. The deny is the proof, and
    /// `allow_subagents` is the only way out.
    #[test]
    fn roster_check_demands_the_subagent_deny_on_every_claude_fleet_pane() {
        const MESSAGE: &str = "a fleet pane must not spawn subagents";

        // The shipped roster already carries it, on the 10 spawnable claude
        // teammates and on the orchestrator.
        let mut r = Roster::builtin().unwrap();
        assert!(r.check().is_empty(), "{:?}", r.check());
        let covered = r
            .teammates
            .values()
            .filter(|t| t.agent == Agent::Claude && (!t.hidden || t.name == "orchestrator"))
            .count();
        assert_eq!(covered, 11, "the rule should cover 11 claude teammates");

        // Take the deny away from a worker and the check fails by name.
        r.teammates.get_mut("opus").unwrap().disallowed_tools = Vec::new();
        assert!(
            r.check()
                .iter()
                .any(|p| p.starts_with("opus: ") && p.contains(MESSAGE)),
            "{:?}",
            r.check()
        );

        // `allow_subagents` waives it without restoring the deny.
        r.teammates.get_mut("opus").unwrap().allow_subagents = true;
        assert!(
            !r.check().iter().any(|p| p.contains(MESSAGE)),
            "{:?}",
            r.check()
        );

        // The deny itself passes, with or without other denied tools.
        let opus = r.teammates.get_mut("opus").unwrap();
        opus.allow_subagents = false;
        opus.disallowed_tools = vec!["Agent".into(), "Write".into()];
        assert!(
            !r.check().iter().any(|p| p.contains(MESSAGE)),
            "{:?}",
            r.check()
        );

        // A codex teammate is not affected: it cannot express the field at all,
        // and its switch is `-c features.multi_agent=false` in `args`.
        let sol = r.teammates.get_mut("codex-sol").unwrap();
        assert!(sol.disallowed_tools.is_empty());
        assert!(!sol.allow_subagents);
        assert!(
            !r.check().iter().any(|p| p.contains(MESSAGE)),
            "{:?}",
            r.check()
        );

        // Nor are the hidden orchestration-* panes, which run a fixed recipe.
        let worker = r.teammates.get_mut("orchestration-worker").unwrap();
        assert_eq!(worker.agent, Agent::Claude);
        assert!(worker.hidden);
        assert!(worker.disallowed_tools.is_empty());
        assert!(
            !r.check().iter().any(|p| p.contains(MESSAGE)),
            "{:?}",
            r.check()
        );

        // The orchestrator is hidden but is still a fleet pane.
        r.teammates
            .get_mut("orchestrator")
            .unwrap()
            .disallowed_tools = Vec::new();
        assert!(
            r.check()
                .iter()
                .any(|p| p.starts_with("orchestrator: ") && p.contains(MESSAGE)),
            "{:?}",
            r.check()
        );
    }

    /// `allow_subagents` is off unless the frontmatter says otherwise.
    #[test]
    fn allow_subagents_parses_from_frontmatter_and_defaults_to_false() {
        let head = "---\nname: x\nbrief_description: X\nagent: claude\nmodel: opus\n";
        let t = parse_teammate("x", &format!("{head}---\nbody")).unwrap();
        assert!(!t.allow_subagents);
        let t = parse_teammate("x", &format!("{head}allow_subagents: true\n---\nbody")).unwrap();
        assert!(t.allow_subagents);
    }

    #[test]
    fn roster_check_rejects_unknown_bundle_selection() {
        let mut r = Roster::builtin().unwrap();
        r.teammates.get_mut("opus").unwrap().skills = vec!["missing-bundle".into()];
        assert!(r
            .check()
            .iter()
            .any(|p| p.contains("unknown bundled skill 'missing-bundle'")));
    }

    /// A skill switched off that the same file also asks for is a
    /// contradiction, and a blank name switches off nothing.
    #[test]
    fn roster_check_rejects_contradictory_disabled_skills() {
        let mut r = Roster::builtin().unwrap();
        let opus = r.teammates.get_mut("opus").unwrap();
        opus.skills = vec!["tdd".into()];
        opus.disabled_skills = vec!["horch:tdd".into(), " ".into()];
        let problems = r.check();
        assert!(
            problems
                .iter()
                .any(|p| p.contains("'horch:tdd' is both in skills and in disabled_skills")),
            "{problems:?}"
        );
        assert!(
            problems
                .iter()
                .any(|p| p.contains("disabled_skills has a blank entry")),
            "{problems:?}"
        );

        let mut t = r.require("codex-sol").unwrap().clone();
        t.disabled_skills = vec!["pdf".into()];
        t.inherit_claudeai_skills = true;
        let fields = t.agent.unsupported_fields(&t);
        assert!(fields.contains(&"disabled_skills"), "{fields:?}");
        assert!(fields.contains(&"inherit_claudeai_skills"), "{fields:?}");
    }

    #[test]
    fn codex_can_use_integrated_skills() {
        let mut t = Roster::builtin()
            .unwrap()
            .require("codex-sol")
            .unwrap()
            .clone();
        t.skills = vec!["tdd".into()];
        assert!(!t.agent.unsupported_fields(&t).contains(&"skills"));
    }

    /// The fleet has exactly one top-tier session: the orchestrator. Even a
    /// file that asks for one is refused at spawn, so a stray teammate cannot
    /// make a second - and the rule does not care which flavor is orchestrating,
    /// so a Fable cannot start an Astra either.
    #[test]
    fn a_top_tier_worker_is_refused_at_spawn_whichever_tier_it_names() {
        let r = Roster::builtin().unwrap();
        for (model, instead) in [
            ("fable", "opus"),
            ("claude-fable-5-1", "opus"),
            ("gpt-6-astra", "codex-sol"),
            ("GPT-7-Astra", "codex-sol"),
        ] {
            let mut t = r.require("opus").unwrap().clone();
            t.model = Some(model.into());
            let err = Roster::is_spawnable(&t).unwrap_err().to_string();
            assert!(
                err.contains("reserved for the orchestrator"),
                "{model}: {err}"
            );
            assert!(
                err.contains(instead),
                "{model} should point at {instead}: {err}"
            );
        }
        for ok in ["opus", "sonnet", "gpt-5.6-sol", "gpt-5.6-terra"] {
            assert!(
                Roster::model_is_spawnable(ok, "t").is_ok(),
                "{ok} must stay spawnable"
            );
        }
    }

    /// The ledger, not the teammate file, decides what `--resume` launches, so
    /// the rule has to hold on a bare model string too. `horch spawn` applies it
    /// to the resolved model as its last gate before the pane starts.
    #[test]
    fn the_rule_holds_on_a_bare_model_string_from_a_ledger_record() {
        let err = Roster::model_is_spawnable("gpt-6-astra", "opus-3")
            .unwrap_err()
            .to_string();
        assert!(err.contains("opus-3"), "{err}");
        assert!(err.contains("astra"), "{err}");
    }

    /// And nothing the orchestrator is offered runs on a reserved tier.
    #[test]
    fn nothing_offered_runs_on_a_reserved_tier() {
        let r = Roster::builtin().unwrap();
        for t in r.offered() {
            assert!(
                reserved_tier(t.model.as_deref().unwrap_or_default()).is_none(),
                "{} is offered but runs on a reserved tier",
                t.name
            );
        }
        // The orchestrators themselves still do, and are hidden.
        for (name, model) in [
            ("orchestrator", "fable"),
            ("orchestrator-codex", "gpt-6-astra"),
        ] {
            let t = r.require(name).unwrap();
            assert_eq!(t.model.as_deref(), Some(model));
            assert!(t.hidden, "{name} must never appear in the roster");
            assert!(
                Roster::is_spawnable(t).is_err(),
                "{name} must not be spawnable"
            );
        }
    }
}

fn parse_teammate(stem: &str, text: &str) -> Result<Teammate> {
    let (front, body) = split_frontmatter(text)?;
    let mut t: Teammate = serde_yaml::from_str(front).context("parsing frontmatter")?;
    if t.name != stem {
        bail!(
            "name is '{}' but the filename says '{stem}'; the filename is the id",
            t.name
        );
    }
    t.persona = body.trim_end_matches('\n').to_string();
    Ok(t)
}

fn parse_base(stem: &str, text: &str) -> Result<Base> {
    let (front, body) = split_frontmatter(text)?;
    let mut b: Base = serde_yaml::from_str(front).context("parsing frontmatter")?;
    if b.name != stem {
        bail!("name is '{}' but the filename says '{stem}'", b.name);
    }
    b.body = body.trim_end_matches('\n').to_string();
    Ok(b)
}
