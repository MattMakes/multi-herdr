//! Antigravity CLI: `agy --model <slug> --effort <level> --prompt-interactive "<prompt>"`.
//!
//! `agy` mints its own conversation ids, and `--conversation <id>` with an id
//! it does not know silently starts a different conversation, so horch cannot
//! choose one. The id is recovered after launch from
//! `~/.gemini/antigravity-cli/cache/last_conversations.json`, which maps each
//! workspace directory to its newest conversation id. A resume passes
//! `--conversation <id>`.

use horch_platform::start::Command;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use anyhow::{bail, Context, Result};

use super::launch::model_for;
use super::{CommandSpec, Harness, HarnessKind, LaunchEnv, Session, Workdir};
use crate::fleetd::worker_profiles::domain::operator_boundary::{Boundary, NotSupported};
use crate::fleetd::worker_profiles::domain::stop_gate;
use crate::roster::Teammate;
use crate::runtime::RuntimeContext;

/// Variables an `agy` child never receives, on top of
/// [`FORBIDDEN_ENV`](super::launch::FORBIDDEN_ENV). Each one moves `agy` off
/// the operator's Google login: `GEMINI_API_KEY` (read when `modelProvider`
/// is `gemini`), `GOOGLE_API_KEY` (the Gemini tooling's other key name) and
/// `GOOGLE_GEMINI_BASE_URL` (sends model calls to another endpoint). Only
/// `agy` children lose them; another harness or a tool may need them.
pub const ANTIGRAVITY_FORBIDDEN_ENV: [&str; 3] =
    ["GEMINI_API_KEY", "GOOGLE_API_KEY", "GOOGLE_GEMINI_BASE_URL"];

/// The levels `agy models` appends to a model id (`gemini-3.8-flash-low`).
/// An id with one runs without `--effort`; a bare id needs `--effort`.
pub const EFFORT_SUFFIXES: [&str; 3] = ["-low", "-medium", "-high"];

/// The session cache under `home`.
pub(crate) fn conversations_cache(home: &Path) -> PathBuf {
    home.join(".gemini/antigravity-cli/cache/last_conversations.json")
}

/// The conversation id the cache records for `workdir`. Directories compare
/// canonically ([`Workdir`]). A value is the id itself, or an object with an
/// `id` (or `conversation_id`), so a richer record still reads.
pub fn parse_last_conversations(json: &str, workdir: &Path) -> Option<String> {
    let map = serde_json::from_str::<serde_json::Value>(json).ok()?;
    let workdir = Workdir::new(workdir);
    map.as_object()?
        .iter()
        .filter(|(dir, _)| workdir.matches(Path::new(dir)))
        .find_map(|(_, v)| {
            v.as_str()
                .or_else(|| v.get("id").and_then(|i| i.as_str()))
                .or_else(|| v.get("conversation_id").and_then(|i| i.as_str()))
                .filter(|id| !id.is_empty())
                .map(str::to_owned)
        })
}

/// The id `cache` records for `workdir`, when the cache changed at or after
/// `since`. The file has no per-entry time, so its mtime stands in: an entry
/// written before this launch is another worker's, or an old one.
pub(crate) fn find_session(cache: &Path, workdir: &Path, since: SystemTime) -> Option<String> {
    let modified = std::fs::metadata(cache).and_then(|m| m.modified()).ok()?;
    // The launch marker's mtime can be coarser than the cache's.
    if modified < since - Duration::from_secs(1) {
        return None;
    }
    parse_last_conversations(&std::fs::read_to_string(cache).ok()?, workdir)
}

/// The workspace hooks file of `agy`, below the worktree. `agy` loads hooks
/// only from this file, `~/.gemini/config/hooks.json` or a plugin, and not
/// from `--settings` (antigravity.google/docs/hooks?tab=cli).
const HOOKS_FILE: &str = ".agents/hooks.json";

const STOP_HOOK_NAME: &str = "horch-result-gate";

/// The command of the `agy` Stop hook. `agy` keeps the agent working only
/// on the stdout JSON `{"decision":"continue"}`, so exit 2 of
/// `horch result --check` maps to it. Each other exit lets the turn end, as
/// in Claude Code.
pub(crate) fn stop_hook_command(horch_exe: &Path) -> String {
    let keep = serde_json::json!({"decision": "continue", "reason": stop_gate::RESULT_FIRST})
        .to_string()
        .replace('\'', r"'\''");
    format!(
        "{} >/dev/null 2>&1; if [ $? -eq 2 ]; then printf '%s\\n' '{keep}'; else printf '%s\\n' '{{\"decision\":\"stop\"}}'; fi",
        stop_gate::hook_command(horch_exe)
    )
}

