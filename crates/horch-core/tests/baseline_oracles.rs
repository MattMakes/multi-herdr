//! A0 behavior oracles: what the pre-refactor code does, frozen in
//! `tests/oracles/` (ARC-01).
//!
//! The oracle files never change. Later phases move the call sites in this
//! file, never the expected files. `HORCH_BLESS=1` writes them; that is for
//! the A0 baseline only, and a diff here means behavior changed.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use horch_core::balance_policy::{self, Decision, GateFlags, PoolLine};
use horch_core::launch::{self, Session};
use horch_core::ledger::{Ledger, Record};
use horch_core::policy::{BalanceMode, Policy};
use horch_core::quota::{QuotaFile, QuotaView};
use horch_core::skills::Bundle;
use horch_core::teammates::{Phase, Roster, Teammate};
use serde_json::{json, Value};

// ─── bless helper ───────────────────────────────────────────────────────────

fn oracles() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/oracles")
}

fn blessing() -> bool {
    std::env::var("HORCH_BLESS").ok().as_deref() == Some("1")
}

/// Write `actual` under `HORCH_BLESS=1`; otherwise compare it with the
/// frozen file, which must exist.
fn check_oracle(rel: &str, actual: &str) {
    let path = oracles().join(rel);
    if blessing() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, actual).unwrap();
        return;
    }
    let want = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "{}: {e}. Oracles are frozen at A0; an absent file is a failure",
            path.display()
        )
    });
    if want != actual {
        let line = want
            .lines()
            .zip(actual.lines())
            .position(|(a, b)| a != b)
            .unwrap_or_else(|| want.lines().count().min(actual.lines().count()));
        panic!(
            "{rel} differs from the A0 oracle at line {}:\n  want: {:?}\n  got:  {:?}\n\
             The oracle is frozen: fix the code, not the file.",
            line + 1,
            want.lines().nth(line),
            actual.lines().nth(line)
        );
    }
}

fn pretty(v: &Value) -> String {
    let mut s = serde_json::to_string_pretty(v).unwrap();
    s.push('\n');
    s
}

// ─── pinned environment ─────────────────────────────────────────────────────

/// Serializes every test that touches process-wide environment.
static ENV_LOCK: Mutex<()> = Mutex::new(());

/// Variables the builders and loaders read today, and what each is pinned to.
/// `<TMP>` and `<HOME>` stand for the temp dirs below.
const PINNED: [(&str, Option<&str>); 13] = [
    ("HOME", Some("<HOME>")),
    ("PATH", Some("<TMP>/bin")),
    ("HORCH_CLAUDE_BIN", None),
    ("HORCH_CODEX_BIN", None),
    ("HORCH_OPENCODE_BIN", None),
    ("HORCH_PI_BIN", None),
    ("HORCH_PRIME_BIN", None),
    ("HORCH_TEAMMATES_DIR", None),
    ("OPENCODE_CONFIG_CONTENT", None),
    ("CODEX_HOME", None),
    ("CLAUDE_CODE_EFFORT_LEVEL", None),
    ("HORCH_NOW", None),
    ("ANTHROPIC_API_KEY", None),
];

/// The operator's settings as the fake home holds them: one enabled plugin,
/// one disabled, a status line, and a second skill-creator install.
const SETTINGS_JSON: &str = r#"{"enabledPlugins":{"alpha@oracle-market":true,"beta@oracle-market":false},"statusLine":{"type":"command","command":"oracle-status"}}"#;
const INSTALLED_PLUGINS_JSON: &str =
    r#"{"version":2,"plugins":{"skill-creator@oracle-fork":[{"installPath":"~/plugins/sc"}]}}"#;

const PROMPT: &str = "ORACLE PROMPT: do the task.";
const FRESH_ID: &str = "00000000-0000-4000-8000-000000000001";
const RESUME_ID: &str = "00000000-0000-4000-8000-000000000002";

