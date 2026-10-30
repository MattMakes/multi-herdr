//! The operator's own settings that reach every pane: the status line, the
//! globally enabled plugins, and anything that overrides a teammate's effort.

use std::path::{Path, PathBuf};

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
///
/// `home` is the operator's home directory; `None` means it is unknown.
pub(crate) fn operator_status_line(home: Option<&Path>) -> Option<serde_json::Value> {
    operator_settings(home)?.get("statusLine").cloned()
}

/// Names of the plugins the operator has enabled globally, from
/// `enabledPlugins` in `~/.claude/settings.json`.
pub(crate) fn operator_enabled_plugins(home: Option<&Path>) -> Vec<String> {
    let Some(settings) = operator_settings(home) else {
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
///
/// `codex_home` is the directory that holds codex's `config.toml`, and
/// `claude_code_effort_level` is `$CLAUDE_CODE_EFFORT_LEVEL`, if set.
pub fn operator_effort_warnings(
    home: Option<&Path>,
    codex_home: &Path,
    claude_code_effort_level: Option<&str>,
) -> Vec<String> {
    let codex_config = std::fs::read_to_string(codex_home.join("config.toml")).ok();
    effort_override_warnings(
        claude_code_effort_level,
        operator_settings(home).as_ref(),
        codex_config.as_deref(),
    )
}

/// The pure half of [`operator_effort_warnings`].
///
/// Claude Code resolves effort in this order: env `CLAUDE_CODE_EFFORT_LEVEL`,
/// `--effort`, settings, model default (cezaar#41). horch passes `--effort`,
/// so only the env var - set in the shell, or in the `env` block of
/// ~/.claude/settings.json, which Claude Code exports into its own process -
/// beats it, and it beats it for every pane at once. `maxEffortLevel` caps
/// every level above it. A codex pane with no effort takes
/// `model_reasoning_effort` from the operator's config.toml.
pub(crate) fn effort_override_warnings(
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
    toml.lines()
        .map(str::trim)
        .take_while(|line| !line.starts_with('['))
        .find_map(|line| {
            let (key, value) = line.split_once('=')?;
            (key.trim() == "model_reasoning_effort").then(|| unquote(value))
        })
}

/// A config value without its `#` comment, its blanks and its quotes.
fn unquote(value: &str) -> String {
    let value = value.split('#').next().unwrap_or_default().trim();
    value.trim_matches(|c| c == '"' || c == '\'').to_string()
}

fn operator_settings(home: Option<&Path>) -> Option<serde_json::Value> {
    let path = home?.join(".claude/settings.json");
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

/// Expand a leading `~/` against `home`. With no `home`, the path is returned
/// as written.
///
/// Teammate files are committed, so a plugin path written as an absolute
/// `/Users/<someone>/...` only works on one machine. `~` keeps them portable
/// without inventing a template syntax for paths.
pub(crate) fn expand_home(path: &str, home: Option<&Path>) -> PathBuf {
    match path.strip_prefix("~/") {
        Some(rest) => match home {
            Some(home) => home.join(rest),
            None => PathBuf::from(path),
        },
        None => PathBuf::from(path),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_root_effort_is_read_without_quotes_or_comment() {
        for (toml, want) in [
            ("model_reasoning_effort = \"high\"\n", "high"),
            ("model_reasoning_effort = 'low'\n", "low"),
            ("model_reasoning_effort = medium\n", "medium"),
            ("model_reasoning_effort=\"xhigh\"", "xhigh"),
            ("  model_reasoning_effort  =  \"high\"   # why\n", "high"),
            ("model_reasoning_effort = \"\"\n", ""),
            (
                "model = \"gpt\"\n# a comment\n\nmodel_reasoning_effort = \"low\"\n",
                "low",
            ),
        ] {
            assert_eq!(
                codex_default_effort(toml).as_deref(),
                Some(want),
                "{toml:?}"
            );
        }
    }

    #[test]
    fn the_first_root_effort_wins() {
        let toml = "model_reasoning_effort = \"low\"\nmodel_reasoning_effort = \"high\"\n";
        assert_eq!(codex_default_effort(toml).as_deref(), Some("low"));
    }

    #[test]
    fn the_first_section_ends_the_root_table() {
        // An effort under a profile or an array of tables is not the default.
        for header in ["[profiles.x]", "[tui]", "[[hooks]]", "  [tui]"] {
            let toml = format!("model = \"gpt\"\n{header}\nmodel_reasoning_effort = \"low\"\n");
            assert_eq!(codex_default_effort(&toml), None, "{header}");
        }
        // One above the first section still counts.
        let toml = "model_reasoning_effort = \"high\"\n[tui]\nmodel_reasoning_effort = \"low\"\n";
        assert_eq!(codex_default_effort(toml).as_deref(), Some("high"));
    }

    #[test]
    fn lines_that_are_not_the_effort_key_are_skipped() {
        for toml in [
            "",
            "\n\n",
            "model = \"gpt\"\n",
            "no equals sign\n",
            "# model_reasoning_effort = \"low\"\n",
            "my_model_reasoning_effort = \"low\"\n",
            "model_reasoning_effort_extra = \"low\"\n",
        ] {
            assert_eq!(codex_default_effort(toml), None, "{toml:?}");
        }
    }
}
