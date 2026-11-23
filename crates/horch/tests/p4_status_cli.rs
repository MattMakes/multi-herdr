#![cfg(unix)]

use std::path::PathBuf;
use std::process::Command;

use horch_core::taskledger::runs::{self, NewRun};
use horch_core::taskledger::Store;

const HEALTHY_MACHINE: &str = r#"{"os":"linux","arch":"x86_64","cpus":8,"load_average_1m":1.0,"mem_total_bytes":17179869184,"mem_available_bytes":8589934592,"disk_free_bytes":214748364800,"disk_total_bytes":536870912000,"gpu":"none","max_open_files":1048576,"max_processes":100000}"#;

const PATH: &str = "/usr/bin:/bin:/usr/local/bin:/opt/homebrew/bin";

const AT: &str = "2026-11-01T00:00:00Z";

struct World {
    _tmp: tempfile::TempDir,
    root: PathBuf,
}

impl World {
    fn new() -> World {
        let tmp = tempfile::tempdir().unwrap();
        let root = horch_platform::path::canonicalize(tmp.path()).unwrap();
        let w = World { _tmp: tmp, root };
        let proj = w.proj();
        std::fs::create_dir_all(&proj).unwrap();
        std::fs::create_dir_all(w.root.join("home")).unwrap();
        w.git(&["init", "-q", "-b", "main"]);
        std::fs::write(proj.join("a.txt"), "a\n").unwrap();
        w.git(&["add", "a.txt"]);
        w.git(&["commit", "-q", "-m", "init"]);
        std::fs::write(w.root.join("machine.json"), HEALTHY_MACHINE).unwrap();
        std::fs::create_dir_all(w.root.join("fleet")).unwrap();
        std::fs::write(
            w.root.join("fleet/fleet.toml"),
            "health_max_process_share = 1.0\nhealth_max_open_file_share = 1.0\n",
        )
        .unwrap();
        let store = Store::open(&w.ledger()).unwrap();
        let new = NewRun {
            slug: "r1".into(),
            ..Default::default()
        };
        runs::create(store.conn(), &w.proj(), &new).unwrap();
        assert!(w.status().starts_with("Run r1 "), "{}", w.status());
        w
    }

    fn proj(&self) -> PathBuf {
        self.root.join("proj")
    }

    fn ledger(&self) -> PathBuf {
        self.root.join("ledger.sqlite3")
    }

    fn git(&self, args: &[&str]) {
        let out = Command::new("git")
            .args(args)
            .current_dir(self.proj())
            .env_clear()
            .env("PATH", PATH)
            .env("HOME", self.root.join("home"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "n")
            .env("GIT_AUTHOR_EMAIL", "a@b")
            .env("GIT_COMMITTER_NAME", "n")
            .env("GIT_COMMITTER_EMAIL", "a@b")
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?}");
    }

