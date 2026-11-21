//! The outbox writer of a worker command (ticket 26 of fleetd-p5): the
//! command finds its own attempt by its parent chain, makes the outbox when
//! it is missing, and writes each file atomically.
//!
//! `docs/specs/fleetd/design.md`, "Workspace layout and file protocol",
//! protocol rules 1, 3 and 7; FR-7, RR-5, RR-8, SR-6;
//! `docs/tickets/fleetd-p5/MAP.md`, Q3 and Q4.
//!
//! Each test uses a temp ledger and a temp fleet root. A "wrapper" is a real
//! process: an `sh` that the test starts, or the parent of this test
//! process. A worker command is a real child process: this test binary,
//! started again to run [`child_entry`].
#![cfg(unix)]

use std::io::Write as _;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use horch_core::fleetd::fleet_runtime::adapters::peer_process::PlatformFacts;
use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;
use horch_core::fleetd::task_execution::adapters::outbox_writer::{OutboxWriter, NO_LIVE_ATTEMPT};
use horch_core::fleetd::task_execution::domain::outbox::{
    handoff_result, LineKind, PROGRESS_JSONL, QUESTIONS, RESULT_JSON,
};
use horch_core::fleetd::task_execution::domain::result::{validate, ResultFile, ResultStatus};
use horch_core::taskledger::schema;
use horch_platform::process;
use rusqlite::{params, Connection, OpenFlags};

const AT: &str = "2026-11-18T12:00:00Z";

/// The environment of a child command: set only by these tests.
const CHILD: &str = "OUTBOX_WRITER_TEST_CHILD";
const CHILD_ROOT: &str = "OUTBOX_WRITER_TEST_ROOT";
const CHILD_LEDGER: &str = "OUTBOX_WRITER_TEST_LEDGER";
const CHILD_ACTION: &str = "OUTBOX_WRITER_TEST_ACTION";

/// A temp ledger with the schema of today and a fleet root.
struct World {
    _temp: tempfile::TempDir,
    root: PathBuf,
    ledger: PathBuf,
}

fn world() -> World {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("fleet");
    std::fs::create_dir_all(&root).unwrap();
    let ledger = temp.path().join("ledger.sqlite3");
    let mut conn = Connection::open(&ledger).unwrap();
    schema::migrate_to(&mut conn, schema::VERSION).unwrap();
    World {
        _temp: temp,
        root,
        ledger,
    }
}

impl World {
    /// Record the live attempt `<task>/A1` whose wrapper is `pid`, and
    /// make its attempt folder, as fleetd does. Its outbox is not made.
    fn live(&self, task: &str, pid: u32) -> String {
        let id = format!("{task}/A1");
        let start = process::start_time(pid).expect("the start time of the wrapper");
        let conn = Connection::open(&self.ledger).unwrap();
        conn.execute_batch("PRAGMA foreign_keys = OFF").unwrap();
        conn.execute(
            "INSERT INTO attempts (id, task_id, worker_id, pid, pid_start, state, started_at)
             VALUES (?1, ?2, 'w-1', ?3, ?4, 'live', ?5)",
            params![id, task, i64::from(pid), i64::try_from(start).unwrap(), AT],
        )
        .unwrap();
        std::fs::create_dir_all(self.attempt_dir(task)).unwrap();
        id
    }

    fn attempt_dir(&self, task: &str) -> PathBuf {
        self.root
            .join("tasks")
            .join(task)
            .join("attempts")
            .join("A1")
    }

    fn outbox(&self, task: &str) -> PathBuf {
        self.attempt_dir(task).join("outbox")
    }

