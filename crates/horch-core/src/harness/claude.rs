//! Claude Code: `claude [variadic flags] --model <m> ... "<prompt>"`.
//!
//! horch mints the session id (`--session-id`), so nothing is discovered
//! after launch. Skills reach it as a `--plugin-dir` plus a `--settings`
//! overlay.

use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result};

use super::launch::model_for;
use serde_json::{json, Value};

use super::{CommandSpec, Harness, HarnessKind, LaunchEnv, Session};
use crate::roster::Teammate;
use crate::roster::{expand_home, operator_enabled_plugins, operator_status_line};
use crate::skills::Bundle;

/// The plugin the skills bundle loads as. Its skills show as `horch:<id>`.
pub(crate) const SKILLS_PLUGIN: &str = "horch";

pub(crate) struct Claude;

impl Harness for Claude {
    fn kind(&self) -> HarnessKind {
        HarnessKind::Claude
    }

    /// Every teammate field is Claude-shaped.
    fn validate(&self, _t: &Teammate) -> Vec<&'static str> {
        Vec::new()
    }

    /// Haiku 4.5 has no effort setting, so claude rejects `--effort` for it.
    fn model_takes_effort(&self, model: &str) -> bool {
        !model
            .split(|c: char| !c.is_ascii_alphanumeric())
            .any(|seg| seg.eq_ignore_ascii_case("haiku"))
    }

    fn skill_namespace(&self) -> Option<&'static str> {
        Some(SKILLS_PLUGIN)
    }

    /// The bundle loads as a plugin: `--plugin-dir <bundle>` with a plugin
    /// manifest, plus a `--settings` overlay merged into the teammate's own.
    fn expose_skills(
        &self,
        teammate: &Teammate,
        skills: &Bundle,
        home: Option<&Path>,
    ) -> Result<Teammate> {
        let manifest = skills.root().join(".claude-plugin");
        std::fs::create_dir_all(&manifest)?;
        std::fs::write(
            manifest.join("plugin.json"),
            json!({"name": SKILLS_PLUGIN, "description": "Phase-selected fleet skills", "version": "0.1.0"})
                .to_string(),
        )?;
        let mut adjusted = teammate.clone();
        // plugin_dirs precede scalar flags in the Claude builder, which
        // fences variadic --plugin-dir parsing away from the briefing.
        adjusted
            .plugin_dirs
            .push(skills.root().to_string_lossy().into_owned());
        adjusted.settings = Some(skills_settings(teammate, home)?.to_string());
        Ok(adjusted)
    }

    fn command(&self, env: &LaunchEnv, spec: &CommandSpec<'_>) -> Result<Command> {
        claude_command(
            env,
            spec.teammate,
            spec.session,
            spec.prompt,
            spec.model_override,
        )
    }
}

