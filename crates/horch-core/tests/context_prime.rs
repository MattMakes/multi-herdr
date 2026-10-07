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
        let tmp = tempfile::tempdir().unwrap();
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

    // <state>/prime/<slug>-<uuid>/agent/, beside the daemon's own files.
    assert_eq!(agent.file_name().unwrap(), "agent");
    let launch_dir = agent.parent().unwrap();
    assert_eq!(launch_dir.parent().unwrap(), w.state().join("prime"));
    assert!(launch_dir
        .file_name()
        .unwrap()
        .to_string_lossy()
        .starts_with("prime-1-"));

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