    /// The ledger, read-only, as a worker command opens it.
    fn read_only(&self) -> Connection {
        Connection::open_with_flags(&self.ledger, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap()
    }

    /// The writer of this test process, with `config`.
    fn writer(&self, config: &FleetConfig) -> anyhow::Result<OutboxWriter> {
        OutboxWriter::of_this_process(&self.root, &self.read_only(), config, &PlatformFacts)
    }

    /// A child command that runs `action`, with its stdin held until
    /// [`go`].
    fn child(&self, action: &str) -> Command {
        let mut cmd = Command::new(std::env::current_exe().unwrap());
        cmd.args([
            "--exact",
            "child_entry",
            "--nocapture",
            "--test-threads",
            "1",
        ])
        .env(CHILD, "1")
        .env(CHILD_ROOT, &self.root)
        .env(CHILD_LEDGER, &self.ledger)
        .env(CHILD_ACTION, action)
        .stdout(Stdio::null())
        .stdin(Stdio::piped());
        cmd
    }
}

/// The parent of this test process: a real process in the chain of this
/// process and of each child that it starts.
fn parent_of_this_process() -> u32 {
    process::parent(std::process::id()).unwrap().unwrap()
}

/// A wrapper `sh` that waits for 1 line on its stdin, then runs `cmd` as
/// its child and exits with its code.
fn wrapper_of(cmd: &Command) -> Child {
    let mut sh = Command::new("sh");
    sh.args(["-c", r#"read go; "$@"; exit $?"#, "sh"])
        .arg(cmd.get_program())
        .args(cmd.get_args())
        .stdin(Stdio::piped())
        .stdout(Stdio::null());
    for (key, value) in cmd.get_envs() {
        match value {
            Some(value) => sh.env(key, value),
            None => sh.env_remove(key),
        };
    }
    // The wrapper does not give its stdin to the child.
    sh.spawn().unwrap()
}

/// A live process that is not in the chain of this test process.
fn stranger() -> Child {
    Command::new("sleep").arg("30").spawn().unwrap()
}

/// Send the line that starts each child, then wait for the success of each.
fn go_and_wait_all(mut children: Vec<Child>) {
    for child in &mut children {
        child.stdin.take().unwrap().write_all(b"go\n").unwrap();
    }
    for mut child in children {
        let status = child.wait().unwrap();
        assert!(status.success(), "the child command failed: {status}");
    }
}

fn go_and_wait(child: Child) {
    go_and_wait_all(vec![child]);
}

fn mode(path: &Path) -> u32 {
    std::fs::symlink_metadata(path)
        .unwrap()
        .permissions()
        .mode()
        & 0o777
}

/// The JSON values of the lines of `path`.
fn lines(path: &Path) -> Vec<serde_json::Value> {
    let text = std::fs::read_to_string(path).unwrap();
    assert!(text.ends_with('\n'), "the log ends with a whole line");
    text.lines()
        .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("line {l:?}: {e}")))
        .collect()
}

fn result_of(writer: &OutboxWriter, status: ResultStatus) -> ResultFile {
    ResultFile {
        task: writer.attempt().task_id.clone(),
        attempt: writer.attempt().attempt().to_string(),
        status,
        summary: "done".into(),
        commits: vec!["3f9c2ab".into()],
        files_changed: vec!["src/a.rs".into()],
        acceptance_self_check: Vec::new(),
        notes_for_next: None,
        handoff: None,
        unlanded: None,
    }
}

/// The body of a child command. It does nothing in a plain test run.
#[test]
fn child_entry() {
    if std::env::var_os(CHILD).is_none() {
        return;
    }
    let mut go = String::new();
    std::io::stdin().read_line(&mut go).unwrap();
    let root = PathBuf::from(std::env::var_os(CHILD_ROOT).unwrap());
    let ledger = PathBuf::from(std::env::var_os(CHILD_LEDGER).unwrap());
    let action = std::env::var(CHILD_ACTION).unwrap();
    let conn = Connection::open_with_flags(&ledger, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let writer =
        OutboxWriter::of_this_process(&root, &conn, &FleetConfig::default(), &PlatformFacts)
            .unwrap();
    let parts: Vec<&str> = action.splitn(3, ':').collect();
    match parts.as_slice() {
        ["whoami", out] => std::fs::write(out, &writer.attempt().attempt_id).unwrap(),
        ["progress", count, fill] => {
            let text = fill.repeat(3000);
            for _ in 0..count.parse::<usize>().unwrap() {
                writer.append(LineKind::Progress, AT, &text).unwrap();
            }
        }
        _ => panic!("unknown action {action:?}"),
    }
}

#[test]
fn command_under_a_wrapper_finds_its_own_attempt() {
    let w = world();
    let other = stranger();
    w.live("T-2", other.id());
    let out = w._temp.path().join("whoami");
    let wrapper = wrapper_of(&w.child(&format!("whoami:{}", out.display())));
    let own = w.live("T-1", wrapper.id());
    go_and_wait(wrapper);
    let mut other = other;
    let _ = other.kill();
    let _ = other.wait();
    assert_eq!(std::fs::read_to_string(&out).unwrap(), own);
}

#[test]
fn worker_that_sets_the_task_or_attempt_of_another_attempt_in_its_environment_writes_only_its_own_outbox(
) {
    let w = world();
    let mut other = stranger();
    w.live("T-2", other.id());
    let mut cmd = w.child("progress:1:a");
    let other_task_yaml = w.attempt_dir("T-2").join("task.yaml");
    cmd.env("HORCH_TASK_YAML", &other_task_yaml)
        .env("HORCH_TASK", "T-2")
        .env("HORCH_TASK_ID", "T-2")
        .env("HORCH_ATTEMPT", "T-2/A1")
        .env("HORCH_ATTEMPT_ID", "T-2/A1")
        .env("HORCH_ROLE", "orchestrator");
    let wrapper = wrapper_of(&cmd);
    w.live("T-1", wrapper.id());
    go_and_wait(wrapper);
    let _ = other.kill();
    let _ = other.wait();
    let own = lines(&w.outbox("T-1").join(PROGRESS_JSONL));
    assert_eq!(own.len(), 1);
    assert_eq!(own[0]["task"], "T-1");
    assert_eq!(own[0]["attempt"], "A1");
    assert!(
        !w.outbox("T-2").exists(),
        "the outbox of the other attempt is not made"
    );
}

#[test]
fn process_outside_each_attempt_is_refused() {
    let w = world();
    let err = w.writer(&FleetConfig::default()).expect_err("refused");
    assert!(err.to_string().contains(NO_LIVE_ATTEMPT), "{err:#}");

    let mut other = stranger();
    w.live("T-2", other.id());
    let err = w.writer(&FleetConfig::default()).expect_err("refused");
    let _ = other.kill();
    let _ = other.wait();
    assert!(err.to_string().contains(NO_LIVE_ATTEMPT), "{err:#}");
    assert!(!w.outbox("T-2").exists());
}

#[test]
fn missing_outbox_is_made_with_mode_0700() {
    let w = world();
    w.live("T-1", parent_of_this_process());
    let outbox = w.outbox("T-1");
    assert!(!outbox.exists());
    let writer = w.writer(&FleetConfig::default()).unwrap();
    writer.append(LineKind::Note, AT, "a note").unwrap();
    assert!(std::fs::symlink_metadata(&outbox).unwrap().is_dir());
    assert_eq!(mode(&outbox), 0o700);
    let log = outbox.join(PROGRESS_JSONL);
    assert_eq!(mode(&log), 0o600);
    let got = lines(&log);
    assert_eq!(got.len(), 1);
    assert_eq!(
        got[0],
        serde_json::json!({
            "task": "T-1", "attempt": "A1", "at": AT, "kind": "note", "text": "a note"
        })
    );
}

#[test]
fn result_is_written_atomically_with_mode_0600() {
    let w = world();
    w.live("T-1", parent_of_this_process());
    let config = FleetConfig::default();
    let writer = w.writer(&config).unwrap();
    let file = result_of(&writer, ResultStatus::Completed);
    let path = writer.write_result(&file).unwrap();
    let outbox = w.outbox("T-1");
    assert_eq!(path, outbox.join(RESULT_JSON));
    assert_eq!(mode(&outbox), 0o700);
    assert_eq!(mode(&path), 0o600);
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(bytes, file.to_json().into_bytes());
    assert_eq!(
        validate(&bytes, "T-1", "A1", config.result_max_bytes).unwrap(),
        file
    );
    let names: Vec<String> = std::fs::read_dir(&outbox)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, [RESULT_JSON], "no temp file stays");

    // An immutable message: a second result is refused, the first stays.
    let second = result_of(&writer, ResultStatus::Failed);
    assert!(writer.write_result(&second).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), bytes);