/// A temp world with the environment pinned. Restores it on drop.
struct World {
    _guard: MutexGuard<'static, ()>,
    _tmp: tempfile::TempDir,
    root: PathBuf,
    home: PathBuf,
    state: PathBuf,
    saved: Vec<(&'static str, Option<std::ffi::OsString>)>,
}

impl World {
    fn new() -> World {
        let guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        // Canonical, so a bundle path (canonicalized by `Bundle::install`)
        // and the home share one prefix.
        let root = tmp.path().canonicalize().unwrap();
        let home = root.join("home");
        let state = root.join("state");
        std::fs::create_dir_all(home.join(".claude/plugins")).unwrap();
        std::fs::create_dir_all(&state).unwrap();
        std::fs::create_dir_all(root.join("bin")).unwrap();
        std::fs::write(home.join(".claude/settings.json"), SETTINGS_JSON).unwrap();
        std::fs::write(
            home.join(".claude/plugins/installed_plugins.json"),
            INSTALLED_PLUGINS_JSON,
        )
        .unwrap();
        let mut saved = Vec::new();
        for (key, value) in PINNED {
            saved.push((key, std::env::var_os(key)));
            match value {
                Some(v) => std::env::set_var(
                    key,
                    v.replace("<HOME>", &home.to_string_lossy())
                        .replace("<TMP>", &root.to_string_lossy()),
                ),
                None => std::env::remove_var(key),
            }
        }
        World {
            _guard: guard,
            _tmp: tmp,
            root,
            home,
            state,
            saved,
        }
    }

    /// Stable placeholders for every machine-specific path.
    fn scrub(&self, s: &str, bundle: Option<&Path>) -> String {
        let mut out = s.to_string();
        if let Some(b) = bundle {
            out = out.replace(&*b.to_string_lossy(), "<BUNDLE>");
        }
        out = out.replace(&*self.home.to_string_lossy(), "<HOME>");
        out = out.replace(&*self.root.to_string_lossy(), "<TMP>");
        out.replace(PROMPT, "<PROMPT>")
    }

    fn roster(&self) -> Roster {
        Roster::load_with(Some(repo_teammates().to_str().unwrap())).unwrap()
    }
}

impl Drop for World {
    fn drop(&mut self) {
        for (key, value) in self.saved.drain(..) {
            match value {
                Some(v) => std::env::set_var(key, v),
                None => std::env::remove_var(key),
            }
        }
    }
}

fn repo_teammates() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../teammates")
}

fn pinned_header() -> Value {
    PINNED
        .iter()
        .map(|(k, v)| (k.to_string(), v.map_or(Value::Null, |v| json!(v))))
        .collect::<serde_json::Map<_, _>>()
        .into()
}

const PHASES: [Phase; 4] = [
    Phase::Research,
    Phase::Plan,
    Phase::Implementation,
    Phase::Validation,
];

// ─── oracle 1: launch argv and env ──────────────────────────────────────────

fn bundle_root(bundle: &Bundle) -> PathBuf {
    bundle.skills_dir().parent().unwrap().to_path_buf()
}

fn launch_oracle(world: &World, t: &Teammate) -> Value {
    let bundle = Bundle::install(&world.state, t);
    let (bundle, bundle_note) = match bundle {
        Ok(b) => (b, Value::Null),
        Err(e) => (None, json!(format!("{e:#}"))),
    };
    let root = bundle.as_ref().map(bundle_root);
    let mut modes = serde_json::Map::new();
    for (label, session) in [
        ("fresh", Session::Fresh(FRESH_ID)),
        ("resume", Session::Resume(RESUME_ID)),
        ("unmanaged", Session::Unmanaged),
    ] {
        let built = launch::command_with_skills(t, session, PROMPT, None, bundle.as_ref());
        let v = match built {
            Ok(cmd) => {
                let scrub =
                    |s: &std::ffi::OsStr| world.scrub(&s.to_string_lossy(), root.as_deref());
                let env: serde_json::Map<String, Value> = cmd
                    .get_envs()
                    .map(|(k, v)| {
                        (
                            k.to_string_lossy().into_owned(),
                            v.map_or(Value::Null, |v| json!(scrub(v))),
                        )
                    })
                    .collect();
                json!({
                    "program": scrub(cmd.get_program()),
                    "args": cmd.get_args().map(scrub).collect::<Vec<_>>(),
                    "env": env,
                    "current_dir": cmd.get_current_dir().map(|d| scrub(d.as_os_str())),
                })
            }
            Err(e) => json!({ "error": world.scrub(&format!("{e:#}"), root.as_deref()) }),
        };
        modes.insert(label.to_string(), v);
    }
    json!({
        "_pinned_env": pinned_header(),
        "_prompt": PROMPT,
        "_session_ids": {"fresh": FRESH_ID, "resume": RESUME_ID},
        "teammate": t.name,
        "skills_bundle": bundle.is_some(),
        "skills_bundle_error": bundle_note,
        "modes": modes,
    })
}