    fn horch(&self, args: &[&str]) -> String {
        let root = &self.root;
        let o = Command::new(env!("CARGO_BIN_EXE_horch"))
            .args(args)
            .current_dir(self.proj())
            .env_clear()
            .env("PATH", PATH)
            .env("HOME", root.join("home"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("XDG_DATA_HOME", root.join("data"))
            .env("HORCH_DATA_DIR", root.join("data"))
            .env("HORCH_STATE_DIR", root.join("state"))
            .env("HORCH_LEDGER_DB", self.ledger())
            .env("HORCH_PROJECT_DIR", self.proj())
            .env("HORCH_MACHINE_FILE", root.join("machine.json"))
            .env("FLEET_ROOT", root.join("fleet"))
            .env("HORCH_NO_WATCHDOG", "1")
            .output()
            .unwrap();
        let out = String::from_utf8(o.stdout).unwrap();
        assert!(
            matches!(o.status.code(), Some(0 | 1)),
            "{args:?}: {out}{}",
            String::from_utf8_lossy(&o.stderr)
        );
        out
    }

    fn status(&self) -> String {
        self.horch(&["ledger", "status"])
    }

    fn status_json(&self) -> serde_json::Value {
        serde_json::from_str(&self.horch(&["ledger", "status", "--json"])).unwrap()
    }

    fn sql(&self, sql: &str) {
        let store = Store::open(&self.ledger()).unwrap();
        store.conn().execute_batch(sql).unwrap();
    }

    fn needs_you(&self, kind: &str, id: &str, text: &str) {
        self.sql(&format!(
            "INSERT INTO needs_you (key, kind, entity, entity_id, state, text, created_at)
             VALUES ('{kind}:{id}', '{kind}', 'task', '{id}', 'open', '{text}', '{AT}');"
        ));
    }

    fn feature(&self, id: &str, name: &str, state: &str, push: &str, failures: i64) {
        self.sql(&format!(
            "INSERT INTO features (id, name, branch, primary_branch, state, push_state,
                push_failures, created_at, updated_at)
             VALUES ('{id}', '{name}', 'fleet/{name}', 'main', '{state}', {push},
                {failures}, '{AT}', '{AT}');"
        ));
    }

    fn bundle(&self, feature: &str, state: &str, candidate: &str) {
        self.sql(&format!(
            "INSERT INTO bundles (feature_id, base_tip, candidate_ref, state, due_at, at)
             VALUES ('{feature}', 'b0', {candidate}, '{state}', 0, '{AT}');"
        ));
    }

    fn worktree_list(&self, repo: &str, run: i64, state: &str, refs: &str) {
        let result = format!(
            r#"{{"passed":true,"verdict":true,"text":"listed","data":{{"rescue_refs":{refs}}}}}"#
        );
        self.sql(&format!(
            "INSERT INTO jobs (kind, subject, repo, input_key, run, state, time_limit_s,
                result_json, created_at)
             VALUES ('worktree_list', '{repo}', '{repo}', 'k', {run}, '{state}', 60,
                '{result}', '{AT}');"
        ));
    }
}

fn rescue_ref(name: &str, bytes: u64, in_newer_tip: bool, in_landed: bool) -> String {
    format!(
        r#"{{"name":"{name}","tip":"t","branch":null,"feature":null,"bytes":{bytes},"in_newer_tip":{in_newer_tip},"in_landed":{in_landed}}}"#
    )
}

fn has_line(board: &str, line: &str) -> bool {
    board.lines().any(|l| l == line)
}

#[test]
fn status_shows_missing_fast_gate_hold() {
    let w = World::new();
    assert!(!w.status().contains("missing fast gate"));
    w.needs_you(
        "missing_fast_gate",
        "P1",
        "fleetd holds new dispatch of the tasks of P1: missing fast gate.",
    );
    let board = w.status();
    assert!(
        has_line(
            &board,
            "Hold on dispatch of project P1: missing fast gate: add [gates] fast to fleet.toml in P1."
        ),
        "{board}"
    );
    assert!(board.contains("Needs you (1):"), "{board}");
}

#[test]
fn status_shows_persona_without_check_sandbox() {
    let w = World::new();
    w.needs_you("no_check_sandbox", "opus", "opus has no check sandbox.");
    let board = w.status();
    assert!(
        has_line(
            &board,
            "Profile opus: not available on this OS: no sandbox."
        ),
        "{board}"
    );
}

#[test]
fn status_shows_bundle_ref_count_and_rescue_classes() {
    let w = World::new();
    let board = w.status();
    assert!(!board.contains("Bundle refs"), "{board}");
    assert!(!board.contains("Rescue refs"), "{board}");
    w.feature("F-9001", "alpha", "open", "NULL", 0);
    w.bundle("F-9001", "gating", "'refs/horch/bundle/1'");
    w.bundle("F-9001", "failed", "'refs/horch/bundle/2'");
    w.bundle("F-9001", "landed", "NULL");
    let refs = format!(
        "[{},{},{}]",
        rescue_ref("refs/horch/rescue/w1/1", 2 * 1024 * 1024, false, false),
        rescue_ref("refs/horch/rescue/w1/2", 1024, true, false),
        rescue_ref("refs/horch/rescue/w2/1", 2048, false, true),
    );
    w.worktree_list("/r/a", 1, "passed", &refs);
    w.worktree_list("/r/a", 2, "queued", "[]");
    let board = w.status();
    assert!(has_line(&board, "Bundle refs: 2."), "{board}");
    assert!(
        has_line(
            &board,
            "Rescue refs: 1 unique (2.0 MB), 2 superseded (3.0 KB)."
        ),
        "{board}"
    );
}

