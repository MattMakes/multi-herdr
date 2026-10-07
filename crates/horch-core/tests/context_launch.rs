//! The launch's native auto-compact window (CTX-05, design
//! `ai_docs/plans/wave2/w1/design.md` §6.4 and §6.5): the operator's value
//! wins, else the fleet default is applied, and the launch records what it
//! decided. Temp homes with hand-written settings files; no real CLI and no
//! machine settings (the managed-settings path is a temp path).

use std::path::{Path, PathBuf};
use std::process::Command;

use horch_core::compaction::window::{WindowDecision, WindowSource};
use horch_core::execution::legacy::{LedgerRecordV1, LedgerWindow, LedgerWindowSource};
use horch_core::execution::store::ExecutionStore;
use horch_core::harness::launch::{self, window_decision, window_in_effect};
use horch_core::harness::{HarnessKind, LaunchEnv, Session, WindowInputs};
use horch_core::roster::{HarnessDefault, Phase, Roster, Teammate};
use horch_core::runtime::{MapEnv, RuntimeContext};
use serde_json::{json, Value};

const WINDOW_KEY: &str = "CLAUDE_CODE_AUTO_COMPACT_WINDOW";
const CODEX_KEY: &str = "model_auto_compact_token_limit";

/// A temp home, a workdir under it and a managed-settings path in it.
struct World {
    _tmp: tempfile::TempDir,
    home: PathBuf,
    workdir: PathBuf,
    managed: PathBuf,
}

impl World {
    fn new() -> World {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let workdir = tmp.path().join("project");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&workdir).unwrap();
        let managed = tmp.path().join("managed-settings.json");
        World {
            _tmp: tmp,
            home,
            workdir,
            managed,
        }
    }

    fn ctx(&self, vars: &[(&str, &str)]) -> RuntimeContext {
        let mut env = MapEnv::new(&self.workdir)
            .with("HOME", &self.home.to_string_lossy())
            .with("PATH", "/nonexistent")
            .with(
                "HORCH_CLAUDE_MANAGED_SETTINGS",
                &self.managed.to_string_lossy(),
            )
            .with(
                "HORCH_STATE_DIR",
                &self.home.join("state").to_string_lossy(),
            );
        for (k, v) in vars {
            env = env.with(k, v);
        }
        RuntimeContext::from_env(&env).unwrap()
    }

    fn write(&self, path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn user_settings(&self) -> PathBuf {
        self.home.join(".claude/settings.json")
    }

    fn codex_config(&self) -> PathBuf {
        self.home.join(".codex/config.toml")
    }
}

/// A settings file whose `env` sets the window to `value`.
fn window_file(value: Value) -> String {
    json!({"env": {WINDOW_KEY: value}}).to_string()
}

fn claude_teammate(sources: Option<&[&str]>) -> Teammate {
    Teammate {
        name: "t".into(),
        agent: HarnessKind::Claude,
        model: Some("opus".into()),
        setting_sources: sources.map(|s| s.iter().map(|x| x.to_string()).collect()),
        ..Teammate::default()
    }
}

/// The Claude adapter's operator window for `t` in `world`, with
/// `process` as the inherited `CLAUDE_CODE_AUTO_COMPACT_WINDOW`.
fn claude_operator(
    world: &World,
    t: &Teammate,
    process: Option<&str>,
) -> Option<(Option<u64>, String)> {
    let codex_home = world.home.join(".codex");
    let prime = world.home.join(".prime/agent");
    let inputs = WindowInputs {
        home: &world.home,
        claude_config_dir: None,
        claude_managed_settings: &world.managed,
        codex_home: &codex_home,
        prime_agent_dir: &prime,
        workdir: &world.workdir,
        process_window: process,
        teammate: t,
        model: "opus",
    };
    HarnessKind::Claude
        .adapter()
        .operator_window(&inputs)
        .map(|w| (w.tokens, w.detail))
}

fn argv(cmd: &Command) -> Vec<String> {
    cmd.get_args()
        .map(|a| a.to_string_lossy().into_owned())
        .collect()
}

/// The JSON of the `--settings` argument, or `None` without one.
fn settings_arg(args: &[String]) -> Option<Value> {
    let i = args.iter().position(|a| a == "--settings")?;
    serde_json::from_str(&args[i + 1]).ok()
}

