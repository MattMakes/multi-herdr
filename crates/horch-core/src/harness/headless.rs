//! The headless judge run (B4): `claude -p`, one prompt on stdin, one JSON
//! answer on stdout, no terminal and no pane.
//!
//! The judge reads the blind bundle and nothing else (SEC-04). Its tools are
//! the read-only set, every writing tool is denied whatever the teammate file
//! says, no plugin, skill or MCP server loads, and `FORBIDDEN_ENV` is removed
//! (JDG-09, SEC-08). The caller sets the working directory to the bundle.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use anyhow::{bail, Result};

use super::claude::overlay_skill_switches;
use super::launch::{teammate_env, LaunchEnv};
use crate::evaluation::rubric::{rubric_text, schema_text};
use crate::ids::SessionId;
use crate::roster::operator_enabled_plugins;
use crate::roster::Teammate;
use crate::runtime::RuntimeContext;

/// The only tools a headless judge may have. They read; none writes or runs.
pub const READ_ONLY_TOOLS: [&str; 3] = ["Read", "Grep", "Glob"];

/// Denied on every headless run, on top of the teammate's own list.
pub const HEADLESS_DENIED_TOOLS: [&str; 5] = ["Agent", "Edit", "Write", "NotebookEdit", "Bash"];

/// How long `claude --help` may take before the probe says "no".
const HELP_PROBE_TIMEOUT: Duration = Duration::from_secs(10);

/// The judge prompt: the teammate body (`persona`; judge.md has no base) with `{rubric}` and `{schema}`
/// replaced. `agent_prompt` cannot render it, because both placeholders are
/// unknown there.
pub fn judge_prompt(teammate: &Teammate) -> String {
    teammate
        .persona
        .replace("{rubric}", rubric_text())
        .replace("{schema}", schema_text())
}

/// The `claude -p` command of a headless run. The prompt goes on stdin; the
/// caller sets the working directory and the stdio.
///
/// `schema` is the JSON schema text for `--json-schema`. Pass it only when
/// [`supports_json_schema`] says the CLI has the flag. The flag takes the
/// schema itself, not a path (`claude --help`, 2026-10).
pub fn headless_command(
    ctx: &RuntimeContext,
    teammate: &Teammate,
    session: &SessionId,
    schema: Option<&str>,
) -> Result<Command> {
    if !teammate.agent.capabilities().headless {
        bail!(
            "teammate '{}' runs {}, which has no headless mode",
            teammate.name,
            teammate.agent.as_str()
        );
    }
    let Some(tools) = teammate.tools.as_ref().filter(|t| !t.is_empty()) else {
        bail!(
            "teammate '{}' names no tools; a headless run needs the read-only list",
            teammate.name
        );
    };
    if let Some(bad) = tools
        .iter()
        .find(|t| !READ_ONLY_TOOLS.contains(&t.as_str()))
    {
        bail!(
            "teammate '{}' asks for '{bad}'; a headless run allows only {}",
            teammate.name,
            READ_ONLY_TOOLS.join(", ")
        );
    }
    let mut denied: Vec<String> = HEADLESS_DENIED_TOOLS
        .iter()
        .map(|t| t.to_string())
        .collect();
    for t in &teammate.disallowed_tools {
        if !denied.contains(t) {
            denied.push(t.clone());
        }
    }

    let env = LaunchEnv::from_context(ctx);
    let mut cmd = Command::new(&env.bins.claude);
    cmd.arg("-p")
        .arg("--output-format")
        .arg("json")
        .arg("--session-id")
        .arg(session.as_str());
    // The variadic flags first, then `--model` fences them off (claude.rs).
    // `--strict-mcp-config` with an empty object: no MCP server at all.
    cmd.arg("--mcp-config")
        .arg(serde_json::json!({ "mcpServers": {} }).to_string())
        .arg("--strict-mcp-config");
    cmd.arg("--tools").arg(tools.join(","));
    cmd.arg("--allowedTools").arg(tools.join(","));
    cmd.arg("--disallowedTools").arg(denied.join(","));
    let model = super::launch::model_for(teammate, None)?;
    cmd.arg("--model").arg(model);
    if let Some(effort) = &teammate.effort {
        if teammate.agent.model_takes_effort(model) {
            cmd.arg("--effort").arg(effort);
        }
    }
    // No skills: the judge reads files and answers.
    cmd.arg("--disable-slash-commands");
    let mut overlay = serde_json::Map::new();
    let off: serde_json::Map<String, serde_json::Value> = operator_enabled_plugins(env.home())
        .into_iter()
        .map(|name| (name, serde_json::Value::Bool(false)))
        .collect();
    if !off.is_empty() {
        overlay.insert("enabledPlugins".into(), serde_json::Value::Object(off));
    }
    // A headless judge never inherits plugins, whatever its file says.
    let mut sealed = teammate.clone();
    sealed.inherit_plugins = false;
    overlay_skill_switches(&sealed, &mut overlay, env.home())?;
    cmd.arg("--settings")
        .arg(serde_json::Value::Object(overlay).to_string());
    if let Some(schema) = schema {
        cmd.arg("--json-schema").arg(schema);
    }
    cmd.stdin(Stdio::piped());
    crate::runtime::process::inherit_env(&mut cmd, teammate_env(teammate));
    crate::runtime::process::strip_forbidden(&mut cmd);
    Ok(cmd)
}