fn git_out(dir: &Path, args: &[&str]) -> Option<String> {
    let out = horch_platform::start::command("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

/// The Stop hook serves FM-6. It helps the agent and is not a security
/// boundary: the worker can edit it.
fn seed_stop_hook(worktree: &Path, horch_exe: &Path) -> Result<()> {
    if git_out(worktree, &["ls-files", "--error-unmatch", HOOKS_FILE]).is_some() {
        return Ok(());
    }
    if let Some(common) = git_out(
        worktree,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    ) {
        exclude_hooks_file(&Path::new(&common).join("info/exclude"))?;
    }
    let path = worktree.join(HOOKS_FILE);
    let mut hooks = match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&text)
            .with_context(|| format!("{} must be a JSON object", path.display()))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => serde_json::Map::new(),
        Err(e) => return Err(e).with_context(|| format!("read {}", path.display())),
    };
    hooks.insert(
        STOP_HOOK_NAME.to_owned(),
        serde_json::json!({"Stop": [{"type": "command", "command": stop_hook_command(horch_exe)}]}),
    );
    std::fs::create_dir_all(worktree.join(".agents"))?;
    std::fs::write(&path, serde_json::to_string_pretty(&hooks)?)
        .with_context(|| format!("write {}", path.display()))
}

fn exclude_hooks_file(exclude: &Path) -> Result<()> {
    let line = format!("/{HOOKS_FILE}");
    let text = match std::fs::read_to_string(exclude) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e).with_context(|| format!("read {}", exclude.display())),
    };
    if text.lines().any(|l| l == line) {
        return Ok(());
    }
    let sep = if text.is_empty() || text.ends_with('\n') {
        ""
    } else {
        "\n"
    };
    if let Some(dir) = exclude.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(exclude, format!("{text}{sep}{line}\n"))
        .with_context(|| format!("write {}", exclude.display()))
}

/// The Antigravity adapter.
pub struct Antigravity;

impl Harness for Antigravity {
    fn kind(&self) -> HarnessKind {
        HarnessKind::Antigravity
    }

    fn command(&self, env: &LaunchEnv, spec: &CommandSpec<'_>) -> Result<Command> {
        let teammate = spec.teammate;
        let mut cmd = horch_platform::start::command(&env.bins.antigravity);
        cmd.arg("--model")
            .arg(model_for(teammate, spec.model_override)?);
        if let Some(effort) = &teammate.effort {
            cmd.arg("--effort").arg(effort);
        }
        if let Some(mode) = teammate.permission_mode {
            match mode.antigravity_args() {
                Some(args) => {
                    cmd.args(args);
                }
                None => bail!(
                    "teammate '{}' sets permission_mode '{}', which has no antigravity equivalent",
                    teammate.name,
                    mode.as_str()
                ),
            }
        }
        // Resume only: a fresh conversation's id is discovered afterwards.
        if let Session::Resume(id) = spec.session {
            cmd.arg("--conversation").arg(id);
        }
        cmd.args(&teammate.args);
        // `agy` has no positional prompt. `--prompt-interactive` runs the
        // opening prompt and stays in the TUI; `-p` would exit after it.
        // Beside `--conversation` it runs as a new turn of that conversation
        // (agy 1.3.0, `agy-resume-prompt` in scripts/live/harnesses.sh), so
        // a resume keeps it. OpenCode ignores the same pair (`opencode.rs`).
        cmd.arg("--prompt-interactive").arg(spec.prompt);
        // Removed, not just unset: a removed key counts as taken, so the
        // teammate env and the child env layers cannot add it back.
        for key in ANTIGRAVITY_FORBIDDEN_ENV {
            cmd.env_remove(key);
        }
        if let (Some(_), Some(worktree)) = (&env.task_dir, &env.worktree) {
            seed_stop_hook(worktree, &env.horch_exe)?;
        }
        Ok(cmd)
    }

    fn discover_sessions(
        &self,
        ctx: &RuntimeContext,
        workdir: &Path,
        since: SystemTime,
        _sessions_dir: Option<&Path>,
    ) -> Vec<String> {
        find_session(&conversations_cache(&ctx.paths.home), workdir, since)
            .into_iter()
            .collect()
    }
}

