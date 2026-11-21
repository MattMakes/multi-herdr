#![cfg(unix)]

use std::fs::{self, OpenOptions, Permissions};
use std::io::Write as _;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output};
use std::time::{Duration, Instant};

use horch_core::fleetd::fleet_runtime::adapters::channel_server::{ChannelServer, Options};
use horch_core::fleetd::fleet_runtime::adapters::effects::heartbeat_path;
use horch_core::fleetd::fleet_runtime::adapters::request_spool::{
    self, FileFact, Handled, ScanOutcome, Spool,
};
use horch_core::fleetd::fleet_runtime::adapters::scans::{outbox_dir, requests_dir, ScanClock};
use horch_core::fleetd::fleet_runtime::adapters::token_file;
use horch_core::fleetd::fleet_runtime::application::requests::Handlers;
use horch_core::fleetd::fleet_runtime::domain::budget::{Cursor, Deadline};
use horch_core::fleetd::fleet_runtime::domain::ledger::Generation;
use horch_core::fleetd::task_execution::domain::outbox::{LineKind, ProgressLine};
use horch_core::fleetd::task_execution::domain::result::{ResultFile, ResultStatus};
use horch_core::procid;
use horch_core::taskledger::model::{Actor, WorkerKind};
use horch_core::taskledger::runs::{self, NewRun, Run};
use horch_core::taskledger::tasks::{self, NewTask};
use horch_core::taskledger::workers::NewWorker;
use horch_core::taskledger::{schema, util};
use horch_e2e::fleetd::Fleetds;
use horch_e2e::harness::{wait_for, Harness};
use horch_platform::channel::User;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;

const RUN: &str = "r1";

const FLEET_TOML: &str = "pass_period_s = 1\n\
                          progress_max_bytes = 2048\n\
                          progress_line_max_bytes = 512\n";

const PROGRESS_MAX_BYTES: usize = 2048;

const OTHER_UID: u32 = 4_242_424;

struct Fleet {
    fleetds: Fleetds,
    h: Harness,
    run: Run,
    agents: Vec<Child>,
    started: usize,
}

struct Attempt {
    task: String,
    id: String,
}

impl Attempt {
    fn outbox(&self, f: &Fleet) -> PathBuf {
        outbox_dir(&f.root(), &self.task, "A1")
    }
}