    // A result that names another attempt is refused before the write.
    let other = w.outbox("T-1").join("..").join("..").join("A2");
    let mut foreign = file.clone();
    foreign.attempt = "A2".into();
    assert!(writer.write_result(&foreign).is_err());
    assert!(!other.exists());
}

#[test]
fn progress_lines_from_2_processes_never_interleave() {
    const EACH: usize = 200;
    let w = world();
    w.live("T-1", parent_of_this_process());
    let a = w.child(&format!("progress:{EACH}:a")).spawn().unwrap();
    let b = w.child(&format!("progress:{EACH}:b")).spawn().unwrap();
    go_and_wait_all(vec![a, b]);
    let got = lines(&w.outbox("T-1").join(PROGRESS_JSONL));
    assert_eq!(got.len(), 2 * EACH);
    let mut count = [0usize; 2];
    for line in &got {
        let text = line["text"].as_str().unwrap();
        assert_eq!(text.len(), 3000);
        match &text[..1] {
            "a" => count[0] += 1,
            "b" => count[1] += 1,
            other => panic!("unexpected text {other:?}"),
        }
        assert!(
            text.bytes().all(|c| c == text.as_bytes()[0]),
            "1 line, 1 writer"
        );
    }
    assert_eq!(count, [EACH, EACH]);
}

