//! Per-harness launch defaults: `teammates/_base/harness-defaults.md`
//! (HDF-01 to HDF-04, design `ai_docs/plans/wave2/w1/design.md` section 6A).
//! No real harness binary and no herdr: each test builds the command and
//! reads its argv and env.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use horch_core::harness::codex::codex_config_value;
use horch_core::harness::launch::{self, teammate_env, FORBIDDEN_ENV};
use horch_core::harness::{HarnessKind, LaunchEnv, Session};
use horch_core::roster::{HarnessDefault, Roster, Teammate};
use horch_core::runtime::{Inherited, MapEnv, Paths, RuntimeContext};
use serde_json::{json, Value};

/// The data file of the temp rosters: 1 Claude entry, 2 Codex entries (the
/// second only for fleet workers) and 1 OpenCode entry.
const DEFAULTS: &str = r#"---
name: harness-defaults
defaults:
  - harness: claude
    env:
      DISABLE_AUTOUPDATER: "1"
  - harness: codex
    args: ["-c", "tui.auto_recap=false"]
  - harness: codex
    base: fleet-worker
    args: ["-c", "notify=[]"]
    force: "test: the notify command is noise"
  - harness: opencode
    env:
      OPENCODE_DISABLE_EXTERNAL_SKILLS: "1"
    config: {"agent": {"title": {"disable": true}}}
---
"#;

fn teammate_file(name: &str, agent: &str, base: Option<&str>, extra: &str) -> String {
    let base = base.map(|b| format!("base: {b}\n")).unwrap_or_default();
    format!(
        "---\nname: {name}\nbrief_description: test teammate\nhidden: true\n{base}agent: {agent}\nmodel: m\n{extra}---\nbody\n"
    )
}

/// A teammates dir holding `_base/harness-defaults.md` (`defaults`) and the
/// given teammate files.
fn roster_dir(defaults: &str, files: &[(&str, String)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("_base")).unwrap();
    std::fs::write(dir.path().join("_base/harness-defaults.md"), defaults).unwrap();
    for (name, text) in files {
        std::fs::write(dir.path().join(format!("{name}.md")), text).unwrap();
    }
    dir
}

fn load(dir: &Path) -> Roster {
    Roster::load_layered(None, None, Some(dir.to_str().unwrap())).unwrap()
}

/// A launch environment with a temp home, a workdir and a managed-settings
/// path that does not exist: nothing of the machine's leaks in.
fn ctx_with(home: &Path, workdir: &Path, vars: &[(&str, &str)]) -> RuntimeContext {
    let mut env = MapEnv::new(workdir)
        .with("HOME", &home.to_string_lossy())
        .with("PATH", "/nonexistent")
        .with(
            "HORCH_CLAUDE_MANAGED_SETTINGS",
            &home.join("no-managed-settings.json").to_string_lossy(),
        );
    for (k, v) in vars {
        env = env.with(k, v);
    }
    RuntimeContext::from_env(&env).unwrap()
}

fn launch_env(home: &Path) -> LaunchEnv {
    LaunchEnv::from_context(&ctx_with(home, home, &[]))
}

fn build(env: &LaunchEnv, t: &Teammate) -> Command {
    launch::command_with_skills_in(env, t, Session::Unmanaged, "PROMPT", None, None).unwrap()
}

fn argv(cmd: &Command) -> Vec<String> {
    cmd.get_args()
        .map(|a| a.to_string_lossy().into_owned())
        .collect()
}

/// The env the child starts with: the teammate env layer, then the values
/// the builder set on the command, which win.
fn child_env(env: &LaunchEnv, t: &Teammate, cmd: &Command) -> BTreeMap<String, String> {
    let mut out: BTreeMap<String, String> = teammate_env(t, env).into_iter().collect();
    for (k, v) in cmd.get_envs() {
        let k = k.to_string_lossy().into_owned();
        match v {
            Some(v) => out.insert(k, v.to_string_lossy().into_owned()),
            None => out.remove(&k),
        };
    }
    out
}

