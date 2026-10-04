//! The frontmatter types of `teammates/*.md` and `_base/*.md`, and the
//! constants that decide which tier and which capabilities stay with the
//! orchestrator.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::{PermissionMode, Phase};
use crate::harness::HarnessKind;

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
pub(crate) const ORCHESTRATOR_TIERS: [(&str, &str); 2] =
    [("fable", "opus"), ("astra", "codex-sol")];

/// The teammate files `horch fleet` launches as the orchestrator.
pub(crate) const FLEET_ORCHESTRATORS: [&str; 2] = ["orchestrator", "orchestrator-codex"];

/// Bundled skills only a fleet orchestrator may carry. `check` fails any other
/// teammate that names one in `skills:`, and none is in a phase catalog.
pub(crate) const ORCHESTRATOR_ONLY_SKILLS: [&str; 2] = ["orchestrate", "skill-creator"];

/// Tools the Claude orchestrator must deny: `Agent` starts a subagent, in the
/// foreground or the background, and `RemoteTrigger` starts a cloud agent.
/// Workers are the orchestrator's only way to delegate.
pub(crate) const ORCHESTRATOR_DENIED_TOOLS: [&str; 2] = ["Agent", "RemoteTrigger"];

/// Teammates that run only as a headless `claude -p` job the dataset
/// coordinator starts, never in a fleet pane. `horch spawn` refuses them: the
/// judge must see the anonymous bundle and nothing else (JDG-01, SEC-04).
pub(crate) const HEADLESS_ONLY: [&str; 1] = ["judge"];

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
pub(crate) const BRIEF_DESCRIPTION_MAX: usize = 120;

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
    pub agent: HarnessKind,
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
    /// Catalog skills offered by name only: materialized with the launch, and
    /// named under "Also available" in the briefing without a description.
    /// For a related skill that is worth one name, not a full description.
    #[serde(default)]
    pub available_skills: Vec<String>,
    /// Skills copied at launch from a directory on the operator's machine,
    /// such as the ones `xcrun agent skills export` writes. They are never
    /// compiled in. Expected, like `skills:`. See `OperatorSkills`.
    #[serde(default)]
    pub operator_skills: Option<OperatorSkills>,
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
    /// Claude's Remote Control bridge, as `remoteControlAtStartup` in the
    /// `--settings` overlay. Every claude pane gets the key: `true` here,
    /// `false` everywhere else, so an operator or org default cannot turn it
    /// on in a worker. Only an orchestrator may set it; `check` enforces that.
    #[serde(default)]
    pub remote_control: bool,
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
    /// Teammates whose launch settings a spawn borrows when this one's usage
    /// pool cannot serve it, in order of preference. The persona, base,
    /// phase and skills stay this teammate's. See `balance_policy.rs`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fallbacks: Vec<String>,
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
            settings: None,
            mcp_servers: None,
            mcp_config_files: Vec::new(),
            args: Vec::new(),
            env: BTreeMap::new(),
            trains_on_input: false,
            first_instruction: None,
            fallbacks: Vec::new(),
            persona: String::new(),
        }
    }
}

/// `operator_skills:`: skill directories `names` under `dir`, an
/// operator-local directory. `~/` in `dir` expands against the launch's
/// home. Each name must be a `<dir>/<name>/SKILL.md` whose `name:` matches,
/// must not clash with a catalog skill, and must not be a subagent skill.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperatorSkills {
    pub dir: String,
    pub names: Vec<String>,
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