#[test]
fn line_over_the_limit_is_refused_before_the_write() {
    let w = world();
    w.live("T-1", parent_of_this_process());
    let config = FleetConfig {
        progress_line_max_bytes: 256,
        ..FleetConfig::default()
    };
    let writer = w.writer(&config).unwrap();
    let log = w.outbox("T-1").join(PROGRESS_JSONL);
    let err = writer
        .append(LineKind::Progress, AT, &"x".repeat(300))
        .unwrap_err();
    assert!(err.to_string().contains("256"), "{err:#}");
    assert!(!log.exists(), "nothing is written");

    writer.append(LineKind::Progress, AT, "short").unwrap();
    let before = std::fs::read(&log).unwrap();
    assert!(writer
        .append(LineKind::Progress, AT, &"x".repeat(300))
        .is_err());
    assert_eq!(std::fs::read(&log).unwrap(), before, "the log is unchanged");
}

#[test]
fn handoff_result_has_status_blocked_and_reason_handoff() {
    let w = world();
    w.live("T-1", parent_of_this_process());
    let config = FleetConfig::default();
    let writer = w.writer(&config).unwrap();
    let handoff = w.attempt_dir("T-1").join("handoff.md");
    let file = handoff_result(
        &writer.attempt().task_id,
        writer.attempt().attempt(),
        "context limit",
        &handoff.to_string_lossy(),
    );
    let path = writer.write_result(&file).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(json["status"], "blocked");
    assert_eq!(json["reason"], "handoff");
    assert_eq!(json["handoff"], handoff.to_string_lossy().as_ref());
    let read = validate(&bytes, "T-1", "A1", config.result_max_bytes).unwrap();
    assert_eq!(read.status, ResultStatus::Blocked);
    assert_eq!(read.handoff.as_deref(), Some(&*handoff.to_string_lossy()));
}

#[test]
fn question_files_are_numbered_from_1_with_mode_0600() {
    let w = world();
    w.live("T-1", parent_of_this_process());
    let writer = w.writer(&FleetConfig::default()).unwrap();
    let first = writer.write_question(AT, "which file?", None).unwrap();
    let second = writer.write_question(AT, "which test?", None).unwrap();
    let dir = w.outbox("T-1").join(QUESTIONS);
    assert_eq!(first, dir.join("1.json"));
    assert_eq!(second, dir.join("2.json"));
    assert_eq!(mode(&dir), 0o700);
    assert_eq!(mode(&first), 0o600);
    let json: serde_json::Value = serde_json::from_slice(&std::fs::read(&second).unwrap()).unwrap();
    assert_eq!(
        json,
        serde_json::json!({"task": "T-1", "attempt": "A1", "at": AT, "text": "which test?"})
    );
}