#[test]
fn oracle_launch_matches() {
    let world = World::new();
    let roster = world.roster();
    for name in roster.names() {
        let t = roster.get(name).unwrap();
        check_oracle(
            &format!("launch/{name}.json"),
            &pretty(&launch_oracle(&world, t)),
        );
    }
}

// ─── oracle 2: routing decisions ────────────────────────────────────────────

fn quota_fixtures() -> Vec<(String, PathBuf)> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/telemetry/quota");
    let mut out: Vec<(String, PathBuf)> = std::fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("json"))
        .map(|p| (p.file_stem().unwrap().to_string_lossy().into_owned(), p))
        .collect();
    out.sort();
    out
}

/// The time every quota fixture is judged at, as the QUO unit tests do.
const ROUTING_NOW: &str = "2026-09-28T18:00:00Z";

fn quota_view(path: &Path) -> QuotaView {
    let file = QuotaFile::read(path).unwrap();
    let now = horch_core::clock::parse(ROUTING_NOW).unwrap();
    QuotaView::new(file, now, Policy::default(), true)
}

/// The object `horch route --json` prints, built the way `cmd/route.rs`
/// builds it.
fn route_json(
    t: &Teammate,
    roster: &Roster,
    view: &QuotaView,
    mode: BalanceMode,
    decision: &Decision,
) -> Value {
    let mut pools: Vec<PoolLine> = Vec::new();
    for c in std::iter::once(t).chain(t.fallbacks.iter().filter_map(|f| roster.get(f))) {
        let a = view.assess(c.agent.as_str(), c.model.as_deref().unwrap_or_default());
        if !pools.iter().any(|p| p.pool == a.pool) {
            pools.push(PoolLine {
                pool: a.pool.clone(),
                state: a.state.to_string(),
                detail: balance_policy::summary(&a, view),
            });
        }
    }
    let (kind, via, reason) = match decision {
        Decision::Spawn { note, .. } => ("spawn", None, note.clone()),
        Decision::Substitute { via, reason, .. } => {
            ("substitute", Some(via.clone()), Some(reason.clone()))
        }
        Decision::Refuse { reason, .. } => ("refuse", None, Some(reason.clone())),
    };
    json!({
        "decision": kind,
        "teammate": t.name,
        "via": via,
        "reason": reason,
        "line": decision.line(),
        "mode": mode.as_str(),
        "pools": pools,
    })
}

#[test]
fn oracle_routing_matches() {
    let world = World::new();
    let roster = world.roster();
    let flags = [
        ("none", GateFlags::default()),
        (
            "exact",
            GateFlags {
                exact: true,
                force: false,
            },
        ),
        (
            "force",
            GateFlags {
                exact: false,
                force: true,
            },
        ),
    ];
    let modes = [
        ("auto", BalanceMode::Auto),
        ("advise", BalanceMode::Advise),
        ("off", BalanceMode::Off),
    ];
    for (fixture, path) in quota_fixtures() {
        let view = quota_view(&path);
        let mut out = BTreeMap::new();
        for name in roster.names() {
            let t = roster.get(name).unwrap();
            for (flag_label, flag) in flags {
                for (mode_label, mode) in modes {
                    let decision = balance_policy::decide(t, &roster, &view, mode, flag);
                    let resolved = balance_policy::resolve(t, &roster, &decision).map(|r| {
                        json!({
                            "name": r.name,
                            "agent": r.agent.as_str(),
                            "model": r.model,
                            "effort": r.effort,
                        })
                    });
                    out.insert(
                        format!("{name}|{flag_label}|{mode_label}"),
                        json!({
                            "route_json": route_json(t, &roster, &view, mode, &decision),
                            "decision": serde_json::to_value(&decision).unwrap(),
                            "resolved": resolved,
                        }),
                    );
                }
            }
        }
        let doc = json!({
            "_now": ROUTING_NOW,
            "_policy": "Policy::default()",
            "decisions": out,
        });
        check_oracle(&format!("routing/{fixture}.json"), &pretty(&doc));
    }
    check_oracle(
        "routing/fallback_problems.json",
        &pretty(&json!(balance_policy::fallback_problems(&roster))),
    );
}