/// The facts of 1 exec attempt of `agy` (design, "How each adapter applies
/// a profile", column Antigravity CLI).
#[derive(Debug, Clone, Copy)]
pub struct ExecLaunch<'a> {
    /// The `agy` program.
    pub bin: &'a Path,
    pub model: &'a str,
    /// `--effort`; `None` for a model id that carries its level.
    pub effort: Option<&'a str>,
    /// The private `HOME` of the attempt, with the seeded
    /// `~/.gemini/antigravity-cli/settings.json`. The login stays in the
    /// OS keychain (antigravity.google/docs/cli/troubleshooting).
    pub home: &'a Path,
    pub workdir: &'a Path,
    /// The path of `inbox/START.md` of the task.
    pub prompt: &'a str,
}

/// The exec launch of 1 attempt:
/// `agy -p <START.md> --output-format stream-json --mode auto --model <m>
/// [--effort <e>]`. `-p` takes the next argument as its prompt. `--mode auto`
/// runs safe tools without a dialog, and headless mode soft-denies the rest
/// (antigravity.google/docs/cli/headless), so the attempt never waits.
pub fn exec_command(launch: &ExecLaunch<'_>) -> Command {
    let mut cmd = horch_platform::start::command(launch.bin);
    cmd.arg("-p")
        .arg(launch.prompt)
        .args(["--output-format", "stream-json", "--mode", "auto"])
        .arg("--model")
        .arg(launch.model);
    if let Some(effort) = launch.effort {
        cmd.arg("--effort").arg(effort);
    }
    cmd.current_dir(launch.workdir).env("HOME", launch.home);
    for key in ANTIGRAVITY_FORBIDDEN_ENV {
        cmd.env_remove(key);
    }
    cmd
}

/// What 1 line of the `stream-json` output means to the wrapper.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// The run made progress.
    Activity,
    /// A tool call that needed approval was denied, and the run goes on.
    SoftDenied { tool: String, message: String },
    /// The plan quota of the model is spent: the outcome `quota` (RR-11).
    UsageLimited { message: String },
    /// The final `result` event with its status, `SUCCESS` or another.
    Result { status: String },
    /// The context of the last model turn, in tokens (FM-20).
    ContextTokens(u64),
    /// A line that is not an event of the stream.
    Other,
}

impl Event {
    /// The events that show a live run: each renews the heartbeat.
    pub fn is_activity(&self) -> bool {
        matches!(
            self,
            Event::Activity | Event::SoftDenied { .. } | Event::ContextTokens(_)
        )
    }
}

/// Words of a tool error that name a soft denial. The docs give the error
/// object (`type`, `message`) but not its text for a denial, so this list
/// is to confirm in conformance (fleetd-p7 MAP R4).
const DENIAL_WORDS: [&str; 3] = ["permission", "denied", "approval"];

/// Words of an `ERROR` result that name a usage limit. Not in the docs; to
/// confirm in conformance (fleetd-p7 MAP R4).
const LIMIT_WORDS: [&str; 4] = ["quota", "rate limit", "usage limit", "resource_exhausted"];

/// Read 1 line of `agy -p --output-format stream-json`
/// (antigravity.google/docs/cli/headless).
pub fn read_event(line: &str) -> Event {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
        return Event::Other;
    };
    match v.get("event").and_then(|e| e.as_str()) {
        Some("init") => Event::Activity,
        Some("step_update") => v.get("step_update").map_or(Event::Other, read_step),
        Some("result") => v.get("result").map_or(Event::Other, read_result),
        _ => Event::Other,
    }
}

fn str_at<'v>(v: &'v serde_json::Value, key: &str) -> &'v str {
    v.get(key).and_then(|s| s.as_str()).unwrap_or("")
}

fn read_step(step: &serde_json::Value) -> Event {
    if let Some(error) = step.get("tool_info").and_then(|t| t.get("error")) {
        let message = str_at(error, "message");
        let text = format!("{} {message}", str_at(error, "type")).to_lowercase();
        if DENIAL_WORDS.iter().any(|w| text.contains(w)) {
            return Event::SoftDenied {
                tool: str_at(step, "tool_name").to_owned(),
                message: message.to_owned(),
            };
        }
    }
    // Only a model turn reports the context: `input_tokens` excludes the
    // cached part, and a checkpoint step is a small call of its own.
    if str_at(step, "step_type") == "agent_response" {
        if let Some(usage) = step.get("usage") {
            let count = |key| usage.get(key).and_then(|n| n.as_u64()).unwrap_or(0);
            return Event::ContextTokens(count("input_tokens") + count("cache_read_tokens"));
        }
    }
    Event::Activity
}

