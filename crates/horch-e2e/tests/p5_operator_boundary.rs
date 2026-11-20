//! The SR-8 gate test of Phase 5 (`docs/specs/fleetd/design.md`, Migration
//! plan item 7; ticket 43 of fleetd-p5): a worker cannot send an operator
//! request in any way, and neither can a test of the project that runs in
//! a gate (SR-4, SR-5, SR-8, FM-32, Q2 and Q21 of
//! `docs/tickets/fleetd-p5/MAP.md`).
//!
//! Each test starts 1 real `horch fleetd` on its own temporary
//! `HORCH_DATA_DIR`, `HORCH_STATE_DIR`, `HORCH_LEDGER_DB` and `FLEET_ROOT`,
//! with 1 pass each second, and waits for an event (a row, a file, an
//! exit), never for a time. Each `horch` command of a test runs with
//! `HORCH_TEST_ASSERT_READ_ONLY=1`; fleetd and its job processes do not,
//! because `horch job run` still writes the rows of its job (T-883).
//!
//! The live attempt of a test is a stand-in wrapper: a `sleep` process that
//! leads its own process group, which the test records as the wrapper of a
//! live attempt, as the launcher does. Each attacker joins that group, or is
//! a child of the wrapper, so the peer process check of fleetd finds it in
//! the attempt. The attacker is this test binary, started again with the
//! act in its environment ([`attack_child`]). The worker sandbox is the
//! platform sandbox of ticket 06 with the 3 operator denials, so an attack
//! from inside it meets the OS first and fleetd second.
//!
//! Unix only: the channel is a Unix socket, and on Windows the platform
//! sandbox is `NotSupported`.

#![cfg(unix)]

use std::collections::BTreeSet;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::Duration;

use horch_core::fleetd::fleet_runtime::adapters::request_wire::Request;
use horch_core::fleetd::fleet_runtime::adapters::scans::requests_dir;
use horch_core::fleetd::fleet_runtime::adapters::token_file;
use horch_core::fleetd::fleet_runtime::application::jobs::{JobCtx, Queued};
use horch_core::fleetd::fleet_runtime::application::ledger::Cause;
use horch_core::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;
use horch_core::fleetd::fleet_runtime::domain::jobs::Kind as JobKind;
use horch_core::fleetd::fleet_runtime::domain::requests::{Class, Kind, RequestId};
use horch_core::fleetd::ideas::application::needs_you::NeedsYouList;
use horch_core::fleetd::integration::adapters::exec_gate::{self, GateRequest};
use horch_core::fleetd::integration::adapters::sqlite::{
    self as integration, NewBranch, NewBundle, NewFeature,
};
use horch_core::fleetd::worker_profiles::domain::operator_boundary::OperatorPaths;
use horch_core::fleetd::worktree_custody::adapters::sqlite::WorktreeState;
use horch_core::fleetd::worktree_custody::application::gate_tree::{
    self, GateTree, GateTreeRequest,
};
use horch_core::taskledger::model::{Actor, WorkerKind};
use horch_core::taskledger::runs::{self, Run};
use horch_core::taskledger::store::ASSERT_READ_ONLY_ENV;
use horch_core::taskledger::tasks::{self, NewTask};
use horch_core::taskledger::workers::{self, NewWorker};
use horch_e2e::fleetd::Fleetds;
use horch_e2e::harness::{wait_for, wait_for_or, Harness};
use horch_platform::sandbox::{sandboxed, Denials};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde_json::{json, Value};

// ─── the attacker ───────────────────────────────────────────────────────

/// The act of the attacker: `read`, `connect` or `file`.
const ACT_ENV: &str = "HORCH_P5_ACT";
/// The path of the act: the token file, the channel, or the request file.
const PATH_ENV: &str = "HORCH_P5_PATH";
/// The request line of a `connect` or a `file` act, with no `\n`.
const LINE_ENV: &str = "HORCH_P5_LINE";
/// With `1`, the attacker leaves the process group of its parent first
/// (`setsid`), so only its parent chain holds it in the attempt.
const NEW_SESSION_ENV: &str = "HORCH_P5_NEW_SESSION";

/// The start of the line of the attacker that says what it got. libtest
/// prints its own text on the same stream.
const RESULT: &str = "p5-attack: ";

/// The name of the test that is the attacker, for `--exact`.
const ATTACK_CHILD: &str = "attack_child";

