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

const ROGUE: &str = "CHANNEL_CLIENT_ROGUE";
const MARK: &str = "ROGUE ";
const MAX: u64 = 64 * 1024;

struct Fleet {
    dir: TempDir,
}

impl Fleet {
    fn bare() -> Fleet {
        let dir = tempfile::Builder::new()
            .prefix("cc")
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir_in("/tmp")
            .unwrap();
        let fleet = Fleet { dir };
        Store::open(&fleet.db()).unwrap();
        fleet
    }

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

    fn fleetd_is_this_process(&self) {
        let pid = std::process::id();
        self.set_fleetd(pid, horch_platform::process::start_time(pid).unwrap());
    }

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

#[derive(Clone, Copy)]
enum Act {
    ApplyAndDrop,
    PendingAndDrop,
    Serve,
}

struct Server {
    received: Arc<Mutex<Vec<String>>>,
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

    fn finish(self) -> (Vec<String>, usize) {
        self.thread.join().unwrap();
        let received = self.received.lock().unwrap().clone();
        (received, self.handled.load(Ordering::SeqCst))
    }
}

#[test]
fn rogue_child() {
    let Some(path) = std::env::var_os(ROGUE) else {
        return;
    };
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

fn tell_rogue(socket: &Path, word: &str) {
    let mut stream = std::os::unix::net::UnixStream::connect(socket).unwrap();
    stream.write_all(word.as_bytes()).unwrap();
}

struct Rogue(std::process::Child);

impl Drop for Rogue {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn next_report(out: &mut impl BufRead) -> String {
    loop {
        let mut line = String::new();
        assert_ne!(
            out.read_line(&mut line).unwrap(),
            0,
            "the rogue child ended"
        );
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

    let err = channel_client::send(&fleet.ctx(), Kind::TaskCancel, cancel()).unwrap_err();
    assert!(format!("{err:#}").contains("fleetd row"), "{err:#}");
    tell_rogue(&fleet.socket(), "mark");
    assert_eq!(next_report(&mut out), "sent 0");

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

    let err = channel_client::send(&fleet.ctx(), Kind::Progress, Map::new()).unwrap_err();
    assert!(format!("{err:#}").contains("worker"), "{err:#}");
    assert!(fleet.request_files().is_empty());
}

#[test]
fn lost_reply_asks_for_the_result_before_a_second_send() {
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

    let sent = channel_client::send(&fleet.ctx(), Kind::TaskCancel, cancel()).unwrap();
    check_queued(&fleet, sent);

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
