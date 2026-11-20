//! The channel server of fleetd: bind, the peer user check, 1 request line
//! with a deadline, the decision and the reply
//! (docs/tickets/fleetd-p5/13-channel-server.md; SR-5, SR-8, RR-8, FM-15).
//!
//! Each test uses a temp fleet root with a ledger at generation 1, a test
//! `fleet.toml` and the operator token of that root.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use horch_core::fleetd::fleet_runtime::adapters::channel_server::{
    socket_path, ChannelServer, Options,
};
use horch_core::fleetd::fleet_runtime::adapters::request_wire::{
    parse_reply, Outcome, Reply, Request,
};
use horch_core::fleetd::fleet_runtime::adapters::token_file;
use horch_core::fleetd::fleet_runtime::application::requests::Handlers;
use horch_core::fleetd::fleet_runtime::domain::ledger::Generation;
use horch_core::fleetd::fleet_runtime::domain::requests::{Kind, RequestId};
use horch_core::taskledger::{schema, util};
use horch_platform::channel::{Stream, User};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Map, Value};

/// The `request_max_bytes` of the test `fleet.toml`.
const MAX_BYTES: usize = 512;

/// The env of the child that sends 1 line as a process of an attempt.
const CHILD_SOCKET: &str = "HORCH_TEST_CHANNEL_SOCKET";
const CHILD_LINE: &str = "HORCH_TEST_CHANNEL_LINE";

/// A temp fleet root with its ledger at generation 1 and its token.
struct World {
    temp: tempfile::TempDir,
    calls: Arc<AtomicUsize>,
}

