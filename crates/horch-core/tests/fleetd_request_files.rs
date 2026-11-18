//! The request files of `<fleet root>/run/requests/`: the folder modes, the
//! checks of each file, the `requests` row, then the archive
//! (docs/tickets/fleetd-p5/14-request-file-spool.md; SR-5, SR-6, RR-8,
//! FM-15, FM-23).
//!
//! Each test uses a temp fleet root, a temp ledger at generation 1, the
//! operator token of the fleet root, and a handler of `task_cancel` that
//! counts its calls and writes 1 row of the table `probe` for each call.
//! The time of a scan comes from a test clock, never from the host.
#![cfg(unix)]

use std::fs::{self, OpenOptions, Permissions};
use std::io::Write;
use std::os::unix::fs::{symlink, OpenOptionsExt, PermissionsExt};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use horch_core::fleetd::fleet_runtime::adapters::cursors_store;
use horch_core::fleetd::fleet_runtime::adapters::request_spool::{
    scan, FileFact, Handled, ScanOutcome, Spool, FOLDER_REFUSED,
};
use horch_core::fleetd::fleet_runtime::adapters::request_wire::{Outcome, Request};
use horch_core::fleetd::fleet_runtime::adapters::scans::{requests_dir, ScanClock};
use horch_core::fleetd::fleet_runtime::adapters::token_file::{self, Token};
use horch_core::fleetd::fleet_runtime::application::requests::{handle, Handlers};
use horch_core::fleetd::fleet_runtime::domain::authorize::Decision;
use horch_core::fleetd::fleet_runtime::domain::budget::{Cursor, Deadline, ScanKind};
use horch_core::fleetd::fleet_runtime::domain::ledger::Generation;
use horch_core::fleetd::fleet_runtime::domain::requests::{Kind, RequestId};
use horch_core::taskledger::{schema, util};
use horch_platform::channel::User;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};

const ID: &str = "6f1c2a3e-0b9d-4c55-8e7f-1a2b3c4d5e6f";
const STAMP: &str = "2026-11-18T00:00:00Z";

/// A temp fleet root with its token, and a temp ledger at generation 1 with
/// the table `probe` of the handler.
struct World {
    _temp: tempfile::TempDir,
    root: PathBuf,
    conn: Connection,
    token: Token,
    owner: User,
    handlers: Handlers,
    calls: Arc<AtomicUsize>,
}

fn world() -> World {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("fleet");
    let token = token_file::ensure(&root).unwrap();
    let mut conn = Connection::open(temp.path().join("ledger.sqlite3")).unwrap();
    conn.busy_timeout(Duration::from_secs(10)).unwrap();
    schema::migrate_to(&mut conn, schema::VERSION).unwrap();
    conn.execute(
        "INSERT INTO fleetd (id, generation, pid, pid_start, started_at, fleet_clock_s, clock_ticked_at)
         VALUES (1, 1, ?1, 0, ?2, 0, ?2)",
        params![std::process::id(), util::now()],
    )
    .unwrap();
    conn.execute_batch("CREATE TABLE probe (request TEXT NOT NULL)")
        .unwrap();
    let (handlers, calls) = counting(None);
    World {
        _temp: temp,
        root,
        conn,
        token,
        owner: User::current().unwrap(),
        handlers,
        calls,
    }
}

/// Handlers with 1 handler for `task_cancel`: it counts its calls, writes 1
/// row of `probe`, and returns the count. When `panic_at` is that call, it
/// panics after its row, as a crash of fleetd before the commit.
fn counting(panic_at: Option<usize>) -> (Handlers, Arc<AtomicUsize>) {
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&calls);
    let mut handlers = Handlers::new();
    handlers.insert(Kind::TaskCancel, move |tx, payload| {
        let n = seen.fetch_add(1, Ordering::SeqCst) + 1;
        tx.execute(
            "INSERT INTO probe (request) VALUES (?1)",
            [payload.to_string()],
        )?;
        assert_ne!(Some(n), panic_at, "fleetd crashes before the commit");
        Ok(json!({ "calls": n }))
    });
    (handlers, calls)
}

