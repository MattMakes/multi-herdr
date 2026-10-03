//! The operator's own settings that reach every pane: the status line, the
//! globally enabled plugins, and anything that overrides a teammate's effort.

use std::path::PathBuf;

/// The operator's `statusLine` block, read from `~/.claude/settings.json`.
///
/// Every agent in a fleet - orchestrator and workers alike - shows the same
/// status line as an ordinary session. That is not decoration: a pane with no
/// status line gives no model, no context usage and no session cost, and a
/// fleet is exactly where that information matters most.
///
/// It has to be re-injected explicitly because `--setting-sources ""`, which is
/// how a teammate sheds the operator's globally-enabled plugins, sheds their
/// `statusLine` with everything else.
pub fn operator_status_line() -> Option<serde_json::Value> {
    operator_settings()?.get("statusLine").cloned()
}

/// Names of the plugins the operator has enabled globally, from
/// `enabledPlugins` in `~/.claude/settings.json`.
pub fn operator_enabled_plugins() -> Vec<String> {
    let Some(settings) = operator_settings() else {
        return Vec::new();
    };
    match settings.get("enabledPlugins").and_then(|v| v.as_object()) {
        Some(map) => map
            .iter()
            .filter(|(_, on)| on.as_bool() == Some(true))
            .map(|(name, _)| name.clone())
            .collect(),
        None => Vec::new(),
    }
}

/// Operator settings that silently override a teammate's `effort`, as
/// warning lines for `horch doctor`. Empty when nothing overrides.
pub fn operator_effort_warnings() -> Vec<String> {
    let codex_config = std::fs::read_to_string(
        crate::codex::codex_home(&crate::agent::home_dir()).join("config.toml"),
    )
    .ok();
    effort_override_warnings(
        std::env::var("CLAUDE_CODE_EFFORT_LEVEL").ok().as_deref(),
        operator_settings().as_ref(),
        codex_config.as_deref(),
    )
}

/// The pure half of [`operator_effort_warnings`].
///
/// Claude Code resolves effort as: env `CLAUDE_CODE_EFFORT_LEVEL` > `--effort`
/// > settings > model default (cezaar#41). horch passes `--effort`, so only
/// the env var - set in the shell, or in the `env` block of
/// ~/.claude/settings.json, which Claude Code exports into its own process -
/// beats it, and it beats it for every pane at once. `maxEffortLevel` caps
/// every level above it. A codex pane with no effort takes
/// `model_reasoning_effort` from the operator's config.toml.
pub fn effort_override_warnings(
    env_level: Option<&str>,
    settings: Option<&serde_json::Value>,
    codex_config: Option<&str>,
) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(level) = env_level.filter(|l| !l.is_empty()) {
        out.push(format!(
            "CLAUDE_CODE_EFFORT_LEVEL={level} is set in this shell: it beats --effort, so \
             every claude pane runs at {level} whatever its teammate file says"
        ));
    }
    if let Some(settings) = settings {
        if let Some(level) = settings
            .pointer("/env/CLAUDE_CODE_EFFORT_LEVEL")
            .and_then(|v| v.as_str())
        {
            out.push(format!(
                "~/.claude/settings.json sets env.CLAUDE_CODE_EFFORT_LEVEL={level}: it beats \
                 --effort, so every claude pane runs at {level}"
            ));
        }
        if let Some(cap) = settings.get("maxEffortLevel").and_then(|v| v.as_str()) {
            out.push(format!(
                "~/.claude/settings.json caps effort at maxEffortLevel={cap}; teammates set \
                 above it run at {cap}"
            ));
        }
    }
    if let Some(level) = codex_config.and_then(codex_default_effort) {
        out.push(format!(
            "~/.codex/config.toml sets model_reasoning_effort=\"{level}\": any codex pane \
             with no effort (the orchestration recipe's) runs at {level}"
        ));
    }
    out
}

/// The top-level `model_reasoning_effort` in a codex config.toml, if any.
/// A line scan rather than a TOML parser: only the root table counts, so the
/// scan stops at the first `[section]`.
fn codex_default_effort(toml: &str) -> Option<String> {
    for line in toml.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            return None;
        }
        if let Some((key, value)) = line.split_once('=') {
            if key.trim() == "model_reasoning_effort" {
                let value = value.split('#').next().unwrap_or_default().trim();
                return Some(value.trim_matches(|c| c == '"' || c == '\'').to_string());
            }
        }
    }
    None
}

fn operator_settings() -> Option<serde_json::Value> {
    let home = std::env::var_os("HOME")?;
    let path = PathBuf::from(home).join(".claude/settings.json");
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

/// Expand a leading `~/` against `$HOME`.
///
/// Teammate files are committed, so a plugin path written as an absolute
/// `/Users/<someone>/...` only works on one machine. `~` keeps them portable
/// without inventing a template syntax for paths.
pub fn expand_home(path: &str) -> PathBuf {
    match path.strip_prefix("~/") {
        Some(rest) => match std::env::var_os("HOME") {
            Some(home) => PathBuf::from(home).join(rest),
            None => PathBuf::from(path),
        },
        None => PathBuf::from(path),
    }
}