fn window_in_settings(args: &[String]) -> Option<Value> {
    settings_arg(args)?.get("env")?.get(WINDOW_KEY).cloned()
}

fn codex_limit_flag(args: &[String]) -> Option<String> {
    args.windows(2).filter(|w| w[0] == "-c").find_map(|w| {
        w[1].strip_prefix(&format!("{CODEX_KEY}="))
            .map(str::to_string)
    })
}

/// The launch path of `run_flow_code` without the CLI: decide, build with
/// the decision on the launch env, then read the command back (step 3).
fn launch_with(
    ctx: &RuntimeContext,
    roster: &Roster,
    t: &Teammate,
    session: Session<'_>,
    bundle: Option<&horch_core::skills::Bundle>,
) -> (WindowDecision, Vec<String>) {
    let model = t.model.clone().unwrap();
    let fleet = roster.fleet_window(t, &model);
    let workdir = ctx.paths.cwd.clone().unwrap();
    let decision = window_decision(ctx, t, &model, &workdir, fleet);
    let mut env = LaunchEnv::from_context(ctx);
    env.compact_window = Some(decision.clone());
    let cmd = launch::command_with_skills_in(&env, t, session, "PROMPT", None, bundle).unwrap();
    (window_in_effect(decision, t, &cmd), argv(&cmd))
}

// ─── CTX-05 ─────────────────────────────────────────────────────────────────

#[test]
fn ctx_05_claude_settings_chain_precedence() {
    let all = claude_teammate(None);

    // User file only.
    let w = World::new();
    w.write(&w.user_settings(), &window_file(json!("500000")));
    let (tokens, detail) = claude_operator(&w, &all, None).unwrap();
    assert_eq!(tokens, Some(500_000));
    assert!(
        detail.contains(&*w.user_settings().to_string_lossy()),
        "{detail}"
    );

    // Project beats user.
    w.write(
        &w.workdir.join(".claude/settings.json"),
        &window_file(json!(250000)),
    );
    assert_eq!(claude_operator(&w, &all, None).unwrap().0, Some(250_000));
    // Local beats project.
    w.write(
        &w.workdir.join(".claude/settings.local.json"),
        &window_file(json!("240000")),
    );
    assert_eq!(claude_operator(&w, &all, None).unwrap().0, Some(240_000));
    // Managed (the context's temp path) beats all.
    w.write(&w.managed, &window_file(json!("230000")));
    assert_eq!(claude_operator(&w, &all, None).unwrap().0, Some(230_000));

    // `setting_sources: ["project"]` ignores the user file.
    let w = World::new();
    w.write(&w.user_settings(), &window_file(json!("500000")));
    let project_only = claude_teammate(Some(&["project"]));
    assert_eq!(claude_operator(&w, &project_only, None), None);
    w.write(
        &w.workdir.join(".claude/settings.json"),
        &window_file(json!("250000")),
    );
    assert_eq!(
        claude_operator(&w, &project_only, None).unwrap().0,
        Some(250_000)
    );

    // The process env counts only when no file sets the window.
    let w = World::new();
    let (tokens, _) = claude_operator(&w, &all, Some("400000")).unwrap();
    assert_eq!(tokens, Some(400_000));
    w.write(&w.user_settings(), &window_file(json!("500000")));
    assert_eq!(
        claude_operator(&w, &all, Some("400000")).unwrap().0,
        Some(500_000)
    );

    // A percentage override in the same file: set, but the trigger is unknown.
    let w = World::new();
    w.write(
        &w.user_settings(),
        &json!({"env": {WINDOW_KEY: "500000", "CLAUDE_AUTOCOMPACT_PCT_OVERRIDE": "70"}})
            .to_string(),
    );
    let (tokens, detail) = claude_operator(&w, &all, None).unwrap();
    assert_eq!(tokens, None);
    assert_eq!(
        detail,
        format!(
            "{} (CLAUDE_AUTOCOMPACT_PCT_OVERRIDE set; trigger unknown)",
            w.user_settings().display()
        )
    );

    // Not a number.
    let w = World::new();
    w.write(&w.user_settings(), &window_file(json!("abc")));
    let (tokens, detail) = claude_operator(&w, &all, None).unwrap();
    assert_eq!(tokens, None);
    assert!(detail.ends_with("(not a number)"), "{detail}");

    // Nothing anywhere.
    assert_eq!(claude_operator(&World::new(), &all, None), None);
}

