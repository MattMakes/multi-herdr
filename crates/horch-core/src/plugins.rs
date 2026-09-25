//! Claude Code plugins whose skills a teammate reinforces by name.
//!
//! A plugin often ships many skills while a teammate should use two of them.
//! `plugin_skills: { <plugin>: [a, b] }` does three things with that:
//! - names `a` and `b`, with their descriptions, in the worker's briefing as
//!   skills it is expected to use (`skills.rs`, `Bundle::briefing`);
//! - switches every other skill of that plugin off for the session, as
//!   `skillOverrides` entries `"<plugin>:<skill>": "off"`;
//! - keeps an installed plugin enabled even when `inherit_plugins: false`
//!   switches the operator's plugins off.
//!
//! A plugin is found in the teammate's `plugin_dirs` first, then in the
//! operator's `~/.claude/plugins/installed_plugins.json`. Its skills are the
//! `skills/*/SKILL.md` files under its root.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use serde_json::Value;

use crate::teammates::{expand_home, Teammate};

/// One plugin, resolved to its skills.
#[derive(Debug, Clone, PartialEq)]
pub struct Plugin {
    pub name: String,
    /// The `name@marketplace` key when the plugin came from the operator's
    /// installed plugins, so the overlay can keep it enabled. `None` when it
    /// came from a `plugin_dirs` entry, which `--plugin-dir` already loads.
    pub installed_key: Option<String>,
    pub root: PathBuf,
    /// `(name, description)` of every skill the plugin ships, sorted by name.
    pub skills: Vec<(String, String)>,
}

impl Plugin {
    pub fn description(&self, skill: &str) -> Option<&str> {
        self.skills
            .iter()
            .find(|(name, _)| name == skill)
            .map(|(_, d)| d.as_str())
    }
}

/// Resolve `plugin` for `teammate`, reading the operator's installed plugins.
pub fn resolve(teammate: &Teammate, plugin: &str) -> Result<Plugin> {
    let dirs: Vec<PathBuf> = teammate.plugin_dirs.iter().map(|d| expand_home(d)).collect();
    resolve_in(&dirs, installed_plugins().as_ref(), plugin)
}

/// Every `plugin_skills` entry of `teammate`, resolved and checked: each
/// plugin exists and ships each named skill.
pub fn resolve_all(teammate: &Teammate) -> Result<Vec<(Plugin, Vec<String>)>> {
    let mut out = Vec::new();
    for (plugin, wanted) in &teammate.plugin_skills {
        if wanted.is_empty() {
            bail!(
                "plugin_skills.{plugin}: list at least one skill; to switch the whole \
                 plugin off, leave it out of plugin_skills"
            );
        }
        let resolved = resolve(teammate, plugin)?;
        for skill in wanted {
            if resolved.description(skill).is_none() {
                let have: Vec<&str> = resolved.skills.iter().map(|(n, _)| n.as_str()).collect();
                bail!(
                    "plugin_skills.{plugin}: no skill '{skill}' in {} (it ships: {})",
                    resolved.root.display(),
                    have.join(", ")
                );
            }
        }
        out.push((resolved, wanted.clone()));
    }
    Ok(out)
}

/// The pure half of [`resolve`]: explicit plugin directories and the parsed
/// `installed_plugins.json`.
pub fn resolve_in(dirs: &[PathBuf], installed: Option<&Value>, plugin: &str) -> Result<Plugin> {
    for dir in dirs {
        if plugin_name(dir).as_deref() == Some(plugin) {
            return Ok(Plugin {
                name: plugin.to_string(),
                installed_key: None,
                root: dir.clone(),
                skills: read_skills(dir)?,
            });
        }
    }
    if let Some((key, root)) = installed.and_then(|v| installed_root(v, plugin)) {
        return Ok(Plugin {
            name: plugin.to_string(),
            installed_key: Some(key),
            skills: read_skills(&root)?,
            root,
        });
    }
    bail!(
        "plugin '{plugin}' is neither in plugin_dirs nor installed \
         (~/.claude/plugins/installed_plugins.json)"
    )
}

/// A plugin directory's name: `.claude-plugin/plugin.json`'s `name`, else the
/// directory's own name.
fn plugin_name(dir: &Path) -> Option<String> {
    let manifest = std::fs::read_to_string(dir.join(".claude-plugin/plugin.json")).ok();
    manifest
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|v| v.get("name").and_then(|n| n.as_str()).map(str::to_owned))
        .or_else(|| dir.file_name().map(|n| n.to_string_lossy().into_owned()))
}

