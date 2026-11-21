use std::path::Path;
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};

use super::launch::model_for;
use super::{CommandSpec, Harness, HarnessKind, LaunchEnv, Session, Workdir};
use crate::roster::{HarnessDefault, Teammate};
use crate::runtime::RuntimeContext;
use crate::skills::Bundle;

#[derive(Debug, Clone, PartialEq)]
pub struct SessionCandidate {
    pub session_id: String,
    pub created: SystemTime,
}

pub(crate) fn find_sessions(
    bin: &Path,
    project_dir: &str,
    since: SystemTime,
) -> Vec<SessionCandidate> {
    let output = Command::new(bin)
        .args(["session", "list", "--format", "json"])
        .current_dir(project_dir)
        .output();
    match output {
        Ok(o) if o.status.success() => {
            parse_sessions(&String::from_utf8_lossy(&o.stdout), project_dir, since)
        }
        _ => Vec::new(),
    }
}

pub fn parse_sessions(json: &str, project_dir: &str, since: SystemTime) -> Vec<SessionCandidate> {
    let Ok(records) = serde_json::from_str::<Vec<serde_json::Value>>(json) else {
        return Vec::new();
    };
    let since = since - Duration::from_secs(1);
    let workdir = Workdir::new(project_dir);
    let mut found: Vec<SessionCandidate> = records
        .iter()
        .filter(|r| in_project(r, &workdir))
        .filter_map(candidate)
        .filter(|c| c.created >= since)
        .collect();
    found.sort_by_key(|c| std::cmp::Reverse(c.created));
    found
}

fn in_project(record: &serde_json::Value, workdir: &Workdir) -> bool {
    record
        .get("directory")
        .and_then(|d| d.as_str())
        .is_some_and(|d| workdir.matches(Path::new(d)))
}

fn candidate(record: &serde_json::Value) -> Option<SessionCandidate> {
    let created = millis(record.get("created")?)?;
    Some(SessionCandidate {
        session_id: record.get("id")?.as_str()?.to_string(),
        created,
    })
}

fn millis(value: &serde_json::Value) -> Option<SystemTime> {
    let ms = value.as_u64()?;
    Some(UNIX_EPOCH + Duration::from_millis(ms))
}

pub struct OpenCode;

impl Harness for OpenCode {
    fn kind(&self) -> HarnessKind {
        HarnessKind::OpenCode
    }

    fn model_takes_effort(&self, model: &str) -> bool {
        !(model.starts_with("opencode/")
            && (model.ends_with("-free") || model == "opencode/big-pickle"))
    }

