//! Portable phase catalogs. Only selected skill files are materialized per launch;
//! native harness loaders expose metadata and read bodies on demand.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::teammates::{
    expand_home, operator_enabled_plugins, operator_status_line, Agent, Phase, Teammate,
};

include!(concat!(env!("OUT_DIR"), "/bundled_skills.rs"));

#[derive(Debug, Deserialize)]
struct Metadata {
    name: String,
    description: String,
}

pub fn phase_skills(phase: Phase) -> &'static [&'static str] {
    match phase {
        Phase::Research => &["brainstorm", "research-codebase", "trace", "handoff"],
        Phase::Plan => &["create-plan", "pre-flight", "handoff"],
        Phase::Implementation => &["execute", "tdd", "debug", "check", "handoff"],
        Phase::Validation => &[
            "check",
            "code-analysis",
            "code-review",
            "security-review",
            "document",
            "handoff",
        ],
    }
}

fn catalog() -> Result<BTreeMap<String, (Metadata, usize)>> {
    let mut out = BTreeMap::new();
    for (path, bytes) in BUNDLED_SKILL_FILES {
        let Some(name) = path.strip_suffix("/SKILL.md") else {
            continue;
        };
        if name.contains('/') {
            continue;
        }
        let text = std::str::from_utf8(bytes)
            .context("skill is not UTF-8")?
            .replace("\r\n", "\n");
        let front = text
            .strip_prefix("---\n")
            .and_then(|s| s.split_once("\n---").map(|p| p.0))
            .with_context(|| format!("{path}: missing YAML frontmatter"))?;
        let meta: Metadata =
            serde_yaml::from_str(front).with_context(|| format!("{path}: invalid metadata"))?;
        if meta.name != name
            || name.is_empty()
            || name.starts_with('-')
            || name.ends_with('-')
            || name.contains("--")
            || !name
                .bytes()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
            || meta.description.trim().is_empty()
            || meta.description.len() > 1024
        {
            bail!("{path}: invalid skill name or description");
        }
        out.insert(name.to_owned(), (meta, bytes.len()));
    }
    Ok(out)
}

pub fn selected(teammate: &Teammate) -> Result<Vec<String>> {
    let mut names: BTreeSet<String> = teammate.skills.iter().cloned().collect();
    if let Some(phase) = teammate.phase {
        names.extend(phase_skills(phase).iter().map(|s| (*s).to_owned()));
    }
    let known = catalog()?;
    for name in &names {
        if !known.contains_key(name) {
            bail!(
                "teammate '{}': unknown bundled skill '{name}'",
                teammate.name
            );
        }
    }
    if !names.is_empty() && (teammate.disable_skills || teammate.agent == Agent::None) {
        bail!(
            "teammate '{}': selected skills cannot load with disabled skills or no agent",
            teammate.name
        );
    }
    Ok(names.into_iter().collect())
}

