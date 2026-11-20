//! The release test of Phase 2 (`docs/specs/fleetd/design.md`, "What a
//! release does to a run in flight", row Phase 2;
//! `docs/tickets/fleetd-p2/MAP.md`, Q6, Q7 and Q21).
//!
//! A held task with a live worker starts on Phase 1. After the update, the
//! worker is not reaped, and the task has 1 lease.
//!
//! The Phase 1 ledger is built by the real migration steps up to
//! [`schema::PHASE_1_VERSION`], then the rows of a run in flight are
//! written as a Phase 1 binary left them. The live agent is a stand-in: a
//! `sleep` child of the test. The old supervisor is a stand-in too: the test
//! writes `workers.last_checkin_at` each 30 s (`runs.heartbeat_every_s`).
//!
//! The fleet clock counts the awake time of 1 fleetd process, so a
//! `horch ledger watch --once` process adds 0 to it. The test moves the
//! fleet clock by the same step as `HORCH_NOW`.

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output};

use horch_core::taskledger::runs::root_key;
use horch_core::taskledger::schema;
use horch_core::taskledger::Store;
use horch_core::{clock, procid};
use horch_e2e::harness::Harness;
use rusqlite::Connection;

/// The fleet clock of the Phase 1 `fleetd` row at the update.
const CLOCK: i64 = 5000;
/// The default `liveness.lease_s` of the design.
const LEASE_S: i64 = 120;
/// The default `liveness.max_wait_s` of the design.
const MAX_WAIT_S: i64 = 3600;
/// The checkin period of the old supervisor (`runs.heartbeat_every_s`).
const STEP_S: i64 = 30;
/// The passes after the update: 10 min of the fleet clock.
const PASSES: i64 = 600 / STEP_S;
/// The wall time of the first checkin of the old supervisor.
const T0: i64 = 1_893_456_000; // 2030-01-01T00:00:00Z

const RUN: &str = "zeta";

const TASK: &str = "T-1";
const ATTEMPT: &str = "T-1/A1";
const WORKER: &str = "w-live";

