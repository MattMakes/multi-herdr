//! Claude Code: `claude [variadic flags] --model <m> ... "<prompt>"`.
//!
//! horch mints the session id (`--session-id`), so nothing is discovered
//! after launch. Skills reach it as a `--plugin-dir` plus a `--settings`
//! overlay.

use std::ffi::OsString;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result};

use super::launch::model_for;
use serde_json::{json, Value};

use super::{CommandSpec, Harness, HarnessKind, LaunchEnv, Session, WindowInputs};
use crate::compaction::window::OperatorWindow;
use crate::roster::{expand_home, operator_enabled_plugins, operator_status_line};
use crate::roster::{HarnessDefault, Teammate};
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
        // Each plugin_skills plugin loads as a filtered copy in the bundle.
        adjusted.plugin_dirs = super::claude_plugins::materialize_filtered(teammate, skills, home)?;
        // plugin_dirs precede scalar flags in the Claude builder, which
        // fences variadic --plugin-dir parsing away from the briefing.
        adjusted
            .plugin_dirs
            .push(skills.root().to_string_lossy().into_owned());
        adjusted.settings = Some(skills_settings(teammate, home)?.to_string());
        Ok(adjusted)
    }

    /// The operator's `CLAUDE_CODE_AUTO_COMPACT_WINDOW`: the settings chain,
    /// then the inherited process env (design §6.4).
    fn operator_window(&self, inputs: &WindowInputs<'_>) -> Option<OperatorWindow> {
        let texts = read_claude_settings(
            Some(inputs.claude_managed_settings),
            Some(inputs.home),
            inputs.claude_config_dir,
            Some(inputs.workdir),
        );
        claude_operator_window(
            &texts,
            inputs.teammate.setting_sources.as_deref(),
            inputs.process_window,
        )
    }

    /// The `env.CLAUDE_CODE_AUTO_COMPACT_WINDOW` of the inline `--settings`
    /// JSON. A settings file is not read.
    fn window_in_command(&self, cmd: &Command) -> Option<u64> {
        let args: Vec<_> = cmd.get_args().collect();
        let i = args.iter().position(|a| *a == "--settings")?;
        let doc: Value = serde_json::from_str(args.get(i + 1)?.to_str()?).ok()?;
        window_tokens(doc.get("env")?.get(WINDOW_ENV)?)
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
    let mut cmd = Command::new(&env.bins.claude);

    // ORDERING MATTERS. `--mcp-config`, `--tools`, `--allowedTools` and
    // `--disallowedTools` are variadic (`<x...>`): they keep consuming
    // arguments until the next flag. The prompt is a bare positional at the
    // end, so any variadic flag that ended up last would swallow it as one
    // more config path or tool name. Every variadic flag therefore goes
    // first, and `--model` - always present, never variadic - fences them off.
    push_mcp_args(&mut cmd, env, teammate);
    push_tool_args(&mut cmd, teammate);

    let model = model_for(teammate, model_override)?;
    cmd.arg("--model").arg(model);

    push_model_args(&mut cmd, teammate, model);
    push_plugin_args(&mut cmd, env, teammate)?;
    check_sandbox_host(env, teammate)?;
    push_settings_arg(&mut cmd, env, teammate)?;
    push_session_args(&mut cmd, session);
    // Escape-hatch args sit right before the prompt: a variadic flag at the
    // end of `args` would eat it. `Roster::check` warns about that.
    cmd.args(&teammate.args);
    // An empty prompt is no prompt: a resumed orchestrator starts idle.
    if !prompt.is_empty() {
        cmd.arg(prompt);
    }
    Ok(cmd)
}

/// `--strict-mcp-config` makes `--mcp-config` the whole truth rather than an
/// addition, so an empty `mcpServers` object is what "no MCP servers at all"
/// looks like. Without strict, the operator's servers would still load.
fn push_mcp_args(cmd: &mut Command, env: &LaunchEnv, teammate: &Teammate) {
    if teammate.mcp_servers.is_some() || teammate.worker_channel {
        cmd.arg("--mcp-config")
            .arg(mcp_servers_config(env, teammate));
        for file in &teammate.mcp_config_files {
            cmd.arg("--mcp-config").arg(env.expand_home(file));
        }
        if teammate.mcp_servers.is_some() {
            cmd.arg("--strict-mcp-config");
        }
    } else if !teammate.mcp_config_files.is_empty() {
        cmd.arg("--mcp-config");
        for file in &teammate.mcp_config_files {
            cmd.arg(env.expand_home(file));
        }
    }
}

/// The inline `--mcp-config` JSON: the teammate's servers plus the horch
/// channel server of a worker channel.
fn mcp_servers_config(env: &LaunchEnv, teammate: &Teammate) -> String {
    let mut servers = teammate.mcp_servers.clone().unwrap_or_default();
    if teammate.worker_channel {
        servers.insert(
            "horch".into(),
            json!({
                "command": env.horch_exe,
                "args": ["channel", "serve", "--agent", "claude"]
            }),
        );
    }
    json!({ "mcpServers": servers }).to_string()
}

fn push_tool_args(cmd: &mut Command, teammate: &Teammate) {
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
    if teammate.worker_channel {
        cmd.args(["--dangerously-load-development-channels", "server:horch"]);
    }
}

fn push_model_args(cmd: &mut Command, teammate: &Teammate, model: &str) {
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
}

fn push_plugin_args(cmd: &mut Command, env: &LaunchEnv, teammate: &Teammate) -> Result<()> {
    if teammate.disable_skills {
        cmd.arg("--disable-slash-commands");
    } else {
        // Only a filtered copy hides a plugin's other skills.
        super::claude_plugins::check_filtered(teammate, env.home())?;
    }
    for dir in &teammate.plugin_dirs {
        cmd.arg("--plugin-dir").arg(env.expand_home(dir));
    }
    Ok(())
}

/// A sandboxed teammate never starts on a host that cannot sandbox it.
fn check_sandbox_host(env: &LaunchEnv, teammate: &Teammate) -> Result<()> {
    let Some(problem) = teammate.sandbox.as_ref().and_then(|_| {
        sandbox_host_problem(
            std::env::consts::OS,
            env.path.as_deref(),
            Path::new(SANDBOX_EXEC).exists(),
        )
    }) else {
        return Ok(());
    };
    anyhow::bail!(
        "{}: refusing to launch without a sandbox: {problem}",
        teammate.name
    )
}

/// `--settings` takes a path OR a JSON string, and is not repeatable, so
/// everything horch wants to overlay has to be assembled into one object.
fn push_settings_arg(cmd: &mut Command, env: &LaunchEnv, teammate: &Teammate) -> Result<()> {
    let arg = match &teammate.settings {
        Some(settings) => inline_settings_arg(env, teammate, settings).map(Some),
        None => default_overlay(env, teammate),
    }?;
    if let Some(arg) = arg {
        cmd.arg("--settings").arg(arg);
    }
    Ok(())
}

/// The `--settings` argument for a teammate that names its own settings.
fn inline_settings_arg(env: &LaunchEnv, teammate: &Teammate, settings: &str) -> Result<OsString> {
    let inline = settings.trim_start().starts_with('{');
    // A skills launch gives inline JSON with the sandbox merged in. A file
    // the teammate named would replace the overlay and drop the sandbox.
    if teammate.sandbox.is_some() && !inline {
        anyhow::bail!(
            "{}: sandbox cannot be combined with a settings file",
            teammate.name
        );
    }
    let has_defaults = HarnessDefault::for_harness(&teammate.harness_defaults, HarnessKind::Claude)
        .any(|d| d.settings.is_some());
    let window = applied_window(env);
    if inline && (has_defaults || window.is_some()) {
        merged_inline_settings(env, teammate, settings, window)
    } else {
        // The teammate named its own file; that file owns the overlay.
        Ok(env.expand_home(settings).into_os_string())
    }
}

/// Inline JSON (the teammate's own, or the skills overlay): the harness
/// defaults and the fleet window merge under it. Unchanged text when none
/// adds a key.
fn merged_inline_settings(
    env: &LaunchEnv,
    teammate: &Teammate,
    settings: &str,
    window: Option<u64>,
) -> Result<OsString> {
    let mut value: Value =
        serde_json::from_str(settings).context("invalid teammate settings JSON")?;
    let obj = value
        .as_object_mut()
        .context("teammate settings must be a JSON object")?;
    let changed = merge_settings_defaults(teammate, obj, || operator_settings(env))?
        | overlay_window(window, obj);
    Ok(if changed {
        value.to_string().into()
    } else {
        settings.into()
    })
}