#[test]
fn ctx_05_claude_overlay_gets_fleet_window_only_without_operator_value() {
    let roster = Roster::builtin().unwrap();
    let opus = roster.require("opus").unwrap().clone();
    let mut skilled = opus.clone();
    skilled.phase = Some(Phase::Research);

    // Empty home: the fleet window, in the plain path and the skills path.
    let w = World::new();
    let ctx = w.ctx(&[]);
    let (d, args) = launch_with(&ctx, &roster, &opus, Session::Fresh("sid"), None);
    assert_eq!(window_in_settings(&args), Some(json!("200000")), "{args:?}");
    assert_eq!(
        d,
        WindowDecision {
            tokens: Some(200_000),
            source: WindowSource::Fleet,
            detail: "context-policy windows claude/opus".into(),
            applied: true,
        }
    );
    let bundle = horch_core::skills::Bundle::install(&w.home.join("state"), &skilled)
        .unwrap()
        .unwrap();
    let (d, args) = launch_with(
        &ctx,
        &roster,
        &skilled,
        Session::Fresh("sid"),
        Some(&bundle),
    );
    assert!(args.contains(&"--plugin-dir".to_string()), "{args:?}");
    assert_eq!(window_in_settings(&args), Some(json!("200000")), "{args:?}");
    assert!(d.applied);

    // An operator value: no key, and the decision names the user file.
    let w = World::new();
    w.write(&w.user_settings(), &window_file(json!("500000")));
    let ctx = w.ctx(&[]);
    let (d, args) = launch_with(&ctx, &roster, &opus, Session::Fresh("sid"), None);
    assert_eq!(window_in_settings(&args), None, "{args:?}");
    assert_eq!(
        d,
        WindowDecision {
            tokens: Some(500_000),
            source: WindowSource::Operator,
            detail: w.user_settings().to_string_lossy().into_owned(),
            applied: false,
        }
    );

    // A harness-default `env` key in the overlay keeps its value.
    let w = World::new();
    let ctx = w.ctx(&[]);
    let mut with_default = opus.clone();
    let mut settings = serde_json::Map::new();
    settings.insert("env".into(), json!({"OTHER_KEY": "1"}));
    with_default.harness_defaults.push(HarnessDefault {
        harness: HarnessKind::Claude,
        settings: Some(settings),
        ..HarnessDefault::default()
    });
    let (d, args) = launch_with(&ctx, &roster, &with_default, Session::Fresh("sid"), None);
    let overlay = settings_arg(&args).unwrap();
    assert_eq!(overlay["env"]["OTHER_KEY"], json!("1"), "{overlay}");
    assert_eq!(overlay["env"][WINDOW_KEY], json!("200000"), "{overlay}");
    assert!(d.applied);
}

#[test]
fn ctx_05_codex_operator_limit_from_profile_or_top_level() {
    let limit = |config: Option<&str>, args: &[&str]| {
        let w = World::new();
        if let Some(text) = config {
            w.write(&w.codex_config(), text);
        }
        let t = Teammate {
            name: "c".into(),
            agent: HarnessKind::Codex,
            model: Some("gpt-5.6-sol".into()),
            args: args.iter().map(|a| a.to_string()).collect(),
            ..Teammate::default()
        };
        let codex_home = w.home.join(".codex");
        let prime = w.home.join(".prime/agent");
        let inputs = WindowInputs {
            home: &w.home,
            claude_config_dir: None,
            claude_managed_settings: &w.managed,
            codex_home: &codex_home,
            prime_agent_dir: &prime,
            workdir: &w.workdir,
            process_window: None,
            teammate: &t,
            model: "gpt-5.6-sol",
        };
        let got = HarnessKind::Codex.adapter().operator_window(&inputs);
        if let Some(op) = &got {
            assert!(op.detail.contains("config.toml"), "{}", op.detail);
        }
        got.map(|op| op.tokens)
    };
    assert_eq!(
        limit(Some("model_auto_compact_token_limit = 180_000\n"), &[]),
        Some(Some(180_000))
    );
    assert_eq!(
        limit(
            Some("profile = \"p\"\n[profiles.p]\nmodel_auto_compact_token_limit = 170000\n"),
            &[]
        ),
        Some(Some(170_000))
    );
    assert_eq!(
        limit(
            Some("[profiles.q]\nmodel_auto_compact_token_limit = 160000\n"),
            &["-p", "q"]
        ),
        Some(Some(160_000))
    );
    assert_eq!(limit(Some("model = \"x\"\n"), &[]), None);
    assert_eq!(limit(None, &[]), None);
}