fn read_result(result: &serde_json::Value) -> Event {
    let status = str_at(result, "status");
    let error = str_at(result, "error");
    if status == "ERROR" && names_a_limit(error) {
        return Event::UsageLimited {
            message: error.to_owned(),
        };
    }
    Event::Result {
        status: status.to_owned(),
    }
}

/// `error` names a usage limit: 1 of [`LIMIT_WORDS`], or the HTTP status
/// 429 as a whole word.
fn names_a_limit(error: &str) -> bool {
    let error = error.to_lowercase();
    LIMIT_WORDS.iter().any(|w| error.contains(w))
        || error
            .split(|c: char| !c.is_ascii_alphanumeric())
            .any(|word| word == "429")
}

/// The operator boundary of the Antigravity adapter (SR-8, A37): not
/// supported on any OS. `agy` has no rule that denies a connect to a Unix
/// socket, and its docs do not say that a `read_file` deny binds the
/// commands of its terminal sandbox (antigravity.google/docs/permissions).
/// So no config of `agy` denies the 3 operator acts, and profile
/// validation refuses each Antigravity worker profile.
pub fn operator_boundary(
    _teammate: &Teammate,
    _fleet_root: &Path,
) -> Result<Boundary, NotSupported> {
    Err(NotSupported {
        adapter: "antigravity",
        os: std::env::consts::OS.into(),
        reason: "agy has no rule that denies a connect to a Unix socket, and its docs do not say that a read_file deny binds its terminal commands",
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::launch::command_in;
    use crate::roster::{PermissionMode, Roster};

    fn argv(cmd: &Command) -> Vec<String> {
        std::iter::once(cmd.get_program())
            .chain(cmd.get_args())
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    }

    /// A plain worker moved onto agy, so these tests need no roster entry.
    fn teammate() -> crate::roster::Teammate {
        let mut t = Roster::builtin()
            .unwrap()
            .require("sonnet")
            .unwrap()
            .clone();
        t.agent = HarnessKind::Antigravity;
        t.model = Some("gemini-3.8-flash".into());
        t.effort = None;
        t.permission_mode = None;
        t.disallowed_tools.clear();
        t.args.clear();
        t
    }

    #[test]
    fn fresh_launch_argv() {
        let mut t = teammate();
        t.model = Some("gemini-3.8-flash".into());
        t.effort = Some("high".into());
        t.permission_mode = Some(PermissionMode::Auto);
        let cmd = command_in(&LaunchEnv::for_test(), &t, Session::Unmanaged, "-go", None).unwrap();
        assert_eq!(
            argv(&cmd),
            [
                "agy",
                "--model",
                "gemini-3.8-flash",
                "--effort",
                "high",
                "--sandbox",
                "--dangerously-skip-permissions",
                "--prompt-interactive",
                "-go",
            ]
        );
    }

    #[test]
    fn resume_names_the_conversation_and_args_precede_the_prompt() {
        let mut t = teammate();
        t.args = vec!["--add-dir".into(), "/x".into()];
        let cmd = command_in(
            &LaunchEnv::for_test(),
            &t,
            Session::Resume("c-1"),
            "p",
            None,
        )
        .unwrap();
        let args = argv(&cmd);
        let conv = args.iter().position(|a| a == "--conversation").unwrap();
        assert_eq!(args[conv + 1], "c-1");
        let add = args.iter().position(|a| a == "--add-dir").unwrap();
        let prompt = args
            .iter()
            .position(|a| a == "--prompt-interactive")
            .unwrap();
        assert!(conv < prompt && add < prompt, "{args:?}");
        assert_eq!(args.last().unwrap(), "p");
    }

    /// A fresh launch says nothing about the session: `agy` mints the id.
    #[test]
    fn a_fresh_launch_names_no_conversation() {
        let cmd = command_in(
            &LaunchEnv::for_test(),
            &teammate(),
            Session::Fresh("c-1"),
            "p",
            None,
        )
        .unwrap();
        assert!(!argv(&cmd).contains(&"--conversation".to_string()));
    }

    #[test]
    fn permission_modes_map_or_fail() {
        let mut t = teammate();
        for (mode, want) in [
            (PermissionMode::Plan, vec!["--mode", "plan"]),
            (PermissionMode::AcceptEdits, vec!["--mode", "accept-edits"]),
            (
                PermissionMode::BypassPermissions,
                vec!["--dangerously-skip-permissions"],
            ),
        ] {
            t.permission_mode = Some(mode);
            let cmd =
                command_in(&LaunchEnv::for_test(), &t, Session::Unmanaged, "p", None).unwrap();
            let args = argv(&cmd);
            assert!(
                args.windows(want.len()).any(|w| w == want.as_slice()),
                "{mode:?}: {args:?}"
            );
        }
        for mode in [PermissionMode::Manual, PermissionMode::DontAsk] {
            t.permission_mode = Some(mode);
            assert!(command_in(&LaunchEnv::for_test(), &t, Session::Unmanaged, "p", None).is_err());
        }
    }

    /// Every forbidden key is removed from the child, ANTHROPIC_API_KEY
    /// included.
    #[test]
    fn forbidden_keys_are_removed() {
        let cmd = command_in(
            &LaunchEnv::for_test(),
            &teammate(),
            Session::Unmanaged,
            "p",
            None,
        )
        .unwrap();
        let removed: Vec<String> = cmd
            .get_envs()
            .filter(|(_, v)| v.is_none())
            .map(|(k, _)| k.to_string_lossy().into_owned())
            .collect();
        for key in ANTIGRAVITY_FORBIDDEN_ENV
            .iter()
            .chain(crate::harness::launch::FORBIDDEN_ENV.iter())
        {
            assert!(removed.iter().any(|k| k == key), "{key}: {removed:?}");
        }
    }

    #[test]
    fn the_cache_names_this_workdir_only() {
        let json = r#"{"/proj/other":"c-other","/proj/mine":"c-mine"}"#;
        assert_eq!(
            parse_last_conversations(json, Path::new("/proj/mine")).as_deref(),
            Some("c-mine")
        );
        assert_eq!(
            parse_last_conversations(json, Path::new("/proj/none")),
            None
        );
        let rich = r#"{"/proj/mine":{"id":"c-rich","title":"t"}}"#;
        assert_eq!(
            parse_last_conversations(rich, Path::new("/proj/mine")).as_deref(),
            Some("c-rich")
        );
        for bad in [
            "not json",
            "[]",
            r#"{"/proj/mine":""}"#,
            r#"{"/proj/mine":7}"#,
        ] {
            assert_eq!(
                parse_last_conversations(bad, Path::new("/proj/mine")),
                None,
                "{bad}"
            );
        }
    }

    /// A cache written before the launch is not this worker's.
    #[test]
    fn a_stale_cache_is_ignored() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = conversations_cache(tmp.path());
        std::fs::create_dir_all(cache.parent().unwrap()).unwrap();
        let dir = tmp.path().join("work");
        std::fs::create_dir_all(&dir).unwrap();
        let key = serde_json::to_string(&dir.to_string_lossy()).unwrap();
        std::fs::write(&cache, format!("{{{key}:\"c-1\"}}")).unwrap();
        let before = SystemTime::now() - Duration::from_secs(60);
        assert_eq!(find_session(&cache, &dir, before).as_deref(), Some("c-1"));
        let after = SystemTime::now() + Duration::from_secs(60);
        assert_eq!(find_session(&cache, &dir, after), None);
    }

    #[cfg(unix)]
    #[test]
    fn the_stop_hook_maps_exit_2_to_continue() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let horch = tmp.path().join("my horch");
        for (code, want) in [("2", "continue"), ("0", "stop"), ("1", "stop")] {
            std::fs::write(&horch, format!("#!/bin/sh\necho noise\nexit {code}\n")).unwrap();
            std::fs::set_permissions(&horch, std::fs::Permissions::from_mode(0o755)).unwrap();
            let out = horch_platform::start::command("sh")
                .arg("-c")
                .arg(stop_hook_command(&horch))
                .output()
                .unwrap();
            let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
            assert_eq!(v["decision"], want, "exit {code}");
            if want == "continue" {
                assert_eq!(v["reason"], stop_gate::RESULT_FIRST);
            }
        }
    }

    #[test]
    fn a_limit_is_named_by_a_word_not_by_a_digit_run() {
        assert!(names_a_limit("HTTP 429 Too Many Requests"));
        assert!(names_a_limit("Rate limit reached"));
        assert!(!names_a_limit("step 14290 failed"));
        assert!(!names_a_limit(""));
    }
}