/// The `--settings` overlay of a teammate with no settings of its own, as
/// JSON text; `None` when it would be empty.
fn default_overlay(env: &LaunchEnv, teammate: &Teammate) -> Result<Option<OsString>> {
    let mut overlay = serde_json::Map::new();
    disable_operator_plugins(&mut overlay, teammate, env.home())?;
    // Whatever inherit_plugins says: the generic tiers inherit plugins, but
    // no pane has a use for the operator's claude.ai skills.
    overlay_skill_switches(teammate, &mut overlay, env.home())?;
    copy_operator_status_line(&mut overlay, teammate, env.home());
    overlay_sandbox(teammate, &mut overlay)?;
    merge_settings_defaults(teammate, &mut overlay, || operator_settings(env))?;
    overlay_window(applied_window(env), &mut overlay);
    Ok((!overlay.is_empty()).then(|| Value::Object(overlay).to_string().into()))
}

fn push_session_args(cmd: &mut Command, session: Session<'_>) {
    if let Session::Fresh(id) = session {
        cmd.arg("--session-id").arg(id);
    } else if let Session::Resume(id) = session {
        cmd.arg("--resume").arg(id);
    }
}

/// Unless the teammate inherits plugins, switch the operator's
/// globally-enabled plugins off, by name, for this session. Settings merge per key, so this touches nothing else - verified
/// against a live launch: "Found 3 plugins (0 enabled, 3 disabled)" with
/// every other setting intact.
pub(crate) fn disable_operator_plugins(
    obj: &mut serde_json::Map<String, Value>,
    teammate: &Teammate,
    home: Option<&Path>,
) -> Result<()> {
    if teammate.inherit_plugins {
        return Ok(());
    }
    for name in operator_enabled_plugins(home) {
        object_entry(obj, "enabledPlugins")?.insert(name, json!(false));
    }
    Ok(())
}

/// Restricting settings would take the status line with it. Every pane in a
/// fleet keeps the operator's, so a worker reads like any session.
fn copy_operator_status_line(
    obj: &mut serde_json::Map<String, Value>,
    teammate: &Teammate,
    home: Option<&Path>,
) {
    let wanted = teammate.setting_sources.is_some() && !obj.contains_key("statusLine");
    if let Some(status) = wanted.then(|| operator_status_line(home)).flatten() {
        obj.insert("statusLine".into(), status);
    }
}

/// The object under `key`, made empty when it is missing.
fn object_entry<'a>(
    map: &'a mut serde_json::Map<String, Value>,
    key: &str,
) -> Result<&'a mut serde_json::Map<String, Value>> {
    map.entry(key)
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .with_context(|| format!("{key} must be an object"))
}

/// The `--settings` overlay of a launch with a skills bundle: the teammate's
/// own settings (inline JSON or a file), with horch's switches merged in.
fn skills_settings(teammate: &Teammate, home: Option<&Path>) -> Result<Value> {
    let mut settings = read_settings_value(teammate.settings.as_deref(), home)?;
    let obj = settings
        .as_object_mut()
        .context("teammate settings must be a JSON object")?;
    obj.entry("disableBundledSkills").or_insert(json!(true));
    obj.entry("disableWorkflows").or_insert(json!(true));
    disable_operator_plugins(obj, teammate, home)?;
    // After the plugin switch-off: a plugin_skills plugin stays enabled.
    overlay_skill_switches(teammate, obj, home)?;
    copy_operator_status_line(obj, teammate, home);
    overlay_sandbox(teammate, obj)?;
    Ok(settings)
}

/// A teammate's `settings` source as JSON: inline text, or a file to read.
fn read_settings_value(source: Option<&str>, home: Option<&Path>) -> Result<Value> {
    let Some(source) = source else {
        return Ok(json!({}));
    };
    let text = if source.trim_start().starts_with('{') {
        source.to_string()
    } else {
        std::fs::read_to_string(expand_home(source, home)).context("reading teammate settings")?
    };
    serde_json::from_str(&text).context("invalid teammate settings JSON")
}

/// The texts of the operator's Claude settings files, for one launch. A
/// file that does not exist is `None`.
#[derive(Debug, Clone, Default)]
pub(crate) struct ClaudeSettingsTexts {
    pub managed: Option<String>,
    pub local: Option<String>,
    pub project: Option<String>,
    pub user: Option<String>,
    /// Where `user` was read from, for the detail.
    pub user_path: String,
}

/// The operator's settings files for a launch in `env`.
fn operator_settings(env: &LaunchEnv) -> ClaudeSettingsTexts {
    read_claude_settings(
        env.claude_managed_settings.as_deref(),
        env.home(),
        env.claude_config_dir.as_deref(),
        env.workdir.as_deref(),
    )
}

/// Read the settings files of the operator chain. `managed` is
/// `RuntimeContext.paths.claude_managed_settings`; `None` reads no managed
/// file. The user file is `<config_dir or home/.claude>/settings.json`.
/// The project files belong to the repository, so each read is bounded: a
/// FIFO, a device or a file over 1 MiB counts as absent.
pub(crate) fn read_claude_settings(
    managed: Option<&Path>,
    home: Option<&Path>,
    config_dir: Option<&Path>,
    workdir: Option<&Path>,
) -> ClaudeSettingsTexts {
    let read = |p: &Path| crate::fsx::read_regular_bounded(p).ok().flatten();
    let user_path = config_dir
        .map(Path::to_path_buf)
        .or_else(|| home.map(|h| h.join(".claude")))
        .map(|d| d.join("settings.json"));
    ClaudeSettingsTexts {
        managed: managed.and_then(read),
        local: workdir.and_then(|w| read(&w.join(".claude/settings.local.json"))),
        project: workdir.and_then(|w| read(&w.join(".claude/settings.json"))),
        user: user_path.as_deref().and_then(read),
        user_path: user_path
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default(),
    }
}

/// The first source of the operator chain (managed, local, project, user)
/// that sets the dotted `path`, as (value, detail). A source other than
/// managed counts only when `sources` is `None` or names it (the teammate's
/// `setting_sources`).
pub(crate) fn claude_operator_value(
    t: &ClaudeSettingsTexts,
    sources: Option<&[String]>,
    path: &str,
) -> Option<(Value, String)> {
    operator_docs(t, sources).find_map(|(doc, detail)| Some((json_at(&doc, path)?.clone(), detail)))
}

/// The parsed files of the operator chain that count, in order, each with
/// its detail. A file that is not valid JSON is skipped.
fn operator_docs<'a>(
    t: &'a ClaudeSettingsTexts,
    sources: Option<&'a [String]>,
) -> impl Iterator<Item = (Value, String)> + 'a {
    let named = |name: &str| sources.is_none_or(|s| s.iter().any(|x| x == name));
    let chain = [
        (t.managed.as_deref(), true, "managed settings".to_string()),
        (
            t.local.as_deref(),
            named("local"),
            ".claude/settings.local.json".into(),
        ),
        (
            t.project.as_deref(),
            named("project"),
            ".claude/settings.json".into(),
        ),
        (t.user.as_deref(), named("user"), t.user_path.clone()),
    ];
    chain.into_iter().filter_map(|(text, on, detail)| {
        let doc: Value = serde_json::from_str(text.filter(|_| on)?).ok()?;
        Some((doc, detail))
    })
}

/// The env key of Claude's auto-compact window.
const WINDOW_ENV: &str = "CLAUDE_CODE_AUTO_COMPACT_WINDOW";
/// The env key that makes the window a percentage: the trigger is unknown.
const PCT_OVERRIDE_ENV: &str = "CLAUDE_AUTOCOMPACT_PCT_OVERRIDE";

/// The window the launch applies: the decision's tokens when it is applied.
fn applied_window(env: &LaunchEnv) -> Option<u64> {
    env.compact_window
        .as_ref()
        .filter(|d| d.applied)
        .and_then(|d| d.tokens)
}