/// The operator's installed-plugins registry, if it exists and parses.
pub fn installed_plugins() -> Option<Value> {
    let home = std::env::var_os("HOME")?;
    let path = PathBuf::from(home).join(".claude/plugins/installed_plugins.json");
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

/// Find `plugin` in `installed_plugins.json`. Keys are `name@marketplace`;
/// the value is an install record, or (version 2 of the file) a list of them,
/// each carrying an `installPath`.
fn installed_root(installed: &Value, plugin: &str) -> Option<(String, PathBuf)> {
    let plugins = installed.get("plugins")?.as_object()?;
    for (key, value) in plugins {
        let name = key.split('@').next().unwrap_or(key);
        if name != plugin && key != plugin {
            continue;
        }
        let records: Vec<&Value> = match value {
            Value::Array(list) => list.iter().collect(),
            other => vec![other],
        };
        for record in records {
            if let Some(path) = record.get("installPath").and_then(|p| p.as_str()) {
                return Some((key.clone(), expand_home(path)));
            }
        }
    }
    None
}

#[derive(Deserialize)]
struct SkillFront {
    name: Option<String>,
    description: Option<String>,
}

/// `(name, description)` for each `skills/<dir>/SKILL.md` under `root`.
fn read_skills(root: &Path) -> Result<Vec<(String, String)>> {
    let dir = root.join("skills");
    let entries =
        std::fs::read_dir(&dir).with_context(|| format!("reading {}", dir.display()))?;
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let file = entry.path().join("SKILL.md");
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        let text = text.replace("\r\n", "\n");
        let front: SkillFront = text
            .strip_prefix("---\n")
            .and_then(|s| s.split_once("\n---").map(|p| p.0))
            .and_then(|f| serde_yaml::from_str(f).ok())
            .unwrap_or(SkillFront {
                name: None,
                description: None,
            });
        let name = front
            .name
            .unwrap_or_else(|| entry.file_name().to_string_lossy().into_owned());
        let description = front
            .description
            .unwrap_or_default()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        out.push((name, description));
    }
    out.sort();
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plugin(root: &Path, name: &str, skills: &[(&str, &str)]) -> PathBuf {
        let dir = root.join(name);
        std::fs::create_dir_all(dir.join(".claude-plugin")).unwrap();
        std::fs::write(
            dir.join(".claude-plugin/plugin.json"),
            format!(r#"{{"name":"{name}"}}"#),
        )
        .unwrap();
        for (skill, description) in skills {
            let s = dir.join("skills").join(skill);
            std::fs::create_dir_all(&s).unwrap();
            std::fs::write(
                s.join("SKILL.md"),
                format!("---\nname: {skill}\ndescription: {description}\n---\nbody\n"),
            )
            .unwrap();
        }
        dir
    }

    #[test]
    fn a_plugin_resolves_from_plugin_dirs_first_then_the_installed_registry() {
        let tmp = tempfile::tempdir().unwrap();
        let local = plugin(tmp.path(), "code", &[("review", "Review a diff."), ("lint", "Lint.")]);
        let found = resolve_in(&[local.clone()], None, "code").unwrap();
        assert_eq!(found.installed_key, None);
        assert_eq!(
            found.skills,
            vec![
                ("lint".to_string(), "Lint.".to_string()),
                ("review".to_string(), "Review a diff.".to_string())
            ]
        );

        let installed_dir = plugin(&tmp.path().join("cache"), "dev", &[("tdd", "Red, green.")]);
        for registry in [
            serde_json::json!({"plugins": {"dev@market": {"installPath": installed_dir}}}),
            serde_json::json!({"version": 2, "plugins": {"dev@market": [{"scope": "user", "installPath": installed_dir}]}}),
        ] {
            let found = resolve_in(&[local.clone()], Some(&registry), "dev").unwrap();
            assert_eq!(found.installed_key.as_deref(), Some("dev@market"));
            assert_eq!(found.description("tdd"), Some("Red, green."));
        }

        let err = resolve_in(&[local], None, "missing").unwrap_err().to_string();
        assert!(err.contains("plugin 'missing' is neither"), "{err}");
    }
}
