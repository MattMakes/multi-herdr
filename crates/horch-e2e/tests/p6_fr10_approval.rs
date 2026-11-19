//! FR-10 through the binary: no process of a fleet attempt can record an
//! approval, also after its attempt ended, and no agent outlives its
//! attempt (ticket 60 of fleetd-build, T-898; `docs/specs/fleetd/design.md`:
//! FR-10, SR-8, FM-32).
//!
//! Each test makes its own ledger, fleet root and git project in a temp
//! folder, and runs a test `horch fleetd` with the fake herdr and the fake
//! idea-lead of `horch_e2e::fr10_agent`. A word `[fr10:<key>]` in the text
//! of an idea gives the key of its fake.
//!
//! In each case the idea stays in `awaiting_approval`. A try through the
//! CLI is refused with a reason that names the caller as a fleet process.
//! A raw request with the operator token is refused by fleetd with such a
//! reason, and its `requests` row records the try (design, "The `requests`
//! row records the decision"). A forged `approval.json` is ignored, and 1
//! event of the idea records it.

#![cfg(unix)]

use std::path::PathBuf;
use std::process::Output;
use std::time::Duration;

use horch_e2e::harness::{fixtures, wait_for_or, Harness};
use rusqlite::{Connection, OpenFlags};
use serde_json::Value;

/// The `fleet.toml` of each test: 1 pass each second, and a short grace
/// before the kill of a stopped process group.
const FLEET_TOML: &str = "pass_period_s = 1\nstop_grace_s = 5\n";

/// The fleet clock ticks that a test waits for to see that fleetd leaves an
/// idea where it is.
const LATER_TICKS: i64 = 3;

/// What the reason of each refusal names.
const FLEET_PROCESS: &str = "fleet process";

/// The text of the idea that waits for the operator.
const WAITING_IDEA: &str = "Show the queue length in the status bar";

