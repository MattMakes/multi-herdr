//! End-to-end tests of the context policy (`docs/specs/context-policy.md`):
//! the built `horch` binary on the context fixture home, and scans of the
//! live check `scripts/live/context.sh` and of the CTX code (CTX-27).
//!
//! Hermetic: the environment is cleared (so `ANTHROPIC_API_KEY`,
//! `CODEX_HOME`, `HORCH_TEAMMATES_DIR` and the operator's
//! `CLAUDE_CODE_AUTO_COMPACT_WINDOW` never reach the binary); `HOME` is a
//! temp copy of `crates/horch-core/tests/fixtures/context/home`, the state
//! dir is temp, the project is `/fixture` (the fixture transcripts' slug),
//! and the managed-settings path points at a file that does not exist.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// The project the fixture transcripts are under (`~/.claude/projects/-fixture`).
const PROJECT: &str = "/fixture";

/// The fixture sessions (`horch-core/tests/fixtures/context/home`).
const OVER: &str = "11111111-1111-4111-8111-111111111111"; // 311,225 tokens
const PENDING: &str = "22222222-2222-4222-8222-222222222222"; // compacted, post 10,486
const SMALL: &str = "33333333-3333-4333-8333-333333333333"; // 33,352 tokens
const CODEX: &str = "01a0de6e-0000-7000-8000-000000000001"; // 227,306 tokens
const MISSING: &str = "99999999-9999-4999-8999-999999999999"; // no transcript

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).unwrap();
        }
    }
}

/// A ledger record of the fixture project, live, in workspace `w1`.
fn record(id: &str, role: &str, agent: &str, tier: &str, sid: &str) -> serde_json::Value {
    serde_json::json!({
        "record_id": id, "session_id": sid, "agent": agent, "tier": tier,
        "model": tier, "role": role, "status": "working", "task": "work",
        "history": [{"at": "2026-10-06T09:00:00Z", "event": "spawned", "text": "work"}],
        "created_at": "2026-10-06T09:00:00Z", "updated_at": "2026-10-06T09:00:00Z",
        "project": PROJECT, "workspace_id": "w1"
    })
}

struct World {
    _tmp: tempfile::TempDir,
    root: PathBuf,
}

impl World {
    /// The fixture home with the operator's Claude window of this Mac
    /// (500,000 in `~/.claude/settings.json`, design §8.1), so a Claude
    /// threshold is 300,000, and a hand-written ledger:
    /// - `orchestrator` on 3333: 33,352 tokens, `ok`;
    /// - `sonnet-1` on 1111: 311,225 tokens, `over`;
    /// - `opus-1` on 2222: a compaction with no response after it, `pending`;
    /// - `codex-sol-1` on Codex 001: 227,306 tokens over the recorded fleet
    ///   limit's threshold 160,000, `over`;
    /// - `sonnet-2` on a session with no transcript, `no-transcript`.
    fn new() -> World {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        copy_dir(
            &repo().join("crates/horch-core/tests/fixtures/context/home"),
            &root.join("home"),
        );
        std::fs::write(
            root.join("home/.claude/settings.json"),
            r#"{"env":{"CLAUDE_CODE_AUTO_COMPACT_WINDOW":"500000"}}"#,
        )
        .unwrap();
        std::fs::create_dir_all(root.join("state")).unwrap();

        let mut orchestrator = record("rec-orch", "orchestrator", "claude", "orchestrator", SMALL);
        orchestrator["kind"] = "orchestrator".into();
        orchestrator["model"] = "opus".into();
        orchestrator["task"] = "(orchestrating)".into();
        let mut codex = record("rec-cdx", "codex-sol-1", "codex", "codex-sol", CODEX);
        codex["model"] = "gpt-5.6-sol".into();
        codex["compact_window"] = serde_json::json!({
            "tokens": 200000, "source": "fleet",
            "detail": "context-policy windows codex/gpt-5.6-sol", "applied": true
        });
        let ledger = serde_json::json!([
            orchestrator,
            record("rec-s1", "sonnet-1", "claude", "sonnet", OVER),
            record("rec-o1", "opus-1", "claude", "opus", PENDING),
            codex,
            record("rec-s2", "sonnet-2", "claude", "sonnet", MISSING),
        ]);
        let path = horch_core::execution::records::Ledger::for_project(root.join("state"), PROJECT)
            .path()
            .to_path_buf();
        std::fs::write(path, serde_json::to_string_pretty(&ledger).unwrap()).unwrap();
        World { _tmp: tmp, root }
    }

