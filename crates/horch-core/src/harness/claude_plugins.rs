//! Claude Code plugins whose skills a teammate reinforces by name.
//!
//! A plugin often ships many skills while a teammate should use two of them.
//! `plugin_skills: { <plugin>: [a, b] }` does three things with that:
//! - names `a` and `b`, with their descriptions, in the worker's briefing as
//!   skills it is expected to use (`skills.rs`, `Bundle::briefing`);
//! - loads a filtered copy of the plugin from the launch's skills bundle
//!   (`<bundle>/plugins/<plugin>/`, [`materialize_filtered`]) with
//!   `--plugin-dir`: the plugin minus every skill and command the teammate
//!   did not name. Claude Code ignores `skillOverrides` for a plugin skill
//!   (2.1.289), so only a copy without the other skills hides them;
//! - switches the installed original off (`"<plugin>@<marketplace>":
//!   false`) and the copy on (`"<plugin>@inline": true`), whatever
//!   `inherit_plugins` says.
//!
//! A plugin is found in the teammate's `plugin_dirs` first, then in the
//! operator's `~/.claude/plugins/installed_plugins.json`. Its skills are the
//! `skills/*/SKILL.md` files under its root.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use serde_json::Value;

use crate::roster::expand_home;
use crate::roster::Teammate;

/// One plugin, resolved to its skills.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Plugin {
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
    pub(crate) fn description(&self, skill: &str) -> Option<&str> {
        self.skills
            .iter()
            .find(|(name, _)| name == skill)
            .map(|(_, d)| d.as_str())
    }
}

/// Resolve `plugin` for `teammate`, reading the operator's installed plugins
/// under `home` (`$HOME`).
pub(crate) fn resolve(teammate: &Teammate, plugin: &str, home: Option<&Path>) -> Result<Plugin> {
    let dirs: Vec<PathBuf> = teammate
        .plugin_dirs
        .iter()
        .map(|d| expand_home(d, home))
        .collect();
    resolve_in(&dirs, installed_plugins_in(home).as_ref(), plugin, home)
}

