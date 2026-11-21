//! The wiring of Phase 5 into the pass of fleetd: the operator token, the
//! channel server, the request files, the outbox and wrapper files and the
//! job result files (docs/tickets/fleetd-p5/41-pass-wiring-phase5.md).
//!
//! `docs/specs/fleetd/design.md`, Migration plan item 7, "The command
//! channel" and "Workspace layout and file protocol"; RR-5, RR-8, SR-5,
//! SR-6, FM-15, FM-23 and Q3 of `docs/tickets/fleetd-p5/MAP.md`.
//!
//! Each test uses a temp ledger at generation 1 and a temp fleet root, and
//! runs the start and the passes as library calls with no herdr snapshot.
//! The time of a budget comes from a test clock, never from the host.
#![cfg(unix)]

use std::collections::BTreeMap;
use std::fs::{self, OpenOptions, Permissions};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use horch_core::fleetd::fleet_runtime::adapters::channel_server::socket_path;
use horch_core::fleetd::fleet_runtime::adapters::effects;
use horch_core::fleetd::fleet_runtime::adapters::job_result_file::{self, JobResult};
use horch_core::fleetd::fleet_runtime::adapters::jobs_store;
use horch_core::fleetd::fleet_runtime::adapters::request_wire::{parse_reply, Outcome, Request};
use horch_core::fleetd::fleet_runtime::adapters::scans::{requests_dir, FolderItem};
use horch_core::fleetd::fleet_runtime::adapters::token_file;
use horch_core::fleetd::fleet_runtime::application::jobs::{self, JobCtx, JobKey, Queued};
use horch_core::fleetd::fleet_runtime::application::pass::{
    NoDuties, Outside, Pass, PassReport, Ports, Settings, TOKEN_REFUSED,
};
use horch_core::fleetd::fleet_runtime::application::requests::Handlers;
use horch_core::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;
use horch_core::fleetd::fleet_runtime::domain::health::Hold;
use horch_core::fleetd::fleet_runtime::domain::jobs::{Kind as JobKind, State};
use horch_core::fleetd::fleet_runtime::domain::ledger::Generation;
use horch_core::fleetd::fleet_runtime::domain::requests::{Kind, RequestId};
use horch_core::fleetd::ideas::application::needs_you::NeedsYouList;
use horch_core::fleetd::task_execution::domain::outbox::{LineKind, ProgressLine};
use horch_core::fleetd::task_execution::domain::result::{ResultFile, ResultStatus};
use horch_core::fleetd::task_execution::domain::task::{AttemptRow, InPane, Recorded};
use horch_core::fleetd::worker_profiles::adapters::launcher::Launcher;
use horch_core::fleetd::worker_profiles::application::ports::LaunchSpec;
use horch_core::runtime::fault::Faults;
use horch_core::taskledger::model::{Actor, WorkerKind, WorktreeMode};
use horch_core::taskledger::runs::{self, NewRun, Run};
use horch_core::taskledger::supervisor::{
    ExitFile, HeartbeatFile, OutcomeHint, EXIT_JSON, HEARTBEAT_JSON,
};
use horch_core::taskledger::tasks::{self, NewTask};
use horch_core::taskledger::testing::bind_live;
use horch_core::taskledger::workers::{self, NewWorker};
use horch_core::taskledger::{schema, util};
use horch_platform::channel::Stream;
use rusqlite::{params, Connection};
use serde_json::{json, Value};

// ─── the world ──────────────────────────────────────────────────────────

/// A temp ledger at generation 1, a fleet root and 1 run.
struct World {
    _temp: tempfile::TempDir,
    dir: PathBuf,
    conn: Connection,
    run: Run,
    config: FleetConfig,
    /// The calls of the `task_cancel` handler.
    calls: Arc<AtomicUsize>,
}

fn open(db: &Path) -> Connection {
    let conn = Connection::open(db).unwrap();
    conn.busy_timeout(Duration::from_secs(10)).unwrap();
    conn
}

