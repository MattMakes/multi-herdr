use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use serde_json::Value;

use crate::roster::expand_home;
use crate::roster::Teammate;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Plugin {
    pub name: String,
    pub installed_key: Option<String>,
    pub root: PathBuf,
    pub version: Option<String>,
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

pub(crate) fn resolve(teammate: &Teammate, plugin: &str, home: Option<&Path>) -> Result<Plugin> {
    let dirs: Vec<PathBuf> = teammate
        .plugin_dirs
        .iter()
        .map(|d| expand_home(d, home))
        .collect();
    resolve_in(&dirs, installed_plugins_in(home).as_ref(), plugin, home)
}

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

pub(crate) fn resolve_in(
    dirs: &[PathBuf],
    installed: Option<&Value>,
    plugin: &str,
    home: Option<&Path>,
) -> Result<Plugin> {
    if let Some(dir) = dirs
        .iter()
        .find(|d| plugin_name(d).as_deref() == Some(plugin))
    {
        plugin_from_dir(plugin, dir)
    } else if let Some((key, root, version)) =
        installed.and_then(|v| installed_root(v, plugin, home))
    {
        plugin_from_install(plugin, key, root, version)
    } else {
        bail!(
            "plugin '{plugin}' is neither in plugin_dirs nor installed \
             (~/.claude/plugins/installed_plugins.json)"
        )
    }
}

fn plugin_from_dir(plugin: &str, dir: &Path) -> Result<Plugin> {
    Ok(Plugin {
        name: plugin.to_string(),
        installed_key: None,
        root: dir.to_path_buf(),
        version: manifest_version(dir),
        skills: read_skills(dir)?,
    })
}

fn plugin_from_install(
    plugin: &str,
    key: String,
    root: PathBuf,
    version: Option<String>,
) -> Result<Plugin> {
    Ok(Plugin {
        name: plugin.to_string(),
        installed_key: Some(key),
        skills: read_skills(&root)?,
        version: version.or_else(|| manifest_version(&root)),
        root,
    })
}

fn plugin_name(dir: &Path) -> Option<String> {
    manifest_field(dir, "name")
        .or_else(|| dir.file_name().map(|n| n.to_string_lossy().into_owned()))
}

fn manifest_version(dir: &Path) -> Option<String> {
    manifest_field(dir, "version")
}

fn manifest_field(dir: &Path, field: &str) -> Option<String> {
    let manifest = std::fs::read_to_string(dir.join(".claude-plugin/plugin.json")).ok();
    manifest
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|v| v.get(field).and_then(|n| n.as_str()).map(str::to_owned))
}