/// Every `plugin_skills` entry of `teammate`, resolved and checked: each
/// plugin exists and ships each named skill. `home` is `$HOME`.
pub(crate) fn resolve_all_in(
    teammate: &Teammate,
    home: Option<&Path>,
) -> Result<Vec<(Plugin, Vec<String>)>> {
    let mut out = Vec::new();
    for (plugin, wanted) in &teammate.plugin_skills {
        if wanted.is_empty() {
            bail!(
                "plugin_skills.{plugin}: list at least one skill; to switch the whole \
                 plugin off, leave it out of plugin_skills"
            );
        }
        let resolved = resolve(teammate, plugin, home)?;
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
/// `installed_plugins.json`, whose `~/` paths expand against `home`.
pub(crate) fn resolve_in(
    dirs: &[PathBuf],
    installed: Option<&Value>,
    plugin: &str,
    home: Option<&Path>,
) -> Result<Plugin> {
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
    if let Some((key, root)) = installed.and_then(|v| installed_root(v, plugin, home)) {
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

/// The operator's installed-plugins registry under `home` (`$HOME`), if it
/// exists and parses.
pub(crate) fn installed_plugins_in(home: Option<&Path>) -> Option<Value> {
    let path = home?.join(".claude/plugins/installed_plugins.json");
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

/// Find `plugin` in `installed_plugins.json`. Keys are `name@marketplace`;
/// the value is an install record, or (version 2 of the file) a list of them,
/// each carrying an `installPath`.
fn installed_root(
    installed: &Value,
    plugin: &str,
    home: Option<&Path>,
) -> Option<(String, PathBuf)> {
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
                return Some((key.clone(), expand_home(path, home)));
            }
        }
    }
    None
}

/// Every `name@marketplace` key under which `plugin` is installed, plus
/// `always`, sorted. `always` covers a machine where it is not installed yet:
/// an `enabledPlugins` entry for a missing plugin does nothing.
pub(crate) fn installed_keys(installed: Option<&Value>, plugin: &str, always: &str) -> Vec<String> {
    let mut keys = registry_keys(installed, plugin);
    keys.push(always.to_string());
    keys.sort();
    keys.dedup();
    keys
}

#[derive(Deserialize)]
struct SkillFront {
    name: Option<String>,
    description: Option<String>,
}

/// `(name, description)` for each `skills/<dir>/SKILL.md` under `root`.
fn read_skills(root: &Path) -> Result<Vec<(String, String)>> {
    let mut out: Vec<(String, String)> = skill_dirs(root)?
        .into_iter()
        .map(|(name, description, _)| (name, description))
        .collect();
    out.sort();
    Ok(out)
}

/// `(name, description, directory)` for each `skills/<dir>/SKILL.md` under
/// `root`. The name is the front matter's `name`, else the directory's.
fn skill_dirs(root: &Path) -> Result<Vec<(String, String, PathBuf)>> {
    let dir = root.join("skills");
    let entries = std::fs::read_dir(&dir).with_context(|| format!("reading {}", dir.display()))?;
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
        out.push((name, description, entry.path()));
    }
    Ok(out)
}

/// The directory under a skills bundle that holds the filtered plugins.
pub(crate) const FILTERED_DIR: &str = "plugins";

/// What a filtered copy leaves out of the plugin root: the skills and the
/// commands (Claude Code lists a command as a skill), which it rebuilds from
/// the named skills only; the manifest, which it rewrites; and Claude Code's
/// own markers in an installed plugin's cache directory.
const LEFT_OUT: [&str; 6] = [
    "skills",
    "commands",
    ".claude-plugin",
    ".in_use",
    ".orphaned_at",
    ".git",
];

/// Manifest keys a filtered copy drops: `skills` adds skill directories to
/// the `skills/` scan, and `commands` names command files.
const MANIFEST_LEFT_OUT: [&str; 2] = ["skills", "commands"];

/// Write a filtered copy of each `plugin_skills` plugin under
/// `<bundle>/plugins/<plugin>/` and return the teammate's `plugin_dirs` with
/// each plugin's original directory replaced by its copy, and the copy of
/// each installed plugin appended. `home` is `$HOME`.
///
/// A copy is the plugin minus its commands and every skill the teammate did
/// not name. Its agents, hooks, MCP and LSP servers, scripts and other files
/// stay: `plugin_skills` narrows skills, not tools. Each named skill's copy
/// must hold the same bytes as its source (`tree_digest`), or the launch
/// fails.
pub(crate) fn materialize_filtered(
    teammate: &Teammate,
    bundle: &Path,
    home: Option<&Path>,
) -> Result<Vec<String>> {
    let mut dirs: Vec<String> = teammate.plugin_dirs.clone();
    for (plugin, wanted) in resolve_all_in(teammate, home)? {
        let name = plugin.name.as_str();
        if name.is_empty() || matches!(name, "." | "..") || name.contains(['/', '\\', '\0']) {
            bail!("plugin name '{name}' cannot name a directory");
        }
        let copy = bundle.join(FILTERED_DIR).join(name);
        filtered_copy(&plugin, &wanted, &copy)?;
        let copy = copy.to_string_lossy().into_owned();
        let original = teammate
            .plugin_dirs
            .iter()
            .position(|d| expand_home(d, home) == plugin.root);
        match original {
            Some(at) => dirs[at] = copy,
            None => dirs.push(copy),
        }
    }
    Ok(dirs)
}

/// Copy `plugin` to `to` with only the `wanted` skills.
fn filtered_copy(plugin: &Plugin, wanted: &[String], to: &Path) -> Result<()> {
    std::fs::create_dir_all(to.join(".claude-plugin"))
        .with_context(|| format!("creating {}", to.display()))?;
    let manifest_path = plugin.root.join(".claude-plugin/plugin.json");
    let mut manifest = match std::fs::read_to_string(&manifest_path) {
        Ok(text) => serde_json::from_str::<Value>(&text)
            .with_context(|| format!("parsing {}", manifest_path.display()))?,
        Err(_) => serde_json::json!({}),
    };
    let fields = manifest
        .as_object_mut()
        .with_context(|| format!("{} must be a JSON object", manifest_path.display()))?;
    for key in MANIFEST_LEFT_OUT {
        fields.remove(key);
    }
    // A manifest-less plugin takes its name from its directory, which the
    // copy does not share.
    fields.insert("name".into(), Value::String(plugin.name.clone()));
    std::fs::write(to.join(".claude-plugin/plugin.json"), manifest.to_string())?;

    let entries = std::fs::read_dir(&plugin.root)
        .with_context(|| format!("reading {}", plugin.root.display()))?;
    for entry in entries {
        let entry = entry?;
        if LEFT_OUT.iter().any(|n| entry.file_name() == **n) {
            continue;
        }
        copy_entry(&entry.path(), &to.join(entry.file_name()))?;
    }

    for (name, _, from) in skill_dirs(&plugin.root)? {
        if !wanted.contains(&name) {
            continue;
        }
        let target = to
            .join("skills")
            .join(from.file_name().context("a skill directory has a name")?);
        let digest = |dir: &Path| {
            horch_marketplace::integrity::tree_digest(dir)
                .map_err(|e| anyhow::anyhow!("plugin skill '{}:{name}': {e}", plugin.name))
        };
        let expected = digest(&from)?;
        copy_entry(&from, &target)?;
        let actual = digest(&target)?;
        if actual != expected {
            bail!(
                "plugin skill '{}:{name}': the copy holds {actual}, {} holds {expected}; \
                 the plugin changed during the launch",
                plugin.name,
                from.display()
            );
        }
    }
    Ok(())
}

/// Copy a file, a symlink (as a symlink) or a directory tree.
fn copy_entry(from: &Path, to: &Path) -> Result<()> {
    let kind = std::fs::symlink_metadata(from)
        .with_context(|| format!("reading {}", from.display()))?
        .file_type();
    if kind.is_symlink() {
        let target = std::fs::read_link(from)?;
        #[cfg(unix)]
        std::os::unix::fs::symlink(&target, to)
            .with_context(|| format!("linking {}", to.display()))?;
        #[cfg(not(unix))]
        bail!("{} is a symlink", from.display());
    } else if kind.is_dir() {
        std::fs::create_dir_all(to)?;
        for item in
            std::fs::read_dir(from).with_context(|| format!("reading {}", from.display()))?
        {
            let item = item?;
            copy_entry(&item.path(), &to.join(item.file_name()))?;
        }
    } else if kind.is_file() {
        std::fs::copy(from, to).with_context(|| format!("copying {}", from.display()))?;
    } else {
        bail!("{} is not a regular file", from.display());
    }
    Ok(())
}

/// Every `name@marketplace` key under which `plugin` is installed, sorted.
fn registry_keys(installed: Option<&Value>, plugin: &str) -> Vec<String> {
    let mut keys: Vec<String> = installed
        .and_then(|v| v.get("plugins"))
        .and_then(|p| p.as_object())
        .map(|p| {
            p.keys()
                .filter(|k| k.split('@').next() == Some(plugin))
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    keys.sort();
    keys
}

/// Switch each `plugin_skills` plugin to its filtered copy: every installed
/// `"<plugin>@<marketplace>"` off, so only the copy can load, and
/// `"<plugin>@inline"` (the `--plugin-dir` copy) on, even where
/// `inherit_plugins: false` or the operator's settings switched it off.
/// Must run after that switch-off, so it wins.
pub(crate) fn overlay_plugin_skills(
    teammate: &Teammate,
    overlay: &mut serde_json::Map<String, serde_json::Value>,
    home: Option<&Path>,
) -> Result<()> {
    let installed = installed_plugins_in(home);
    for (plugin, _) in resolve_all_in(teammate, home)? {
        let plugins = overlay
            .entry("enabledPlugins")
            .or_insert_with(|| serde_json::json!({}))
            .as_object_mut()
            .context("enabledPlugins must be an object")?;
        for key in registry_keys(installed.as_ref(), &plugin.name) {
            plugins.insert(key, serde_json::Value::Bool(false));
        }
        plugins.insert(
            format!("{}@inline", plugin.name),
            serde_json::Value::Bool(true),
        );
    }
    Ok(())
}

/// Fail a launch whose `plugin_skills` plugins are not filtered copies: a
/// launch without a skills bundle has nowhere to put one, and the original
/// plugin would load with every skill, or (installed and switched off) with
/// none. `home` is `$HOME`.
pub(crate) fn check_filtered(teammate: &Teammate, home: Option<&Path>) -> Result<()> {
    for (plugin, wanted) in resolve_all_in(teammate, home)? {
        let extra: Vec<&str> = plugin
            .skills
            .iter()
            .map(|(n, _)| n.as_str())
            .filter(|n| !wanted.iter().any(|w| w == n))
            .collect();
        if plugin.installed_key.is_some() || !extra.is_empty() {
            bail!(
                "{}: plugin_skills.{} loads a filtered copy of the plugin from the \
                 skills bundle, and this launch has no bundle; give the teammate a \
                 phase or a skill",
                teammate.name,
                plugin.name
            );
        }
    }
    Ok(())
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
        let local = plugin(
            tmp.path(),
            "code",
            &[("review", "Review a diff."), ("lint", "Lint.")],
        );
        let found = resolve_in(std::slice::from_ref(&local), None, "code", None).unwrap();
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
            let found =
                resolve_in(std::slice::from_ref(&local), Some(&registry), "dev", None).unwrap();
            assert_eq!(found.installed_key.as_deref(), Some("dev@market"));
            assert_eq!(found.description("tdd"), Some("Red, green."));
        }

        let err = resolve_in(&[local], None, "missing", None)
            .unwrap_err()
            .to_string();
        assert!(err.contains("plugin 'missing' is neither"), "{err}");
    }

    #[test]
    fn installed_keys_finds_every_marketplace_and_keeps_the_fallback() {
        let registry = serde_json::json!({"version": 2, "plugins": {
            "sc@official": [{"installPath": "/x"}],
            "sc@fork": [{"installPath": "/y"}],
            "other@official": [{"installPath": "/z"}]
        }});
        assert_eq!(
            installed_keys(Some(&registry), "sc", "sc@official"),
            ["sc@fork", "sc@official"]
        );
        assert_eq!(installed_keys(None, "sc", "sc@official"), ["sc@official"]);
    }
}