fn world() -> World {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().to_path_buf();
    let mut conn = open(&dir.join("ledger.sqlite3"));
    schema::migrate_to(&mut conn, schema::VERSION).unwrap();
    conn.execute(
        "INSERT INTO fleetd (id, generation, pid, pid_start, started_at, fleet_clock_s, clock_ticked_at)
         VALUES (1, 1, ?1, 0, ?2, 0, ?2)",
        params![std::process::id(), util::now()],
    )
    .unwrap();
    let new_run = NewRun {
        slug: "r1".into(),
        ..Default::default()
    };
    let run = runs::create(&conn, &dir, &new_run).unwrap();
    // FR-12: a task needs an idea in flight of the feature of its run.
    horch_core::taskledger::schema::run_idea(
        &conn,
        &run.run_id,
        &horch_core::taskledger::util::now(),
        true,
    )
    .unwrap();
    fs::create_dir_all(dir.join("fleet")).unwrap();
    World {
        _temp: temp,
        dir,
        conn,
        run,
        config: FleetConfig::default(),
        calls: Arc::new(AtomicUsize::new(0)),
    }
}

impl World {
    fn root(&self) -> PathBuf {
        self.dir.join("fleet")
    }

    fn settings(&self) -> Settings {
        Settings {
            fleet_root: self.root(),
            generation: Generation(1),
            config: self.config.clone(),
            horch: self.job_program(),
            faults: Faults::default(),
        }
    }

    /// The program of each job process: it exits at once and writes no
    /// result file. The test writes the result file.
    fn job_program(&self) -> PathBuf {
        let path = self.dir.join("job.sh");
        if !path.exists() {
            fs::write(&path, "#!/bin/sh\nexit 0\n").unwrap();
            horch_platform::fs::make_executable(&path).unwrap();
        }
        path
    }

    /// Handlers with 1 handler for `task_cancel` that counts its calls.
    fn handlers(&self) -> Handlers {
        let calls = Arc::clone(&self.calls);
        let mut handlers = Handlers::new();
        handlers.insert(Kind::TaskCancel, move |_, _| {
            let n = calls.fetch_add(1, Ordering::SeqCst) + 1;
            Ok(json!({ "calls": n }))
        });
        handlers
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }

    fn pass<'a>(&self, outside: &'a Fake, ports_host: &'a Launcher) -> Pass<'a> {
        let ports = Ports {
            outside,
            duties: &NoDuties,
            needs_you: &NeedsYouList,
            host: ports_host,
        };
        Pass::new(self.settings(), ports).with_requests(self.handlers())
    }

    fn count(&self, sql: &str) -> i64 {
        self.conn.query_row(sql, [], |r| r.get(0)).unwrap()
    }

    /// A worker of the run in state `working`, with the agent `agent`.
    fn worker(&self, agent: Option<Recorded>) -> String {
        let new = NewWorker {
            run_id: self.run.run_id.clone(),
            kind: Some(WorkerKind::Worker),
            persona: "dev".into(),
            ..Default::default()
        };
        let id = workers::insert(&self.conn, &new).unwrap();
        self.conn
            .execute(
                "UPDATE workers SET status = 'spawning', agent_pid = ?2, agent_started = ?3
                 WHERE worker_id = ?1",
                params![
                    id,
                    agent.map(|a| a.pid),
                    agent.and_then(|a| a.pid_start).map(|s| s as i64)
                ],
            )
            .unwrap();
        id
    }

    /// A task `name` bound to a worker with the agent `agent`: 1 live
    /// attempt. Gives the task id and the attempt id.
    fn claimed_task(&self, name: &str, agent: Option<Recorded>) -> (String, String) {
        let ticket = format!("tickets/{name}.md");
        let mut new = NewTask::work(name, &ticket, "dev");
        new.worktree_mode = WorktreeMode::Shared;
        let task = tasks::add(&self.conn, &self.run, &new, Actor::Orchestrator).unwrap();
        let worker = NewWorker {
            run_id: self.run.run_id.clone(),
            kind: Some(WorkerKind::Worker),
            persona: "dev".into(),
            ..Default::default()
        };
        bind_live(&self.conn, &task.task_id, &worker, agent).unwrap();
        let attempt: String = self
            .conn
            .query_row(
                "SELECT id FROM attempts WHERE task_id = ?1",
                [&task.task_id],
                |r| r.get(0),
            )
            .unwrap();
        (task.task_id, attempt)
    }

    /// The folder of attempt `attempt` (`<task id>/A<n>`) of task `task`.
    fn attempt_dir(&self, task: &str, attempt: &str) -> PathBuf {
        let a = attempt.rsplit_once('/').map_or(attempt, |(_, a)| a);
        self.root()
            .join("tasks")
            .join(task)
            .join("attempts")
            .join(a)
    }

