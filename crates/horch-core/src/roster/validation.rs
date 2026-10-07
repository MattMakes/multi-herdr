//! Everything `horch teammates --check` refuses, judged without launching
//! anything: one teammate's fields, the roster as a whole, and the static
//! fallback rules (design section 13.3).

use std::collections::BTreeMap;

use anyhow::{bail, Result};

use super::operator::expand_home;
use super::parser::parse_base;
use super::repository::BUILTIN_BASES;
use super::teammate::{
    HarnessDefault, CONTEXT_MESSAGES_BASE, CONTEXT_MESSAGE_KEYS, CONTEXT_MESSAGE_PLACEHOLDERS,
    CONTEXT_WINDOWS_BASE, HARNESS_DEFAULTS_BASE, HEADLESS_ONLY,
};
use super::{
    effort_problem, reserved_tier, ExecRule, PermissionMode, Roster, Teammate,
    BRIEF_DESCRIPTION_MAX, FLEET_ORCHESTRATORS, ORCHESTRATOR_DENIED_TOOLS,
    ORCHESTRATOR_ONLY_SKILLS,
};
use crate::compaction::window::{
    headroom, native_trigger, threshold, BASE_THRESHOLD, HEADROOM_FLOOR,
};
use crate::harness::capabilities::TriggerRule;
use crate::harness::launch::FORBIDDEN_ENV;
use crate::harness::HarnessKind;
use crate::routing::decision::merge;
use crate::routing::eligible::trains_on_input;
use crate::routing::quota;
use crate::runtime::context::DEFAULT_ENV_YIELD;
use crate::skills::SkillCatalog;

impl Roster {
    /// Whether this teammate may be started by `horch spawn`. Orchestrators
    /// are launched by `pane-launch`, never spawned, so this is where the
    /// one-top-tier-session rule bites. A headless-only teammate never gets
    /// a pane at all.
    pub fn is_spawnable(t: &Teammate) -> Result<()> {
        if HEADLESS_ONLY.contains(&t.name.as_str()) {
            bail!(
                "'{}' is headless only (HEADLESS_ONLY): the dataset coordinator runs it \
                 as a `claude -p` job, and `horch spawn` never starts it in a pane",
                t.name
            );
        }
        Roster::model_is_spawnable(t.model.as_deref().unwrap_or_default(), &t.name)
    }

    /// The same rule, applied to the model a spawn is actually about to launch.
    ///
    /// `horch spawn --resume` takes its model from the ledger record rather than
    /// from the teammate file, so checking the file alone would leave a stale or
    /// hand-edited record able to start a second top-tier session behind an
    /// innocent-looking tier name.
    pub(crate) fn model_is_spawnable(model: &str, who: &str) -> Result<()> {
        if let Some((tier, instead)) = reserved_tier(model) {
            bail!(
                "'{who}' runs on {model}, and the {tier} tier is reserved for the \
                 orchestrator; the fleet has at most one top-tier session, whichever \
                 agent is orchestrating. Use {instead}."
            );
        }
        Ok(())
    }

    /// Judge skill names against `catalog`, which may include installed
    /// marketplace skills, instead of the compiled-in catalog.
    pub fn with_skill_catalog(mut self, catalog: SkillCatalog) -> Roster {
        self.skill_catalog = Some(catalog);
        self
    }

    /// The skills teammates may name: the attached catalog, else the
    /// compiled-in one.
    pub fn skill_catalog(&self) -> Result<SkillCatalog> {
        match &self.skill_catalog {
            Some(catalog) => Ok(catalog.clone()),
            None => SkillCatalog::bundled(),
        }
    }

    /// What is wrong with `t.skills_when`: each pattern as `offer_when`'s,
    /// and each skill must be a catalog skill the teammate can load and does
    /// not already name, switch off or reserve for the orchestrator.
    fn skills_when_problems(&self, t: &Teammate) -> Vec<String> {
        let mut problems = Vec::new();
        if t.skills_when.is_empty() {
            return problems;
        }
        let who = &t.name;
        let catalog = match self.skill_catalog() {
            Ok(catalog) => Some(catalog),
            Err(e) => {
                problems.push(format!("{who}: skills_when: {e:#}"));
                None
            }
        };
        let loads = !t.disable_skills
            && t.agent.capabilities().skill_exposure != crate::harness::SkillExposure::None;
        if !loads {
            problems.push(format!(
                "{who}: skills_when adds skills, but skills cannot load with disabled \
                 skills or no agent"
            ));
        }
        for (pattern, skills) in &t.skills_when {
            if let Some(problem) = super::offer::pattern_problem(pattern) {
                problems.push(format!("{who}: skills_when pattern '{pattern}' {problem}"));
            }
            if skills.is_empty() {
                problems.push(format!("{who}: skills_when '{pattern}' names no skill"));
            }
            for name in skills {
                let unknown = catalog.as_ref().is_some_and(|c| c.lookup(name).is_none());
                if unknown {
                    problems.push(format!(
                        "{who}: skills_when '{pattern}' names unknown skill '{name}'"
                    ));
                }
                if t.skills.contains(name) {
                    problems.push(format!(
                        "{who}: '{name}' is both in skills and in skills_when '{pattern}'"
                    ));
                }
                if t.disabled_skills
                    .iter()
                    .any(|d| d == name || *d == format!("horch:{name}"))
                {
                    problems.push(format!(
                        "{who}: '{name}' is both in skills_when '{pattern}' and in \
                         disabled_skills"
                    ));
                }
                if !FLEET_ORCHESTRATORS.contains(&who.as_str())
                    && ORCHESTRATOR_ONLY_SKILLS.contains(&name.as_str())
                {
                    problems.push(format!(
                        "{who}: skill '{name}' belongs to the orchestrator only"
                    ));
                }
            }
        }
        problems
    }

