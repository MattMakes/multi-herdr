//! The client of the command channel: it sends 1 request to the fleetd of
//! the `fleetd` row, asks for the stored result after a lost reply, and
//! writes a request file when fleetd is down
//! (docs/tickets/fleetd-p5/15-channel-client.md; RR-8, SR-5, SR-8, FM-15).
//!
//! The tests are unix only: the socket path must be short, and the mode
//! checks need unix modes. A test that needs a second process starts this
//! test binary again as a child with `CHANNEL_CLIENT_ROGUE` set.

#![cfg(unix)]

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use horch_core::fleetd::fleet_runtime::adapters::channel_client::{self, Sent};
use horch_core::fleetd::fleet_runtime::adapters::request_wire::{
    parse_request, Outcome, Reply, Request,
};
use horch_core::fleetd::fleet_runtime::adapters::{requests_store, token_file};
use horch_core::fleetd::fleet_runtime::domain::requests::Kind;
use horch_core::runtime::{MapEnv, RuntimeContext};
use horch_core::taskledger::store::Store;
use horch_platform::channel::Listener;
use serde_json::{json, Map, Value};
use tempfile::TempDir;

/// The env key that makes this binary a rogue listener at its path.
const ROGUE: &str = "CHANNEL_CLIENT_ROGUE";
/// The mark before each report line of the rogue child.
const MARK: &str = "ROGUE ";
/// The limit of a request line in these tests: the default of fleet.toml.
const MAX: u64 = 64 * 1024;

/// A fleet in temporary folders: its ledger, its fleet root and its token.
struct Fleet {
    dir: TempDir,
}

impl Fleet {
    /// A fleet with a migrated ledger and no token file.
    fn bare() -> Fleet {
        // A short folder: `sun_path` holds about 104 bytes on macOS.
        let dir = tempfile::Builder::new()
            .prefix("cc")
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir_in("/tmp")
            .unwrap();
        let fleet = Fleet { dir };
        Store::open(&fleet.db()).unwrap();
        fleet
    }

    /// A fleet with a token file, as fleetd makes it at its start.
    fn new() -> Fleet {
        let fleet = Fleet::bare();
        token_file::ensure(&fleet.root()).unwrap();
        fleet
    }

    fn db(&self) -> PathBuf {
        self.dir.path().join("l.db")
    }

    fn root(&self) -> PathBuf {
        self.dir.path().join("f")
    }

    fn socket(&self) -> PathBuf {
        self.root().join("run").join("fleetd.sock")
    }

    fn requests(&self) -> PathBuf {
        self.root().join("run").join("requests")
    }

    fn token(&self) -> String {
        token_file::read(&self.root()).unwrap().text().to_string()
    }

    fn ctx(&self) -> RuntimeContext {
        let dir = self.dir.path();
        let text = |p: PathBuf| p.to_string_lossy().into_owned();
        let env = MapEnv::new(dir)
            .with("HOME", &text(dir.to_path_buf()))
            .with("PATH", "/nonexistent")
            .with("HORCH_LEDGER_DB", &text(self.db()))
            .with("FLEET_ROOT", &text(self.root()))
            .with("HORCH_STATE_DIR", &text(dir.join("state")))
            .with("HORCH_DATA_DIR", &text(dir.join("data")));
        RuntimeContext::from_env(&env).unwrap()
    }

    /// Write the `fleetd` row for the process `pid` with the start time
    /// `pid_start`.
    fn set_fleetd(&self, pid: u32, pid_start: u64) {
        Store::open(&self.db())
            .unwrap()
            .conn()
            .execute(
                "INSERT OR REPLACE INTO fleetd
                 (id, generation, pid, pid_start, started_at, fleet_clock_s, clock_ticked_at)
                 VALUES (1, 1, ?1, ?2, 'now', 0, 'now')",
                rusqlite::params![i64::from(pid), i64::try_from(pid_start).unwrap()],
            )
            .unwrap();
    }

    /// The `fleetd` row names this test process, which runs the test
    /// server in a thread.
    fn fleetd_is_this_process(&self) {
        let pid = std::process::id();
        self.set_fleetd(pid, horch_platform::process::start_time(pid).unwrap());
    }

    /// The names of the files in the request folder.
    fn request_files(&self) -> Vec<String> {
        let Ok(entries) = std::fs::read_dir(self.requests()) else {
            return Vec::new();
        };
        let mut names: Vec<String> = entries
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }
}