// ─── oracle 3: legacy ledgers ───────────────────────────────────────────────

const LEDGERS: [&str; 4] = ["bash-era", "pre-effort", "pr14-substituted", "orchestrator"];

/// Every field, defaults included, so a skipped-on-disk default is visible.
fn record_json(r: &Record) -> Value {
    json!({
        "record_id": r.record_id,
        "session_id": r.session_id,
        "agent": r.agent,
        "tier": r.tier,
        "model": r.model,
        "effort": r.effort,
        "phase": r.phase.map(|p| p.as_str()),
        "role": r.role,
        "status": r.status,
        "task": r.task,
        "history": r.history.iter().map(|h| json!({"at": h.at, "event": h.event, "text": h.text})).collect::<Vec<_>>(),
        "created_at": r.created_at,
        "updated_at": r.updated_at,
        "kind": r.kind,
        "is_orchestrator": r.is_orchestrator(),
        "project": r.project,
        "plan": r.plan,
        "workspace_id": r.workspace_id,
        "via": r.via,
        "substitution_reason": r.substitution_reason,
    })
}

#[test]
fn oracle_ledgers_match() {
    let tmp = tempfile::tempdir().unwrap();
    for name in LEDGERS {
        let fixture = oracles().join(format!("ledgers/{name}.json"));
        let ledger = Ledger::for_project(tmp.path(), &format!("/oracle/{name}"));
        std::fs::copy(&fixture, ledger.path()).unwrap_or_else(|e| {
            panic!("{}: {e}", fixture.display());
        });
        let records = ledger.read().unwrap();
        let loaded: Vec<Value> = records.iter().map(record_json).collect();
        check_oracle(
            &format!("ledgers/{name}.loaded.json"),
            &pretty(&json!(loaded)),
        );
        // The exact bytes `Ledger::write` (private) produces for these
        // records: `serde_json::to_string_pretty`, no trailing newline.
        let saved = serde_json::to_string_pretty(&records).unwrap();
        check_oracle(&format!("ledgers/{name}.saved.json"), &saved);
    }
}

// ─── oracle 4: skills briefings ─────────────────────────────────────────────

#[test]
fn oracle_skills_match() {
    let world = World::new();
    let roster = world.roster();
    for name in roster.names() {
        for phase in PHASES {
            let mut t = roster.get(name).unwrap().clone();
            t.phase = Some(phase);
            let text = match Bundle::install(&world.state, &t) {
                Ok(Some(bundle)) => {
                    let root = bundle_root(&bundle);
                    world.scrub(&bundle.briefing(&t), Some(&root))
                }
                Ok(None) => "NO BUNDLE\n".to_string(),
                Err(e) => format!("ERROR: {}\n", world.scrub(&format!("{e:#}"), None)),
            };
            check_oracle(&format!("skills/{name}-{phase}.txt"), &text);
        }
    }
}

// ─── ARC-01 ─────────────────────────────────────────────────────────────────

/// Counts at the A0 freeze. The roster had 33 teammates and the quota corpus
/// 12 fixtures.
const TEAMMATES_AT_A0: usize = 33;
const QUOTA_FIXTURES_AT_A0: usize = 12;

fn files_in(dir: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .collect();
    out.sort();
    out
}

#[test]
fn arc_01_baseline_oracles_present() {
    let cli = Path::new(env!("CARGO_MANIFEST_DIR")).join("../horch/tests/oracles");
    let expect = [
        (oracles().join("launch"), TEAMMATES_AT_A0),
        (oracles().join("routing"), QUOTA_FIXTURES_AT_A0 + 1),
        (oracles().join("ledgers"), LEDGERS.len() * 3),
        (oracles().join("skills"), TEAMMATES_AT_A0 * PHASES.len()),
        // skills, skills --json, and text plus JSON for each phase.
        (cli.join("skills"), 2 + 2 * PHASES.len()),
        // text plus JSON for each ledger fixture.
        (cli.join("sessions"), 2 * LEDGERS.len()),
    ];
    for (dir, count) in expect {
        let files = files_in(&dir);
        assert_eq!(files.len(), count, "{}: oracle file count", dir.display());
        for f in &files {
            let len = std::fs::metadata(f).unwrap().len();
            assert!(len > 0, "{} is empty", f.display());
        }
    }
}
