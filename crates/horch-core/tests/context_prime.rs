//! Prime's fleet agent dir (CTX-05, design `ai_docs/plans/wave2/w1/design.md`
//! §6.4 Prime rules and §6.9): each launch gets its own agent dir that links
//! the operator's entries, carries the fleet window in a `models.json` with
//! only the override, and yields to every value the operator sets. A temp
//! home with a fake `~/.prime/agent/` tree; no real Prime.

use std::path::{Path, PathBuf};

use horch_core::compaction::window::{WindowDecision, WindowSource};
use horch_core::execution::legacy::LedgerRecordV1;
use horch_core::execution::store::ExecutionStore;
use horch_core::execution::SessionMode;
use horch_core::harness::launch::{self, window_decision, window_in_effect, DiscoveryTarget};
use horch_core::harness::{CommandSpec, HarnessKind, LaunchEnv, PrepareRequest, Prepared, Session};
use horch_core::ids::SessionId;
use horch_core::messaging::mailbox::Mailbox;
use horch_core::roster::{Roster, Teammate};
use horch_core::runtime::{MapEnv, RuntimeContext};
use serde_json::{json, Value};

const MODEL: &str = "anthropic/claude-opus-5-5";
const AGENT_DIR_ENV: &str = "PRIME_AGENT_CODING_AGENT_DIR";
/// The fake credential: it must never appear in a regular file of the
/// fleet agent dir.
const TOKEN: &str = "sk-ant-oat-FAKE-TOKEN-u9";

/// A temp home with a fake Prime agent dir, a workdir and a state root.
struct World {
    tmp: tempfile::TempDir,
    home: PathBuf,
    workdir: PathBuf,
}

impl World {
    fn new() -> World {
        // A short path: the daemon socket path of a launch must stay within
        // 103 bytes, and the default temp dir of macOS is long.
        let tmp = tempfile::Builder::new()
            .prefix("hp")
            .tempdir_in("/tmp")
            .unwrap();
        let home = tmp.path().join("home");
        let workdir = tmp.path().join("project");
        std::fs::create_dir_all(&workdir).unwrap();
        let w = World { tmp, home, workdir };
        w.fake_agent_dir(&w.source());
        w
    }

    /// `~/.prime/agent` with the entries of this Mac (design §2.4).
    fn source(&self) -> PathBuf {
        self.home.join(".prime/agent")
    }

    fn fake_agent_dir(&self, dir: &Path) {
        std::fs::create_dir_all(dir.join("kernel-venv/bin")).unwrap();
        let auth = json!({"anthropic": {"type": "oauth", "access": TOKEN}});
        write(&dir.join("auth.json"), &auth.to_string());
        write(&dir.join("telemetry.json"), r#"{"id":"t"}"#);
        write(
            &dir.join("settings.json"),
            r#"{"telemetry":{"noticeShown":true}}"#,
        );
        write(
            &dir.join("models.json"),
            r#"{"providers":{"ollama":{"baseUrl":"http://localhost:11434/v1","models":[{"id":"qwen3.8"}]}}}"#,
        );
    }

    fn ctx(&self, vars: &[(&str, &str)]) -> RuntimeContext {
        let mut env = MapEnv::new(&self.workdir)
            .with("HOME", &self.home.to_string_lossy())
            .with("PATH", "/nonexistent")
            .with(
                "HORCH_CLAUDE_MANAGED_SETTINGS",
                &self.tmp.path().join("managed.json").to_string_lossy(),
            )
            .with("HORCH_STATE_DIR", &self.state().to_string_lossy())
            .with(
                "HORCH_DATA_DIR",
                &self.tmp.path().join("data").to_string_lossy(),
            )
            .with("HORCH_PRIME_BIN", &self.fake_prime().to_string_lossy());
        for (k, v) in vars {
            env = env.with(k, v);
        }
        RuntimeContext::from_env(&env).unwrap()
    }

    fn state(&self) -> PathBuf {
        self.tmp.path().join("state")
    }

    /// A `prime-agent` that exits 0 and prints nothing.
    fn fake_prime(&self) -> PathBuf {
        self.tmp.path().join("prime-agent")
    }
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn read_json(path: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn prime_teammate() -> (Roster, Teammate) {
    let roster = Roster::builtin().unwrap();
    let t = roster.require("prime").unwrap().clone();
    (roster, t)
}

/// The decision `run_flow_code` makes before `prepare`.
fn decide(w: &World, ctx: &RuntimeContext) -> (Teammate, WindowDecision) {
    let (roster, t) = prime_teammate();
    let fleet = roster.fleet_window(&t, MODEL);
    let decision = window_decision(ctx, &t, MODEL, &w.workdir, fleet);
    (t, decision)
}

fn prepare(w: &World, ctx: &RuntimeContext, t: &Teammate, decision: &WindowDecision) -> Prepared {
    HarnessKind::Prime
        .adapter()
        .prepare(
            ctx,
            &PrepareRequest {
                role: "prime-1",
                exec_rules: &[],
                skills: None,
                compact_window: Some(decision),
                teammate: t,
                model: MODEL,
                workdir: &w.workdir,
                shell_env: &[],
            },
        )
        .unwrap()
}

fn agent_dir(prepared: &Prepared) -> PathBuf {
    let (_, dir) = prepared
        .env
        .iter()
        .find(|(k, _)| k == AGENT_DIR_ENV)
        .expect("PRIME_AGENT_CODING_AGENT_DIR in Prepared.env");
    PathBuf::from(dir)
}

fn is_link(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink())
}

/// Every regular file under `dir`, not following links.
fn regular_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let kind = entry.file_type().unwrap();
        if kind.is_dir() {
            regular_files(&entry.path(), out);
        } else if kind.is_file() {
            out.push(entry.path());
        }
    }
}