/// The attacker: with no act in its environment it does nothing, so a
/// normal run of this file passes it. A test starts this binary again with
/// an act. It exits 0 when the act succeeds and prints what it got, else it
/// prints the error and exits 1.
///
/// - `read`: read the token file. An empty read is a refusal: the Linux
///   sandbox puts an empty file over the token.
/// - `connect`: connect to the channel, send the line, read the reply.
/// - `file`: write the line as `<path>.tmp` with mode 0600, then rename it
///   to `<path>`, as the client of the channel writes a request file.
#[test]
fn attack_child() {
    let Ok(act) = std::env::var(ACT_ENV) else {
        return;
    };
    if std::env::var(NEW_SESSION_ENV).as_deref() == Ok("1") {
        // SAFETY: setsid has no memory effect; this process leads no group.
        unsafe { libc::setsid() };
    }
    let path = PathBuf::from(std::env::var(PATH_ENV).unwrap_or_default());
    let line = std::env::var(LINE_ENV).unwrap_or_default();
    let done = match act.as_str() {
        "read" => match std::fs::read(&path) {
            Ok(bytes) if bytes.is_empty() => Err("the read gave no bytes".to_string()),
            Ok(_) => Ok("the read gave the token".to_string()),
            Err(e) => Err(format!("read {}: {e}", path.display())),
        },
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

/// The command that starts this binary as the attacker of `act` on `path`
/// with `line`. It inherits no variable of the operator but `PATH`: the
/// Linux sandbox finds `bwrap` there.
fn attacker(act: &str, path: &Path, line: &str) -> Command {
    let mut cmd = Command::new(std::env::current_exe().unwrap());
    cmd.args([
        "--exact",
        ATTACK_CHILD,
        "--nocapture",
        "--test-threads",
        "1",
    ])
    .env_clear()
    .env("PATH", std::env::var_os("PATH").unwrap_or_default())
    .env(ACT_ENV, act)
    .env(PATH_ENV, path)
    .env(LINE_ENV, line);
    cmd
}

/// What 1 attack did.
#[derive(Debug)]
struct Attack {
    succeeded: bool,
    /// The last line that the attacker printed: what it got, or the error.
    text: String,
}

fn attack_of(out: &Output) -> Attack {
    let stream = if out.status.success() {
        &out.stdout
    } else {
        &out.stderr
    };
    let text = String::from_utf8_lossy(stream)
        .lines()
        .find_map(|l| l.strip_prefix(RESULT))
        .unwrap_or_default()
        .to_string();
    Attack {
        succeeded: out.status.success(),
        text,
    }
}

// ─── requests ───────────────────────────────────────────────────────────

/// Each `operator` kind of the catalog (the table "Request kinds" of the
/// MAP, after the kinds that Phase 6 retired).
fn operator_kinds() -> Vec<Kind> {
    Kind::ALL
        .into_iter()
        .filter(|k| k.class() == Class::Operator)
        .collect()
}

/// A request id of `[A-Za-z0-9-]` from `parts`.
fn request_id(parts: &[&str]) -> RequestId {
    RequestId::parse(&parts.join("-").replace('_', "-")).unwrap()
}

/// The request line of `kind` with `payload`, and `token` if any, with no
/// `\n`.
fn line(id: &RequestId, kind: Kind, payload: Value, token: Option<&str>) -> String {
    let Value::Object(payload) = payload else {
        panic!("a payload is an object");
    };
    Request {
        request_id: id.clone(),
        kind,
        payload,
        token: token.map(str::to_string),
    }
    .to_line()
    .trim_end()
    .to_string()
}

/// 1 row of the `requests` table.
#[derive(Debug, Clone, PartialEq)]
struct RequestRow {
    id: String,
    kind: String,
    class: String,
    state: String,
    auth: Option<String>,
    reason: Option<String>,
}

/// SR-8: the row of a refused operator request records the refusal with its
/// reason and a null `auth`.
fn assert_refused(row: &RequestRow, reason_has: &str) {
    assert_eq!(row.state, "refused", "{row:?}");
    assert_eq!(row.class, "operator", "{row:?}");
    assert_eq!(row.auth, None, "a refused request has no auth: {row:?}");
    let reason = row.reason.as_deref().unwrap_or_default();
    assert!(
        reason.contains(reason_has),
        "the reason names `{reason_has}`: {row:?}"
    );
}

// ─── the world of a test ────────────────────────────────────────────────

/// The feature of each test, and its branch.
const FEATURE: &str = "F-0001";
const FEATURE_BRANCH: &str = "fleet/F-0001";

/// The persona of the old orchestrator seat, which Phase 6 removed (ticket
/// 44 of fleetd-p6).
const SEAT: &str = "orchestrator";

/// The persona of the worker of a test.
const WORKER: &str = "opus-hardening";

/// The `fleet.toml` of a test: 1 pass each second and the longest
/// heartbeat age.
fn fleet_toml() -> String {
    "pass_period_s = 1\nheartbeat_max_age_s = 300\n".to_string()
}

/// 1 live attempt of a test: its id and its stand-in wrapper, which leads
/// its own process group.
struct Attempt {
    id: String,
    worker: String,
    task: Option<String>,
    wrapper: u32,
}

/// A harness with a git repository, a real fleetd, the run `p5` and the
/// stand-in wrappers of the live attempts.
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
        World::with_gate_sandbox(name, false)
    }

    /// The world. With `gate`, the fleetd of the test can find `bwrap` on
    /// Linux, the gate sandbox of this host.
    fn with_gate_sandbox(name: &str, gate: bool) -> World {
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
        if gate {
            link_bwrap(&h);
        }
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

    fn db(&self) -> PathBuf {
        self.h.root.join("ledger.sqlite3")
    }

    fn fleet_root(&self) -> PathBuf {
        self.h.root.join("fleet")
    }

    fn paths(&self) -> OperatorPaths {
        OperatorPaths::of(&self.fleet_root())
    }

    /// The operator token, as the test gives it to an attack.
    fn token(&self) -> String {
        token_file::read(&self.fleet_root())
            .unwrap()
            .text()
            .to_string()
    }

    /// A connection of the test to the ledger, for its fixtures and reads.
    fn conn(&self) -> Connection {
        open(&self.db())
    }

    /// The evidence of a wait that failed: the output of fleetd and each
    /// job with its result.
    fn fleetd_output(&self) -> String {
        let conn = self.conn();
        let mut stmt = conn
            .prepare(
                "SELECT id || ' ' || kind || ' ' || subject || ' ' || state || ' '
                     || COALESCE(result_json, '-') FROM jobs ORDER BY id",
            )
            .unwrap();
        let jobs: Vec<String> = stmt
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        format!(
            "fleetd stderr:\n{}\njobs:\n{}",
            self.fleetds.output("fleetd", "stderr"),
            jobs.join("\n")
        )
    }

    /// A new live attempt of `persona` with the task `task`, or with no
    /// task when `task` is `None`. Its stand-in wrapper is a `sleep` that
    /// leads a new process group.
    fn live_attempt(&mut self, persona: &str, kind: WorkerKind, task: bool) -> Attempt {
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
        self.record_attempt(persona, kind, task, pid, start)
    }

    /// Record the live attempt whose wrapper is `pid` with the start time
    /// `start`, as fleetd records a launch.
    fn record_attempt(
        &self,
        persona: &str,
        kind: WorkerKind,
        task: bool,
        pid: u32,
        start: u64,
    ) -> Attempt {
        let mut conn = self.conn();
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        let worker = workers::insert(
            &tx,
            &NewWorker {
                run_id: self.run.run_id.clone(),
                kind: Some(kind),
                persona: persona.into(),
                ..Default::default()
            },
        )
        .unwrap();
        let task_id = task.then(|| {
            let new = NewTask::work(&format!("work of {worker}"), "t.md", persona);
            let task = tasks::add(&tx, &self.run, &new, Actor::Orchestrator)
                .unwrap()
                .task_id;
            tx.execute(
                "UPDATE tasks SET status = 'running', current_worker_id = ?2 WHERE task_id = ?1",
                params![task, worker],
            )
            .unwrap();
            task
        });
        tx.execute(
            "UPDATE workers SET status = 'working', task_id = ?2 WHERE worker_id = ?1",
            params![worker, task_id],
        )
        .unwrap();
        let id = format!("A-{worker}");
        tx.execute(
            "INSERT INTO attempts (id, task_id, worker_id, pid, pid_start, state, started_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 'live', '2026-11-18T12:00:00Z')",
            params![id, task_id, worker, pid, i64::try_from(start).unwrap()],
        )
        .unwrap();
        tx.commit().unwrap();
        Attempt {
            id,
            worker,
            task: task_id,
            wrapper: pid,
        }
    }

    /// A `horch` command of the test, with the seam of RR-5: it panics when
    /// it opens the ledger for a write (ticket 42). fleetd and its job
    /// processes run with no seam: `horch job run` still writes the rows of
    /// its job until T-883.
    fn horch(&self, args: &[&str]) -> Command {
        let mut cmd = self.h.horch(args);
        cmd.env(ASSERT_READ_ONLY_ENV, "1");
        cmd
    }

    /// The folder that the worker sandbox lets an attacker write.
    fn work_area(&self) -> PathBuf {
        let dir = self.h.root.join("work-area");
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// The worker sandbox of ticket 06: write only the work area, and the
    /// 3 operator denials (the token, the channel, each write in `run/`).
    fn worker_sandbox(&self, cmd: Command) -> Command {
        let paths = self.paths();
        let denied = Denials {
            read: vec![paths.token.clone()],
            connect: vec![paths.channel.clone()],
            write: vec![paths.run.clone()],
        };
        match sandboxed(cmd, &[self.work_area()], &denied) {
            Ok(cmd) => cmd,
            Err(gap) => panic!("this host has no worker sandbox: {gap}"),
        }
    }

    /// The attacker of `act` on `path` with `line`, in the worker sandbox
    /// when `sandbox`, in the process group of the attempt `a`.
    fn attack(&self, a: &Attempt, sandbox: bool, act: &str, path: &Path, line: &str) -> Attack {
        let cmd = attacker(act, path, line);
        let cmd = if sandbox {
            self.worker_sandbox(cmd)
        } else {
            cmd
        };
        attack_of(&World::in_attempt(cmd, a))
    }

    /// Run `cmd` in the process group of the attempt `a`.
    fn in_attempt(mut cmd: Command, a: &Attempt) -> Output {
        cmd.process_group(i32::try_from(a.wrapper).unwrap())
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    /// The row of the request `id`.
    fn request(&self, id: &str) -> Option<RequestRow> {
        self.conn()
            .query_row(
                "SELECT request_id, kind, class, state, auth, reason FROM requests
                 WHERE request_id = ?1",
                [id],
                request_row,
            )
            .optional()
            .unwrap()
    }

    /// Each row of the `requests` table, in the order of its insert.
    fn requests(&self) -> Vec<RequestRow> {
        let conn = self.conn();
        let mut stmt = conn
            .prepare(
                "SELECT request_id, kind, class, state, auth, reason FROM requests ORDER BY rowid",
            )
            .unwrap();
        let rows = stmt.query_map([], request_row).unwrap();
        rows.collect::<rusqlite::Result<_>>().unwrap()
    }

    /// Wait until the request `id` is decided, and give its row.
    fn decided(&self, id: &str) -> RequestRow {
        wait_for_or(
            &format!("fleetd to decide the request {id}"),
            || self.request(id).filter(|r| r.state != "pending"),
            || self.fleetd_output(),
        )
    }

    /// The names of the files in the request folder, `archive/` left out.
    fn request_files(&self) -> BTreeSet<String> {
        std::fs::read_dir(requests_dir(&self.fleet_root()))
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n != "archive")
            .collect()
    }

    /// The state of the ledger that an operator request changes: each task,
    /// worker, attempt, job, feature and branch with its state.
    fn state(&self) -> Vec<String> {
        let conn = self.conn();
        let mut out = Vec::new();
        for sql in [
            "SELECT 'task ' || task_id || ' ' || status FROM tasks ORDER BY task_id",
            "SELECT 'worker ' || worker_id || ' ' || status FROM workers ORDER BY worker_id",
            "SELECT 'attempt ' || id || ' ' || state FROM attempts ORDER BY id",
            "SELECT 'feature ' || id || ' ' || state FROM features ORDER BY id",
            "SELECT 'branch ' || name || ' ' || COALESCE(landing_state, '-') FROM branches ORDER BY name",
            "SELECT 'run ' || run_id || ' ' || status FROM runs ORDER BY run_id",
        ] {
            let mut stmt = conn.prepare(sql).unwrap();
            let rows = stmt.query_map([], |r| r.get::<_, String>(0)).unwrap();
            out.extend(rows.map(Result::unwrap));
        }
        out
    }
}

/// The feature [`FEATURE`] of the project `project` and its internal run,
/// with 1 idea in flight: a task needs one (FR-12).
fn new_feature_run(conn: &Connection, project: &Path) -> Run {
    let tx = rusqlite::Transaction::new_unchecked(conn, TransactionBehavior::Immediate).unwrap();
    let feature = runs::create_feature_run(&tx, project, "main").unwrap();
    assert_eq!(feature, FEATURE, "the first run of the ledger");
    let new = NewFeature {
        id: FEATURE,
        name: Some(FEATURE),
        branch: Some(FEATURE_BRANCH),
        primary_branch: "main",
    };
    let cause = Cause {
        actor: Actor::Horch,
        text: "the test adds the feature",
    };
    integration::insert_feature(&tx, None, &new, cause).unwrap();
    tx.execute(
        "INSERT INTO ideas (id, feature_id, state, created_at, updated_at)
         VALUES ('I-1', ?1, 'in_progress', '2026-11-18T12:00:00Z', '2026-11-18T12:00:00Z')",
        [FEATURE],
    )
    .unwrap();
    let run = runs::feature_run(&tx, FEATURE).unwrap().unwrap();
    tx.commit().unwrap();
    run
}

/// A connection of the test to the ledger `db`. It waits for the writes of
/// fleetd.
fn open(db: &Path) -> Connection {
    let conn = Connection::open(db).unwrap();
    conn.busy_timeout(Duration::from_secs(30)).unwrap();
    conn
}

fn request_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<RequestRow> {
    Ok(RequestRow {
        id: r.get(0)?,
        kind: r.get(1)?,
        class: r.get(2)?,
        state: r.get(3)?,
        auth: r.get(4)?,
        reason: r.get(5)?,
    })
}

/// Link `bwrap` of this host into the `bin/` of `h`, so that fleetd and its
/// jobs find the Linux sandbox on their sealed `PATH`. macOS needs no link:
/// its sandbox is `/usr/bin/sandbox-exec`.
fn link_bwrap(h: &Harness) {
    let found = std::env::var_os("PATH").and_then(|path| {
        std::env::split_paths(&path)
            .map(|dir| dir.join("bwrap"))
            .find(|p| p.is_file())
    });
    if let Some(bwrap) = found {
        std::os::unix::fs::symlink(bwrap, h.bin.join("bwrap")).unwrap();
    }
}

/// The reason of a refusal on the channel from a peer in a worker attempt.
fn peer_reason(a: &Attempt) -> String {
    format!("the peer process is a fleet process of attempt {}", a.id)
}

/// The reason of a refusal of an operator request with no valid token.
const NO_TOKEN: &str = "an operator kind needs the valid operator token";

// ─── SR-8 from a worker sandbox ─────────────────────────────────────────

/// For each `operator` kind, 4 attacks from a live worker attempt:
/// 1. inside the worker sandbox, a raw request line on the channel;
/// 2. inside the worker sandbox, a request file with the right mode;
/// 3. with the sandbox off, a raw request line with a copy of a valid
///    token, which the peer process check refuses;
/// 4. with the sandbox off, a request file with no token.
///
/// 1 and 2 fail at the OS, and no row or file is left. 3 and 4 reach fleetd,
/// and the `requests` row records the refusal with its reason and a null
/// `auth`. No request of the attacks is applied, and the state of the
/// ledger does not change.
#[test]
fn operator_request_from_a_worker_sandbox_is_refused_for_each_operator_kind() {
    let mut w = World::new("p5-sr8-kinds");
    let a = w.live_attempt(WORKER, WorkerKind::Worker, true);
    let paths = w.paths();
    let token = w.token();
    let before = w.state();
    let mut files = Vec::new();
    for kind in operator_kinds() {
        let name = kind.as_str();
        let id1 = request_id(&["a1", name]);
        let channel = w.attack(
            &a,
            true,
            "connect",
            &paths.channel,
            &line(&id1, kind, json!({}), None),
        );
        assert!(
            !channel.succeeded,
            "attack 1 of {name}: the sandbox lets a worker connect to the channel: {channel:?}"
        );

        let id2 = request_id(&["a2", name]);
        let path2 = paths.requests.join(format!("{}.json", id2.as_str()));
        let file = w.attack(&a, true, "file", &path2, &line(&id2, kind, json!({}), None));
        assert!(
            !file.succeeded,
            "attack 2 of {name}: the sandbox lets a worker write a request file: {file:?}"
        );

        let id3 = request_id(&["a3", name]);
        let raw = w.attack(
            &a,
            false,
            "connect",
            &paths.channel,
            &line(&id3, kind, json!({}), Some(&token)),
        );
        assert!(raw.succeeded, "attack 3 of {name} reaches fleetd: {raw:?}");
        let reply: Value = serde_json::from_str(&raw.text).unwrap_or_else(|e| {
            panic!(
                "attack 3 of {name}: {e}: {raw:?}\n{}\n{}\n{:#?}",
                w.fleetd_output(),
                w.fleetds.output("fleetd", "stdout"),
                w.requests()
            )
        });
        assert_eq!(reply["state"], "refused", "attack 3 of {name}: {reply}");
        assert_refused(&w.decided(id3.as_str()), &peer_reason(&a));

        let id4 = request_id(&["a4", name]);
        let path4 = paths.requests.join(format!("{}.json", id4.as_str()));
        let file = w.attack(
            &a,
            false,
            "file",
            &path4,
            &line(&id4, kind, json!({}), None),
        );
        assert!(
            file.succeeded,
            "attack 4 of {name} writes its file: {file:?}"
        );
        files.push(id4);

        for id in [&id1, &id2] {
            assert_eq!(w.request(id.as_str()), None, "a refused act leaves no row");
        }
        assert!(!path2.exists() && !PathBuf::from(format!("{}.tmp", path2.display())).exists());
    }
    for id in &files {
        assert_refused(&w.decided(id.as_str()), NO_TOKEN);
    }
    let rows = w.requests();
    assert_eq!(rows.len(), 2 * operator_kinds().len(), "{rows:#?}");
    assert!(rows.iter().all(|r| r.state == "refused"), "{rows:#?}");
    let token = w.token();
    let read = w.attack(&a, true, "read", &paths.token, "");
    assert!(
        !read.succeeded,
        "the sandbox lets a worker read the token: {read:?}"
    );
    assert!(
        !read.text.contains(&token),
        "the attacker never prints the token"
    );
    assert_eq!(
        w.state(),
        before,
        "no attack changes the state of the ledger"
    );
}

// ─── SR-8 from a process of a live attempt ──────────────────────────────

/// A process of a live attempt that left the process group of its wrapper
/// (`setsid`) is still in the attempt by its parent chain. Its raw request
/// with a copy of a valid token is refused, and the row records the refusal
/// with a null `auth`.
#[test]
fn operator_request_from_a_process_of_a_live_attempt_is_refused() {
    let w = World::new("p5-sr8-chain");
    let paths = w.paths();
    let id = request_id(&["chain", "ledger-gc"]);
    let text = line(
        &id,
        Kind::LedgerGc,
        json!({ "deleted": [] }),
        Some(&w.token()),
    );
    let mut attack = attacker("connect", &paths.channel, &text);
    attack.env(NEW_SESSION_ENV, "1");
    // The stand-in wrapper waits for 1 line on its stdin, then starts the
    // attacker as its child.
    let mut wrapper = Command::new("/bin/sh");
    wrapper
        .args(["-c", "read go; \"$@\"", "wrapper"])
        .arg(attack.get_program())
        .args(attack.get_args())
        .env_clear()
        .envs(attack.get_envs().filter_map(|(k, v)| Some((k, v?))))
        .process_group(0)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = wrapper.spawn().unwrap();
    let pid = child.id();
    let start = wait_for("the start time of the wrapper", || {
        horch_platform::process::start_time(pid)
    });
    let a = w.record_attempt(WORKER, WorkerKind::Worker, true, pid, start);
    child.stdin.take().unwrap().write_all(b"go\n").unwrap();
    let out = child.wait_with_output().unwrap();

    let raw = attack_of(&out);
    assert!(raw.succeeded, "the attack reaches fleetd: {raw:?}");
    let reply: Value = serde_json::from_str(&raw.text).unwrap();
    assert_eq!(reply["state"], "refused", "{reply}");
    assert_refused(&w.decided(id.as_str()), &peer_reason(&a));
}

// ─── REQUIRED item 3: the operator-only commands ────────────────────────

/// Where an operator-only command run from a worker attempt is refused.
enum RefusedBy {
    /// fleetd: the command sends its request, and the peer process check
    /// refuses it.
    Fleetd,
    /// The command itself, before it sends a request, with this line. fleetd
    /// refuses its kind too: attack 3 of
    /// [`operator_request_from_a_worker_sandbox_is_refused_for_each_operator_kind`].
    Client(&'static str),
}

/// The task branch of the worker of a test, in the ledger and in git, so
/// that `horch branch abandon` finds it.
const TASK_BRANCH: &str = "fleet/F-0001/attack";

/// Each operator-only command, run from a live worker attempt with
/// `HORCH_ROLE=orchestrator` and with no `HORCH_ROLE`, is refused: its
/// `requests` rows are refused by fleetd with a null `auth`, and the state
/// of the ledger does not change. The attacker keeps `HORCH_WORKER_ID` only
/// for `horch ledger worktree drop`, which needs it. `horch rescue drop`
/// needs a rescue ref in git: its own test is
/// [`operator_only_rescue_drop_from_a_worker_is_refused_by_fleetd`].
#[test]
fn operator_only_command_with_horch_role_set_or_removed_is_refused_for_each_command() {
    let mut w = World::new("p5-sr8-commands");
    let a = w.live_attempt(WORKER, WorkerKind::Worker, true);
    let task = a.task.clone().unwrap();
    w.task_branch(&a);
    let commands: Vec<(Vec<&str>, RefusedBy)> = vec![
        (vec!["job", "requeue", "1"], RefusedBy::Fleetd),
        (
            vec!["ledger", "requeue", &task, "attack"],
            RefusedBy::Fleetd,
        ),
        (vec!["ledger", "cancel", &task, "attack"], RefusedBy::Fleetd),
        (
            vec!["ledger", "waive", &task, "F1", "attack"],
            RefusedBy::Fleetd,
        ),
        (vec!["task", "requeue", &task, "attack"], RefusedBy::Fleetd),
        (vec!["task", "cancel", &task, "attack"], RefusedBy::Fleetd),
        (vec!["worktree", "restore", TASK_BRANCH], RefusedBy::Fleetd),
        (
            vec!["ledger", "worktree", "drop", "attack", "--reason", "attack"],
            RefusedBy::Fleetd,
        ),
        (
            vec!["feature", "abandon", FEATURE],
            RefusedBy::Client("refused inside a fleet attempt"),
        ),
        (vec!["branch", "abandon", TASK_BRANCH], RefusedBy::Fleetd),
    ];
    let before = w.state();
    for (args, refused_by) in &commands {
        for role in [Some("orchestrator"), None] {
            let shown = format!("horch {} with HORCH_ROLE {role:?}", args.join(" "));
            let seen = w.requests().len();
            let out = w.command_in_attempt(&a, args, role);
            let rows = w.requests().split_off(seen);
            let stderr = String::from_utf8_lossy(&out.stderr);
            assert!(!out.status.success(), "{shown} is refused: {stderr}");
            match refused_by {
                RefusedBy::Fleetd => {
                    assert!(!rows.is_empty(), "{shown} reaches fleetd: {stderr}");
                    for row in &rows {
                        assert_refused(row, &peer_reason(&a));
                    }
                    assert!(stderr.contains(&peer_reason(&a)), "{shown}: {stderr}");
                }
                RefusedBy::Client(line) => {
                    assert!(rows.is_empty(), "{shown} sends no request: {rows:#?}");
                    assert!(stderr.contains(line), "{shown}: {stderr}");
                }
            }
        }
    }
    assert_eq!(
        w.state(),
        before,
        "no command changes the state of the ledger"
    );
}

/// The rescue ref that the attacker tries to drop.
const RESCUE_REF: &str = "refs/horch/rescue/attack/1";

/// REQUIRED item 3 for `horch rescue drop`: from a live worker attempt,
/// with `HORCH_ROLE` set or removed, fleetd refuses its `rescue_drop`
/// request, and the rescue ref stays.
#[test]
fn operator_only_rescue_drop_from_a_worker_is_refused_by_fleetd() {
    let mut w = World::new("p5-sr8-rescue");
    let a = w.live_attempt(WORKER, WorkerKind::Worker, true);
    let tip = w.h.head_sha().unwrap();
    let out =
        w.h.git_cmd(&["update-ref", RESCUE_REF, &tip])
            .output()
            .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    for role in [Some("orchestrator"), None] {
        let seen = w.requests().len();
        let out = w.command_in_attempt(&a, &["rescue", "drop", RESCUE_REF], role);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            !out.status.success(),
            "horch rescue drop is refused: {stderr}"
        );
        let rows = w.requests().split_off(seen);
        assert_eq!(rows.len(), 1, "horch rescue drop reaches fleetd: {stderr}");
        assert_eq!(rows[0].kind, "rescue_drop");
        assert_refused(&rows[0], &peer_reason(&a));
        let kept =
            w.h.git_cmd(&["rev-parse", "--verify", RESCUE_REF])
                .output()
                .unwrap();
        assert!(kept.status.success(), "the rescue ref stays: {stderr}");
    }
}

impl World {
    /// Run `horch args` in the process group of the attempt `a`, with
    /// `HORCH_ROLE` set to `role` or removed.
    fn command_in_attempt(&self, a: &Attempt, args: &[&str], role: Option<&str>) -> Output {
        let mut cmd = self.horch(args);
        match role {
            Some(role) => cmd.env("HORCH_ROLE", role),
            None => cmd.env_remove("HORCH_ROLE"),
        };
        if args.starts_with(&["ledger", "worktree"]) {
            cmd.env("HORCH_WORKER_ID", &a.worker);
        }
        World::in_attempt(cmd, a)
    }

    /// The task branch [`TASK_BRANCH`] of the task of `a`: a git branch of
    /// the project with its row, and its worktree `attack` with its row.
    fn task_branch(&self, a: &Attempt) {
        let task = a.task.as_deref().unwrap();
        let tree = self.h.project.join(".worktrees").join("attack");
        let tree_text = tree.to_string_lossy();
        let out = self
            .h
            .git_cmd(&["worktree", "add", "-q", "-b", TASK_BRANCH, &tree_text])
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        self.conn()
            .execute(
                "INSERT INTO worktrees (worktree_id, task_id, worker_id, repo_root, path, branch,
                    base, status, created_at)
                 VALUES ('W-attack', ?1, ?2, ?3, ?4, ?5, 'main', 'open', '2026-11-18T12:00:00Z')",
                params![
                    task,
                    a.worker,
                    self.h.project.to_string_lossy(),
                    tree_text,
                    TASK_BRANCH
                ],
            )
            .unwrap();
        let new = NewBranch {
            name: TASK_BRANCH,
            feature_id: FEATURE,
            task_id: Some(task),
            owner: "worker",
            due_at: 0,
        };
        let cause = Cause {
            actor: Actor::Horch,
            text: "the test binds the task branch",
        };
        let mut conn = self.conn();
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        integration::insert_branch(&tx, None, &new, cause).unwrap();
        tx.commit().unwrap();
    }
}

// ─── Q2: the orchestrator seat ──────────────────────────────────────────

impl World {
    /// The raw request `ledger_gc` with a copy of the valid token, on the
    /// channel, from the attacker in the attempt `a` with the extra
    /// environment `env`. Gives the decided row.
    fn gc_from(&self, a: &Attempt, name: &str, env: &[(&str, &str)]) -> RequestRow {
        let id = request_id(&["gc", name]);
        let text = line(
            &id,
            Kind::LedgerGc,
            json!({ "deleted": [] }),
            Some(&self.token()),
        );
        let mut cmd = attacker("connect", &self.paths().channel, &text);
        cmd.envs(env.iter().copied());
        let raw = attack_of(&World::in_attempt(cmd, a));
        assert!(raw.succeeded, "the request {name} reaches fleetd: {raw:?}");
        self.decided(id.as_str())
    }
}

/// A worker cannot make itself an orchestrator seat. Each of these is
/// refused:
/// - a worker attempt whose environment names the orchestrator persona,
///   its role and the worker of a real seat;
/// - an attempt whose worker has the persona of a seat but a task;
/// - an attempt with a null task of another persona.
#[test]
fn forged_orchestrator_seat_is_refused() {
    let mut w = World::new("p5-sr8-forged");
    let seat = w.live_attempt(SEAT, WorkerKind::Orchestrator, false);
    let worker = w.live_attempt(WORKER, WorkerKind::Worker, true);
    let forged_env = [
        ("HORCH_ROLE", "orchestrator"),
        ("HORCH_PERSONA", SEAT),
        ("HORCH_WORKER_ID", seat.worker.as_str()),
    ];
    let row = w.gc_from(&worker, "env", &forged_env);
    assert_refused(&row, &peer_reason(&worker));

    let seat_persona_with_task = w.live_attempt(SEAT, WorkerKind::Worker, true);
    let row = w.gc_from(&seat_persona_with_task, "task", &[]);
    assert_refused(&row, &peer_reason(&seat_persona_with_task));

    let no_task_not_a_seat = w.live_attempt(WORKER, WorkerKind::Orchestrator, false);
    let row = w.gc_from(&no_task_not_a_seat, "persona", &[]);
    assert_refused(&row, &peer_reason(&no_task_not_a_seat));
}

/// The control case: you, outside each live attempt, run an operator-only
/// command, send a raw request with the token and write a request file with
/// the token, while a worker attempt is live. fleetd authorizes each one.
#[test]
fn operator_on_the_command_line_is_authorized() {
    let mut w = World::new("p5-sr8-operator");
    let _worker = w.live_attempt(WORKER, WorkerKind::Worker, true);
    let paths = w.paths();
    let token = w.token();

    let out = w.horch(&["ledger", "gc"]).output().unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "horch ledger gc: {stderr}");
    let rows = w.requests();
    assert_eq!(rows.len(), 1, "horch ledger gc sends 1 request: {stderr}");
    assert_eq!(rows[0].kind, "ledger_gc");
    assert_eq!(rows[0].state, "applied", "{:?}", rows[0]);
    assert_eq!(
        rows[0].auth.as_deref(),
        Some("token; peer outside each live attempt")
    );

    let id = request_id(&["operator", "channel"]);
    let text = line(&id, Kind::LedgerGc, json!({ "deleted": [] }), Some(&token));
    let raw = attack_of(&attacker("connect", &paths.channel, &text).output().unwrap());
    assert!(raw.succeeded, "{raw:?}");
    let row = w.decided(id.as_str());
    assert_eq!(row.state, "applied", "{row:?}");
    assert_eq!(
        row.auth.as_deref(),
        Some("token; peer outside each live attempt")
    );

    let id = request_id(&["operator", "file"]);
    let text = line(&id, Kind::LedgerGc, json!({ "deleted": [] }), Some(&token));
    let path = paths.requests.join(format!("{}.json", id.as_str()));
    let file = attack_of(&attacker("file", &path, &text).output().unwrap());
    assert!(file.succeeded, "{file:?}");
    let row = w.decided(id.as_str());
    assert_eq!(row.state, "applied", "{row:?}");
    assert_eq!(row.auth.as_deref(), Some("token; request file"));
}