/// Put the fleet window into a `--settings` overlay as
/// `env.CLAUDE_CODE_AUTO_COMPACT_WINDOW`, at the harness-default level: an
/// overlay that sets the key (or a non-object `env`) keeps its value, and
/// the other `env` keys stay. Returns whether the overlay changed.
fn overlay_window(window: Option<u64>, overlay: &mut serde_json::Map<String, Value>) -> bool {
    let Some(n) = window else {
        return false;
    };
    let Some(env) = overlay
        .entry("env")
        .or_insert_with(|| json!({}))
        .as_object_mut()
    else {
        return false;
    };
    if env.contains_key(WINDOW_ENV) {
        return false;
    }
    env.insert(WINDOW_ENV.into(), Value::String(n.to_string()));
    true
}

/// A window value as tokens: a number, or a string that holds one.
fn window_tokens(v: &Value) -> Option<u64> {
    match v {
        Value::Number(n) => n.as_u64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// The operator's Claude window over the settings texts (design §6.4): the
/// first source of the chain whose `env` sets the window, else the process
/// env. A file whose `env` also sets `CLAUDE_AUTOCOMPACT_PCT_OVERRIDE`, or
/// a value that is not a number, gives `tokens: None`.
pub(crate) fn claude_operator_window(
    t: &ClaudeSettingsTexts,
    sources: Option<&[String]>,
    process_window: Option<&str>,
) -> Option<OperatorWindow> {
    operator_docs(t, sources)
        .find_map(|(doc, detail)| {
            let env = doc.get("env")?;
            let value = env.get(WINDOW_ENV)?;
            Some(if env.get(PCT_OVERRIDE_ENV).is_some() {
                OperatorWindow {
                    tokens: None,
                    detail: format!("{detail} ({PCT_OVERRIDE_ENV} set; trigger unknown)"),
                }
            } else {
                parse_window(value, detail)
            })
        })
        .or_else(|| {
            let process = Value::String(process_window?.to_string());
            Some(parse_window(&process, format!("process env {WINDOW_ENV}")))
        })
}

fn parse_window(v: &Value, detail: String) -> OperatorWindow {
    match window_tokens(v) {
        Some(n) => OperatorWindow {
            tokens: Some(n),
            detail,
        },
        None => OperatorWindow {
            tokens: None,
            detail: format!("{detail} (not a number)"),
        },
    }
}

/// The value at a dotted key path in a JSON document.
fn json_at<'a>(doc: &'a Value, path: &str) -> Option<&'a Value> {
    path.split('.').try_fold(doc, |node, key| node.get(key))
}

/// Every leaf key path of a settings object, dotted: a nested object is
/// walked, any other value (or an empty object) is a leaf.
fn leaf_paths(obj: &serde_json::Map<String, Value>, prefix: &str, out: &mut Vec<(String, Value)>) {
    for (key, value) in obj {
        let path = if prefix.is_empty() {
            key.clone()
        } else {
            format!("{prefix}.{key}")
        };
        match value {
            Value::Object(inner) if !inner.is_empty() => leaf_paths(inner, &path, out),
            _ => out.push((path, value.clone())),
        }
    }
}

/// Whether the overlay sets `parents.key`, or a parent of it as a
/// non-object: either way a default cannot go there.
fn overlay_blocks(overlay: &serde_json::Map<String, Value>, parents: &[&str], key: &str) -> bool {
    let mut node = overlay;
    for parent in parents {
        let Some(child) = node.get(*parent) else {
            return false;
        };
        let Value::Object(inner) = child else {
            return true;
        };
        node = inner;
    }
    node.contains_key(key)
}

/// The operator's settings files, read at most once and only when a
/// non-forced default key asks whether the operator already sets it.
struct OperatorChain<'a, F> {
    texts: Option<F>,
    read: Option<ClaudeSettingsTexts>,
    sources: Option<&'a [String]>,
}

impl<F: FnOnce() -> ClaudeSettingsTexts> OperatorChain<'_, F> {
    fn sets(&mut self, path: &str) -> bool {
        let chain = self
            .read
            .get_or_insert_with(|| (self.texts.take().expect("read once"))());
        claude_operator_value(chain, self.sources, path).is_some()
    }
}

/// The leaf key paths of harness-default settings objects as (path, value,
/// forced), from entries of (settings, forced). A later entry wins.
pub(super) fn default_leaves<'a>(
    entries: impl Iterator<Item = (&'a serde_json::Map<String, Value>, bool)>,
) -> Vec<(String, Value, bool)> {
    let mut leaves: Vec<(String, Value, bool)> = Vec::new();
    for (settings, forced) in entries {
        let mut paths = Vec::new();
        leaf_paths(settings, "", &mut paths);
        for (path, value) in paths {
            leaves.retain(|(p, _, _)| *p != path);
            leaves.push((path, value, forced));
        }
    }
    leaves
}

/// Merge the teammate's Claude harness-default `settings` into a
/// `--settings` overlay at the lowest precedence. A key path is left out
/// when the overlay has it (the teammate's inline settings or horch's fixed
/// switches), or when the operator chain sets it and the entry has no
/// `force`. Among the defaults, a later entry wins. `texts` is read only
/// when a non-forced key needs it. Returns whether the overlay changed.
pub(crate) fn merge_settings_defaults(
    teammate: &Teammate,
    overlay: &mut serde_json::Map<String, Value>,
    texts: impl FnOnce() -> ClaudeSettingsTexts,
) -> Result<bool> {
    let mut chain = OperatorChain {
        texts: Some(texts),
        read: None,
        sources: teammate.setting_sources.as_deref(),
    };
    let mut changed = false;
    let entries = HarnessDefault::for_harness(&teammate.harness_defaults, HarnessKind::Claude)
        .filter_map(|entry| Some((entry.settings.as_ref()?, entry.force.is_some())));
    for (path, value, forced) in default_leaves(entries) {
        let mut parents: Vec<&str> = path.split('.').collect();
        let key = parents.pop().expect("a non-empty key path");
        let skipped = overlay_blocks(overlay, &parents, key) || (!forced && chain.sets(&path));
        if !skipped {
            anyhow::ensure!(
                insert_at_path(overlay, &parents, key, value),
                "checked above: an object"
            );
            changed = true;
        }
    }
    Ok(changed)
}

/// Set `parents.key` in `overlay`, making the parent objects; whether it
/// was set. A parent that is not an object is left alone, and so is the
/// value.
pub(super) fn insert_at_path(
    overlay: &mut serde_json::Map<String, Value>,
    parents: &[&str],
    key: &str,
    value: Value,
) -> bool {
    let node = parents.iter().try_fold(overlay, |node, parent| {
        node.entry(*parent)
            .or_insert_with(|| json!({}))
            .as_object_mut()
    });
    node.map(|node| node.insert(key.to_string(), value))
        .is_some()
}

/// Where macOS keeps the Seatbelt launcher Claude Code's sandbox runs under.
const SANDBOX_EXEC: &str = "/usr/bin/sandbox-exec";

/// The `sandbox` keys horch sets on every sandboxed launch, whatever the
/// teammate file says, as (path, value). `check` rejects a file that sets
/// one to anything else.
/// - `enabled`: the field means "sandbox this pane".
/// - `failIfUnavailable`: Claude Code exits at startup rather than run
///   commands unsandboxed.
/// - `allowUnsandboxedCommands: false`: no retry outside the sandbox. Given
///   through `--settings`, it also makes the sandbox admin-required, so a
///   repository's `.claude/settings.json` cannot widen it.
/// - `network.strictAllowlist`: a host outside `allowedDomains` is denied in
///   every permission mode, auto mode's per-command domains included.
pub(crate) const SANDBOX_FORCED: [(&[&str], bool); 4] = [
    (&["enabled"], true),
    (&["failIfUnavailable"], true),
    (&["allowUnsandboxedCommands"], false),
    (&["network", "strictAllowlist"], true),
];