/// The `-c` pairs of an argv, as a set.
fn config_pairs(args: &[String]) -> BTreeSet<String> {
    args.windows(2)
        .filter(|w| w[0] == "-c")
        .map(|w| w[1].clone())
        .collect()
}

/// The JSON of the `--settings` argument.
fn settings_arg(args: &[String]) -> Value {
    let i = args
        .iter()
        .position(|a| a == "--settings")
        .expect("--settings");
    serde_json::from_str(&args[i + 1]).unwrap()
}

fn entry(harness: HarnessKind) -> HarnessDefault {
    HarnessDefault {
        harness,
        ..HarnessDefault::default()
    }
}

fn teammate(name: &str, agent: HarnessKind, defaults: Vec<HarnessDefault>) -> Teammate {
    Teammate {
        name: name.into(),
        agent,
        model: Some("m".into()),
        harness_defaults: defaults,
        ..Teammate::default()
    }
}

// ─── HDF-01 ─────────────────────────────────────────────────────────────────

/// HDF-01: the roster attaches the entries whose base matches, for every
/// harness; the builder selects by harness, in file order.
#[test]
fn hdf_01_entries_attach_by_harness_and_base() {
    let dir = roster_dir(
        DEFAULTS,
        &[
            (
                "t-claude",
                teammate_file("t-claude", "claude", Some("fleet-worker"), ""),
            ),
            (
                "t-codex-w",
                teammate_file("t-codex-w", "codex", Some("fleet-worker"), ""),
            ),
            (
                "t-codex-o",
                teammate_file("t-codex-o", "codex", Some("fleet-orchestrator"), ""),
            ),
        ],
    );
    let roster = load(dir.path());
    let all = &roster.base("harness-defaults").unwrap().defaults;
    assert_eq!(all.len(), 4);
    let no_base: Vec<HarnessDefault> = all.iter().filter(|d| d.base.is_none()).cloned().collect();

    let claude = roster.require("t-claude").unwrap();
    let worker = roster.require("t-codex-w").unwrap();
    let other = roster.require("t-codex-o").unwrap();
    assert_eq!(
        &claude.harness_defaults, all,
        "a fleet worker gets every entry"
    );
    assert_eq!(&worker.harness_defaults, all);
    assert_eq!(
        other.harness_defaults, no_base,
        "another base: only the base-less entries"
    );

    let select = |t: &Teammate| -> Vec<HarnessDefault> {
        HarnessDefault::for_harness(&t.harness_defaults, t.agent)
            .cloned()
            .collect()
    };
    assert_eq!(select(claude), vec![all[0].clone()]);
    assert_eq!(select(worker), vec![all[1].clone(), all[2].clone()]);
    assert_eq!(select(other), vec![all[1].clone()]);
}

/// HDF-01 (finding 11): the Claude orchestrator teammate run on Codex (the
/// `OrchestrationCodex` pane, `cmd/recipes.rs`) gets the Codex defaults and
/// none of the Claude ones.
#[test]
fn hdf_01_defaults_follow_the_final_harness() {
    let home = tempfile::tempdir().unwrap();
    let env = launch_env(home.path());
    let mut t = Roster::builtin()
        .unwrap()
        .require("orchestrator")
        .unwrap()
        .clone();
    assert_eq!(t.agent, HarnessKind::Claude);
    t.agent = HarnessKind::Codex;
    t.effort = None;
    t.model = Some("gpt-5.6-sol".into());
    // Only the defaults are under test, not the file's own `env:`.
    t.env.clear();
    let cmd = build(&env, &t);
    let args = argv(&cmd);
    assert!(
        args.windows(2).any(|w| w == ["-c", "tui.auto_recap=false"]),
        "{args:?}"
    );
    let env = child_env(&env, &t, &cmd);
    assert!(!env.contains_key("DISABLE_AUTOUPDATER"), "{env:?}");
    assert!(
        !env.contains_key("CLAUDE_CODE_ENABLE_PROMPT_SUGGESTION"),
        "{env:?}"
    );
}