fn text(o: &Output) -> String {
    format!(
        "status: {}\nstdout: {}\nstderr: {}",
        o.status,
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

/// A fleet with a test fleetd, the herdr workspace of the project and a
/// git project with gates.
struct World {
    h: Harness,
}

impl World {
    /// The fleet, with `scenario` as the keys of each fake agent.
    fn new(name: &str, scenario: &str) -> World {
        let mut h = Harness::new(name).with_git();
        let fleet = h.root.join("fleet");
        std::fs::create_dir_all(&fleet).unwrap();
        std::fs::write(fleet.join("fleet.toml"), FLEET_TOML).unwrap();
        h.set(
            "HORCH_LEDGER_DB",
            h.root.join("ledger.sqlite3").to_string_lossy(),
        );
        h.set("FLEET_ROOT", fleet.to_string_lossy());
        h.set("HORCH_NO_WAKE", "1");
        h.set("HORCH_NO_WATCHDOG", "1");
        h.set(
            "HORCH_QUOTA_FILE",
            fixtures().join("quota/all-ok.json").to_string_lossy(),
        );
        let docs = h.root.join("docs-fixture");
        std::fs::create_dir_all(&docs).unwrap();
        h.set("HORCH_TEST_DOCS_FIXTURE", docs.to_string_lossy());
        // `exec`: each pane of the fake herdr runs its command.
        h.set("HORCH_FAKE_SCENARIO", format!("exec,{scenario}"));
        h.fleet_workspace();
        let w = World { h };
        // Plan validation refuses a plan when the project declares no gates.
        std::fs::write(
            w.h.project.join("fleet.toml"),
            "[gates]\nfast = \"true\"\nfull = \"true\"\n",
        )
        .unwrap();
        w.git(&["add", "-A"]);
        w.git(&["commit", "-q", "-m", "base"]);
        w.h.serve_fleetd();
        w
    }

    fn folder(&self, idea: &str) -> PathBuf {
        self.h.fleet_root().join("ideas").join(idea)
    }

    fn git(&self, args: &[&str]) {
        let o = self
            .h
            .git_cmd(args)
            .current_dir(&self.h.project)
            .output()
            .unwrap();
        assert!(o.status.success(), "git {}: {}", args.join(" "), text(&o));
    }

    /// `horch idea add <idea>`; the id of the new idea.
    fn add(&self, idea: &str) -> String {
        let o = self.h.run(&["idea", "add", idea]);
        assert!(o.status.success(), "horch idea add: {}", text(&o));
        self.wait("the idea file of the new idea", || {
            self.rows("SELECT id FROM ideas ORDER BY id")
                .into_iter()
                .find(|id| {
                    std::fs::read_to_string(self.folder(id).join("idea.md"))
                        .is_ok_and(|t| t.contains(idea))
                })
        })
    }

    /// The rows of `sql`, each row its columns joined with 1 space.
    fn rows(&self, sql: &str) -> Vec<String> {
        let path = self.h.root.join("ledger.sqlite3");
        let Ok(conn) = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY) else {
            return Vec::new();
        };
        let _ = conn.busy_timeout(Duration::from_secs(5));
        let Ok(mut stmt) = conn.prepare(sql) else {
            return Vec::new();
        };
        let n = stmt.column_count();
        stmt.query_map([], |r| {
            let mut cols = Vec::new();
            for i in 0..n {
                let v: rusqlite::types::Value = r.get(i)?;
                cols.push(match v {
                    rusqlite::types::Value::Null => "-".to_string(),
                    rusqlite::types::Value::Integer(i) => i.to_string(),
                    rusqlite::types::Value::Real(f) => f.to_string(),
                    rusqlite::types::Value::Text(t) => t,
                    rusqlite::types::Value::Blob(_) => "<blob>".to_string(),
                });
            }
            Ok(cols.join(" "))
        })
        .map(|rows| rows.filter_map(Result::ok).collect())
        .unwrap_or_default()
    }

    fn one(&self, sql: &str) -> Option<String> {
        self.rows(sql).into_iter().next()
    }

    fn idea_state(&self, idea: &str) -> Option<String> {
        self.one(&format!("SELECT state FROM ideas WHERE id = '{idea}'"))
    }

    fn fleet_clock(&self) -> Option<i64> {
        self.one("SELECT fleet_clock_s FROM fleetd WHERE id = 1")?
            .parse()
            .ok()
    }

    /// Wait until fleetd ran [`LATER_TICKS`] more passes.
    fn later_passes(&self) {
        let from = self.wait("the fleet clock", || self.fleet_clock());
        self.wait("later ticks of the fleet clock", || {
            (self.fleet_clock()? >= from + LATER_TICKS).then_some(())
        });
    }

    /// The first call of the fake `claude` that logged `key`, once it did.
    fn fake_call(&self, key: &str) -> Value {
        self.wait(&format!("the fake idea-lead to log {key}"), || {
            self.h
                .calls_of("claude")
                .into_iter()
                .find(|c| c.get(key).is_some())
        })
    }

    /// FR-10: the idea stays in `awaiting_approval` with no approval, and
    /// no `idea_approve` request is applied.
    fn assert_still_waiting(&self, idea: &str) {
        self.wait("the idea in awaiting_approval", || {
            (self.idea_state(idea).as_deref() == Some("awaiting_approval")).then_some(())
        });
        self.later_passes();
        assert_eq!(
            self.idea_state(idea).as_deref(),
            Some("awaiting_approval"),
            "{}",
            self.context()
        );
        assert_eq!(
            self.one(&format!(
                "SELECT IFNULL(approved_sha256, '-') FROM ideas WHERE id = '{idea}'"
            ))
            .as_deref(),
            Some("-"),
            "{}",
            self.context()
        );
        let moves = self.rows(&format!(
            "SELECT to_state FROM events WHERE entity = 'idea' AND entity_id = '{idea}'
               AND to_state IS NOT NULL"
        ));
        assert!(!moves.iter().any(|s| s == "specifying"), "{moves:?}");
        let applied = self.rows(
            "SELECT request_id FROM requests WHERE kind = 'idea_approve' AND state = 'applied'",
        );
        assert!(applied.is_empty(), "{applied:?}\n{}", self.context());
    }

    /// A try of the CLI is refused, and its reason names a fleet process.
    fn assert_cli_refused(&self, name: &str, tried: &Value) {
        assert_ne!(tried["exit"], 0, "{name}: {tried}\n{}", self.context());
        let err = tried["stderr"].as_str().unwrap_or_default();
        assert!(
            err.contains("refused inside a fleet attempt") && err.contains(FLEET_PROCESS),
            "{name}: {tried}"
        );
    }

    /// A raw request with the operator token is refused by fleetd: the
    /// reply and the `requests` row name a fleet process.
    fn assert_raw_refused(&self, tried: &Value) {
        let reply: Value = tried["reply"]
            .as_str()
            .and_then(|r| serde_json::from_str(r).ok())
            .unwrap_or(Value::Null);
        assert_eq!(reply["state"], "refused", "{tried}\n{}", self.context());
        let reason = reply["reason"].as_str().unwrap_or_default();
        assert!(reason.contains(FLEET_PROCESS), "{tried}");
        let id = tried["request_id"].as_str().unwrap();
        let row = self.wait("the row of the raw request", || {
            self.one(&format!(
                "SELECT state || ' ' || IFNULL(reason, '-') FROM requests WHERE request_id = '{id}'"
            ))
        });
        assert!(
            row.starts_with("refused ") && row.contains(FLEET_PROCESS),
            "{row}"
        );
    }

    /// What a failed wait shows.
    fn context(&self) -> String {
        let show =
            |title: &str, sql: &str| format!("{title}:\n  {}\n", self.rows(sql).join("\n  "));
        let mut calls: Vec<String> = self
            .h
            .calls_of("claude")
            .into_iter()
            .map(|c| c.to_string())
            .collect();
        calls.truncate(6);
        [
            show(
                "ideas",
                "SELECT id, state, IFNULL(approved_sha256, '-') FROM ideas",
            ),
            show(
                "idea events",
                "SELECT entity_id, kind, IFNULL(to_state, '-'), text FROM events
                 WHERE entity = 'idea' ORDER BY rowid",
            ),
            show(
                "requests",
                "SELECT request_id, kind, state, IFNULL(reason, '-') FROM requests ORDER BY rowid",
            ),
            show(
                "attempts",
                "SELECT id, IFNULL(pid, '-'), IFNULL(pid_start, '-'), state,
                        started_at, IFNULL(ended_at, '-'), IFNULL(outcome, '-')
                 FROM attempts ORDER BY rowid",
            ),
            format!("claude calls:\n  {}\n", calls.join("\n  ")),
            format!(
                "fleetd.stderr:\n{}\n",
                std::fs::read_to_string(self.h.root.join("fleetd.stderr")).unwrap_or_default()
            ),
        ]
        .concat()
    }

    fn wait<T>(&self, what: &str, f: impl FnMut() -> Option<T>) -> T {
        wait_for_or(what, f, || self.context())
    }
}

/// Whether process `pid` exists.
fn alive(pid: i64) -> bool {
    // SAFETY: signal 0 sends nothing; `pid` is a pid that a fake logged.
    unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
}

/// Step 1a: the agent of a live attempt runs `horch idea approve` of an
/// idea in `awaiting_approval`, and sends a raw `idea_approve` with the
/// operator token. Both are refused.
#[test]
fn fr10_a_agent_of_a_live_attempt_cannot_approve() {
    let w = World::new("p6-fr10-a", "clarify");
    let waiting = w.add(WAITING_IDEA);
    w.add("Show the disk use [fr10:approve_waiting]");
    let call = w.fake_call("tries");
    assert_eq!(call["target"], waiting.as_str(), "{}", w.context());
    let attempts = call["attempts"].to_string();
    assert!(
        attempts.contains(" live"),
        "the own attempt is live: {attempts}"
    );
    w.assert_cli_refused("cli", &call["tries"]["cli"]);
    w.assert_raw_refused(&call["tries"]["raw"]);
    w.assert_still_waiting(&waiting);
}

/// Step 1b: fleetd ended the attempt of the agent, and the agent still
/// runs: it ignores the polite stop. The operator approves the own idea of
/// the agent, so fleetd ends its `clarifying` attempt (an old phase) and
/// stops its process group. Within the grace, the agent tries to approve
/// another idea in `awaiting_approval`: both tries are refused. Then the
/// stop ladder kills it: no agent outlives its attempt.
#[test]
fn fr10_b_agent_after_its_attempt_ended_cannot_approve_and_is_stopped() {
    let w = World::new("p6-fr10-b", "clarify");
    let waiting = w.add(WAITING_IDEA);
    let own = w.add("Show the disk use [fr10:outlive]");
    for idea in [&waiting, &own] {
        w.wait("the idea in awaiting_approval", || {
            (w.idea_state(idea).as_deref() == Some("awaiting_approval")).then_some(())
        });
    }
    // The operator approves from outside the fleet: not refused.
    let o = w.h.run(&["idea", "approve", &own]);
    assert!(o.status.success(), "the operator approves: {}", text(&o));
    let call = w.fake_call("tries");
    assert_eq!(call["target"], waiting.as_str(), "{}", w.context());
    let attempts: Vec<String> = serde_json::from_value(call["attempts"].clone()).unwrap();
    assert!(
        !attempts.is_empty() && attempts.iter().all(|a| a.ends_with(" ended")),
        "the attempt ended before the tries: {attempts:?}\n{}",
        w.context()
    );
    w.assert_cli_refused("cli", &call["tries"]["cli"]);
    w.assert_raw_refused(&call["tries"]["raw"]);
    w.assert_still_waiting(&waiting);
    let pid = call["pid"].as_i64().unwrap();
    w.wait("the stop of the agent that outlived its attempt", || {
        (!alive(pid)).then_some(())
    });
}

/// Step 1c: a child shell of the agent, and a detached child of the agent
/// (a new session and a new process group, whose parent waits), each run
/// `horch idea approve`. Both are refused.
#[test]
fn fr10_c_child_shell_and_detached_child_cannot_approve() {
    let w = World::new("p6-fr10-c", "clarify");
    let waiting = w.add(WAITING_IDEA);
    w.add("Show the disk use [fr10:approve_waiting]");
    let call = w.fake_call("tries");
    w.assert_cli_refused("child_shell", &call["tries"]["child_shell"]);
    w.assert_cli_refused("detached", &call["tries"]["detached"]);
    w.assert_still_waiting(&waiting);
}

/// Step 1c, the hard case: a double fork. The child makes a new session
/// and a new process group, starts the grandchild and exits. The
/// grandchild runs `horch idea approve` with no parent in the attempt.
#[test]
fn fr10_c_double_fork_orphan_cannot_approve() {
    horch_e2e::expected::check(file!(), "fr10_c_double_fork_orphan_cannot_approve", || {
        let w = World::new("p6-fr10-c2", "clarify");
        let waiting = w.add(WAITING_IDEA);
        w.add("Show the disk use [fr10:orphan]");
        let call = w.fake_call("tries");
        w.assert_cli_refused("orphan", &call["tries"]["orphan"]);
        w.assert_still_waiting(&waiting);
    });
}

/// Step 1d: the agent writes `approval.json` itself, with the sha256 of
/// its `assumptions.md`. fleetd ignores it, and 1 event of the idea records
/// it.
#[test]
fn fr10_d_agent_written_approval_is_not_accepted() {
    let w = World::new("p6-fr10-d", "");
    let idea = w.add("Show the queue length [fr10:forge]");
    let call = w.fake_call("forged");
    assert_eq!(call["forged"], true, "{call}");
    assert!(w.folder(&idea).join("approval.json").is_file());
    w.assert_still_waiting(&idea);
    let ignored = w.rows(&format!(
        "SELECT text FROM events WHERE entity = 'idea' AND entity_id = '{idea}'
           AND text LIKE 'approval.json ignored%'"
    ));
    assert_eq!(ignored.len(), 1, "{ignored:?}\n{}", w.context());
}