/// Put the teammate's `sandbox:` block into a `--settings` object, with the
/// [`SANDBOX_FORCED`] keys set. The sandbox wraps Bash only, so the Read,
/// Grep and Glob tools get `permissions.blockReadsOutsideWorkingDirectories`:
/// without it they read what the sandbox denies to a shell command. With the
/// sandbox on, the same key also closes the home directory to sandboxed
/// commands; the block's `allowRead` re-opens what the teammate needs.
pub(crate) fn overlay_sandbox(
    teammate: &Teammate,
    overlay: &mut serde_json::Map<String, Value>,
) -> Result<()> {
    let Some(block) = &teammate.sandbox else {
        return Ok(());
    };
    let mut sandbox = Value::Object(block.clone());
    for (path, value) in SANDBOX_FORCED {
        let (key, parents) = path.split_last().expect("a non-empty key path");
        let mut node = &mut sandbox;
        for parent in parents {
            node = node
                .as_object_mut()
                .context("sandbox must be an object")?
                .entry(*parent)
                .or_insert_with(|| json!({}));
        }
        node.as_object_mut()
            .with_context(|| format!("sandbox.{} must be an object", parents.join(".")))?
            .insert((*key).to_string(), Value::Bool(value));
    }
    overlay.insert("sandbox".into(), sandbox);
    overlay
        .entry("permissions")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .context("permissions must be an object")?
        .insert(
            "blockReadsOutsideWorkingDirectories".into(),
            Value::Bool(true),
        );
    Ok(())
}

/// What in a teammate's `sandbox:` block would open a way out, one line
/// each: a value against a [`SANDBOX_FORCED`] key, `excludedCommands` (runs
/// a command outside the sandbox) or `filesystem.disabled`.
pub(crate) fn sandbox_problems(block: &serde_json::Map<String, Value>) -> Vec<String> {
    let mut problems = Vec::new();
    for (path, forced) in SANDBOX_FORCED {
        let mut node = Some(block);
        let (key, parents) = path.split_last().expect("a non-empty key path");
        for parent in parents {
            node = node.and_then(|n| n.get(*parent)).and_then(Value::as_object);
        }
        if let Some(value) = node.and_then(|n| n.get(*key)) {
            if value != &Value::Bool(forced) {
                problems.push(format!(
                    "sandbox.{} must be {forced} or left out (horch sets it)",
                    path.join(".")
                ));
            }
        }
    }
    let excluded = block.get("excludedCommands");
    if excluded.is_some_and(|v| v.as_array().is_none_or(|a| !a.is_empty())) {
        problems.push(
            "sandbox.excludedCommands runs commands outside the sandbox; leave it out".into(),
        );
    }
    let disabled = block.get("filesystem").and_then(|f| f.get("disabled"));
    if disabled.is_some_and(|v| v != &Value::Bool(false)) {
        problems.push("sandbox.filesystem.disabled turns the filesystem layer off".into());
    }
    problems
}

/// Why this host cannot run Claude Code's sandbox, or `None` when it can.
/// macOS needs Seatbelt's `sandbox-exec`; Linux needs `bwrap` and `socat`
/// (code.claude.com/docs/en/sandboxing, "Set up Linux and WSL2"). The
/// sandbox runs nowhere else.
pub(crate) fn sandbox_host_problem(
    os: &str,
    path: Option<&std::ffi::OsStr>,
    sandbox_exec: bool,
) -> Option<String> {
    match os {
        "macos" if sandbox_exec => None,
        "macos" => Some(format!("{SANDBOX_EXEC} is missing")),
        "linux" => {
            let missing: Vec<&str> = ["bwrap", "socat"]
                .into_iter()
                .filter(|bin| crate::runtime::process::which(path, None, bin).is_none())
                .collect();
            (!missing.is_empty()).then(|| {
                format!(
                    "{} not on PATH; install bubblewrap and socat",
                    missing.join(" and ")
                )
            })
        }
        other => Some(format!("Claude Code has no sandbox on {other}")),
    }
}

/// A `PATH` of 1 temp directory for a sandbox test: with fake `bwrap` and
/// `socat` executables when `tools` is true, else empty. The Linux sandbox
/// check looks there, so a test does not depend on the host's PATH.
#[cfg(test)]
pub(crate) fn sandbox_tools_path(dir: &Path, tools: bool) -> std::ffi::OsString {
    std::fs::create_dir_all(dir).unwrap();
    if tools {
        for bin in ["bwrap", "socat"] {
            let path = dir.join(bin);
            std::fs::write(&path, "#!/bin/sh\n").unwrap();
            #[cfg(unix)]
            crate::runtime::process::make_executable(&path).unwrap();
        }
    }
    std::env::join_paths([dir]).unwrap()
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
/// launch (Claude Code 2.1.276).
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
    let overrides = object_entry(overlay, "skillOverrides")?;
    for name in &teammate.disabled_skills {
        overrides.insert(name.clone(), "off".into());
    }
    // The ambient copies of skill-creator go off in every pane. The
    // orchestrator carries its own as `horch:skill-creator`; no worker has one.
    for name in AMBIENT_SKILL_CREATOR {
        overrides.insert(name.to_string(), "off".into());
    }
    super::claude_plugins::overlay_plugin_skills(teammate, overlay, home)?;
    disable_skill_plugins(teammate, overlay, home)
}

/// A plugin skill ignores skillOverrides, in every key form (Claude Code
/// 2.1.283, tested live), so the official plugin goes off as a plugin. For
/// the same reason a disabled `<plugin>:<skill>` switches its whole plugin
/// off (Claude Code 2.1.289).
/// `or_insert` keeps what plugin_skills set. Its filtered copy is
/// `<plugin>@inline`, a key this never names, so it stays on.
fn disable_skill_plugins(
    teammate: &Teammate,
    overlay: &mut serde_json::Map<String, Value>,
    home: Option<&Path>,
) -> Result<()> {
    let plugins = object_entry(overlay, "enabledPlugins")?;
    let installed = super::claude_plugins::installed_plugins_in(home);
    let skill_creator = super::claude_plugins::installed_keys(
        installed.as_ref(),
        "skill-creator",
        "skill-creator@claude-plugins-official",
    );
    let enabled = operator_enabled_plugins(home);
    let disabled = teammate
        .disabled_skills
        .iter()
        .filter_map(|name| name.split_once(':'))
        .flat_map(|(plugin, _)| plugin_keys(plugin, &enabled, installed.as_ref()));
    for key in skill_creator.into_iter().chain(disabled) {
        plugins.entry(key).or_insert(Value::Bool(false));
    }
    Ok(())
}

