use std::path::PathBuf;

use horch_core::fleetd::fleet_runtime::adapters::peer_process::ProcessFacts;
use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;
use horch_core::fleetd::task_execution::adapters::outbox_writer::OutboxWriter;
use horch_core::fleetd::task_execution::domain::outbox::{PROGRESS_JSONL, RESULT_JSON};
use horch_core::fleetd::task_execution::domain::result::validate;
use horch_core::messaging::report::{self, Request};
use horch_core::taskledger::schema;
use rusqlite::{params, Connection, OpenFlags};

const AT: &str = "2026-11-18T12:00:00Z";

const START: u64 = 7;

struct Facts;

impl ProcessFacts for Facts {
    fn parent(&self, _pid: u32) -> Option<u32> {
        Some(0)
    }

    fn start_time(&self, pid: u32) -> Option<u64> {
        (pid == std::process::id()).then_some(START)
    }

    fn in_group(&self, _pid: u32, _leader: u32, _leader_start: u64) -> Option<bool> {
        None
    }
}

struct World {
    temp: tempfile::TempDir,
    root: PathBuf,
    ledger: PathBuf,
}

fn world() -> World {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("fleet");
    std::fs::create_dir_all(root.join("tasks/T-1/attempts/A1")).unwrap();
    let ledger = temp.path().join("ledger.sqlite3");
    let mut conn = Connection::open(&ledger).unwrap();
    schema::migrate_to(&mut conn, schema::VERSION).unwrap();
    conn.execute_batch("PRAGMA foreign_keys = OFF").unwrap();
    conn.execute(
        "INSERT INTO attempts (id, task_id, worker_id, pid, pid_start, state, started_at)
         VALUES ('T-1/A1', 'T-1', 'w-1', ?1, ?2, 'live', ?3)",
        params![
            i64::from(std::process::id()),
            i64::try_from(START).unwrap(),
            AT
        ],
    )
    .unwrap();
    World { temp, root, ledger }
}

impl World {
    fn writer(&self) -> anyhow::Result<OutboxWriter> {
        let conn = Connection::open_with_flags(&self.ledger, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        OutboxWriter::of_this_process(&self.root, &conn, &FleetConfig::default(), &Facts)
    }

    fn outbox(&self) -> PathBuf {
        self.root.join("tasks/T-1/attempts/A1/outbox")
    }
}

fn no_outbox() -> anyhow::Result<OutboxWriter> {
    panic!("a tell of a report pane writes no outbox file")
}

#[test]
fn pres_06_s11_worker_report_is_a_file_on_every_os() {
    let w = world();
    let socket = w.temp.path().join("report.sock");
    report::report(
        &socket,
        &Request::Progress {
            text: "step 1".into(),
        },
        || w.writer(),
        AT,
    )
    .unwrap();
    report::report(
        &socket,
        &Request::Done {
            text: "all done".into(),
        },
        || w.writer(),
        AT,
    )
    .unwrap();
    assert!(!socket.exists());

    let progress = std::fs::read_to_string(w.outbox().join(PROGRESS_JSONL)).unwrap();
    assert_eq!(
        progress,
        format!(
            "{{\"task\": \"T-1\", \"attempt\": \"A1\", \"at\": \"{AT}\", \
             \"kind\": \"progress\", \"text\": \"step 1\"}}\n"
        )
    );
    let result = std::fs::read(w.outbox().join(RESULT_JSON)).unwrap();
    let file = validate(
        &result,
        "T-1",
        "A1",
        FleetConfig::default().result_max_bytes,
    )
    .unwrap();
    assert_eq!(file.status.as_str(), "completed");
    assert_eq!(file.summary, "all done");

    let err = report::report(
        &socket,
        &Request::Progress { text: " ".into() },
        || w.writer(),
        AT,
    )
    .unwrap_err();
    assert_eq!(err.to_string(), "the text is empty");
    let lines = std::fs::read_to_string(w.outbox().join(PROGRESS_JSONL)).unwrap();
    assert_eq!(lines.lines().count(), 1);
}

#[cfg(unix)]
use std::sync::{Arc, Mutex};

#[cfg(unix)]
fn server(path: &std::path::Path) -> (Arc<Mutex<Vec<Request>>>, report::Server) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    let exec: Box<report::Executor> = Box::new(move |r: &Request| {
        log.lock().unwrap().push(r.clone());
        Ok(())
    });
    let server =
        report::Server::start(path.to_path_buf(), Some("orchestrator".into()), exec).unwrap();
    (seen, server)
}

#[cfg(unix)]
fn socket_dir() -> tempfile::TempDir {
    tempfile::Builder::new().tempdir_in("/tmp").unwrap()
}

#[cfg(unix)]
#[test]
fn report_socket_refuses_progress_and_done() {
    let tmp = socket_dir();
    let path = report::socket_path(tmp.path(), "r1");
    let (seen, _server) = server(&path);
    for request in [
        Request::Progress { text: "x".into() },
        Request::Done { text: "x".into() },
    ] {
        let err = report::send(&path, &request).unwrap_err();
        assert_eq!(
            err.to_string(),
            "progress and done write the outbox of the attempt; this socket takes only tell"
        );
    }
    assert!(seen.lock().unwrap().is_empty());
}

#[cfg(unix)]
#[test]
fn report_socket_keeps_tell() {
    let tmp = socket_dir();
    let path = report::socket_path(tmp.path(), "r1");
    let (seen, _server) = server(&path);
    let tell = Request::Tell {
        to: "orchestrator".into(),
        text: "[r] NOTE: hi".into(),
    };
    report::report(&path, &tell, no_outbox, AT).unwrap();
    assert_eq!(*seen.lock().unwrap(), vec![tell]);
    let err = report::report(
        &path,
        &Request::Tell {
            to: "sonnet-1".into(),
            text: "x".into(),
        },
        no_outbox,
        AT,
    )
    .unwrap_err();
    assert!(err.to_string().contains("only to orchestrator"), "{err}");
    assert_eq!(seen.lock().unwrap().len(), 1);
}