pub(crate) fn installed_plugins_in(home: Option<&Path>) -> Option<Value> {
    let path = home?.join(".claude/plugins/installed_plugins.json");
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

fn installed_root(
    installed: &Value,
    plugin: &str,
    home: Option<&Path>,
) -> Option<(String, PathBuf, Option<String>)> {
    let plugins = installed.get("plugins")?.as_object()?;
    plugins
        .iter()
        .filter(|(key, _)| key.split('@').next().unwrap_or(key) == plugin || *key == plugin)
        .find_map(|(key, value)| install_record(key, value, home))
}

fn install_record(
    key: &str,
    value: &Value,
    home: Option<&Path>,
) -> Option<(String, PathBuf, Option<String>)> {
    let records = match value {
        Value::Array(list) => list.as_slice(),
        other => std::slice::from_ref(other),
    };
    records.iter().find_map(|record| {
        let path = record.get("installPath")?.as_str()?;
        let version = record
            .get("version")
            .and_then(|v| v.as_str())
            .map(str::to_owned);
        Some((key.to_string(), expand_home(path, home), version))
    })
}

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

fn read_skills(root: &Path) -> Result<Vec<(String, String)>> {
    let mut out: Vec<(String, String)> = skill_dirs(root)?
        .into_iter()
        .map(|(name, description, _)| (name, description))
        .collect();
    out.sort();
    Ok(out)
}

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

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct NamedPluginSkill {
    pub plugin: String,
    pub skill: String,
    pub plugin_key: String,
    pub version: Option<String>,
    pub description: String,
    pub dir: PathBuf,
    pub skill_file_bytes: usize,
    pub digest: String,
}

pub(crate) fn named_skills(
    teammate: &Teammate,
    home: Option<&Path>,
) -> Result<Vec<NamedPluginSkill>> {
    let mut out = Vec::new();
    for (plugin, wanted) in resolve_all_in(teammate, home)? {
        let plugin_key = plugin
            .installed_key
            .clone()
            .unwrap_or_else(|| format!("{}@inline", plugin.name));
        let dirs = skill_dirs(&plugin.root)?;
        for skill in wanted {
            out.push(named_skill(&plugin, &plugin_key, &dirs, skill)?);
        }
    }
    Ok(out)
}

fn named_skill(
    plugin: &Plugin,
    plugin_key: &str,
    dirs: &[(String, String, PathBuf)],
    skill: String,
) -> Result<NamedPluginSkill> {
    let (_, description, dir) = dirs
        .iter()
        .find(|(name, _, _)| *name == skill)
        .with_context(|| format!("plugin skill '{}:{skill}' has no directory", plugin.name))?;
    let digest = skill_digest(plugin, &skill, dir)?;
    let skill_file_bytes = std::fs::metadata(dir.join("SKILL.md"))
        .map(|m| m.len() as usize)
        .unwrap_or(0);
    Ok(NamedPluginSkill {
        plugin: plugin.name.clone(),
        skill,
        plugin_key: plugin_key.to_string(),
        version: plugin.version.clone(),
        description: description.clone(),
        dir: dir.clone(),
        skill_file_bytes,
        digest,
    })
}

pub(crate) const FILTERED_DIR: &str = "plugins";

const LEFT_OUT: [&str; 6] = [
    "skills",
    "commands",
    ".claude-plugin",
    ".in_use",
    ".orphaned_at",
    ".git",
];

const MANIFEST_LEFT_OUT: [&str; 2] = ["skills", "commands"];

pub(crate) fn materialize_filtered(
    teammate: &Teammate,
    skills: &crate::skills::Bundle,
    home: Option<&Path>,
) -> Result<Vec<String>> {
    let bundle = skills.root();
    let mut dirs: Vec<String> = teammate.plugin_dirs.clone();
    for (plugin, wanted) in resolve_all_in(teammate, home)? {
        let name = plugin.name.as_str();
        if !is_dir_name(name) {
            bail!("plugin name '{name}' cannot name a directory");
        }
        let copy = bundle.join(FILTERED_DIR).join(name);
        let pinned = |skill: &str| pinned_digest(skills, name, skill);
        filtered_copy(&plugin, &wanted, &copy, pinned)?;
        let copy = copy.to_string_lossy().into_owned();
        let original = teammate
            .plugin_dirs
            .iter()
            .position(|d| expand_home(d, home) == plugin.root);
        put_copy(&mut dirs, original, copy);
    }
    Ok(dirs)
}

fn is_dir_name(name: &str) -> bool {
    !name.is_empty() && !matches!(name, "." | "..") && !name.contains(['/', '\\', '\0'])
}

fn pinned_digest(skills: &crate::skills::Bundle, plugin: &str, skill: &str) -> Option<String> {
    let id = format!("{plugin}:{skill}");
    skills
        .plan()
        .activated
        .iter()
        .find(|r| r.id.as_str() == id)
        .map(|r| r.digest.to_string())
}

fn put_copy(dirs: &mut Vec<String>, original: Option<usize>, copy: String) {
    match original {
        Some(at) => dirs[at] = copy,
        None => dirs.push(copy),
    }
}

fn filtered_copy(
    plugin: &Plugin,
    wanted: &[String],
    to: &Path,
    pinned: impl Fn(&str) -> Option<String>,
) -> Result<()> {
    write_filtered_manifest(plugin, to)?;
    copy_top_level(plugin, to)?;
    skill_dirs(&plugin.root)?
        .into_iter()
        .filter(|(name, _, _)| wanted.contains(name))
        .try_for_each(|(name, _, from)| copy_checked_skill(plugin, &name, &from, to, pinned(&name)))
}

fn write_filtered_manifest(plugin: &Plugin, to: &Path) -> Result<()> {
    std::fs::create_dir_all(to.join(".claude-plugin"))
        .with_context(|| format!("creating {}", to.display()))?;
    let manifest_path = plugin.root.join(".claude-plugin/plugin.json");
    let mut manifest = read_manifest(&manifest_path)?;
    let fields = manifest
        .as_object_mut()
        .with_context(|| format!("{} must be a JSON object", manifest_path.display()))?;
    for key in MANIFEST_LEFT_OUT {
        fields.remove(key);
    }
    fields.insert("name".into(), Value::String(plugin.name.clone()));
    std::fs::write(to.join(".claude-plugin/plugin.json"), manifest.to_string())
        .map_err(anyhow::Error::from)
}

fn read_manifest(path: &Path) -> Result<Value> {
    match std::fs::read_to_string(path) {
        Ok(text) => {
            serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))
        }
        Err(_) => Ok(serde_json::json!({})),
    }
}

