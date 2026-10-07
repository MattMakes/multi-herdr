//! Antigravity CLI: `agy --model <slug> --effort <level> --prompt-interactive "<prompt>"`.
//!
//! `agy` mints its own conversation ids, and `--conversation <id>` with an id
//! it does not know silently starts a different conversation, so horch cannot
//! choose one. The id is recovered after launch from
//! `~/.gemini/antigravity-cli/cache/last_conversations.json`, which maps each
//! workspace directory to its newest conversation id. A resume passes
//! `--conversation <id>`.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime};

use anyhow::{bail, Result};

use super::launch::model_for;
use super::{CommandSpec, Harness, HarnessKind, LaunchEnv, Session, Workdir};
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

/// The Antigravity adapter.
pub struct Antigravity;

impl Harness for Antigravity {
    fn kind(&self) -> HarnessKind {
        HarnessKind::Antigravity
    }

    fn command(&self, env: &LaunchEnv, spec: &CommandSpec<'_>) -> Result<Command> {
        let teammate = spec.teammate;
        let mut cmd = Command::new(&env.bins.antigravity);
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
}