impl World {
    fn dir(&self) -> PathBuf {
        requests_dir(&self.root)
    }

    /// 1 scan with `owner` as the ledger owner and the time of `clock`.
    fn scan_with(
        &mut self,
        owner: &User,
        cursor: &mut Cursor,
        clock: &mut ScanClock<'_>,
    ) -> ScanOutcome {
        let spool = Spool {
            fleet_root: &self.root,
            owner,
            token: Some(&self.token),
            max_bytes: 64 * 1024,
            guard: Generation(1),
        };
        scan(&mut self.conn, &spool, cursor, clock, &self.handlers).unwrap()
    }

    /// 1 scan with no deadline in sight.
    fn scan(&mut self, cursor: &mut Cursor) -> ScanOutcome {
        let owner = self.owner.clone();
        self.scan_as(&owner, cursor)
    }

    /// 1 scan as `owner` with no deadline in sight.
    fn scan_as(&mut self, owner: &User, cursor: &mut Cursor) -> ScanOutcome {
        let start = Instant::now();
        let mut now = move || start;
        let mut clock = ScanClock {
            deadline: Deadline::pass(start, 3600),
            now: &mut now,
            stamp: STAMP,
        };
        self.scan_with(owner, cursor, &mut clock)
    }

    /// The request file of `id` with the token of the fleet, as the client
    /// writes it: `<id>.json.tmp`, then a rename, with mode 0600.
    fn put(&self, id: &str) -> PathBuf {
        fs::create_dir_all(self.dir()).unwrap();
        fs::set_permissions(self.dir(), Permissions::from_mode(0o700)).unwrap();
        let line = request(id, Some(self.token.text())).to_line();
        let path = self.dir().join(format!("{id}.json"));
        let tmp = self.dir().join(format!("{id}.json.tmp"));
        write_mode(&tmp, line.as_bytes(), 0o600);
        fs::rename(&tmp, &path).unwrap();
        path
    }

    fn archived(&self, id: &str) -> PathBuf {
        self.dir().join("archive").join(format!("{id}.json"))
    }

    fn count(&self, table: &str) -> i64 {
        self.conn
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
            .unwrap()
    }

