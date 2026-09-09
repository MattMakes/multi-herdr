//! Turning a [`Teammate`] into an agent-CLI command line.
//!
//! One builder per agent, shared by every caller. `horch worker` and
//! `horch pane-launch` used to construct their own argv, which is how the
//! orchestrator ended up with an effort level and a model that appeared
//! nowhere in any file. Everything below is derived from the teammate's
//! frontmatter; nothing is hard-coded per call site.

use std::process::Command;

use anyhow::{bail, Result};

use crate::agent;
use crate::teammates::{
    expand_home, operator_enabled_plugins, operator_status_line, Agent, Teammate,
};

/// How a launch relates to an agent session.
#[derive(Debug, Clone, Copy)]
pub enum Session<'a> {
    /// Caller-minted id, passed on a fresh start (Claude).
    Fresh(&'a str),
    /// Resume an existing id.
    Resume(&'a str),
    /// The agent mints its own id after launch (Codex), or there is no ledger
    /// record at all (an orchestrator pane).
    Unmanaged,
}

/// Environment a teammate needs exported before its CLI starts.
///
/// Applied to this process, because the agent runs as a child and inherits it.
pub fn apply_env(teammate: &Teammate) {
    if let Some(sub) = &teammate.subagent_model {
        std::env::set_var("CLAUDE_CODE_SUBAGENT_MODEL", sub);
    }
    for (key, value) in &teammate.env {
        std::env::set_var(key, value);
    }
}

/// Build the command for a teammate, whichever CLI it names.
///
/// `model_override` exists for the fixed `orchestration` recipe, where the
/// model belongs to the pane rather than to the teammate file.
pub fn command(
    teammate: &Teammate,
    session: Session<'_>,
    prompt: &str,
    model_override: Option<&str>,
) -> Result<Command> {
    match teammate.agent {
        Agent::Claude => claude_command(teammate, session, prompt, model_override),
        Agent::Codex => codex_command(teammate, session, prompt, model_override),
        Agent::None => bail!("teammate '{}' has agent: none and cannot be launched", teammate.name),
    }
}

fn model_for<'a>(teammate: &'a Teammate, override_: Option<&'a str>) -> Result<&'a str> {
    match override_.or(teammate.model.as_deref()) {
        Some(m) => Ok(m),
        None => bail!("teammate '{}' has no model and none was supplied", teammate.name),
    }
}

fn claude_command(
    teammate: &Teammate,
    session: Session<'_>,
    prompt: &str,
    model_override: Option<&str>,
) -> Result<Command> {
    let bin = agent::claude_bin();
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
            cmd.arg("--mcp-config").arg(expand_home(file));
        }
        cmd.arg("--strict-mcp-config");
    } else if !teammate.mcp_config_files.is_empty() {
        cmd.arg("--mcp-config");
        for file in &teammate.mcp_config_files {
            cmd.arg(expand_home(file));
        }
    }
    // `Some([])` means "no tools", which is `--tools ""`, not "omit the flag".
    if let Some(tools) = &teammate.tools {
        cmd.arg("--tools").arg(tools.join(","));
    }
    if !teammate.allowed_tools.is_empty() {
        cmd.arg("--allowedTools").arg(teammate.allowed_tools.join(","));
    }
    if !teammate.disallowed_tools.is_empty() {
        cmd.arg("--disallowedTools")
            .arg(teammate.disallowed_tools.join(","));
    }

    cmd.arg("--model").arg(model_for(teammate, model_override)?);

    if let Some(effort) = &teammate.effort {
        cmd.arg("--effort").arg(effort);
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
        cmd.arg("--plugin-dir").arg(expand_home(dir));
    }

    // `--settings` takes a path OR a JSON string, and is not repeatable, so
    // everything horch wants to overlay has to be assembled into one object.
    if let Some(settings) = &teammate.settings {
        // The teammate named its own file; that file owns the overlay.
        cmd.arg("--settings").arg(expand_home(settings));
    } else {
        let mut overlay = serde_json::Map::new();
        // Switch the operator's globally-enabled plugins off, by name, for this
        // session. Settings merge per key, so this touches nothing else -
        // verified against a live launch: "Found 3 plugins (0 enabled,
        // 3 disabled)" with every other setting intact.
        if !teammate.inherit_plugins {
            let off: serde_json::Map<String, serde_json::Value> = operator_enabled_plugins()
                .into_iter()
                .map(|name| (name, serde_json::Value::Bool(false)))
                .collect();
            if !off.is_empty() {
                overlay.insert("enabledPlugins".into(), serde_json::Value::Object(off));
            }
        }
        // Restricting settings would take the status line with it. Every pane
        // in a fleet keeps the operator's, so a worker reads like any session.
        if teammate.setting_sources.is_some() {
            if let Some(status_line) = operator_status_line() {
                overlay.insert("statusLine".into(), status_line);
            }
        }
        if !overlay.is_empty() {
            cmd.arg("--settings").arg(serde_json::Value::Object(overlay).to_string());
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

fn codex_command(
    teammate: &Teammate,
    session: Session<'_>,
    prompt: &str,
    model_override: Option<&str>,
) -> Result<Command> {
    let bin = agent::codex_bin();
    let model = format!("model=\"{}\"", model_for(teammate, model_override)?);
    let mut cmd = Command::new(&bin);

    // `resume` is a subcommand, so it has to lead.
    if let Session::Resume(_) = session {
        cmd.arg("resume");
    }
    cmd.arg("-c").arg(&model);

    if let Some(mode) = teammate.permission_mode {
        match mode.codex_args() {
            Some(args) => {
                cmd.args(args);
            }
            None => bail!(
                "teammate '{}' sets permission_mode '{}', which has no codex equivalent",
                teammate.name,
                mode.as_str()
            ),
        }
    }
    if let Some(effort) = &teammate.effort {
        cmd.arg("-c").arg(format!("model_reasoning_effort=\"{effort}\""));
    }
    cmd.args(&teammate.args);
    if let Session::Resume(id) = session {
        cmd.arg(id);
    }
    cmd.arg(prompt);
    Ok(cmd)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::teammates::Roster;

    fn argv(cmd: &Command) -> Vec<String> {
        cmd.get_args().map(|a| a.to_string_lossy().into_owned()).collect()
    }

    #[test]
    fn claude_fresh_carries_session_and_mode() {
        let r = Roster::builtin().unwrap();
        let cmd = command(r.require("opus").unwrap(), Session::Fresh("sid"), "p", None).unwrap();
        let a = argv(&cmd);
        assert_eq!(
            a,
            vec![
                "--model", "opus", "--effort", "xhigh", "--permission-mode", "acceptEdits",
                "--session-id", "sid", "p"
            ]
        );
    }

    #[test]
    fn claude_resume_swaps_the_session_flag() {
        let r = Roster::builtin().unwrap();
        let cmd = command(r.require("sonnet").unwrap(), Session::Resume("old"), "p", None).unwrap();
        assert!(argv(&cmd).contains(&"--resume".to_string()));
        assert!(!argv(&cmd).contains(&"--session-id".to_string()));
    }

    #[test]
    fn codex_resume_leads_with_the_subcommand() {
        let r = Roster::builtin().unwrap();
        let cmd =
            command(r.require("codex-sol").unwrap(), Session::Resume("rid"), "p", None).unwrap();
        let a = argv(&cmd);
        assert_eq!(a[0], "resume");
        assert_eq!(a[1], "-c");
        assert_eq!(a[2], r#"model="gpt-5.6-sol""#);
        assert_eq!(a.last().unwrap(), "p");
        assert!(a.contains(&"rid".to_string()));
    }

    #[test]
    fn codex_permission_mode_becomes_sandbox_and_approval() {
        let r = Roster::builtin().unwrap();
        let cmd =
            command(r.require("codex-terra").unwrap(), Session::Unmanaged, "p", None).unwrap();
        let a = argv(&cmd);
        assert!(a.windows(2).any(|w| w == ["-s", "workspace-write"]), "{a:?}");
        assert!(a.windows(2).any(|w| w == ["-a", "on-request"]), "{a:?}");
    }

    /// The codex orchestrator is the one pane that launches codex with a model,
    /// an effort and a sandbox all at once. The prompt must still land last: a
    /// briefing swallowed by an earlier flag is an orchestrator that comes up
    /// with no instructions and no error.
    #[test]
    fn the_codex_orchestrator_launches_with_astra_and_keeps_its_prompt_last() {
        let r = Roster::builtin().unwrap();
        let t = r.require("orchestrator-codex").unwrap();
        let cmd = command(t, Session::Unmanaged, "BRIEFING", None).unwrap();
        let a = argv(&cmd);
        assert!(a.contains(&r#"model="gpt-6-astra""#.to_string()), "{a:?}");
        assert!(
            a.contains(&r#"model_reasoning_effort="xhigh""#.to_string()),
            "{a:?}"
        );
        assert!(a.windows(2).any(|w| w == ["-s", "workspace-write"]), "{a:?}");
        assert_eq!(a.last().unwrap(), "BRIEFING");
        assert_ne!(a[0], "resume", "a fresh orchestrator is not a resume");
    }

    fn fake_home(settings_json: &str) -> (tempfile::TempDir, HomeGuard) {
        let home = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(home.path().join(".claude")).unwrap();
        std::fs::write(home.path().join(".claude/settings.json"), settings_json).unwrap();
        let guard = HomeGuard::set(home.path());
        (home, guard)
    }

    const OPERATOR: &str = r#"{
        "statusLine": {"type":"command","command":"bash sl.sh"},
        "enabledPlugins": {"herdr@m": true, "ddd@m": true, "old@m": false},
        "disableWorkflows": true
    }"#;

    fn settings_overlay(a: &[String]) -> Option<serde_json::Value> {
        let i = a.iter().position(|x| x == "--settings")?;
        serde_json::from_str(&a[i + 1]).ok()
    }

    /// The orchestrator keeps the operator's settings - they are already tuned
    /// for token economy - and sheds only plugins and MCP servers. Not skills:
    /// `--disable-slash-commands` also removes the built-in `/context` and
    /// `/config`, and there is no way to keep just those two.
    #[test]
    fn the_orchestrator_sheds_plugins_and_mcp_but_keeps_settings_and_builtins() {
        let (_home, _g) = fake_home(OPERATOR);
        let r = Roster::builtin().unwrap();
        let cmd =
            command(r.require("orchestrator").unwrap(), Session::Unmanaged, "p", None).unwrap();
        let a = argv(&cmd);
        assert!(!a.contains(&"--setting-sources".to_string()), "settings must be inherited: {a:?}");
        assert!(
            !a.contains(&"--disable-slash-commands".to_string()),
            "would remove the built-in /context and /config: {a:?}"
        );
        assert!(a.contains(&"--strict-mcp-config".to_string()), "{a:?}");
        assert!(a.contains(&r#"{"mcpServers":{}}"#.to_string()), "{a:?}");
        assert!(!a.contains(&"--plugin-dir".to_string()), "{a:?}");

        // Only the operator's ENABLED plugins are switched off, by name.
        let overlay = settings_overlay(&a).expect("a --settings overlay");
        assert_eq!(overlay["enabledPlugins"]["herdr@m"], false);
        assert_eq!(overlay["enabledPlugins"]["ddd@m"], false);
        assert!(overlay["enabledPlugins"].get("old@m").is_none(), "{overlay}");
        // And nothing else rides along - no statusLine override, no tuning.
        assert!(overlay.get("statusLine").is_none(), "{overlay}");
        assert!(overlay.get("disableWorkflows").is_none(), "{overlay}");
    }

    /// `--strict-mcp-config` is what makes an `--mcp-config` subtractive. Without
    /// it the operator's servers load too and the isolation is a no-op.
    #[test]
    fn declared_mcp_servers_are_strict_and_complete() {
        let r = Roster::builtin().unwrap();
        let cmd = command(
            r.require("frontend-developer").unwrap(),
            Session::Unmanaged,
            "p",
            None,
        )
        .unwrap();
        let a = argv(&cmd);
        let json = a
            .iter()
            .find(|x| x.starts_with(r#"{"mcpServers""#))
            .expect("an --mcp-config payload");
        for server in ["playwright", "chrome-devtools", "context7"] {
            assert!(json.contains(server), "{server} missing from {json}");
        }
        assert!(a.contains(&"--strict-mcp-config".to_string()), "{a:?}");
        assert_eq!(a.iter().filter(|x| *x == "--plugin-dir").count(), 2, "{a:?}");
    }

    /// `--mcp-config`, `--tools`, `--allowedTools`, `--disallowedTools` are
    /// variadic. The prompt is the last positional. A variadic flag anywhere
    /// after `--model` could reach it; a live probe lost the prompt this way.
    #[test]
    fn variadic_flags_are_fenced_off_from_the_prompt_by_model() {
        let (_home, _g) = fake_home(OPERATOR);
        let r = Roster::builtin().unwrap();
        let mut t = r.require("frontend-developer").unwrap().clone();
        t.tools = Some(vec!["Read".into(), "Bash".into()]);
        t.allowed_tools = vec!["Bash(git *)".into()];
        t.disallowed_tools = vec!["Write".into()];
        t.mcp_config_files = vec!["/tmp/extra.json".into()];
        t.effort = None;
        t.permission_mode = None;
        let cmd = command(&t, Session::Unmanaged, "PROMPT", None).unwrap();
        let a = argv(&cmd);
        let model_at = a.iter().position(|x| x == "--model").unwrap();
        for flag in ["--mcp-config", "--tools", "--allowedTools", "--disallowedTools"] {
            for (i, x) in a.iter().enumerate() {
                if x == flag {
                    assert!(i < model_at, "{flag} at {i} is after --model at {model_at}: {a:?}");
                }
            }
        }
        assert_eq!(a.last().unwrap(), "PROMPT");
    }

    /// Plugin paths are written with `~` so a committed file is not bound to one
    /// home directory; they must be expanded before the CLI sees them.
    #[test]
    fn plugin_paths_are_expanded_before_launch() {
        let r = Roster::builtin().unwrap();
        let cmd =
            command(r.require("staff-engineer").unwrap(), Session::Unmanaged, "p", None).unwrap();
        assert!(
            argv(&cmd).iter().all(|a| !a.starts_with('~')),
            "a literal ~ reached the command line"
        );
    }

    /// A teammate that leaves these unset must not gain flags it never asked
    /// for - the generics deliberately inherit the operator's environment.
    #[test]
    fn unset_isolation_fields_add_no_flags() {
        let (_home, _g) = fake_home(OPERATOR);
        let r = Roster::builtin().unwrap();
        let cmd = command(r.require("sonnet").unwrap(), Session::Unmanaged, "p", None).unwrap();
        let a = argv(&cmd);
        for flag in [
            "--setting-sources",
            "--disable-slash-commands",
            "--mcp-config",
            "--strict-mcp-config",
            "--plugin-dir",
            "--settings",
        ] {
            assert!(!a.contains(&flag.to_string()), "{flag} appeared: {a:?}");
        }
    }

    /// The blunt instrument still keeps the status line: every pane in the
    /// fleet shows the operator's, or the orchestrator runs blind on context
    /// and cost, exactly where that information matters most.
    #[test]
    fn restricted_settings_keep_the_operator_status_line() {
        let (_home, _g) = fake_home(OPERATOR);
        let r = Roster::builtin().unwrap();
        let mut t = r.require("sonnet").unwrap().clone();
        t.setting_sources = Some(vec![]);
        t.disable_skills = true;
        let cmd = command(&t, Session::Unmanaged, "p", None).unwrap();
        let a = argv(&cmd);
        assert!(a.windows(2).any(|w| w == ["--setting-sources", ""]), "{a:?}");
        let overlay = settings_overlay(&a).expect("a --settings overlay");
        assert_eq!(overlay["statusLine"]["command"], "bash sl.sh");
    }

    /// Serialises HOME across tests that touch it.
    struct HomeGuard {
        prev: Option<std::ffi::OsString>,
        _lock: std::sync::MutexGuard<'static, ()>,
    }

    impl HomeGuard {
        fn set(dir: &std::path::Path) -> Self {
            static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
            let lock = LOCK.lock().unwrap_or_else(|e| e.into_inner());
            let prev = std::env::var_os("HOME");
            std::env::set_var("HOME", dir);
            HomeGuard { prev, _lock: lock }
        }
    }

    impl Drop for HomeGuard {
        fn drop(&mut self) {
            match &self.prev {
                Some(p) => std::env::set_var("HOME", p),
                None => std::env::remove_var("HOME"),
            }
        }
    }

    #[test]
    fn a_none_agent_cannot_be_launched() {
        let r = Roster::builtin().unwrap();
        let err = command(r.require("smoke").unwrap(), Session::Unmanaged, "p", None)
            .unwrap_err()
            .to_string();
        assert!(err.contains("agent: none"), "{err}");
    }
}