#[test]
fn ctx_05_codex_fleet_limit_flag_only_without_operator_value() {
    let roster = Roster::builtin().unwrap();
    let sol = roster.require("codex-sol").unwrap().clone();

    let w = World::new();
    w.write(&w.codex_config(), "model = \"x\"\n");
    let ctx = w.ctx(&[]);
    for session in [Session::Unmanaged, Session::Resume("sid")] {
        let (d, args) = launch_with(&ctx, &roster, &sol, session, None);
        assert_eq!(
            codex_limit_flag(&args).as_deref(),
            Some("200000"),
            "{args:?}"
        );
        let flag = args
            .iter()
            .position(|a| a == &format!("{CODEX_KEY}=200000"))
            .unwrap();
        assert!(flag < args.len() - 1, "the flag goes before the prompt");
        assert_eq!(args.last().unwrap(), "PROMPT");
        assert!(d.applied);
        assert_eq!(d.source, WindowSource::Fleet);
        println!("codex-sol argv: {args:?}");
    }

    let w = World::new();
    w.write(
        &w.codex_config(),
        "model_auto_compact_token_limit = 300000\n",
    );
    let ctx = w.ctx(&[]);
    let (d, args) = launch_with(&ctx, &roster, &sol, Session::Unmanaged, None);
    assert_eq!(codex_limit_flag(&args), None, "{args:?}");
    assert_eq!(
        (d.tokens, d.source, d.applied),
        (Some(300_000), WindowSource::Operator, false)
    );

    let luna = roster.require("codex-luna").unwrap().clone();
    let w = World::new();
    let ctx = w.ctx(&[]);
    let (d, args) = launch_with(&ctx, &roster, &luna, Session::Unmanaged, None);
    assert_eq!(codex_limit_flag(&args), None, "{args:?}");
    assert_eq!(d.source, WindowSource::Harness);
    assert!(!d.applied);
}

/// The wire shape of design §6.5.
const GOLDEN: &str = r#"{"tokens":200000,"source":"fleet","detail":"context-policy windows codex/gpt-5.6-sol","applied":true}"#;

#[test]
fn ctx_05_launch_records_window_decision() {
    let golden = LedgerWindow {
        tokens: Some(200_000),
        source: LedgerWindowSource::Fleet,
        detail: "context-policy windows codex/gpt-5.6-sol".into(),
        applied: true,
    };
    assert_eq!(serde_json::to_string(&golden).unwrap(), GOLDEN);
    assert_eq!(
        serde_json::from_str::<LedgerWindow>(GOLDEN).unwrap(),
        golden
    );

    // Through the store.
    let tmp = tempfile::tempdir().unwrap();
    let store = ExecutionStore::for_project(tmp.path(), "/p");
    store
        .insert(LedgerRecordV1 {
            record_id: "rec-1".into(),
            role: "codex-sol-1".into(),
            ..LedgerRecordV1::default()
        })
        .unwrap();
    assert_eq!(store.get("rec-1").unwrap().compact_window, None);
    store.set_compact_window("rec-1", &golden).unwrap();
    assert_eq!(
        store.get("rec-1").unwrap().compact_window,
        Some(golden.clone())
    );
    let text = std::fs::read_to_string(store.path()).unwrap();
    assert!(text.contains("\"compact_window\""), "{text}");

    // The store boundary converts each source both ways.
    for source in [
        WindowSource::Operator,
        WindowSource::Fleet,
        WindowSource::Harness,
    ] {
        for (tokens, applied) in [(Some(150_000), true), (None, false)] {
            let d = WindowDecision {
                tokens,
                source,
                detail: "d".into(),
                applied,
            };
            let stored = LedgerWindow::from(&d);
            assert_eq!(WindowDecision::from_ledger(&stored), Some(d));
        }
    }
}