    /// Everything wrong with one teammate, judged against this roster (its
    /// bases, its rules, its skill catalog). Also run on a teammate merged
    /// with a fallback.
    pub(crate) fn check_teammate(&self, t: &Teammate) -> Vec<String> {
        let mut problems = Vec::new();
        let who = &t.name;
        let selected = match &self.skill_catalog {
            Some(catalog) => crate::skills::selected_in(t, catalog),
            None => crate::skills::selected(t),
        };
        if let Err(error) = selected {
            problems.push(format!("{error:#}"));
        }
        for name in &t.available_skills {
            if t.skills.contains(name) {
                problems.push(format!(
                    "{who}: '{name}' is both in skills and in available_skills"
                ));
            }
        }
        // Operator skills are read from the operator's machine, so `--check`
        // reads the same directory the launch will.
        if t.operator_skills.is_some() {
            let extended = self
                .skill_catalog()
                .and_then(|c| c.with_host_skills(t, self.home.as_deref()));
            if let Err(error) = extended {
                problems.push(format!("{error:#}"));
            }
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
        for pattern in &t.offer_when {
            if let Some(problem) = super::offer::pattern_problem(pattern) {
                problems.push(format!("{who}: offer_when pattern '{pattern}' {problem}"));
            }
        }
        problems.extend(self.skills_when_problems(t));
        // Anything the orchestrator can pick must be something it may spawn.
        if !t.hidden {
            if let Err(e) = Roster::is_spawnable(t) {
                problems.push(format!("{e:#}"));
            }
        }
        if t.agent != HarnessKind::None && t.model.is_none() && t.name != "orchestration-worker" {
            problems.push(format!("{who}: agent is {} but no model is set", t.agent));
        }
        if let Some(effort) = &t.effort {
            if let Some(why) = effort_problem(t.agent, t.model.as_deref(), effort) {
                problems.push(format!("{who}: {why}"));
            }
        }
        // Reinforced plugin skills must exist, or the briefing promises a
        // skill the pane cannot load.
        if t.agent == HarnessKind::Claude && !t.plugin_skills.is_empty() {
            if t.disable_skills {
                problems.push(format!(
                    "{who}: plugin_skills cannot load with disable_skills: true"
                ));
            }
            if let Err(e) = crate::harness::claude_plugins::resolve_all_in(t, self.home.as_deref())
            {
                problems.push(format!("{who}: {e:#}"));
            }
            // The kept skills are named in the skill-bundle briefing, and
            // the bundle path is the one that merges the switch-offs into
            // a teammate's own `settings:` file. No phase and no skills
            // means no bundle: the briefing would never name them.
            if t.phase.is_none() && t.skills.is_empty() {
                problems.push(format!(
                    "{who}: plugin_skills needs a phase or skills, so the briefing \
                     can name them"
                ));
            }
        }
        // A codex worker with no effort silently takes whatever the
        // operator's ~/.codex/config.toml says (the pane's private
        // CODEX_HOME links it). That made two workers run at "medium"
        // nobody chose, so every offered codex teammate states its own.
        if t.agent == HarnessKind::Codex && !t.hidden && t.effort.is_none() {
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
        // Codex, OpenCode, pi and Prime are elsewhere.
        if t.agent == HarnessKind::Claude
            && (!t.hidden || t.name == "orchestrator")
            && !t.allow_subagents
            && !t.disallowed_tools.iter().any(|x| x == "Agent")
        {
            problems.push(format!(
                "{who}: a fleet pane must not spawn subagents; add \
                 disallowed_tools: [Agent] or set allow_subagents: true"
            ));
        }
        // The orchestrator delegates to workers and nothing else: no
        // subagent and no background or cloud agent. `allow_subagents`
        // does not waive this one.
        if t.agent == HarnessKind::Claude && FLEET_ORCHESTRATORS.contains(&who.as_str()) {
            for tool in ORCHESTRATOR_DENIED_TOOLS {
                if !t.disallowed_tools.iter().any(|x| x == tool) {
                    problems.push(format!(
                        "{who}: the orchestrator delegates only to workers; add \
                         {tool} to disallowed_tools"
                    ));
                }
            }
        }
        // The operator's rule (CLAUDE.md): no teammate uses
        // ANTHROPIC_API_KEY. The launch removes it; a file that sets it is
        // a mistake to report, not to hide.
        for key in crate::harness::launch::FORBIDDEN_ENV {
            if t.env.contains_key(key) {
                problems.push(format!("{who}: env must not set {key}; never use it"));
            }
        }
        // An agy child signs in with the operator's Google login. These keys
        // bypass it, and the launch removes them from an agy child.
        if t.agent == HarnessKind::Antigravity {
            for key in crate::harness::antigravity::ANTIGRAVITY_FORBIDDEN_ENV {
                if t.env.contains_key(key) {
                    problems.push(format!(
                        "{who}: env must not set {key}; antigravity uses the Google login"
                    ));
                }
            }
            // agy 1.3.0 refuses a bare model id without --effort ("requires
            // --effort"), so the worker would die at launch. An id with the
            // level in it (gemini-3.8-flash-low) runs without one.
            if let Some(model) = &t.model {
                if t.effort.is_none()
                    && !crate::harness::antigravity::EFFORT_SUFFIXES
                        .iter()
                        .any(|s| model.ends_with(s))
                {
                    problems.push(format!(
                        "{who}: antigravity model '{model}' needs an effort; agy refuses a \
                         bare model id without --effort (set effort: low, medium or high)"
                    ));
                }
            }
        }
        // Network access and "never ask" do not go together: a codex
        // worker with both can send code out, or pull code in and run it,
        // and nothing stops it. `codex-network` pairs the network with
        // acceptEdits (-a on-request).
        if t.agent == HarnessKind::Codex
            && t.args
                .iter()
                .any(|a| a.replace(' ', "") == "sandbox_workspace_write.network_access=true")
            && matches!(
                t.permission_mode,
                Some(PermissionMode::Auto) | Some(PermissionMode::BypassPermissions)
            )
        {
            problems.push(format!(
                "{who}: network access with permission_mode '{}' never asks before \
                 it sends or fetches; use acceptEdits",
                t.permission_mode.unwrap().as_str()
            ));
        }
        // Orchestrator-only capabilities stay with the orchestrator.
        if !FLEET_ORCHESTRATORS.contains(&who.as_str()) {
            for skill in ORCHESTRATOR_ONLY_SKILLS {
                if t.skills
                    .iter()
                    .chain(&t.available_skills)
                    .any(|s| s == skill)
                {
                    problems.push(format!(
                        "{who}: skill '{skill}' belongs to the orchestrator only"
                    ));
                }
            }
            if t.plugin_skills.contains_key("skill-creator") {
                problems.push(format!(
                    "{who}: plugin 'skill-creator' belongs to the orchestrator only"
                ));
            }
            if t.remote_control {
                problems.push(format!(
                    "{who}: remote_control belongs to the orchestrator only"
                ));
            }
        }
        // Only Claude has an OS sandbox horch can switch on per launch. On any
        // other agent the field would promise a boundary that never exists.
        if let Some(block) = &t.sandbox {
            if t.agent != HarnessKind::Claude {
                problems.push(format!(
                    "{who}: sandbox is Claude only; agent {} cannot honour it",
                    t.agent
                ));
            }
            if t.settings.is_some() {
                problems.push(format!(
                    "{who}: sandbox cannot be combined with settings (the file \
                     replaces the overlay that carries the sandbox)"
                ));
            }
            for problem in crate::harness::claude::sandbox_problems(block) {
                problems.push(format!("{who}: {problem}"));
            }
        }
        if let Some(mode) = t.permission_mode {
            let ok = match t.agent {
                HarnessKind::Codex => mode.codex_args().is_some(),
                HarnessKind::OpenCode => mode.opencode_args().is_some(),
                HarnessKind::Antigravity => mode.antigravity_args().is_some(),
                // pi and Prime run their tools without asking, so there is
                // no gate for a mode to set. Saying nothing is correct;
                // saying `acceptEdits` implies a restraint that is absent.
                HarnessKind::Pi | HarnessKind::Prime => false,
                HarnessKind::Claude | HarnessKind::None => true,
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
        if t.permission_mode == Some(PermissionMode::Plan) && t.agent == HarnessKind::Claude {
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
            if !expand_home(dir, self.home.as_deref()).is_dir() {
                problems.push(format!("{who}: plugin_dir '{dir}' does not exist"));
            }
        }
        // A teammate that brings its own settings takes over the status line
        // too; one without a statusLine would be the only pane in the fleet
        // with none. `settings` is inline JSON (starts with `{`) or a file.
        if let Some(settings) = &t.settings {
            let inline = is_inline(settings);
            let path = expand_home(settings, self.home.as_deref());
            match self.teammate_settings(t) {
                None if inline => {
                    problems.push(format!("{who}: inline settings are not a JSON object"))
                }
                None => problems.push(format!(
                    "{who}: settings file '{}' is missing or not valid JSON",
                    path.display()
                )),
                Some(v) if !v.contains_key("statusLine") => {
                    let what = if inline {
                        "inline settings define".to_string()
                    } else {
                        format!("settings file '{}' defines", path.display())
                    };
                    problems.push(format!(
                        "{who}: {what} no statusLine, so this pane would be the only one in \
                         the fleet without one"
                    ))
                }
                Some(_) => {}
            }
        }
        for file in &t.mcp_config_files {
            if !expand_home(file, self.home.as_deref()).is_file() {
                problems.push(format!(
                    "{who}: mcp_config_files entry '{file}' does not exist"
                ));
            }
        }
        problems
    }

    /// Everything wrong with the loaded roster, as human-readable lines.
    /// Empty means healthy.
    pub fn check(&self) -> Vec<String> {
        // A file that did not load is a problem here, though loading goes on
        // without it: the gate stays strict.
        let mut problems = self.load_warnings();
        for t in self.teammates.values() {
            problems.extend(self.check_teammate(t));
        }
        problems.extend(fallback_problems(self));
        problems.extend(self.harness_default_problems());
        problems.extend(self.context_policy_problems());
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

/// Env keys, `-c` keys and settings key paths that set a native compaction
/// window. A harness default never sets one: the per-tier windows live in
/// the context-windows base.
const WINDOW_KEYS: [&str; 4] = [
    "CLAUDE_CODE_AUTO_COMPACT_WINDOW",
    "CLAUDE_AUTOCOMPACT_PCT_OVERRIDE",
    "model_auto_compact_token_limit",
    "autoCompactWindow",
];

impl Roster {
    /// Check rules 2 and 3 of the harness defaults (design 6A.3): no
    /// teammate file sets `harness_defaults`, and every entry of
    /// `_base/harness-defaults.md` is well formed.
    fn harness_default_problems(&self) -> Vec<String> {
        let mut problems: Vec<String> = self
            .defaults_set_in_file
            .iter()
            .map(|name| {
                format!(
                    "{name}: harness_defaults is set by _base/{HARNESS_DEFAULTS_BASE}.md; delete it from the teammate file"
                )
            })
            .collect();
        for t in self.teammates.values() {
            problems.extend(repeated_defaults(t));
        }
        let Some(base) = self.bases.get(HARNESS_DEFAULTS_BASE) else {
            return problems;
        };
        for (i, entry) in base.defaults.iter().enumerate() {
            let who = format!(
                "_base/{HARNESS_DEFAULTS_BASE}.md entry {} ({})",
                i + 1,
                entry.harness
            );
            problems.extend(
                harness_default_entry_problems(entry)
                    .into_iter()
                    .map(|p| format!("{who}: {p}")),
            );
        }
        problems
    }
}

impl Roster {
    /// The teammate's `settings` as a JSON object: the inline JSON, or the
    /// file it names. None: no settings, or they do not read or parse as an
    /// object.
    fn teammate_settings(
        &self,
        t: &Teammate,
    ) -> Option<serde_json::Map<String, serde_json::Value>> {
        let settings = t.settings.as_deref()?;
        let text = if is_inline(settings) {
            settings.to_string()
        } else {
            std::fs::read_to_string(expand_home(settings, self.home.as_deref())).ok()?
        };
        match serde_json::from_str(&text).ok()? {
            serde_json::Value::Object(map) => Some(map),
            _ => None,
        }
    }

    /// The context-policy rules of `horch teammates --check` (design 6.3):
    /// 1 the headroom floor (CTX-08), 2 no window outside `compact_window`,
    /// 3 no `compact_window` without a lever, 4 the 2 context bases.
    fn context_policy_problems(&self) -> Vec<String> {
        let mut problems = Vec::new();
        for t in self.teammates.values() {
            problems.extend(self.headroom_problem(t));
            let settings = self.teammate_settings(t);
            problems.extend(window_outside_compact_window(t, settings.as_ref()));
            let trigger = t.agent.capabilities().compaction.trigger;
            if t.compact_window.is_some()
                && matches!(trigger, TriggerRule::Unknown | TriggerRule::PerModel)
            {
                problems.push(format!(
                    "{}: compact_window does nothing on harness {}: it has no window setting",
                    t.name, t.agent
                ));
            }
        }
        problems.extend(self.windows_key_problems());
        problems.extend(self.context_base_problems());
        problems
    }

    /// Rule 1 (CTX-08) for a teammate's own `compact_window`: it leaves at
    /// least `HEADROOM_FLOOR` tokens between the watch threshold and the
    /// native trigger. The `windows` keys are checked by themselves, in
    /// [`Self::windows_key_problems`]. A harness whose trigger horch cannot
    /// compute is skipped.
    fn headroom_problem(&self, t: &Teammate) -> Option<String> {
        let w = t.compact_window?;
        let model = t.model.as_deref().unwrap_or("");
        let rule = t.agent.capabilities().compaction.trigger;
        let native = native_trigger(rule, t.agent.as_str(), model, Some(w), None)?;
        let (t_watch, h) = below_floor(native)?;
        Some(format!(
            "{}: compact window {w} leaves headroom {h} tokens, below the {HEADROOM_FLOOR} floor (native trigger {native}, threshold {t_watch})",
            t.name
        ))
    }

    /// Rules 1 and 4 for each `windows` key `<harness>/<name>`, independent
    /// of the teammates: a substitution launches a teammate on its
    /// fallback's harness, so any key can apply. Each key has a harness with
    /// a window setting and a name, meets the floor, and has 1 reading.
    fn windows_key_problems(&self) -> Vec<String> {
        let file = format!("_base/{CONTEXT_WINDOWS_BASE}.md");
        let Some(windows) = self.context_windows().and_then(|b| b.windows.as_ref()) else {
            return Vec::new();
        };
        let mut problems = Vec::new();
        for (key, &w) in windows {
            let Some((kind, name)) = split_window_key(key) else {
                problems.push(format!(
                    "{file}: windows key {key} is not <harness>/<teammate or model> with a harness name"
                ));
                continue;
            };
            if name.is_empty() {
                problems.push(format!(
                    "{file}: windows key {key} has no teammate or model name"
                ));
                continue;
            }
            let rule = kind.capabilities().compaction.trigger;
            let native = match rule {
                TriggerRule::Unknown | TriggerRule::PerModel => {
                    problems.push(format!(
                        "{file}: windows key {key} does nothing on harness {kind}: it has no window setting"
                    ));
                    continue;
                }
                // A teammate name as the model part: the model window is
                // unknown, so the key's value is the window.
                TriggerRule::WindowMinusReserve { reserve } => {
                    native_trigger(rule, kind.as_str(), name, Some(w), None)
                        .unwrap_or_else(|| w.saturating_sub(reserve))
                }
                TriggerRule::CodexLimit => w,
            };
            if let Some((t_watch, h)) = below_floor(native) {
                problems.push(format!(
                    "{file}: windows {key} {w} leaves headroom {h} tokens, below the {HEADROOM_FLOOR} floor (native trigger {native}, threshold {t_watch})"
                ));
            }
            // Finding 3: teammate `name` runs another model, and `name` is
            // also the model of a teammate on the same harness.
            if let Some(t) = self.get(name).filter(|t| t.agent == kind) {
                let model = t.model.as_deref().unwrap_or("");
                let also_model = self
                    .teammates
                    .values()
                    .any(|o| o.agent == kind && o.model.as_deref() == Some(name));
                if model != name && also_model {
                    problems.push(format!(
                        "{file}: windows key {key} is teammate {name} (model {model}) and model {name}; set compact_window in {name}.md instead"
                    ));
                }
            }
        }
        problems
    }

    /// Rule 4: both context bases exist, each holds only its own keys, no
    /// other base holds them, and every name and message is well formed.
    fn context_base_problems(&self) -> Vec<String> {
        let mut problems = Vec::new();
        for name in [CONTEXT_WINDOWS_BASE, CONTEXT_MESSAGES_BASE] {
            if !self.bases.contains_key(name) {
                problems.push(format!("_base/{name}.md is missing"));
            }
        }
        for (name, base) in &self.bases {
            let file = format!("_base/{name}.md");
            for (key, set, owner) in [
                ("windows", base.windows.is_some(), CONTEXT_WINDOWS_BASE),
                ("in_place", base.in_place.is_some(), CONTEXT_WINDOWS_BASE),
                ("messages", !base.messages.is_empty(), CONTEXT_MESSAGES_BASE),
            ] {
                if set && name != owner {
                    problems.push(format!(
                        "{file}: {key} belongs in _base/{owner}.md; delete it here"
                    ));
                }
            }
        }
        if let Some(base) = self.context_windows() {
            let file = format!("_base/{CONTEXT_WINDOWS_BASE}.md");
            for (key, set) in [
                ("windows", base.windows.is_some()),
                ("in_place", base.in_place.is_some()),
            ] {
                if !set {
                    problems.push(format!(
                        "{file} sets no {key}; a copy replaces the whole file, so keep both keys"
                    ));
                }
            }
            for entry in base.in_place.iter().flatten() {
                match harness_kind(entry) {
                    None => problems.push(format!(
                        "{file}: in_place entry {entry} is not a harness name"
                    )),
                    Some(kind) if !kind.capabilities().compaction.has_command() => problems
                        .push(format!(
                        "{file}: in_place entry {entry}: horch has no compact command for {entry}"
                    )),
                    Some(_) => {}
                }
            }
        }
        if let Some(base) = self.context_messages() {
            let file = format!("_base/{CONTEXT_MESSAGES_BASE}.md");
            for key in CONTEXT_MESSAGE_KEYS {
                if !base.messages.contains_key(key) {
                    problems.push(format!("{file}: message {key} is missing"));
                }
            }
            let known: BTreeMap<&str, &str> = CONTEXT_MESSAGE_PLACEHOLDERS
                .into_iter()
                .map(|p| (p, ""))
                .collect();
            for (key, message) in &base.messages {
                if message.trim().is_empty() {
                    problems.push(format!("{file}: message {key} is empty"));
                }
                if message.contains(['\n', '\r']) {
                    problems.push(format!(
                        "{file}: message {key} has a newline; a newline submits the line early"
                    ));
                }
                if let Err(e) = crate::prompts::render(message, &known) {
                    problems.push(format!("{file}: message {key}: {e}"));
                }
            }
        }
        problems
    }
}

/// Warnings, not errors, of the context policy:
/// - a `context-messages` base whose content differs from the built-in one
///   (the briefings name its prefixes);
/// - a `windows` key whose name is no teammate and no model of a teammate on
///   its harness (an operator can add a model before its teammate).
pub fn context_policy_warnings(roster: &Roster) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(windows) = roster.context_windows().and_then(|b| b.windows.as_ref()) {
        for key in windows.keys() {
            let Some((kind, name)) = split_window_key(key) else {
                continue;
            };
            // An empty name or a harness without a window setting is
            // already a check problem.
            let lever = !matches!(
                kind.capabilities().compaction.trigger,
                TriggerRule::Unknown | TriggerRule::PerModel
            );
            let known = name.is_empty()
                || !lever
                || roster.get(name).is_some()
                || roster
                    .teammates
                    .values()
                    .any(|t| t.agent == kind && t.model.as_deref() == Some(name));
            if !known {
                out.push(format!(
                    "_base/{CONTEXT_WINDOWS_BASE}.md: windows key {key} names no teammate and no model of a teammate on {kind}"
                ));
            }
        }
    }
    let builtin = BUILTIN_BASES
        .iter()
        .find(|(name, _)| *name == CONTEXT_MESSAGES_BASE)
        .and_then(|(name, text)| parse_base(name, text).ok());
    if let (Some(loaded), Some(builtin), Some(path)) = (
        roster.context_messages(),
        builtin,
        roster.base_origin(CONTEXT_MESSAGES_BASE),
    ) {
        if *loaded != builtin {
            out.push(format!(
                "_base/{CONTEXT_MESSAGES_BASE}.md: {} replaces the built-in protocol messages; the briefings name their prefixes, so delete the copy",
                path.display()
            ));
        }
    }
    out
}

/// The harness and the name part of a `windows` key. None: the part before
/// the first `/` is not a harness name.
fn split_window_key(key: &str) -> Option<(HarnessKind, &str)> {
    let (harness, name) = key.split_once('/')?;
    Some((harness_kind(harness)?, name))
}

fn harness_kind(name: &str) -> Option<HarnessKind> {
    HarnessKind::ALL
        .iter()
        .copied()
        .find(|k| k.as_str() == name)
}

/// The threshold and headroom of `native` when the headroom is below
/// `HEADROOM_FLOOR`. None: the floor holds.
fn below_floor(native: u64) -> Option<(u64, u64)> {
    let t_watch = threshold(BASE_THRESHOLD, Some(native));
    let h = headroom(native, t_watch);
    (h < HEADROOM_FLOOR).then_some((t_watch, h))
}

/// Whether a `settings:` value is inline JSON rather than a file path.
fn is_inline(settings: &str) -> bool {
    settings.trim_start().starts_with('{')
}

/// Rule 2: a teammate sets its compaction window in `env`, in its `settings`
/// (inline JSON or a file) or in an `args` `-c` pair, with any of
/// [`WINDOW_KEYS`]. These bypass the operator check or do nothing;
/// `compact_window` is the lever.
fn window_outside_compact_window(
    t: &Teammate,
    settings: Option<&serde_json::Map<String, serde_json::Value>>,
) -> Vec<String> {
    let mut places: Vec<String> = t
        .env
        .keys()
        .filter(|k| WINDOW_KEYS.contains(&k.as_str()))
        .map(|k| format!("env {k}"))
        .collect();
    let mut paths = Vec::new();
    if let Some(settings) = settings {
        json_paths(settings, "", &mut paths);
    }
    paths.sort();
    places.extend(
        paths
            .iter()
            .filter(|p| {
                p.rsplit('.')
                    .next()
                    .is_some_and(|k| WINDOW_KEYS.contains(&k))
            })
            .map(|p| format!("settings {p}")),
    );
    places.extend(
        HarnessDefault::config_keys(&t.args)
            .into_iter()
            .filter(|k| WINDOW_KEYS.contains(k))
            .map(|k| format!("args {k}")),
    );
    places
        .into_iter()
        .map(|place| {
            format!(
                "{}: set the compaction window with compact_window, not {place}",
                t.name
            )
        })
        .collect()
}

/// Check rule 1: the teammate's lines that only repeat a harness default of
/// its own harness: an `env` value, an `args` `-c` pair or flag, a
/// `settings` key path, an `OPENCODE_CONFIG_CONTENT` key path. A value that
/// differs from the default is the override, and is allowed.
fn repeated_defaults(t: &Teammate) -> Vec<String> {
    let mut env = BTreeMap::new();
    let mut args = Vec::new();
    let mut settings = serde_json::Map::new();
    let mut config = serde_json::Map::new();
    for entry in HarnessDefault::for_harness(&t.harness_defaults, t.agent) {
        env.extend(entry.env.iter());
        args.extend(entry.args.iter().map(String::as_str));
        for (from, into) in [
            (&entry.settings, &mut settings),
            (&entry.config, &mut config),
        ] {
            if let Some(from) = from {
                into.extend(from.clone());
            }
        }
    }
    let kind = t.agent;
    let who = &t.name;
    let mut out = Vec::new();
    for (key, value) in &t.env {
        if env.get(key) == Some(&value) {
            out.push(format!(
                "{who}: env {key} repeats the {kind} harness default; delete the line"
            ));
        }
    }
    let mut own = t.args.iter();
    while let Some(arg) = own.next() {
        if arg == "-c" {
            let Some(pair) = own.next() else { break };
            if args.windows(2).any(|w| w[0] == "-c" && w[1] == pair) {
                out.push(format!(
                    "{who}: args -c {pair} repeats the {kind} harness default; delete the pair"
                ));
            }
        } else if args.contains(&arg.as_str()) {
            out.push(format!(
                "{who}: args {arg} repeats the {kind} harness default; delete it"
            ));
        }
    }
    let inline = |text: Option<&String>| {
        text.filter(|s| s.trim_start().starts_with('{'))
            .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
    };
    for (what, own, defaults) in [
        ("settings", inline(t.settings.as_ref()), &settings),
        (
            "OPENCODE_CONFIG_CONTENT",
            inline(t.env.get("OPENCODE_CONFIG_CONTENT")),
            &config,
        ),
    ] {
        let Some(serde_json::Value::Object(own)) = own else {
            continue;
        };
        let mut paths = Vec::new();
        json_leaves(defaults, "", &mut paths);
        for (path, value) in paths {
            let mut node = Some(&own);
            let mut found = None;
            let parts: Vec<&str> = path.split('.').collect();
            for (i, part) in parts.iter().enumerate() {
                let next = node.and_then(|n| n.get(*part));
                if i + 1 == parts.len() {
                    found = next;
                } else {
                    node = next.and_then(|v| v.as_object());
                }
            }
            if found == Some(&value) {
                out.push(format!(
                    "{who}: {what} {path} repeats the {kind} harness default; delete it"
                ));
            }
        }
    }
    out
}

/// Every leaf of a JSON object as (dotted path, value).
fn json_leaves(
    obj: &serde_json::Map<String, serde_json::Value>,
    prefix: &str,
    out: &mut Vec<(String, serde_json::Value)>,
) {
    for (key, value) in obj {
        let path = if prefix.is_empty() {
            key.clone()
        } else {
            format!("{prefix}.{key}")
        };
        match value {
            serde_json::Value::Object(inner) if !inner.is_empty() => json_leaves(inner, &path, out),
            _ => out.push((path, value.clone())),
        }
    }
}

/// What is wrong with one harness-default entry (check rule 3).
fn harness_default_entry_problems(entry: &HarnessDefault) -> Vec<String> {
    let mut out = Vec::new();
    let kind = entry.harness;
    for (field, set, owner) in [
        ("settings", entry.settings.is_some(), HarnessKind::Claude),
        ("config", entry.config.is_some(), HarnessKind::OpenCode),
        (
            "agent_settings",
            entry.agent_settings.is_some(),
            HarnessKind::Prime,
        ),
    ] {
        if set && kind != owner {
            out.push(format!("{field} is only for harness {owner}"));
        }
    }
    if entry.force.as_deref().is_some_and(|f| f.trim().is_empty()) {
        out.push("force must give the reason and the decision that allowed it".into());
    }
    let mut args = entry.args.iter();
    while let Some(arg) = args.next() {
        if arg == "-c" {
            match args.next() {
                Some(pair) if pair.contains('=') => {}
                _ => out.push("args: -c needs a <key>=<value> after it".into()),
            }
        } else if !arg.starts_with('-') {
            out.push(format!(
                "args: '{arg}' is neither a -c <key>=<value> pair nor a flag"
            ));
        }
    }
    for key in entry.env.keys() {
        if FORBIDDEN_ENV.contains(&key.as_str()) || key.starts_with("ANTHROPIC_") {
            out.push(format!("env {key} is forbidden in a fleet launch"));
        } else if kind != HarnessKind::Claude && !DEFAULT_ENV_YIELD.contains(&key.as_str()) {
            out.push(format!(
                "env {key}: add it to runtime::context::DEFAULT_ENV_YIELD"
            ));
        }
    }
    let mut settings_paths = Vec::new();
    if let Some(settings) = &entry.settings {
        json_paths(settings, "", &mut settings_paths);
    }
    let window = entry
        .env
        .keys()
        .map(String::as_str)
        .chain(HarnessDefault::config_keys(&entry.args))
        .chain(settings_paths.iter().flat_map(|p| p.rsplit('.').next()))
        .find(|k| WINDOW_KEYS.contains(k));
    if let Some(key) = window {
        out.push(format!(
            "{key} sets a compaction window; windows belong in _base/context-windows.md"
        ));
    }
    out
}

/// Every key path of a JSON object, dotted, parents included.
fn json_paths(
    obj: &serde_json::Map<String, serde_json::Value>,
    prefix: &str,
    out: &mut Vec<String>,
) {
    for (key, value) in obj {
        let path = if prefix.is_empty() {
            key.clone()
        } else {
            format!("{prefix}.{key}")
        };
        if let serde_json::Value::Object(inner) = value {
            json_paths(inner, &path, out);
        }
        out.push(path);
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

/// Tools a Claude persona may lean on that no other agent has (rule 6).
const CLAUDE_ONLY_TOOLS: [&str; 7] = [
    "Agent",
    "Task",
    "TodoWrite",
    "WebFetch",
    "WebSearch",
    "NotebookEdit",
    "Skill",
];

/// Section 13.3 rules 1-5, as `horch teammates --check` errors (BAL-02).
pub fn fallback_problems(roster: &Roster) -> Vec<String> {
    let mut out = Vec::new();
    for name in roster.names() {
        let Some(t) = roster.get(name) else { continue };
        let pool = quota::pool_for(t.agent.as_str(), t.model.as_deref().unwrap_or_default());
        for fb in &t.fallbacks {
            let who = &t.name;
            let Some(f) = roster.get(fb) else {
                out.push(format!("{who}: fallback '{fb}' does not exist"));
                continue;
            };
            if f.hidden {
                out.push(format!("{who}: fallback '{fb}' is hidden"));
            }
            if let Err(e) = Roster::is_spawnable(f) {
                out.push(format!("{who}: fallback '{fb}' is not spawnable: {e:#}"));
            }
            let fpool = quota::pool_for(f.agent.as_str(), f.model.as_deref().unwrap_or_default());
            if fpool == pool {
                out.push(format!(
                    "{who}: fallback '{fb}' draws from the same pool ({pool}), so it cannot help"
                ));
            }
            if trains_on_input(f) {
                out.push(format!(
                    "{who}: fallback '{fb}' trains on its input; free tiers are chosen by \
                     the orchestrator, never automatically"
                ));
            }
            let merged = merge(t, f);
            for p in roster.check_teammate(&merged) {
                out.push(format!("{who} via {fb}: {p}"));
            }
        }
    }
    out
}

/// Rule 6: warnings, not errors. A persona that names a Claude-only tool,
/// on a fallback whose agent lacks it.
pub fn fallback_warnings(roster: &Roster) -> Vec<String> {
    let mut out = Vec::new();
    for name in roster.names() {
        let Some(t) = roster.get(name) else { continue };
        for fb in &t.fallbacks {
            let Some(f) = roster.get(fb) else { continue };
            if f.agent == HarnessKind::Claude {
                continue;
            }
            for tool in CLAUDE_ONLY_TOOLS {
                if names_tool(&t.persona, tool) {
                    out.push(format!(
                        "{}: the persona names the {tool} tool, which {} (fallback {fb}) lacks",
                        t.name, f.agent
                    ));
                }
            }
        }
    }
    out
}

/// Warnings, not errors: operator skills this host does not have. The
/// launch skips them and says so in the briefing, because each host exports
/// its own copy with Xcode. A real problem with the field is an error in
/// [`Roster::check`] instead.
pub fn operator_skill_warnings(roster: &Roster) -> Vec<String> {
    let mut out = Vec::new();
    for name in roster.names() {
        let Some(t) = roster.get(name) else { continue };
        if t.operator_skills.is_none() {
            continue;
        }
        let Ok(catalog) = roster
            .skill_catalog()
            .and_then(|c| c.with_host_skills(t, roster.home.as_deref()))
        else {
            continue;
        };
        for skipped in catalog.skipped_operator() {
            out.push(format!("{name}: {}", skipped.note()));
        }
    }
    out
}

/// Whether `text` names `tool` as a tool: the name in backticks, or followed
/// by the word "tool". Plain English "Task" or "Agent" does not count.
pub(crate) fn names_tool(text: &str, tool: &str) -> bool {
    text.contains(&format!("`{tool}`"))
        || text.contains(&format!("{tool} tool"))
        || text.contains(&format!("{tool}("))
}

#[cfg(test)]
mod spawnable_tests {
    use super::super::operator::effort_override_warnings;
    use super::super::parser::parse_teammate;
    use super::super::Phase;
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

    /// A teammate may name an installed marketplace skill once the roster
    /// carries a catalog that holds it; without one, the name is unknown.
    #[test]
    fn marketplace_skills_pass_the_check_with_the_installed_catalog() {
        let lock = crate::skills::catalog::marketplace::LockEntry {
            id: "demo".into(),
            source: "https://example.com/r.git".into(),
            requested_revision: Some("v1".into()),
            resolved_commit: Some("ab".repeat(20)),
            version: "git+abababababab".into(),
            digest: format!("sha256:{}", "cd".repeat(32)),
            installed_at: "2026-10-02T12:00:00Z".into(),
        };
        let mut r = Roster::builtin().unwrap();
        r.teammates
            .get_mut("sonnet")
            .unwrap()
            .skills
            .push("demo".into());
        let unknown = "teammate 'sonnet': unknown bundled skill 'demo'";
        assert!(r.check().iter().any(|p| p == unknown), "{:?}", r.check());
        let catalog = SkillCatalog::bundled().unwrap().with_lock(&[lock]).unwrap();
        let r = r.with_skill_catalog(catalog);
        assert!(r.skill_catalog().unwrap().lookup("demo").is_some());
        assert!(r.check().is_empty(), "{:?}", r.check());
        let sonnet = r.require("sonnet").unwrap();
        assert!(
            crate::skills::ensure_supported_in(sonnet, &SkillCatalog::bundled().unwrap()).is_err()
        );
        crate::skills::ensure_supported_in(sonnet, &r.skill_catalog().unwrap()).unwrap();
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
                // antigravity exposes no skills, so it can have no phase.
                "smoke" | "judge" | "antigravity" => None,
                "researcher" | "product-lead" | "designer" | "design-director" => {
                    Some(Phase::Research)
                }
                "staff-engineer"
                | "opus-architect"
                | "orchestrator"
                | "orchestrator-codex"
                | "orchestration-orchestrator"
                | "ue-tech-lead"
                | "godot-tech-lead" => Some(Phase::Plan),
                "architect-reviewer"
                | "qa-engineer"
                | "codex-reviewer"
                | "design-critic"
                | "ue-qa-engineer"
                | "ue-code-reviewer"
                | "swift-reviewer"
                | "swift-qa-engineer"
                | "apple-accessibility-auditor"
                | "codex-swift-reviewer"
                | "godot-qa-engineer"
                | "godot-code-reviewer" => Some(Phase::Validation),
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
            (HarnessKind::Claude, "opus", "medium"),
            (HarnessKind::Claude, "sonnet", "max"),
            (HarnessKind::Codex, "gpt-5.6-sol", "none"),
            (HarnessKind::Codex, "gpt-6-astra", "xhigh"),
            (HarnessKind::OpenCode, "anthropic/claude-sonnet-5", "high"),
            (HarnessKind::Pi, "ollama/qwen3.8", "off"),
            (HarnessKind::Prime, "anthropic/claude-opus-5-5", "minimal"),
        ] {
            assert_eq!(
                effort_problem(agent, Some(model), effort),
                None,
                "{agent} {effort}"
            );
        }
        // Refused, each with a reason that names the fix.
        for (agent, model, effort, says) in [
            (
                HarnessKind::Claude,
                "opus",
                "minimal",
                "expected one of: low, medium",
            ),
            (HarnessKind::Claude, "haiku", "low", "no effort setting"),
            (
                HarnessKind::Claude,
                "claude-haiku-4-5-20251001",
                "low",
                "no effort setting",
            ),
            (HarnessKind::Codex, "gpt-5.6-sol", "minimal", "API error"),
            (
                HarnessKind::Codex,
                "gpt-5.6-sol",
                "ultra",
                "multiplies spend",
            ),
            (HarnessKind::Codex, "gpt-6-astra", "none", "use low"),
            (
                HarnessKind::OpenCode,
                "opencode/big-pickle",
                "high",
                "no effort setting",
            ),
            (
                HarnessKind::OpenCode,
                "opencode/nemotron-3-ultra-free",
                "high",
                "no effort setting",
            ),
            (HarnessKind::Pi, "ollama/qwen3.8", "ultra", "not a pi level"),
            (
                HarnessKind::Prime,
                "anthropic/claude-opus-5-5",
                "none",
                "not a prime level",
            ),
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
        assert!(
            effort_override_warnings(
                Some(""),
                Some(&serde_json::json!({"effortLevel": "high"})),
                Some("model = \"gpt-5.6-sol\"\n[profiles.x]\nmodel_reasoning_effort = \"low\"\n"),
            )
            .is_empty(),
            "effortLevel loses to --effort, and a profile's key is not the default"
        );

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
        assert!(
            w[1].contains("env.CLAUDE_CODE_EFFORT_LEVEL=medium"),
            "{w:?}"
        );
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
        let bare = r.teammates.get_mut("opus").unwrap();
        bare.phase = None;
        bare.skills.clear();
        assert!(
            r.check()
                .iter()
                .any(|p| p.starts_with("opus: plugin_skills needs a phase or skills")),
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

    /// Only `codex-network` opens the codex sandbox's network, and it is not
    /// allowed to pair that with "never ask".
    #[test]
    fn codex_network_access_is_contained() {
        let mut r = Roster::builtin().unwrap();
        let net = "sandbox_workspace_write.network_access=true";
        for name in r.names() {
            let t = r.require(name).unwrap();
            let has = t.args.iter().any(|a| a == net);
            assert_eq!(has, name == "codex-network", "{name}");
        }
        let cmd = crate::harness::launch::command_in(
            &crate::harness::launch::LaunchEnv::for_test(),
            r.require("codex-network").unwrap(),
            crate::harness::launch::Session::Unmanaged,
            "p",
            None,
        )
        .unwrap();
        let a: Vec<String> = cmd
            .get_args()
            .map(|x| x.to_string_lossy().into_owned())
            .collect();
        assert!(
            a.windows(4)
                .any(|w| w == ["-s", "workspace-write", "-a", "on-request"]),
            "{a:?}"
        );
        assert!(a.windows(2).any(|w| w == ["-c", net]), "{a:?}");

        r.teammates
            .get_mut("codex-network")
            .unwrap()
            .permission_mode = Some(PermissionMode::Auto);
        assert!(
            r.check().iter().any(|p| p.starts_with(
                "codex-network: network access with permission_mode 'auto'"
            )),
            "{:?}",
            r.check()
        );
    }

    /// `sandbox:` is Claude only, cannot pair with a settings file, and
    /// cannot carry a way out. A plain block on a claude teammate passes.
    #[test]
    fn roster_check_holds_the_sandbox_to_claude_and_no_way_out() {
        let block = |v: serde_json::Value| Some(v.as_object().unwrap().clone());
        let mut r = Roster::builtin().unwrap();
        r.teammates.get_mut("sonnet").unwrap().sandbox =
            block(serde_json::json!({"network": {"allowedDomains": ["x.example"]}}));
        assert!(
            !r.check().iter().any(|p| p.starts_with("sonnet:")),
            "{:?}",
            r.check()
        );

        r.teammates.get_mut("codex-sol").unwrap().sandbox = block(serde_json::json!({}));
        let sonnet = r.teammates.get_mut("sonnet").unwrap();
        sonnet.sandbox = block(serde_json::json!({"excludedCommands": ["asc *"]}));
        sonnet.settings = Some("~/s.json".into());
        let problems = r.check();
        for expected in [
            "codex-sol: sandbox is Claude only; agent codex cannot honour it",
            "sonnet: sandbox cannot be combined with settings",
            "sonnet: sandbox.excludedCommands runs commands outside the sandbox",
        ] {
            assert!(
                problems.iter().any(|p| p.starts_with(expected)),
                "{expected}: {problems:?}"
            );
        }
    }

    /// An agy teammate that sets a Google key would leave the operator's
    /// login. Another agent may set it: only agy children lose it.
    #[test]
    fn an_antigravity_teammate_cannot_set_a_google_key() {
        let mut r = Roster::builtin().unwrap();
        let t = r.teammates.get_mut("sonnet").unwrap();
        t.env.insert("GEMINI_API_KEY".into(), "x".into());
        assert!(!r.check().iter().any(|p| p.contains("GEMINI_API_KEY")));
        let t = r.teammates.get_mut("sonnet").unwrap();
        t.agent = HarnessKind::Antigravity;
        t.model = Some("gemini-3.8-flash".into());
        t.effort = None;
        for key in crate::harness::antigravity::ANTIGRAVITY_FORBIDDEN_ENV {
            r.teammates
                .get_mut("sonnet")
                .unwrap()
                .env
                .insert(key.into(), "x".into());
        }
        let problems = r.check();
        for key in crate::harness::antigravity::ANTIGRAVITY_FORBIDDEN_ENV {
            assert!(
                problems.iter().any(|p| p
                    == &format!(
                        "sonnet: env must not set {key}; antigravity uses the Google login"
                    )),
                "{key}: {problems:?}"
            );
        }
    }

    /// agy refuses a bare model id without --effort, so the roster check
    /// stops it before a launch fails. An id that names the level runs.
    #[test]
    fn an_antigravity_model_needs_an_effort() {
        let mut r = Roster::builtin().unwrap();
        let t = r.teammates.get_mut("sonnet").unwrap();
        t.agent = HarnessKind::Antigravity;
        t.model = Some("gemini-3.8-flash".into());
        t.effort = None;
        t.permission_mode = None;
        let needs = |r: &Roster| r.check().iter().any(|p| p.contains("needs an effort"));
        assert!(
            r.check().iter().any(|p| p
                == "sonnet: antigravity model 'gemini-3.8-flash' needs an effort; agy refuses \
                    a bare model id without --effort (set effort: low, medium or high)"),
            "{:?}",
            r.check()
        );
        r.teammates.get_mut("sonnet").unwrap().effort = Some("low".into());
        assert!(!needs(&r), "{:?}", r.check());
        let t = r.teammates.get_mut("sonnet").unwrap();
        t.effort = None;
        t.model = Some("gemini-3.8-flash-low".into());
        assert!(!needs(&r), "{:?}", r.check());
        // Another agent picks its own default effort.
        let t = r.teammates.get_mut("sonnet").unwrap();
        t.agent = HarnessKind::Claude;
        t.model = Some("sonnet".into());
        assert!(!needs(&r), "{:?}", r.check());
    }

    /// skill-creator, orchestrate and Remote Control are the orchestrator's.
    /// No worker may carry them, and the orchestrator must deny both ways to
    /// start an agent of its own.
    #[test]
    fn orchestrator_only_capabilities_stay_with_the_orchestrator() {
        let mut r = Roster::builtin().unwrap();
        let orch = r.require("orchestrator").unwrap();
        assert!(orch.remote_control);
        assert_eq!(orch.permission_mode, Some(PermissionMode::Auto));
        assert!(orch.skills.iter().any(|s| s == "skill-creator"));
        assert!(r.check().is_empty(), "{:?}", r.check());

        let opus = r.teammates.get_mut("opus").unwrap();
        opus.skills.push("skill-creator".into());
        opus.skills.push("orchestrate".into());
        opus.remote_control = true;
        opus.plugin_skills
            .insert("skill-creator".into(), vec!["skill-creator".into()]);
        let problems = r.check();
        for expected in [
            "opus: skill 'skill-creator' belongs to the orchestrator only",
            "opus: skill 'orchestrate' belongs to the orchestrator only",
            "opus: remote_control belongs to the orchestrator only",
            "opus: plugin 'skill-creator' belongs to the orchestrator only",
        ] {
            assert!(
                problems.iter().any(|p| p == expected),
                "{expected}: {problems:?}"
            );
        }

        let mut r = Roster::builtin().unwrap();
        let orch = r.teammates.get_mut("orchestrator").unwrap();
        orch.disallowed_tools = vec!["Agent".into()];
        orch.allow_subagents = true;
        assert!(
            r.check().iter().any(|p| p.starts_with(
                "orchestrator: the orchestrator delegates only to workers; add RemoteTrigger"
            )),
            "{:?}",
            r.check()
        );
    }

    /// A fleet pane must not spawn subagents. The deny is the proof, and
    /// `allow_subagents` is the only way out.
    #[test]
    fn roster_check_demands_the_subagent_deny_on_every_claude_fleet_pane() {
        const MESSAGE: &str = "a fleet pane must not spawn subagents";

        // The shipped roster already carries it, on the 34 spawnable claude
        // teammates and on the orchestrator.
        let mut r = Roster::builtin().unwrap();
        assert!(r.check().is_empty(), "{:?}", r.check());
        let covered = r
            .teammates
            .values()
            .filter(|t| t.agent == HarnessKind::Claude && (!t.hidden || t.name == "orchestrator"))
            .count();
        assert_eq!(covered, 61, "the rule should cover 61 claude teammates");

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
        assert_eq!(worker.agent, HarnessKind::Claude);
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
    /// GW11: every `skills_when` skill must exist and be loadable, and the
    /// patterns follow `offer_when`'s rules.
    #[test]
    fn roster_check_holds_skills_when_to_the_catalog() {
        // One teammate's check: the whole roster's takes minutes in a debug
        // build.
        let r = Roster::builtin().unwrap();
        let mut opus = r.require("opus").unwrap().clone();
        opus.skills = vec!["tdd".into()];
        opus.disabled_skills = vec!["horch:debug".into()];
        opus.skills_when = [
            ("*.csproj".to_string(), vec!["missing-skill".into()]),
            ("src/*.cs".to_string(), vec!["tdd".into()]),
            ("*.sln".to_string(), vec![]),
            (
                "*.gd".to_string(),
                vec!["debug".into(), "orchestrate".into()],
            ),
        ]
        .into();
        let problems = r.check_teammate(&opus);
        for want in [
            "opus: skills_when '*.csproj' names unknown skill 'missing-skill'",
            "opus: skills_when pattern 'src/*.cs' contains a path separator",
            "opus: 'tdd' is both in skills and in skills_when 'src/*.cs'",
            "opus: skills_when '*.sln' names no skill",
            "opus: 'debug' is both in skills_when '*.gd' and in disabled_skills",
            "opus: skill 'orchestrate' belongs to the orchestrator only",
        ] {
            assert!(
                problems.iter().any(|p| p.contains(want)),
                "{want}: {problems:?}"
            );
        }

        let mut opus = r.require("opus").unwrap().clone();
        opus.skills_when = [("*.csproj".to_string(), vec!["debug".into()])].into();
        let problems = r.check_teammate(&opus);
        assert!(
            problems.iter().all(|p| !p.contains("skills_when")),
            "{problems:?}"
        );
        opus.disable_skills = true;
        assert!(r
            .check_teammate(&opus)
            .iter()
            .any(|p| p.contains("opus: skills_when adds skills, but skills cannot load")));
    }

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