pub(super) fn claude_command(
    env: &LaunchEnv,
    teammate: &Teammate,
    session: Session<'_>,
    prompt: &str,
    model_override: Option<&str>,
) -> Result<Command> {
    let bin = env.bins.claude.clone();
    let mut cmd = Command::new(&bin);

    // ORDERING MATTERS. `--mcp-config`, `--tools`, `--allowedTools` and
    // `--disallowedTools` are variadic (`<x...>`): they keep consuming
    // arguments until the next flag. The prompt is a bare positional at the
    // end, so any variadic flag that ended up last would swallow it as one
    // more config path or tool name. Every variadic flag therefore goes
    // first, and `--model` - always present, never variadic - fences them off.
    //
    // `--strict-mcp-config` makes `--mcp-config` the whole truth rather than an
    // addition, so an empty `mcpServers` object is what "no MCP servers at all"
    // looks like. Without strict, the operator's servers would still load.
    if let Some(servers) = &teammate.mcp_servers {
        let config = serde_json::json!({ "mcpServers": servers });
        cmd.arg("--mcp-config").arg(config.to_string());
        for file in &teammate.mcp_config_files {
            cmd.arg("--mcp-config").arg(env.expand_home(file));
        }
        cmd.arg("--strict-mcp-config");
    } else if !teammate.mcp_config_files.is_empty() {
        cmd.arg("--mcp-config");
        for file in &teammate.mcp_config_files {
            cmd.arg(env.expand_home(file));
        }
    }
    // `Some([])` means "no tools", which is `--tools ""`, not "omit the flag".
    if let Some(tools) = &teammate.tools {
        cmd.arg("--tools").arg(tools.join(","));
    }
    if !teammate.allowed_tools.is_empty() {
        cmd.arg("--allowedTools")
            .arg(teammate.allowed_tools.join(","));
    }
    if !teammate.disallowed_tools.is_empty() {
        cmd.arg("--disallowedTools")
            .arg(teammate.disallowed_tools.join(","));
    }

    let model = model_for(teammate, model_override)?;
    cmd.arg("--model").arg(model);

    // Haiku 4.5 has no effort setting, and a `--model haiku` override can land
    // on a teammate whose file sets one; drop it rather than fail the launch.
    if let Some(effort) = &teammate.effort {
        if Claude.model_takes_effort(model) {
            cmd.arg("--effort").arg(effort);
        }
    }
    if let Some(mode) = teammate.permission_mode {
        cmd.arg("--permission-mode").arg(mode.as_str());
    }
    // Which of the operator's settings files load. Normally all of them: the
    // operator has tuned settings.json for token economy (disableWorkflows,
    // disableBundledSkills, ...) and a fleet worker should keep every bit of
    // that. An empty list drops them all; it is a blunt instrument, kept for
    // the case where nothing narrower will do.
    if let Some(sources) = &teammate.setting_sources {
        cmd.arg("--setting-sources").arg(sources.join(","));
    }
    if teammate.disable_skills {
        cmd.arg("--disable-slash-commands");
    }
    for dir in &teammate.plugin_dirs {
        cmd.arg("--plugin-dir").arg(env.expand_home(dir));
    }

    // `--settings` takes a path OR a JSON string, and is not repeatable, so
    // everything horch wants to overlay has to be assembled into one object.
    if let Some(settings) = &teammate.settings {
        // The teammate named its own file; that file owns the overlay.
        cmd.arg("--settings").arg(env.expand_home(settings));
    } else {
        let mut overlay = serde_json::Map::new();
        // Switch the operator's globally-enabled plugins off, by name, for this
        // session. Settings merge per key, so this touches nothing else -
        // verified against a live launch: "Found 3 plugins (0 enabled,
        // 3 disabled)" with every other setting intact.
        if !teammate.inherit_plugins {
            let off: serde_json::Map<String, serde_json::Value> =
                operator_enabled_plugins(env.home())
                    .into_iter()
                    .map(|name| (name, serde_json::Value::Bool(false)))
                    .collect();
            if !off.is_empty() {
                overlay.insert("enabledPlugins".into(), serde_json::Value::Object(off));
            }
        }
        // Whatever inherit_plugins says: the generic tiers inherit plugins, but
        // no pane has a use for the operator's claude.ai skills.
        overlay_skill_switches(teammate, &mut overlay, env.home())?;
        // Restricting settings would take the status line with it. Every pane
        // in a fleet keeps the operator's, so a worker reads like any session.
        if teammate.setting_sources.is_some() {
            if let Some(status_line) = operator_status_line(env.home()) {
                overlay.insert("statusLine".into(), status_line);
            }
        }
        if !overlay.is_empty() {
            cmd.arg("--settings")
                .arg(serde_json::Value::Object(overlay).to_string());
        }
    }

    match session {
        Session::Fresh(id) => {
            cmd.arg("--session-id").arg(id);
        }
        Session::Resume(id) => {
            cmd.arg("--resume").arg(id);
        }
        Session::Unmanaged => {}
    }
    // Escape-hatch args sit right before the prompt: a variadic flag at the
    // end of `args` would eat it. `Roster::check` warns about that.
    cmd.args(&teammate.args);
    cmd.arg(prompt);
    Ok(cmd)
}

