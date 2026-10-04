//! Everything `horch teammates --check` refuses, judged without launching
//! anything: one teammate's fields, the roster as a whole, and the static
//! fallback rules (design section 13.3).

use anyhow::{bail, Result};

use super::operator::expand_home;
use super::teammate::HEADLESS_ONLY;
use super::{
    effort_problem, reserved_tier, ExecRule, PermissionMode, Roster, Teammate,
    BRIEF_DESCRIPTION_MAX, FLEET_ORCHESTRATORS, ORCHESTRATOR_DENIED_TOOLS,
    ORCHESTRATOR_ONLY_SKILLS,
};
use crate::harness::HarnessKind;
use crate::routing::decision::merge;
use crate::routing::eligible::trains_on_input;
use crate::routing::quota;
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
                .and_then(|c| c.with_operator_skills(t, self.home.as_deref()));
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
        // Codex, OpenCode, pi and Prime are elsewhere; see
        // `ai_docs/reports/no-subagents.md`.
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
        // A teammate that brings its own settings file takes over the status
        // line too; one without a statusLine would be the only pane in the
        // fleet with none.
        if let Some(path) = &t.settings {
            let path = expand_home(path, self.home.as_deref());
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
        let mut problems = Vec::new();
        for t in self.teammates.values() {
            problems.extend(self.check_teammate(t));
        }
        problems.extend(fallback_problems(self));
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
                | "ue-tech-lead" => Some(Phase::Plan),
                "architect-reviewer" | "qa-engineer" | "codex-reviewer" | "design-critic"
                | "ue-qa-engineer" | "ue-code-reviewer" | "swift-reviewer"
                | "swift-qa-engineer" => Some(Phase::Validation),
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
        t.model = Some("gemini-3-1-pro".into());
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
        assert_eq!(covered, 38, "the rule should cover 38 claude teammates");

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
