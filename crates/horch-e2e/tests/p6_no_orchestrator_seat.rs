//! Phase 6 removes the orchestrator seat (SR-8, FM-32; ticket 44 of
//! fleetd-p6): no agent has operator rights. A fake agent in a live attempt
//! of the old seat (a null task and the persona `orchestrator`) sends an
//! `operator` request on the channel and as a request file. fleetd refuses
//! both, and the `requests` row records the refusal and its reason.
//!
//! The `fleet.toml` of each test still names the old seat in
//! `orchestrator_seats`: the key gives 1 warning line and no right (Q19 of
//! `docs/tickets/fleetd-p6/MAP.md`).
//!
//! Each test starts 1 real `horch fleetd` on its own temporary
//! `HORCH_DATA_DIR`, `HORCH_LEDGER_DB` and `FLEET_ROOT`, with 1 pass each
//! second, and waits for an event (a row, a file), never for a time. The
//! live attempt is a stand-in wrapper: a `sleep` that leads its own process
//! group, recorded as the launcher records a wrapper. The fake agent is
//! this test binary, started again in that group with its act in its
//! environment ([`agent_child`]), as in `p5_operator_boundary.rs`.
//!
//! Unix only: the channel is a Unix socket.

#![cfg(unix)]

use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use horch_core::fleetd::fleet_runtime::adapters::request_wire::Request;
use horch_core::fleetd::fleet_runtime::adapters::token_file;
use horch_core::fleetd::fleet_runtime::application::ledger::Cause;
use horch_core::fleetd::fleet_runtime::domain::requests::{Kind, RequestId};
use horch_core::fleetd::integration::adapters::sqlite::{self as integration, NewFeature};
use horch_core::fleetd::worker_profiles::domain::operator_boundary::OperatorPaths;
use horch_core::taskledger::model::{Actor, WorkerKind};
use horch_core::taskledger::runs::{self, Run};
use horch_core::taskledger::workers::{self, NewWorker};
use horch_e2e::fleetd::Fleetds;
use horch_e2e::harness::{wait_for, wait_for_or, Harness};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde_json::{json, Value};

// ─── the fake agent ─────────────────────────────────────────────────────

/// The act of the fake agent: `connect` or `file`.
const ACT_ENV: &str = "HORCH_P6_ACT";
/// The path of the act: the channel, or the request file.
const PATH_ENV: &str = "HORCH_P6_PATH";
/// The request line of the act, with no `\n`.
const LINE_ENV: &str = "HORCH_P6_LINE";

/// The start of the line of the fake agent that says what it got. libtest
/// prints its own text on the same stream.
const RESULT: &str = "p6-agent: ";

/// The name of the test that is the fake agent, for `--exact`.
const AGENT_CHILD: &str = "agent_child";

/// The fake agent: with no act in its environment it does nothing, so a
/// normal run of this file passes it. A test starts this binary again with
/// an act. It prints what it got and exits 0, or prints the error and exits
/// 1.
///
/// - `connect`: connect to the channel, send the line, read the reply.
/// - `file`: write the line as `<path>.tmp` with mode 0600, then rename it
///   to `<path>`, as the client of the channel writes a request file.
#[test]
fn agent_child() {
    let Ok(act) = std::env::var(ACT_ENV) else {
        return;
    };
    let path = PathBuf::from(std::env::var(PATH_ENV).unwrap_or_default());
    let line = std::env::var(LINE_ENV).unwrap_or_default();
    let done = match act.as_str() {
        "connect" => send_line(&path, &line),
        "file" => write_request_file(&path, &line),
        other => Err(format!("unknown act {other}")),
    };
    match done {
        Ok(text) => {
            println!("\n{RESULT}{text}");
            std::process::exit(0);
        }
        Err(e) => {
            eprintln!("\n{RESULT}{e}");
            std::process::exit(1);
        }
    }
}