fn payload(v: Value) -> Map<String, Value> {
    v.as_object().unwrap().clone()
}

fn cancel() -> Map<String, Value> {
    payload(json!({"task": "T-1"}))
}

fn applied() -> Value {
    json!({"task": "T-1", "state": "cancelled"})
}

fn mode(path: &Path) -> u32 {
    std::fs::symlink_metadata(path)
        .unwrap()
        .permissions()
        .mode()
        & 0o7777
}

/// What the test server does with 1 connection.
#[derive(Clone, Copy)]
enum Act {
    /// Apply the request (1 handler call, the row `applied`), then end the
    /// connection with no reply.
    ApplyAndDrop,
    /// Write the `pending` row, then end the connection with no reply, as a
    /// fleetd that stops in the middle of the request.
    PendingAndDrop,
    /// Reply. A request that is applied gets its stored result and no
    /// handler call; else the handler applies it and the server replies.
    Serve,
}

/// A test fleetd in a thread of this process: it accepts 1 connection for
/// each act, then closes the channel.
struct Server {
    /// The `request_id` of each request line that the server read.
    received: Arc<Mutex<Vec<String>>>,
    /// The count of handler calls.
    handled: Arc<AtomicUsize>,
    thread: JoinHandle<()>,
}

impl Server {
    fn start(fleet: &Fleet, acts: Vec<Act>) -> Server {
        fleet.fleetd_is_this_process();
        let listener = Listener::bind(&fleet.socket()).unwrap();
        let db = fleet.db();
        let received = Arc::new(Mutex::new(Vec::new()));
        let handled = Arc::new(AtomicUsize::new(0));
        let (seen, calls) = (received.clone(), handled.clone());
        let thread = std::thread::spawn(move || {
            let store = Store::open(&db).unwrap();
            let conn = store.conn();
            for act in acts {
                let (mut stream, _) = listener.accept().unwrap();
                let mut line = Vec::new();
                BufReader::new(&stream)
                    .read_until(b'\n', &mut line)
                    .unwrap();
                let request = parse_request(&line, MAX).unwrap();
                let id = request.request_id.as_str().to_string();
                seen.lock().unwrap().push(id.clone());
                let row = requests_store::get(conn, &id).unwrap();
                if row.is_none() {
                    requests_store::insert_pending(conn, &request, "uid").unwrap();
                }
                let result = match (act, row.map(|r| r.stored)) {
                    (Act::PendingAndDrop, _) => continue,
                    (Act::Serve, Some(requests_store::Stored::Applied(stored))) => stored,
                    (_, _) => {
                        calls.fetch_add(1, Ordering::SeqCst);
                        requests_store::set_applied(conn, &id, "token", &applied()).unwrap();
                        applied()
                    }
                };
                if let Act::ApplyAndDrop = act {
                    continue;
                }
                let reply = Reply {
                    request_id: request.request_id,
                    outcome: Outcome::Applied(result),
                };
                stream.write_all(reply.to_line().as_bytes()).unwrap();
            }
        });
        Server {
            received,
            handled,
            thread,
        }
    }

    /// Wait for the end of each act; give the ids read and the handler calls.
    fn finish(self) -> (Vec<String>, usize) {
        self.thread.join().unwrap();
        let received = self.received.lock().unwrap().clone();
        (received, self.handled.load(Ordering::SeqCst))
    }
}

/// The child half of `client_refuses_a_server_that_is_not_the_fleetd_of_the_ledger`:
/// bind the channel at the path of `ROGUE`, and count the bytes that each
/// client sends. A connection that sends `mark` reports the count since the
/// last mark; a connection that sends `quit` ends the child. It does
/// nothing in a normal test run.
#[test]
fn rogue_child() {
    let Some(path) = std::env::var_os(ROGUE) else {
        return;
    };
    // A plain socket, as a rogue process binds it. The accept of the
    // platform reads the facts of the peer, which fail for a client that
    // closed already.
    let listener = std::os::unix::net::UnixListener::bind(Path::new(&path)).unwrap();
    println!("{MARK}bound");
    let mut sent = 0;
    loop {
        let (mut stream, _) = listener.accept().unwrap();
        let mut bytes = Vec::new();
        stream.read_to_end(&mut bytes).unwrap();
        match bytes.as_slice() {
            b"mark" => {
                println!("{MARK}sent {sent}");
                sent = 0;
            }
            b"quit" => break,
            _ => sent += bytes.len(),
        }
    }
}