    fn expose_skills_env(
        &self,
        cmd: &mut Command,
        teammate: &Teammate,
        skills: &Bundle,
        inherited: Option<&str>,
    ) -> Result<()> {
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

pub(super) fn opencode_command(
    env: &LaunchEnv,
    teammate: &Teammate,
    session: Session<'_>,
    prompt: &str,
    model_override: Option<&str>,
) -> Result<Command> {
    let mut cmd = Command::new(&env.bins.opencode);
    cmd.arg("--model").arg(model_for(teammate, model_override)?);

    if let Some(config) = config_env(env, teammate)? {
        cmd.env("OPENCODE_CONFIG_CONTENT", config);
    }
    push_permission_args(&mut cmd, teammate)?;
    if !teammate.inherit_plugins {
        cmd.arg("--pure");
    }
    push_session_args(&mut cmd, session, &teammate.args, prompt);
    Ok(cmd)
}

fn config_env(env: &LaunchEnv, teammate: &Teammate) -> Result<Option<String>> {
    let base = teammate
        .env
        .get("OPENCODE_CONFIG_CONTENT")
        .or(env.opencode_config_content.as_ref());
    let config = with_config_defaults(base.map(String::as_str), teammate)?;
    match &teammate.effort {
        Some(effort) => opencode_variant_config(config.as_deref(), effort).map(Some),
        None => Ok(config.filter(|c| Some(c) != base)),
    }
}

fn push_permission_args(cmd: &mut Command, teammate: &Teammate) -> Result<()> {
    let Some(mode) = teammate.permission_mode else {
        return Ok(());
    };
    let args = mode.opencode_args().with_context(|| {
        format!(
            "teammate '{}' sets permission_mode '{}', which has no opencode equivalent",
            teammate.name,
            mode.as_str()
        )
    })?;
    cmd.args(args);
    Ok(())
}

fn push_session_args(cmd: &mut Command, session: Session<'_>, args: &[String], prompt: &str) {
    if let Session::Resume(id) = session {
        cmd.arg("--session").arg(id).args(args);
    } else {
        cmd.args(args).arg("--prompt").arg(prompt);
    }
}

fn parse_config(text: Option<&str>) -> Result<serde_json::Value> {
    match text {
        Some(s) => serde_json::from_str(s).context("invalid OPENCODE_CONFIG_CONTENT JSON"),
        None => Ok(serde_json::json!({})),
    }
}

pub(crate) fn opencode_variant_config(inherited: Option<&str>, effort: &str) -> Result<String> {
    let mut value = parse_config(inherited)?;
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

pub(crate) fn with_config_defaults(
    base: Option<&str>,
    teammate: &Teammate,
) -> Result<Option<String>> {
    let defaults = opencode_defaults(teammate);
    let merged = if defaults.is_empty() {
        None
    } else {
        merged_config(base, &defaults)?
    };
    Ok(merged.or_else(|| base.map(str::to_owned)))
}

fn opencode_defaults(teammate: &Teammate) -> serde_json::Map<String, serde_json::Value> {
    let mut defaults = serde_json::Map::new();
    HarnessDefault::for_harness(&teammate.harness_defaults, HarnessKind::OpenCode)
        .filter_map(|entry| entry.config.as_ref())
        .for_each(|config| {
            merge_json(&mut defaults, config, true);
        });
    defaults
}

fn merged_config(
    base: Option<&str>,
    defaults: &serde_json::Map<String, serde_json::Value>,
) -> Result<Option<String>> {
    let mut value = parse_config(base)?;
    let object = value
        .as_object_mut()
        .context("OPENCODE_CONFIG_CONTENT must be an object")?;
    let changed = merge_json(object, defaults, false);
    Ok(changed.then(|| value.to_string()))
}

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

fn skills_config(inherited: Option<&str>, skills: &Path) -> Result<String> {
    let mut value = parse_config(inherited)?;
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

    fn pickle() -> crate::roster::Teammate {
        let mut t = crate::roster::Roster::builtin()
            .unwrap()
            .require("opencode-pickle")
            .unwrap()
            .clone();
        t.effort = None;
        t.permission_mode = None;
        t.inherit_plugins = true;
        t.args = Vec::new();
        t.env.clear();
        t.harness_defaults.clear();
        t
    }

    fn config_env(cmd: &Command) -> Option<String> {
        cmd.get_envs()
            .find(|(k, _)| *k == "OPENCODE_CONFIG_CONTENT")
            .and_then(|(_, v)| v.map(|v| v.to_string_lossy().into_owned()))
    }

    fn defaults_with(config: serde_json::Value) -> Vec<HarnessDefault> {
        vec![HarnessDefault {
            harness: HarnessKind::OpenCode,
            base: None,
            env: Default::default(),
            args: Vec::new(),
            settings: None,
            config: config.as_object().cloned(),
            agent_settings: None,
            force: None,
        }]
    }

    #[test]
    fn flags_come_in_order_pure_session_args_prompt() {
        let mut t = pickle();
        t.inherit_plugins = false;
        t.permission_mode = Some(crate::roster::PermissionMode::Auto);
        t.args = vec!["--extra".into()];
        let env = LaunchEnv::for_test();
        let fresh = argv(&opencode_command(&env, &t, Session::Unmanaged, "TASK", None).unwrap());
        assert_eq!(fresh[0], "--model");
        assert_eq!(
            fresh[2..],
            ["--auto", "--pure", "--extra", "--prompt", "TASK"]
        );
        let resumed =
            argv(&opencode_command(&env, &t, Session::Resume("ses_1"), "TASK", None).unwrap());
        assert_eq!(
            resumed[2..],
            ["--auto", "--pure", "--session", "ses_1", "--extra"]
        );
    }

    #[test]
    fn a_mode_without_an_opencode_equivalent_is_refused() {
        let mut t = pickle();
        t.permission_mode = Some(crate::roster::PermissionMode::Plan);
        let err = opencode_command(&LaunchEnv::for_test(), &t, Session::Unmanaged, "x", None)
            .unwrap_err()
            .to_string();
        assert!(err.contains("has no opencode equivalent"), "{err}");
        assert!(err.contains("'plan'"), "{err}");
    }

    #[test]
    fn config_env_follows_effort_and_defaults() {
        let env = LaunchEnv::for_test();
        let launch = |t: &crate::roster::Teammate| {
            config_env(&opencode_command(&env, t, Session::Unmanaged, "x", None).unwrap())
        };
        assert_eq!(launch(&pickle()), None);

        let mut t = pickle();
        t.effort = Some("high".into());
        let v: serde_json::Value = serde_json::from_str(&launch(&t).unwrap()).unwrap();
        assert_eq!(v["agent"]["build"]["variant"], "high");

        let mut t = pickle();
        t.harness_defaults = defaults_with(serde_json::json!({"share":"disabled"}));
        let v: serde_json::Value = serde_json::from_str(&launch(&t).unwrap()).unwrap();
        assert_eq!(v, serde_json::json!({"share":"disabled"}));

        t.env.insert(
            "OPENCODE_CONFIG_CONTENT".into(),
            r#"{"share":"manual"}"#.into(),
        );
        assert_eq!(launch(&t), None);

        t.effort = Some("max".into());
        t.env
            .insert("OPENCODE_CONFIG_CONTENT".into(), r#"{"theme":"x"}"#.into());
        let v: serde_json::Value = serde_json::from_str(&launch(&t).unwrap()).unwrap();
        assert_eq!(v["theme"], "x");
        assert_eq!(v["share"], "disabled");
        assert_eq!(v["agent"]["build"]["variant"], "max");
    }

    #[test]
    fn config_defaults_keep_the_base_bytes_when_nothing_is_added() {
        let t = pickle();
        assert_eq!(
            with_config_defaults(Some(" {\"a\" : 1} "), &t).unwrap(),
            Some(" {\"a\" : 1} ".to_string())
        );
        let mut t = pickle();
        t.harness_defaults = defaults_with(serde_json::json!({"a":2}));
        assert_eq!(
            with_config_defaults(Some(r#"{"a":1}"#), &t)
                .unwrap()
                .as_deref(),
            Some(r#"{"a":1}"#)
        );
        assert_eq!(
            with_config_defaults(None, &t).unwrap().as_deref(),
            Some(r#"{"a":2}"#)
        );
        assert!(with_config_defaults(Some("[]"), &t).is_err());
        assert!(with_config_defaults(Some("oops"), &t).is_err());
    }

    #[test]
    fn find_sessions_is_empty_when_the_binary_fails_or_is_missing() {
        assert!(find_sessions(Path::new("/nonexistent/opencode"), "/tmp", at(0)).is_empty());
        assert!(find_sessions(Path::new("false"), "/tmp", at(0)).is_empty());
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

    #[test]
    fn a_session_from_the_launch_second_is_not_missed() {
        let found = parse_sessions(
            r#"[{"id":"ses_a","created":1788648167100,"directory":"/proj/mine"}]"#,
            "/proj/mine",
            at(1788648167900),
        );
        assert_eq!(found.len(), 1, "{found:#?}");
    }

    #[test]
    fn unparseable_output_is_empty_not_fatal() {
        assert!(parse_sessions("not json", "/proj/mine", at(0)).is_empty());
        assert!(parse_sessions("[]", "/proj/mine", at(0)).is_empty());
        assert!(parse_sessions(r#"[{"id":"x"}]"#, "/proj/mine", at(0)).is_empty());
        let one = r#"{"id":"ses_a","created":5000,"directory":"/proj/mine"}"#;
        assert!(parse_sessions(one, "/proj/mine", at(0)).is_empty());
    }

    #[test]
    fn a_record_of_this_project_needs_a_numeric_created_and_a_text_id() {
        let json = r#"[
            {"id":"ses_ok1","created":5000,"directory":"/proj/mine"},
            {"id":"ses_nocreated","directory":"/proj/mine"},
            {"id":"ses_text","created":"5000","directory":"/proj/mine"},
            {"id":"ses_float","created":5000.5,"directory":"/proj/mine"},
            {"id":"ses_negative","created":-5000,"directory":"/proj/mine"},
            {"created":5000,"directory":"/proj/mine"},
            {"id":7,"created":5000,"directory":"/proj/mine"},
            {"id":"ses_nodir","created":5000},
            {"id":"ses_dirnum","created":5000,"directory":5},
            {"id":"ses_ok2","created":5000,"directory":"/proj/mine"}
        ]"#;
        let found = parse_sessions(json, "/proj/mine", at(0));
        let ids: Vec<&str> = found.iter().map(|c| c.session_id.as_str()).collect();
        assert_eq!(ids, ["ses_ok1", "ses_ok2"]);
        assert!(found.iter().all(|c| c.created == at(5000)), "{found:#?}");
    }

    #[test]
    fn the_cut_is_one_second_before_the_marker_and_includes_it() {
        let json = r#"[
            {"id":"ses_edge","created":9000,"directory":"/proj/mine"},
            {"id":"ses_early","created":8999,"directory":"/proj/mine"}
        ]"#;
        let found = parse_sessions(json, "/proj/mine", at(10_000));
        let ids: Vec<&str> = found.iter().map(|c| c.session_id.as_str()).collect();
        assert_eq!(ids, ["ses_edge"]);
    }
}