#[cfg(unix)]
fn mode(path: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).unwrap().permissions().mode() & 0o777
}

// ─── CTX-05: Prime ──────────────────────────────────────────────────────────

#[test]
fn ctx_05_prime_agent_dir_links_and_fleet_models() {
    let w = World::new();
    let ctx = w.ctx(&[]);
    let (t, decision) = decide(&w, &ctx);
    assert_eq!(
        decision,
        WindowDecision {
            tokens: Some(200_000),
            source: WindowSource::Fleet,
            detail: "teammate prime compact_window".into(),
            applied: true,
        }
    );
    let prepared = prepare(&w, &ctx, &t, &decision);
    let agent = agent_dir(&prepared);

    // <state>/prime/<12 hex>/agent/, beside the daemon's own files.
    assert_eq!(agent.file_name().unwrap(), "agent");
    let launch_dir = agent.parent().unwrap();
    assert_eq!(launch_dir.parent().unwrap(), w.state().join("prime"));
    let id = launch_dir.file_name().unwrap().to_string_lossy();
    assert!(
        id.len() == 12 && id.chars().all(|c| c.is_ascii_hexdigit()),
        "{id}"
    );

    let mut listing: Vec<String> = std::fs::read_dir(&agent)
        .unwrap()
        .flatten()
        .map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            if is_link(&e.path()) {
                format!(
                    "{name} -> {}",
                    std::fs::read_link(e.path()).unwrap().display()
                )
            } else {
                name
            }
        })
        .collect();
    listing.sort();
    eprintln!("agent dir {}:\n  {}", agent.display(), listing.join("\n  "));

    // Every source entry but the 2 generated files is a link to the source.
    for name in ["auth.json", "kernel-venv", "telemetry.json"] {
        let entry = agent.join(name);
        assert!(is_link(&entry), "{name} is not a link");
        assert_eq!(std::fs::read_link(&entry).unwrap(), w.source().join(name));
    }
    // Step 9 of u9b: Prime's global harness state dir is created empty in
    // the source and linked, so it persists as it does without horch.
    assert!(w.source().join("harness").is_dir());
    assert_eq!(
        std::fs::read_link(agent.join("harness")).unwrap(),
        w.source().join("harness")
    );
    // No credential is copied.
    let mut files = Vec::new();
    regular_files(&agent, &mut files);
    for file in &files {
        let text = std::fs::read_to_string(file).unwrap_or_default();
        assert!(!text.contains(TOKEN), "{} holds the token", file.display());
    }

    let settings = agent.join("settings.json");
    let models = agent.join("models.json");
    assert!(!is_link(&settings) && !is_link(&models));
    assert_eq!(
        read_json(&settings),
        json!({"telemetry":{"noticeShown":true},"autoRefine":{"enabled":false}})
    );
    assert_eq!(
        read_json(&models),
        json!({"providers":{"anthropic":{"modelOverrides":{"claude-opus-5-5":{"contextWindow":200000}}}}})
    );
    #[cfg(unix)]
    {
        assert_eq!(mode(&settings), 0o600);
        assert_eq!(mode(&models), 0o600);
    }
    assert!(!w.workdir.join(".prime").exists(), "wrote into the workdir");

    // The built command carries the window (design §6.5 step 3).
    let adapter = HarnessKind::Prime.adapter();
    let mut built = t.clone();
    built.args.extend(prepared.extra_args.iter().cloned());
    let mut cmd = adapter
        .build_command(
            &LaunchEnv::from_context(&ctx),
            &CommandSpec {
                teammate: &built,
                session: Session::Unmanaged,
                prompt: "P",
                model_override: None,
            },
        )
        .unwrap();
    assert_eq!(adapter.window_in_command(&cmd), None, "no agent dir yet");
    for (k, v) in &prepared.env {
        cmd.env(k, v);
    }
    assert_eq!(adapter.window_in_command(&cmd), Some(200_000));
    assert!(window_in_effect(decision, &built, &cmd).applied);
    // Dropped, not finished: no daemon runs.
    drop(prepared);
}

/// The launch dir of a prepared launch: it holds `sessions/`, the pane file
/// and (when built) `agent/`.
fn launch_dir(prepared: &Prepared) -> PathBuf {
    let sessions = prepared.sessions_dir.clone().expect("a sessions dir");
    sessions.parent().unwrap().to_path_buf()
}

/// The daemon flags come first and in this order, with or without a fleet
/// agent dir.
fn assert_daemon_flags(prepared: &Prepared) {
    let args = &prepared.extra_args;
    assert_eq!(args.len(), 4, "{args:?}");
    assert_eq!(
        (args[0].as_str(), args[2].as_str()),
        ("--daemon-socket", "--session-dir")
    );
    assert_eq!(
        args[3],
        launch_dir(prepared).join("sessions").to_string_lossy()
    );
}