fn skill_digest(plugin: &Plugin, name: &str, dir: &Path) -> Result<String> {
    horch_marketplace::integrity::tree_digest(dir)
        .map_err(|e| anyhow::anyhow!("plugin skill '{}:{name}': {e}", plugin.name))
}

fn copy_top_level(plugin: &Plugin, to: &Path) -> Result<()> {
    let entries = std::fs::read_dir(&plugin.root)
        .with_context(|| format!("reading {}", plugin.root.display()))?;
    for entry in entries {
        let entry = entry?;
        let left_out = LEFT_OUT.iter().any(|n| entry.file_name() == **n);
        if !left_out {
            copy_entry(&entry.path(), &to.join(entry.file_name()))?;
        }
    }
    Ok(())
}

fn copy_checked_skill(
    plugin: &Plugin,
    name: &str,
    from: &Path,
    to: &Path,
    pinned: Option<String>,
) -> Result<()> {
    let target = to
        .join("skills")
        .join(from.file_name().context("a skill directory has a name")?);
    let (expected, by) = match pinned {
        Some(d) => (d, "the plan pins".to_string()),
        None => (
            skill_digest(plugin, name, from)?,
            format!("{} holds", from.display()),
        ),
    };
    copy_entry(from, &target)?;
    let actual = skill_digest(plugin, name, &target)?;
    if actual != expected {
        bail!(
            "plugin skill '{}:{name}': the copy holds {actual}, {by} {expected}; \
             the plugin changed during the launch",
            plugin.name
        );
    }
    Ok(())
}

fn copy_entry(from: &Path, to: &Path) -> Result<()> {
    let kind = std::fs::symlink_metadata(from)
        .with_context(|| format!("reading {}", from.display()))?
        .file_type();
    if kind.is_symlink() {
        copy_symlink(from, to)
    } else if kind.is_dir() {
        copy_dir(from, to)
    } else if kind.is_file() {
        std::fs::copy(from, to)
            .map(drop)
            .with_context(|| format!("copying {}", from.display()))
    } else {
        bail!("{} is not a regular file", from.display())
    }
}

#[cfg(unix)]
fn copy_symlink(from: &Path, to: &Path) -> Result<()> {
    let target = std::fs::read_link(from)?;
    std::os::unix::fs::symlink(&target, to).with_context(|| format!("linking {}", to.display()))
}

#[cfg(not(unix))]
fn copy_symlink(from: &Path, _to: &Path) -> Result<()> {
    std::fs::read_link(from)?;
    bail!("{} is a symlink", from.display())
}