#[test]
fn status_shows_stale_remote_since() {
    let w = World::new();
    w.sql(&format!(
        "INSERT INTO remote_tips (repo, branch, tip, fetched_at, stale_since) VALUES
            ('/r/a', 'main', 't1', '{AT}', '2026-11-02T03:04:05Z'),
            ('/r/a', 'fleet/alpha', 't2', '{AT}', NULL);"
    ));
    let board = w.status();
    assert!(
        has_line(
            &board,
            "Remote tip of main in /r/a: stale since 2026-11-02T03:04:05Z."
        ),
        "{board}"
    );
    assert!(!board.contains("fleet/alpha in /r/a"), "{board}");
}

#[test]
fn status_shows_the_push_state_of_each_feature() {
    let w = World::new();
    w.feature("F-9001", "alpha", "open", "NULL", 0);
    w.feature("F-9002", "beta", "ready", "'ok'", 0);
    w.feature("F-9003", "gamma", "open", "'failed'", 2);
    w.feature("F-9004", "delta", "merged", "'ok'", 0);
    let board = w.status();
    for line in [
        "Feature F-9001 (alpha) open: not pushed.",
        "Feature F-9002 (beta) ready: push ok.",
        "Feature F-9003 (gamma) open: push failed 2 times.",
    ] {
        assert!(has_line(&board, line), "{line}\n{board}");
    }
    assert!(!board.contains("F-9004"), "{board}");
}

#[test]
fn status_json_has_the_phase4_facts() {
    let w = World::new();
    w.needs_you("missing_fast_gate", "P1", "held");
    w.needs_you("no_check_sandbox", "opus", "no sandbox");
    w.feature("F-9001", "alpha", "open", "'failed'", 3);
    w.bundle("F-9001", "gating", "'refs/horch/bundle/1'");
    let refs = format!(
        "[{},{}]",
        rescue_ref("refs/horch/rescue/w1/1", 10, false, false),
        rescue_ref("refs/horch/rescue/w1/2", 20, true, true),
    );
    w.worktree_list("/r/a", 1, "passed", &refs);
    w.sql(&format!(
        "INSERT INTO remote_tips (repo, branch, tip, fetched_at, stale_since)
         VALUES ('/r/a', 'main', 't1', '{AT}', '2026-11-02T03:04:05Z');"
    ));
    let j = w.status_json();
    assert_eq!(j["no_check_sandbox"], serde_json::json!(["opus"]), "{j}");
    let i = &j["integration"];
    assert_eq!(
        i["holds"],
        serde_json::json!([{
            "project": "P1",
            "cause": "missing fast gate: add [gates] fast to fleet.toml in P1"
        }]),
        "{j}"
    );
    assert_eq!(i["bundle_refs"], 1, "{j}");
    assert_eq!(
        i["rescue"],
        serde_json::json!({
            "unique": {"count": 1, "bytes": 10},
            "superseded": {"count": 1, "bytes": 20}
        }),
        "{j}"
    );
    assert_eq!(
        i["features"],
        serde_json::json!([{
            "id": "F-9001",
            "name": "alpha",
            "state": "open",
            "push_state": "failed",
            "push_failures": 3
        }]),
        "{j}"
    );
    assert_eq!(
        i["stale_remotes"],
        serde_json::json!([{
            "repo": "/r/a",
            "branch": "main",
            "stale_since": "2026-11-02T03:04:05Z"
        }]),
        "{j}"
    );
}