    /// The state of the `requests` row of `id`, and its caller.
    fn row(&self, id: &str) -> Option<(String, String)> {
        self.conn
            .query_row(
                "SELECT state, caller FROM requests WHERE request_id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .unwrap()
    }

    /// The outcome and the reason of the `ingested` row of `path`.
    fn ingested(&self, path: &Path) -> Option<(String, Option<String>)> {
        self.conn
            .query_row(
                "SELECT outcome, reason FROM ingested WHERE path = ?1",
                [path.display().to_string()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .unwrap()
    }
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

fn write_mode(path: &Path, bytes: &[u8], mode: u32) {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode)
        .open(path)
        .unwrap();
    file.write_all(bytes).unwrap();
    // The umask can narrow the mode of the create; set it exactly.
    fs::set_permissions(path, Permissions::from_mode(mode)).unwrap();
}

/// The facts of a scan that read the folder.
fn facts(outcome: ScanOutcome) -> Vec<FileFact> {
    match outcome {
        ScanOutcome::Read(scan) => scan.facts,
        other => panic!("expected a scan of the folder, got {other:?}"),
    }
}

fn names(n: usize) -> Vec<String> {
    (0..n)
        .map(|i| format!("00000000-0000-4000-8000-00000000000{i}"))
        .collect()
}

#[test]
fn request_file_is_applied_then_archived() {
    let mut w = world();
    let path = w.put(ID);

    let read = facts(w.scan(&mut Cursor::new(STAMP)));

    assert_eq!(
        read,
        vec![FileFact {
            name: format!("{ID}.json"),
            handled: Handled::Archived(Outcome::Applied(json!({ "calls": 1 }))),
        }]
    );
    assert_eq!(w.calls.load(Ordering::SeqCst), 1);
    assert_eq!(w.count("probe"), 1);
    let User::Uid(uid) = w.owner else {
        panic!("a unix user is a uid")
    };
    assert_eq!(w.row(ID), Some(("applied".into(), format!("uid {uid}"))));
    assert!(!path.exists(), "the file left the request folder");
    assert!(w.archived(ID).is_file(), "the file is in archive/");
    let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&w.dir()), 0o700);
    assert_eq!(mode(&w.dir().join("archive")), 0o700);
    assert_eq!(
        w.count("ingested"),
        0,
        "a file that parses has its requests row"
    );
}

#[test]
fn request_file_with_no_token_gets_a_refused_row() {
    let mut w = world();
    fs::create_dir_all(w.dir()).unwrap();
    fs::set_permissions(w.dir(), Permissions::from_mode(0o700)).unwrap();
    let path = w.dir().join(format!("{ID}.json"));
    write_mode(&path, request(ID, None).to_line().as_bytes(), 0o600);

    let read = facts(w.scan(&mut Cursor::new(STAMP)));

    let [FileFact {
        handled: Handled::Archived(Outcome::Refused(reason)),
        ..
    }] = read.as_slice()
    else {
        panic!("expected 1 refusal, got {read:?}")
    };
    assert!(reason.contains("operator token"), "{reason}");
    assert_eq!(w.row(ID).unwrap().0, "refused");
    assert_eq!(w.calls.load(Ordering::SeqCst), 0);
    assert!(w.archived(ID).is_file());
}

#[test]
fn file_with_mode_0644_is_refused_with_its_reason() {
    let mut w = world();
    let path = w.put(ID);
    fs::set_permissions(&path, Permissions::from_mode(0o644)).unwrap();

    let read = facts(w.scan(&mut Cursor::new(STAMP)));

    let reason = "mode 0644 is wider than 0600".to_string();
    assert_eq!(
        read,
        vec![FileFact {
            name: format!("{ID}.json"),
            handled: Handled::Rejected(reason.clone()),
        }]
    );
    assert_eq!(w.ingested(&path), Some(("rejected".into(), Some(reason))));
    assert_eq!(w.row(ID), None);
    assert!(path.is_file(), "a rejected file stays where it is");

    // The path is not read again.
    assert_eq!(facts(w.scan(&mut Cursor::new(STAMP))), Vec::new());
    assert_eq!(w.count("ingested"), 1);
}

#[test]
fn file_of_another_owner_is_refused() {
    let mut w = world();
    let path = w.put(ID);
    let User::Uid(uid) = w.owner else {
        panic!("a unix user is a uid")
    };
    // A test cannot make a file of another user, so the ledger owner of the
    // scan is a user that does not own the file (Q13).
    let other = User::Uid(uid + 1);
    let outcome = w.scan_as(&other, &mut Cursor::new(STAMP));

    let reason = format!("owner uid {uid} is not the ledger owner uid {}", uid + 1);
    assert_eq!(
        facts(outcome),
        vec![FileFact {
            name: format!("{ID}.json"),
            handled: Handled::Rejected(reason.clone()),
        }]
    );
    assert_eq!(w.ingested(&path), Some(("rejected".into(), Some(reason))));
    assert_eq!(w.row(ID), None);
    assert_eq!(w.calls.load(Ordering::SeqCst), 0);
}

#[test]
fn symlink_request_file_is_rejected() {
    let mut w = world();
    let target = w.put(ID);
    let moved = w.root.join("elsewhere.json");
    fs::rename(&target, &moved).unwrap();
    let link = w.dir().join(format!("{ID}.json"));
    symlink(&moved, &link).unwrap();

    let read = facts(w.scan(&mut Cursor::new(STAMP)));

    assert_eq!(
        read,
        vec![FileFact {
            name: format!("{ID}.json"),
            handled: Handled::Rejected("the file is a link".into()),
        }]
    );
    assert_eq!(w.row(ID), None);
    assert_eq!(w.calls.load(Ordering::SeqCst), 0);
    assert!(fs::symlink_metadata(&link)
        .unwrap()
        .file_type()
        .is_symlink());
}

#[test]
fn file_whose_name_is_not_its_request_id_is_rejected() {
    let mut w = world();
    let path = w.put(ID);
    let other = w.dir().join(format!("{}.json", names(1)[0]));
    fs::rename(&path, &other).unwrap();

    let read = facts(w.scan(&mut Cursor::new(STAMP)));

    let [FileFact {
        handled: Handled::Rejected(reason),
        ..
    }] = read.as_slice()
    else {
        panic!("expected 1 rejected file, got {read:?}")
    };
    assert!(reason.contains("is not <request_id>.json"), "{reason}");
    assert_eq!(w.count("requests"), 0);
}

#[test]
fn tmp_file_is_skipped() {
    let mut w = world();
    fs::create_dir_all(w.dir()).unwrap();
    fs::set_permissions(w.dir(), Permissions::from_mode(0o700)).unwrap();
    let tmp = w.dir().join(format!("{ID}.json.tmp"));
    write_mode(
        &tmp,
        request(ID, Some(w.token.text())).to_line().as_bytes(),
        0o600,
    );

    let read = facts(w.scan(&mut Cursor::new(STAMP)));

    assert_eq!(read, Vec::new());
    assert!(tmp.is_file(), "a .tmp file stays for its writer");
    assert_eq!(w.count("requests"), 0);
    assert_eq!(w.count("ingested"), 0);
}

#[test]
fn crash_before_commit_reads_the_file_again_with_1_effect() {
    let mut w = world();
    let path = w.put(ID);
    let (crashing, calls) = counting(Some(1));
    w.handlers = crashing;
    w.calls = calls;

    let crashed = catch_unwind(AssertUnwindSafe(|| w.scan(&mut Cursor::new(STAMP))));

    assert!(crashed.is_err(), "the handler crashed fleetd");
    assert!(path.is_file(), "the file stays until its row is committed");
    assert!(!w.archived(ID).exists());
    assert_eq!(
        w.count("probe"),
        0,
        "the rows of the crashed apply rolled back"
    );
    assert_ne!(w.row(ID).map(|r| r.0), Some("applied".to_string()));

    let read = facts(w.scan(&mut Cursor::new(STAMP)));

    assert_eq!(
        read,
        vec![FileFact {
            name: format!("{ID}.json"),
            handled: Handled::Archived(Outcome::Applied(json!({ "calls": 2 }))),
        }]
    );
    assert_eq!(w.count("probe"), 1, "1 effect");
    assert_eq!(w.row(ID).unwrap().0, "applied");
    assert!(!path.exists());
    assert!(w.archived(ID).is_file());
}

#[test]
fn crash_after_commit_only_archives() {
    let mut w = world();
    let path = w.put(ID);
    // fleetd commits the row of the file, then crashes before the archive.
    let decision = Decision::Authorized {
        auth: "token; request file".into(),
    };
    let committed = handle(
        &mut w.conn,
        Generation(1),
        &request(ID, Some(w.token.text())),
        Some("me"),
        &decision,
        &w.handlers,
    )
    .unwrap();
    assert_eq!(committed.outcome, Outcome::Applied(json!({ "calls": 1 })));

    let read = facts(w.scan(&mut Cursor::new(STAMP)));

    assert_eq!(
        read,
        vec![FileFact {
            name: format!("{ID}.json"),
            handled: Handled::Archived(Outcome::Applied(json!({ "calls": 1 }))),
        }]
    );
    assert_eq!(w.calls.load(Ordering::SeqCst), 1, "no second effect");
    assert_eq!(w.count("probe"), 1);
    assert_eq!(w.count("requests"), 1);
    assert!(!path.exists());
    assert!(w.archived(ID).is_file());
}

#[test]
fn folder_with_mode_0755_is_refused_and_opens_1_needs_you_item() {
    let mut w = world();
    let path = w.put(ID);
    fs::set_permissions(w.dir(), Permissions::from_mode(0o755)).unwrap();

    for _ in 0..2 {
        match w.scan(&mut Cursor::new(STAMP)) {
            ScanOutcome::FolderRefused { reason } => {
                assert!(reason.contains("mode 0755 is wider than 0700"), "{reason}")
            }
            other => panic!("expected a refused folder, got {other:?}"),
        }
    }

    let open: Vec<(String, String)> = {
        let mut stmt = w
            .conn
            .prepare("SELECT kind, text FROM needs_you WHERE state = 'open'")
            .unwrap();
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    };
    assert_eq!(open.len(), 1, "{open:?}");
    assert_eq!(open[0].0, FOLDER_REFUSED);
    assert!(open[0].1.contains("mode 0755"), "{}", open[0].1);
    assert!(path.is_file(), "no file of the folder is read");
    assert_eq!(w.count("requests"), 0);
    assert_eq!(w.count("ingested"), 0);
    assert_eq!(
        fs::metadata(w.dir()).unwrap().permissions().mode() & 0o777,
        0o755
    );

    // You narrow the mode: the scan reads the folder and ends the item.
    fs::set_permissions(w.dir(), Permissions::from_mode(0o700)).unwrap();
    assert_eq!(facts(w.scan(&mut Cursor::new(STAMP))).len(), 1);
    let still_open: i64 = w
        .conn
        .query_row(
            "SELECT count(*) FROM needs_you WHERE state = 'open'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(still_open, 0);
}

#[test]
fn missing_folder_is_made_with_mode_0700() {
    let mut w = world();
    assert!(!w.dir().exists());

    assert_eq!(facts(w.scan(&mut Cursor::new(STAMP))), Vec::new());

    let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&w.dir()), 0o700);
    assert_eq!(mode(&w.dir().join("archive")), 0o700);
}