#[test]
fn ctx_05_old_ledger_loads() {
    let oracles = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/oracles/ledgers");
    let tmp = tempfile::tempdir().unwrap();
    let store = ExecutionStore::for_project(tmp.path(), "/oracle/project");
    std::fs::copy(oracles.join("orchestrator.json"), store.path()).unwrap();
    let records = store.read().unwrap();
    assert!(records.iter().all(|r| r.compact_window.is_none()));
    let saved = ExecutionStore::render_json(&records).unwrap();
    let want = std::fs::read_to_string(oracles.join("orchestrator.saved.json")).unwrap();
    assert_eq!(saved, want, "an old ledger re-serializes byte-identical");

    // A newer binary's source loads as Unknown and resolves now.
    let later: Value = json!([{
        "record_id": "rec-later", "session_id": null, "agent": "codex",
        "tier": "codex-sol", "model": "gpt-5.6-sol", "role": "codex-sol-1",
        "status": "working", "task": "t", "history": [],
        "created_at": "2026-10-06T00:00:00Z", "updated_at": "2026-10-06T00:00:00Z",
        "compact_window": {"tokens": 1, "source": "later", "detail": "x", "applied": true}
    }]);
    std::fs::write(store.path(), later.to_string()).unwrap();
    let record = store.get("rec-later").unwrap();
    let stored = record.compact_window.unwrap();
    assert_eq!(stored.source, LedgerWindowSource::Unknown);
    assert_eq!(WindowDecision::from_ledger(&stored), None);
}

#[test]
fn ctx_05_recorded_applied_matches_the_command() {
    let roster = Roster::builtin().unwrap();
    let opus = roster.require("opus").unwrap().clone();

    // Inline `settings:` on the plain path: the teammate owns the overlay.
    let mut inline = opus.clone();
    inline.settings = Some(r#"{"theme":"dark"}"#.into());
    let w = World::new();
    let ctx = w.ctx(&[]);
    let (d, args) = launch_with(&ctx, &roster, &inline, Session::Fresh("sid"), None);
    assert_eq!(window_in_settings(&args), None, "{args:?}");
    assert_eq!(
        (d.applied, d.detail.as_str()),
        (
            false,
            "not applied: teammate inline settings own the overlay"
        )
    );

    // The record stores what the command carries.
    let store = ExecutionStore::open_in(&ctx).unwrap();
    store
        .insert(LedgerRecordV1 {
            record_id: "rec-opus".into(),
            role: "opus-1".into(),
            ..LedgerRecordV1::default()
        })
        .unwrap();
    store
        .set_compact_window("rec-opus", &LedgerWindow::from(&d))
        .unwrap();
    let stored = store.get("rec-opus").unwrap().compact_window.unwrap();
    assert!(!stored.applied);

    // Without `settings:`: applied, and the key is in the argv.
    let (d, args) = launch_with(&ctx, &roster, &opus, Session::Fresh("sid"), None);
    assert!(d.applied);
    assert_eq!(window_in_settings(&args), Some(json!("200000")));

    // A settings file: never applied.
    let mut file = opus.clone();
    let path = w.home.join("mine.json");
    w.write(&path, "{}");
    file.settings = Some(path.to_string_lossy().into_owned());
    let (d, args) = launch_with(&ctx, &roster, &file, Session::Fresh("sid"), None);
    assert_eq!(window_in_settings(&args), None, "{args:?}");
    assert_eq!(
        (d.applied, d.detail.as_str()),
        (false, "not applied: teammate settings file")
    );

    // Codex: applied equals whether the argv has the flag.
    let sol = roster.require("codex-sol").unwrap().clone();
    for config in [None, Some("model_auto_compact_token_limit = 300000\n")] {
        let w = World::new();
        if let Some(text) = config {
            w.write(&w.codex_config(), text);
        }
        let ctx = w.ctx(&[]);
        let (d, args) = launch_with(&ctx, &roster, &sol, Session::Unmanaged, None);
        assert_eq!(d.applied, codex_limit_flag(&args).is_some(), "{args:?}");
    }
}
