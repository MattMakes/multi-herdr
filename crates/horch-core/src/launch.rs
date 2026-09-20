//! Turning a [`Teammate`] into an agent-CLI command line.
//!
//! One builder per agent, shared by every caller. `horch worker` and
//! `horch pane-launch` used to construct their own argv, which is how the
//! orchestrator ended up with an effort level and a model that appeared
//! nowhere in any file. Everything below is derived from the teammate's
//! frontmatter; nothing is hard-coded per call site.

use std::process::Command;

use anyhow::{bail, Context, Result};

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
        Agent::Opencode => opencode_command(teammate, session, prompt, model_override),
        // pi and Prime Agent share a CLI surface - Prime is a fork of pi - but
        // they have drifted where it matters most: pi can be told its session
        // id, Prime cannot, and Prime runs a daemon. One builder, two dialects.
        Agent::Pi => pi_family_command(agent::pi_bin(), teammate, session, prompt, model_override),
        Agent::Prime => pi_family_command(
            agent::prime_bin(),
            teammate,
            session,
            prompt,
            model_override,
        ),
        Agent::None => bail!(
            "teammate '{}' has agent: none and cannot be launched",
            teammate.name
        ),
    }
}

/// Native skill discovery plus a short routing instruction. The caller holds
/// the bundle until the child exits so lazy reads remain valid throughout.
pub fn command_with_skills(
    teammate: &Teammate,
    session: Session<'_>,
    prompt: &str,
    model_override: Option<&str>,
    bundle: Option<&crate::skills::Bundle>,
) -> Result<Command> {
    let Some(bundle) = bundle else {
        return command(teammate, session, prompt, model_override);
    };
    let adjusted = bundle.configure(teammate)?;
    let prompt = format!(
        "{}\n{prompt}",
        bundle.briefing(teammate.agent, teammate.phase)
    );
    let mut cmd = command(&adjusted, session, &prompt, model_override)?;
    bundle.apply_env(&mut cmd, teammate)?;
    Ok(cmd)
}

/// OpenCode: `opencode --model provider/model --prompt "..."`.
///
/// The prompt is a FLAG here, not a trailing positional, so it cannot be eaten
/// by a variadic - but `args` still goes before it, to keep every builder in
/// this file ordered the same way.
fn opencode_command(
    teammate: &Teammate,
    session: Session<'_>,
    prompt: &str,
    model_override: Option<&str>,
) -> Result<Command> {
    let mut cmd = Command::new(agent::opencode_bin());
    cmd.arg("--model").arg(model_for(teammate, model_override)?);

    // OpenCode calls reasoning effort a model "variant", and passes it straight
    // through to the provider, so the teammate's `effort` needs no translation.
    if let Some(effort) = &teammate.effort {
        cmd.arg("--variant").arg(effort);
    }
    if let Some(mode) = teammate.permission_mode {
        match mode.opencode_args() {
            Some(args) => {
                cmd.args(args);
            }
            None => bail!(
                "teammate '{}' sets permission_mode '{}', which has no opencode equivalent",
                teammate.name,
                mode.as_str()
            ),
        }
    }
    // `--pure` drops external plugins while leaving the operator's providers and
    // credentials alone - the same intent as `inherit_plugins: false` on claude.
    if !teammate.inherit_plugins {
        cmd.arg("--pure");
    }
    // Resume only. OpenCode mints its own `ses_...` ids, so a fresh session is
    // started by saying nothing and harvested afterwards.
    if let Session::Resume(id) = session {
        cmd.arg("--session").arg(id);
    }
    cmd.args(&teammate.args);
    cmd.arg("--prompt").arg(prompt);
    Ok(cmd)
}