/// The `--settings` overlay of a launch with a skills bundle: the teammate's
/// own settings (inline JSON or a file), with horch's switches merged in.
fn skills_settings(teammate: &Teammate, home: Option<&Path>) -> Result<Value> {
    let mut settings = match &teammate.settings {
        Some(source) => {
            let text = if source.trim_start().starts_with('{') {
                source.clone()
            } else {
                std::fs::read_to_string(expand_home(source, home))
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
    if !teammate.inherit_plugins {
        let plugins = obj
            .entry("enabledPlugins")
            .or_insert(json!({}))
            .as_object_mut()
            .context("enabledPlugins must be an object")?;
        for name in operator_enabled_plugins(home) {
            plugins.insert(name, json!(false));
        }
    }
    // After the plugin switch-off: a plugin_skills plugin stays enabled.
    overlay_skill_switches(teammate, obj, home)?;
    if teammate.setting_sources.is_some() && !obj.contains_key("statusLine") {
        if let Some(status) = operator_status_line(home) {
            obj.insert("statusLine".into(), status);
        }
    }
    Ok(settings)
}

/// Add the skill switches every Claude launch overlays on the operator's
/// settings. Shared by the plain overlay above and [`skills_settings`], which
/// is the path fleet panes take.
///
/// The skills synced from claude.ai (`anthropic-skills:<name>`) go off unless
/// the teammate opts back in. `syncClaudeAiSkills: false` given through
/// `--settings` hides them for this session only and moves nothing on disk;
/// the same value in the operator's settings.json would trash the cache.
/// Each `disabled_skills` entry goes off by name. Settings merge per key, so
/// the operator's own `skillOverrides` still apply - verified against a live
/// launch (Claude Code 2.1.276, ai_docs/reports/claudeai-synced-skills.md).
pub(crate) fn overlay_skill_switches(
    teammate: &Teammate,
    overlay: &mut serde_json::Map<String, serde_json::Value>,
    home: Option<&Path>,
) -> Result<()> {
    if !teammate.inherit_claudeai_skills {
        overlay
            .entry("syncClaudeAiSkills")
            .or_insert(serde_json::Value::Bool(false));
    }
    // Plugins synced from claude.ai are plugins too: a teammate that sheds the
    // operator's plugins sheds these, for this session only.
    if !teammate.inherit_plugins {
        overlay
            .entry("syncClaudeAiPlugins")
            .or_insert(serde_json::Value::Bool(false));
    }
    // Remote Control is the orchestrator's. Stated in every pane, so an
    // operator or org default of "on" never reaches a worker.
    overlay
        .entry("remoteControlAtStartup")
        .or_insert(serde_json::Value::Bool(teammate.remote_control));
    let overrides = overlay
        .entry("skillOverrides")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .context("skillOverrides must be an object")?;
    for name in &teammate.disabled_skills {
        overrides.insert(name.clone(), "off".into());
    }
    // The ambient copies of skill-creator go off in every pane. The
    // orchestrator carries its own as `horch:skill-creator`; no worker has one.
    for name in AMBIENT_SKILL_CREATOR {
        overrides.insert(name.to_string(), "off".into());
    }
    super::claude_plugins::overlay_plugin_skills(teammate, overlay, home)?;
    // A plugin skill ignores skillOverrides, in every key form (Claude Code
    // 2.1.283, tested live), so the official plugin goes off as a plugin.
    let plugins = overlay
        .entry("enabledPlugins")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .context("enabledPlugins must be an object")?;
    for key in super::claude_plugins::installed_keys(
        super::claude_plugins::installed_plugins_in(home).as_ref(),
        "skill-creator",
        "skill-creator@claude-plugins-official",
    ) {
        plugins.entry(key).or_insert(serde_json::Value::Bool(false));
    }
    Ok(())
}

/// The names skill-creator loads under as a skill, outside horch's bundle and
/// the plugin: a user or project skill, and the claude.ai-synced copy.
pub(crate) const AMBIENT_SKILL_CREATOR: [&str; 2] =
    ["skill-creator", "anthropic-skills:skill-creator"];
