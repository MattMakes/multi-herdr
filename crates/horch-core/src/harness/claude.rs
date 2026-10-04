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

    // A sandboxed teammate never starts on a host that cannot sandbox it.
    if teammate.sandbox.is_some() {
        if let Some(problem) = sandbox_host_problem(
            std::env::consts::OS,
            std::env::var_os("PATH").as_deref(),
            Path::new(SANDBOX_EXEC).exists(),
        ) {
            anyhow::bail!(
                "{}: refusing to launch without a sandbox: {problem}",
                teammate.name
            );
        }
    }

    // `--settings` takes a path OR a JSON string, and is not repeatable, so
    // everything horch wants to overlay has to be assembled into one object.
    if let Some(settings) = &teammate.settings {
        // A skills launch gives inline JSON with the sandbox merged in. A file
        // the teammate named would replace the overlay and drop the sandbox.
        if teammate.sandbox.is_some() && !settings.trim_start().starts_with('{') {
            anyhow::bail!(
                "{}: sandbox cannot be combined with a settings file",
                teammate.name
            );
        }
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
        overlay_sandbox(teammate, &mut overlay)?;
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
    overlay_sandbox(teammate, obj)?;
    Ok(settings)
}

/// Where macOS keeps the Seatbelt launcher Claude Code's sandbox runs under.
const SANDBOX_EXEC: &str = "/usr/bin/sandbox-exec";

/// The `sandbox` keys horch sets on every sandboxed launch, whatever the
/// teammate file says, as (path, value). `check` rejects a file that sets
/// one to anything else. See ai_docs/reports/finish/release-sandbox.md.
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
    // For the same reason a disabled `<plugin>:<skill>` switches its whole
    // plugin off (Claude Code 2.1.289, ai_docs/reports/finish/skill-overrides.md).
    // `or_insert`: a plugin that plugin_skills keeps enabled stays enabled.
    let enabled = operator_enabled_plugins(home);
    let installed = super::claude_plugins::installed_plugins_in(home);
    for name in &teammate.disabled_skills {
        let Some((plugin, _)) = name.split_once(':') else {
            continue;
        };
        for key in plugin_keys(plugin, &enabled, installed.as_ref()) {
            plugins.entry(key).or_insert(serde_json::Value::Bool(false));
        }
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

    /// A plugin that plugin_skills keeps enabled stays enabled.
    #[test]
    fn plugin_skills_wins_over_a_disabled_plugin_skill() {
        let mut overlay = serde_json::Map::new();
        overlay.insert("enabledPlugins".into(), json!({"herdr@m": true}));
        let plugins = overlay["enabledPlugins"].as_object_mut().unwrap();
        for key in plugin_keys("herdr", &["herdr@m".into()], None) {
            plugins.entry(key).or_insert(json!(false));
        }
        assert_eq!(overlay["enabledPlugins"], json!({"herdr@m": true}));
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

    fn host_can_sandbox() -> bool {
        sandbox_host_problem(
            std::env::consts::OS,
            std::env::var_os("PATH").as_deref(),
            Path::new(SANDBOX_EXEC).exists(),
        )
        .is_none()
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
        let built = super::super::launch::command_in(
            &LaunchEnv::for_test(),
            &t,
            Session::Fresh("id"),
            "go",
            None,
        );
        if !host_can_sandbox() {
            let err = built.unwrap_err().to_string();
            assert!(
                err.contains("refusing to launch without a sandbox"),
                "{err}"
            );
            return;
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

    /// A settings file replaces the overlay, so it would drop the sandbox.
    #[test]
    fn a_sandbox_with_a_settings_file_is_refused() {
        let mut t = sandboxed(json!({}));
        t.settings = Some("~/my-settings.json".into());
        let err = super::super::launch::command_in(
            &LaunchEnv::for_test(),
            &t,
            Session::Unmanaged,
            "go",
            None,
        )
        .unwrap_err()
        .to_string();
        let expected = if host_can_sandbox() {
            "sandbox cannot be combined with a settings file"
        } else {
            "refusing to launch without a sandbox"
        };
        assert!(err.contains(expected), "{err}");
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
}