fn send_line(channel: &Path, line: &str) -> Result<String, String> {
    let stream = std::os::unix::net::UnixStream::connect(channel)
        .map_err(|e| format!("connect {}: {e}", channel.display()))?;
    let mut writer = &stream;
    writer
        .write_all(format!("{line}\n").as_bytes())
        .and_then(|()| writer.flush())
        .map_err(|e| format!("send: {e}"))?;
    let mut reply = String::new();
    BufReader::new(&stream)
        .read_line(&mut reply)
        .map_err(|e| format!("reply: {e}"))?;
    Ok(reply.trim_end().to_string())
}

fn write_request_file(path: &Path, line: &str) -> Result<String, String> {
    let tmp = PathBuf::from(format!("{}.tmp", path.display()));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&tmp)
        .map_err(|e| format!("open {}: {e}", tmp.display()))?;
    file.write_all(format!("{line}\n").as_bytes())
        .and_then(|()| file.sync_all())
        .map_err(|e| format!("write {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("rename to {}: {e}", path.display()))?;
    Ok(format!("wrote {}", path.display()))
}

/// What the fake agent printed: what it got, or the error.
fn agent_text(out: &std::process::Output) -> String {
    let stream = if out.status.success() {
        &out.stdout
    } else {
        &out.stderr
    };
    String::from_utf8_lossy(stream)
        .lines()
        .find_map(|l| l.strip_prefix(RESULT))
        .unwrap_or_default()
        .to_string()
}

// ─── the world of a test ────────────────────────────────────────────────

/// The persona of the old orchestrator seat.
const OLD_SEAT: &str = "orchestrator";

/// The feature of each test.
const FEATURE: &str = "F-0001";

/// The `fleet.toml` of a test: 1 pass each second, the longest heartbeat
/// age, and the retired key that named the old seat.
fn fleet_toml() -> String {
    format!(
        "pass_period_s = 1\nheartbeat_max_age_s = 300\norchestrator_seats = [\"{OLD_SEAT}\"]\n"
    )
}

/// A row of the `requests` table.
#[derive(Debug)]
struct RequestRow {
    kind: String,
    class: String,
    state: String,
    auth: Option<String>,
    reason: Option<String>,
}

/// The live attempt of the old seat: its id and its stand-in wrapper.
struct Attempt {
    id: String,
    wrapper: u32,
}

/// A harness with a git repository, a real fleetd and 1 feature run.
struct World {
    /// Dropped before `h`: the teardown of the harness fails a test whose
    /// processes still run.
    fleetds: Fleetds,
    h: Harness,
    run: Run,
    wrappers: Vec<Child>,
}

impl Drop for World {
    fn drop(&mut self) {
        for child in &mut self.wrappers {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl World {
    fn new(name: &str) -> World {
        let mut h = Harness::new(name).with_git();
        let db = h.root.join("ledger.sqlite3");
        let fleet = h.root.join("fleet");
        let data = h.root.join("data");
        for dir in [&fleet, &data] {
            std::fs::create_dir_all(dir).unwrap();
        }
        std::fs::write(fleet.join("fleet.toml"), fleet_toml()).unwrap();
        h.set("HORCH_LEDGER_DB", db.to_string_lossy());
        h.set("HORCH_DATA_DIR", data.to_string_lossy());
        h.set("FLEET_ROOT", fleet.to_string_lossy());
        h.set("HORCH_NO_WAKE", "1");
        h.set("HORCH_NO_WATCHDOG", "1");
        let mut fleetds = Fleetds::new(&h);
        fleetds.start(&h, "fleetd");
        let paths = OperatorPaths::of(&fleet);
        wait_for_or(
            "fleetd to make the token, the channel and the request folder",
            || {
                (paths.token.is_file() && paths.channel.exists() && paths.requests.is_dir())
                    .then_some(())
            },
            || fleetds.output("fleetd", "stderr"),
        );
        let run = new_feature_run(&open(&db), &h.project);
        World {
            fleetds,
            h,
            run,
            wrappers: Vec::new(),
        }
    }

    fn fleet_root(&self) -> PathBuf {
        self.h.root.join("fleet")
    }

    fn paths(&self) -> OperatorPaths {
        OperatorPaths::of(&self.fleet_root())
    }

    fn conn(&self) -> Connection {
        open(&self.h.root.join("ledger.sqlite3"))
    }

    /// A live attempt of the old seat: a worker of kind `orchestrator` with
    /// the persona [`OLD_SEAT`] and a null task. Its stand-in wrapper is a
    /// `sleep` that leads a new process group.
    fn old_seat_attempt(&mut self) -> Attempt {
        let wrapper = Command::new("/bin/sleep")
            .arg("3600")
            .process_group(0)
            .stdin(Stdio::null())
            .spawn()
            .unwrap();
        let pid = wrapper.id();
        self.wrappers.push(wrapper);
        let start = wait_for("the start time of the wrapper", || {
            horch_platform::process::start_time(pid)
        });
        let mut conn = self.conn();
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        let worker = workers::insert(
            &tx,
            &NewWorker {
                run_id: self.run.run_id.clone(),
                kind: Some(WorkerKind::Orchestrator),
                persona: OLD_SEAT.into(),
                ..Default::default()
            },
        )
        .unwrap();
        tx.execute(
            "UPDATE workers SET status = 'working' WHERE worker_id = ?1",
            [&worker],
        )
        .unwrap();
        let id = format!("A-{worker}");
        tx.execute(
            "INSERT INTO attempts (id, task_id, worker_id, pid, pid_start, state, started_at)
             VALUES (?1, NULL, ?2, ?3, ?4, 'live', '2026-11-18T12:00:00Z')",
            params![id, worker, pid, i64::try_from(start).unwrap()],
        )
        .unwrap();
        tx.commit().unwrap();
        Attempt { id, wrapper: pid }
    }

    /// Run the fake agent with `act` on `path` and `line` in the process
    /// group of the attempt `a`. Gives what it printed; it must succeed.
    fn agent(&self, a: &Attempt, act: &str, path: &Path, line: &str) -> String {
        let out = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", AGENT_CHILD, "--nocapture", "--test-threads", "1"])
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env(ACT_ENV, act)
            .env(PATH_ENV, path)
            .env(LINE_ENV, line)
            .process_group(i32::try_from(a.wrapper).unwrap())
            .stdin(Stdio::null())
            .output()
            .unwrap();
        let text = agent_text(&out);
        assert!(out.status.success(), "the {act} act reaches fleetd: {text}");
        text
    }

    /// Wait until the request `id` is decided, and give its row.
    fn decided(&self, id: &RequestId) -> RequestRow {
        wait_for_or(
            &format!("fleetd to decide the request {}", id.as_str()),
            || {
                self.conn()
                    .query_row(
                        "SELECT kind, class, state, auth, reason FROM requests
                         WHERE request_id = ?1",
                        [id.as_str()],
                        |r| {
                            Ok(RequestRow {
                                kind: r.get(0)?,
                                class: r.get(1)?,
                                state: r.get(2)?,
                                auth: r.get(3)?,
                                reason: r.get(4)?,
                            })
                        },
                    )
                    .optional()
                    .unwrap()
                    .filter(|r| r.state != "pending")
            },
            || self.fleetds.output("fleetd", "stderr"),
        )
    }

    /// Each attempt with its state: the refused request stops no attempt.
    fn attempt_states(&self) -> Vec<String> {
        let conn = self.conn();
        let mut stmt = conn
            .prepare("SELECT id || ' ' || state FROM attempts ORDER BY id")
            .unwrap();
        let rows = stmt.query_map([], |r| r.get::<_, String>(0)).unwrap();
        rows.map(Result::unwrap).collect()
    }

    /// SR-8 and Q19: fleetd printed 1 warning line for the retired key.
    fn assert_1_warning_line(&self) {
        let stderr = self.fleetds.output("fleetd", "stderr");
        let lines: Vec<&str> = stderr
            .lines()
            .filter(|l| l.contains("orchestrator_seats"))
            .collect();
        assert_eq!(lines.len(), 1, "1 warning line: {stderr}");
        assert!(lines[0].contains("warning"), "{stderr}");
    }
}

/// The feature [`FEATURE`] of `project` and its internal run.
fn new_feature_run(conn: &Connection, project: &Path) -> Run {
    let tx = rusqlite::Transaction::new_unchecked(conn, TransactionBehavior::Immediate).unwrap();
    let feature = runs::create_feature_run(&tx, project, "main").unwrap();
    assert_eq!(feature, FEATURE, "the first run of the ledger");
    let new = NewFeature {
        id: FEATURE,
        name: Some(FEATURE),
        branch: Some("fleet/F-0001"),
        primary_branch: "main",
    };
    let cause = Cause {
        actor: Actor::Horch,
        text: "the test adds the feature",
    };
    integration::insert_feature(&tx, None, &new, cause).unwrap();
    let run = runs::feature_run(&tx, FEATURE).unwrap().unwrap();
    tx.commit().unwrap();
    run
}

fn open(db: &Path) -> Connection {
    let conn = Connection::open(db).unwrap();
    conn.busy_timeout(Duration::from_secs(30)).unwrap();
    conn
}

/// The request line of `ledger_gc` with `token` if any, with no `\n`.
fn gc_line(id: &RequestId, token: Option<String>) -> String {
    let Value::Object(payload) = json!({ "deleted": [] }) else {
        unreachable!("a payload is an object");
    };
    Request {
        request_id: id.clone(),
        kind: Kind::LedgerGc,
        payload,
        token,
    }
    .to_line()
    .trim_end()
    .to_string()
}

/// The row records a refused operator request, its reason and no `auth`.
fn assert_refused(row: &RequestRow, reason_has: &str) {
    assert_eq!(row.kind, "ledger_gc", "{row:?}");
    assert_eq!(row.class, "operator", "{row:?}");
    assert_eq!(row.state, "refused", "{row:?}");
    assert_eq!(row.auth, None, "a refused request has no auth: {row:?}");
    let reason = row.reason.as_deref().unwrap_or_default();
    assert!(reason.contains(reason_has), "the reason names `{reason_has}`: {row:?}");
}

// ─── the tests ──────────────────────────────────────────────────────────

/// The worst case on the channel: the agent of the old seat has a copy of
/// the valid token. fleetd finds it in a live attempt and refuses it.
#[test]
fn old_seat_operator_request_on_the_channel_is_refused() {
    let mut w = World::new("p6-old-seat-channel");
    let a = w.old_seat_attempt();
    let token = token_file::read(&w.fleet_root()).unwrap().text().to_string();
    let id = RequestId::parse("p6-old-seat-channel").unwrap();
    let reply = w.agent(&a, "connect", &w.paths().channel, &gc_line(&id, Some(token)));
    let reply: Value = serde_json::from_str(&reply).unwrap();
    assert_eq!(reply["state"], "refused", "{reply}");

    let row = w.decided(&id);
    assert_refused(&row, &format!("the peer process is in live attempt {}", a.id));
    assert!(
        row.reason.as_deref().unwrap_or_default().contains(OLD_SEAT),
        "the reason names the persona of the attempt: {row:?}"
    );
    assert_eq!(w.attempt_states(), [format!("{} live", a.id)]);
    w.assert_1_warning_line();
}

/// The agent of the old seat has no token, so its request file has none.
/// fleetd refuses it for the token.
#[test]
fn old_seat_operator_request_file_is_refused() {
    let mut w = World::new("p6-old-seat-file");
    let a = w.old_seat_attempt();
    let id = RequestId::parse("p6-old-seat-file").unwrap();
    let path = w.paths().requests.join(format!("{}.json", id.as_str()));
    w.agent(&a, "file", &path, &gc_line(&id, None));

    let row = w.decided(&id);
    assert_refused(&row, "an operator kind needs the valid operator token");
    assert_eq!(w.attempt_states(), [format!("{} live", a.id)]);
    w.assert_1_warning_line();
}