fn text(o: &Output) -> String {
    format!(
        "status: {}\nstdout: {}\nstderr: {}",
        o.status,
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

/// `secs` after the Unix epoch as a ledger timestamp, the form of
/// `HORCH_NOW`.
fn rfc3339(secs: i64) -> String {
    clock::stamp(clock::from_epoch_ms(secs * 1000).unwrap())
}

/// A harness with its own ledger file and a git repository, and no
/// watchdog.
fn world(name: &str) -> Harness {
    let mut h = Harness::new(name).with_git();
    h.set("HORCH_LEDGER_DB", db(&h).to_string_lossy());
    h.set("HORCH_NO_WAKE", "1");
    h.set("HORCH_NO_WATCHDOG", "1");
    h
}

fn db(h: &Harness) -> PathBuf {
    h.root.join("ledger.sqlite3")
}

/// The stand-in agent: a `sleep` process, killed and waited on drop, so a
/// failed assertion leaks no process.
struct Agent {
    child: Child,
    started: u64,
}

impl Agent {
    fn start() -> Agent {
        let child = Command::new("/bin/sleep").arg("600").spawn().unwrap();
        let started = procid::start_time(child.id()).expect("the start time of sleep");
        Agent { child, started }
    }

    fn pid(&self) -> u32 {
        self.child.id()
    }

    fn alive(&self) -> bool {
        procid::alive(self.pid(), Some(self.started))
    }
}

impl Drop for Agent {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The wrapper of the attempt: a process of its own that runs each worker
/// command as its child. The test is never the wrapper, since a fleet
/// process starts no fleetd (T-171) and the test starts fleetd. Killed and
/// waited on drop.
struct Wrapper {
    child: Child,
    stdin: std::process::ChildStdin,
    replies: std::io::BufReader<std::process::ChildStdout>,
    dir: PathBuf,
    runs: u32,
}

impl Wrapper {
    fn start(h: &Harness) -> Wrapper {
        let mut child = Command::new("/bin/sh")
            .args([
                "-c",
                r#"while read -r s; do /bin/sh "$s" >"$s.out" 2>"$s.err"; echo $?; done"#,
            ])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take().unwrap();
        let replies = std::io::BufReader::new(child.stdout.take().unwrap());
        Wrapper {
            child,
            stdin,
            replies,
            dir: h.root.clone(),
            runs: 0,
        }
    }

    fn pid(&self) -> u32 {
        self.child.id()
    }

    /// Run `cmd` as a child of the wrapper, with the program, arguments,
    /// environment and folder of `cmd`.
    fn run(&mut self, cmd: &Command) -> Output {
        use std::io::{BufRead, Write};
        use std::os::unix::process::ExitStatusExt;
        self.runs += 1;
        let script = self.dir.join(format!("worker-{}.sh", self.runs));
        let quote =
            |s: &std::ffi::OsStr| format!("'{}'", s.to_string_lossy().replace('\'', r"'\''"));
        let mut text = String::new();
        if let Some(dir) = cmd.get_current_dir() {
            text += &format!("cd {}\n", quote(dir.as_os_str()));
        }
        text += "exec /usr/bin/env -i";
        for (key, value) in cmd.get_envs() {
            if let Some(value) = value {
                text += &format!(" {}={}", quote(key), quote(value));
            }
        }
        text += &format!(" {}", quote(cmd.get_program()));
        for arg in cmd.get_args() {
            text += &format!(" {}", quote(arg));
        }
        std::fs::write(&script, text + "\n").unwrap();
        writeln!(self.stdin, "{}", script.display()).unwrap();
        let mut reply = String::new();
        self.replies.read_line(&mut reply).unwrap();
        let code: i32 = reply.trim().parse().unwrap();
        let read = |ext: &str| std::fs::read(format!("{}.{ext}", script.display())).unwrap();
        Output {
            status: std::process::ExitStatus::from_raw(code << 8),
            stdout: read("out"),
            stderr: read("err"),
        }
    }
}

impl Drop for Wrapper {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn phase_1_ledger(path: &Path, project: &Path, agent: &Agent, wrapper: &Wrapper) {
    let mut conn = Connection::open(path).unwrap();
    schema::migrate_to(&mut conn, schema::PHASE_1_VERSION).unwrap();
    let wrapper = wrapper.pid();
    let t0 = rfc3339(T0);
    conn.execute(
        "INSERT INTO projects (project_id, root, created_at) VALUES ('p', ?1, ?2)",
        (root_key(project), &t0),
    )
    .unwrap();
    conn.execute(
        "INSERT INTO runs (run_id, project_id, slug, status, started_at)
            VALUES ('r1', 'p', ?1, 'active', ?2)",
        (RUN, &t0),
    )
    .unwrap();
    conn.execute(
        "INSERT INTO tasks (task_id, run_id, seq, title, task_file, primary_persona, claimable_by,
                status, attempts, current_worker_id, created_at, updated_at)
            VALUES (?1, 'r1', 1, 'T0', 'tickets/t0.md', 'opus', 'opus', 'running', 1, ?2, ?3, ?3)",
        (TASK, WORKER, &t0),
    )
    .unwrap();
    conn.execute(
        "INSERT INTO workers (worker_id, run_id, kind, persona, host, agent_pid, agent_started,
                last_checkin_at, task_id, attempt, status, spawned_at, started_at, claimed_at)
            VALUES (?1, 'r1', 'worker', 'opus', ?2, ?3, ?4, ?5, ?6, 1, 'working', ?5, ?5, ?5)",
        (
            WORKER,
            horch_core::taskledger::util::host_name(),
            agent.pid(),
            agent.started,
            &t0,
            TASK,
        ),
    )
    .unwrap();
    conn.execute(
        "INSERT INTO attempts (id, task_id, worker_id, pid, pid_start, state, started_at)
            VALUES (?1, ?2, ?3, ?4, ?5, 'live', ?6)",
        (
            ATTEMPT,
            TASK,
            WORKER,
            wrapper,
            procid::start_time(wrapper).expect("the start time of the wrapper"),
            &t0,
        ),
    )
    .unwrap();
    conn.execute(
        "INSERT INTO worker_waits (worker_id, kind, reason, started_at, deadline)
            VALUES (?1, 'background', 'cargo test', ?2, ?3)",
        (WORKER, &t0, rfc3339(T0 + 24 * 3600)),
    )
    .unwrap();
    // The Phase 1 fleetd stopped for the update: its PID and start time
    // name no live process.
    conn.execute(
        "INSERT INTO fleetd (id, generation, pid, pid_start, started_at, fleet_clock_s, clock_ticked_at)
            VALUES (1, 1, 7, 1, ?1, ?2, ?1)",
        (&t0, CLOCK),
    )
    .unwrap();
    assert_eq!(user_version(&conn), schema::PHASE_1_VERSION);
}

fn user_version(conn: &Connection) -> i64 {
    conn.query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap()
}

/// The checkin of the old supervisor at the wall time `at`.
fn checkin(h: &Harness, at: i64) {
    let conn = Connection::open(db(h)).unwrap();
    conn.execute(
        "UPDATE workers SET last_checkin_at = ?2 WHERE worker_id = ?1",
        (WORKER, rfc3339(at)),
    )
    .unwrap();
}

/// Run `horch` at the wall time `at`.
fn horch_at(h: &Harness, at: i64, args: &[&str]) -> Output {
    run_at(h.horch(args), at, args)
}

/// Run `horch` at the wall time `at` as the worker [`WORKER`].
fn worker_at(h: &Harness, wrapper: &mut Wrapper, at: i64, args: &[&str]) -> Output {
    let mut cmd = h.horch(args);
    cmd.env("HORCH_WORKER_ID", WORKER);
    cmd.env("HORCH_NOW", rfc3339(at));
    let o = wrapper.run(&cmd);
    assert!(o.status.success(), "horch {}: {}", args.join(" "), text(&o));
    o
}

fn run_at(mut cmd: Command, at: i64, args: &[&str]) -> Output {
    cmd.env("HORCH_NOW", rfc3339(at));
    let o = cmd.output().unwrap();
    assert!(o.status.success(), "horch {}: {}", args.join(" "), text(&o));
    o
}

/// The leases of [`TASK`]: `(attempt_id, expires_at)`.
fn leases(conn: &Connection) -> Vec<(String, i64)> {
    conn.prepare("SELECT attempt_id, expires_at FROM leases WHERE task_id = ?1")
        .unwrap()
        .query_map([TASK], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}

fn scalar_text(conn: &Connection, sql: &str) -> String {
    conn.query_row(sql, [], |r| r.get(0)).unwrap()
}

#[test]
fn held_task_with_a_live_worker_on_phase_1_is_not_reaped_after_the_update_and_has_1_lease() {
    let h = world("p2-release");
    let agent = Agent::start();
    let mut wrapper = Wrapper::start(&h);
    phase_1_ledger(&db(&h), &h.project, &agent, &wrapper);

    // Phase 1: the old supervisor checks in each 30 s.
    let mut now = T0;
    for _ in 0..4 {
        checkin(&h, now);
        now += STEP_S;
    }

    let store = Store::open(&db(&h)).unwrap();
    drop(store);
    let conn = Connection::open(db(&h)).unwrap();
    assert_eq!(user_version(&conn), schema::VERSION);
    let first = leases(&conn);
    assert_eq!(
        first,
        [(ATTEMPT.to_string(), CLOCK + LEASE_S)],
        "1 lease with a full time on the fleet clock"
    );
    let due: Option<i64> = conn
        .query_row(
            "SELECT due_at FROM worker_waits WHERE worker_id = ?1 AND kind = 'background'",
            [WORKER],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        due,
        Some(CLOCK + MAX_WAIT_S),
        "the open wait gets max_wait_s"
    );
    drop(conn);
    h.serve_fleetd();

    // 10 min of the fleet clock: the old supervisor checks in, the agent
    // records progress, and fleetd runs 1 pass, each 30 s.
    let mut seen = Vec::new();
    for _ in 0..PASSES {
        now += STEP_S;
        h.advance_fleet_clock(STEP_S);
        checkin(&h, now);
        worker_at(&h, &mut wrapper, now, &["ledger", "progress", "building"]);
        let o = h.without_fleetd(|| {
            horch_at(
                &h,
                now,
                &[
                    "ledger", "watch", "--once", "--force", "--json", "--run", RUN,
                ],
            )
        });
        seen.push(text(&o));
    }
    let seen = seen.join("\n");

    let conn = Connection::open(db(&h)).unwrap();
    let status = scalar_text(
        &conn,
        &format!("SELECT status FROM tasks WHERE task_id = '{TASK}'"),
    );
    assert_eq!(status, "running", "{seen}");
    let worker = scalar_text(
        &conn,
        &format!("SELECT status FROM workers WHERE worker_id = '{WORKER}'"),
    );
    assert_eq!(worker, "working", "the worker is not reaped\n{seen}");
    let attempt = scalar_text(
        &conn,
        &format!("SELECT state FROM attempts WHERE id = '{ATTEMPT}'"),
    );
    assert_eq!(attempt, "live", "{seen}");
    assert!(agent.alive(), "the agent goes on\n{seen}");
    let last = leases(&conn);
    assert_eq!(last.len(), 1, "1 lease: {last:?}\n{seen}");
    assert_eq!(last[0].0, ATTEMPT);
    assert!(
        last[0].1 > first[0].1,
        "the lease renewed: {last:?} after {first:?}\n{seen}"
    );
    let clock: i64 = conn
        .query_row("SELECT fleet_clock_s FROM fleetd WHERE id = 1", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert!(clock >= CLOCK + 600, "10 min of the fleet clock: {clock}");
    assert!(last[0].1 > clock, "the lease is not expired\n{seen}");
}
