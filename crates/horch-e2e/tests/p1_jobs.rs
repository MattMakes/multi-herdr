//! Phase 1 gate tests for jobs (`docs/specs/fleetd/design.md`, "Migration
//! plan" item 3; "Supervision, liveness and reconciliation": "Long work is a
//! job. A pass never waits for it"; FM-12).
//!
//! Each test starts 1 real fleetd in stand-in mode on a temp ledger and
//! queues 1 `task_check` job through the jobs API (Phase 1 queues no job by
//! itself). The check runs through the fake `codex sandbox`, as the check of
//! a Codex worker does (SR-4). The tests stand in for the service manager:
//! they call `horch fleetd check` themselves, as its 30 s timer does.

#![cfg(unix)]

use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

use horch_core::fleetd::fleet_runtime::adapters::clock_store::{self, awake_now};
use horch_core::fleetd::fleet_runtime::adapters::effects::read_heartbeat;
use horch_core::fleetd::fleet_runtime::adapters::request_wire::{parse_reply, Request};
use horch_core::fleetd::fleet_runtime::adapters::{config_file, jobs_store};
use horch_core::fleetd::fleet_runtime::application::jobs::{self, JobCtx, JobKey, Queued};
use horch_core::fleetd::fleet_runtime::domain::heartbeat::is_hung;
use horch_core::fleetd::fleet_runtime::domain::jobs::{Kind, State, NEEDS_YOU_KIND};
use horch_core::fleetd::fleet_runtime::domain::requests::{Kind as RequestKind, RequestId};
use horch_core::fleetd::ideas::application::needs_you::NeedsYouList;
use horch_core::procid;
use horch_core::taskledger::{finish, Store};
use horch_e2e::harness::Harness;
use horch_e2e::ledger_seed::{seed_run, seed_task};
use horch_platform::channel::Stream;

const SIGKILL: i32 = 9;
const SIGTERM: i32 = 15;

/// The persona of the worker of the task: a Codex teammate, so its check
/// has a sandbox profile (SR-4).
const CODEX_PERSONA: &str = "codex-sol";

/// The hang limit of the design: a heartbeat older than this is a hung
/// fleetd.
const HANG_LIMIT_S: u64 = 30;

