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
/// A fleet has exactly one top-tier session: the orchestrator, running either
/// Claude on Fable or Codex on Astra. `horch spawn` refuses to start a worker on
/// EITHER tier, whichever flavor is orchestrating - so a Fable orchestrator
/// cannot start an Astra, an Astra cannot start a Fable, and neither can clone
/// itself. The orchestrator writes every brief for an Opus/Codex-Sol reader
/// instead.
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

/// Longest a `brief_description` may be. Every non-hidden teammate's
/// description is concatenated into the orchestrator's briefing on every run,
/// so this is a direct, permanent tax on the orchestrator's context.
pub const BRIEF_DESCRIPTION_MAX: usize = 120;

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
        // Codex is the only one of these with no notion of a skill to load, and
        // no way to switch the operator's extensions off for one session: its
        // capability comes from the sandbox and the execpolicy instead.
        if self == Agent::Codex {
            if !t.skills.is_empty() {
                out.push("skills");
            }
            if !t.inherit_plugins {
                out.push("inherit_plugins");
            }
        }
        // Claude-shaped configuration with no counterpart anywhere else.
        for (field, set) in [
            ("plugin_dirs", !t.plugin_dirs.is_empty()),
            ("settings", t.settings.is_some()),
            ("subagent_model", t.subagent_model.is_some()),
            ("setting_sources", t.setting_sources.is_some()),
            ("disable_skills", t.disable_skills),
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
    #[serde(default)]
    pub permission_mode: Option<PermissionMode>,
    /// `--tools`. `None` omits the flag; `Some([])` passes `""` (no tools).
    #[serde(default)]
    pub tools: Option<Vec<String>>,
    #[serde(default)]
    pub allowed_tools: Vec<String>,
    #[serde(default)]
    pub disallowed_tools: Vec<String>,
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
            permission_mode: None,
            tools: None,
            allowed_tools: Vec::new(),
            disallowed_tools: Vec::new(),
            setting_sources: None,
            disable_skills: false,
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
            r.bases
                .insert(name.to_string(), parse_base(name, text).with_context(|| {
                    format!("compiled-in base '{name}'")
                })?);
        }
        for (name, text) in BUILTIN_TEAMMATES {
            r.teammates
                .insert(name.to_string(), parse_teammate(name, text).with_context(|| {
                    format!("compiled-in teammate '{name}'")
                })?);
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
                let base = parse_base(&stem, &text)
                    .with_context(|| format!("in {}", path.display()))?;
                self.bases.insert(stem, base);
            }
        }
        for (stem, path) in md_files(dir)? {
            if stem.starts_with('_') {
                continue;
            }
            let text = std::fs::read_to_string(&path)
                .with_context(|| format!("reading {}", path.display()))?;
            let t = parse_teammate(&stem, &text)
                .with_context(|| format!("in {}", path.display()))?;
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
        self.bases
            .get(name)
            .with_context(|| format!("teammate names base '{name}', which does not exist in _base/"))
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
            .map(|t| format!("  {:<width$}  {}", t.name, t.brief_description, width = width))
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
                 orchestrator; the fleet has exactly one top-tier session, whichever \
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
            // Every claude-shaped field this agent cannot express. Naming the
            // field beats a generic "unsupported": the author set it on purpose.
            for field in t.agent.unsupported_fields(t) {
                problems.push(format!(
                    "{who}: '{field}' is not something {} can express; \
                     use args, or move this work to a claude teammate",
                    t.agent
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
            if t.setting_sources.as_ref().map(|v| v.is_empty()).unwrap_or(false)
                && !t.disable_skills
                && t.plugin_dirs.is_empty()
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
            if !t.skills.is_empty() && t.disable_skills {
                problems.push(format!("{who}: declares skills but also disable_skills"));
            }
            // `args` is emitted immediately before the prompt. These flags are
            // variadic and would take the prompt as one more value.
            if let Some(last) = t.args.last() {
                if ["--mcp-config", "--tools", "--allowedTools", "--allowed-tools",
                    "--disallowedTools", "--disallowed-tools"]
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
                    problems.push(format!("{who}: mcp_config_files entry '{file}' does not exist"));
                }
            }
        }
        // Every command an agent is told to run must be allowed for codex, and
        // nothing more: a rule with no command behind it is standing permission
        // nobody asked for.
        if let Some(worker) = self.bases.get("fleet-worker") {
            let told = format!("{}{}", worker.body, worker.task_idle);
            problems.extend(unused_rules(self.exec_rules(), &told, "worker", "fleet-worker"));
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
            assert!(err.contains("reserved for the orchestrator"), "{model}: {err}");
            assert!(err.contains(instead), "{model} should point at {instead}: {err}");
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
        for (name, model) in [("orchestrator", "fable"), ("orchestrator-codex", "gpt-6-astra")] {
            let t = r.require(name).unwrap();
            assert_eq!(t.model.as_deref(), Some(model));
            assert!(t.hidden, "{name} must never appear in the roster");
            assert!(Roster::is_spawnable(t).is_err(), "{name} must not be spawnable");
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
