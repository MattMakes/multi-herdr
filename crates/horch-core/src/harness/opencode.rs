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
use crate::runtime::RuntimeContext;
use crate::skills::Bundle;
use crate::teammates::Teammate;

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
pub fn find_sessions(bin: &Path, project_dir: &str, since: SystemTime) -> Vec<SessionCandidate> {
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

/// Split out from [`find_sessions`] so the filtering is testable without an
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
        let inherited = cmd
            .get_envs()
            .find(|(k, _)| *k == "OPENCODE_CONFIG_CONTENT")
            .and_then(|(_, v)| v.map(|v| v.to_string_lossy().into_owned()))
            .or_else(|| teammate.env.get("OPENCODE_CONFIG_CONTENT").cloned())
            .or_else(|| inherited.map(str::to_owned));
        cmd.env(
            "OPENCODE_CONFIG_CONTENT",
            skills_config(inherited.as_deref(), &skills.skills_dir())?,
        );
        Ok(())
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
/// this file ordered the same way.
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
    // launches has no `--variant` flag - 1.18.2 swallows it silently
    // (ai_docs/reports/env-research/codex-opencode.md) - so the level goes
    // through the config overlay instead, as the default agent's `variant`.
    if let Some(effort) = &teammate.effort {
        let inherited = teammate
            .env
            .get("OPENCODE_CONFIG_CONTENT")
            .cloned()
            .or_else(|| env.opencode_config_content.clone());
        cmd.env(
            "OPENCODE_CONFIG_CONTENT",
            opencode_variant_config(inherited.as_deref(), effort)?,
        );
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