/// The operator has no agent dir: nothing to link, and a login would land in
/// a dir nobody keeps. Prime runs on its own default dir, and horch creates
/// nothing in the operator's home. Outside a herdr pane, no pane file.
#[test]
fn prime_launch_without_an_operator_agent_dir_builds_none() {
    let w = World::new();
    std::fs::remove_dir_all(w.source()).unwrap();
    let ctx = w.ctx(&[]);
    let (t, decision) = decide(&w, &ctx);
    let prepared = prepare(&w, &ctx, &t, &decision);
    assert!(
        prepared.env.iter().all(|(k, _)| k != AGENT_DIR_ENV),
        "{:?}",
        prepared.env
    );
    let launch = launch_dir(&prepared);
    assert!(
        std::fs::symlink_metadata(launch.join("agent")).is_err(),
        "an agent dir was built"
    );
    assert!(!w.source().exists(), "the operator's agent dir was created");
    assert!(!launch.join("pane").exists(), "a pane file outside a pane");
    assert_daemon_flags(&prepared);
    // Dropped, not finished: no daemon runs.
    drop(prepared);
}

/// An agent dir that cannot be built does not stop the launch: the half-built
/// dir is removed, no variable points at it, and the pane uses the operator's
/// own dir. The daemon is reserved all the same.
#[cfg(unix)]
#[test]
fn prime_launch_goes_on_when_the_agent_dir_cannot_be_built() {
    use std::os::unix::fs::PermissionsExt;
    let w = World::new();
    let ctx = w.ctx(&[]);
    let (t, decision) = decide(&w, &ctx);
    // The entries of an operator dir that cannot be read cannot be linked.
    std::fs::set_permissions(w.source(), std::fs::Permissions::from_mode(0o000)).unwrap();
    // Root reads anything; then there is nothing to test.
    let readable = std::fs::read_dir(w.source()).is_ok();
    let prepared = prepare(&w, &ctx, &t, &decision);
    std::fs::set_permissions(w.source(), std::fs::Permissions::from_mode(0o755)).unwrap();
    if readable {
        return;
    }
    assert!(
        prepared.env.iter().all(|(k, _)| k != AGENT_DIR_ENV),
        "{:?}",
        prepared.env
    );
    assert!(
        std::fs::symlink_metadata(launch_dir(&prepared).join("agent")).is_err(),
        "the half-built agent dir stayed"
    );
    assert_daemon_flags(&prepared);
    // Dropped, not finished: no daemon runs.
    drop(prepared);
}

#[test]
fn ctx_05_prime_operator_override_wins() {
    // The operator's own window for this model.
    let w = World::new();
    let models = w.source().join("models.json");
    write(
        &models,
        &json!({"providers":{"anthropic":{"modelOverrides":{"claude-opus-5-5":{"contextWindow":300000}}}}})
            .to_string(),
    );
    let ctx = w.ctx(&[]);
    let (t, decision) = decide(&w, &ctx);
    assert_eq!(decision.source, WindowSource::Operator);
    assert_eq!(decision.tokens, Some(300_000));
    assert!(!decision.applied);
    let agent = agent_dir(&prepare(&w, &ctx, &t, &decision));
    assert!(is_link(&agent.join("models.json")));
    assert_eq!(
        std::fs::read_link(agent.join("models.json")).unwrap(),
        models
    );

    // Finding 10: a provider the operator defines at all is the operator's.
    let w = World::new();
    let models = w.source().join("models.json");
    write(
        &models,
        r#"{"providers":{"anthropic":{"baseUrl":"https://proxy.example/v1"}}}"#,
    );
    let ctx = w.ctx(&[]);
    let (t, decision) = decide(&w, &ctx);
    assert_eq!(
        decision,
        WindowDecision {
            tokens: None,
            source: WindowSource::Operator,
            detail: format!("{} (provider anthropic defined)", models.display()),
            applied: false,
        }
    );
    let agent = agent_dir(&prepare(&w, &ctx, &t, &decision));
    assert!(
        is_link(&agent.join("models.json")),
        "the pane keeps the proxy"
    );

    // The operator's own autoRefine is kept: no fleet key to add, so the
    // operator's file is linked, not copied (S6).
    let w = World::new();
    let operator = w.source().join("settings.json");
    write(
        &operator,
        r#"{"autoRefine":{"enabled":true},"theme":"dark"}"#,
    );
    let ctx = w.ctx(&[]);
    let (t, decision) = decide(&w, &ctx);
    let agent = agent_dir(&prepare(&w, &ctx, &t, &decision));
    assert_eq!(
        std::fs::read_link(agent.join("settings.json")).unwrap(),
        operator
    );

    // Project compaction settings: the trigger is the operator's, unknown.
    let w = World::new();
    let project = w.workdir.join(".prime/agent/settings.json");
    write(
        &project,
        r#"{"compaction":{"reserveTokens":30000},"autoRefine":{"enabled":true}}"#,
    );
    let ctx = w.ctx(&[]);
    let (t, decision) = decide(&w, &ctx);
    assert_eq!(
        decision,
        WindowDecision {
            tokens: None,
            source: WindowSource::Operator,
            detail: format!("{} (compaction set; trigger unknown)", project.display()),
            applied: false,
        }
    );
    let agent = agent_dir(&prepare(&w, &ctx, &t, &decision));
    assert!(is_link(&agent.join("models.json")));
    // The project file sets autoRefine, so the default yields to it and
    // the operator's file is linked.
    assert!(is_link(&agent.join("settings.json")));
    assert_eq!(
        std::fs::read_dir(w.workdir.join(".prime/agent"))
            .unwrap()
            .count(),
        1,
        "wrote into the workdir"
    );
}