    fn outbox(&self, task: &str, attempt: &str) -> PathBuf {
        self.attempt_dir(task, attempt).join("outbox")
    }

    /// The `request_id` `id` as a request file with `token`, written as the
    /// client writes it while fleetd is down.
    fn put_request(&self, id: &str, token: &str) -> PathBuf {
        let dir = requests_dir(&self.root());
        make_dir(&dir);
        let line = request(id, Some(token)).to_line();
        let path = dir.join(format!("{id}.json"));
        let tmp = dir.join(format!("{id}.json.tmp"));
        write_mode(&tmp, line.as_bytes(), 0o600);
        fs::rename(&tmp, &path).unwrap();
        path
    }

    /// The state of the `requests` row of `id`.
    fn request_state(&self, id: &str) -> Option<String> {
        self.conn
            .query_row(
                "SELECT state FROM requests WHERE request_id = ?1",
                [id],
                |r| r.get(0),
            )
            .ok()
    }
}

/// Make the folder `dir` and its parents with mode 0700.
fn make_dir(dir: &Path) {
    fs::create_dir_all(dir).unwrap();
    let mut d = Some(dir);
    while let Some(p) = d {
        if p.ends_with("fleet") {
            break;
        }
        fs::set_permissions(p, Permissions::from_mode(0o700)).unwrap();
        d = p.parent();
    }
}

fn write_mode(path: &Path, bytes: &[u8], mode: u32) {
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(mode)
        .open(path)
        .unwrap();
    file.write_all(bytes).unwrap();
    file.sync_all().unwrap();
}

fn append(path: &Path, text: &str) {
    let mut file = OpenOptions::new()
        .append(true)
        .create(true)
        .mode(0o600)
        .open(path)
        .unwrap();
    file.write_all(text.as_bytes()).unwrap();
}

fn request(id: &str, token: Option<&str>) -> Request {
    let Value::Object(payload) = json!({ "task": "T-1" }) else {
        unreachable!()
    };
    Request {
        request_id: RequestId::parse(id).unwrap(),
        kind: Kind::TaskCancel,
        payload,
        token: token.map(str::to_string),
    }
}

fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// 1 progress line of `attempt` of `task`.
fn progress_line(task: &str, attempt: &str, text: &str) -> String {
    ProgressLine {
        task: task.into(),
        attempt: attempt.into(),
        at: util::now(),
        kind: LineKind::Progress,
        text: text.into(),
    }
    .to_line(4096)
    .unwrap()
}

/// The attempt part `A<n>` of an attempt id.
fn short(attempt: &str) -> &str {
    attempt.rsplit_once('/').map_or(attempt, |(_, a)| a)
}

/// A process of the test that sleeps.
struct Proc(Child);

impl Proc {
    fn sleep() -> Proc {
        Proc(Command::new("sleep").arg("60").spawn().unwrap())
    }

    fn recorded(&self) -> Recorded {
        let pid = self.0.id();
        Recorded {
            pid,
            pid_start: horch_platform::process::start_time(pid),
        }
    }
}

impl Drop for Proc {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// The outside of the tests: no herdr snapshot, no health hold, no
/// worktree folder.
struct Fake;

impl Outside for Fake {
    fn panes(&self) -> Option<BTreeMap<String, InPane>> {
        None
    }

    fn close_pane(&self, _pane: &str) -> Result<()> {
        Ok(())
    }

    fn holds(&self) -> Vec<Hold> {
        Vec::new()
    }

    fn worktree_folders(&self, _conn: &Connection) -> Result<Vec<FolderItem>> {
        Ok(Vec::new())
    }