    fn horch(&self, args: &[&str], env: &[(&str, &str)]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_horch"))
            .args(args)
            .current_dir(&self.root)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", self.root.join("home"))
            .env("HORCH_STATE_DIR", self.root.join("state"))
            .env("HORCH_PROJECT_DIR", PROJECT)
            .env(
                "HORCH_CLAUDE_MANAGED_SETTINGS",
                self.root.join("managed-settings.json"),
            )
            .envs(env.iter().copied())
            .output()
            .unwrap()
    }

    /// stdout of a run that must exit 0 with nothing on stderr.
    fn stdout(&self, args: &[&str], env: &[(&str, &str)]) -> String {
        let out = self.horch(args, env);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(out.status.success(), "horch {args:?}: {stderr}");
        assert_eq!(stderr, "", "horch {args:?} wrote to stderr");
        String::from_utf8(out.stdout).unwrap()
    }

    fn history(&self, id: &str) -> Vec<(String, String)> {
        let ledger =
            horch_core::execution::records::Ledger::for_project(self.root.join("state"), PROJECT);
        ledger
            .get(id)
            .unwrap()
            .history
            .into_iter()
            .map(|h| (h.event, h.text))
            .collect()
    }
}

/// CTX-01, CTX-03, CTX-04, CTX-09, CTX-10 end to end: `horch context`,
/// `horch context --over` and the `horch note` warning on the fixture home.
#[test]
fn ctx_01_cli_table_from_fixture_home() {
    let w = World::new();
    let table = w.stdout(&["context"], &[]);
    let want = "\
ROLE          HARNESS  MODEL             CONTEXT     WINDOW   NATIVE  SOURCE          THRESHOLD  LAST COMPACT              COMPACTIONS  STATE
orchestrator  claude   claude-opus-5-5   33,352   1,000,000  467,000  operator (now)    300,000  2026-09-20T10:05:00.000Z  1n/0h        ok
codex-sol-1   codex    gpt-5.6-sol      227,306     258,400  200,000  fleet             160,000  -                         0n/0h        over
opus-1        claude   claude-opus-5-5   10,486*  1,000,000  467,000  operator (now)    300,000  2026-09-20T10:05:00.000Z  1n/0h        pending
sonnet-1      claude   claude-opus-5-5  311,225   1,000,000  467,000  operator (now)    300,000  -                         0n/0h        over
sonnet-2      claude   sonnet                 -   1,000,000  467,000  operator (now)    300,000  -                         -            no-transcript
reasons:
  sonnet-2: no-transcript: no transcript found
";
    assert_eq!(table, want, "\n{table}");

    let over = w.stdout(&["context", "--over"], &[]);
    let rows: Vec<&str> = over.lines().skip(1).collect();
    assert!(over.starts_with("ROLE "), "{over}");
    assert_eq!(rows.len(), 2, "{over}");
    assert!(
        rows[0].starts_with("codex-sol-1 ") && rows[0].ends_with(" over"),
        "{over}"
    );
    assert!(
        rows[1].starts_with("sonnet-1 ") && rows[1].ends_with(" over"),
        "{over}"
    );

    // Design §6.3, the rendered `warning` for sonnet-1 at 311,225 tokens.
    let warning = "NOTE: Context warning. Your context is 311225 tokens. Your threshold is 300000 tokens. \
This horch note call is your stopping point. Do not start new work. \
Write ai_docs/handoffs/sonnet-1-whats-next.md with the horch:handoff skill. \
Then run: horch note \"handoff: ai_docs/handoffs/sonnet-1-whats-next.md\". \
Then send: horch tell orchestrator \"[sonnet-1] NOTE: COMPACT-READY ai_docs/handoffs/sonnet-1-whats-next.md\". \
Then end your turn and wait.\n";
    let me = [("HORCH_RECORD_ID", "rec-s1")];
    assert_eq!(w.stdout(&["note", "step 1 done"], &me), warning);
    let handoff = "handoff: ai_docs/handoffs/sonnet-1-whats-next.md";
    assert_eq!(w.stdout(&["note", handoff], &me), "", "1 warning per cycle");
    let history = w.history("rec-s1");
    let notes: Vec<&str> = history
        .iter()
        .filter(|(e, _)| e == "note")
        .map(|(_, t)| t.as_str())
        .collect();
    assert_eq!(notes, ["step 1 done", handoff]);
    let warned: Vec<&str> = history
        .iter()
        .filter(|(e, _)| e == "context-warned")
        .map(|(_, t)| t.as_str())
        .collect();
    assert_eq!(warned, ["tokens 311225 threshold 300000"]);

    let requested = w.stdout(&["context", "--over"], &[]);
    let row = requested
        .lines()
        .find(|l| l.starts_with("sonnet-1 "))
        .unwrap();
    assert!(
        row.ends_with(" requested"),
        "a warned row is requested:\n{requested}"
    );
}