#[test]
fn ctx_05_prime_inherited_agent_dir_is_the_source() {
    let w = World::new();
    let other = w.tmp.path().join("other-agent");
    w.fake_agent_dir(&other);
    write(&other.join("bin/tool"), "x");
    let ctx = w.ctx(&[(AGENT_DIR_ENV, &other.to_string_lossy())]);
    let (t, decision) = decide(&w, &ctx);
    let agent = agent_dir(&prepare(&w, &ctx, &t, &decision));
    for name in ["auth.json", "kernel-venv", "telemetry.json", "bin"] {
        assert_eq!(
            std::fs::read_link(agent.join(name)).unwrap(),
            other.join(name),
            "{name}"
        );
    }
}

/// Review finding 12 with Prime: the launch reads the window back after the
/// environment `prepare` set is on the command, so the record says applied.
/// Before the fix the read-back ran first and recorded `applied: false`.
#[cfg(unix)]
#[test]
fn ctx_05_prime_launch_records_the_applied_window() {
    use std::os::unix::fs::PermissionsExt;
    let w = World::new();
    write(&w.fake_prime(), "#!/bin/sh\nexit 0\n");
    std::fs::set_permissions(w.fake_prime(), std::fs::Permissions::from_mode(0o755)).unwrap();
    let ctx = w.ctx(&[]);
    let (roster, mut t) = prime_teammate();
    // No skills: a resume checks them against the record, which has none.
    t.phase = None;
    t.skills.clear();
    t.available_skills.clear();
    let store = ExecutionStore::open_in(&ctx).unwrap();
    store
        .insert(LedgerRecordV1 {
            record_id: "rec-1".into(),
            role: "prime-1".into(),
            ..LedgerRecordV1::default()
        })
        .unwrap();
    let mailbox = Mailbox::under(w.tmp.path().join("mail"), "ws");
    let workdir = w.workdir.to_string_lossy().into_owned();
    // A resume: no discovery thread to wait for.
    let session = SessionMode::Resume(SessionId::new("/s/old.jsonl").unwrap());
    launch::run_flow(
        &ctx,
        launch::LaunchRequest {
            role: "prime-1",
            teammate: &t,
            session: &session,
            prompt: "P",
            model_override: None,
            exec_rules: &[],
            child_env: Vec::new(),
            record: Some(DiscoveryTarget {
                mailbox: &mailbox,
                record_id: "rec-1",
                workdir: &workdir,
            }),
            fleet_window: roster.fleet_window(&t, MODEL),
            briefing_skill: None,
        },
    )
    .unwrap();
    let recorded = store.get("rec-1").unwrap().compact_window.unwrap();
    assert_eq!(recorded.tokens, Some(200_000));
    assert!(recorded.applied, "{recorded:?}");
}

/// Review S2 and S10: a teammate's own agent dir is the source, `~/`
/// expanded; a provider defined there is the operator's.
#[test]
fn ctx_05_prime_teammate_agent_dir_is_the_source() {
    let w = World::new();
    let own = w.home.join("work-agent");
    w.fake_agent_dir(&own);
    write(
        &own.join("models.json"),
        r#"{"providers":{"anthropic":{"baseUrl":"https://proxy.example/v1"}}}"#,
    );
    let ctx = w.ctx(&[]);
    let (roster, mut t) = prime_teammate();
    t.env.insert(AGENT_DIR_ENV.into(), "~/work-agent".into());
    let decision = window_decision(&ctx, &t, MODEL, &w.workdir, roster.fleet_window(&t, MODEL));
    assert_eq!(decision.source, WindowSource::Operator);
    assert_eq!(
        decision.detail,
        format!(
            "{} (provider anthropic defined)",
            own.join("models.json").display()
        )
    );
    let agent = agent_dir(&prepare(&w, &ctx, &t, &decision));
    for name in ["auth.json", "kernel-venv", "telemetry.json", "models.json"] {
        assert_eq!(
            std::fs::read_link(agent.join(name)).unwrap(),
            own.join(name),
            "{name}"
        );
    }
}

/// An inherited agent dir inside `<state_root>/prime/` is another launch's
/// (a pane started from a Prime pane): it is ignored.
#[test]
fn ctx_05_prime_inherited_launch_dir_is_ignored() {
    let w = World::new();
    let other_launch = w.state().join("prime/prime-0-old/agent");
    w.fake_agent_dir(&other_launch);
    let ctx = w.ctx(&[(AGENT_DIR_ENV, &other_launch.to_string_lossy())]);
    let (t, decision) = decide(&w, &ctx);
    assert!(decision.applied);
    let agent = agent_dir(&prepare(&w, &ctx, &t, &decision));
    assert_eq!(
        std::fs::read_link(agent.join("auth.json")).unwrap(),
        w.source().join("auth.json")
    );
}

/// Review S3 and S4: lock dirs and temp files are not linked; `auth.json`
/// is always a link, a dangling one when the operator has none.
#[test]
fn ctx_05_prime_transient_entries_and_missing_auth() {
    let w = World::new();
    let src = w.source();
    std::fs::create_dir_all(src.join("auth.json.lock")).unwrap();
    std::fs::create_dir_all(src.join("settings.json.lock")).unwrap();
    write(&src.join("auth.json.4242.0b5e.tmp"), "{}");
    std::fs::remove_file(src.join("auth.json")).unwrap();
    let ctx = w.ctx(&[]);
    let (t, decision) = decide(&w, &ctx);
    let agent = agent_dir(&prepare(&w, &ctx, &t, &decision));
    for name in [
        "auth.json.lock",
        "settings.json.lock",
        "auth.json.4242.0b5e.tmp",
    ] {
        assert!(
            std::fs::symlink_metadata(agent.join(name)).is_err(),
            "{name} linked"
        );
    }
    assert!(is_link(&agent.join("auth.json")));
    assert_eq!(
        std::fs::read_link(agent.join("auth.json")).unwrap(),
        src.join("auth.json")
    );
    assert!(
        !src.join("auth.json").exists(),
        "nothing was created in src"
    );
}

