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

/// The orchestrator's model. Reserved: a fleet has exactly one Fable session,
/// the orchestrator, and `horch spawn` refuses to start a worker on it. The
/// orchestrator writes every brief for an Opus/Codex-Sol reader instead.
pub const ORCHESTRATOR_MODEL: &str = "fable";

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
    /// No real agent: the smoke teammate exercises the machinery only.
    None,
}

impl Agent {
    pub fn as_str(self) -> &'static str {
        match self {
            Agent::Claude => "claude",
            Agent::Codex => "codex",
            Agent::None => "none",
        }
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
            "none" => Ok(Agent::None),
            other => Err(format!("unknown agent '{other}' (claude, codex, none)")),
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

    /// The execpolicy rules every codex agent needs installed before launch.
    pub fn exec_rules(&self) -> &[ExecRule] {
        match self.bases.get("codex-execpolicy") {
            Some(b) => &b.rules,
            None => &[],
        }
    }

    /// Whether this teammate may be started by `horch spawn`. Orchestrators
    /// are launched by `pane-launch`, never spawned, so this is where the
    /// single-Fable rule bites.
    pub fn is_spawnable(t: &Teammate) -> Result<()> {
        if t.model.as_deref() == Some(ORCHESTRATOR_MODEL) {
            bail!(
                "teammate '{}' runs on {ORCHESTRATOR_MODEL}, which is reserved for the \
                 orchestrator; the fleet has exactly one {ORCHESTRATOR_MODEL} session. Use opus.",
                t.name
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
            if t.agent == Agent::Codex {
                for (field, empty) in [
                    ("tools", t.tools.is_none()),
                    ("allowed_tools", t.allowed_tools.is_empty()),
                    ("disallowed_tools", t.disallowed_tools.is_empty()),
                    ("skills", t.skills.is_empty()),
                    ("plugin_dirs", t.plugin_dirs.is_empty()),
                    ("settings", t.settings.is_none()),
                    ("subagent_model", t.subagent_model.is_none()),
                ] {
                    if !empty {
                        problems.push(format!(
                            "{who}: '{field}' is claude-only; codex controls capability \
                             through its sandbox. Use args."
                        ));
                    }
                }
                if t.setting_sources.is_some() {
                    problems.push(format!("{who}: setting_sources is claude-only"));
                }
                if t.disable_skills {
                    problems.push(format!("{who}: disable_skills is claude-only"));
                }
                if !t.inherit_plugins {
                    problems.push(format!("{who}: inherit_plugins is claude-only"));
                }
                if t.mcp_servers.is_some() || !t.mcp_config_files.is_empty() {
                    problems.push(format!(
                        "{who}: MCP configuration is claude-only; codex uses `codex mcp`"
                    ));
                }
                if let Some(mode) = t.permission_mode {
                    if mode.codex_args().is_none() {
                        problems.push(format!(
                            "{who}: permission_mode '{}' has no codex equivalent",
                            mode.as_str()
                        ));
                    }
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
        // Every command a worker is told to run must be allowed for codex.
        if let Some(worker_base) = self.bases.get("fleet-worker") {
            let told = format!("{}{}", worker_base.body, worker_base.task_idle);
            for rule in self.exec_rules() {
                let cmd: Vec<&str> = rule
                    .pattern
                    .split(',')
                    .map(|p| p.trim().trim_matches('"'))
                    .collect();
                let joined = cmd.join(" ");
                if !told.contains(&joined) {
                    problems.push(format!(
                        "codex-execpolicy allows '{joined}', which _base/fleet-worker.md \
                         never tells a worker to run"
                    ));
                }
            }
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

    /// The fleet has exactly one Fable: the orchestrator. Even a file that
    /// asks for it is refused at spawn, so a stray teammate cannot make a
    /// second one.
    #[test]
    fn a_fable_worker_is_refused_at_spawn() {
        let r = Roster::builtin().unwrap();
        let mut t = r.require("opus").unwrap().clone();
        t.model = Some(ORCHESTRATOR_MODEL.into());
        let err = Roster::is_spawnable(&t).unwrap_err().to_string();
        assert!(err.contains("reserved for the orchestrator"), "{err}");
        assert!(Roster::is_spawnable(r.require("opus").unwrap()).is_ok());
    }

    /// And nothing the orchestrator is offered runs on its own model.
    #[test]
    fn nothing_offered_runs_on_fable() {
        let r = Roster::builtin().unwrap();
        for t in r.offered() {
            assert_ne!(t.model.as_deref(), Some(ORCHESTRATOR_MODEL), "{}", t.name);
        }
        // The orchestrators themselves still do, and are hidden.
        assert_eq!(r.require("orchestrator").unwrap().model.as_deref(), Some("fable"));
        assert!(r.require("orchestrator").unwrap().hidden);
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
