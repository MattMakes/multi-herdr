//! FR-9 and Phase 7 of `docs/specs/fleetd/design.md`: each Claude and Codex
//! seat of the repository roster launches with 256000 as its native
//! compaction window, and `_base/context-windows.md` gives 256000 to each
//! key. A temp home with no operator settings: the fleet value applies.

use std::path::{Path, PathBuf};

use horch_core::harness::launch::{self, window_decision};
use horch_core::harness::{HarnessKind, LaunchEnv, Session};
use horch_core::roster::Roster;
use horch_core::runtime::{MapEnv, RuntimeContext};
use serde_json::Value;

const LIMIT: &str = "256000";

fn teammates_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../teammates")
}

/// A context with an empty temp home: no operator window anywhere.
fn empty_home_ctx(tmp: &Path) -> RuntimeContext {
    let home = tmp.join("home");
    let workdir = tmp.join("project");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&workdir).unwrap();
    let env = MapEnv::new(&workdir)
        .with("HOME", &home.to_string_lossy())
        .with("PATH", "/nonexistent")
        .with(
            "HORCH_CLAUDE_MANAGED_SETTINGS",
            &tmp.join("managed-settings.json").to_string_lossy(),
        )
        .with("HORCH_STATE_DIR", &home.join("state").to_string_lossy());
    RuntimeContext::from_env(&env).unwrap()
}

/// The window that the launch argv of a Claude or Codex seat sets.
fn window_in_argv(kind: HarnessKind, args: &[String]) -> Option<String> {
    match kind {
        HarnessKind::Claude => {
            let i = args.iter().position(|a| a == "--settings")?;
            let settings: Value = serde_json::from_str(&args[i + 1]).ok()?;
            settings["env"]["CLAUDE_CODE_AUTO_COMPACT_WINDOW"]
                .as_str()
                .map(str::to_string)
        }
        HarnessKind::Codex => args.windows(2).filter(|w| w[0] == "-c").find_map(|w| {
            w[1].strip_prefix("model_auto_compact_token_limit=")
                .map(str::to_string)
        }),
        _ => None,
    }
}

#[test]
fn launch_environment_sets_256k_context_limit() {
    let roster = Roster::builtin().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let ctx = empty_home_ctx(tmp.path());
    let workdir = ctx.paths.cwd.clone().unwrap();

    let mut seats = 0;
    let mut wrong = Vec::new();
    for name in roster.names() {
        let t = roster.require(name).unwrap();
        if !matches!(t.agent, HarnessKind::Claude | HarnessKind::Codex) {
            continue;
        }
        let Some(model) = t.model.clone() else {
            continue;
        };
        seats += 1;
        let fleet = roster.fleet_window(t, &model);
        let decision = window_decision(&ctx, t, &model, &workdir, fleet);
        let mut env = LaunchEnv::from_context(&ctx);
        env.compact_window = Some(decision);
        let cmd =
            launch::command_with_skills_in(&env, t, Session::Fresh("sid"), "PROMPT", None, None)
                .unwrap();
        let args: Vec<String> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        let got = window_in_argv(t.agent, &args);
        if got.as_deref() != Some(LIMIT) {
            wrong.push(format!("{name}: {got:?}"));
        }
    }
    assert!(seats > 0, "the roster has no Claude or Codex seat");
    assert!(wrong.is_empty(), "seats without {LIMIT}: {wrong:#?}");
}

#[test]
fn every_window_of_context_windows_is_256000() {
    let text = std::fs::read_to_string(teammates_dir().join("_base/context-windows.md")).unwrap();
    let front = text
        .strip_prefix("---\n")
        .and_then(|rest| rest.split("\n---\n").next())
        .expect("frontmatter");
    let doc: serde_yaml::Value = serde_yaml::from_str(front).unwrap();
    let windows = doc["windows"].as_mapping().expect("windows");
    assert!(!windows.is_empty());
    for (key, value) in windows {
        assert_eq!(value.as_u64(), Some(256_000), "{key:?}");
    }
}