fn text(o: &Output) -> String {
    format!(
        "status: {}\nstdout: {}\nstderr: {}",
        o.status,
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

fn ok(h: &Harness, args: &[&str]) -> String {
    let o = h.run(args);
    assert!(o.status.success(), "horch {}: {}", args.join(" "), text(&o));
    String::from_utf8_lossy(&o.stdout).into_owned()
}

/// A harness with its own ledger file and a git repository.
fn world(name: &str) -> Harness {
    let mut h = Harness::new(name).with_git();
    let db = h.root.join("ledger.sqlite3");
    h.set("HORCH_LEDGER_DB", db.to_string_lossy());
    h.set("HORCH_NO_WAKE", "1");
    h.set("HORCH_NO_WATCHDOG", "1");
    h
}

fn db(h: &Harness) -> PathBuf {
    h.root.join("ledger.sqlite3")
}

/// The fleet root of the harness: `<ledger dir>/fleet` (MAP.md, Q4).
fn fleet_root(h: &Harness) -> PathBuf {
    h.root.join("fleet")
}

/// The PID and start time of the fleetd that the `fleetd` row of the ledger
/// names. `None` while no row exists.
fn fleetd_row(db: &Path) -> Option<(u32, Option<u64>)> {
    let store = Store::open(db).ok()?;
    let (pid, started): (i64, i64) = store
        .conn()
        .query_row("SELECT pid, pid_start FROM fleetd WHERE id = 1", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .ok()?;
    Some((u32::try_from(pid).ok()?, u64::try_from(started).ok()))
}

/// Stop the process `pid` only while it is the one that started at `started`.
fn stop(pid: u32, started: Option<u64>, signal: i32) {
    if let Some(started) = started.or_else(|| procid::start_time(pid)) {
        procid::signal_same(pid, started, signal);
    }
}

/// Each fleetd of a test, the job processes of its ledger and the stand-in
/// agent of the worker. Drop stops them all, also when the test fails.
struct Fleetds {
    db: PathBuf,
    children: Vec<Child>,
    seen: Vec<(u32, Option<u64>)>,
}

impl Fleetds {
    fn new(h: &Harness) -> Self {
        Fleetds {
            db: db(h),
            children: Vec::new(),
            seen: Vec::new(),
        }
    }

    /// The fleetd that the `fleetd` row names, recorded for the teardown.
    fn current(&mut self) -> Option<(u32, Option<u64>)> {
        let row = fleetd_row(&self.db)?;
        if !self.seen.contains(&row) {
            self.seen.push(row);
        }
        Some(row)
    }
}

impl Drop for Fleetds {
    fn drop(&mut self) {
        let _ = self.current();
        for &(pid, started) in &self.seen {
            stop(pid, started, SIGKILL);
        }
        for child in &mut self.children {
            let _ = child.kill();
            let _ = child.wait();
        }
        // A job process stops the group of its check on a SIGTERM, so no
        // `sleep` of a check outlives the test.
        if let Ok(store) = Store::open(&self.db) {
            for row in jobs_store::running(store.conn()).unwrap_or_default() {
                if let Some(pid) = row.pid {
                    stop(pid, row.pid_start, SIGTERM);
                }
            }
        }
    }
}

/// Start `horch fleetd` in the sealed environment of `h`, and wait until the
/// `fleetd` row names it. Returns its PID.
fn start_fleetd(h: &Harness, fleetds: &mut Fleetds) -> u32 {
    let out = h.root.join("fleetd.stdout");
    let err = h.root.join("fleetd.stderr");
    let mut cmd = Command::new(Harness::horch_bin());
    h.seal(&mut cmd);
    let child = cmd
        .arg("fleetd")
        .stdin(Stdio::null())
        .stdout(File::create(&out).unwrap())
        .stderr(File::create(&err).unwrap())
        .spawn()
        .unwrap();
    let pid = child.id();
    fleetds.children.push(child);
    let until = Instant::now() + Duration::from_secs(30);
    loop {
        if fleetds.current().is_some_and(|(p, _)| p == pid) {
            return pid;
        }
        let child = fleetds.children.last_mut().unwrap();
        if let Some(status) = child.try_wait().unwrap() {
            panic!(
                "horch fleetd exited at start: status: {status}\nstdout: {}\nstderr: {}",
                std::fs::read_to_string(&out).unwrap_or_default(),
                std::fs::read_to_string(&err).unwrap_or_default()
            );
        }
        assert!(
            Instant::now() < until,
            "horch fleetd (PID {pid}) runs, but after 30 s the fleetd row of the ledger names {:?}",
            fleetd_row(&fleetds.db)
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn checked_task(h: &Harness, fleetds: &mut Fleetds, check: &str) -> String {
    let run = seed_run(h, "jobs", None, "w1:p1");
    finish::set_default_check(Store::open(&db(h)).unwrap().conn(), &run.run_id, check).unwrap();
    let dir = h.project.join("tickets");
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join("t1.md");
    std::fs::write(&p, "# T1\n").unwrap();
    let seeded = seed_task(h, &run, &p, CODEX_PERSONA);
    run_check_in_shared_checkout(h, &seeded.task_id);
    let agent = Command::new("sleep").arg("3600").spawn().unwrap();
    let agent_pid = agent.id();
    fleetds.children.push(agent);
    horch_e2e::bind::bind(h, CODEX_PERSONA, Some(agent_pid));
    Store::open(&db(h))
        .unwrap()
        .conn()
        .query_row("SELECT task_id FROM tasks WHERE seq = 1", [], |r| r.get(0))
        .unwrap()
}

fn run_check_in_shared_checkout(h: &Harness, task_id: &str) {
    Store::open(&db(h))
        .unwrap()
        .conn()
        .execute(
            "UPDATE tasks SET worktree_mode = 'shared' WHERE task_id = ?1",
            [task_id],
        )
        .unwrap();
}

/// Queue 1 `task_check` job of `task` at the fleet clock, through the jobs
/// API, as a pass of a later phase does. With `time_limit_s`, write that
/// limit in the new row in the same transaction, before a pass can start
/// it. Returns the id of the job.
fn queue_check(h: &Harness, task: &str, time_limit_s: Option<u64>) -> i64 {
    let config = config_file::load(&fleet_root(h)).unwrap();
    let ctx = JobCtx {
        guard: None,
        config: &config,
        needs_you: &NeedsYouList,
    };
    let key = JobKey {
        kind: Kind::TaskCheck,
        subject: task,
        input_key: "tip1",
    };
    let mut store = Store::open(&db(h)).unwrap();
    store
        .immediate(|tx| {
            let clock = clock_store::read(tx)?;
            let Queued::New(id) = jobs::queue(&ctx, tx, &key, None, clock)? else {
                anyhow::bail!("the task_check job of {task} was not queued");
            };
            if let Some(s) = time_limit_s {
                tx.execute(
                    "UPDATE jobs SET time_limit_s = ?1 WHERE id = ?2",
                    [i64::try_from(s)?, id],
                )?;
            }
            Ok(id)
        })
        .unwrap()
}

/// Each `task_check` job row, in the order of its run. The `worktree_add`
/// job of `horch ledger worktree new` is not a check, so it does not count.
fn job_rows(db: &Path) -> Vec<jobs_store::JobRow> {
    let store = Store::open(db).unwrap();
    let ids: Vec<i64> = {
        let mut stmt = store
            .conn()
            .prepare("SELECT id FROM jobs WHERE kind = 'task_check' ORDER BY run")
            .unwrap();
        let ids = stmt
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        ids
    };
    ids.into_iter()
        .map(|id| jobs_store::get(store.conn(), id).unwrap().unwrap())
        .collect()
}

/// The count of needs-you items of abandoned jobs in `state`.
fn needs_you(db: &Path, state: &str) -> i64 {
    Store::open(db)
        .unwrap()
        .conn()
        .query_row(
            "SELECT count(*) FROM needs_you WHERE kind = ?1 AND state = ?2",
            [NEEDS_YOU_KIND, state],
            |r| r.get(0),
        )
        .unwrap()
}

/// The job rows and the output of fleetd, for a failure message.
fn evidence(h: &Harness) -> String {
    let log = |f: &str| std::fs::read_to_string(h.root.join(f)).unwrap_or_default();
    format!(
        "jobs: {:#?}\nfleetd stdout: {}\nfleetd stderr: {}",
        job_rows(&db(h)),
        log("fleetd.stdout"),
        log("fleetd.stderr")
    )
}

/// Poll `f` every 200 ms until it gives a value, for at most `limit`.
fn wait_for<T>(h: &Harness, what: &str, limit: Duration, mut f: impl FnMut() -> Option<T>) -> T {
    let until = Instant::now() + limit;
    loop {
        if let Some(v) = f() {
            return v;
        }
        assert!(
            Instant::now() < until,
            "timed out after {limit:?} waiting for {what}\n{}",
            evidence(h)
        );
        std::thread::sleep(Duration::from_millis(200));
    }
}

#[test]
fn job_of_5_min_runs_while_fleetd_stays_healthy_and_is_not_restarted() {
    let h = world("p1-job-5min");
    let mut fleetds = Fleetds::new(&h);
    let task = checked_task(&h, &mut fleetds, "/bin/sleep 300");
    start_fleetd(&h, &mut fleetds);
    let fleetd = fleetds.current().expect("the fleetd row");
    let id = queue_check(&h, &task, None);
    let job = |db: &Path| {
        jobs_store::get(Store::open(db).unwrap().conn(), id)
            .unwrap()
            .unwrap()
    };
    wait_for(&h, "the job to run", Duration::from_secs(30), || {
        (job(&db(&h)).state == State::Running).then_some(())
    });
    let started = Instant::now();
    let mut checked: Option<Instant> = None;
    let end = loop {
        // The timer of the service manager runs `horch fleetd check` every
        // 30 s. A healthy fleetd is not restarted.
        if checked.is_none_or(|t| t.elapsed() >= Duration::from_secs(30)) {
            let o = h.run(&["fleetd", "check"]);
            assert!(o.status.success(), "horch fleetd check: {}", text(&o));
            checked = Some(Instant::now());
        }
        assert_eq!(
            fleetds.current(),
            Some(fleetd),
            "the PID or the start time of the fleetd row changed after {:?} of the job",
            started.elapsed()
        );
        assert!(
            procid::alive(fleetd.0, fleetd.1),
            "fleetd (PID {}) ended after {:?} of the job",
            fleetd.0,
            started.elapsed()
        );
        let hb = read_heartbeat(&fleet_root(&h))
            .unwrap()
            .expect("the heartbeat of fleetd");
        assert!(
            !is_hung(&hb, &awake_now().unwrap(), HANG_LIMIT_S),
            "the heartbeat of fleetd is older than {HANG_LIMIT_S} s after {:?} of the job: {}",
            started.elapsed(),
            hb.at
        );
        let row = job(&db(&h));
        if row.state != State::Running {
            break row;
        }
        assert!(
            started.elapsed() < Duration::from_secs(420),
            "the job of `sleep 300` still runs after {:?}",
            started.elapsed()
        );
        std::thread::sleep(Duration::from_secs(1));
    };
    assert_eq!(
        end.state,
        State::Passed,
        "the job of `sleep 300` ended {:?}: {:?}",
        end.state,
        end.result_json
    );
    assert!(
        started.elapsed() >= Duration::from_secs(290),
        "the job of `sleep 300` ended after only {:?}: {:?}",
        started.elapsed(),
        end.result_json
    );
}

#[test]
fn job_past_its_limit_runs_again_after_its_backoff_and_after_3_abandoned_runs_goes_to_you() {
    let h = world("p1-job-backoff");
    write_short_job_config(&h);
    let mut fleetds = Fleetds::new(&h);
    let task = checked_task(&h, &mut fleetds, "/bin/sleep 600");
    start_fleetd(&h, &mut fleetds);
    // `task_check` has the fixed time limit of today, 1200 s (CHECK_TIMEOUT),
    // and no key in `job_limit_s`. So the test writes a limit of 5 s in the
    // row of run 1. Each next run copies the limit of the abandoned run.
    let limit = 5;
    let first = queue_check(&h, &task, Some(limit));
    let ended = wait_for(
        &h,
        "3 abandoned runs and 1 open needs-you item",
        Duration::from_secs(90),
        || {
            let rows = job_rows(&db(&h));
            let abandoned = rows.iter().filter(|r| r.state == State::Abandoned).count();
            (abandoned == 3 && needs_you(&db(&h), "open") == 1).then_some(rows)
        },
    );
    assert_eq!(ended[0].id, first);
    assert_3_abandoned_runs(&ended, limit);
    assert_each_run_waited_for_its_backoff(&ended);
    assert_no_4th_run_starts(&h, &ended[2]);
    assert_requeue_starts_run_4(&h, ended[2].id);
}

/// The backoff before run 2, 3 and 4, in seconds.
const BACKOFF_S: [u64; 3] = [1, 2, 4];

/// A `fleet.toml` with short numbers, so the test ends in less than 1 min.
/// The rule is the same with the starting values.
fn write_short_job_config(h: &Harness) {
    let backoff: Vec<String> = BACKOFF_S.iter().map(u64::to_string).collect();
    std::fs::create_dir_all(fleet_root(h)).unwrap();
    std::fs::write(
        fleet_root(h).join("fleet.toml"),
        format!(
            "pass_period_s = 1\nstop_grace_s = 1\nabandoned_run_limit = 3\njob_backoff_s = [{}]\n",
            backoff.join(", ")
        ),
    )
    .unwrap();
}

/// Runs 1 to 3 ended `abandoned` with no result, each with the count of
/// abandoned runs so far and the limit of run 1.
fn assert_3_abandoned_runs(ended: &[jobs_store::JobRow], limit: u64) {
    assert_eq!(ended.len(), 3, "{ended:#?}");
    for (i, row) in ended.iter().enumerate() {
        let run = i as i64 + 1;
        assert_eq!(row.run, run, "{ended:#?}");
        assert_eq!(row.state, State::Abandoned, "run {run}: {row:#?}");
        assert_eq!(row.abandoned_runs, run as u64, "run {run}: {row:#?}");
        assert_eq!(
            row.time_limit_s, limit,
            "run {run} copies the limit: {row:#?}"
        );
        assert!(row.result_json.is_none(), "run {run}: {row:#?}");
    }
}

/// Each next run starts only after its backoff on the fleet clock: its run
/// before it ended at its deadline or later, and its start on the fleet
/// clock is its deadline minus its time limit.
fn assert_each_run_waited_for_its_backoff(ended: &[jobs_store::JobRow]) {
    for pair in ended.windows(2) {
        let (before, next) = (&pair[0], &pair[1]);
        let wait = BACKOFF_S[before.abandoned_runs as usize - 1] as i64;
        let not_before = next.not_before.expect("the backoff of the next run").0;
        let before_deadline = before.deadline.expect("the deadline of the run").0;
        let start = next.deadline.expect("the deadline of the run").0 - next.time_limit_s as i64;
        assert!(
            not_before >= before_deadline + wait,
            "run {} may start at {not_before}, before the end of run {} at {before_deadline} plus its backoff of {wait} s",
            next.run,
            before.run
        );
        assert!(
            start >= not_before,
            "run {} started at {start} on the fleet clock, before the end of its backoff at {not_before}",
            next.run
        );
    }
}

fn assert_no_4th_run_starts(h: &Harness, third: &jobs_store::JobRow) {
    let until = third.deadline.expect("the deadline of run 3").0 + BACKOFF_S[2] as i64 + 2;
    wait_for(
        h,
        "the fleet clock to pass the backoff of run 4",
        Duration::from_secs(30),
        || {
            let store = Store::open(&db(h)).unwrap();
            (clock_store::read(store.conn()).unwrap().0 >= until).then_some(())
        },
    );
    let rows = job_rows(&db(h));
    assert_eq!(
        rows.len(),
        3,
        "a 4th run started with no requeue: {rows:#?}"
    );
    assert_eq!(needs_you(&db(h), "open"), 1);
}

/// The exit of the needs-you list: `horch job requeue <job>` starts run 4
/// with the count at 0, and resolves the item.
fn assert_requeue_starts_run_4(h: &Harness, last: i64) {
    ok(h, &["job", "requeue", &last.to_string()]);
    let fourth = wait_for(h, "run 4 to run", Duration::from_secs(30), || {
        job_rows(&db(h))
            .into_iter()
            .find(|r| r.run == 4 && r.state == State::Running)
    });
    assert_eq!(fourth.abandoned_runs, 0, "{fourth:#?}");
    assert_eq!(needs_you(&db(h), "open"), 0);
    assert_eq!(needs_you(&db(h), "resolved"), 1);
}

fn tokenless_request_line(id: &str) -> String {
    Request {
        request_id: RequestId::parse(id).unwrap(),
        kind: RequestKind::SpawnGateNote,
        payload: serde_json::Map::new(),
        token: None,
    }
    .to_line()
}

fn connect_and_send(channel: &Path, id: &str) -> Stream {
    let stream = Stream::connect(channel, Instant::now() + Duration::from_secs(5)).unwrap();
    let mut writer = &stream;
    writer
        .write_all(tokenless_request_line(id).as_bytes())
        .and_then(|()| writer.flush())
        .unwrap();
    stream
}

#[test]
fn running_job_and_non_reading_channel_client_do_not_block_passes_or_bounded_fleetd_stop() {
    let h = world("p1-job-channel");
    write_short_job_config(&h);
    let mut fleetds = Fleetds::new(&h);
    let task = checked_task(&h, &mut fleetds, "/bin/sleep 60");
    start_fleetd(&h, &mut fleetds);
    let fleetd = fleetds.current().expect("the fleetd row");
    let id = queue_check(&h, &task, None);
    let job = |db: &Path| {
        jobs_store::get(Store::open(db).unwrap().conn(), id)
            .unwrap()
            .unwrap()
    };
    let channel = fleet_root(&h).join("run/fleetd.sock");
    wait_for(
        &h,
        "the job to run and the channel to exist",
        Duration::from_secs(30),
        || (job(&db(&h)).state == State::Running && channel.exists()).then_some(()),
    );

    let stalled = connect_and_send(&channel, "00000000-0000-4000-8000-000000000001");
    let hb0 = read_heartbeat(&fleet_root(&h))
        .unwrap()
        .expect("the heartbeat of fleetd");

    let two_pass_periods_ms = 2_000;
    wait_for(
        &h,
        "the heartbeat to advance while a client does not read",
        Duration::from_secs(15),
        || {
            let hb = read_heartbeat(&fleet_root(&h)).unwrap()?;
            let advanced = hb.awake.awake_ms >= hb0.awake.awake_ms + two_pass_periods_ms;
            (advanced && !is_hung(&hb, &awake_now().unwrap(), HANG_LIMIT_S)).then_some(())
        },
    );
    assert_eq!(job(&db(&h)).state, State::Running);

    let second = connect_and_send(&channel, "00000000-0000-4000-8000-000000000002");
    second.set_read_timeout(Duration::from_secs(10)).unwrap();
    let mut line = Vec::new();
    BufReader::new(&second)
        .read_until(b'\n', &mut line)
        .unwrap();
    let reply = parse_reply(&line, 65_536).unwrap();
    assert_eq!(
        reply.request_id.as_str(),
        "00000000-0000-4000-8000-000000000002"
    );
    ok(&h, &["fleetd", "check"]);
    let board = h.run(&["ledger", "status"]);
    let (out, err) = (
        String::from_utf8_lossy(&board.stdout),
        String::from_utf8_lossy(&board.stderr),
    );
    assert!(
        out.starts_with("Run jobs (active).")
            && out.trim_end().ends_with("Not complete.")
            && err.is_empty(),
        "horch ledger status exits 1 on an incomplete run, and must print only the board: {}",
        text(&board)
    );
    assert_eq!(
        fleetds.current(),
        Some(fleetd),
        "the check restarted fleetd while a client did not read"
    );

    stop(fleetd.0, fleetd.1, SIGTERM);
    let child = fleetds
        .children
        .iter_mut()
        .find(|c| c.id() == fleetd.0)
        .expect("the child of fleetd");
    let status = wait_for(
        &h,
        "fleetd to end after SIGTERM",
        Duration::from_secs(10),
        || child.try_wait().unwrap(),
    );
    assert!(
        !procid::alive(fleetd.0, fleetd.1),
        "fleetd (PID {}) ended with {status} but its process is alive",
        fleetd.0
    );
    drop(stalled);
}