/// pi and Prime Agent: `<bin> --model <pattern> --thinking <level> -- "<prompt>"`.
///
/// Neither has an approval gate to bypass: their tools run, which is what makes
/// them usable in a pane nobody is watching. `permission_mode` therefore has
/// nothing to map onto, and the roster check rejects it rather than pretending.
fn pi_family_command(
    bin: std::path::PathBuf,
    teammate: &Teammate,
    session: Session<'_>,
    prompt: &str,
    model_override: Option<&str>,
) -> Result<Command> {
    let mut cmd = Command::new(bin);
    cmd.arg("--model").arg(model_for(teammate, model_override)?);

    if let Some(effort) = &teammate.effort {
        cmd.arg("--thinking").arg(effort);
    }
    if let Some(tools) = &teammate.tools {
        // `Some([])` means "no tools at all", which is its own flag here rather
        // than an empty list.
        if tools.is_empty() {
            cmd.arg("--no-tools");
        } else {
            cmd.arg("--tools").arg(tools.join(","));
        }
    }
    // `--exclude-tools` is pi's; Prime 0.9.4 has no denylist, and the roster
    // check rejects `disallowed_tools` on a Prime teammate rather than dropping
    // it here, so this only ever fires for pi.
    if !teammate.disallowed_tools.is_empty() && teammate.agent.takes_tool_denylist() {
        cmd.arg("--exclude-tools")
            .arg(teammate.disallowed_tools.join(","));
    }
    // Discovery of everything the repo or the operator might have lying around.
    // Nothing here is on by default in a fleet: a worker with one narrow job
    // should not inherit a project's extensions or the operator's skills.
    if !teammate.inherit_plugins {
        cmd.arg("--no-extensions")
            .arg("--no-skills")
            .arg("--no-prompt-templates")
            .arg("--no-themes");
    }

    match session {
        // pi's `--session-id` creates the session if it does not exist, so a
        // fresh launch and a resume are the same flag with a different id.
        // Prime has no such flag: `horch worker` gives it a `--session-dir` it
        // owns instead, and reads back whatever session appears there.
        Session::Fresh(id) if teammate.agent.mints_session_id() => {
            cmd.arg("--session-id").arg(id);
        }
        Session::Resume(id) => {
            if teammate.agent.mints_session_id() {
                cmd.arg("--session-id").arg(id);
            } else {
                cmd.arg("--resume").arg(id);
            }
        }
        Session::Fresh(_) | Session::Unmanaged => {}
    }
    cmd.args(&teammate.args);
    // `--` ends option parsing: without it a prompt beginning with a dash would
    // be read as a flag.
    cmd.arg("--").arg(prompt);
    Ok(cmd)
}