fn copy_dir(from: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir_all(to)?;
    for item in std::fs::read_dir(from).with_context(|| format!("reading {}", from.display()))? {
        let item = item?;
        copy_entry(&item.path(), &to.join(item.file_name()))?;
    }
    Ok(())
}

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
    #[test]
    fn an_empty_skill_list_is_refused_with_the_way_out() {
        let teammate = Teammate {
            plugin_skills: [("code".to_string(), Vec::<String>::new())]
                .into_iter()
                .collect(),
            ..Teammate::default()
        };
        let err = resolve_all_in(&teammate, None).unwrap_err().to_string();
        assert_eq!(
            err,
            "plugin_skills.code: list at least one skill; to switch the whole plugin off, \
             leave it out of plugin_skills"
        );
    }

    fn wanted(names: &[&str]) -> Vec<String> {
        names.iter().map(|n| n.to_string()).collect()
    }

    fn resolved(dir: &Path) -> Plugin {
        resolve_in(&[dir.to_path_buf()], None, "code", None).unwrap()
    }

    fn digest_of(dir: &Path) -> String {
        horch_marketplace::integrity::tree_digest(dir).unwrap()
    }

    #[test]
    fn a_filtered_copy_keeps_the_named_skills_and_the_plugins_other_files() {
        let tmp = tempfile::tempdir().unwrap();
        let src = plugin(tmp.path(), "code", &[("review", "R."), ("lint", "L.")]);
        std::fs::create_dir_all(src.join("hooks")).unwrap();
        std::fs::write(src.join("hooks/h.json"), "{}").unwrap();
        std::fs::create_dir_all(src.join("commands")).unwrap();
        std::fs::write(src.join("commands/c.md"), "c").unwrap();
        std::fs::write(src.join("README.md"), "readme").unwrap();
        for left_out in [".in_use", ".orphaned_at", ".git"] {
            std::fs::write(src.join(left_out), "x").unwrap();
        }
        let plugin = resolved(&src);
        std::fs::write(
            src.join(".claude-plugin/plugin.json"),
            r#"{"name":"stale","version":"2","skills":["./x"],"commands":"./c","hooks":"./hooks/h.json"}"#,
        )
        .unwrap();
        let to = tmp.path().join("out");
        filtered_copy(&plugin, &wanted(&["review"]), &to, |_| None).unwrap();

        let mut top: Vec<String> = std::fs::read_dir(&to)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        top.sort();
        assert_eq!(top, [".claude-plugin", "README.md", "hooks", "skills"]);
        assert!(to.join("skills/review/SKILL.md").is_file());
        assert!(!to.join("skills/lint").exists());
        let manifest: Value = serde_json::from_str(
            &std::fs::read_to_string(to.join(".claude-plugin/plugin.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            manifest,
            serde_json::json!({"name":"code","version":"2","hooks":"./hooks/h.json"})
        );
    }

    #[test]
    fn a_filtered_copy_names_a_manifest_less_plugin_after_the_plugin() {
        let tmp = tempfile::tempdir().unwrap();
        let src = plugin(tmp.path(), "code", &[("review", "R.")]);
        std::fs::remove_file(src.join(".claude-plugin/plugin.json")).unwrap();
        let to = tmp.path().join("out");
        filtered_copy(&resolved(&src), &wanted(&["review"]), &to, |_| None).unwrap();
        let manifest = std::fs::read_to_string(to.join(".claude-plugin/plugin.json")).unwrap();
        assert_eq!(manifest, r#"{"name":"code"}"#);
    }

    #[test]
    fn a_filtered_copy_refuses_a_bad_manifest() {
        let tmp = tempfile::tempdir().unwrap();
        let src = plugin(tmp.path(), "code", &[("review", "R.")]);
        let plugin = resolved(&src);
        for (text, why) in [("{oops", "parsing"), ("[]", "must be a JSON object")] {
            std::fs::write(src.join(".claude-plugin/plugin.json"), text).unwrap();
            let err = filtered_copy(&plugin, &wanted(&["review"]), &tmp.path().join("o"), |_| {
                None
            })
            .unwrap_err()
            .to_string();
            assert!(err.contains(why), "{err}");
        }
    }

    #[test]
    fn a_skill_that_differs_from_its_pinned_digest_fails_the_launch() {
        let tmp = tempfile::tempdir().unwrap();
        let src = plugin(tmp.path(), "code", &[("review", "R.")]);
        let plugin = resolved(&src);
        let real = digest_of(&src.join("skills/review"));

        let ok = filtered_copy(&plugin, &wanted(&["review"]), &tmp.path().join("a"), |s| {
            (s == "review").then(|| real.clone())
        });
        assert!(ok.is_ok(), "{ok:?}");

        let err = filtered_copy(&plugin, &wanted(&["review"]), &tmp.path().join("b"), |_| {
            Some("sha256:other".to_string())
        })
        .unwrap_err()
        .to_string();
        assert!(
            err.starts_with("plugin skill 'code:review': the copy holds "),
            "{err}"
        );
        assert!(
            err.ends_with(", the plan pins sha256:other; the plugin changed during the launch"),
            "{err}"
        );

        let ok = filtered_copy(&plugin, &wanted(&["review"]), &tmp.path().join("c"), |_| {
            None
        });
        assert!(ok.is_ok(), "{ok:?}");
    }

    #[test]
    fn a_skill_is_copied_under_its_directory_name() {
        let tmp = tempfile::tempdir().unwrap();
        let src = plugin(tmp.path(), "code", &[("dir-name", "D.")]);
        std::fs::write(
            src.join("skills/dir-name/SKILL.md"),
            "---\nname: front-name\ndescription: D.\n---\n",
        )
        .unwrap();
        let to = tmp.path().join("out");
        filtered_copy(&resolved(&src), &wanted(&["front-name"]), &to, |_| None).unwrap();
        assert!(to.join("skills/dir-name/SKILL.md").is_file());
        assert!(!to.join("skills/front-name").exists());
    }

    #[test]
    fn copy_entry_copies_files_trees_and_links_and_refuses_the_rest() {
        let tmp = tempfile::tempdir().unwrap();
        let from = tmp.path().join("from");
        std::fs::create_dir_all(from.join("d/e")).unwrap();
        std::fs::write(from.join("d/e/f.txt"), "f").unwrap();
        std::fs::write(from.join("top.txt"), "t").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink("top.txt", from.join("link")).unwrap();
        let to = tmp.path().join("to");
        copy_entry(&from, &to).unwrap();
        assert_eq!(std::fs::read_to_string(to.join("d/e/f.txt")).unwrap(), "f");
        assert_eq!(std::fs::read_to_string(to.join("top.txt")).unwrap(), "t");
        #[cfg(unix)]
        {
            let meta = std::fs::symlink_metadata(to.join("link")).unwrap();
            assert!(meta.file_type().is_symlink());
            assert_eq!(
                std::fs::read_link(to.join("link")).unwrap(),
                Path::new("top.txt")
            );
        }

        let err = copy_entry(&tmp.path().join("missing"), &tmp.path().join("x"))
            .unwrap_err()
            .to_string();
        assert!(err.starts_with("reading "), "{err}");

        #[cfg(unix)]
        {
            let fifo = tmp.path().join("fifo");
            let made = std::process::Command::new("mkfifo").arg(&fifo).status();
            if made.is_ok_and(|s| s.success()) {
                let err = copy_entry(&fifo, &tmp.path().join("y"))
                    .unwrap_err()
                    .to_string();
                assert!(err.ends_with("is not a regular file"), "{err}");
            }
        }
    }

    #[test]
    fn the_installed_registry_matches_by_name_or_key_and_takes_the_first_record_with_a_path() {
        let home = Path::new("/home/u");
        let registry = serde_json::json!({"plugins": {
            "other@m": {"installPath": "/other"},
            "dev@a": [{"scope": "x"}, {"installPath": "~/cache/dev", "version": "1.2"}, {"installPath": "/second"}],
            "bare": {"installPath": "/bare"},
            "nopath@m": {"version": "9"}
        }});
        assert_eq!(
            installed_root(&registry, "dev", Some(home)),
            Some((
                "dev@a".to_string(),
                PathBuf::from("/home/u/cache/dev"),
                Some("1.2".to_string())
            ))
        );
        assert_eq!(
            installed_root(&registry, "dev@a", Some(home)).map(|r| r.0),
            Some("dev@a".to_string())
        );
        assert_eq!(
            installed_root(&registry, "bare", None),
            Some(("bare".to_string(), PathBuf::from("/bare"), None))
        );
        assert_eq!(installed_root(&registry, "nopath", None), None);
        assert_eq!(installed_root(&registry, "absent", None), None);
        assert_eq!(installed_root(&serde_json::json!({}), "dev", None), None);
        assert_eq!(
            installed_root(&serde_json::json!({"plugins": []}), "dev", None),
            None
        );
    }

    #[test]
    fn an_installed_plugin_takes_its_version_from_the_record_else_the_manifest() {
        let tmp = tempfile::tempdir().unwrap();
        let installed = plugin(&tmp.path().join("cache"), "dev", &[("tdd", "T.")]);
        std::fs::write(
            installed.join(".claude-plugin/plugin.json"),
            r#"{"name":"dev","version":"3.0"}"#,
        )
        .unwrap();
        let with_record =
            serde_json::json!({"plugins": {"dev@m": {"installPath": installed, "version": "1.0"}}});
        let found = resolve_in(&[], Some(&with_record), "dev", None).unwrap();
        assert_eq!(found.version.as_deref(), Some("1.0"));
        let without = serde_json::json!({"plugins": {"dev@m": {"installPath": installed}}});
        let found = resolve_in(&[], Some(&without), "dev", None).unwrap();
        assert_eq!(found.version.as_deref(), Some("3.0"));
        let local = plugin(tmp.path(), "dir-name", &[("a", "A.")]);
        std::fs::write(
            local.join(".claude-plugin/plugin.json"),
            r#"{"name":"manifest-name","version":"0.1"}"#,
        )
        .unwrap();
        let found = resolve_in(std::slice::from_ref(&local), None, "manifest-name", None).unwrap();
        assert_eq!(
            (found.version.as_deref(), found.installed_key),
            (Some("0.1"), None)
        );
        assert!(resolve_in(&[local], None, "dir-name", None).is_err());
    }

    fn teammate_naming(dirs: &[&Path], skills: &[(&str, &[&str])]) -> Teammate {
        Teammate {
            plugin_dirs: dirs
                .iter()
                .map(|d| d.to_string_lossy().into_owned())
                .collect(),
            plugin_skills: skills
                .iter()
                .map(|(plugin, names)| (plugin.to_string(), wanted(names)))
                .collect(),
            ..Teammate::default()
        }
    }

    fn install_registry(home: &Path, key: &str, dir: &Path, version: &str) {
        std::fs::create_dir_all(home.join(".claude/plugins")).unwrap();
        std::fs::write(
            home.join(".claude/plugins/installed_plugins.json"),
            serde_json::json!({"plugins": {key: {"installPath": dir, "version": version}}})
                .to_string(),
        )
        .unwrap();
    }

    fn plain_bundle(state: &Path) -> crate::skills::Bundle {
        let t = Teammate {
            phase: Some(crate::roster::Phase::Plan),
            ..Teammate::default()
        };
        crate::skills::Bundle::install(state, &t).unwrap().unwrap()
    }

    #[test]
    fn named_skills_follow_the_plugins_in_name_order_and_the_skills_as_named() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let local = plugin(
            tmp.path(),
            "code",
            &[("review", "Review a diff."), ("tdd", "Red, green.")],
        );
        std::fs::write(
            local.join(".claude-plugin/plugin.json"),
            r#"{"name":"code","version":"1.2.0"}"#,
        )
        .unwrap();
        let installed = plugin(&tmp.path().join("cache"), "dev", &[("deploy", "Deploy.")]);
        install_registry(&home, "dev@market", &installed, "3.1");
        let t = teammate_naming(
            &[&local],
            &[("dev", &["deploy"]), ("code", &["tdd", "review"])],
        );

        let got = named_skills(&t, Some(&home)).unwrap();
        let ids: Vec<(&str, &str, &str)> = got
            .iter()
            .map(|n| (n.plugin.as_str(), n.skill.as_str(), n.plugin_key.as_str()))
            .collect();
        assert_eq!(
            ids,
            [
                ("code", "tdd", "code@inline"),
                ("code", "review", "code@inline"),
                ("dev", "deploy", "dev@market"),
            ]
        );
        let tdd = &got[0];
        assert_eq!(tdd.version.as_deref(), Some("1.2.0"));
        assert_eq!(tdd.description, "Red, green.");
        assert_eq!(tdd.dir, local.join("skills/tdd"));
        assert_eq!(
            tdd.skill_file_bytes,
            std::fs::read(tdd.dir.join("SKILL.md")).unwrap().len()
        );
        assert_eq!(tdd.digest, digest_of(&local.join("skills/tdd")));
        assert_ne!(tdd.digest, got[1].digest);
        let deploy = &got[2];
        assert_eq!(deploy.version.as_deref(), Some("3.1"));
        assert_eq!(deploy.dir, installed.join("skills/deploy"));
        assert_eq!(deploy.digest, digest_of(&installed.join("skills/deploy")));
        assert_eq!(named_skills(&Teammate::default(), None).unwrap(), []);
    }

    #[cfg(unix)]
    #[test]
    fn named_skills_refuse_a_skill_tree_the_digest_rules_refuse() {
        let tmp = tempfile::tempdir().unwrap();
        let src = plugin(tmp.path(), "code", &[("lint", "L."), ("review", "R.")]);
        std::os::unix::fs::symlink("SKILL.md", src.join("skills/review/alias")).unwrap();
        let t = teammate_naming(&[&src], &[("code", &["lint"])]);
        assert_eq!(named_skills(&t, None).unwrap().len(), 1);
        let t = teammate_naming(&[&src], &[("code", &["lint", "review"])]);
        let err = named_skills(&t, None).unwrap_err().to_string();
        assert_eq!(err, "plugin skill 'code:review': 'alias' is a symlink");
    }

    #[test]
    fn materialize_filtered_refuses_a_plugin_name_that_cannot_name_a_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let bundle = plain_bundle(&tmp.path().join("state"));
        for (at, bad) in ["", ".", "..", "a/b", "a\\b", "a\0b"]
            .into_iter()
            .enumerate()
        {
            let dir = plugin(tmp.path(), &format!("p{at}"), &[("review", "R.")]);
            std::fs::write(
                dir.join(".claude-plugin/plugin.json"),
                serde_json::json!({ "name": bad }).to_string(),
            )
            .unwrap();
            let t = teammate_naming(&[&dir], &[(bad, &["review"])]);
            let err = materialize_filtered(&t, &bundle, None)
                .unwrap_err()
                .to_string();
            assert_eq!(err, format!("plugin name '{bad}' cannot name a directory"));
        }
        assert!(
            !bundle.root().join(FILTERED_DIR).exists(),
            "a copy was written"
        );
    }

    #[test]
    fn materialize_filtered_swaps_a_plugin_dir_and_appends_an_installed_one() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let bundle = plain_bundle(&tmp.path().join("state"));
        let plugins = home.join("plugins");
        plugin(&plugins, "other", &[("x", "X.")]);
        plugin(&plugins, "code", &[("review", "R."), ("lint", "L.")]);
        let installed = plugin(&tmp.path().join("cache"), "dev", &[("deploy", "D.")]);
        install_registry(&home, "dev@market", &installed, "3.1");
        let t = Teammate {
            plugin_dirs: vec!["~/plugins/other".into(), "~/plugins/code".into()],
            plugin_skills: [
                ("dev".to_string(), wanted(&["deploy"])),
                ("code".to_string(), wanted(&["review"])),
            ]
            .into(),
            ..Teammate::default()
        };

        let dirs = materialize_filtered(&t, &bundle, Some(&home)).unwrap();
        let copy = |name: &str| bundle.root().join(FILTERED_DIR).join(name);
        let text = |path: PathBuf| path.to_string_lossy().into_owned();
        assert_eq!(
            dirs,
            [
                "~/plugins/other".to_string(),
                text(copy("code")),
                text(copy("dev"))
            ]
        );
        assert!(copy("code").join("skills/review/SKILL.md").is_file());
        assert!(!copy("code").join("skills/lint").exists());
        assert!(copy("dev").join("skills/deploy/SKILL.md").is_file());
        assert!(!bundle.root().join(FILTERED_DIR).join("other").exists());
        let none = Teammate {
            plugin_skills: Default::default(),
            ..t
        };
        assert_eq!(
            materialize_filtered(&none, &bundle, Some(&home)).unwrap(),
            ["~/plugins/other", "~/plugins/code"]
        );
    }
}