/// Every `name@marketplace` key under which `plugin` is enabled in the
/// operator's settings or installed, sorted. Empty when it is neither.
fn plugin_keys(plugin: &str, enabled: &[String], installed: Option<&Value>) -> Vec<String> {
    let registered = installed
        .and_then(|v| v.get("plugins"))
        .and_then(|p| p.as_object())
        .map(|p| p.keys().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    let mut keys: Vec<String> = enabled
        .iter()
        .cloned()
        .chain(registered)
        .filter(|k| k.split('@').next() == Some(plugin))
        .collect();
    keys.sort();
    keys.dedup();
    keys
}

/// The names skill-creator loads under as a skill, outside horch's bundle and
/// the plugin: a user or project skill, and the claude.ai-synced copy.
pub(crate) const AMBIENT_SKILL_CREATOR: [&str; 2] =
    ["skill-creator", "anthropic-skills:skill-creator"];

#[cfg(test)]
mod tests {
    use super::*;

    fn command_args(cmd: &Command) -> Vec<String> {
        cmd.get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn arc_30_claude_orchestrator_loads_the_horch_channel() {
        let mut env = LaunchEnv::for_test();
        env.horch_exe = "/opt/horch test".into();
        let teammate = Teammate {
            model: Some("sonnet".into()),
            worker_channel: true,
            mcp_servers: Some(Default::default()),
            ..Default::default()
        };
        let args = command_args(
            &claude_command(&env, &teammate, Session::Unmanaged, "the prompt", None).unwrap(),
        );
        let config_at = args.iter().position(|arg| arg == "--mcp-config").unwrap();
        let config: Value = serde_json::from_str(&args[config_at + 1]).unwrap();
        assert_eq!(config["mcpServers"]["horch"]["command"], "/opt/horch test");
        assert_eq!(
            config["mcpServers"]["horch"]["args"],
            json!(["channel", "serve", "--agent", "claude"])
        );
        assert!(args.iter().any(|arg| arg == "--strict-mcp-config"));
        let channel_at = args
            .iter()
            .position(|arg| arg == "--dangerously-load-development-channels")
            .unwrap();
        assert_eq!(args[channel_at + 1], "server:horch");
        let model_at = args.iter().position(|arg| arg == "--model").unwrap();
        assert!(channel_at < model_at, "{args:?}");
        assert_eq!(args.last().map(String::as_str), Some("the prompt"));
    }

    /// A resumed fleet orchestrator gets no prompt: `--resume <id>` ends
    /// the command line, with no empty argument after it.
    #[test]
    fn an_empty_prompt_is_left_off_the_command_line() {
        let teammate = Teammate {
            model: Some("opus".into()),
            ..Default::default()
        };
        let args = command_args(
            &claude_command(
                &LaunchEnv::for_test(),
                &teammate,
                Session::Resume("s-1"),
                "",
                None,
            )
            .unwrap(),
        );
        assert_eq!(&args[args.len() - 2..], ["--resume", "s-1"], "{args:?}");
    }

    #[test]
    fn arc_30_claude_channel_without_mcp_servers_is_not_strict() {
        let teammate = Teammate {
            model: Some("sonnet".into()),
            worker_channel: true,
            ..Default::default()
        };
        let args = command_args(
            &claude_command(
                &LaunchEnv::for_test(),
                &teammate,
                Session::Unmanaged,
                "p",
                None,
            )
            .unwrap(),
        );
        assert!(args.iter().any(|arg| arg == "--mcp-config"));
        assert!(!args.iter().any(|arg| arg == "--strict-mcp-config"));
    }

    #[test]
    fn arc_30_claude_without_worker_channel_adds_nothing() {
        let teammate = Teammate {
            model: Some("sonnet".into()),
            ..Default::default()
        };
        let args = command_args(
            &claude_command(
                &LaunchEnv::for_test(),
                &teammate,
                Session::Unmanaged,
                "p",
                None,
            )
            .unwrap(),
        );
        assert!(!args.iter().any(|arg| arg == "--mcp-config"));
        assert!(!args
            .iter()
            .any(|arg| arg == "--dangerously-load-development-channels"));
    }

    /// Run `read` in a thread; panic when it does not return within 1 s.
    #[cfg(unix)]
    fn within_1s<T: Send + 'static>(read: impl FnOnce() -> T + Send + 'static) -> T {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(read());
        });
        rx.recv_timeout(std::time::Duration::from_secs(1))
            .expect("the read blocked")
    }

    /// A FIFO at the repository's `.claude/settings.json` neither blocks the
    /// operator chain read nor counts as a file (u9b step 10).
    #[cfg(unix)]
    #[test]
    fn claude_project_settings_fifo_does_not_block() {
        let tmp = tempfile::tempdir().unwrap();
        let workdir = tmp.path().join("project");
        crate::test_support::mkfifo(&workdir.join(".claude/settings.json"));
        std::fs::write(workdir.join(".claude/settings.local.json"), "{}").unwrap();
        let texts = within_1s(move || read_claude_settings(None, None, None, Some(&workdir)));
        assert_eq!(texts.project, None);
        assert_eq!(texts.local.as_deref(), Some("{}"));
    }

    /// HDF-02: the operator chain is managed, then local, then project, then
    /// user; `setting_sources` drops the sources it does not name, never
    /// managed. A dotted path walks nested objects.
    #[test]
    fn hdf_02_claude_operator_value_follows_the_chain_order() {
        let texts = ClaudeSettingsTexts {
            managed: Some(r#"{"a": "managed"}"#.into()),
            local: Some(r#"{"a": "local", "b": "local"}"#.into()),
            project: Some(r#"{"a": "project", "b": "project", "c": "project"}"#.into()),
            user: Some(r#"{"a": "user", "b": "user", "c": "user", "d": {"e": "user"}}"#.into()),
            user_path: "/u/settings.json".into(),
        };
        let value =
            |sources: Option<&[String]>, path: &str| claude_operator_value(&texts, sources, path);
        assert_eq!(value(None, "a").unwrap().0, json!("managed"));
        assert_eq!(value(None, "b").unwrap().0, json!("local"));
        assert_eq!(value(None, "c").unwrap().0, json!("project"));
        let (v, detail) = value(None, "d.e").unwrap();
        assert_eq!((v, detail.as_str()), (json!("user"), "/u/settings.json"));
        assert_eq!(value(None, "z"), None);

        let project = ["project".to_string()];
        assert_eq!(value(Some(&project), "a").unwrap().0, json!("managed"));
        assert_eq!(value(Some(&project), "b").unwrap().0, json!("project"));
        assert_eq!(value(Some(&project), "d.e"), None, "user is not named");
        assert_eq!(value(Some(&[]), "c"), None);
    }

    /// A plugin skill ignores `skillOverrides`, so a disabled `herdr:<skill>`
    /// switches the herdr plugin off under every key it is enabled or
    /// installed as. A bare name and a plugin nobody has add no plugin key.
    #[test]
    fn a_disabled_plugin_skill_switches_its_plugin_off() {
        let home = tempfile::tempdir().unwrap();
        let claude = home.path().join(".claude");
        std::fs::create_dir_all(claude.join("plugins")).unwrap();
        std::fs::write(
            claude.join("settings.json"),
            r#"{"enabledPlugins": {"herdr@m": true, "ddd@m": true}}"#,
        )
        .unwrap();
        std::fs::write(
            claude.join("plugins/installed_plugins.json"),
            r#"{"version": 2, "plugins": {"herdr@fork": [{"installPath": "/x"}]}}"#,
        )
        .unwrap();
        let teammate = Teammate {
            disabled_skills: vec![
                "herdr-worker".into(),
                "herdr:herdr-worker".into(),
                "herdr:herdr-orchestrator".into(),
                "missing:skill".into(),
            ],
            ..Teammate::default()
        };
        let mut overlay = serde_json::Map::new();
        overlay_skill_switches(&teammate, &mut overlay, Some(home.path())).unwrap();
        assert_eq!(
            overlay["enabledPlugins"],
            json!({
                "herdr@fork": false,
                "herdr@m": false,
                "skill-creator@claude-plugins-official": false
            })
        );
        // The bare names still go off: they hide the user-level copies.
        assert_eq!(overlay["skillOverrides"]["herdr-worker"], "off");
    }

    /// The filtered copy that plugin_skills switches on stays on when a
    /// disabled skill switches its plugin off.
    #[test]
    fn plugin_skills_wins_over_a_disabled_plugin_skill() {
        let mut overlay = serde_json::Map::new();
        overlay.insert(
            "enabledPlugins".into(),
            json!({"herdr@m": false, "herdr@inline": true}),
        );
        let plugins = overlay["enabledPlugins"].as_object_mut().unwrap();
        for key in plugin_keys("herdr", &["herdr@m".into()], None) {
            plugins.entry(key).or_insert(json!(false));
        }
        assert_eq!(
            overlay["enabledPlugins"],
            json!({"herdr@m": false, "herdr@inline": true})
        );
        assert!(plugin_keys("herdr", &["herdrx@m".into()], None).is_empty());
    }

    fn sandboxed(block: Value) -> Teammate {
        Teammate {
            name: "boxed".into(),
            model: Some("sonnet".into()),
            sandbox: Some(block.as_object().unwrap().clone()),
            ..Teammate::default()
        }
    }

    /// Whether a launch in `env` passes the host sandbox check.
    fn host_can_sandbox(env: &LaunchEnv) -> bool {
        sandbox_host_problem(
            std::env::consts::OS,
            env.path.as_deref(),
            Path::new(SANDBOX_EXEC).exists(),
        )
        .is_none()
    }

    /// 2 launch environments: PATH with the Linux sandbox tools, and PATH
    /// without them. The temp dir must outlive them.
    fn sandbox_envs(tmp: &Path) -> [LaunchEnv; 2] {
        [true, false].map(|tools| LaunchEnv {
            path: Some(sandbox_tools_path(&tmp.join(tools.to_string()), tools)),
            ..LaunchEnv::for_test()
        })
    }

    fn settings_arg(cmd: &Command) -> Value {
        let args: Vec<String> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        let i = args.iter().position(|a| a == "--settings").unwrap();
        serde_json::from_str(&args[i + 1]).unwrap()
    }

    /// The launch writes the teammate's block into `--settings`, with the
    /// forced keys set over what the file said, and closes the Read tool.
    /// On a host with no sandbox, the same launch is refused instead.
    #[test]
    fn a_sandboxed_launch_writes_the_sandbox_block_or_refuses() {
        let t = sandboxed(json!({
            "network": {"allowedDomains": ["api.appstoreconnect.apple.com"]},
            "filesystem": {"allowRead": ["~/.config/horch/asc"]}
        }));
        let tmp = tempfile::tempdir().unwrap();
        for env in sandbox_envs(tmp.path()) {
            let built =
                super::super::launch::command_in(&env, &t, Session::Fresh("id"), "go", None);
            if !host_can_sandbox(&env) {
                let err = built.unwrap_err().to_string();
                assert!(
                    err.contains("refusing to launch without a sandbox"),
                    "{err}"
                );
                continue;
            }
            let settings = settings_arg(&built.unwrap());
            assert_eq!(
                settings["sandbox"],
                json!({
                    "enabled": true,
                    "failIfUnavailable": true,
                    "allowUnsandboxedCommands": false,
                    "network": {
                        "allowedDomains": ["api.appstoreconnect.apple.com"],
                        "strictAllowlist": true
                    },
                    "filesystem": {"allowRead": ["~/.config/horch/asc"]}
                })
            );
            assert_eq!(
                settings["permissions"]["blockReadsOutsideWorkingDirectories"],
                true
            );
        }
    }

    /// The shipped release preparer launches inside the sandbox: only the
    /// App Store Connect API host, the key directory re-opened, asc's
    /// credential variables unset, and no way out.
    #[test]
    fn the_release_preparer_launches_sandboxed() {
        let roster = crate::roster::Roster::builtin().unwrap();
        let t = roster.require("app-release-preparer").unwrap();
        let block = t.sandbox.as_ref().expect("a sandbox block");
        assert!(sandbox_problems(block).is_empty());
        for tool in ["WebFetch", "WebSearch", "Bash(asc builds upload *)"] {
            assert!(t.disallowed_tools.iter().any(|d| d == tool), "{tool}");
        }
        let mut overlay = serde_json::Map::new();
        overlay_sandbox(t, &mut overlay).unwrap();
        let sandbox = &overlay["sandbox"];
        assert_eq!(
            sandbox["network"]["allowedDomains"],
            json!(["api.appstoreconnect.apple.com"])
        );
        assert_eq!(sandbox["network"]["strictAllowlist"], true);
        assert_eq!(sandbox["allowUnsandboxedCommands"], false);
        assert_eq!(sandbox["failIfUnavailable"], true);
        let fs = &sandbox["filesystem"];
        for denied in ["~/.asc", "~/Library/Keychains", "~/.config"] {
            assert!(fs["denyRead"].as_array().unwrap().contains(&json!(denied)));
        }
        assert!(fs["allowRead"]
            .as_array()
            .unwrap()
            .contains(&json!("~/.config/horch/asc")));
        let vars = sandbox["credentials"]["envVars"].as_array().unwrap();
        assert!(vars.contains(&json!({"name": "ASC_PRIVATE_KEY_PATH", "mode": "deny"})));
        assert_eq!(
            overlay["permissions"]["blockReadsOutsideWorkingDirectories"],
            true
        );
    }

    /// A fleet pane launches with skills: the sandbox rides in the merged
    /// skills overlay too, and a teammate without the field gets no block.
    #[test]
    fn the_skills_overlay_carries_the_sandbox() {
        let settings = skills_settings(&sandboxed(json!({"enabled": false})), None).unwrap();
        assert_eq!(settings["sandbox"]["enabled"], true);
        assert_eq!(settings["sandbox"]["allowUnsandboxedCommands"], false);
        let plain = skills_settings(&Teammate::default(), None).unwrap();
        assert!(plain.get("sandbox").is_none());
        assert!(plain.get("permissions").is_none());
    }

    /// The Linux check finds `bwrap` and `socat` on the launch's PATH, and
    /// names what is missing. Run on every host, macOS included.
    #[cfg(unix)]
    #[test]
    fn the_linux_sandbox_check_reads_the_launch_path() {
        let tmp = tempfile::tempdir().unwrap();
        let [with, without] = sandbox_envs(tmp.path());
        assert_eq!(
            sandbox_host_problem("linux", with.path.as_deref(), false),
            None
        );
        let problem = sandbox_host_problem("linux", without.path.as_deref(), false).unwrap();
        assert!(problem.contains("bwrap and socat not on PATH"), "{problem}");
        assert!(sandbox_host_problem("linux", None, false).is_some());
    }

    /// A settings file replaces the overlay, so it would drop the sandbox.
    #[test]
    fn a_sandbox_with_a_settings_file_is_refused() {
        let mut t = sandboxed(json!({}));
        t.settings = Some("~/my-settings.json".into());
        let tmp = tempfile::tempdir().unwrap();
        for env in sandbox_envs(tmp.path()) {
            let err = super::super::launch::command_in(&env, &t, Session::Unmanaged, "go", None)
                .unwrap_err()
                .to_string();
            let expected = if host_can_sandbox(&env) {
                "sandbox cannot be combined with a settings file"
            } else {
                "refusing to launch without a sandbox"
            };
            assert!(err.contains(expected), "{err}");
        }
    }

    #[test]
    fn every_way_out_of_the_sandbox_is_named() {
        let block = json!({
            "enabled": false,
            "failIfUnavailable": true,
            "allowUnsandboxedCommands": true,
            "excludedCommands": ["asc *"],
            "filesystem": {"disabled": true},
            "network": {"strictAllowlist": false}
        });
        let problems = sandbox_problems(block.as_object().unwrap());
        assert_eq!(problems.len(), 5, "{problems:?}");
        assert!(problems[0].starts_with("sandbox.enabled must be true"));
        assert!(problems[1].starts_with("sandbox.allowUnsandboxedCommands must be false"));
        assert!(problems[2].starts_with("sandbox.network.strictAllowlist must be true"));
        assert!(problems[3].starts_with("sandbox.excludedCommands"));
        assert!(problems[4].starts_with("sandbox.filesystem.disabled"));
        let empty = json!({"excludedCommands": [], "filesystem": {"disabled": false}});
        assert!(sandbox_problems(empty.as_object().unwrap()).is_empty());
    }

    #[test]
    fn the_host_check_names_what_is_missing() {
        assert_eq!(sandbox_host_problem("macos", None, true), None);
        assert!(sandbox_host_problem("macos", None, false)
            .unwrap()
            .contains("sandbox-exec"));
        assert!(sandbox_host_problem("windows", None, true)
            .unwrap()
            .contains("no sandbox on windows"));
        let dir = tempfile::tempdir().unwrap();
        let bwrap = dir.path().join("bwrap");
        std::fs::write(&bwrap, "#!/bin/sh\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&bwrap, std::fs::Permissions::from_mode(0o755)).unwrap();
            let problem = sandbox_host_problem("linux", Some(dir.path().as_os_str()), false);
            assert_eq!(
                problem.as_deref(),
                Some("socat not on PATH; install bubblewrap and socat")
            );
            std::fs::copy(&bwrap, dir.path().join("socat")).unwrap();
            assert_eq!(
                sandbox_host_problem("linux", Some(dir.path().as_os_str()), false),
                None
            );
        }
    }
    // ---- Pins for the claude_command split (#191) ----

    fn claude_default(settings: serde_json::Map<String, Value>, force: bool) -> HarnessDefault {
        HarnessDefault {
            harness: HarnessKind::Claude,
            base: None,
            env: Default::default(),
            args: Vec::new(),
            settings: Some(settings),
            config: None,
            agent_settings: None,
            force: force.then(|| "test".to_string()),
        }
    }

    fn obj(v: Value) -> serde_json::Map<String, Value> {
        v.as_object().unwrap().clone()
    }

    fn quiet(model: &str) -> Teammate {
        Teammate {
            model: Some(model.into()),
            inherit_plugins: true,
            inherit_claudeai_skills: true,
            ..Teammate::default()
        }
    }

    fn built(env: &LaunchEnv, t: &Teammate, s: Session<'_>, prompt: &str) -> Vec<String> {
        command_args(&claude_command(env, t, s, prompt, None).unwrap())
    }

    /// Every flag group at once, in the order the variadic fence needs.
    #[test]
    fn every_flag_group_keeps_its_place() {
        let mut t = quiet("opus");
        t.effort = Some("high".into());
        t.permission_mode = Some(crate::roster::PermissionMode::AcceptEdits);
        t.mcp_servers = Some([("srv".to_string(), json!({"command": "x"}))].into());
        t.mcp_config_files = vec!["~/a.json".into()];
        t.worker_channel = true;
        t.tools = Some(vec!["Read".into()]);
        t.allowed_tools = vec!["Read".into(), "Grep".into()];
        t.disallowed_tools = vec!["Bash".into()];
        t.setting_sources = Some(vec!["user".into()]);
        t.disable_skills = true;
        t.plugin_dirs = vec!["/p".into()];
        t.args = vec!["--x".into()];
        let a = built(&LaunchEnv::for_test(), &t, Session::Fresh("S"), "PROMPT");
        let mcp = json!({"mcpServers": {
            "horch": {"command": "horch", "args": ["channel", "serve", "--agent", "claude"]},
            "srv": {"command": "x"}
        }})
        .to_string();
        assert_eq!(a[..2], ["--mcp-config".to_string(), mcp]);
        let settings = a.iter().position(|x| x == "--settings").unwrap();
        let mut rest = a.clone();
        rest.drain(settings..settings + 2);
        assert_eq!(
            rest[2..],
            [
                "--mcp-config",
                "~/a.json",
                "--strict-mcp-config",
                "--tools",
                "Read",
                "--allowedTools",
                "Read,Grep",
                "--disallowedTools",
                "Bash",
                "--dangerously-load-development-channels",
                "server:horch",
                "--model",
                "opus",
                "--effort",
                "high",
                "--permission-mode",
                "acceptEdits",
                "--setting-sources",
                "user",
                "--disable-slash-commands",
                "--plugin-dir",
                "/p",
                "--session-id",
                "S",
                "--x",
                "PROMPT"
            ]
        );
        assert!(settings > rest.iter().position(|x| x == "--plugin-dir").unwrap());
    }

    #[test]
    fn mcp_flags_per_shape() {
        let env = LaunchEnv::for_test();
        let mcp_part = |t: &Teammate| {
            let a = built(&env, t, Session::Unmanaged, "");
            a[..a.iter().position(|x| x == "--model").unwrap()].to_vec()
        };
        assert!(mcp_part(&quiet("m")).is_empty());

        // Files only: one variadic flag, not strict.
        let mut t = quiet("m");
        t.mcp_config_files = vec!["a.json".into(), "b.json".into()];
        assert_eq!(mcp_part(&t), ["--mcp-config", "a.json", "b.json"]);

        // Declared servers: strict, each file its own flag.
        let mut t = quiet("m");
        t.mcp_servers = Some(Default::default());
        t.mcp_config_files = vec!["a.json".into(), "b.json".into()];
        assert_eq!(
            mcp_part(&t),
            [
                "--mcp-config",
                r#"{"mcpServers":{}}"#,
                "--mcp-config",
                "a.json",
                "--mcp-config",
                "b.json",
                "--strict-mcp-config"
            ]
        );

        // Channel alone: not strict.
        let mut t = quiet("m");
        t.worker_channel = true;
        let a = mcp_part(&t);
        assert!(!a.contains(&"--strict-mcp-config".to_string()), "{a:?}");
        assert_eq!(a.len(), 4, "{a:?}");

        // `Some([])` tools is `--tools ""`.
        let mut t = quiet("m");
        t.tools = Some(vec![]);
        assert_eq!(mcp_part(&t), ["--tools", ""]);
    }

    #[test]
    fn a_session_resume_or_none_and_the_effort_drop() {
        let env = LaunchEnv::for_test();
        let t = quiet("haiku");
        let mut with_effort = t.clone();
        with_effort.effort = Some("high".into());
        let fresh = built(&env, &with_effort, Session::Fresh("S"), "P");
        assert!(!fresh.contains(&"--effort".to_string()), "{fresh:?}");
        assert!(fresh.windows(2).any(|w| w == ["--session-id", "S"]));
        let resumed = built(&env, &t, Session::Resume("S"), "");
        assert!(resumed.windows(2).any(|w| w == ["--resume", "S"]));
        assert_eq!(resumed.last().unwrap(), "S");
        let unmanaged = built(&env, &t, Session::Unmanaged, "P");
        assert!(!unmanaged
            .iter()
            .any(|a| a.starts_with("--session") || a == "--resume"));
    }

    #[test]
    fn inline_settings_gain_defaults_and_window_or_stay_byte_for_byte() {
        let mut env = LaunchEnv::for_test();
        let mut t = quiet("m");
        t.settings = Some(r#" { "a" : 1 } "#.into());
        let value = |a: &[String]| -> String {
            a[a.iter().position(|x| x == "--settings").unwrap() + 1].clone()
        };
        assert_eq!(
            value(&built(&env, &t, Session::Unmanaged, "")),
            r#" { "a" : 1 } "#
        );

        t.harness_defaults = vec![claude_default(obj(json!({"b": {"c": 2}, "a": 9})), false)];
        let merged: Value =
            serde_json::from_str(&value(&built(&env, &t, Session::Unmanaged, ""))).unwrap();
        assert_eq!(merged, json!({"a": 1, "b": {"c": 2}}));

        // A default that adds nothing leaves the text alone.
        t.harness_defaults = vec![claude_default(obj(json!({"a": 9})), false)];
        assert_eq!(
            value(&built(&env, &t, Session::Unmanaged, "")),
            r#" { "a" : 1 } "#
        );

        // The window merges in as env.CLAUDE_CODE_AUTO_COMPACT_WINDOW.
        env.compact_window = Some(crate::compaction::window::WindowDecision {
            tokens: Some(1234),
            source: crate::compaction::window::WindowSource::Fleet,
            detail: "d".into(),
            applied: true,
        });
        let merged: Value =
            serde_json::from_str(&value(&built(&env, &t, Session::Unmanaged, ""))).unwrap();
        assert_eq!(
            merged,
            json!({"a": 1, "env": {"CLAUDE_CODE_AUTO_COMPACT_WINDOW": "1234"}})
        );

        // Invalid inline JSON that needs a merge is refused; a file is passed on.
        t.settings = Some("{oops".into());
        assert!(claude_command(&env, &t, Session::Unmanaged, "", None).is_err());
        t.settings = Some("~/s.json".into());
        env.home = Some("/home/x".into());
        assert_eq!(
            value(&built(&env, &t, Session::Unmanaged, "")),
            "/home/x/s.json"
        );
    }

    fn home_with(settings: &str) -> (tempfile::TempDir, LaunchEnv) {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".claude")).unwrap();
        std::fs::write(tmp.path().join(".claude/settings.json"), settings).unwrap();
        let mut env = LaunchEnv::for_test();
        env.home = Some(tmp.path().to_path_buf());
        (tmp, env)
    }

    #[test]
    fn the_default_overlay_sheds_plugins_and_keeps_the_status_line() {
        let (_tmp, env) = home_with(
            r#"{"enabledPlugins":{"a@m":true,"b@m":false},"statusLine":{"type":"command","command":"x"}}"#,
        );
        let mut t = quiet("m");
        t.inherit_plugins = false;
        t.setting_sources = Some(vec!["user".into()]);
        let a = built(&env, &t, Session::Unmanaged, "");
        let overlay = settings_overlay_of(&a);
        assert_eq!(overlay["enabledPlugins"]["a@m"], false);
        assert!(overlay["enabledPlugins"].get("b@m").is_none());
        assert_eq!(overlay["statusLine"]["command"], "x");
        assert_eq!(overlay["syncClaudeAiPlugins"], false);

        // Without setting_sources the status line stays with the operator.
        t.setting_sources = None;
        let overlay = settings_overlay_of(&built(&env, &t, Session::Unmanaged, ""));
        assert!(overlay.get("statusLine").is_none());
    }

    fn settings_overlay_of(a: &[String]) -> Value {
        serde_json::from_str(&a[a.iter().position(|x| x == "--settings").unwrap() + 1]).unwrap()
    }

    #[test]
    fn skills_settings_reads_its_source_and_adds_the_switches() {
        let (tmp, _env) = home_with(
            r#"{"enabledPlugins":{"a@m":true},"statusLine":{"type":"command","command":"op"}}"#,
        );
        let home = Some(tmp.path());
        let mut t = quiet("m");
        let v = skills_settings(&t, home).unwrap();
        assert_eq!(v["disableBundledSkills"], true);
        assert_eq!(v["disableWorkflows"], true);
        assert!(v.get("statusLine").is_none());

        // A file source is read, and its own keys win.
        std::fs::write(
            tmp.path().join("mine.json"),
            r#"{"disableWorkflows":false,"statusLine":"mine"}"#,
        )
        .unwrap();
        t.settings = Some(tmp.path().join("mine.json").to_string_lossy().into_owned());
        t.inherit_plugins = false;
        t.setting_sources = Some(vec![]);
        let v = skills_settings(&t, home).unwrap();
        assert_eq!(v["disableWorkflows"], false);
        assert_eq!(v["statusLine"], "mine");
        assert_eq!(v["enabledPlugins"]["a@m"], false);

        // Without its own status line the operator's is copied.
        t.settings = Some("{}".into());
        let v = skills_settings(&t, home).unwrap();
        assert_eq!(v["statusLine"]["command"], "op");

        std::fs::write(tmp.path().join("list.json"), "[]").unwrap();
        let list = tmp.path().join("list.json").to_string_lossy().into_owned();
        for (source, why) in [
            (list.as_str(), "teammate settings must be a JSON object"),
            ("{oops", "invalid teammate settings JSON"),
            ("/no/such/file.json", "reading teammate settings"),
        ] {
            t.settings = Some(source.into());
            let err = format!("{:#}", skills_settings(&t, home).unwrap_err());
            assert!(err.contains(why), "{source}: {err}");
        }
        t.settings = Some(r#"{"enabledPlugins":3}"#.into());
        let err = format!("{:#}", skills_settings(&t, home).unwrap_err());
        assert!(err.contains("enabledPlugins must be an object"), "{err}");
    }

    #[test]
    fn operator_window_reads_the_first_source_then_the_process() {
        let texts = ClaudeSettingsTexts {
            managed: Some(r#"{"env":{"OTHER":"1"}}"#.into()),
            local: Some("not json".into()),
            project: Some(r#"{"env":{"CLAUDE_CODE_AUTO_COMPACT_WINDOW":"150000"}}"#.into()),
            user: Some(
                r#"{"env":{"CLAUDE_CODE_AUTO_COMPACT_WINDOW":"9","CLAUDE_AUTOCOMPACT_PCT_OVERRIDE":"50"}}"#
                    .into(),
            ),
            user_path: "/u".into(),
        };
        let w = claude_operator_window(&texts, None, Some("77")).unwrap();
        assert_eq!(w.tokens, Some(150000));
        assert_eq!(w.detail, ".claude/settings.json");

        // Narrowed to user: the percentage override hides the trigger.
        let only_user = vec!["user".to_string()];
        let w = claude_operator_window(&texts, Some(&only_user), Some("77")).unwrap();
        assert_eq!(w.tokens, None);
        assert_eq!(
            w.detail,
            "/u (CLAUDE_AUTOCOMPACT_PCT_OVERRIDE set; trigger unknown)"
        );

        // No file sets it: the process env counts; a non-number is flagged.
        let none = ClaudeSettingsTexts::default();
        let w = claude_operator_window(&none, None, Some("88")).unwrap();
        assert_eq!(
            (w.tokens, w.detail.as_str()),
            (Some(88), "process env CLAUDE_CODE_AUTO_COMPACT_WINDOW")
        );
        let w = claude_operator_window(&none, None, Some("lots")).unwrap();
        assert_eq!(w.tokens, None);
        assert!(w.detail.ends_with("(not a number)"), "{}", w.detail);
        assert_eq!(claude_operator_window(&none, None, None), None);

        let bad = ClaudeSettingsTexts {
            managed: Some(r#"{"env":{"CLAUDE_CODE_AUTO_COMPACT_WINDOW":true}}"#.into()),
            ..Default::default()
        };
        let w = claude_operator_window(&bad, None, None).unwrap();
        assert_eq!(w.tokens, None);
        assert_eq!(w.detail, "managed settings (not a number)");
    }

    #[test]
    fn overlay_window_respects_what_the_overlay_already_says() {
        let mut o = obj(json!({}));
        assert!(!overlay_window(None, &mut o));
        assert!(o.is_empty());
        assert!(overlay_window(Some(5), &mut o));
        assert_eq!(o["env"]["CLAUDE_CODE_AUTO_COMPACT_WINDOW"], "5");
        assert!(!overlay_window(Some(6), &mut o));
        assert_eq!(o["env"]["CLAUDE_CODE_AUTO_COMPACT_WINDOW"], "5");

        let mut o = obj(json!({"env": {"KEEP": "1"}}));
        assert!(overlay_window(Some(7), &mut o));
        assert_eq!(
            o["env"],
            json!({"KEEP": "1", "CLAUDE_CODE_AUTO_COMPACT_WINDOW": "7"})
        );

        // A non-object env is left alone.
        let mut o = obj(json!({"env": 3}));
        assert!(!overlay_window(Some(7), &mut o));
        assert_eq!(o["env"], 3);
    }

    #[test]
    fn overlay_blocks_walks_parents() {
        let o = obj(json!({"a": {"b": {"c": 1}}, "s": 1}));
        assert!(overlay_blocks(&o, &[], "s"));
        assert!(!overlay_blocks(&o, &[], "z"));
        assert!(overlay_blocks(&o, &["a", "b"], "c"));
        assert!(!overlay_blocks(&o, &["a", "b"], "d"));
        assert!(!overlay_blocks(&o, &["a", "x"], "c"));
        assert!(!overlay_blocks(&o, &["x", "y"], "c"));
        // A parent that is not an object blocks every child key.
        assert!(overlay_blocks(&o, &["s"], "anything"));
        assert!(overlay_blocks(&o, &["a", "b", "c"], "d"));
    }

    #[test]
    fn merge_defaults_later_entry_wins_and_operator_yields() {
        let (_tmp, env) = home_with(r#"{"fromOperator": 1, "nest": {"op": 1}}"#);
        let read = || operator_settings(&env);
        let mut t = quiet("m");
        t.harness_defaults = vec![
            claude_default(
                obj(json!({"x": 1, "fromOperator": 2, "nest": {"op": 2, "new": 3}})),
                false,
            ),
            claude_default(
                obj(json!({"x": 5, "forced": 1, "deep": {"er": {"k": true}}})),
                true,
            ),
        ];
        let mut o = obj(json!({"present": 1}));
        assert!(merge_settings_defaults(&t, &mut o, read).unwrap());
        assert_eq!(
            Value::Object(o.clone()),
            json!({
                "present": 1,
                "x": 5,
                "forced": 1,
                "deep": {"er": {"k": true}},
                "nest": {"new": 3}
            })
        );
        // A second pass changes nothing.
        assert!(!merge_settings_defaults(&t, &mut o, read).unwrap());

        // The overlay's own key beats a default, even a forced one; a
        // non-object parent blocks.
        let mut o = obj(json!({"x": 0, "deep": 3}));
        merge_settings_defaults(&t, &mut o, read).unwrap();
        assert_eq!(o["x"], 0);
        assert_eq!(o["deep"], 3);

        // The operator files are read only when a non-forced key needs them.
        let mut only_forced = quiet("m");
        only_forced.harness_defaults = vec![claude_default(obj(json!({"f": 1})), true)];
        let mut o = obj(json!({}));
        assert!(merge_settings_defaults(&only_forced, &mut o, || panic!("read")).unwrap());
    }

    #[test]
    fn skill_switches_refuse_non_object_sections() {
        let t = quiet("m");
        for key in ["skillOverrides", "enabledPlugins"] {
            let mut o = obj(json!({ key: 3 }));
            let err = overlay_skill_switches(&t, &mut o, None)
                .unwrap_err()
                .to_string();
            assert_eq!(err, format!("{key} must be an object"));
        }
    }
}