/// HDF-01: the brief carries the resolved teammate, entries included; a
/// teammate with none serializes without the key.
#[test]
fn hdf_01_brief_carries_harness_defaults() {
    let mut codex = entry(HarnessKind::Codex);
    codex.args = vec!["-c".into(), "tui.auto_recap=false".into()];
    codex.force = Some("reason".into());
    let t = teammate("t", HarnessKind::Codex, vec![codex]);
    let text = serde_json::to_string(&t).unwrap();
    let back: Teammate = serde_json::from_str(&text).unwrap();
    assert_eq!(back.harness_defaults, t.harness_defaults);

    let bare = teammate("t", HarnessKind::Codex, Vec::new());
    let value = serde_json::to_value(&bare).unwrap();
    assert!(value.get("harness_defaults").is_none(), "{value}");
}

// ─── HDF-02 ─────────────────────────────────────────────────────────────────

/// HDF-02: the teammate's own `env` value wins over the default.
#[test]
fn hdf_02_teammate_env_wins_over_default() {
    let mut claude = entry(HarnessKind::Claude);
    claude.env.insert("DISABLE_AUTOUPDATER".into(), "1".into());
    let mut t = teammate("t", HarnessKind::Claude, vec![claude]);
    let env = LaunchEnv::from_context(&ctx_with(Path::new("/h"), Path::new("/w"), &[]));
    let get = |t: &Teammate| {
        teammate_env(t, &env)
            .into_iter()
            .find(|(k, _)| k == "DISABLE_AUTOUPDATER")
            .map(|(_, v)| v)
    };
    assert_eq!(get(&t).as_deref(), Some("1"));
    t.env.insert("DISABLE_AUTOUPDATER".into(), "0".into());
    assert_eq!(get(&t).as_deref(), Some("0"));
}

/// HDF-02: a default `-c` pair the teammate's args set is left out; else it
/// comes before the teammate's args.
#[test]
fn hdf_02_codex_default_args_skip_a_key_the_teammate_sets() {
    let home = tempfile::tempdir().unwrap();
    let env = launch_env(home.path());
    let mut codex = entry(HarnessKind::Codex);
    codex.args = vec!["-c".into(), "tui.auto_recap=false".into()];
    let mut t = teammate("t", HarnessKind::Codex, vec![codex]);

    t.args = vec!["-c".into(), "tui.auto_recap=true".into()];
    let args = argv(&build(&env, &t));
    assert!(
        !args.contains(&"tui.auto_recap=false".to_string()),
        "{args:?}"
    );
    assert_eq!(
        args.iter()
            .filter(|a| a.starts_with("tui.auto_recap"))
            .count(),
        1
    );

    t.args = vec!["-c".into(), "features.multi_agent=false".into()];
    let args = argv(&build(&env, &t));
    let default = args.iter().position(|a| a == "tui.auto_recap=false");
    let own = args.iter().position(|a| a == "features.multi_agent=false");
    assert!(default.is_some() && default < own, "{args:?}");
}