impl World {
    fn new() -> World {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        std::fs::write(
            root.join("fleet.toml"),
            format!("request_max_bytes = {MAX_BYTES}\n"),
        )
        .unwrap();
        let mut conn = Connection::open(ledger_path(root)).unwrap();
        schema::migrate_to(&mut conn, schema::VERSION).unwrap();
        conn.execute(
            "INSERT INTO fleetd (id, generation, pid, pid_start, started_at, fleet_clock_s, clock_ticked_at)
             VALUES (1, 1, ?1, 0, ?2, 0, ?2)",
            params![std::process::id(), util::now()],
        )
        .unwrap();
        token_file::ensure(root).unwrap();
        World {
            temp,
            calls: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn root(&self) -> &Path {
        self.temp.path()
    }

    fn conn(&self) -> Connection {
        let conn = Connection::open(ledger_path(self.root())).unwrap();
        conn.busy_timeout(Duration::from_secs(10)).unwrap();
        conn
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

    fn start(&self, options: Options) -> anyhow::Result<ChannelServer> {
        ChannelServer::start_with(
            self.root(),
            self.conn(),
            Generation(1),
            self.handlers(),
            options,
        )
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }

    fn token(&self) -> String {
        token_file::read(self.root()).unwrap().text().to_string()
    }

    fn socket(&self) -> PathBuf {
        socket_path(self.root())
    }

    /// The state, class, auth and reason of the row of `id`.
    fn row(&self, id: &RequestId) -> Option<(String, String, Option<String>, Option<String>)> {
        self.conn()
            .query_row(
                "SELECT state, class, auth, reason FROM requests WHERE request_id = ?1",
                [id.as_str()],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()
            .unwrap()
    }
}

fn ledger_path(root: &Path) -> PathBuf {
    root.join("ledger.sqlite3")
}

fn new_id() -> RequestId {
    RequestId::parse(&uuid::Uuid::new_v4().to_string()).unwrap()
}

fn request(kind: Kind, token: Option<String>) -> Request {
    let Value::Object(payload) = json!({ "task_id": "T-1" }) else {
        unreachable!()
    };
    Request {
        request_id: new_id(),
        kind,
        payload,
        token,
    }
}

fn connect(socket: &Path) -> Stream {
    Stream::connect(socket, Instant::now() + Duration::from_secs(10)).unwrap()
}

/// Send `line` on a new connection, and read until the server closes it.
fn ask(socket: &Path, line: &[u8]) -> Vec<u8> {
    let stream = connect(socket);
    (&stream).write_all(line).unwrap();
    read_to_close(&stream)
}

fn read_to_close(stream: &Stream) -> Vec<u8> {
    let mut reply = Vec::new();
    let mut reader = stream;
    reader.read_to_end(&mut reply).unwrap();
    reply
}

fn reply(bytes: &[u8]) -> Reply {
    parse_reply(bytes.strip_suffix(b"\n").unwrap_or(bytes), 1 << 20).unwrap()
}

fn refused_reason(bytes: &[u8]) -> String {
    let wire: Map<String, Value> = serde_json::from_slice(bytes).unwrap();
    assert_eq!(wire["state"], "refused", "{wire:?}");
    wire["reason"].as_str().unwrap().to_string()
}

#[test]
fn request_from_this_user_is_applied_and_replied() {
    let w = World::new();
    let server = w.start(Options::default()).unwrap();
    let req = request(Kind::TaskCancel, Some(w.token()));

    let got = reply(&ask(&w.socket(), req.to_line().as_bytes()));

    assert_eq!(got.request_id, req.request_id);
    assert_eq!(got.outcome, Outcome::Applied(json!({ "calls": 1 })));
    assert_eq!(w.calls(), 1);
    let (state, class, auth, _) = w.row(&req.request_id).unwrap();
    assert_eq!(state, "applied");
    assert_eq!(class, "operator");
    assert_eq!(
        auth.as_deref(),
        Some("token; peer outside each live attempt")
    );
    // The same request_id again gets the stored result, with no second call.
    let again = reply(&ask(&w.socket(), req.to_line().as_bytes()));
    assert_eq!(again.outcome, Outcome::Applied(json!({ "calls": 1 })));
    assert_eq!(w.calls(), 1);
    server.stop().unwrap();
}

#[test]
fn peer_user_that_is_not_the_owner_is_refused_with_its_reason() {
    let w = World::new();
    let options = Options {
        peer_user: |_| Ok(User::Sid("S-1-5-21-0-0-0-4242".into())),
        ..Options::default()
    };
    let server = w.start(options).unwrap();
    let req = request(Kind::TaskCancel, Some(w.token()));

    let got = reply(&ask(&w.socket(), req.to_line().as_bytes()));

    let Outcome::Refused(reason) = got.outcome else {
        panic!("a peer of another user is refused: {got:?}");
    };
    assert!(
        reason.contains("S-1-5-21-0-0-0-4242") && reason.contains("is not the user of the ledger"),
        "{reason}"
    );
    assert_eq!(w.calls(), 0);
    let (state, _, auth, recorded) = w.row(&req.request_id).unwrap();
    assert_eq!((state.as_str(), auth), ("refused", None));
    assert_eq!(recorded.as_deref(), Some(reason.as_str()));
    server.stop().unwrap();
}

#[test]
fn line_over_the_limit_is_refused() {
    let w = World::new();
    let server = w.start(Options::default()).unwrap();
    let mut req = request(Kind::TaskCancel, Some(w.token()));
    req.payload
        .insert("pad".into(), Value::String("x".repeat(MAX_BYTES)));
    let line = req.to_line();
    assert!(line.len() > MAX_BYTES + 1);

    let got = ask(&w.socket(), line.as_bytes());

    let reason = refused_reason(&got);
    assert!(
        reason.contains(&format!("over the limit of {MAX_BYTES} bytes")),
        "{reason}"
    );
    assert_eq!(w.calls(), 0);
    assert_eq!(
        w.row(&req.request_id),
        None,
        "a line over the limit is not parsed"
    );
    server.stop().unwrap();
}

#[test]
fn silent_client_is_dropped_at_its_deadline_and_the_next_is_served() {
    let w = World::new();
    let options = Options {
        read_deadline: Duration::from_millis(300),
        ..Options::default()
    };
    let server = w.start(options).unwrap();
    // This client connects and sends nothing.
    let silent = connect(&w.socket());
    let req = request(Kind::TaskCancel, Some(w.token()));

    let got = reply(&ask(&w.socket(), req.to_line().as_bytes()));

    assert_eq!(got.outcome, Outcome::Applied(json!({ "calls": 1 })));
    // The server closed the silent connection with no reply.
    assert_eq!(read_to_close(&silent), b"");
    server.stop().unwrap();
}

/// A reply far larger than the buffer of the channel: 4096 bytes on a
/// Windows pipe, a few KiB on a Unix socket.
const LARGE_REPLY: usize = 128 * 1024;

#[test]
fn non_reading_client_with_large_reply_does_not_block_next_client_or_server_stop() {
    let w = World::new();
    let mut handlers = Handlers::new();
    handlers.insert(Kind::TaskCancel, |_, _| {
        Ok(json!({ "pad": "x".repeat(LARGE_REPLY) }))
    });
    let options = Options {
        read_deadline: Duration::from_millis(300),
        ..Options::default()
    };
    let server =
        ChannelServer::start_with(w.root(), w.conn(), Generation(1), handlers, options).unwrap();

    let silent = connect(&w.socket());
    let first = request(Kind::TaskCancel, Some(w.token()));
    (&silent).write_all(first.to_line().as_bytes()).unwrap();

    let next = request(Kind::TaskCancel, Some(w.token()));
    let got = reply(&ask(&w.socket(), next.to_line().as_bytes()));
    assert_eq!(got.request_id, next.request_id);
    assert_eq!(
        got.outcome,
        Outcome::Applied(json!({ "pad": "x".repeat(LARGE_REPLY) }))
    );

    let stopping = Instant::now();
    server.stop().unwrap();
    assert!(
        stopping.elapsed() < Duration::from_secs(2),
        "the stop ends in time: {:?}",
        stopping.elapsed()
    );
    drop(silent);
}

#[test]
fn worker_kind_on_the_channel_is_refused_and_recorded() {
    let w = World::new();
    let server = w.start(Options::default()).unwrap();
    let req = request(Kind::Progress, Some(w.token()));

    let got = reply(&ask(&w.socket(), req.to_line().as_bytes()));

    let Outcome::Refused(reason) = got.outcome else {
        panic!("a worker kind is refused: {got:?}");
    };
    assert!(reason.contains("worker kind"), "{reason}");
    let (state, class, auth, recorded) = w.row(&req.request_id).unwrap();
    assert_eq!(
        (state.as_str(), class.as_str(), auth),
        ("refused", "worker", None)
    );
    assert_eq!(recorded.as_deref(), Some(reason.as_str()));
    server.stop().unwrap();
}

/// A child of a recorded wrapper sends the request. The wrapper `sh` waits
/// for 1 line on its stdin, so the test records its attempt first. Its
/// child is this test binary, which runs [`channel_client_child`].
#[cfg(unix)]
#[test]
fn operator_request_from_a_process_of_a_live_attempt_is_refused() {
    let w = World::new();
    let server = w.start(Options::default()).unwrap();
    let req = request(Kind::TaskCancel, Some(w.token()));
    let exe = std::env::current_exe().unwrap();
    let mut wrapper = Command::new("sh")
        .args([
            "-c",
            "read _; \"$0\" --exact channel_client_child --ignored --nocapture --test-threads=1; exit $?",
        ])
        .arg(&exe)
        .env(CHILD_SOCKET, w.socket())
        .env(CHILD_LINE, req.to_line())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let pid = wrapper.id();
    let start = horch_platform::process::start_time(pid).unwrap();
    w.conn()
        .execute_batch(&format!(
            "INSERT INTO projects (project_id, root, created_at) VALUES ('p', '/repo', 'now');
             INSERT INTO runs (run_id, project_id, slug, status, started_at)
                VALUES ('r1', 'p', 'r1', 'active', 'now');
             INSERT INTO workers (worker_id, run_id, persona, host, status)
                VALUES ('w1', 'r1', 'opus-hardening', 'h', 'working');
             INSERT INTO attempts (id, task_id, worker_id, pid, pid_start, state)
                VALUES ('A-1', NULL, 'w1', {pid}, {start}, 'live');"
        ))
        .unwrap();
    wrapper.stdin.take().unwrap().write_all(b"go\n").unwrap();

    let stdout = BufReader::new(wrapper.stdout.take().unwrap());
    let printed: Vec<String> = stdout.lines().map(Result::unwrap).collect();
    let line = printed
        .iter()
        .find_map(|l| l.strip_prefix("REPLY ").map(str::to_string))
        .unwrap_or_else(|| panic!("the child prints the reply: {printed:?}"));
    assert!(wrapper.wait().unwrap().success());

    let got = reply(line.as_bytes());
    let Outcome::Refused(reason) = got.outcome else {
        panic!("a process of a live attempt is refused: {got:?}");
    };
    assert!(reason.contains("fleet process of attempt A-1"), "{reason}");
    assert_eq!(w.calls(), 0);
    let (state, _, auth, _) = w.row(&req.request_id).unwrap();
    assert_eq!((state.as_str(), auth), ("refused", None));
    server.stop().unwrap();
}

/// The child of [`operator_request_from_a_process_of_a_live_attempt_is_refused`]:
/// it sends 1 line and prints the reply. It does nothing outside that test.
#[test]
#[ignore = "runs only as the child of a test"]
fn channel_client_child() {
    let (Some(socket), Some(line)) = (
        std::env::var_os(CHILD_SOCKET),
        std::env::var(CHILD_LINE).ok(),
    ) else {
        return;
    };
    let got = ask(Path::new(&socket), line.as_bytes());
    // The harness prints on the same line, so the reply has its own line.
    println!("\nREPLY {}", String::from_utf8(got).unwrap().trim_end());
}

/// On Windows the channel is a pipe, and the path is no file.
#[cfg(unix)]
#[test]
fn old_socket_file_is_replaced_but_a_regular_file_is_not() {
    let w = World::new();
    let socket = w.socket();
    // An old fleetd left its socket file.
    drop(horch_platform::channel::Listener::bind(&socket).unwrap());
    assert!(socket.exists());
    let server = w.start(Options::default()).unwrap();
    let req = request(Kind::TaskCancel, Some(w.token()));
    let got = reply(&ask(&socket, req.to_line().as_bytes()));
    assert!(matches!(got.outcome, Outcome::Applied(_)), "{got:?}");
    server.stop().unwrap();

    // A regular file at the name is never removed.
    std::fs::remove_file(&socket).unwrap();
    std::fs::write(&socket, b"keep me").unwrap();
    let err = w.start(Options::default()).unwrap_err();
    assert!(format!("{err:#}").contains("not a socket"), "{err:#}");
    assert_eq!(std::fs::read(&socket).unwrap(), b"keep me");
    let open: i64 = w
        .conn()
        .query_row(
            "SELECT count(*) FROM needs_you WHERE state = 'open' AND entity_id = ?1",
            [socket.to_string_lossy()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(open, 1, "1 needs-you item names the file");
}