    fn attempt_spec(&self, _attempt: &AttemptRow) -> Result<LaunchSpec> {
        anyhow::bail!("no launch in these tests")
    }
}

fn run(pass: &mut Pass<'_>, w: &mut World) -> PassReport {
    pass.run(&mut w.conn).unwrap()
}

// ─── the start ──────────────────────────────────────────────────────────

/// At its start fleetd makes the operator token (mode 0600), and its
/// channel applies an operator request with that token.
#[test]
fn fleetd_start_makes_the_token_and_serves_the_channel() {
    let mut w = world();
    let mut pass = w.pass(&Fake, &Launcher);

    let failed = pass.start(&mut w.conn).unwrap();
    assert!(failed.is_empty(), "{failed:?}");
    let token_path = token_file::path(&w.root());
    let mode = fs::metadata(&token_path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600, "the token file has mode 0600");
    let token = token_file::read(&w.root()).unwrap();

    let server = pass
        .serve(open(&w.dir.join("ledger.sqlite3")), w.handlers())
        .unwrap();
    let id = new_id();
    let line = request(&id, Some(token.text())).to_line();
    let mut stream = Stream::connect(
        &socket_path(&w.root()),
        Instant::now() + Duration::from_secs(10),
    )
    .unwrap();
    stream.write_all(line.as_bytes()).unwrap();
    stream.write_all(b"\n").unwrap();
    let mut reply = Vec::new();
    BufReader::new(&mut stream)
        .read_until(b'\n', &mut reply)
        .unwrap();
    let reply = parse_reply(&reply, 64 * 1024).unwrap();
    server.stop().unwrap();

    assert_eq!(
        reply.outcome,
        Outcome::Applied(json!({ "calls": 1 })),
        "the channel applies the request"
    );
    assert_eq!(w.request_state(&id).as_deref(), Some("applied"));
}

/// A request file that a `horch` command wrote while fleetd was down is
/// applied at the start, 1 time, and archived.
#[test]
fn request_file_written_while_fleetd_is_down_is_applied_at_start() {
    let mut w = world();
    let token = token_file::ensure(&w.root()).unwrap();
    let id = new_id();
    let path = w.put_request(&id, token.text());
    let mut pass = w.pass(&Fake, &Launcher);

    let failed = pass.start(&mut w.conn).unwrap();

    assert!(failed.is_empty(), "{failed:?}");
    assert_eq!(w.request_state(&id).as_deref(), Some("applied"));
    assert_eq!(w.calls(), 1, "the handler ran 1 time");
    assert!(!path.exists(), "the file left the request folder");
    let archived = requests_dir(&w.root())
        .join("archive")
        .join(format!("{id}.json"));
    assert!(archived.exists(), "the file is in archive/");

    run(&mut pass, &mut w);
    assert_eq!(w.calls(), 1, "a pass applies the request no second time");
}

/// A token file that fails its checks opens 1 needs-you item, and no
/// operator request is applied until it is corrected.
#[test]
fn bad_token_file_opens_1_needs_you_item_and_refuses_operator_requests() {
    let mut w = world();
    let token = token_file::path(&w.root());
    make_dir(token.parent().unwrap());
    let text = "a".repeat(64);
    write_mode(&token, text.as_bytes(), 0o644);
    let id = new_id();
    w.put_request(&id, &text);
    let mut pass = w.pass(&Fake, &Launcher);

    pass.start(&mut w.conn).unwrap();
    run(&mut pass, &mut w);
    run(&mut pass, &mut w);

    let open =
        format!("SELECT count(*) FROM needs_you WHERE kind = '{TOKEN_REFUSED}' AND state = 'open'");
    assert_eq!(w.count(&open), 1, "1 needs-you item names the token file");
    assert_eq!(w.request_state(&id).as_deref(), Some("refused"));
    assert_eq!(w.calls(), 0, "no operator request is applied");
    assert_eq!(
        fs::read_to_string(&token).unwrap(),
        text,
        "fleetd never replaces a token file"
    );

    fs::set_permissions(&token, Permissions::from_mode(0o600)).unwrap();
    run(&mut pass, &mut w);
    assert_eq!(w.count(&open), 0, "the item ends when the file is correct");
}

// ─── the files of the attempts ──────────────────────────────────────────

/// The pass reads the outbox and the wrapper files of a live attempt and
/// writes their rows: the progress line, the result and the heartbeat.
#[test]
fn pass_ingests_progress_result_and_heartbeat() {
    let mut w = world();
    let agent = Proc::sleep();
    let (task, attempt) = w.claimed_task("a", Some(agent.recorded()));
    let outbox = w.outbox(&task, &attempt);
    make_dir(&outbox);
    append(
        &outbox.join("progress.jsonl"),
        &progress_line(&task, short(&attempt), "milestone one"),
    );
    let result = ResultFile {
        task: task.clone(),
        attempt: short(&attempt).into(),
        status: ResultStatus::Completed,
        summary: "done".into(),
        commits: Vec::new(),
        files_changed: Vec::new(),
        acceptance_self_check: Vec::new(),
        notes_for_next: None,
        handoff: None,
        unlanded: None,
    };
    write_mode(
        &outbox.join("result.json"),
        result.to_json().as_bytes(),
        0o600,
    );
    let hb = HeartbeatFile {
        task: task.clone(),
        attempt: short(&attempt).into(),
        pid: None,
        pgid: None,
        mux_pane: None,
        proc_state: None,
        agent_state: None,
        last_output_at: None,
        tokens: None,
        seq: 1,
        written_at: util::now(),
    };
    let hb_path = w.attempt_dir(&task, &attempt).join(HEARTBEAT_JSON);
    write_mode(
        &hb_path,
        serde_json::to_string(&hb).unwrap().as_bytes(),
        0o600,
    );
    let mut pass = w.pass(&Fake, &Launcher);

    run(&mut pass, &mut w);

    let progress = format!(
        "SELECT count(*) FROM events WHERE task_id = '{task}' AND kind = 'progress' AND text = 'milestone one'"
    );
    assert_eq!(w.count(&progress), 1, "the progress line is a row");
    let status: String = w
        .conn
        .query_row(
            "SELECT status FROM tasks WHERE task_id = ?1",
            [&task],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(status, "verifying", "the result moves the task");
    let (seq, checkin): (Option<i64>, Option<String>) = w
        .conn
        .query_row(
            "SELECT a.heartbeat_seq, wk.last_checkin_at FROM attempts a
             JOIN workers wk ON wk.worker_id = a.worker_id WHERE a.id = ?1",
            [&attempt],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(seq, Some(1), "the heartbeat is used");
    assert!(
        checkin.is_some(),
        "the heartbeat is the checkin of the wrapper"
    );
}

/// At its first pass fleetd makes the outbox of each live attempt that has
/// none, with mode 0700, so an old worker writes files from then (Q3).
/// RR-11 in exec mode: the wrapper ends a push attempt at a usage limit
/// and writes `exit.json` with the outcome hint `quota`. The agent exited
/// before fleetd recorded its wrapper. The pass ends the attempt by rung 5
/// at once, with `quota`, which burns no attempt, and not with `no_result`
/// or `launch_failed`.
#[test]
fn exit_with_the_quota_hint_ends_the_attempt_with_quota() {
    let mut w = world();
    let (task, attempt) = w.claimed_task("q", None);
    w.conn
        .execute("UPDATE attempts SET via = 'push' WHERE id = ?1", [&attempt])
        .unwrap();
    let exit = ExitFile {
        task: task.clone(),
        attempt: short(&attempt).into(),
        code: Some(1),
        repaired: false,
        at: util::now(),
        outcome_hint: Some(OutcomeHint::Quota),
    };
    make_dir(&w.attempt_dir(&task, &attempt));
    let path = w.attempt_dir(&task, &attempt).join(EXIT_JSON);
    write_mode(
        &path,
        serde_json::to_string(&exit).unwrap().as_bytes(),
        0o600,
    );
    let mut pass = w.pass(&Fake, &Launcher);

    run(&mut pass, &mut w);

    let (state, outcome): (String, Option<String>) = w
        .conn
        .query_row(
            "SELECT state, outcome FROM attempts WHERE id = ?1",
            [&attempt],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(state, "ended");
    assert_eq!(outcome.as_deref(), Some("quota"));
    let worker: String = w
        .conn
        .query_row(
            "SELECT w.status FROM workers w JOIN attempts a ON a.worker_id = w.worker_id
             WHERE a.id = ?1",
            [&attempt],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(worker, "incomplete");
}

#[test]
fn first_pass_makes_the_outbox_of_each_live_attempt() {
    let mut w = world();
    let agent = Proc::sleep();
    let (task, attempt) = w.claimed_task("a", Some(agent.recorded()));
    let outbox = w.outbox(&task, &attempt);
    assert!(!outbox.exists());
    let mut pass = w.pass(&Fake, &Launcher);

    run(&mut pass, &mut w);

    let meta = fs::symlink_metadata(&outbox).expect("the outbox exists");
    assert!(meta.is_dir());
    assert_eq!(meta.permissions().mode() & 0o777, 0o700);
}

// ─── the job result files ───────────────────────────────────────────────

/// A job process writes its result file and exits; the next pass ends the
/// run from the file, not as abandoned, and archives the file.
#[test]
fn pass_ends_a_job_from_its_result_file() {
    let mut w = world();
    let id = {
        let tx = w.conn.transaction().unwrap();
        let ctx = JobCtx {
            guard: None,
            config: &w.config,
            needs_you: &NeedsYouList,
        };
        let key = JobKey {
            kind: JobKind::TaskCheck,
            subject: "T-1",
            input_key: "k",
        };
        let queued = jobs::queue(&ctx, &tx, &key, None, FleetSeconds(0)).unwrap();
        tx.commit().unwrap();
        match queued {
            Queued::New(id) => id,
            other => panic!("not queued: {other:?}"),
        }
    };
    let mut pass = w.pass(&Fake, &Launcher);
    run(&mut pass, &mut w);
    let row = jobs_store::get(&w.conn, id).unwrap().unwrap();
    assert_eq!(row.state, State::Running, "the pass started the job");
    let result = JobResult {
        job_id: id,
        run: row.run,
        passed: true,
        verdict: true,
        text: "ok".into(),
        data: None,
    };
    let file = job_result_file::write(&w.root(), &result).unwrap();

    run(&mut pass, &mut w);

    let row = jobs_store::get(&w.conn, id).unwrap().unwrap();
    assert_eq!(row.state, State::Passed, "the run ends from its file");
    assert!(!file.exists(), "the file left the folder");
    assert!(job_result_file::folder(&w.root())
        .join("archive")
        .join(file.file_name().unwrap())
        .exists());
}

// ─── the scan budget ────────────────────────────────────────────────────

/// With 10000 live attempts that each have a progress line, 1 pass reads
/// only the outboxes of its scan budget, and still writes its heartbeat
/// well inside the hang limit. The next pass continues at the cursor.
#[test]
fn heartbeat_stays_under_its_limit_with_10000_outboxes() {
    const N: usize = 10_000;
    let mut w = world();
    // A clock that moves 10 ms at each reading: a scan of 100 ms reads
    // about 10 outboxes.
    w.config.scan_budget_ms = 100;
    // 1 live agent for each attempt, so that no attempt ends.
    let agent = Proc::sleep();
    let worker = w.worker(Some(agent.recorded()));
    let tx = w.conn.transaction().unwrap();
    let mut attempts = Vec::new();
    for n in 0..N {
        let name = format!("t{n:05}");
        let new = NewTask::work(&name, &format!("tickets/{name}.md"), "dev");
        let task = tasks::add(&tx, &w.run, &new, Actor::Orchestrator)
            .unwrap()
            .task_id;
        let attempt = format!("{task}/A1");
        tx.execute(
            "INSERT INTO attempts (id, task_id, worker_id, state, started_at)
             VALUES (?1, ?2, ?3, 'live', ?4)",
            params![attempt, task, worker, util::now()],
        )
        .unwrap();
        attempts.push((task, attempt));
    }
    tx.commit().unwrap();
    for (task, attempt) in &attempts {
        let outbox = w.outbox(task, attempt);
        fs::create_dir_all(&outbox).unwrap();
        append(
            &outbox.join("progress.jsonl"),
            &progress_line(task, "A1", "one"),
        );
    }
    let start = Instant::now();
    let mut ms = 0u64;
    let readings = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&readings);
    let clock = Box::new(move || {
        seen.fetch_add(1, Ordering::SeqCst);
        ms += 10;
        start + Duration::from_millis(ms)
    });
    let mut pass = w.pass(&Fake, &Launcher).with_clock(clock);

    let report = run(&mut pass, &mut w);

    assert!(report.heartbeat, "the pass writes its heartbeat");
    let elapsed_ms = readings.load(Ordering::SeqCst) as u64 * 10;
    assert!(
        elapsed_ms < 30_000,
        "the pass ends inside the hang limit: {elapsed_ms} ms"
    );
    let read = "SELECT count(*) FROM ingested WHERE path LIKE '%progress.jsonl'";
    let first = w.count(read);
    assert!(
        first > 0 && first < 100,
        "the pass reads only the outboxes of its budget: {first}"
    );

    run(&mut pass, &mut w);
    let second = w.count(read);
    assert!(second > first, "the next pass continues at the cursor");
    assert!(effects::read_heartbeat(&w.root()).unwrap().is_some());
}
