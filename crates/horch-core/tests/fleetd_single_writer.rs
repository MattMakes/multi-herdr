//! RR-5 and FM-24: only fleetd writes the ledger. Each other process opens
//! it read-only, and a writable open in a process with the test seam
//! `HORCH_TEST_ASSERT_READ_ONLY=1` panics (ticket 42 of fleetd-p5).
//!
//! The seam reads the environment of the process. So each test runs its
//! process side in a child: this test binary again, with `SINGLE_WRITER_CHILD`
//! set to the name of the case and with the seam set. The tests are unix
//! only: the socket path must be short, and the mode checks need unix modes.

#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use horch_core::fleetd::fleet_runtime::adapters::channel_client::{self, Sent};
use horch_core::fleetd::fleet_runtime::adapters::channel_server::ChannelServer;
use horch_core::fleetd::fleet_runtime::adapters::request_wire::Outcome;
use horch_core::fleetd::fleet_runtime::adapters::token_file;
use horch_core::fleetd::fleet_runtime::application::requests::Handlers;
use horch_core::fleetd::fleet_runtime::domain::ledger::Generation;
use horch_core::fleetd::fleet_runtime::domain::requests::Kind;
use horch_core::runtime::{MapEnv, RuntimeContext};
use horch_core::taskledger::runs::{self, NewRun};
use horch_core::taskledger::store::{LedgerWriter, ASSERT_READ_ONLY_ENV};
use horch_core::taskledger::Store;
use serde_json::json;
use tempfile::TempDir;

/// The env key that makes this binary the child of 1 case.
const CHILD: &str = "SINGLE_WRITER_CHILD";
/// The env key that names the folder of the fleet to the child.
const DIR: &str = "SINGLE_WRITER_DIR";
/// The mark before each report line of the child.
const MARK: &str = "CHILD ";

/// A fleet in temporary folders: its ledger and its fleet root.
struct Fleet {
    dir: PathBuf,
}

impl Fleet {
    fn at(dir: &Path) -> Fleet {
        Fleet {
            dir: dir.to_path_buf(),
        }
    }

    fn db(&self) -> PathBuf {
        self.dir.join("l.db")
    }

    fn root(&self) -> PathBuf {
        self.dir.join("f")
    }

    fn proj(&self) -> PathBuf {
        self.dir.join("proj")
    }

    fn ctx(&self) -> RuntimeContext {
        let text = |p: PathBuf| p.to_string_lossy().into_owned();
        let env = MapEnv::new(self.proj())
            .with("HOME", &text(self.dir.clone()))
            .with("PATH", "/nonexistent")
            .with("HORCH_LEDGER_DB", &text(self.db()))
            .with("FLEET_ROOT", &text(self.root()))
            .with("HORCH_STATE_DIR", &text(self.dir.join("state")))
            .with("HORCH_DATA_DIR", &text(self.dir.join("data")));
        RuntimeContext::from_env(&env).unwrap()
    }
}

/// A new fleet folder: short, for the `sun_path` limit of macOS.
fn fleet_dir() -> TempDir {
    let dir = tempfile::Builder::new()
        .prefix("sw")
        .permissions(std::fs::Permissions::from_mode(0o700))
        .tempdir_in("/tmp")
        .unwrap();
    std::fs::create_dir_all(dir.path().join("proj")).unwrap();
    dir
}

/// A fleet with a migrated ledger that holds 1 run, `r1`.
fn fleet_with_a_run(dir: &Path) -> Fleet {
    let fleet = Fleet::at(dir);
    let store = Store::open(&fleet.db()).unwrap();
    runs::create(
        store.conn(),
        &fleet.proj(),
        &NewRun {
            slug: "r1".into(),
            ..Default::default()
        },
    )
    .unwrap();
    fleet
}

/// Run `case` in a child process with the seam set.
fn child(case: &str, dir: &Path) -> Output {
    Command::new(std::env::current_exe().unwrap())
        .args([case, "--exact", "--nocapture", "--test-threads", "1"])
        .env(CHILD, case)
        .env(DIR, dir)
        .env(ASSERT_READ_ONLY_ENV, "1")
        .output()
        .unwrap()
}

/// The case that this process runs as a child, with its fleet folder.
fn as_child(case: &str) -> Option<Fleet> {
    if std::env::var(CHILD).ok().as_deref() != Some(case) {
        return None;
    }
    Some(Fleet::at(Path::new(&std::env::var(DIR).unwrap())))
}

/// The report lines of the child, without the mark. The test harness of
/// the child can print the name of the test before the first one.
fn reports(o: &Output) -> Vec<String> {
    String::from_utf8_lossy(&o.stdout)
        .lines()
        .filter_map(|l| l.find(MARK).map(|at| l[at + MARK.len()..].to_string()))
        .collect()
}

