//! Portable phase catalogs. Only selected skill files are materialized per launch;
//! native harness loaders expose metadata and read bodies on demand.

use std::collections::BTreeMap;
#[cfg(test)]
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde_json::{json, Value};

use crate::roster::{Phase, Teammate};

pub mod activation;
pub mod briefing;
pub mod catalog;
pub mod materialize;
pub mod selection;

pub use activation::{plan_activation, InvocationPolicy, ResolvedSkillRef, SkillActivationPlan};
pub use briefing::BriefingContext;
pub use catalog::{CatalogEntry, CatalogSource, Provenance, SkillCatalog, SkillVersion};
pub use materialize::MaterializedSkills;
pub use selection::{phase_skills, selected, selected_in};

include!(concat!(env!("OUT_DIR"), "/bundled_skills.rs"));

#[derive(Debug)]
struct Metadata {
    name: String,
    description: String,
}

/// The bundled catalog in the shape the legacy callers read.
fn catalog() -> Result<BTreeMap<String, (Metadata, usize)>> {
    Ok(SkillCatalog::bundled()?
        .entries()
        .map(|e| {
            let meta = Metadata {
                name: e.id.to_string(),
                description: e.description.clone(),
            };
            (e.id.to_string(), (meta, e.skill_file_bytes))
        })
        .collect())
}

/// Check platform support before allocating a pane or writing shared rules.
/// Names are checked against `catalog`, which may include installed
/// marketplace skills.
pub fn ensure_supported_in(teammate: &Teammate, catalog: &SkillCatalog) -> Result<()> {
    if !selected_in(teammate, catalog)?.is_empty() {
        teammate.agent.adapter().ensure_skills_supported()?;
    }
    Ok(())
}

/// Inspect costs without launching a harness. Bytes/4 is an estimate, not a tokenizer.
pub fn describe(phase: Option<Phase>) -> Result<Value> {
    let all = catalog()?;
    let names: Vec<&str> = match phase {
        Some(p) => phase_skills(p).to_vec(),
        None => all.keys().map(String::as_str).collect(),
    };
    let mut items = Vec::new();
    let mut metadata_bytes = 0;
    for name in names {
        let (meta, bytes) = all
            .get(name)
            .with_context(|| format!("missing bundled skill '{name}'"))?;
        metadata_bytes += meta.name.len() + meta.description.len();
        items.push(
            json!({"name": meta.name, "description": meta.description, "skill_file_bytes": bytes}),
        );
    }
    Ok(
        json!({"phase": phase, "skills": items, "metadata_bytes": metadata_bytes,
        "metadata_tokens_estimate": metadata_bytes.div_ceil(4),
        "estimate_note": "Names and descriptions only; bytes/4 estimate excludes harness wrappers, paths, ambient skills and tools. Full bodies load on demand."}),
    )
}

/// An owned launch directory. Dropping it only deletes files we created.
///
/// It holds exactly the activated skills. How a CLI discovers them is the
/// harness's business: the teammate's harness adapter exposes the bundle
/// (`expose_skills`, `expose_skills_env`).
#[derive(Debug)]
pub struct Bundle {
    root: PathBuf,
    catalog: SkillCatalog,
    plan: SkillActivationPlan,
    _files: MaterializedSkills,
}

impl Bundle {
    /// The compiled-in catalog, under a fresh directory name.
    pub fn install(state_root: &Path, teammate: &Teammate) -> Result<Option<Self>> {
        Self::install_from(
            state_root,
            teammate,
            SkillCatalog::bundled()?,
            &crate::mint_uuid(),
        )
    }

    /// Plan `teammate`'s skills against `catalog` and materialize the
    /// activated ones under `<state_root>/skill-bundles/<execution_id>/`.
    /// `None` when nothing is activated.
    pub fn install_from(
        state_root: &Path,
        teammate: &Teammate,
        catalog: SkillCatalog,
        execution_id: &str,
    ) -> Result<Option<Self>> {
        let plan = plan_activation(teammate, teammate.phase, &catalog)?;
        if !plan.activated.is_empty() {
            teammate.agent.adapter().ensure_skills_supported()?;
        }
        let Some(files) =
            MaterializedSkills::materialize(&plan, &catalog, state_root, execution_id)?
        else {
            return Ok(None);
        };
        Ok(Some(Self {
            root: files.root.clone(),
            catalog,
            plan,
            _files: files,
        }))
    }

    /// The activation plan this bundle materialized.
    pub fn plan(&self) -> &SkillActivationPlan {
        &self.plan
    }

    /// `<state_root>/skill-bundles/<execution_id>/`.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// One directory per activated skill, and nothing else.
    pub fn skills_dir(&self) -> PathBuf {
        self.root.join("skills")
    }

