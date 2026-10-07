//! OpenCode-specific glue: recovering a session id after launch.
//!
//! OpenCode mints its own `ses_...` ids and offers no way to supply one, so a
//! fresh worker's resume handle has to be recovered the way codex's is. Unlike
//! codex there are no rollout files to scan: `opencode session list` is the
//! supported interface, it takes `--format json`, and every record carries the
//! `directory` it belongs to and when it was created. That is enough to tell one
//! worker's session from another's without reading OpenCode's database.

use std::path::Path;
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context, Result};

use super::launch::model_for;
use super::{CommandSpec, Harness, HarnessKind, LaunchEnv, Session, Workdir};
use crate::roster::{HarnessDefault, Teammate};
use crate::runtime::RuntimeContext;
use crate::skills::Bundle;

/// A session that could belong to this worker.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionCandidate {
    pub session_id: String,
    pub created: SystemTime,
}

/// Sessions created at or after `since` whose directory is `project_dir`,
/// newest first.
///
/// The directory check is what keeps two projects' concurrent OpenCode sessions
/// apart, exactly as the cwd check does for codex rollouts. Directories
/// compare canonically ([`Workdir`]).
pub(crate) fn find_sessions(
    bin: &Path,
    project_dir: &str,
    since: SystemTime,
) -> Vec<SessionCandidate> {
    let output = Command::new(bin)
        .args(["session", "list", "--format", "json"])
        .current_dir(project_dir)
        .output();
    let Ok(output) = output else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    parse_sessions(&String::from_utf8_lossy(&output.stdout), project_dir, since)
}

/// Split out from `find_sessions` so the filtering is testable without an
/// OpenCode install.
pub fn parse_sessions(json: &str, project_dir: &str, since: SystemTime) -> Vec<SessionCandidate> {
    let Ok(records) = serde_json::from_str::<Vec<serde_json::Value>>(json) else {
        return Vec::new();
    };
    // The launch marker's timestamp has whole-second resolution on some
    // filesystems while OpenCode records milliseconds, so a session created in
    // the same second as the marker would otherwise be missed.
    let since = since - Duration::from_secs(1);
    let workdir = Workdir::new(project_dir);
    let mut found: Vec<SessionCandidate> = records
        .iter()
        .filter(|r| {
            r.get("directory")
                .and_then(|d| d.as_str())
                .is_some_and(|d| workdir.matches(Path::new(d)))
        })
        .filter_map(|r| {
            let created = millis(r.get("created")?)?;
            Some(SessionCandidate {
                session_id: r.get("id")?.as_str()?.to_string(),
                created,
            })
        })
        .filter(|c| c.created >= since)
        .collect();
    found.sort_by_key(|c| std::cmp::Reverse(c.created));
    found
}

fn millis(value: &serde_json::Value) -> Option<SystemTime> {
    let ms = value.as_u64()?;
    Some(UNIX_EPOCH + Duration::from_millis(ms))
}

/// The OpenCode adapter.
pub struct OpenCode;

impl Harness for OpenCode {
    fn kind(&self) -> HarnessKind {
        HarnessKind::OpenCode
    }

    /// The OpenCode free-tier models report `variants: {}` (`opencode models
    /// --verbose`), so an effort there is silently a no-op.
    fn model_takes_effort(&self, model: &str) -> bool {
        !(model.starts_with("opencode/")
            && (model.ends_with("-free") || model == "opencode/big-pickle"))
    }

    /// `skills.paths` in `OPENCODE_CONFIG_CONTENT` gains the bundle's
    /// `skills/` directory, which holds exactly the activated skills.
    fn expose_skills_env(
        &self,
        cmd: &mut Command,
        teammate: &Teammate,
        skills: &Bundle,
        inherited: Option<&str>,
    ) -> Result<()> {
        // The builder may already have set it (the effort variant); build
        // on that rather than on the teammate's or the operator's value.
        let set = cmd
            .get_envs()
            .find(|(k, _)| *k == "OPENCODE_CONFIG_CONTENT")
            .and_then(|(_, v)| v.map(|v| v.to_string_lossy().into_owned()));
        let inherited = match set {
            Some(set) => Some(set),
            None => {
                let base = teammate
                    .env
                    .get("OPENCODE_CONFIG_CONTENT")
                    .map(String::as_str)
                    .or(inherited);
                with_config_defaults(base, teammate)?
            }
        };
        cmd.env(
            "OPENCODE_CONFIG_CONTENT",
            skills_config(inherited.as_deref(), &skills.skills_dir())?,
        );
        Ok(())
    }