fn model_for<'a>(teammate: &'a Teammate, override_: Option<&'a str>) -> Result<&'a str> {
    match override_.or(teammate.model.as_deref()) {
        Some(m) => Ok(m),
        None => bail!(
            "teammate '{}' has no model and none was supplied",
            teammate.name
        ),
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
        cmd.arg("--allowedTools")
            .arg(teammate.allowed_tools.join(","));
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
        // Whatever inherit_plugins says: the generic tiers inherit plugins, but
        // no pane has a use for the operator's claude.ai skills.
        overlay_skill_switches(teammate, &mut overlay)?;
        // Restricting settings would take the status line with it. Every pane
        // in a fleet keeps the operator's, so a worker reads like any session.
        if teammate.setting_sources.is_some() {
            if let Some(status_line) = operator_status_line() {
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

/// Add the skill switches every Claude launch overlays on the operator's
/// settings. Shared by the plain overlay above and the skill-bundle one in
/// `skills.rs`, which is the path fleet panes take.
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
) -> Result<()> {
    if !teammate.inherit_claudeai_skills {
        overlay
            .entry("syncClaudeAiSkills")
            .or_insert(serde_json::Value::Bool(false));
    }
    if !teammate.disabled_skills.is_empty() {
        let overrides = overlay
            .entry("skillOverrides")
            .or_insert_with(|| serde_json::json!({}))
            .as_object_mut()
            .context("skillOverrides must be an object")?;
        for name in &teammate.disabled_skills {
            overrides.insert(name.clone(), "off".into());
        }
    }
    Ok(())
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
    // Fleet panes must reach their briefing without startup dialogs. Keep
    // sandbox/command approval policy separate from trust for enabled hooks.
    cmd.args(["-c", "check_for_update_on_startup=false"]);
    cmd.args(["-c", "tui.resume_cwd=\"current\""]);
    if !teammate
        .args
        .iter()
        .any(|arg| arg == "--dangerously-bypass-hook-trust")
    {
        cmd.arg("--dangerously-bypass-hook-trust");
    }

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
        cmd.arg("-c")
            .arg(format!("model_reasoning_effort=\"{effort}\""));
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
        cmd.get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn phase_skills_are_native_on_all_harnesses_and_resume_keeps_prompt_last() {
        use crate::teammates::Phase;
        let r = Roster::builtin().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        for name in ["sonnet", "codex-sol", "opencode-pickle", "pi", "prime"] {
            let mut t = r.require(name).unwrap().clone();
            t.phase = Some(Phase::Research);
            if cfg!(windows) && t.agent == Agent::Codex {
                assert!(crate::skills::Bundle::install(tmp.path(), &t).is_err());
                continue;
            }
            let bundle = crate::skills::Bundle::install(tmp.path(), &t)
                .unwrap()
                .unwrap();
            for session in [Session::Unmanaged, Session::Resume("sid")] {
                let cmd =
                    command_with_skills(&t, session, "BRIEFING", None, Some(&bundle)).unwrap();
                let args = argv(&cmd);
                assert!(
                    args.last().unwrap().ends_with("BRIEFING"),
                    "{name}: {args:?}"
                );
                assert!(args.last().unwrap().contains("Fleet skill phase: research"));
                match t.agent {
                    Agent::Claude => assert!(args.contains(&"--plugin-dir".into())),
                    Agent::Pi | Agent::Prime => {
                        let flag = args.iter().position(|s| s == "--skill").unwrap();
                        assert!(std::path::Path::new(&args[flag + 1])
                            .join("brainstorm/SKILL.md")
                            .exists());
                        assert!(flag < args.iter().position(|s| s == "--").unwrap());
                    }
                    Agent::Opencode => {
                        let (_, config) = cmd
                            .get_envs()
                            .find(|(k, _)| *k == "OPENCODE_CONFIG_CONTENT")
                            .unwrap();
                        let value: serde_json::Value =
                            serde_json::from_str(config.unwrap().to_str().unwrap()).unwrap();
                        assert!(value["skills"]["paths"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .any(|p| p == &serde_json::json!(bundle.skills_dir())));
                    }
                    Agent::Codex => assert!(!args.contains(&"--skill".into())),
                    Agent::None => unreachable!(),
                }
            }
        }
    }

    #[test]
    fn claude_fresh_carries_session_and_mode() {
        let r = Roster::builtin().unwrap();
        let cmd = command(r.require("opus").unwrap(), Session::Fresh("sid"), "p", None).unwrap();
        let a = argv(&cmd);
        assert_eq!(
            a,
            vec![
                "--model",
                "opus",
                "--effort",
                "xhigh",
                "--permission-mode",
                "auto",
                "--settings",
                r#"{"skillOverrides":{"herdr-orchestrator":"off","herdr-worker":"off","herdr:herdr-orchestrator":"off","herdr:herdr-worker":"off"},"syncClaudeAiSkills":false}"#,
                "--session-id",
                "sid",
                "p"
            ]
        );
    }

    #[test]
    fn claude_resume_swaps_the_session_flag() {
        let r = Roster::builtin().unwrap();
        let cmd = command(
            r.require("sonnet").unwrap(),
            Session::Resume("old"),
            "p",
            None,
        )
        .unwrap();
        assert!(argv(&cmd).contains(&"--resume".to_string()));
        assert!(!argv(&cmd).contains(&"--session-id".to_string()));
    }

    #[test]
    fn codex_resume_leads_with_the_subcommand() {
        let r = Roster::builtin().unwrap();
        let cmd = command(
            r.require("codex-sol").unwrap(),
            Session::Resume("rid"),
            "p",
            None,
        )
        .unwrap();
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
        let cmd = command(
            r.require("codex-terra").unwrap(),
            Session::Unmanaged,
            "p",
            None,
        )
        .unwrap();
        let a = argv(&cmd);
        assert!(
            a.windows(2).any(|w| w == ["-s", "workspace-write"]),
            "{a:?}"
        );
        assert!(a.windows(2).any(|w| w == ["-a", "never"]), "{a:?}");
    }

    #[test]
    fn fixed_recipe_codex_worker_keeps_noninteractive_permissions() {
        let r = Roster::builtin().unwrap();
        let mut t = r.require("orchestration-worker").unwrap().clone();
        t.agent = Agent::Codex;
        t.effort = None;
        let a = argv(&command(&t, Session::Unmanaged, "p", Some("gpt-5.6-sol")).unwrap());
        assert!(a.windows(2).any(|w| w == ["-s", "workspace-write"]));
        assert!(a.windows(2).any(|w| w == ["-a", "never"]));
    }

    #[test]
    fn prime_daemon_flags_and_skills_precede_the_prompt_delimiter() {
        let r = Roster::builtin().unwrap();
        let mut t = r.require("prime").unwrap().clone();
        t.args.extend([
            "--daemon-socket".into(),
            "/tmp/socket".into(),
            "--session-dir".into(),
            "/tmp/sessions".into(),
        ]);
        let tmp = tempfile::tempdir().unwrap();
        let bundle = crate::skills::Bundle::install(tmp.path(), &t)
            .unwrap()
            .unwrap();
        let a =
            argv(&command_with_skills(&t, Session::Unmanaged, "p", None, Some(&bundle)).unwrap());
        let delimiter = a.iter().position(|s| s == "--").unwrap();
        for flag in ["--daemon-socket", "--session-dir", "--skill"] {
            assert!(a.iter().position(|s| s == flag).unwrap() < delimiter);
        }
    }

    #[test]
    fn codex_panes_start_and_resume_without_interactive_setup() {
        let r = Roster::builtin().unwrap();
        for name in ["codex-sol", "codex-terra", "orchestrator-codex"] {
            for session in [Session::Unmanaged, Session::Resume("rid")] {
                let a =
                    argv(&command(r.require(name).unwrap(), session, "BRIEFING", None).unwrap());
                assert!(
                    a.windows(2)
                        .any(|w| w == ["-c", "check_for_update_on_startup=false"]),
                    "{name}: {a:?}"
                );
                assert!(
                    a.windows(2)
                        .any(|w| w == ["-c", "tui.resume_cwd=\"current\""]),
                    "{name}: {a:?}"
                );
                assert_eq!(
                    a.iter()
                        .filter(|arg| *arg == "--dangerously-bypass-hook-trust")
                        .count(),
                    1,
                    "{name}: {a:?}"
                );
                assert!(
                    a.windows(2).any(|w| w == ["-s", "workspace-write"]),
                    "{name}: {a:?}"
                );
                assert!(a.windows(2).any(|w| w == ["-a", "never"]), "{name}: {a:?}");
                assert!(!a.iter().any(|arg| {
                    arg == "--dangerously-bypass-approvals-and-sandbox"
                        || arg == "danger-full-access"
                }));
                assert_eq!(a.last().unwrap(), "BRIEFING");
            }
        }
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
        assert!(
            a.windows(2).any(|w| w == ["-s", "workspace-write"]),
            "{a:?}"
        );
        assert_eq!(a.last().unwrap(), "BRIEFING");
        assert_ne!(a[0], "resume", "a fresh orchestrator is not a resume");
    }

    /// OpenCode takes its prompt as a FLAG, and its reasoning effort is a
    /// provider "variant". A worker also needs `--auto`, or it stops on the
    /// first permission prompt in a pane nobody is watching.
    #[test]
    fn opencode_passes_the_prompt_as_a_flag_and_auto_approves() {
        let r = Roster::builtin().unwrap();
        let cmd = command(
            r.require("opencode-pickle").unwrap(),
            Session::Unmanaged,
            "BRIEFING",
            None,
        )
        .unwrap();
        let a = argv(&cmd);
        assert!(
            a.windows(2)
                .any(|w| w == ["--model", "opencode/big-pickle"]),
            "{a:?}"
        );
        assert!(a.windows(2).any(|w| w == ["--variant", "high"]), "{a:?}");
        assert!(a.contains(&"--auto".to_string()), "{a:?}");
        assert!(
            a.contains(&"--pure".to_string()),
            "inherit_plugins: false: {a:?}"
        );
        assert_eq!(a[a.len() - 2], "--prompt");
        assert_eq!(a.last().unwrap(), "BRIEFING");
    }

    /// OpenCode mints its own `ses_...` ids, so a fresh launch says nothing
    /// about sessions and a resume passes the harvested one back.
    #[test]
    fn opencode_only_names_a_session_when_resuming() {
        let r = Roster::builtin().unwrap();
        let t = r.require("opencode-ultra").unwrap();
        let fresh = argv(&command(t, Session::Fresh("ignored"), "p", None).unwrap());
        assert!(!fresh.contains(&"--session".to_string()), "{fresh:?}");
        let resumed = argv(&command(t, Session::Resume("ses_abc"), "p", None).unwrap());
        assert!(
            resumed.windows(2).any(|w| w == ["--session", "ses_abc"]),
            "{resumed:?}"
        );
    }

    /// pi CAN be told its session id, so the ledger knows the resume handle
    /// before the pane starts - the same deal claude gives. `--` fences the
    /// prompt off from option parsing.
    #[test]
    fn pi_takes_a_caller_minted_session_and_fences_its_prompt() {
        let r = Roster::builtin().unwrap();
        let cmd = command(
            r.require("pi").unwrap(),
            Session::Fresh("sid-1"),
            "-x BRIEF",
            None,
        )
        .unwrap();
        let a = argv(&cmd);
        assert!(
            a.windows(2).any(|w| w == ["--model", "ollama/qwen3.8"]),
            "{a:?}"
        );
        assert!(a.windows(2).any(|w| w == ["--thinking", "high"]), "{a:?}");
        assert!(
            a.windows(2).any(|w| w == ["--session-id", "sid-1"]),
            "{a:?}"
        );
        assert!(a.contains(&"--no-extensions".to_string()), "{a:?}");
        assert_eq!(
            a[a.len() - 2],
            "--",
            "a prompt starting with - must not parse as a flag"
        );
        assert_eq!(a.last().unwrap(), "-x BRIEF");
    }

    /// Prime has no `--session-id` (verified against 0.9.4), so a fresh launch
    /// must not invent one - `horch worker` owns its session directory instead.
    #[test]
    fn prime_never_claims_to_set_a_session_id() {
        let r = Roster::builtin().unwrap();
        let t = r.require("prime").unwrap();
        let fresh = argv(&command(t, Session::Fresh("sid-1"), "p", None).unwrap());
        assert!(!fresh.contains(&"--session-id".to_string()), "{fresh:?}");
        assert!(!fresh.contains(&"sid-1".to_string()), "{fresh:?}");
        let resumed = argv(&command(t, Session::Resume("/state/s.jsonl"), "p", None).unwrap());
        assert!(
            resumed
                .windows(2)
                .any(|w| w == ["--resume", "/state/s.jsonl"]),
            "{resumed:?}"
        );
    }

    /// A permission_mode with no counterpart must stop the launch rather than
    /// quietly running more permissively than the file asked for.
    #[test]
    fn a_mode_the_agent_cannot_express_is_refused() {
        let r = Roster::builtin().unwrap();
        let mut t = r.require("opencode-pickle").unwrap().clone();
        t.permission_mode = Some(crate::teammates::PermissionMode::Plan);
        let err = command(&t, Session::Unmanaged, "p", None)
            .unwrap_err()
            .to_string();
        assert!(err.contains("no opencode equivalent"), "{err}");
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
        "skillOverrides": {"explain-diff-notion": "off"},
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
        let cmd = command(
            r.require("orchestrator").unwrap(),
            Session::Unmanaged,
            "p",
            None,
        )
        .unwrap();
        let a = argv(&cmd);
        assert!(
            !a.contains(&"--setting-sources".to_string()),
            "settings must be inherited: {a:?}"
        );
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
        assert!(
            overlay["enabledPlugins"].get("old@m").is_none(),
            "{overlay}"
        );
        // The claude.ai-synced skills go off too.
        assert_eq!(overlay["syncClaudeAiSkills"], false, "{overlay}");
        // And nothing else rides along - no statusLine override and no tuning.
        assert!(overlay.get("statusLine").is_none(), "{overlay}");
        assert!(overlay.get("disableWorkflows").is_none(), "{overlay}");
        // skillOverrides carries this teammate's own two entries and nothing
        // else. The operator's `explain-diff-notion` override is not copied in:
        // Claude merges settings per key, so it still applies by itself.
        assert_eq!(
            overlay["skillOverrides"],
            serde_json::json!({"herdr-orchestrator": "off", "herdr-worker": "off"}),
            "{overlay}"
        );
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
        assert!(
            !a.iter().any(|x| x == "--plugin-dir"),
            "legacy local plugins: {a:?}"
        );
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
        for flag in [
            "--mcp-config",
            "--tools",
            "--allowedTools",
            "--disallowedTools",
        ] {
            for (i, x) in a.iter().enumerate() {
                if x == flag {
                    assert!(
                        i < model_at,
                        "{flag} at {i} is after --model at {model_at}: {a:?}"
                    );
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
        let cmd = command(
            r.require("staff-engineer").unwrap(),
            Session::Unmanaged,
            "p",
            None,
        )
        .unwrap();
        assert!(
            argv(&cmd).iter().all(|a| !a.starts_with('~')),
            "a literal ~ reached the command line"
        );
    }

    /// A teammate that leaves these unset must not gain flags it never asked
    /// for - the generics deliberately inherit the operator's environment. Two
    /// things still ride in the overlay: the claude.ai-synced skills, off in
    /// every pane whether plugins are inherited or not, and this teammate's own
    /// `disabled_skills`, which name the stale external herdr briefings.
    #[test]
    fn unset_isolation_fields_add_no_flags() {
        let (_home, _g) = fake_home(OPERATOR);
        let r = Roster::builtin().unwrap();
        let sonnet = r.require("sonnet").unwrap();
        assert!(sonnet.inherit_plugins);
        let cmd = command(sonnet, Session::Unmanaged, "p", None).unwrap();
        let a = argv(&cmd);
        for flag in [
            "--setting-sources",
            "--disable-slash-commands",
            "--mcp-config",
            "--strict-mcp-config",
            "--plugin-dir",
        ] {
            assert!(!a.contains(&flag.to_string()), "{flag} appeared: {a:?}");
        }
        let overlay = settings_overlay(&a).expect("a --settings overlay");
        assert_eq!(
            overlay,
            serde_json::json!({
                "syncClaudeAiSkills": false,
                "skillOverrides": {
                    "herdr-orchestrator": "off",
                    "herdr-worker": "off",
                    "herdr:herdr-orchestrator": "off",
                    "herdr:herdr-worker": "off"
                }
            })
        );
    }

    /// `inherit_claudeai_skills: true` leaves the switch out. `sonnet` still
    /// gets a `--settings` overlay, because its `disabled_skills` go in the same
    /// place; what must be absent is the `syncClaudeAiSkills` key itself.
    #[test]
    fn opting_in_to_claudeai_skills_leaves_the_switch_out() {
        let (_home, _g) = fake_home(OPERATOR);
        let r = Roster::builtin().unwrap();
        let mut sonnet = r.require("sonnet").unwrap().clone();
        sonnet.inherit_claudeai_skills = true;
        let a = argv(&command(&sonnet, Session::Unmanaged, "p", None).unwrap());
        let overlay = settings_overlay(&a).expect("a --settings overlay");
        assert_eq!(
            overlay,
            serde_json::json!({"skillOverrides": {
                "herdr-orchestrator": "off",
                "herdr-worker": "off",
                "herdr:herdr-orchestrator": "off",
                "herdr:herdr-worker": "off"
            }}),
            "the synced-skills switch must be the only thing opting out removes"
        );

        // Opting in touches only this switch: the plugin off-map stays.
        let mut orch = r.require("orchestrator").unwrap().clone();
        orch.inherit_claudeai_skills = true;
        let a = argv(&command(&orch, Session::Unmanaged, "p", None).unwrap());
        let overlay = settings_overlay(&a).expect("a --settings overlay");
        assert!(overlay.get("syncClaudeAiSkills").is_none(), "{overlay}");
        assert_eq!(overlay["enabledPlugins"]["herdr@m"], false);
    }

    /// Each `disabled_skills` entry is switched off by name, next to the
    /// synced-skills switch. The operator's own overrides are not restated:
    /// Claude merges settings per key, so they still apply.
    #[test]
    fn disabled_skills_become_skill_overrides() {
        let (_home, _g) = fake_home(OPERATOR);
        let r = Roster::builtin().unwrap();
        let mut t = r.require("sonnet").unwrap().clone();
        t.disabled_skills = vec!["dev-prime".into(), "code:core".into()];
        let a = argv(&command(&t, Session::Unmanaged, "p", None).unwrap());
        let overlay = settings_overlay(&a).expect("a --settings overlay");
        assert_eq!(
            overlay,
            serde_json::json!({
                "syncClaudeAiSkills": false,
                "skillOverrides": {"dev-prime": "off", "code:core": "off"}
            })
        );
    }

    /// A teammate's own `settings:` file replaces the plain overlay whole, so
    /// that file has to carry the skill switches itself. (The skill-bundle
    /// path in `skills.rs` merges into the file instead.)
    #[test]
    fn a_teammate_settings_file_replaces_the_plain_overlay() {
        let (_home, _g) = fake_home(OPERATOR);
        let r = Roster::builtin().unwrap();
        let mut t = r.require("sonnet").unwrap().clone();
        t.settings = Some("/tmp/worker-settings.json".into());
        t.disabled_skills = vec!["dev-prime".into()];
        let a = argv(&command(&t, Session::Unmanaged, "p", None).unwrap());
        assert!(
            a.windows(2)
                .any(|w| w == ["--settings", "/tmp/worker-settings.json"]),
            "{a:?}"
        );
        assert!(settings_overlay(&a).is_none(), "{a:?}");
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
        assert!(
            a.windows(2).any(|w| w == ["--setting-sources", ""]),
            "{a:?}"
        );
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