impl Fleet {
    fn new(name: &str) -> Fleet {
        let mut h = Harness::new(name);
        let db = h.root.join("ledger.sqlite3");
        let root = h.root.join("fleet");
        h.set("HORCH_LEDGER_DB", db.to_string_lossy());
        h.set("FLEET_ROOT", root.to_string_lossy());
        h.set("HORCH_NO_WAKE", "1");
        h.set("HORCH_NO_WATCHDOG", "1");
        h.set("HORCH_TEST_ASSERT_READ_ONLY", "1");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("fleet.toml"), FLEET_TOML).unwrap();
        let mut conn = Connection::open(&db).unwrap();
        schema::migrate_to(&mut conn, schema::VERSION).unwrap();
        let run = runs::create(
            &conn,
            &h.project,
            &NewRun {
                slug: RUN.into(),
                ..Default::default()
            },
        )
        .unwrap();
        schema::run_idea(&conn, &run.run_id, &util::now(), true)
            .unwrap()
            .expect("a run that gets a task gets an idea");
        let fleetds = Fleetds::new(&h);
        Fleet {
            h,
            fleetds,
            run,
            agents: Vec::new(),
            started: 0,
        }
    }

    fn db(&self) -> PathBuf {
        self.h.root.join("ledger.sqlite3")
    }

    fn root(&self) -> PathBuf {
        self.h.root.join("fleet")
    }

    fn conn(&self) -> Connection {
        let conn = Connection::open(self.db()).unwrap();
        conn.busy_timeout(Duration::from_secs(10)).unwrap();
        conn
    }

    fn live_attempt(&mut self) -> Attempt {
        let agent = Command::new("/bin/sleep").arg("600").spawn().unwrap();
        let pid = agent.id();
        let start = procid::start_time(pid).expect("the start time of the agent");
        let conn = self.conn();
        let run = &self.run;
        let n: i64 = conn
            .query_row("SELECT count(*) FROM tasks", [], |r| r.get(0))
            .unwrap();
        let mut new = NewTask::work(&format!("t{n}"), &format!("tickets/t{n}.md"), "dev");
        new.worktree_mode = horch_core::taskledger::model::WorktreeMode::Shared;
        let task = tasks::add(&conn, run, &new, Actor::Orchestrator).unwrap();
        horch_core::taskledger::testing::bind_live(
            &conn,
            &task.task_id,
            &NewWorker {
                run_id: run.run_id.clone(),
                kind: Some(WorkerKind::Worker),
                persona: "dev".into(),
                ..Default::default()
            },
            None,
        )
        .unwrap();
        let id = format!("{}/A1", task.task_id);
        let set = conn
            .execute(
                "UPDATE attempts SET via = 'push', pid = ?2, pid_start = ?3 WHERE id = ?1",
                params![id, i64::from(pid), i64::try_from(start).unwrap()],
            )
            .unwrap();
        assert_eq!(set, 1, "the bind writes the attempt {id}");
        self.agents.push(agent);
        let attempt = Attempt {
            task: task.task_id,
            id,
        };
        let outbox = attempt.outbox(self);
        fs::create_dir_all(&outbox).unwrap();
        let tasks_dir = self.root().join("tasks");
        for dir in outbox.ancestors().take_while(|d| d.starts_with(&tasks_dir)) {
            fs::set_permissions(dir, Permissions::from_mode(0o700)).unwrap();
        }
        attempt
    }

    fn start_fleetd(&mut self) -> u32 {
        let name = format!("fleetd-{}", self.started);
        self.started += 1;
        let pid = self.fleetds.start(&self.h, &name);
        wait_for(&format!("the start heartbeat of fleetd PID {pid}"), || {
            self.heartbeat(pid)
        });
        self.passes(pid, 1);
        pid
    }

    fn heartbeat(&self, pid: u32) -> Option<u64> {
        let hb: Value =
            serde_json::from_slice(&fs::read(heartbeat_path(&self.root())).ok()?).ok()?;
        if hb["pid"].as_u64() != Some(u64::from(pid)) {
            return None;
        }
        hb["awake"]["awake_ms"].as_u64()
    }

    fn passes(&self, pid: u32, n: usize) {
        let mut last = self.heartbeat(pid);
        let mut seen = 0;
        wait_for(&format!("{n} more heartbeats of fleetd PID {pid}"), || {
            let at = self.heartbeat(pid)?;
            if last != Some(at) {
                seen += 1;
                last = Some(at);
            }
            (seen >= n).then_some(())
        });
    }

    fn ingested(&self, suffix: &str) -> Option<(String, Option<String>, String)> {
        let rows: Vec<(String, String, Option<String>, String)> = {
            let conn = self.conn();
            let mut stmt = conn
                .prepare("SELECT path, outcome, reason, at FROM ingested")
                .unwrap();
            let rows = stmt
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
                .unwrap()
                .collect::<rusqlite::Result<_>>()
                .unwrap();
            rows
        };
        let mut found = rows.into_iter().filter(|(path, ..)| path.ends_with(suffix));
        let row = found
            .next()
            .map(|(_, outcome, reason, at)| (outcome, reason, at));
        assert!(found.next().is_none(), "1 ingested row ends with {suffix}");
        row
    }

    fn rejected(&self, suffix: &str) -> String {
        let (outcome, reason, _) = wait_for(&format!("the ingested row of {suffix}"), || {
            self.ingested(suffix)
        });
        assert_eq!(outcome, "rejected", "{suffix}: {reason:?}");
        reason.expect("a rejected row has its reason")
    }

    fn request(&self, request_id: &str) -> Option<(String, Option<String>)> {
        self.conn()
            .query_row(
                "SELECT state, reason FROM requests WHERE request_id = ?1",
                [request_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .unwrap()
    }

    fn task_status(&self, task: &str) -> String {
        self.conn()
            .query_row("SELECT status FROM tasks WHERE task_id = ?1", [task], |r| {
                r.get(0)
            })
            .unwrap()
    }

    fn attempt_state(&self, id: &str) -> String {
        self.conn()
            .query_row("SELECT state FROM attempts WHERE id = ?1", [id], |r| {
                r.get(0)
            })
            .unwrap()
    }

    fn progress_texts(&self, task: &str) -> Vec<String> {
        let conn = self.conn();
        let mut stmt = conn
            .prepare(
                "SELECT text FROM events WHERE task_id = ?1 AND kind = 'progress' ORDER BY rowid",
            )
            .unwrap();
        let texts = stmt
            .query_map([task], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        texts
    }

    fn generation(&self) -> Generation {
        Generation(
            self.conn()
                .query_row("SELECT generation FROM fleetd WHERE id = 1", [], |r| {
                    r.get(0)
                })
                .unwrap(),
        )
    }

    fn cancel(&self, task: &str) -> Output {
        self.h
            .horch(&["task", "cancel", task, "untrusted-files test", "--run", RUN])
            .output()
            .unwrap()
    }

    fn queued_cancel(&self, task: &str) -> PathBuf {
        let before = self.request_files();
        let o = self.cancel(task);
        assert!(o.status.success(), "a queued request exits 0: {}", text(&o));
        let mut new: Vec<PathBuf> = self
            .request_files()
            .into_iter()
            .filter(|p| !before.contains(p))
            .collect();
        assert_eq!(new.len(), 1, "1 request file: {new:?}\n{}", text(&o));
        new.pop().unwrap()
    }

    fn request_files(&self) -> Vec<PathBuf> {
        let Ok(entries) = fs::read_dir(requests_dir(&self.root())) else {
            return Vec::new();
        };
        let mut files: Vec<PathBuf> = entries
            .map(|e| e.unwrap().path())
            .filter(|p| p.is_file())
            .collect();
        files.sort();
        files
    }

    fn fleetd_made_then_down(&mut self) -> (u32, u64) {
        self.start_fleetd();
        self.fleetds.kill_current()
    }
}

impl Drop for Fleet {
    fn drop(&mut self) {
        for agent in &mut self.agents {
            let _ = agent.kill();
            let _ = agent.wait();
        }
    }
}

struct TestIsFleetd<'a> {
    f: &'a Fleet,
    down: (u32, u64),
}

impl<'a> TestIsFleetd<'a> {
    fn new(f: &'a Fleet, down: (u32, u64)) -> TestIsFleetd<'a> {
        let me = std::process::id();
        let started = procid::start_time(me).expect("the start time of the test");
        set_fleetd_row(f, (me, started));
        TestIsFleetd { f, down }
    }
}

impl Drop for TestIsFleetd<'_> {
    fn drop(&mut self) {
        set_fleetd_row(self.f, self.down);
    }
}

fn set_fleetd_row(f: &Fleet, (pid, started): (u32, u64)) {
    f.conn()
        .execute(
            "UPDATE fleetd SET pid = ?1, pid_start = ?2 WHERE id = 1",
            params![i64::from(pid), i64::try_from(started).unwrap()],
        )
        .unwrap();
}

fn text(o: &Output) -> String {
    format!(
        "status: {}\nstdout: {}\nstderr: {}",
        o.status,
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

fn request_id(path: &Path) -> String {
    let name = path.file_name().unwrap().to_string_lossy();
    name.strip_suffix(".json")
        .unwrap_or_else(|| panic!("a request file name ends with .json: {name}"))
        .to_string()
}

fn uid() -> u32 {
    match User::current().unwrap() {
        User::Uid(uid) => uid,
        other => panic!("a unix user: {other:?}"),
    }
}

fn write_mode(path: &Path, bytes: &[u8], mode: u32) {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode)
        .open(path)
        .unwrap();
    file.write_all(bytes).unwrap();
    fs::set_permissions(path, Permissions::from_mode(mode)).unwrap();
}

fn result_json(task: &str, attempt: &str) -> String {
    ResultFile {
        task: task.into(),
        attempt: attempt.into(),
        status: ResultStatus::Completed,
        summary: "untrusted-files test".into(),
        commits: Vec::new(),
        files_changed: Vec::new(),
        acceptance_self_check: Vec::new(),
        notes_for_next: None,
        handoff: None,
        unlanded: None,
    }
    .to_json()
}

fn progress_line(a: &Attempt, text: &str) -> String {
    ProgressLine {
        task: a.task.clone(),
        attempt: "A1".into(),
        at: util::now(),
        kind: LineKind::Progress,
        text: text.into(),
    }
    .to_line(512)
    .unwrap()
}

fn outbox_suffix(a: &Attempt, name: &str) -> String {
    format!("tasks/{}/attempts/A1/outbox/{name}", a.task)
}

#[test]
fn request_file_of_another_owner_is_refused_with_a_reason() {
    let mut f = Fleet::new("p5-owner");
    let a = f.live_attempt();
    f.fleetd_made_then_down();
    let file = f.queued_cancel(&a.task);
    let id = request_id(&file);

    let me = uid();
    let other = User::Uid(me + 1);
    let token = token_file::read(&f.root()).unwrap();
    let spool = Spool {
        fleet_root: &f.root(),
        owner: &other,
        token: Some(&token),
        max_bytes: 64 * 1024,
        guard: f.generation(),
    };
    let start = Instant::now();
    let mut now = move || start;
    let stamp = util::now();
    let mut clock = ScanClock {
        deadline: Deadline::pass(start, 3600),
        now: &mut now,
        stamp: &stamp,
    };
    let mut conn = f.conn();
    let outcome = request_spool::scan(
        &mut conn,
        &spool,
        &mut Cursor::new(&stamp),
        &mut clock,
        &Handlers::new(),
    )
    .unwrap();
    let reason = format!("owner uid {me} is not the ledger owner uid {}", me + 1);
    let name = file.file_name().unwrap().to_string_lossy().into_owned();
    let ScanOutcome::Read(scan) = outcome else {
        panic!("the scan reads the folder: {outcome:?}");
    };
    assert_eq!(
        scan.facts,
        vec![FileFact {
            name: name.clone(),
            handled: Handled::Rejected(reason.clone()),
        }]
    );
    assert_eq!(f.rejected(&format!("run/requests/{name}")), reason);
    assert_eq!(f.request(&id), None, "a refused file has no requests row");

    let pid = f.start_fleetd();
    f.passes(pid, 2);
    assert_eq!(f.request(&id), None, "fleetd read the rejected file again");
    assert!(file.exists(), "a rejected request file stays in place");
    assert_eq!(f.task_status(&a.task), "running");
}

#[test]
fn request_file_with_mode_0644_is_refused_with_a_reason() {
    let mut f = Fleet::new("p5-mode");
    let a = f.live_attempt();
    f.fleetd_made_then_down();
    let file = f.queued_cancel(&a.task);
    let id = request_id(&file);
    fs::set_permissions(&file, Permissions::from_mode(0o644)).unwrap();

    let pid = f.start_fleetd();
    let name = file.file_name().unwrap().to_string_lossy().into_owned();
    assert_eq!(
        f.rejected(&format!("run/requests/{name}")),
        "mode 0644 is wider than 0600"
    );
    f.passes(pid, 2);
    assert_eq!(f.request(&id), None, "a refused file has no requests row");
    assert!(file.exists(), "a rejected request file stays in place");
    assert_eq!(f.task_status(&a.task), "running");
    assert_eq!(f.attempt_state(&a.id), "live");
}

#[test]
fn request_from_another_user_on_the_channel_is_refused() {
    let mut f = Fleet::new("p5-peer");
    let a = f.live_attempt();
    let down = f.fleetd_made_then_down();

    let _fleetd = TestIsFleetd::new(&f, down);
    let server = ChannelServer::start_with(
        &f.root(),
        f.conn(),
        f.generation(),
        Handlers::new(),
        Options {
            peer_user: |_| Ok(User::Uid(OTHER_UID)),
            ..Options::default()
        },
    )
    .unwrap();

    let o = f.cancel(&a.task);
    server.stop().unwrap();

    let reason = format!(
        "the caller uid {OTHER_UID} is not the user of the ledger (uid {})",
        uid()
    );
    assert_eq!(o.status.code(), Some(1), "{}", text(&o));
    let stderr = String::from_utf8_lossy(&o.stderr);
    assert!(
        stderr.contains(&format!("refused: {reason}")),
        "the command prints the reason: {}",
        text(&o)
    );
    let (state, recorded): (String, Option<String>) = f
        .conn()
        .query_row(
            "SELECT state, reason FROM requests WHERE kind = 'task_cancel'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(state, "refused");
    assert_eq!(recorded.as_deref(), Some(reason.as_str()));
    assert!(
        f.request_files().is_empty(),
        "a refused send writes no file"
    );
    assert_eq!(f.task_status(&a.task), "running");
}

#[test]
fn symlinked_result_is_rejected_and_fleetd_stays_healthy() {
    let mut f = Fleet::new("p5-link");
    let a = f.live_attempt();
    let target = f.h.root.join("valid-result.json");
    fs::write(&target, result_json(&a.task, "A1")).unwrap();
    std::os::unix::fs::symlink(&target, a.outbox(&f).join("result.json")).unwrap();

    let pid = f.start_fleetd();
    let reason = f.rejected(&outbox_suffix(&a, "result.json"));
    assert!(reason.contains("the file is a link"), "{reason}");
    f.passes(pid, 2);
    assert_eq!(f.task_status(&a.task), "running");
    assert_eq!(f.attempt_state(&a.id), "live");
}

#[test]
fn progress_over_its_limit_is_rejected_and_fleetd_stays_healthy() {
    let mut f = Fleet::new("p5-size");
    let a = f.live_attempt();
    let line = progress_line(&a, "a valid line of a log over its limit");
    let lines = PROGRESS_MAX_BYTES / line.len() + 2;
    let log = line.repeat(lines);
    assert!(log.len() > PROGRESS_MAX_BYTES);
    write_mode(&a.outbox(&f).join("progress.jsonl"), log.as_bytes(), 0o600);

    let pid = f.start_fleetd();
    let reason = f.rejected(&outbox_suffix(&a, "progress.jsonl"));
    assert!(
        reason.contains(&format!("over the limit of {PROGRESS_MAX_BYTES} bytes")),
        "{reason}"
    );
    f.passes(pid, 2);
    assert!(
        f.progress_texts(&a.task).is_empty(),
        "no line of a rejected log is ingested"
    );
    assert_eq!(f.task_status(&a.task), "running");
    assert_eq!(f.attempt_state(&a.id), "live");
}

#[test]
fn result_with_the_attempt_id_of_another_attempt_is_rejected() {
    let mut f = Fleet::new("p5-attempt");
    let a = f.live_attempt();
    write_mode(
        &a.outbox(&f).join("result.json"),
        result_json(&a.task, "A2").as_bytes(),
        0o600,
    );

    let pid = f.start_fleetd();
    let reason = f.rejected(&outbox_suffix(&a, "result.json"));
    assert!(reason.starts_with("attempt_id:"), "{reason}");
    f.passes(pid, 2);
    assert_eq!(f.task_status(&a.task), "running");
    assert_eq!(f.attempt_state(&a.id), "live");
}

#[test]
fn half_written_files_are_never_read() {
    let mut f = Fleet::new("p5-half");
    let a = f.live_attempt();
    let b = f.live_attempt();
    f.fleetd_made_then_down();
    let request = f.queued_cancel(&b.task);
    let id = request_id(&request);
    let request_tmp = request.with_extension("json.tmp");
    fs::rename(&request, &request_tmp).unwrap();
    let result = a.outbox(&f).join("result.json");
    let result_tmp = a.outbox(&f).join("result.json.tmp");
    write_mode(&result_tmp, result_json(&a.task, "A1").as_bytes(), 0o600);
    let first = progress_line(&a, "the first line");
    let second = progress_line(&a, "the second line");
    let (head, tail) = second.split_at(second.len() / 2);
    let log = a.outbox(&f).join("progress.jsonl");
    write_mode(&log, format!("{first}{head}").as_bytes(), 0o600);

    let pid = f.start_fleetd();
    wait_for("the first progress line", || {
        (!f.progress_texts(&a.task).is_empty()).then_some(())
    });
    f.passes(pid, 2);
    assert_eq!(
        f.progress_texts(&a.task),
        vec!["the first line".to_string()]
    );
    assert_eq!(f.request(&id), None, "a *.tmp request file is not read");
    assert_eq!(f.ingested(&format!("run/requests/{id}.json.tmp")), None);
    assert_eq!(f.ingested(&outbox_suffix(&a, "result.json.tmp")), None);
    assert_eq!(f.ingested(&outbox_suffix(&a, "result.json")), None);
    assert_eq!(f.task_status(&a.task), "running");
    assert_eq!(f.task_status(&b.task), "running");

    let mut append = OpenOptions::new().append(true).open(&log).unwrap();
    append.write_all(tail.as_bytes()).unwrap();
    drop(append);
    fs::rename(&result_tmp, &result).unwrap();
    fs::rename(&request_tmp, &request).unwrap();
    wait_for("the second progress line", || {
        (f.progress_texts(&a.task).len() == 2).then_some(())
    });
    assert_eq!(
        f.progress_texts(&a.task),
        vec!["the first line".to_string(), "the second line".to_string()]
    );
    let (outcome, reason, _) = wait_for("the ingested row of result.json", || {
        f.ingested(&outbox_suffix(&a, "result.json"))
    });
    assert_eq!((outcome.as_str(), reason), ("accepted", None));
    wait_for("the requests row of the request file", || f.request(&id));
}

#[test]
fn rejected_path_is_not_read_again() {
    let mut f = Fleet::new("p5-again");
    let a = f.live_attempt();
    let result = a.outbox(&f).join("result.json");
    let target = f.h.root.join("valid-result.json");
    fs::write(&target, result_json(&a.task, "A1")).unwrap();
    std::os::unix::fs::symlink(&target, &result).unwrap();

    let pid = f.start_fleetd();
    let suffix = outbox_suffix(&a, "result.json");
    let reason = f.rejected(&suffix);
    let (_, _, at) = f.ingested(&suffix).unwrap();

    fs::remove_file(&result).unwrap();
    write_mode(&result, result_json(&a.task, "A1").as_bytes(), 0o600);
    f.passes(pid, 2);
    f.fleetds.kill_current();
    let pid = f.start_fleetd();
    f.passes(pid, 2);
    assert_eq!(
        f.ingested(&suffix),
        Some(("rejected".to_string(), Some(reason), at)),
        "the rejected row is not written again"
    );
    assert_eq!(f.task_status(&a.task), "running");
    assert_eq!(f.attempt_state(&a.id), "live");
}