/// Review S6: the settings copy drops the legacy `apiKeys`.
#[test]
fn ctx_05_prime_settings_copy_drops_api_keys() {
    let w = World::new();
    write(
        &w.source().join("settings.json"),
        r#"{"mcpServers":{"x":{"url":"https://mcp.example","headers":{"Authorization":"Bearer FAKE-MCP"}}},"apiKeys":{"anthropic":"FAKE-LEGACY-KEY"}}"#,
    );
    let ctx = w.ctx(&[]);
    let (t, decision) = decide(&w, &ctx);
    let agent = agent_dir(&prepare(&w, &ctx, &t, &decision));
    let settings = agent.join("settings.json");
    assert!(
        !is_link(&settings),
        "a fleet key was added, so this is a copy"
    );
    let copy = read_json(&settings);
    assert!(copy.get("apiKeys").is_none(), "{copy}");
    assert_eq!(copy["autoRefine"], json!({"enabled": false}));
    let text = std::fs::read_to_string(&settings).unwrap();
    assert!(!text.contains("FAKE-LEGACY-KEY"));
}

/// Review S7: a FIFO in the repository's Prime settings neither blocks the
/// decision nor counts as absent.
#[cfg(unix)]
#[test]
fn ctx_05_prime_project_fifo_does_not_block() {
    let w = World::new();
    let project = w.workdir.join(".prime/agent/settings.json");
    std::fs::create_dir_all(project.parent().unwrap()).unwrap();
    let made = std::process::Command::new("mkfifo")
        .arg(&project)
        .status()
        .unwrap();
    assert!(made.success());
    let ctx = w.ctx(&[]);
    let (tx, rx) = std::sync::mpsc::channel();
    let workdir = w.workdir.clone();
    std::thread::spawn(move || {
        let (roster, t) = prime_teammate();
        let fleet = roster.fleet_window(&t, MODEL);
        let _ = tx.send(window_decision(&ctx, &t, MODEL, &workdir, fleet));
    });
    let decision = rx
        .recv_timeout(std::time::Duration::from_secs(1))
        .expect("window_decision blocked on a FIFO");
    assert_eq!(
        decision,
        WindowDecision {
            tokens: None,
            source: WindowSource::Operator,
            detail: format!("{} (not a regular file)", project.display()),
            applied: false,
        }
    );
}

/// Review S5 and outside-scope 3: the launch dir and `sessions/` are owner
/// only; `finish` removes `agent/` and keeps `sessions/` and the operator's
/// `auth.json`.
#[cfg(unix)]
#[test]
fn ctx_05_prime_finish_removes_the_agent_dir() {
    use std::os::unix::fs::PermissionsExt;
    let w = World::new();
    // `status --json` names no daemon.
    write(&w.fake_prime(), "#!/bin/sh\necho '[]'\n");
    std::fs::set_permissions(w.fake_prime(), std::fs::Permissions::from_mode(0o755)).unwrap();
    let ctx = w.ctx(&[]);
    let (t, decision) = decide(&w, &ctx);
    let prepared = prepare(&w, &ctx, &t, &decision);
    let agent = agent_dir(&prepared);
    let launch = agent.parent().unwrap().to_path_buf();
    let sessions = prepared.sessions_dir.clone().unwrap();
    assert_eq!(mode(&launch), 0o700);
    assert_eq!(mode(&sessions), 0o700);
    assert_eq!(mode(&agent), 0o700);
    prepared.finish();
    assert!(std::fs::symlink_metadata(&agent).is_err(), "agent/ stayed");
    assert!(sessions.is_dir(), "sessions/ was removed");
    assert!(w.source().join("auth.json").is_file());
}

/// Review S5: a launch removes the agent dirs of earlier launches that are
/// over (no socket, no live launcher), and leaves the others.
#[cfg(unix)]
#[test]
fn ctx_05_prime_launch_sweeps_old_agent_dirs() {
    let w = World::new();
    let prime = w.state().join("prime");
    let make = |name: &str, launcher: Option<String>, socket: bool| {
        let launch = prime.join(name);
        std::fs::create_dir_all(launch.join("agent")).unwrap();
        std::os::unix::fs::symlink(w.source().join("auth.json"), launch.join("agent/auth.json"))
            .unwrap();
        if let Some(text) = launcher {
            write(&launch.join("launcher"), &text);
        }
        if socket {
            write(&launch.join("d.sock"), "");
        }
        launch.join("agent")
    };
    let mut gone = std::process::Command::new("true").spawn().unwrap();
    let dead_pid = gone.id();
    gone.wait().unwrap();
    let dead = make("prime-1-dead", Some(format!("{dead_pid} \n")), false);
    let unmarked = make("prime-1-unmarked", None, false);
    let me = std::process::id();
    let live = make(
        "prime-1-live",
        Some(format!(
            "{me} {}\n",
            horch_core::procid::start_time(me).unwrap()
        )),
        false,
    );
    let socket = make("prime-1-socket", None, true);

    let ctx = w.ctx(&[]);
    let (t, decision) = decide(&w, &ctx);
    let _prepared = prepare(&w, &ctx, &t, &decision);
    assert!(
        std::fs::symlink_metadata(&dead).is_err(),
        "dead launcher kept"
    );
    assert!(
        std::fs::symlink_metadata(&unmarked).is_err(),
        "unmarked kept"
    );
    assert!(live.is_dir(), "a running launch lost its agent dir");
    assert!(
        socket.is_dir(),
        "a launch with a daemon socket lost its agent dir"
    );
    assert!(w.source().join("auth.json").is_file());
}