fn text(o: &Output) -> String {
    format!(
        "status {:?}\nstdout:\n{}\nstderr:\n{}",
        o.status,
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

/// The bytes of the ledger file, and the size of its WAL file (0 when it
/// has none). SQLite makes an empty WAL file for a read-only reader.
fn ledger_bytes(db: &Path) -> (Vec<u8>, u64) {
    let mut wal = db.as_os_str().to_owned();
    wal.push("-wal");
    let wal = std::fs::metadata(wal).map_or(0, |m| m.len());
    (std::fs::read(db).unwrap(), wal)
}

const CLI_CASE: &str = "cli_command_opens_ledger_read_only_and_sends_a_request";

/// A command that changes state: under the seam it opens the ledger only
/// read-only (to check the fleetd row) and sends 1 request. fleetd, here
/// the channel server in this test process, writes the `requests` row.
#[test]
fn cli_command_opens_ledger_read_only_and_sends_a_request() {
    if let Some(fleet) = as_child(CLI_CASE) {
        let sent = channel_client::send(
            &fleet.ctx(),
            Kind::TaskCancel,
            json!({"task": "T-1"}).as_object().unwrap().clone(),
        )
        .unwrap();
        let Sent::Replied(reply) = sent else {
            panic!("expected a reply of fleetd, got {sent:?}");
        };
        let Outcome::Applied(result) = reply.outcome else {
            panic!("expected an applied request, got {:?}", reply.outcome);
        };
        println!("{MARK}applied {result}");
        return;
    }
    let dir = fleet_dir();
    let fleet = fleet_with_a_run(dir.path());
    token_file::ensure(&fleet.root()).unwrap();
    // The fleetd of the test: the `fleetd` row names this process.
    let pid = std::process::id();
    let start = horch_platform::process::start_time(pid).unwrap();
    let writer = LedgerWriter::fleetd();
    let ledger = writer.open(&fleet.db()).unwrap();
    ledger
        .conn()
        .execute(
            "INSERT INTO fleetd (id, generation, pid, pid_start, started_at, fleet_clock_s, clock_ticked_at)
             VALUES (1, 1, ?1, ?2, 'now', 0, 'now')",
            rusqlite::params![i64::from(pid), i64::try_from(start).unwrap()],
        )
        .unwrap();
    let mut handlers = Handlers::new();
    handlers.insert(Kind::TaskCancel, |_tx, payload| {
        Ok(json!({"cancelled": payload["task"]}))
    });
    let conn = rusqlite::Connection::open(fleet.db()).unwrap();
    conn.busy_timeout(std::time::Duration::from_secs(10))
        .unwrap();
    let _server = ChannelServer::start(&fleet.root(), conn, Generation(1), handlers).unwrap();

    let o = child(CLI_CASE, dir.path());
    assert!(o.status.success(), "{}", text(&o));
    assert_eq!(
        reports(&o),
        [r#"applied {"cancelled":"T-1"}"#],
        "{}",
        text(&o)
    );
    let rows: Vec<(String, String)> = {
        let mut stmt = ledger
            .conn()
            .prepare("SELECT kind, state FROM requests")
            .unwrap();
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    };
    assert_eq!(rows, [("task_cancel".to_string(), "applied".to_string())]);
}

const READ_CASE: &str = "read_command_on_a_read_only_ledger_file_works";

/// A read opens the ledger read-only: it works on a ledger file that the
/// process cannot write, and it does not change the file.
#[test]
fn read_command_on_a_read_only_ledger_file_works() {
    if let Some(fleet) = as_child(READ_CASE) {
        let ctx = fleet.ctx();
        let store = horch_core::taskledger::open_existing_read_only(&ctx)
            .unwrap()
            .expect("the ledger exists");
        for run in runs::of_project(store.conn(), &fleet.proj()).unwrap() {
            println!("{MARK}run {}", run.slug);
        }
        return;
    }
    let dir = fleet_dir();
    let fleet = fleet_with_a_run(dir.path());
    std::fs::set_permissions(fleet.db(), std::fs::Permissions::from_mode(0o444)).unwrap();
    let before = ledger_bytes(&fleet.db());
    assert_eq!(before.1, 0, "the WAL of the ledger holds no frame");

    let o = child(READ_CASE, dir.path());
    assert!(o.status.success(), "{}", text(&o));
    assert_eq!(reports(&o), ["run r1"], "{}", text(&o));
    assert_eq!(
        ledger_bytes(&fleet.db()),
        before,
        "the read changed the ledger"
    );
    let mode = std::fs::metadata(fleet.db()).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o444);
}

const SEAM_CASE: &str = "writable_open_outside_fleetd_panics_under_the_seam";

/// Under the seam, a writable open in a process that made no fleetd writer
/// panics before it touches the file. The writer of fleetd opens it.
#[test]
fn writable_open_outside_fleetd_panics_under_the_seam() {
    if let Some(fleet) = as_child(SEAM_CASE) {
        let ctx = fleet.ctx();
        let opened = std::panic::catch_unwind(|| Store::open(&fleet.db()).map(drop));
        println!("{MARK}Store::open panics: {}", opened.is_err());
        let opened = std::panic::catch_unwind(|| horch_core::taskledger::open(&ctx).map(drop));
        println!("{MARK}taskledger::open panics: {}", opened.is_err());
        println!("{MARK}ledger exists: {}", fleet.db().exists());
        let writer = LedgerWriter::fleetd();
        writer.open(&fleet.db()).unwrap();
        println!("{MARK}the fleetd writer opens it");
        Store::open(&fleet.db()).unwrap();
        println!("{MARK}after the writer, Store::open works");
        return;
    }
    let dir = fleet_dir();
    let o = child(SEAM_CASE, dir.path());
    assert!(o.status.success(), "{}", text(&o));
    assert_eq!(
        reports(&o),
        [
            "Store::open panics: true",
            "taskledger::open panics: true",
            "ledger exists: false",
            "the fleetd writer opens it",
            "after the writer, Store::open works",
        ],
        "{}",
        text(&o)
    );
    assert!(
        String::from_utf8_lossy(&o.stderr).contains(ASSERT_READ_ONLY_ENV),
        "the panic names the seam: {}",
        text(&o)
    );
    // Without the seam, this process opens the ledger as each test does.
    Store::open(&dir.path().join("other.db")).unwrap();
}