/// Connect to the rogue child and send `word`. The child reads each
/// connection in the order of the connects, so a `mark` comes after each
/// connection of the client.
fn tell_rogue(socket: &Path, word: &str) {
    let mut stream = std::os::unix::net::UnixStream::connect(socket).unwrap();
    stream.write_all(word.as_bytes()).unwrap();
}

/// The rogue child. A failed test kills it, so no child outlives the test.
struct Rogue(std::process::Child);

impl Drop for Rogue {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// The next report line of the rogue child.
fn next_report(out: &mut impl BufRead) -> String {
    loop {
        let mut line = String::new();
        assert_ne!(
            out.read_line(&mut line).unwrap(),
            0,
            "the rogue child ended"
        );
        // libtest can print `test rogue_child ... ` before the mark on 1 line.
        if let Some((_, report)) = line.trim_end().split_once(MARK) {
            return report.to_string();
        }
    }
}

#[test]
fn client_refuses_a_server_that_is_not_the_fleetd_of_the_ledger() {
    let fleet = Fleet::new();
    let mut child = Rogue(
        Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "rogue_child", "--nocapture", "--test-threads=1"])
            .env(ROGUE, fleet.socket())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let mut out = BufReader::new(child.0.stdout.take().unwrap());
    assert_eq!(next_report(&mut out), "bound");

    // No `fleetd` row: the server process is unknown, so nothing is sent.
    let err = channel_client::send(&fleet.ctx(), Kind::TaskCancel, cancel()).unwrap_err();
    assert!(format!("{err:#}").contains("fleetd row"), "{err:#}");
    tell_rogue(&fleet.socket(), "mark");
    assert_eq!(next_report(&mut out), "sent 0");

    // The row names this test process; the listener is the rogue child.
    fleet.fleetd_is_this_process();
    let err = channel_client::send(&fleet.ctx(), Kind::TaskCancel, cancel()).unwrap_err();
    let text = format!("{err:#}");
    assert!(text.contains("is not the fleetd"), "{text}");
    assert!(!text.contains(&fleet.token()), "the error hides the token");
    tell_rogue(&fleet.socket(), "mark");
    assert_eq!(next_report(&mut out), "sent 0");
    tell_rogue(&fleet.socket(), "quit");

    assert!(child.0.wait().unwrap().success());
    assert!(
        fleet.request_files().is_empty(),
        "a refusal writes no request file"
    );
}

#[test]
fn client_sends_once_and_gets_the_reply() {
    let fleet = Fleet::new();
    let server = Server::start(&fleet, vec![Act::Serve]);
    let sent = channel_client::send(&fleet.ctx(), Kind::TaskCancel, cancel()).unwrap();
    let (received, handled) = server.finish();
    let Sent::Replied(reply) = sent else {
        panic!("expected a reply, got {sent:?}");
    };
    assert_eq!(received, vec![reply.request_id.as_str().to_string()]);
    assert_eq!(reply.outcome, Outcome::Applied(applied()));
    assert_eq!(handled, 1);

    // The channel is closed now, so a send of a worker kind that reached
    // it would write a request file. The client refuses it first.
    let err = channel_client::send(&fleet.ctx(), Kind::Progress, Map::new()).unwrap_err();
    assert!(format!("{err:#}").contains("worker"), "{err:#}");
    assert!(fleet.request_files().is_empty());
}

#[test]
fn lost_reply_asks_for_the_result_before_a_second_send() {
    // fleetd applied the request, and the reply was lost: the client reads
    // the stored result and sends no second time. The channel is closed
    // after 1 connection, so a second send would write a request file.
    let fleet = Fleet::new();
    let server = Server::start(&fleet, vec![Act::ApplyAndDrop]);
    let sent = channel_client::send(&fleet.ctx(), Kind::TaskCancel, cancel()).unwrap();
    let (received, handled) = server.finish();
    let Sent::Replied(reply) = sent else {
        panic!("expected the stored result, got {sent:?}");
    };
    assert_eq!(received, vec![reply.request_id.as_str().to_string()]);
    assert_eq!(reply.outcome, Outcome::Applied(applied()));
    assert_eq!(handled, 1);
    assert!(fleet.request_files().is_empty());

    // fleetd stopped with the row `pending`: the client sends again with
    // the same `request_id`, and the handler runs 1 time.
    let fleet = Fleet::new();
    let server = Server::start(&fleet, vec![Act::PendingAndDrop, Act::Serve]);
    let sent = channel_client::send(&fleet.ctx(), Kind::TaskCancel, cancel()).unwrap();
    let (received, handled) = server.finish();
    let Sent::Replied(reply) = sent else {
        panic!("expected a reply, got {sent:?}");
    };
    let id = reply.request_id.as_str().to_string();
    assert_eq!(received, vec![id.clone(), id]);
    assert_eq!(reply.outcome, Outcome::Applied(applied()));
    assert_eq!(handled, 1);
}