// ─── Prime 0.10.0: daemon stop, sweep and herdr release ────────────────────

/// The scan dir name of Prime 0.10.0 under `$TMPDIR`.
#[cfg(unix)]
fn scan_dir() -> String {
    // SAFETY: getuid has no preconditions and cannot fail.
    format!("prime-agent-{}", unsafe { libc::getuid() })
}

/// A `prime-agent` like the Rust build, as a shell script. Each call
/// appends `<command> <TMPDIR>` to `<tmp>/prime.log`. `shutdown` names
/// `$TMPDIR/prime-agent-<uid>/d.sock` as stopped when that socket exists,
/// unless `<tmp>/shutdown-fails` exists. `status` prints
/// `<tmp>/status.json`, else `[]`. It runs once before it is given back,
/// so the first-exec scan of macOS does not fall inside a test's wait.
#[cfg(unix)]
fn fake_rust_prime(w: &World) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let dir = w.tmp.path();
    let script = format!(
        r#"#!/bin/sh
echo "$1 $TMPDIR" >> '{dir}/prime.log'
case "$1" in
shutdown)
  sock="${{TMPDIR}}{scan}/d.sock"
  if [ -S "$sock" ] && [ ! -e '{dir}/shutdown-fails' ]; then
    printf '{{"stopped":[{{"socketPath":"%s","action":"stopped"}}],"failed":[]}}\n' "$sock"
  else
    echo '{{"stopped":[],"failed":[]}}'
  fi;;
status) cat '{dir}/status.json' 2>/dev/null || echo '[]';;
esac
"#,
        dir = dir.display(),
        scan = scan_dir()
    );
    write(&w.fake_prime(), &script);
    std::fs::set_permissions(w.fake_prime(), std::fs::Permissions::from_mode(0o755)).unwrap();
    std::process::Command::new(w.fake_prime())
        .arg("warm")
        .output()
        .unwrap();
    let _ = std::fs::remove_file(dir.join("prime.log"));
    w.fake_prime()
}

#[cfg(unix)]
fn prime_calls(w: &World) -> Vec<String> {
    std::fs::read_to_string(w.tmp.path().join("prime.log"))
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .collect()
}

/// An earlier launch dir: `agent/`, `sessions/`, the launcher file, and a
/// socket in the Rust layout (`new_layout`) or the 0.9.4 layout. `listen`
/// binds the socket, so a process listens on it.
#[cfg(unix)]
fn old_launch(
    w: &World,
    name: &str,
    launcher: &str,
    new_layout: bool,
    listen: bool,
) -> (PathBuf, PathBuf, Option<std::os::unix::net::UnixListener>) {
    let dir = w.state().join("prime").join(name);
    std::fs::create_dir_all(dir.join("agent/daemon-workers")).unwrap();
    std::fs::create_dir_all(dir.join("sessions")).unwrap();
    write(&dir.join("sessions/s.jsonl"), "{}");
    write(&dir.join("launcher"), launcher);
    let socket = if new_layout {
        dir.join(scan_dir()).join("d.sock")
    } else {
        dir.join("d.sock")
    };
    std::fs::create_dir_all(socket.parent().unwrap()).unwrap();
    let listener = if listen {
        Some(std::os::unix::net::UnixListener::bind(&socket).unwrap())
    } else {
        write(&socket, "");
        None
    };
    (dir, socket, listener)
}

/// The pid of a process that has ended, for a launcher file.
#[cfg(unix)]
fn dead_launcher() -> String {
    let mut gone = std::process::Command::new("true").spawn().unwrap();
    let pid = gone.id();
    gone.wait().unwrap();
    format!("{pid} \n")
}

/// The flags Prime 0.10.0 refuses ("Unknown option", exit 1) are not in a
/// launch: no `-e` status hook, and the pane file names the pane.
#[cfg(unix)]
#[test]
fn arc_10_prime_launch_in_a_herdr_pane_has_no_refused_flag() {
    let w = World::new();
    let (herdr, _log) = fake_herdr(&w);
    let ctx = w.ctx(&[
        ("HERDR_PANE_ID", "p_7"),
        ("HORCH_HERDR_BIN", &herdr.to_string_lossy()),
    ]);
    let (t, decision) = decide(&w, &ctx);
    let prepared = prepare(&w, &ctx, &t, &decision);
    for flag in [
        "-e",
        "--extension",
        "--no-extensions",
        "--tools",
        "--no-tools",
        "--exclude-tools",
    ] {
        assert!(
            !prepared.extra_args.iter().any(|a| a == flag),
            "{flag} in {:?}",
            prepared.extra_args
        );
    }
    let launch = prepared.sessions_dir.clone().unwrap();
    let launch = launch.parent().unwrap();
    assert_eq!(
        std::fs::read_to_string(launch.join("pane")).unwrap(),
        "p_7\n"
    );
    assert_eq!(mode(&launch.join("pane")), 0o600);
    let names: Vec<String> = std::fs::read_dir(launch)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(
        names.iter().all(|n| !n.ends_with(".mjs")),
        "a status hook was written: {names:?}"
    );
}