    /// The skill paragraph prepended to a worker's briefing, with plugin
    /// skills resolved under `home`; see `briefing::render`.
    pub fn briefing_in(&self, teammate: &Teammate, home: Option<&Path>) -> String {
        let namespace = teammate.agent.adapter().skill_namespace();
        let mut plugin_lines = Vec::new();
        // Plugin skills exist only where skills load as plugins. A plugin
        // that does not resolve is reported by `--check`; the briefing must
        // not fail a launch over a description.
        if namespace.is_some() {
            if let Ok(plugins) = crate::harness::claude_plugins::resolve_all_in(teammate, home) {
                for (plugin, wanted) in plugins {
                    for skill in wanted {
                        let description = plugin.description(&skill).unwrap_or_default();
                        plugin_lines.push(format!("- {}:{skill}: {description}", plugin.name));
                    }
                }
            }
        }
        briefing::render(
            &self.plan,
            &self.catalog,
            &BriefingContext {
                phase: teammate.phase,
                declared: &teammate.skills,
                namespace,
                plugin_lines: &plugin_lines,
                skills_dir: &self.skills_dir(),
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skills_catalog_is_portable_and_every_phase_resolves() {
        let catalog = catalog().unwrap();
        assert_eq!(catalog.len(), 16);
        // `orchestrate` and `skill-creator` are attached by name, to the
        // orchestrators only, and belong to no phase catalog. Adding either to
        // `Phase::Plan` would hand it to `staff-engineer` and every other
        // plan-phase teammate, which is the opposite of what it is for.
        let phased: BTreeSet<&str> = [
            Phase::Research,
            Phase::Plan,
            Phase::Implementation,
            Phase::Validation,
        ]
        .into_iter()
        .flat_map(|p| phase_skills(p).iter().copied())
        .collect();
        let by_name_only: Vec<&str> = catalog
            .keys()
            .map(String::as_str)
            .filter(|n| !phased.contains(n))
            .collect();
        assert_eq!(by_name_only, crate::roster::ORCHESTRATOR_ONLY_SKILLS);
        for phase in [
            Phase::Research,
            Phase::Plan,
            Phase::Implementation,
            Phase::Validation,
        ] {
            let t = Teammate {
                phase: Some(phase),
                ..Teammate::default()
            };
            assert!(!selected(&t).unwrap().is_empty());
        }
        for (path, body) in BUNDLED_SKILL_FILES {
            assert!(!Path::new(path).is_absolute());
            assert!(!path.split('/').any(|p| p == ".."));
            if path.ends_with("SKILL.md") {
                let body = std::str::from_utf8(body).unwrap();
                assert!(!body.contains("${CLAUDE_PLUGIN_ROOT}"), "{path}");
                assert!(!body.contains("/Users/"), "{path}");
            }
        }
    }

    #[test]
    fn skills_phase_selection_is_scoped_and_explicit_skills_are_deduplicated() {
        let t = Teammate {
            phase: Some(Phase::Plan),
            skills: vec!["create-plan".into(), "trace".into()],
            ..Teammate::default()
        };
        let names = selected(&t).unwrap();
        assert_eq!(names, ["create-plan", "handoff", "pre-flight", "trace"]);
        assert!(!names.contains(&"execute".into()));
        let bad = Teammate {
            skills: vec!["../escape".into()],
            ..Teammate::default()
        };
        assert!(selected(&bad).is_err());
        let disabled = Teammate {
            disable_skills: true,
            ..t
        };
        assert!(selected(&disabled).is_err());
    }

    #[test]
    fn skills_bundles_are_independent_and_cleanup_only_their_own_files() {
        let tmp = tempfile::tempdir().unwrap();
        let t = Teammate {
            phase: Some(Phase::Plan),
            ..Teammate::default()
        };
        let a = Bundle::install(tmp.path(), &t).unwrap().unwrap();
        let b = Bundle::install(tmp.path(), &t).unwrap().unwrap();
        let gone = a.root.clone();
        assert_ne!(a.root, b.root);
        assert!(a.skills_dir().join("create-plan/SKILL.md").exists());
        assert!(!a.skills_dir().join("execute").exists());
        let brief = a.briefing_in(&t, None);
        assert!(brief.contains("horch:create-plan"));
        assert!(!brief.contains("# Create"));
        assert!(
            !brief.contains("expected to use"),
            "no skills: listed: {brief}"
        );
        drop(a);
        assert!(!gone.exists());
        assert!(b.skills_dir().join("create-plan/SKILL.md").exists());
    }

    /// A teammate's own skills are named as expected, with descriptions; the
    /// rest of its phase catalog stays available by name only.
    #[test]
    fn skills_briefing_names_expected_skills_with_descriptions() {
        let tmp = tempfile::tempdir().unwrap();
        let roster = crate::roster::Roster::builtin().unwrap();
        let t = roster.require("backend-developer").unwrap();
        let bundle = Bundle::install(tmp.path(), t).unwrap().unwrap();
        let brief = bundle.briefing_in(t, None);
        let all = catalog().unwrap();
        for skill in ["tdd", "security-review"] {
            let description = all[skill]
                .0
                .description
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            assert!(
                brief.contains(&format!("- horch:{skill}: {description}")),
                "{skill} missing from:\n{brief}"
            );
        }
        let also = brief
            .lines()
            .find(|l| l.starts_with("Also available in this phase: "))
            .unwrap_or_else(|| panic!("{brief}"));
        assert!(also.contains("horch:execute"), "{also}");
        assert!(
            !also.contains("horch:tdd"),
            "an expected skill is not repeated: {also}"
        );
        assert!(
            !brief.contains(&all["execute"].0.description),
            "only expected skills carry descriptions"
        );

        // Codex names skills bare.
        let codex = roster.require("codex-reviewer").unwrap();
        if !cfg!(windows) {
            let bundle = Bundle::install(tmp.path(), codex).unwrap().unwrap();
            assert!(bundle
                .briefing_in(codex, None)
                .contains("\n- code-review: "));
        }
    }

    #[test]
    fn skills_claude_overlay_retains_explicit_settings() {
        let tmp = tempfile::tempdir().unwrap();
        let t = Teammate { phase: Some(Phase::Plan), settings: Some(r#"{"permissions":{"deny":["Bash(rm *)"]},"statusLine":{"type":"command","command":"keep"}}"#.into()), ..Teammate::default() };
        let bundle = Bundle::install(tmp.path(), &t).unwrap().unwrap();
        let configured = t.agent.adapter().expose_skills(&t, &bundle, None).unwrap();
        let value: Value = serde_json::from_str(configured.settings.as_ref().unwrap()).unwrap();
        assert_eq!(value["permissions"]["deny"], json!(["Bash(rm *)"]));
        assert_eq!(value["statusLine"]["command"], "keep");
        assert_eq!(value["disableBundledSkills"], true);
        assert_eq!(value["disableWorkflows"], true);
        assert_eq!(value["syncClaudeAiSkills"], false);
    }

    /// The orchestrator's playbook reaches the orchestrator and nobody else.
    /// Both teammates sit on `phase: plan`, so the only difference between
    /// these two lists is the `skills: [orchestrate, skill-creator]` line in
    /// `teammates/orchestrator.md`. skill-creator is the Claude flavor's only.
    #[test]
    fn skills_orchestrate_is_attached_by_name_to_the_orchestrator_only() {
        let roster = crate::roster::Roster::builtin().unwrap();
        assert_eq!(
            selected(roster.require("orchestrator").unwrap()).unwrap(),
            [
                "create-plan",
                "handoff",
                "orchestrate",
                "pre-flight",
                "skill-creator"
            ]
        );
        assert_eq!(
            selected(roster.require("orchestrator-codex").unwrap()).unwrap(),
            ["create-plan", "handoff", "orchestrate", "pre-flight"]
        );
        assert_eq!(
            selected(roster.require("staff-engineer").unwrap()).unwrap(),
            ["create-plan", "handoff", "pre-flight"]
        );
    }

    /// Fleet panes take this path, not the plain overlay in `launch.rs`, so the
    /// skill switches have to land here too: into the teammate's own
    /// `skillOverrides`, and out entirely on opt-in.
    #[test]
    fn skills_claude_overlay_carries_the_skill_switches() {
        let tmp = tempfile::tempdir().unwrap();
        let t = Teammate {
            phase: Some(Phase::Plan),
            settings: Some(r#"{"skillOverrides":{"keep":"name-only"}}"#.into()),
            disabled_skills: vec!["dev-prime".into()],
            ..Teammate::default()
        };
        assert!(t.inherit_plugins, "the switch must not depend on plugins");
        let bundle = Bundle::install(tmp.path(), &t).unwrap().unwrap();
        let value: Value = serde_json::from_str(
            t.agent
                .adapter()
                .expose_skills(&t, &bundle, None)
                .unwrap()
                .settings
                .as_ref()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(value["syncClaudeAiSkills"], false);
        assert_eq!(value["remoteControlAtStartup"], false);
        assert_eq!(
            value["skillOverrides"],
            json!({
                "keep": "name-only",
                "dev-prime": "off",
                "skill-creator": "off",
                "anthropic-skills:skill-creator": "off"
            })
        );

        let opted_in = Teammate {
            inherit_claudeai_skills: true,
            disabled_skills: Vec::new(),
            ..t
        };
        let value: Value = serde_json::from_str(
            opted_in
                .agent
                .adapter()
                .expose_skills(&opted_in, &bundle, None)
                .unwrap()
                .settings
                .as_ref()
                .unwrap(),
        )
        .unwrap();
        assert!(value.get("syncClaudeAiSkills").is_none(), "{value}");
        assert_eq!(
            value["skillOverrides"],
            json!({
                "keep": "name-only",
                "skill-creator": "off",
                "anthropic-skills:skill-creator": "off"
            })
        );
    }
}