/// Check the request file of a queued send: its place, its modes and its
/// request.
fn check_queued(fleet: &Fleet, sent: Sent) {
    let Sent::Queued { request_id, path } = sent else {
        panic!("expected a request file, got {sent:?}");
    };
    assert_eq!(
        path,
        fleet
            .requests()
            .join(format!("{}.json", request_id.as_str()))
    );
    assert_eq!(mode(&path), 0o600);
    assert_eq!(mode(&fleet.requests()), 0o700);
    let request = parse_request(&std::fs::read(&path).unwrap(), MAX).unwrap();
    assert_eq!(
        request,
        Request {
            request_id,
            kind: Kind::TaskCancel,
            payload: cancel(),
            token: Some(fleet.token()),
        }
    );
}

#[test]
fn fleetd_down_writes_a_request_file_with_mode_0600() {
    let fleet = Fleet::new();
    fleet.fleetd_is_this_process();

    // No socket file: NotFound.
    let sent = channel_client::send(&fleet.ctx(), Kind::TaskCancel, cancel()).unwrap();
    check_queued(&fleet, sent);

    // A socket file with no listener: the connection is refused.
    drop(std::os::unix::net::UnixListener::bind(fleet.socket()).unwrap());
    assert!(fleet.socket().exists());
    let sent = channel_client::send(&fleet.ctx(), Kind::TaskCancel, cancel()).unwrap();
    check_queued(&fleet, sent);

    let files = fleet.request_files();
    assert_eq!(files.len(), 2, "{files:?}");
    assert!(files.iter().all(|f| f.ends_with(".json")), "{files:?}");
}

#[test]
fn request_file_is_written_atomically() {
    const SENDS: usize = 40;
    let fleet = Fleet::new();
    let folder = fleet.requests();
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o700)).unwrap();
    let blob = "x".repeat(30_000);

    // A reader reads each complete name in the folder while the client
    // writes: each read gives a whole request.
    let done = Arc::new(AtomicBool::new(false));
    let reader = {
        let (done, folder) = (done.clone(), folder.clone());
        std::thread::spawn(move || {
            let mut reads = 0usize;
            while !done.load(Ordering::SeqCst) {
                for entry in std::fs::read_dir(&folder).unwrap() {
                    let path = entry.unwrap().path();
                    if path.extension().is_some_and(|e| e == "tmp") {
                        continue;
                    }
                    let bytes = std::fs::read(&path).unwrap();
                    if let Err(e) = parse_request(&bytes, MAX) {
                        panic!("{} is partial ({} bytes): {e}", path.display(), bytes.len());
                    }
                    reads += 1;
                }
            }
            reads
        })
    };
    for _ in 0..SENDS {
        let sent = channel_client::send(
            &fleet.ctx(),
            Kind::TaskCancel,
            payload(json!({"task": "T-1", "blob": blob})),
        )
        .unwrap();
        assert!(matches!(sent, Sent::Queued { .. }), "{sent:?}");
    }
    done.store(true, Ordering::SeqCst);
    reader.join().unwrap();

    let files = fleet.request_files();
    assert_eq!(files.len(), SENDS, "no temp file stays: {files:?}");
    for name in files {
        let path = folder.join(&name);
        assert_eq!(mode(&path), 0o600, "{name}");
        assert!(parse_request(&std::fs::read(&path).unwrap(), MAX).is_ok());
    }
}

#[test]
fn missing_token_file_is_an_error_that_names_the_file() {
    let fleet = Fleet::bare();
    let token = token_file::path(&fleet.root());
    let err = channel_client::send(&fleet.ctx(), Kind::TaskCancel, cancel()).unwrap_err();
    let text = format!("{err:#}");
    assert!(text.contains(&token.display().to_string()), "{text}");
    assert!(fleet.request_files().is_empty());
}