// ─── SR-4, Q21: a test of the project in a gate ─────────────────────────

/// The file of the gate tree in which the test of the project writes the
/// result of each act.
const GATE_REPORT: &str = "p5-gate-attack.txt";

/// The test of the project that the fast gate runs: it tries each operator
/// act with the attacker `exe` and writes 1 line for each act in
/// [`GATE_REPORT`]: `<act> <kind> <exit code> <what the attacker got>`.
/// It exits 0, so the gate passes when each act is refused.
fn gate_test_script(exe: &Path, paths: &OperatorPaths) -> String {
    // The gate runs with the sealed PATH of the harness.
    let mut script = String::from("set -u\nPATH=/usr/bin:/bin\n");
    let mut act = |act: &str, kind: &str, path: &Path, line: &str| {
        script.push_str(&format!(
            "{ACT_ENV}={act} {PATH_ENV}={path} {LINE_ENV}={line} {exe} --exact {ATTACK_CHILD} \
             --nocapture --test-threads 1 > p5-act.out 2>&1\n\
             rc=$?\n\
             printf '%s %s %s %s\\n' {act} {kind} \"$rc\" \"$(grep -h '^{RESULT}' p5-act.out)\" \
             >> {GATE_REPORT}\n",
            path = quoted(&path.to_string_lossy()),
            line = quoted(line),
            exe = quoted(&exe.to_string_lossy()),
        ));
    };
    act("read", "-", &paths.token, "");
    for kind in operator_kinds() {
        let name = kind.as_str();
        let id = request_id(&["gate", name]);
        // The test of the project has no token: it sends a raw line.
        act(
            "connect",
            name,
            &paths.channel,
            &line(&id, kind, json!({}), None),
        );
        let file_id = request_id(&["gate-file", name]);
        let file = paths.requests.join(format!("{}.json", file_id.as_str()));
        act("file", name, &file, &line(&file_id, kind, json!({}), None));
    }
    script.push_str("exit 0\n");
    script
}

