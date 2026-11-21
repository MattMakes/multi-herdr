use std::path::{Path, PathBuf};

pub(crate) fn operator_status_line(home: Option<&Path>) -> Option<serde_json::Value> {
    operator_settings(home)?.get("statusLine").cloned()
}

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

fn codex_default_effort(toml: &str) -> Option<String> {
    toml.lines()
        .map(str::trim)
        .take_while(|line| !line.starts_with('['))
        .find_map(|line| {
            let (key, value) = line.split_once('=')?;
            (key.trim() == "model_reasoning_effort").then(|| unquote(value))
        })
}

fn unquote(value: &str) -> String {
    let value = value.split('#').next().unwrap_or_default().trim();
    value.trim_matches(|c| c == '"' || c == '\'').to_string()
}

fn operator_settings(home: Option<&Path>) -> Option<serde_json::Value> {
    let path = home?.join(".claude/settings.json");
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

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
        for header in ["[profiles.x]", "[tui]", "[[hooks]]", "  [tui]"] {
            let toml = format!("model = \"gpt\"\n{header}\nmodel_reasoning_effort = \"low\"\n");
            assert_eq!(codex_default_effort(&toml), None, "{header}");
        }
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