/// A `herdr` that appends each argv to `<tmp>/herdr.log`, 1 line per call.
/// Like herdr 0.8.2, it fails when the pane id does not follow the
/// subcommand (`pane release-agent --source x ...` exits 2).
#[cfg(unix)]
fn fake_herdr(w: &World) -> (PathBuf, PathBuf) {
    use std::os::unix::fs::PermissionsExt;
    let bin = w.tmp.path().join("herdr");
    let log = w.tmp.path().join("herdr.log");
    write(
        &bin,
        &format!(
            "#!/bin/sh\ncase \"$3\" in --*) echo \"unknown option: $4\" >&2; exit 2;; esac\necho \"$@\" >> '{}'\n",
            log.display()
        ),
    );
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    // The first run of a new executable on macOS can be slow.
    std::process::Command::new(&bin).status().unwrap();
    std::fs::remove_file(&log).unwrap();
    (bin, log)
}

/// On finish in a herdr pane, horch releases the pane's agent once, with
/// the source and agent label of Prime's built-in reporter: a clean-up
/// after a crash.
#[cfg(unix)]
#[test]
fn x2_prime_finish_releases_the_built_in_reporters_agent() {
    let w = World::new();
    fake_rust_prime(&w);
    let (herdr, log) = fake_herdr(&w);
    let ctx = w.ctx(&[
        ("HERDR_PANE_ID", "p_7"),
        ("HORCH_HERDR_BIN", &herdr.to_string_lossy()),
    ]);
    let (t, decision) = decide(&w, &ctx);
    let prepared = prepare(&w, &ctx, &t, &decision);
    prepared.finish();
    let calls = std::fs::read_to_string(&log).unwrap();
    let lines: Vec<&str> = calls.lines().collect();
    assert_eq!(lines.len(), 1, "{calls}");
    assert!(
        lines[0].starts_with(&format!(
            "pane release-agent p_7 --source {} --agent {} --seq ",
            horch_core::harness::prime::STATUS_SOURCE,
            horch_core::harness::prime::STATUS_AGENT
        )),
        "{calls}"
    );
    assert_eq!(horch_core::harness::prime::STATUS_SOURCE, "herdr:pi");
}

/// The sweep handles both layouts, and stops only a daemon that its launch
/// started:
/// - Rust layout, nobody listens: files removed, no Prime call.
/// - 0.9.4 layout, nobody listens: files removed.
/// - 0.9.4 layout, a process listens: the Rust `prime-agent` cannot name
///   it, so it is left running with its files.
/// - Rust layout, the scoped stop fails, `status` names a process started
///   after the launcher file: SIGTERM.
/// - Rust layout, `status` names a process started before the launcher
///   file: left running.
///
/// Every child sleeps 1 h, so none ends by itself during the test.
#[cfg(unix)]
#[test]
fn ctx_05_prime_sweep_handles_both_layouts() {
    use std::os::unix::process::ExitStatusExt;
    let w = World::new();
    fake_rust_prime(&w);
    let dead = dead_launcher();
    let (new_idle, new_idle_sock, _) = old_launch(&w, "0a0a0a0a0a0a", &dead, true, false);
    let (old_idle, old_idle_sock, _) = old_launch(&w, "prime-1-old", &dead, false, false);
    let (old_live, old_live_sock, _old_listener) =
        old_launch(&w, "prime-2-live", &dead, false, true);
    // Started before its launch's launcher file: not that launch's daemon.
    let mut foreign = std::process::Command::new("sleep")
        .arg("3600")
        .spawn()
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(2100));
    let (older, older_sock, _older_listener) = old_launch(&w, "0b0b0b0b0b0b", &dead, true, true);
    let (orphan, orphan_sock, _orphan_listener) = old_launch(&w, "0c0c0c0c0c0c", &dead, true, true);
    let mut daemon = std::process::Command::new("sleep")
        .arg("3600")
        .spawn()
        .unwrap();
    // The scoped stop fails, so the `status` fallback runs. The fake's
    // `status` ignores TMPDIR; horch matches the socket path.
    write(&w.tmp.path().join("shutdown-fails"), "");
    write(
        &w.tmp.path().join("status.json"),
        &format!(
            r#"[{{"socketPath":"{}","pid":{}}},{{"socketPath":"{}","pid":{}}}]"#,
            orphan_sock.display(),
            daemon.id(),
            older_sock.display(),
            foreign.id()
        ),
    );

    let ctx = w.ctx(&[]);
    let (t, decision) = decide(&w, &ctx);
    let _prepared = prepare(&w, &ctx, &t, &decision);

    for (dir, sock) in [(&new_idle, &new_idle_sock), (&old_idle, &old_idle_sock)] {
        assert!(
            std::fs::symlink_metadata(sock).is_err(),
            "{}",
            dir.display()
        );
        assert!(!dir.join("agent").exists(), "{}", dir.display());
        assert!(dir.join("sessions/s.jsonl").is_file(), "{}", dir.display());
    }
    assert!(!new_idle.join(scan_dir()).exists(), "the scan dir stayed");
    assert!(
        old_live_sock.exists(),
        "a live 0.9.4 daemon lost its socket"
    );
    assert!(old_live.join("agent").is_dir());

    let ended = daemon.wait().unwrap();
    assert_eq!(ended.signal(), Some(libc::SIGTERM), "{ended:?}");
    assert!(!orphan_sock.exists());
    assert!(!orphan.join("agent").exists());
    if let Some(status) = foreign.try_wait().unwrap() {
        panic!("a foreign daemon ended: {status:?}");
    }
    assert!(older_sock.exists(), "a running daemon lost its socket");
    assert!(older.join("agent").is_dir());
    foreign.kill().unwrap();
    foreign.wait().unwrap();

    // Only the launches with a listening socket got a Prime call, each
    // scoped by its own launch dir.
    let calls = prime_calls(&w);
    let scoped = |dir: &Path, cmd: &str| format!("{cmd} {}/", dir.display());
    for dir in [&new_idle, &old_idle] {
        assert!(
            !calls
                .iter()
                .any(|c| c.ends_with(&format!("{}/", dir.display()))),
            "{calls:?}"
        );
    }
    for dir in [&old_live, &older, &orphan] {
        assert!(calls.contains(&scoped(dir, "shutdown")), "{calls:?}");
        assert!(calls.contains(&scoped(dir, "status")), "{calls:?}");
    }
}