/// Whether `claude --help` at `bin` lists `--json-schema`. Probed once per
/// binary per process; a probe that fails or hangs says no.
pub fn supports_json_schema(bin: &Path) -> bool {
    static CACHE: OnceLock<Mutex<BTreeMap<PathBuf, bool>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(BTreeMap::new()));
    if let Some(known) = cache.lock().expect("probe cache").get(bin) {
        return *known;
    }
    let found = help_lists(bin, "--json-schema");
    cache
        .lock()
        .expect("probe cache")
        .insert(bin.to_path_buf(), found);
    found
}

fn help_lists(bin: &Path, flag: &str) -> bool {
    let mut cmd = Command::new(bin);
    cmd.arg("--help")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    crate::runtime::process::strip_forbidden(&mut cmd);
    let Ok(mut child) = cmd.spawn() else {
        return false;
    };
    let mut stdout = child.stdout.take().expect("piped stdout");
    let reader = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = std::io::Read::read_to_string(&mut stdout, &mut text);
        text
    });
    let deadline = Instant::now() + HELP_PROBE_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(20));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
        }
    }
    reader
        .join()
        .map(|text| {
            text.split_whitespace()
                .any(|w| w.trim_end_matches(',') == flag)
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::MapEnv;

    fn judge() -> Teammate {
        crate::roster::Roster::builtin()
            .unwrap()
            .require("judge")
            .unwrap()
            .clone()
    }

    fn ctx() -> RuntimeContext {
        let env = MapEnv::new("/proj")
            .with("HOME", "/nonexistent-home-b4")
            .with("PATH", "/nowhere-b4")
            .with("HORCH_CLAUDE_BIN", "/fake/claude")
            .with("ANTHROPIC_API_KEY", "SENTINEL");
        RuntimeContext::from_env(&env).unwrap()
    }

    fn argv(cmd: &Command) -> Vec<String> {
        cmd.get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    }

    fn value_of(argv: &[String], flag: &str) -> Option<String> {
        argv.windows(2).find(|w| w[0] == flag).map(|w| w[1].clone())
    }

    #[test]
    fn headless_argv_is_print_mode_read_only_and_keyless() {
        let session = SessionId::new("0192a6c4-0000-7000-8000-000000000001").unwrap();
        let cmd = headless_command(&ctx(), &judge(), &session, None).unwrap();
        assert_eq!(cmd.get_program(), "/fake/claude");
        let a = argv(&cmd);
        assert_eq!(a[0], "-p");
        assert_eq!(value_of(&a, "--output-format").as_deref(), Some("json"));
        assert_eq!(
            value_of(&a, "--session-id").as_deref(),
            Some(session.as_str())
        );
        assert_eq!(value_of(&a, "--model").as_deref(), Some("opus"));
        assert_eq!(value_of(&a, "--effort").as_deref(), Some("high"));
        assert_eq!(value_of(&a, "--tools").as_deref(), Some("Read,Grep,Glob"));
        assert_eq!(
            value_of(&a, "--allowedTools").as_deref(),
            Some("Read,Grep,Glob")
        );
        assert_eq!(
            value_of(&a, "--disallowedTools").as_deref(),
            Some("Agent,Edit,Write,NotebookEdit,Bash")
        );
        assert!(a.contains(&"--strict-mcp-config".to_string()));
        assert_eq!(
            value_of(&a, "--mcp-config").as_deref(),
            Some(r#"{"mcpServers":{}}"#)
        );
        assert!(a.contains(&"--disable-slash-commands".to_string()));
        assert!(!a.contains(&"--json-schema".to_string()));
        let settings: serde_json::Value =
            serde_json::from_str(&value_of(&a, "--settings").unwrap()).unwrap();
        assert_eq!(settings["syncClaudeAiPlugins"], false);
        // The prompt is not on the command line.
        assert!(!a.iter().any(|x| x.contains("blind evaluator")));

        // The key is removed from the child, never set.
        let envs: Vec<(String, Option<String>)> = cmd
            .get_envs()
            .map(|(k, v)| {
                (
                    k.to_string_lossy().into_owned(),
                    v.map(|v| v.to_string_lossy().into_owned()),
                )
            })
            .collect();
        assert!(
            envs.contains(&("ANTHROPIC_API_KEY".to_string(), None)),
            "{envs:?}"
        );
        assert!(!envs.iter().any(|(_, v)| v.as_deref() == Some("SENTINEL")));
    }

    #[test]
    fn headless_passes_the_schema_text_when_supported() {
        let session = SessionId::new("s-1").unwrap();
        let cmd = headless_command(&ctx(), &judge(), &session, Some(schema_text())).unwrap();
        assert_eq!(
            value_of(&argv(&cmd), "--json-schema").as_deref(),
            Some(schema_text())
        );
    }

    #[test]
    fn headless_refuses_a_writing_tool() {
        let mut t = judge();
        t.tools = Some(vec!["Read".into(), "Edit".into()]);
        let err = headless_command(&ctx(), &t, &SessionId::new("s").unwrap(), None).unwrap_err();
        assert!(err.to_string().contains("'Edit'"), "{err}");
        t.tools = None;
        assert!(headless_command(&ctx(), &t, &SessionId::new("s").unwrap(), None).is_err());
    }

    #[test]
    fn judge_prompt_fills_rubric_and_schema() {
        let p = judge_prompt(&judge());
        assert!(!p.contains("{rubric}") && !p.contains("{schema}"));
        assert!(p.contains(rubric_text().trim()));
        assert!(p.contains(schema_text().trim()));
    }
}