    /// opencode 1.18.34 opens `--session <id>` and ignores `--prompt`
    /// (LA-3, 2026-10-04; re-checked live 2026-10-06, U-64,
    /// docs/live-checks/harnesses.md). The resume argv carries no `--prompt`
    /// (`opencode_command`), so the typed prompt is the 1 delivery path
    /// and a later opencode cannot deliver the task twice.
    fn resume_prompt_typed(&self) -> bool {
        true
    }

    fn command(&self, env: &LaunchEnv, spec: &CommandSpec<'_>) -> Result<Command> {
        opencode_command(
            env,
            spec.teammate,
            spec.session,
            spec.prompt,
            spec.model_override,
        )
    }

    /// `opencode session list` records whose directory is `workdir`.
    fn discover_sessions(
        &self,
        ctx: &RuntimeContext,
        workdir: &Path,
        since: SystemTime,
        _sessions_dir: Option<&Path>,
    ) -> Vec<String> {
        find_sessions(
            &ctx.bins.harness.opencode,
            &workdir.to_string_lossy(),
            since,
        )
        .into_iter()
        .map(|c| c.session_id)
        .collect()
    }
}

/// OpenCode: `opencode --model provider/model --prompt "..."`.
///
/// The prompt is a FLAG here, not a trailing positional, so it cannot be eaten
/// by a variadic - but `args` still goes before it, to keep every builder in
/// this file ordered the same way. A resume passes no `--prompt`: opencode
/// ignores it beside `--session`, and the launch types the prompt in
/// instead ([`Harness::resume_prompt_typed`]), so the task arrives once.
pub(super) fn opencode_command(
    env: &LaunchEnv,
    teammate: &Teammate,
    session: Session<'_>,
    prompt: &str,
    model_override: Option<&str>,
) -> Result<Command> {
    let mut cmd = Command::new(&env.bins.opencode);
    cmd.arg("--model").arg(model_for(teammate, model_override)?);

    // OpenCode calls reasoning effort a model "variant". The TUI horch
    // launches has no `--variant` flag - 1.18.2 swallows it silently - so the level goes
    // through the config overlay instead, as the default agent's `variant`.
    // The teammate's config, else the operator's, with the harness default
    // `config` keys merged under it.
    let base = teammate
        .env
        .get("OPENCODE_CONFIG_CONTENT")
        .or(env.opencode_config_content.as_ref());
    let config = with_config_defaults(base.map(String::as_str), teammate)?;
    if let Some(effort) = &teammate.effort {
        cmd.env(
            "OPENCODE_CONFIG_CONTENT",
            opencode_variant_config(config.as_deref(), effort)?,
        );
    } else if config.as_ref() != base {
        // Only when a default added a key: else the teammate env layer (or
        // the inherited env) already carries this exact value.
        if let Some(config) = config {
            cmd.env("OPENCODE_CONFIG_CONTENT", config);
        }
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
    if !matches!(session, Session::Resume(_)) {
        cmd.arg("--prompt").arg(prompt);
    }
    Ok(cmd)
}

/// Merge `agent.build.variant` into an OpenCode config overlay. `build` is
/// the primary agent the TUI starts in, and a variant set there applies to
/// that agent's model, which is the one horch passes with `--model`.
pub(crate) fn opencode_variant_config(inherited: Option<&str>, effort: &str) -> Result<String> {
    let mut value: serde_json::Value = match inherited {
        Some(s) => serde_json::from_str(s).context("invalid OPENCODE_CONFIG_CONTENT JSON")?,
        None => serde_json::json!({}),
    };
    let build = value
        .as_object_mut()
        .context("OPENCODE_CONFIG_CONTENT must be an object")?
        .entry("agent")
        .or_insert(serde_json::json!({}))
        .as_object_mut()
        .context("agent config must be an object")?
        .entry("build")
        .or_insert(serde_json::json!({}))
        .as_object_mut()
        .context("agent.build config must be an object")?;
    build.insert("variant".into(), serde_json::json!(effort));
    Ok(value.to_string())
}

/// `base` (an `OPENCODE_CONFIG_CONTENT` text) with the `config` keys of the
/// teammate's OpenCode harness defaults deep-merged under it: a key path
/// `base` already has wins. Among the defaults, a later entry wins. `base`
/// comes back unchanged, byte for byte, when no default adds a key.
pub(crate) fn with_config_defaults(
    base: Option<&str>,
    teammate: &Teammate,
) -> Result<Option<String>> {
    let mut defaults = serde_json::Map::new();
    for entry in HarnessDefault::for_harness(&teammate.harness_defaults, HarnessKind::OpenCode) {
        if let Some(config) = &entry.config {
            merge_json(&mut defaults, config, true);
        }
    }
    if defaults.is_empty() {
        return Ok(base.map(str::to_owned));
    }
    let mut value: serde_json::Value = match base {
        Some(s) => serde_json::from_str(s).context("invalid OPENCODE_CONFIG_CONTENT JSON")?,
        None => serde_json::json!({}),
    };
    let object = value
        .as_object_mut()
        .context("OPENCODE_CONFIG_CONTENT must be an object")?;
    if !merge_json(object, &defaults, false) {
        return Ok(base.map(str::to_owned));
    }
    Ok(Some(value.to_string()))
}

/// Deep-merge `from` into `into`. `overwrite`: a leaf of `from` replaces the
/// same leaf of `into`; else `into` keeps it. Returns whether `into` changed.
pub(crate) fn merge_json(
    into: &mut serde_json::Map<String, serde_json::Value>,
    from: &serde_json::Map<String, serde_json::Value>,
    overwrite: bool,
) -> bool {
    let mut changed = false;
    for (key, value) in from {
        match (into.get_mut(key), value) {
            (Some(serde_json::Value::Object(inner)), serde_json::Value::Object(add)) => {
                changed |= merge_json(inner, add, overwrite);
            }
            (Some(old), _) => {
                if overwrite && old != value {
                    *old = value.clone();
                    changed = true;
                }
            }
            (None, _) => {
                into.insert(key.clone(), value.clone());
                changed = true;
            }
        }
    }
    changed
}

/// Add `skills` to `skills.paths` in an OpenCode config overlay, keeping
/// everything else in it.
fn skills_config(inherited: Option<&str>, skills: &Path) -> Result<String> {
    let mut value: serde_json::Value = match inherited {
        Some(s) => serde_json::from_str(s).context("invalid OPENCODE_CONFIG_CONTENT JSON")?,
        None => serde_json::json!({}),
    };
    let object = value
        .as_object_mut()
        .context("OPENCODE_CONFIG_CONTENT must be an object")?;
    let config = object
        .entry("skills")
        .or_insert(serde_json::json!({}))
        .as_object_mut()
        .context("skills config must be an object")?;
    let paths = config
        .entry("paths")
        .or_insert(serde_json::json!([]))
        .as_array_mut()
        .context("skills.paths must be an array")?;
    let path = serde_json::json!(skills);
    if !paths.contains(&path) {
        paths.push(path);
    }
    Ok(value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(cmd: &Command) -> Vec<String> {
        cmd.get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    }

    /// U-64: opencode 1.18.34 ignores `--prompt` beside `--session` (seen
    /// live, docs/live-checks/harnesses.md), and the launch types the prompt
    /// in. A resume argv without the flag delivers the task once, even if a
    /// later opencode starts to honour it. A fresh launch keeps it.
    #[test]
    fn only_a_fresh_launch_passes_the_prompt_flag() {
        use super::super::launch::command_in;
        let teammate = crate::roster::Roster::builtin()
            .unwrap()
            .require("opencode-pickle")
            .unwrap()
            .clone();
        let env = LaunchEnv::for_test();
        let fresh = argv(&command_in(&env, &teammate, Session::Unmanaged, "TASK", None).unwrap());
        assert_eq!(fresh[fresh.len() - 2..], ["--prompt", "TASK"], "{fresh:?}");

        let resumed =
            argv(&command_in(&env, &teammate, Session::Resume("ses_abc"), "TASK", None).unwrap());
        assert!(
            resumed.windows(2).any(|w| w == ["--session", "ses_abc"]),
            "{resumed:?}"
        );
        assert!(
            !resumed.iter().any(|a| a == "--prompt" || a == "TASK"),
            "{resumed:?}"
        );
        assert!(OpenCode.resume_prompt_typed());
    }

    #[test]
    fn skills_opencode_overlay_preserves_provider_and_denials() {
        let before = r#"{"provider":{"private":{"name":"keep"}},"permission":{"bash":"deny"},"skills":{"paths":["old"],"urls":["https://example.com"]}}"#;
        let actual: serde_json::Value =
            serde_json::from_str(&skills_config(Some(before), Path::new("/new skills")).unwrap())
                .unwrap();
        assert_eq!(actual["provider"]["private"]["name"], "keep");
        assert_eq!(actual["permission"]["bash"], "deny");
        assert_eq!(
            actual["skills"]["paths"],
            serde_json::json!(["old", "/new skills"])
        );
        assert_eq!(
            actual["skills"]["urls"],
            serde_json::json!(["https://example.com"])
        );
        for invalid in [
            "oops",
            "[]",
            r#"{"skills":false}"#,
            r#"{"skills":{"paths":false}}"#,
        ] {
            assert!(skills_config(Some(invalid), Path::new("/x")).is_err());
        }
    }

    fn at(ms: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_millis(ms)
    }

    const LIST: &str = r#"[
        {"id":"ses_new","title":"t","updated":1788648167866,"created":1788648167000,
         "projectId":"global","directory":"/proj/mine"},
        {"id":"ses_old","title":"t","updated":1788648100000,"created":1700000000000,
         "projectId":"global","directory":"/proj/mine"},
        {"id":"ses_elsewhere","title":"t","updated":1788648167900,"created":1788648167900,
         "projectId":"global","directory":"/proj/other"}
    ]"#;

    /// The two filters that keep workers apart: another project's session is not
    /// this worker's, and neither is one that existed before it launched.
    #[test]
    fn only_this_project_and_only_after_launch() {
        let found = parse_sessions(LIST, "/proj/mine", at(1788648160000));
        assert_eq!(found.len(), 1, "{found:#?}");
        assert_eq!(found[0].session_id, "ses_new");
    }

    #[test]
    fn newest_first() {
        let json = r#"[
            {"id":"ses_a","created":1000000,"directory":"/proj/mine"},
            {"id":"ses_c","created":3000000,"directory":"/proj/mine"},
            {"id":"ses_b","created":2000000,"directory":"/proj/mine"}
        ]"#;
        let found = parse_sessions(json, "/proj/mine", at(0));
        let ids: Vec<&str> = found.iter().map(|c| c.session_id.as_str()).collect();
        assert_eq!(ids, ["ses_c", "ses_b", "ses_a"]);
    }

    /// A session created in the same second the marker was written still counts:
    /// the marker's mtime can be coarser than OpenCode's millisecond stamp.
    #[test]
    fn a_session_from_the_launch_second_is_not_missed() {
        let found = parse_sessions(
            r#"[{"id":"ses_a","created":1788648167100,"directory":"/proj/mine"}]"#,
            "/proj/mine",
            at(1788648167900),
        );
        assert_eq!(found.len(), 1, "{found:#?}");
    }

    /// Output that is not the JSON this expects must yield nothing rather than
    /// panicking a background thread nobody is watching.
    #[test]
    fn unparseable_output_is_empty_not_fatal() {
        assert!(parse_sessions("not json", "/proj/mine", at(0)).is_empty());
        assert!(parse_sessions("[]", "/proj/mine", at(0)).is_empty());
        assert!(parse_sessions(r#"[{"id":"x"}]"#, "/proj/mine", at(0)).is_empty());
    }
}