/// `horch done` closes the pane, and herdr kills the pane's processes
/// within about 1 s, often before the launcher stops the daemon (live check
/// X2: prime-4's daemon ran on). So `horch done` stops the daemon of each
/// launch that ran in its pane, and only those: the scoped stop runs with
/// that launch's `TMPDIR` only.
#[cfg(unix)]
#[test]
fn ctx_05_prime_finish_pane_stops_only_that_panes_daemon() {
    let w = World::new();
    fake_rust_prime(&w);
    let me = format!("{} \n", std::process::id());
    let (mine, mine_sock, _mine_listener) = old_launch(&w, "0d0d0d0d0d0d", &me, true, true);
    let (other, other_sock, _other_listener) = old_launch(&w, "0e0e0e0e0e0e", &me, true, true);
    write(&mine.join("pane"), "w3F:p42\n");
    write(&other.join("pane"), "w3F:p43\n");

    horch_core::harness::prime::finish_pane(&w.state(), &w.fake_prime(), "w3F:p42");

    assert_eq!(
        prime_calls(&w),
        [format!("shutdown {}/", mine.display())],
        "only this pane's launch is stopped"
    );
    assert!(std::fs::symlink_metadata(&mine_sock).is_err());
    assert!(!mine.join("agent").exists());
    assert!(
        mine.join("sessions/s.jsonl").is_file(),
        "sessions/ was removed"
    );
    assert!(other_sock.exists());
    assert!(other.join("agent").is_dir());

    // No Prime launch dir at all: nothing to do, no error.
    horch_core::harness::prime::finish_pane(&w.tmp.path().join("none"), &w.fake_prime(), "w3F:p42");
}

/// Live check (docs/live-checks/harnesses.md, "Prime adapter, Rust build"):
/// run inside a scratch herdr pane, with the real `prime-agent` and
/// `HORCH_STATE_DIR` set to a short scratch dir. It runs the Prime adapter's
/// `prepare`, the command horch builds without the `--` and the prompt (no
/// model turn), the TUI in this terminal until the operator quits it, and
/// then `finish`. It prints the argv and the scoped `status` before and
/// after the finish. Never run by the gate.
#[cfg(unix)]
#[test]
#[ignore = "live: needs the real prime-agent in a scratch herdr pane"]
fn live_prime_tui_start_and_finish() {
    use horch_core::runtime::ProcessEnv;
    let ctx = RuntimeContext::from_env(&ProcessEnv).unwrap();
    let workdir = std::env::current_dir().unwrap();
    let (roster, t) = prime_teammate();
    let fleet = roster.fleet_window(&t, MODEL);
    let decision = window_decision(&ctx, &t, MODEL, &workdir, fleet);
    let adapter = HarnessKind::Prime.adapter();
    let prepared = adapter
        .prepare(
            &ctx,
            &PrepareRequest {
                role: "prime-live",
                exec_rules: &[],
                skills: None,
                compact_window: Some(&decision),
                teammate: &t,
                model: MODEL,
                workdir: &workdir,
                shell_env: &[],
            },
        )
        .unwrap();
    let mut teammate = t.clone();
    teammate.args.extend(prepared.extra_args.iter().cloned());
    let built = adapter
        .build_command(
            &LaunchEnv::from_context(&ctx),
            &CommandSpec {
                teammate: &teammate,
                session: Session::Unmanaged,
                prompt: "",
                model_override: None,
            },
        )
        .unwrap();
    let args: Vec<String> = built
        .get_args()
        .map(|a| a.to_string_lossy().into_owned())
        .take_while(|a| a != "--")
        .collect();
    println!("argv: {:?} {args:?}", built.get_program());
    let mut cmd = std::process::Command::new(built.get_program());
    cmd.args(&args).env_remove("ANTHROPIC_API_KEY");
    for (k, v) in built.get_envs() {
        match v {
            Some(v) => cmd.env(k, v),
            None => cmd.env_remove(k),
        };
    }
    for (k, v) in &prepared.env {
        cmd.env(k, v);
    }
    let status = cmd.status().unwrap();
    println!("prime-agent exited: {status}");
    let launch = prepared.sessions_dir.clone().unwrap();
    let launch = launch.parent().unwrap().to_path_buf();
    let scoped = |what: &str| {
        let out = std::process::Command::new(&ctx.bins.harness.prime)
            .args(["status", "--json"])
            .env("TMPDIR", format!("{}/", launch.display()))
            .env_remove("ANTHROPIC_API_KEY")
            .output()
            .unwrap();
        println!("{what}: {}", String::from_utf8_lossy(&out.stdout).trim());
    };
    scoped("status before finish");
    prepared.finish();
    scoped("status after finish");
    println!("launch dir: {}", launch.display());
}