/// HDF-02: a codex default yields to the operator's `config.toml`, unless
/// the entry has `force`.
#[test]
fn hdf_02_codex_default_yields_to_operator_config_unless_forced() {
    // The reader.
    let text = r##"
model = "gpt-5.6-sol" # a comment
model_auto_compact_token_limit = 200_000
profile = "p"
tui.theme = "dark"

[tui]
auto_recap = false

[profiles.p]
notify = ["say", "#not a comment"]

[profiles.q]
model = "q-model"
"##;
    assert_eq!(
        codex_config_value(text, None, "model").as_deref(),
        Some("gpt-5.6-sol")
    );
    assert_eq!(
        codex_config_value(text, None, "model_auto_compact_token_limit").as_deref(),
        Some("200000")
    );
    assert_eq!(
        codex_config_value(text, None, "tui.auto_recap").as_deref(),
        Some("false")
    );
    assert_eq!(
        codex_config_value(text, None, "tui.theme").as_deref(),
        Some("dark")
    );
    // The file's own profile.
    assert_eq!(
        codex_config_value(text, None, "notify").as_deref(),
        Some(r##"["say", "#not a comment"]"##)
    );
    // An explicit profile wins over the file's.
    assert_eq!(
        codex_config_value(text, Some("q"), "model").as_deref(),
        Some("q-model")
    );
    assert_eq!(codex_config_value(text, Some("q"), "notify"), None);
    assert_eq!(codex_config_value("# model = \"x\"\n", None, "model"), None);
    assert_eq!(codex_config_value(text, None, "absent"), None);

    // The launch.
    let home = tempfile::tempdir().unwrap();
    let codex_home = home.path().join("codex-home");
    std::fs::create_dir_all(&codex_home).unwrap();
    std::fs::write(
        codex_home.join("config.toml"),
        "notify = [\"say\"]\n[tui]\nauto_recap = true\n",
    )
    .unwrap();
    let env = LaunchEnv::from_context(&ctx_with(
        home.path(),
        home.path(),
        &[("CODEX_HOME", &codex_home.to_string_lossy())],
    ));
    let mut recap = entry(HarnessKind::Codex);
    recap.args = vec!["-c".into(), "tui.auto_recap=false".into()];
    let mut notify = entry(HarnessKind::Codex);
    notify.args = vec!["-c".into(), "notify=[]".into()];
    notify.force = Some("test".into());
    let t = teammate("t", HarnessKind::Codex, vec![recap, notify]);
    let args = argv(&build(&env, &t));
    assert!(
        !args.contains(&"tui.auto_recap=false".to_string()),
        "{args:?}"
    );
    assert!(
        args.windows(2).any(|w| w == ["-c", "notify=[]"]),
        "{args:?}"
    );
}

/// A Claude teammate with 1 settings default per key in `keys`.
fn claude_with_settings(keys: &[&str]) -> Teammate {
    let mut claude = entry(HarnessKind::Claude);
    let mut settings = serde_json::Map::new();
    for k in keys {
        settings.insert((*k).into(), json!("fleet"));
    }
    claude.settings = Some(settings);
    teammate("t", HarnessKind::Claude, vec![claude])
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

/// HDF-02: a Claude settings default is left out of the `--settings`
/// overlay when the operator's chain sets the key, in both overlay paths.
/// The chain's order is pinned by the claude.rs unit test
/// `hdf_02_claude_operator_value_follows_the_chain_order`.
#[test]
fn hdf_02_claude_settings_default_yields_to_operator_chain() {
    let home = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    write(
        &home.path().join(".claude/settings.json"),
        r#"{"byUser": 1}"#,
    );
    write(
        &work.path().join(".claude/settings.json"),
        r#"{"byProject": 1}"#,
    );
    write(
        &work.path().join(".claude/settings.local.json"),
        r#"{"byLocal": 1}"#,
    );
    let env = LaunchEnv::from_context(&ctx_with(home.path(), work.path(), &[]));
    let keys = ["byUser", "byProject", "byLocal", "free"];

    // The plain overlay: only the key no source sets.
    let t = claude_with_settings(&keys);
    let overlay = settings_arg(&argv(&build(&env, &t)));
    for k in ["byUser", "byProject", "byLocal"] {
        assert!(overlay.get(k).is_none(), "{k}: {overlay}");
    }
    assert_eq!(overlay["free"], "fleet", "{overlay}");

    // setting_sources [project]: the user and local files do not load.
    let mut t = claude_with_settings(&keys);
    t.setting_sources = Some(vec!["project".into()]);
    let overlay = settings_arg(&argv(&build(&env, &t)));
    assert_eq!(overlay["byUser"], "fleet", "{overlay}");
    assert_eq!(overlay["byLocal"], "fleet", "{overlay}");
    assert!(overlay.get("byProject").is_none(), "{overlay}");

    // The inline overlay (as the skills launch passes it): the teammate's
    // own keys win, and the defaults merge under them.
    let mut t = claude_with_settings(&keys);
    t.settings = Some(r#"{"free": "mine", "disableWorkflows": true}"#.into());
    let overlay = settings_arg(&argv(&build(&env, &t)));
    assert_eq!(overlay["free"], "mine", "{overlay}");
    assert_eq!(overlay["disableWorkflows"], true, "{overlay}");
    assert!(overlay.get("byUser").is_none(), "{overlay}");

    let mut t = claude_with_settings(&["free"]);
    t.settings = Some(r#"{"disableWorkflows": true}"#.into());
    let overlay = settings_arg(&argv(&build(&env, &t)));
    assert_eq!(overlay["free"], "fleet", "{overlay}");
}

/// HDF-02 (finding 5): the managed-settings path is a context path.
#[test]
fn hdf_02_managed_settings_path_comes_from_the_context() {
    let tmp = tempfile::tempdir().unwrap();
    let managed = tmp.path().join("m.json");
    let env = MapEnv::new("/w").with("HORCH_CLAUDE_MANAGED_SETTINGS", &managed.to_string_lossy());
    assert_eq!(Paths::from_env(&env).claude_managed_settings, managed);
    let platform = if cfg!(target_os = "macos") {
        "/Library/Application Support/ClaudeCode/managed-settings.json"
    } else if cfg!(windows) {
        r"C:\Program Files\ClaudeCode\managed-settings.json"
    } else {
        "/etc/claude-code/managed-settings.json"
    };
    assert_eq!(
        Paths::from_env(&MapEnv::new("/w")).claude_managed_settings,
        PathBuf::from(platform)
    );

    write(&managed, r#"{"byManaged": {"deep": 1}}"#);
    let ctx = RuntimeContext::from_env(
        &MapEnv::new(tmp.path())
            .with("HOME", &tmp.path().to_string_lossy())
            .with("HORCH_CLAUDE_MANAGED_SETTINGS", &managed.to_string_lossy()),
    )
    .unwrap();
    let env = LaunchEnv::from_context(&ctx);
    let mut claude = entry(HarnessKind::Claude);
    claude.settings = Some(
        json!({"byManaged": {"deep": 2, "other": 3}})
            .as_object()
            .unwrap()
            .clone(),
    );
    let t = teammate("t", HarnessKind::Claude, vec![claude]);
    let overlay = settings_arg(&argv(&build(&env, &t)));
    assert_eq!(overlay["byManaged"], json!({"other": 3}), "{overlay}");
}

/// HDF-02 (finding 6): `Inherited` keeps only named variables; a forbidden
/// name is never captured.
#[test]
fn hdf_02_inherited_env_is_an_allow_list() {
    let mut env = MapEnv::new("/w")
        .with("RLM_MAX_DEPTH", "rlm-value-7")
        .with("FOO", "foo-value-9");
    for (i, name) in FORBIDDEN_ENV.iter().enumerate() {
        env = env.with(name, &format!("forbidden-value-{i}"));
    }
    env = env.with("ANTHROPIC_API_KEY", "forbidden-value-key");
    let inherited = Inherited::from_env(&env);
    let debug = format!("{inherited:?}");
    assert!(debug.contains("rlm-value-7"), "{debug}");
    assert!(!debug.contains("forbidden-value"), "{debug}");
    assert!(!debug.contains("foo-value-9"), "{debug}");
    assert_eq!(inherited.operator_env("RLM_MAX_DEPTH"), Some("rlm-value-7"));
    assert_eq!(inherited.operator_env("FOO"), None);
    assert_eq!(inherited.operator_env("ANTHROPIC_API_KEY"), None);
    for name in Inherited::DEFAULT_ENV_YIELD {
        assert!(!FORBIDDEN_ENV.contains(&name), "{name}");
    }

    // A non-Claude default yields to the operator's value.
    let mut prime = entry(HarnessKind::Prime);
    prime.env.insert("RLM_MAX_DEPTH".into(), "1".into());
    let t = teammate("t", HarnessKind::Prime, vec![prime]);
    let with = LaunchEnv::from_context(&RuntimeContext::from_env(&env).unwrap());
    assert!(teammate_env(&t, &with)
        .iter()
        .all(|(k, _)| k != "RLM_MAX_DEPTH"));
    let without = LaunchEnv::from_context(&RuntimeContext::from_env(&MapEnv::new("/w")).unwrap());
    assert!(teammate_env(&t, &without).contains(&("RLM_MAX_DEPTH".into(), "1".into())));
}

/// HDF-02: the OpenCode `config` default merges under the teammate's JSON.
#[test]
fn hdf_02_opencode_config_default_merges_under_teammate_json() {
    let home = tempfile::tempdir().unwrap();
    let env = launch_env(home.path());
    let mut opencode = entry(HarnessKind::OpenCode);
    opencode.config = Some(
        json!({"agent": {"title": {"disable": true}}})
            .as_object()
            .unwrap()
            .clone(),
    );
    let config = |own: &str| {
        let mut t = teammate("t", HarnessKind::OpenCode, vec![opencode.clone()]);
        t.env.insert("OPENCODE_CONFIG_CONTENT".into(), own.into());
        let cmd = build(&env, &t);
        let text = child_env(&env, &t, &cmd)["OPENCODE_CONFIG_CONTENT"].clone();
        serde_json::from_str::<Value>(&text).unwrap()
    };
    assert_eq!(
        config(r#"{"agent":{"build":{"x":1}}}"#),
        json!({"agent": {"build": {"x": 1}, "title": {"disable": true}}})
    );
    assert_eq!(
        config(r#"{"agent":{"title":{"disable":false}}}"#),
        json!({"agent": {"title": {"disable": false}}})
    );
}

// ─── HDF-03 ─────────────────────────────────────────────────────────────────

fn problems_with(roster: &Roster, needle: &str) -> Vec<String> {
    roster
        .check()
        .into_iter()
        .filter(|p| p.contains(needle))
        .collect()
}

/// HDF-03: a teammate line that only repeats a default of its own harness
/// fails the check; a different value is the override.
#[test]
fn hdf_03_repeat_of_a_default_fails_the_check() {
    let dir = roster_dir(
        DEFAULTS,
        &[
            (
                "t-same",
                teammate_file(
                    "t-same",
                    "claude",
                    None,
                    "env:\n  DISABLE_AUTOUPDATER: \"1\"\n",
                ),
            ),
            (
                "t-other",
                teammate_file(
                    "t-other",
                    "claude",
                    None,
                    "env:\n  DISABLE_AUTOUPDATER: \"0\"\n",
                ),
            ),
            (
                "t-args",
                teammate_file(
                    "t-args",
                    "codex",
                    None,
                    "args: [\"-c\", \"tui.auto_recap=false\"]\n",
                ),
            ),
        ],
    );
    let roster = load(dir.path());
    let found = problems_with(&roster, "repeats");
    let mine: Vec<&String> = found.iter().filter(|p| p.starts_with("t-")).collect();
    assert_eq!(mine.len(), 2, "{found:?}");
    assert!(mine.iter().any(|p| p.starts_with("t-same:")
        && p.contains("DISABLE_AUTOUPDATER")
        && p.contains("claude harness default")));
    assert!(mine
        .iter()
        .any(|p| p.starts_with("t-args:") && p.contains("tui.auto_recap=false")));
}

/// HDF-03: no built-in teammate repeats a default (W2's lines moved).
#[test]
fn hdf_03_builtin_roster_has_no_repeats() {
    let roster = Roster::builtin().unwrap();
    let found = problems_with(&roster, "repeats");
    assert!(found.is_empty(), "{found:?}");
}

/// HDF-03: a malformed entry, and a teammate file that sets
/// `harness_defaults`, each fail the check once.
#[test]
fn hdf_03_malformed_entry_fails_the_check() {
    let defaults = r#"---
name: harness-defaults
defaults:
  - harness: codex
    settings: {"a": 1}
  - harness: codex
    args: ["-c", "x=1"]
    force: ""
  - harness: claude
    env:
      ANTHROPIC_API_KEY: "x"
  - harness: codex
    args: ["-c"]
---
"#;
    let dir = roster_dir(
        defaults,
        &[(
            "t-sets",
            teammate_file(
                "t-sets",
                "codex",
                None,
                "harness_defaults:\n  - harness: codex\n    args: [\"-c\", \"a=b\"]\n",
            ),
        )],
    );
    let roster = load(dir.path());
    for (needle, who) in [
        ("settings is only for harness claude", "entry 1"),
        ("force must give the reason", "entry 2"),
        ("env ANTHROPIC_API_KEY is forbidden", "entry 3"),
        ("-c needs a <key>=<value>", "entry 4"),
    ] {
        let found = problems_with(&roster, needle);
        assert_eq!(found.len(), 1, "{needle}: {found:?}");
        assert!(found[0].contains(who), "{found:?}");
    }
    let found = problems_with(&roster, "set by _base/harness-defaults.md");
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].starts_with("t-sets:"), "{found:?}");
}

// ─── HDF-04 ─────────────────────────────────────────────────────────────────

/// HDF-04: after W2's lines moved into the defaults, each harness launches
/// with the env and the `-c` pairs W2 wrote into the teammate files
/// (148eb16, 2a48638, bd61495), hard-coded here.
#[test]
fn hdf_04_migrated_launch_env_and_args_match_w2() {
    let home = tempfile::tempdir().unwrap();
    let env = launch_env(home.path());
    let roster = Roster::builtin().unwrap();
    let claude_env = [
        ("CLAUDE_CODE_ENABLE_PROMPT_SUGGESTION", "false"),
        ("DISABLE_AUTOUPDATER", "1"),
    ];
    let codex_worker = [
        "check_for_update_on_startup=false",
        "features.multi_agent=false",
        "model=\"gpt-5.6-sol\"",
        "model_reasoning_effort=\"medium\"",
        "notify=[]",
        "tui.auto_recap=false",
        "tui.resume_cwd=\"current\"",
    ];
    let codex_orchestrator = [
        "check_for_update_on_startup=false",
        "features.multi_agent=false",
        "model=\"gpt-6-astra\"",
        "model_reasoning_effort=\"xhigh\"",
        "tui.auto_recap=false",
        "tui.resume_cwd=\"current\"",
    ];
    let launch = |name: &str| {
        let t = roster.require(name).unwrap();
        let cmd = build(&env, t);
        (child_env(&env, t, &cmd), config_pairs(&argv(&cmd)))
    };
    for name in ["opus", "sonnet", "orchestrator"] {
        let (got, _) = launch(name);
        for (k, v) in claude_env {
            assert_eq!(got.get(k).map(String::as_str), Some(v), "{name}: {k}");
        }
    }
    for (name, want) in [
        ("codex-sol", &codex_worker[..]),
        ("orchestrator-codex", &codex_orchestrator[..]),
    ] {
        let (_, pairs) = launch(name);
        let want: BTreeSet<String> = want.iter().map(|s| s.to_string()).collect();
        assert_eq!(pairs, want, "{name}");
    }
    let (got, _) = launch("opencode-pickle");
    assert_eq!(got["OPENCODE_DISABLE_EXTERNAL_SKILLS"], "1");
    let config: Value = serde_json::from_str(&got["OPENCODE_CONFIG_CONTENT"]).unwrap();
    assert_eq!(config, json!({"agent": {"title": {"disable": true}}}));
    let (got, _) = launch("prime");
    assert_eq!(got["RLM_MAX_DEPTH"], "1");
}