#[test]
fn scan_continues_at_its_cursor() {
    let mut w = world();
    let ids = names(3);
    for id in &ids {
        w.put(id);
    }
    // A test clock: each read is 1 s later, and the deadline is 2 s after
    // the start, so 1 scan reads 2 files.
    let scan_2 = |w: &mut World, cursor: &mut Cursor| {
        let start = Instant::now();
        let mut ticks = 0u64;
        let mut now = move || {
            ticks += 1;
            start + Duration::from_secs(ticks - 1)
        };
        let mut clock = ScanClock {
            deadline: Deadline::pass(start, 2),
            now: &mut now,
            stamp: STAMP,
        };
        let owner = w.owner.clone();
        let out = w.scan_with(&owner, cursor, &mut clock);
        match out {
            ScanOutcome::Read(scan) => scan,
            other => panic!("expected a scan of the folder, got {other:?}"),
        }
    };

    let mut cursor = cursors_store::load(&w.conn, ScanKind::Requests.as_str(), STAMP).unwrap();
    let first = scan_2(&mut w, &mut cursor);
    assert!(first.stopped_at_budget);
    let tx = w.conn.transaction().unwrap();
    cursors_store::save(&tx, ScanKind::Requests.as_str(), &cursor).unwrap();
    tx.commit().unwrap();

    // A new pass loads the cursor and goes on after the last file read.
    let mut cursor = cursors_store::load(&w.conn, ScanKind::Requests.as_str(), STAMP).unwrap();
    let second = scan_2(&mut w, &mut cursor);

    let read: Vec<String> = first
        .facts
        .iter()
        .chain(&second.facts)
        .map(|f| f.name.clone())
        .collect();
    let want: Vec<String> = ids.iter().map(|id| format!("{id}.json")).collect();
    assert_eq!(read, want);
    assert!(!second.stopped_at_budget);
    assert_eq!(w.count("probe"), 3);
}