/// Check platform support before allocating a pane or writing shared rules.
pub fn ensure_supported(teammate: &Teammate) -> Result<()> {
    let names = selected(teammate)?;
    if cfg!(windows) && !names.is_empty() && teammate.agent == Agent::Codex {
        bail!("Codex phase skills require a private CODEX_HOME; use WSL on Windows, or explicitly set phase: null and skills: [] for the legacy shared-rules launcher");
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
#[derive(Debug)]
pub struct Bundle {
    root: PathBuf,
    names: Vec<String>,
}

impl Bundle {
    pub fn install(state_root: &Path, teammate: &Teammate) -> Result<Option<Self>> {
        ensure_supported(teammate)?;
        let names = selected(teammate)?;
        if names.is_empty() {
            return Ok(None);
        }
        let parent = state_root.join("skill-bundles");
        std::fs::create_dir_all(&parent)?;
        let root = parent.canonicalize()?.join(crate::mint_uuid());
        std::fs::create_dir(&root)?;
        let bundle = Self { root, names };
        for (path, bytes) in BUNDLED_SKILL_FILES {
            let name = path.split('/').next().unwrap_or_default();
            if !bundle.names.iter().any(|s| s == name) {
                continue;
            }
            let target = bundle.skills_dir().join(path);
            std::fs::create_dir_all(target.parent().expect("skill path has parent"))?;
            std::fs::write(target, bytes)?;
        }
        std::fs::create_dir_all(bundle.root.join(".claude-plugin"))?;
        std::fs::write(
            bundle.root.join(".claude-plugin/plugin.json"),
            r#"{"name":"horch","description":"Phase-selected fleet skills","version":"0.1.0"}"#,
        )?;
        Ok(Some(bundle))
    }

    pub fn skills_dir(&self) -> PathBuf {
        self.root.join("skills")
    }

    /// Add native discovery flags before the builder appends its positional prompt.
    pub fn configure(&self, teammate: &Teammate) -> Result<Teammate> {
        let mut adjusted = teammate.clone();
        match teammate.agent {
            Agent::Claude => {
                // plugin_dirs precede scalar flags in the Claude builder, which
                // fences variadic --plugin-dir parsing away from the briefing.
                adjusted
                    .plugin_dirs
                    .push(self.root.to_string_lossy().into_owned());
                let settings = self.claude_settings(teammate)?;
                adjusted.settings = Some(settings.to_string());
            }
            Agent::Pi | Agent::Prime => adjusted
                .args
                .extend(native_args(teammate.agent, &self.root)),
            Agent::Codex | Agent::Opencode => {}
            Agent::None => bail!("skills need an agent harness"),
        }
        Ok(adjusted)
    }

    fn claude_settings(&self, teammate: &Teammate) -> Result<Value> {
        let mut settings = match &teammate.settings {
            Some(source) => {
                let text = if source.trim_start().starts_with('{') {
                    source.clone()
                } else {
                    std::fs::read_to_string(expand_home(source))
                        .context("reading teammate settings")?
                };
                serde_json::from_str::<Value>(&text).context("invalid teammate settings JSON")?
            }
            None => json!({}),
        };
        let obj = settings
            .as_object_mut()
            .context("teammate settings must be a JSON object")?;
        obj.entry("disableBundledSkills").or_insert(json!(true));
        obj.entry("disableWorkflows").or_insert(json!(true));
        crate::launch::overlay_skill_switches(teammate, obj)?;
        if !teammate.inherit_plugins {
            let plugins = obj
                .entry("enabledPlugins")
                .or_insert(json!({}))
                .as_object_mut()
                .context("enabledPlugins must be an object")?;
            for name in operator_enabled_plugins() {
                plugins.insert(name, json!(false));
            }
        }
        if teammate.setting_sources.is_some() && !obj.contains_key("statusLine") {
            if let Some(status) = operator_status_line() {
                obj.insert("statusLine".into(), status);
            }
        }
        Ok(settings)
    }

    pub fn apply_env(&self, cmd: &mut std::process::Command, teammate: &Teammate) -> Result<()> {
        if teammate.agent == Agent::Opencode {
            let inherited = teammate
                .env
                .get("OPENCODE_CONFIG_CONTENT")
                .cloned()
                .or_else(|| std::env::var("OPENCODE_CONFIG_CONTENT").ok());
            cmd.env(
                "OPENCODE_CONFIG_CONTENT",
                opencode_config(inherited.as_deref(), &self.skills_dir())?,
            );
        }
        Ok(())
    }

    pub fn briefing(&self, agent: Agent, phase: Option<Phase>) -> String {
        let names: Vec<String> = self
            .names
            .iter()
            .map(|s| {
                if agent == Agent::Claude {
                    format!("horch:{s}")
                } else {
                    s.clone()
                }
            })
            .collect();
        format!("\n\nFleet skill phase: {}. Available native skills: {}. Load only the skill matching your current step; do not read every skill at startup. If a same-named ambient skill exists, use the fleet copy under {}. Skills do not change tool permissions. Report unresolved dependencies through horch tell orchestrator.\n",
            phase.map(|p| p.to_string()).unwrap_or_else(|| "custom".into()), names.join(", "), self.skills_dir().display())
    }
}

impl Drop for Bundle {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn native_args(agent: Agent, root: &Path) -> Vec<String> {
    match agent {
        Agent::Claude => vec!["--plugin-dir".into(), root.to_string_lossy().into_owned()],
        Agent::Pi | Agent::Prime => vec![
            "--skill".into(),
            root.join("skills").to_string_lossy().into_owned(),
        ],
        _ => Vec::new(),
    }
}

fn opencode_config(inherited: Option<&str>, skills: &Path) -> Result<String> {
    let mut value: Value = match inherited {
        Some(s) => serde_json::from_str(s).context("invalid OPENCODE_CONFIG_CONTENT JSON")?,
        None => json!({}),
    };
    let object = value
        .as_object_mut()
        .context("OPENCODE_CONFIG_CONTENT must be an object")?;
    let config = object
        .entry("skills")
        .or_insert(json!({}))
        .as_object_mut()
        .context("skills config must be an object")?;
    let paths = config
        .entry("paths")
        .or_insert(json!([]))
        .as_array_mut()
        .context("skills.paths must be an array")?;
    let path = json!(skills);
    if !paths.contains(&path) {
        paths.push(path);
    }
    Ok(value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skills_native_loaders_receive_explicit_paths() {
        let root = Path::new("/tmp/a bundle");
        assert_eq!(
            native_args(Agent::Claude, root),
            ["--plugin-dir", "/tmp/a bundle"]
        );
        for agent in [Agent::Pi, Agent::Prime] {
            assert_eq!(
                native_args(agent, root),
                [
                    "--skill".to_owned(),
                    root.join("skills").to_string_lossy().into_owned()
                ]
            );
        }
        assert!(native_args(Agent::Codex, root).is_empty());
        assert!(native_args(Agent::Opencode, root).is_empty());
    }

    #[test]
    fn skills_opencode_overlay_preserves_provider_and_denials() {
        let before = r#"{"provider":{"private":{"name":"keep"}},"permission":{"bash":"deny"},"skills":{"paths":["old"],"urls":["https://example.com"]}}"#;
        let actual: Value =
            serde_json::from_str(&opencode_config(Some(before), Path::new("/new skills")).unwrap())
                .unwrap();
        assert_eq!(actual["provider"]["private"]["name"], "keep");
        assert_eq!(actual["permission"]["bash"], "deny");
        assert_eq!(actual["skills"]["paths"], json!(["old", "/new skills"]));
        assert_eq!(actual["skills"]["urls"], json!(["https://example.com"]));
        for invalid in [
            "oops",
            "[]",
            r#"{"skills":false}"#,
            r#"{"skills":{"paths":false}}"#,
        ] {
            assert!(opencode_config(Some(invalid), Path::new("/x")).is_err());
        }
    }

    #[test]
    fn skills_catalog_is_portable_and_every_phase_resolves() {
        let catalog = catalog().unwrap();
        assert_eq!(catalog.len(), 14);
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
        let brief = a.briefing(Agent::Claude, t.phase);
        assert!(brief.contains("horch:create-plan"));
        assert!(!brief.contains("# Create"));
        drop(a);
        assert!(!gone.exists());
        assert!(b.skills_dir().join("create-plan/SKILL.md").exists());
    }

    #[test]
    fn skills_claude_overlay_retains_explicit_settings() {
        let tmp = tempfile::tempdir().unwrap();
        let t = Teammate { phase: Some(Phase::Plan), settings: Some(r#"{"permissions":{"deny":["Bash(rm *)"]},"statusLine":{"type":"command","command":"keep"}}"#.into()), ..Teammate::default() };
        let bundle = Bundle::install(tmp.path(), &t).unwrap().unwrap();
        let configured = bundle.configure(&t).unwrap();
        let value: Value = serde_json::from_str(configured.settings.as_ref().unwrap()).unwrap();
        assert_eq!(value["permissions"]["deny"], json!(["Bash(rm *)"]));
        assert_eq!(value["statusLine"]["command"], "keep");
        assert_eq!(value["disableBundledSkills"], true);
        assert_eq!(value["disableWorkflows"], true);
        assert_eq!(value["syncClaudeAiSkills"], false);
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
        let value: Value =
            serde_json::from_str(bundle.configure(&t).unwrap().settings.as_ref().unwrap()).unwrap();
        assert_eq!(value["syncClaudeAiSkills"], false);
        assert_eq!(
            value["skillOverrides"],
            json!({"keep": "name-only", "dev-prime": "off"})
        );

        let opted_in = Teammate {
            inherit_claudeai_skills: true,
            disabled_skills: Vec::new(),
            ..t
        };
        let value: Value = serde_json::from_str(
            bundle
                .configure(&opted_in)
                .unwrap()
                .settings
                .as_ref()
                .unwrap(),
        )
        .unwrap();
        assert!(value.get("syncClaudeAiSkills").is_none(), "{value}");
        assert_eq!(value["skillOverrides"], json!({"keep": "name-only"}));
    }
}