fn script() -> String {
    std::fs::read_to_string(repo().join("scripts/live/context.sh")).unwrap()
}

/// The text of live-check step `name` (`# ---- A2 ...` up to the next step).
fn step(script: &str, name: &str) -> String {
    let start = script
        .find(&format!("# ---- {name} "))
        .unwrap_or_else(|| panic!("scripts/live/context.sh has no step {name}"));
    let rest = &script[start + 7..];
    let end = rest.find("# ---- ").unwrap_or(rest.len());
    rest[..end].to_string()
}

/// CTX-06: the live check exists, is executable, and checks every Claude
/// and Codex teammate that `horch context --windows` lists: the Claude tier
/// loop runs `/autocompact` for every claude row and every source, and the
/// Codex step reads every codex row and the launch limit flag.
#[test]
fn ctx_06_live_check_covers_every_tier() {
    let path = repo().join("scripts/live/context.sh");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_ne!(mode & 0o111, 0, "{} is not executable", path.display());
    }
    let s = script();

    let w = World::new();
    let windows = w.stdout(&["context", "--windows"], &[]);
    let harnesses: Vec<(&str, &str)> = windows
        .lines()
        .skip(1)
        .filter_map(|l| {
            let mut cols = l.split_whitespace();
            Some((cols.next()?, cols.next()?))
        })
        .filter(|(_, h)| *h == "claude" || *h == "codex")
        .collect();
    assert!(harnesses.iter().any(|(_, h)| *h == "claude"), "{windows}");
    assert!(harnesses.iter().any(|(_, h)| *h == "codex"), "{windows}");

    let a2 = step(&s, "A2");
    assert!(a2.contains("context --windows"), "A2 reads --windows");
    assert!(
        a2.contains(r#"$2 == "claude""#),
        "A2 loops over every claude row"
    );
    assert!(a2.contains(r#""/autocompact""#), "A2 asks the harness");
    for source in ["operator)", "fleet)", "harness)"] {
        assert!(a2.contains(source), "A2 handles source {source}");
    }
    for tier in [
        "claude/sonnet:150000",
        "claude/opus:200000",
        "claude/orchestrator:300000",
    ] {
        assert!(a2.contains(tier), "A2 checks the fleet value {tier}");
    }
    let a4 = step(&s, "A4");
    assert!(a4.contains(r#"$2 == "codex""#), "A4 reads every codex row");
    assert!(a4.contains("model_auto_compact_token_limit=200000"));
    for (teammate, harness) in harnesses {
        let checked_by = if harness == "claude" { &a2 } else { &a4 };
        assert!(
            checked_by.contains(&format!(r#"$2 == "{harness}""#)),
            "{teammate} ({harness}) has no step"
        );
    }
}

/// CTX-17: the live check has a reader step and a compaction step for
/// `claude` and for `codex`, and checks each by its marker.
#[test]
fn ctx_17_live_check_covers_claude_and_codex() {
    let s = script();
    let a5 = step(&s, "A5");
    assert!(a5.contains("A5 claude reader") && a5.contains("A5 codex reader"));
    assert!(a5.contains(r#""/context""#), "Claude's own total");
    assert!(a5.contains("tokens used"), "Codex's own line");
    let a6 = step(&s, "A6");
    assert!(
        a6.contains(r#"--resume "$ksid" "/compact""#),
        "claude compacts with no argument"
    );
    assert!(
        a6.contains(r#"codex exec resume "$cxsid" "/compact""#),
        "codex compacts"
    );
    assert!(
        a6.contains(r#""subtype":"compact_boundary""#),
        "the claude marker"
    );
    assert!(a6.contains(r#""type":"compacted""#), "the codex marker");
    assert!(s.contains("B1 Claude worker round trip") && s.contains("B2 Codex worker round trip"));
}

/// CTX-17 slice 2, CTX-20: part C of the live check has a reader step for
/// `opencode`, `pi` and `prime` against the harness's own number, a canary
/// for pi and Prime by their compaction marker, the Prime agent dir check,
/// and the pane steps for `opencode-pickle`, `pi` and `prime`: herdr
/// `agent_status` busy and idle, `horch note`, and the compaction round trip.
#[test]
fn ctx_17_live_check_covers_slice_two_harnesses() {
    let s = script();
    let c2 = step(&s, "C2");
    for reader in ["C2 pi reader", "C2 prime reader", "C2 opencode reader"] {
        assert!(c2.contains(reader), "part C has {reader}");
    }
    assert!(c2.contains("totalTokens"), "the pi and Prime own number");
    assert!(c2.contains("step_finish"), "the OpenCode own number");
    let c3 = step(&s, "C3");
    for canary in ["C3 pi canary", "C3 prime canary"] {
        assert!(c3.contains(canary), "part C has {canary}");
    }
    let part_c = &s[s
        .find("# ---- Part C")
        .expect("scripts/live/context.sh has no part C")..];
    assert!(
        part_c.contains("KEEP-CANARY-"),
        "the first pi and Prime turn plants a canary"
    );
    assert!(
        c3.contains(r#"{"type":"compact"}"#),
        "compacts with no argument"
    );
    assert!(
        c3.contains(r#""type":"compaction""#),
        "the pi and Prime marker"
    );
    for result in ["honoured", "ignored", "not verifiable"] {
        assert!(c3.contains(result), "C3 prints {result}");
    }
    let c4 = step(&s, "C4");
    assert!(
        c4.contains("PRIME_AGENT_CODING_AGENT_DIR"),
        "the pane's agent dir"
    );
    assert!(c4.contains("autoRefine"), "auto-refine off");
    assert!(c4.contains("prime-agent model list"), "Prime's own window");
    let c5 = step(&s, "C5");
    assert!(
        c5.contains("for h in opencode-pickle pi prime") && c5.contains("horch spawn $h "),
        "C5 spawns opencode-pickle, pi and prime"
    );
    assert!(c5.contains("agent_status"), "herdr agent_status");
    assert!(
        c5.contains("working") && c5.contains("idle"),
        "busy and idle"
    );
    assert!(c5.contains("horch note"), "horch note in the pane");
    assert!(c5.contains("round_trip"), "the compaction round trip");
}

/// CTX-20: the live check plants a canary in the compact-instructions block
/// and reports `honoured`, `ignored` or `not verifiable`.
#[test]
fn ctx_20_live_check_plants_a_canary() {
    let a6 = step(&script(), "A6");
    assert!(a6.contains("KEEP-CANARY-"), "plants a canary");
    assert!(
        a6.contains("== Compact instructions =="),
        "in a copy of the block"
    );
    assert!(
        a6.contains("isCompactSummary"),
        "searches the claude summary"
    );
    for result in ["honoured", "ignored", "not verifiable"] {
        assert!(a6.contains(result), "prints {result}");
    }
}

/// CTX-15: step A8 proves that a detached child outlives a Claude Bash tool
/// call and a `codex exec` call.
#[test]
fn ctx_15_live_check_proves_detached_survival() {
    let a8 = step(&script(), "A8");
    assert!(a8.contains("start_new_session=True"), "a new-session child");
    assert!(a8.contains("beat.log"), "the beat file");
    assert!(
        a8.contains("--allowedTools 'Bash("),
        "started by a claude Bash tool call"
    );
    assert!(a8.contains("codex exec"), "started by a codex exec call");
    assert!(
        a8.contains(r#"survives "A8 survival after a claude"#)
            && a8.contains(r#"survives "A8 survival after a codex"#),
        "beat.log is checked after each call ends"
    );
    assert!(
        a8.contains("stop_beats"),
        "the children are killed at the end"
    );
}

/// The `.rs` files under `dir`, recursively.
fn rs_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            out.extend(rs_files(&path));
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
    out
}

/// CTX-27 (review finding 6): no CTX code reads, sets or passes
/// `ANTHROPIC_API_KEY`: every line that names it removes it. And
/// `Inherited::from_env` captures no `FORBIDDEN_ENV` name.
#[test]
fn ctx_27_no_api_key_in_context_policy_code() {
    let r = repo();
    let core = r.join("crates/horch-core/src");
    let mut files = rs_files(&core.join("compaction"));
    assert!(files.len() >= 4, "compaction/ has its modules");
    for rel in [
        "crates/horch-core/src/heartbeat.rs",
        "crates/horch-core/src/telemetry/context.rs",
        "crates/horch-core/src/runtime/context.rs",
        "crates/horch/src/cmd/context.rs",
        "crates/horch/src/cmd/compact.rs",
        "teammates/_base/context-windows.md",
        "teammates/_base/context-messages.md",
        "teammates/_base/harness-defaults.md",
        "scripts/live/context.sh",
    ] {
        files.push(r.join(rel));
    }
    let removes = ["unset", "env -u", "FORBIDDEN_ENV", "env_remove"];
    let mut found = Vec::new();
    for file in &files {
        let text =
            std::fs::read_to_string(file).unwrap_or_else(|e| panic!("{}: {e}", file.display()));
        for (n, line) in text.lines().enumerate() {
            if line.contains("ANTHROPIC_API_KEY") && !removes.iter().any(|m| line.contains(m)) {
                found.push(format!("{}:{}: {}", file.display(), n + 1, line.trim()));
            }
        }
    }
    assert!(
        found.is_empty(),
        "lines that keep the key:\n{}",
        found.join("\n")
    );

    let context = std::fs::read_to_string(core.join("runtime/context.rs")).unwrap();
    let start = context
        .find("pub fn from_env(env: &dyn EnvSource) -> Inherited {")
        .expect("Inherited::from_env");
    let body = &context[start..];
    let body = &body[..body
        .find("\n    }\n")
        .expect("the end of Inherited::from_env")];
    assert!(body.contains("env.var("), "{body}");
    for name in horch_core::harness::launch::FORBIDDEN_ENV {
        assert!(!body.contains(name), "Inherited::from_env reads {name}");
    }
}

/// The keys of 1 JSON object, in the order the text has them (serde_json
/// sorts a `Value`'s keys, so a `Value` cannot show the order), and the keys
/// of its `last_compaction` object.
struct Keys(Vec<String>, Option<Vec<String>>);

impl<'de> serde::Deserialize<'de> for Keys {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Keys, D::Error> {
        struct Visit;
        impl<'de> serde::de::Visitor<'de> for Visit {
            type Value = Keys;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("an object")
            }
            fn visit_map<M: serde::de::MapAccess<'de>>(self, mut m: M) -> Result<Keys, M::Error> {
                let (mut keys, mut mark) = (Vec::new(), None);
                while let Some(k) = m.next_key::<String>()? {
                    if k == "last_compaction" {
                        mark = m.next_value::<Option<Keys>>()?.map(|k| k.0);
                    } else {
                        m.next_value::<serde::de::IgnoredAny>()?;
                    }
                    keys.push(k);
                }
                Ok(Keys(keys, mark))
            }
        }
        d.deserialize_map(Visit)
    }
}

/// CTX-24: `horch context --json` on the fixture home, plus a pi session
/// whose last entry is a compaction: every key in order, unknown values
/// `null`. No herdr answers (`PATH` has none), so `pane` and `pane_status`
/// are `null` and the run still exits 0 with an empty stderr.
#[test]
fn ctx_24_json_has_every_key_and_nulls() {
    let w = World::new();
    let ledger = horch_core::execution::records::Ledger::for_project(w.root.join("state"), PROJECT);
    let mut records: Vec<serde_json::Value> =
        serde_json::from_str(&std::fs::read_to_string(ledger.path()).unwrap()).unwrap();
    let mut pi = record(
        "rec-pi",
        "pi-1",
        "pi",
        "pi",
        "aaaaaaaa-0000-4000-8000-000000000002",
    );
    pi["model"] = "ollama/qwen3.8".into();
    records.push(pi);
    std::fs::write(ledger.path(), serde_json::to_string(&records).unwrap()).unwrap();

    let out = w.stdout(&["context", "--json"], &[]);
    let keys: Vec<Keys> = serde_json::from_str(&out).unwrap();
    let rows: Vec<serde_json::Value> = serde_json::from_str(&out).unwrap();
    let want = [
        "role",
        "record_id",
        "harness",
        "model",
        "session_id",
        "transcript",
        "pane",
        "pane_status",
        "tokens",
        "provisional",
        "pending",
        "window",
        "native_trigger",
        "native_setting",
        "native_source",
        "native_detail",
        "threshold",
        "last_compaction",
        "native_compactions",
        "horch_compactions",
        "state",
        "reason",
        "route",
        "handoff",
    ];
    let roles: Vec<&str> = rows.iter().map(|r| r["role"].as_str().unwrap()).collect();
    assert_eq!(
        roles,
        [
            "orchestrator",
            "codex-sol-1",
            "opus-1",
            "pi-1",
            "sonnet-1",
            "sonnet-2"
        ],
        "{out}"
    );
    for k in &keys {
        assert_eq!(k.0, want, "{out}");
        if let Some(mark) = &k.1 {
            assert_eq!(mark, &["at", "trigger", "pre_tokens", "post_tokens"]);
        }
    }
    assert_eq!(keys.iter().filter(|k| k.1.is_some()).count(), 3, "{out}");
    let row = |role: &str| rows.iter().find(|r| r["role"] == role).unwrap();
    for r in &rows {
        assert_eq!(r["pane"], serde_json::Value::Null, "{r}");
        assert_eq!(r["pane_status"], serde_json::Value::Null, "{r}");
        let handoff = format!(
            "{PROJECT}/ai_docs/handoffs/{}-whats-next.md",
            r["role"].as_str().unwrap()
        );
        assert_eq!(r["handoff"], handoff.as_str(), "{r}");
    }

    let s1 = row("sonnet-1");
    assert_eq!(s1["record_id"], "rec-s1");
    assert_eq!(s1["harness"], "claude");
    assert_eq!(s1["session_id"], OVER);
    assert!(s1["transcript"]
        .as_str()
        .unwrap()
        .ends_with(&format!("{OVER}.jsonl")));
    assert_eq!(s1["tokens"], 311_225);
    assert_eq!(s1["provisional"], false);
    assert_eq!(s1["pending"], false);
    assert_eq!(s1["window"], 1_000_000);
    assert_eq!(s1["native_trigger"], 467_000);
    assert_eq!(s1["native_setting"], 500_000);
    assert_eq!(s1["native_source"], "operator");
    assert!(s1["native_detail"]
        .as_str()
        .unwrap()
        .ends_with("/.claude/settings.json"));
    assert_eq!(s1["threshold"], 300_000);
    assert_eq!(s1["last_compaction"], serde_json::Value::Null);
    assert_eq!(s1["state"], "over");
    assert_eq!(s1["reason"], serde_json::Value::Null);
    assert_eq!(s1["route"], "in-place");

    let o1 = row("opus-1");
    assert_eq!(o1["tokens"], 10_486);
    assert_eq!(o1["provisional"], true);
    assert_eq!(o1["pending"], true);
    assert_eq!(o1["last_compaction"]["at"], "2026-09-20T10:05:00.000Z");
    assert_eq!(o1["last_compaction"]["post_tokens"], 10_486);

    let pi = row("pi-1");
    assert_eq!(pi["harness"], "pi");
    assert_eq!(pi["tokens"], serde_json::Value::Null);
    assert_eq!(pi["pending"], true);
    assert_eq!(pi["provisional"], false);
    assert_eq!(pi["last_compaction"]["pre_tokens"], 201_500);
    assert_eq!(pi["last_compaction"]["trigger"], serde_json::Value::Null);
    assert_eq!(
        pi["last_compaction"]["post_tokens"],
        serde_json::Value::Null
    );
    assert_eq!(pi["route"], "fresh");

    let s2 = row("sonnet-2");
    assert_eq!(s2["state"], "no-transcript");
    assert_eq!(s2["transcript"], serde_json::Value::Null);
    assert_eq!(s2["tokens"], serde_json::Value::Null);
    assert_eq!(s2["provisional"], serde_json::Value::Null);
    assert_eq!(s2["pending"], serde_json::Value::Null);
    assert_eq!(s2["last_compaction"], serde_json::Value::Null);
    assert_eq!(s2["reason"], "no transcript found");

    let codex = row("codex-sol-1");
    assert_eq!(codex["native_setting"], 200_000);
    assert_eq!(codex["native_source"], "fleet");
    assert_eq!(
        codex["native_detail"],
        "context-policy windows codex/gpt-5.6-sol"
    );
}

impl World {
    /// Add `rec` to the ledger.
    fn add_record(&self, rec: serde_json::Value) {
        let ledger =
            horch_core::execution::records::Ledger::for_project(self.root.join("state"), PROJECT);
        let mut records: Vec<serde_json::Value> =
            serde_json::from_str(&std::fs::read_to_string(ledger.path()).unwrap()).unwrap();
        records.push(rec);
        std::fs::write(ledger.path(), serde_json::to_string(&records).unwrap()).unwrap();
    }

    /// The `--json` object of `role`.
    fn json_row(&self, role: &str) -> serde_json::Value {
        let out = self.stdout(&["context", "--json"], &[]);
        let rows: Vec<serde_json::Value> = serde_json::from_str(&out).unwrap();
        rows.into_iter()
            .find(|r| r["role"] == role)
            .unwrap_or_else(|| panic!("no row {role}: {out}"))
    }
}

/// The text table cell of `column` in the row of `role`. Columns are apart
/// by 2 spaces or more; a cell holds 1 space at most (`operator (now)`).
fn cell(table: &str, role: &str, column: &str) -> String {
    let split = |line: &str| -> Vec<String> {
        line.split("  ")
            .map(str::trim)
            .filter(|c| !c.is_empty())
            .map(String::from)
            .collect()
    };
    let mut lines = table.lines();
    let head = split(lines.next().unwrap());
    let at = head.iter().position(|h| h == column).unwrap();
    let row = lines
        .find(|l| l.starts_with(&format!("{role} ")))
        .unwrap_or_else(|| panic!("no row {role}:\n{table}"));
    split(row)[at].clone()
}

/// CTX-21: a teammate's `compact_at` is its watch base. On a native trigger
/// of 467,000 a base of 60,000 gives the threshold 60,000 in `horch
/// context`, in `--json` and in the `horch note` check, which all build the
/// row with 1 function.
#[test]
fn ctx_21_compact_at_sets_base() {
    let w = World::new();
    let dir = w.root.join("home/.config/horch/teammates");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("low-base.md"),
        "---\nname: low-base\nbrief_description: a teammate with its own watch base\n\
         hidden: true\nbase: fleet-worker\nagent: claude\nmodel: opus\ncompact_at: 60000\n---\nbody\n",
    )
    .unwrap();
    // SMALL holds 33,352 tokens: under 60,000.
    w.add_record(record("rec-low", "low-1", "claude", "low-base", SMALL));
    let mut big = record("rec-big", "low-2", "claude", "low-base", OVER);
    big["updated_at"] = "2026-10-06T09:01:00Z".into();
    w.add_record(big);

    let table = w.stdout(&["context"], &[]);
    assert_eq!(cell(&table, "low-1", "NATIVE"), "467,000", "\n{table}");
    assert_eq!(cell(&table, "low-1", "THRESHOLD"), "60,000", "\n{table}");
    assert_eq!(cell(&table, "low-1", "STATE"), "ok", "\n{table}");
    assert_eq!(cell(&table, "low-2", "THRESHOLD"), "60,000", "\n{table}");
    assert_eq!(cell(&table, "low-2", "STATE"), "over", "\n{table}");
    // A teammate with no `compact_at` keeps the fleet base.
    assert_eq!(
        cell(&table, "sonnet-1", "THRESHOLD"),
        "300,000",
        "\n{table}"
    );

    let row = w.json_row("low-2");
    assert_eq!(row["native_trigger"], 467_000);
    assert_eq!(row["threshold"], 60_000);

    let me = [("HORCH_RECORD_ID", "rec-big")];
    let warning = w.stdout(&["note", "step 1 done"], &me);
    assert!(
        warning.contains("Your context is 311225 tokens. Your threshold is 60000 tokens."),
        "{warning}"
    );
    let me = [("HORCH_RECORD_ID", "rec-low")];
    assert_eq!(
        w.stdout(&["note", "step 1 done"], &me),
        "",
        "33,352 is under 60,000"
    );

    let windows = w.stdout(&["context", "--windows"], &[]);
    assert_eq!(
        cell(&windows, "low-base", "THRESHOLD"),
        "60,000",
        "\n{windows}"
    );
}

/// CTX-25: `horch context` counts the compactions of a session as native
/// and horch-driven. opus-1's transcript has 1 marker at 10:05:00; a
/// `compacted` event at 10:06:00 makes it horch-driven. The orchestrator's
/// marker has no event: native.
#[test]
fn ctx_25_cli_counts_native_and_horch_compactions() {
    let w = World::new();
    let ledger = horch_core::execution::records::Ledger::for_project(w.root.join("state"), PROJECT);
    let mut records: Vec<serde_json::Value> =
        serde_json::from_str(&std::fs::read_to_string(ledger.path()).unwrap()).unwrap();
    let opus = records.iter_mut().find(|r| r["role"] == "opus-1").unwrap();
    opus["history"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!(
            {"at": "2026-09-20T10:06:00Z", "event": "compacted", "text": "marker at 10:05"}
        ));
    std::fs::write(ledger.path(), serde_json::to_string(&records).unwrap()).unwrap();

    let table = w.stdout(&["context"], &[]);
    assert_eq!(cell(&table, "opus-1", "COMPACTIONS"), "0n/1h", "\n{table}");
    assert_eq!(
        cell(&table, "orchestrator", "COMPACTIONS"),
        "1n/0h",
        "\n{table}"
    );
    assert_eq!(
        cell(&table, "sonnet-1", "COMPACTIONS"),
        "0n/0h",
        "\n{table}"
    );
    assert_eq!(cell(&table, "sonnet-2", "COMPACTIONS"), "-", "\n{table}");

    let out = w.stdout(&["context", "--json"], &[]);
    let rows: Vec<serde_json::Value> = serde_json::from_str(&out).unwrap();
    let row = |role: &str| rows.iter().find(|r| r["role"] == role).unwrap().clone();
    assert_eq!(row("opus-1")["native_compactions"], 0);
    assert_eq!(row("opus-1")["horch_compactions"], 1);
    assert_eq!(row("orchestrator")["native_compactions"], 1);
    assert_eq!(row("orchestrator")["horch_compactions"], 0);
    assert_eq!(
        row("sonnet-2")["native_compactions"],
        serde_json::Value::Null
    );
    assert_eq!(
        row("sonnet-2")["horch_compactions"],
        serde_json::Value::Null
    );
}