/// `text` in single quotes for `sh`.
fn quoted(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

impl World {
    /// A commit on `branch` from `main` with `files`. Gives its tip.
    fn commit(&self, branch: &str, files: &[(&str, &str)]) -> String {
        let git = |args: &[&str]| {
            let out = self.h.git_cmd(args).output().unwrap();
            assert!(
                out.status.success(),
                "git {args:?}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        };
        git(&["checkout", "-q", "-b", branch, "main"]);
        for (name, text) in files {
            std::fs::write(self.h.project.join(name), text).unwrap();
            git(&["add", name]);
        }
        git(&["commit", "-q", "-m", branch]);
        let tip = git(&["rev-parse", "HEAD"]);
        git(&["checkout", "-q", "main"]);
        tip
    }

    /// The gate tree of [`FEATURE`] at `main`: the test reserves it, and
    /// fleetd runs its `worktree_add` job and observes it.
    fn gate_tree(&self, repo: &Path, main: &str) -> PathBuf {
        std::fs::create_dir_all(repo.join(".worktrees")).unwrap();
        let req = GateTreeRequest {
            feature_id: FEATURE,
            run_id: &self.run.run_id,
            repo,
            base: FEATURE_BRANCH,
            tip: main,
        };
        let reserved = self.jobs_tx(|ctx, tx| gate_tree::reserve(ctx, tx, &req, FleetSeconds(0)));
        assert!(
            matches!(reserved, GateTree::Reserved { .. }),
            "{reserved:?}"
        );
        wait_for_or(
            "fleetd to make the gate tree",
            || {
                gate_tree::of_feature(&self.conn(), repo, FEATURE)
                    .unwrap()
                    .filter(|t| t.state == WorktreeState::Created)
                    .map(|t| PathBuf::from(t.path))
            },
            || self.fleetd_output(),
        )
    }

    /// A bundle of 1 task branch of [`FEATURE`] on `base_tip`. Its task is
    /// done, so fleetd dispatches nothing.
    fn bundle(&self, base_tip: &str) -> i64 {
        let mut conn = self.conn();
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        let new = NewTask::work("the gated work", "t.md", WORKER);
        let task = tasks::add(&tx, &self.run, &new, Actor::Orchestrator)
            .unwrap()
            .task_id;
        tx.execute(
            "UPDATE tasks SET status = 'done' WHERE task_id = ?1",
            [&task],
        )
        .unwrap();
        let cause = || Cause {
            actor: Actor::Horch,
            text: "the test bundles the task branch",
        };
        let branch = NewBranch {
            name: "fleet/F-0001/gated",
            feature_id: FEATURE,
            task_id: Some(&task),
            owner: "worker",
            due_at: 0,
        };
        integration::insert_branch(&tx, None, &branch, cause()).unwrap();
        let bundle = NewBundle {
            feature_id: FEATURE,
            base_tip,
            parent_id: None,
            members: &["fleet/F-0001/gated"],
            due_at: 0,
        };
        let id = integration::insert_bundle(&tx, None, &bundle, cause()).unwrap();
        tx.commit().unwrap();
        id
    }

    /// Run `f` in 1 immediate transaction of the test, with the job context
    /// of a test (no generation).
    fn jobs_tx<T>(&self, f: impl FnOnce(&JobCtx<'_>, &Connection) -> anyhow::Result<T>) -> T {
        let mut conn = self.conn();
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        let config = FleetConfig::default();
        let ctx = JobCtx {
            guard: None,
            config: &config,
            needs_you: &NeedsYouList,
        };
        let out = f(&ctx, &tx).unwrap();
        tx.commit().unwrap();
        out
    }
}

/// The test project declares a fast gate whose command is a test of the
/// project. The test tries each operator act: it reads the token, sends a
/// raw request line of each `operator` kind on the channel, and writes a
/// request file of each `operator` kind. fleetd runs the `fast_gate` job in
/// the gate sandbox with the 3 operator denials (SR-4, Q21), so the OS
/// refuses each act. The `gate_runs` row records the result, no `requests`
/// row exists, and the state of the ledger does not change.
#[test]
fn operator_request_from_a_test_in_a_gate_is_refused_for_each_act() {
    let w = World::with_gate_sandbox("p5-sr8-gate", true);
    let repo = w.h.project.canonicalize().unwrap();
    let main = w.h.head_sha().unwrap();
    let tree = w.gate_tree(&repo, &main);
    let paths = w.paths();
    let exe = std::env::current_exe().unwrap();
    let gates = "[gates]\nfast = \"/bin/sh p5-gate-test.sh\"\nfull = \"false\"\n";
    let candidate = w.commit(
        "candidate",
        &[
            ("fleet.toml", gates),
            ("p5-gate-test.sh", &gate_test_script(&exe, &paths)),
        ],
    );
    let bundle = w.bundle(&main).to_string();
    wait_for_or(
        "fleetd to move the bundled branch to ready and to drop its bundle",
        || {
            w.conn()
                .query_row(
                    "SELECT 1 FROM branches b, bundles u
                      WHERE b.name = 'fleet/F-0001/gated' AND b.landing_state = 'ready'
                        AND u.id = ?1 AND u.state = 'dropped'",
                    [&bundle],
                    |_| Ok(()),
                )
                .optional()
                .unwrap()
        },
        || w.fleetd_output(),
    );
    let before = w.state();
    let req = GateRequest {
        kind: JobKind::FastGate,
        subject: &bundle,
        feature_id: FEATURE,
        tip: &candidate,
        base_tip: &main,
        repo: &repo,
    };
    let queued = w.jobs_tx(|ctx, tx| exec_gate::queue(ctx, tx, &req, FleetSeconds(0)));
    let Some(Queued::New(job)) = queued else {
        panic!("the fast gate is queued: {queued:?}");
    };

    wait_for_or(
        "fleetd to run the fast gate to its end",
        || {
            w.conn()
                .query_row(
                    "SELECT 1 FROM jobs WHERE id = ?1 AND state IN ('passed', 'failed')",
                    [job],
                    |_| Ok(()),
                )
                .optional()
                .unwrap()
        },
        || w.fleetd_output(),
    );
    let result = integration::gate_run_of(&w.conn(), FEATURE, "fast", &candidate, &main)
        .unwrap()
        .and_then(|row| row.result)
        .unwrap_or_default();
    let report = std::fs::read_to_string(tree.join(GATE_REPORT)).unwrap_or_else(|e| {
        panic!(
            "the gate ({result}) wrote no {GATE_REPORT}: {e}\n{}",
            w.fleetd_output()
        )
    });
    let lines: Vec<&str> = report.lines().collect();
    assert_eq!(lines.len(), 1 + 2 * operator_kinds().len(), "{report}");
    for l in &lines {
        let mut parts = l.splitn(4, ' ');
        let (act, kind, rc, got) = (parts.next(), parts.next(), parts.next(), parts.next());
        assert_eq!(
            rc,
            Some("1"),
            "the OS refuses the act {act:?} of {kind:?}: {l}"
        );
        assert!(
            got.is_some_and(|g| g.starts_with(RESULT)),
            "the attacker names the refusal: {l}"
        );
    }
    assert_eq!(result, "pass", "the gate ran its test to the end: {report}");
    assert_eq!(
        w.requests(),
        Vec::new(),
        "no request of the gate reaches fleetd"
    );
    assert_eq!(
        w.request_files(),
        BTreeSet::new(),
        "no request file is left"
    );
    assert_eq!(w.state(), before, "the gate changes no state of the ledger");
}
