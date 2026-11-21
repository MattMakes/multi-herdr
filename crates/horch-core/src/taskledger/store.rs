use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result};
use rusqlite::{Connection, OpenFlags, OptionalExtension, Transaction, TransactionBehavior};

use super::schema;

pub const BUSY_TIMEOUT: Duration = Duration::from_secs(30);

const OPEN_LOCK_STALE_AFTER: Duration = Duration::from_secs(5 * 60);

const OPEN_LOCK_NAME: &str = "task-ledger-open";

pub fn default_path(home: &Path) -> PathBuf {
    home.join(".horch").join("ledger.sqlite3")
}

pub const ALLOW_MIGRATE_ENV: &str = "HORCH_ALLOW_LEDGER_MIGRATE";

pub fn is_dev_build(exe: &Path) -> bool {
    let parts: Vec<&std::ffi::OsStr> = exe
        .parent()
        .map(|dir| dir.iter().collect())
        .unwrap_or_default();
    let is = |i: usize, words: &[&str]| {
        parts
            .get(i)
            .is_some_and(|p| words.iter().any(|w| p == &std::ffi::OsStr::new(w)))
    };
    (0..parts.len()).any(|i| {
        parts[i] == "target"
            && (is(i + 1, &["debug", "release"]) || is(i + 2, &["debug", "release", "deps"]))
    })
}

pub fn is_shared_ledger(ledger: &Path, passwd_home: &Path) -> bool {
    ledger == default_path(passwd_home)
}

pub fn migrate_refusal(
    exe: &Path,
    ledger: &Path,
    passwd_home: Option<&Path>,
    allow: Option<&str>,
    from: i64,
) -> Option<String> {
    if !refused(exe, ledger, passwd_home, allow) {
        return None;
    }
    Some(format!(
        "the development build {exe} refuses to migrate the shared task ledger {ledger} \
         from schema version {from} to {to}, because the installed horch would then refuse it.\n\
         Set HORCH_LEDGER_DB=<scratch file> to use a scratch ledger, \
         or set {ALLOW_MIGRATE_ENV}=1 to migrate the shared ledger.",
        exe = exe.display(),
        ledger = ledger.display(),
        to = schema::VERSION,
    ))
}

fn refused(exe: &Path, ledger: &Path, passwd_home: Option<&Path>, allow: Option<&str>) -> bool {
    let shared = passwd_home.is_some_and(|home| is_shared_ledger(ledger, home));
    shared && is_dev_build(exe) && allow != Some("1")
}

pub fn write_refusal(
    exe: &Path,
    ledger: &Path,
    passwd_home: Option<&Path>,
    allow: Option<&str>,
) -> Option<String> {
    if !refused(exe, ledger, passwd_home, allow) {
        return None;
    }
    Some(format!(
        "the development build {exe} refuses to open the shared task ledger {ledger} for a write, \
         because its rows name the live runs and processes of the installed horch.\n\
         Set HORCH_LEDGER_DB=<scratch file> to use a scratch ledger, \
         or set {ALLOW_MIGRATE_ENV}=1 to write the shared ledger.",
        exe = exe.display(),
        ledger = ledger.display(),
    ))
}

pub fn canonical(path: &Path) -> PathBuf {
    let mut rest = Vec::new();
    let mut at = path;
    loop {
        if let Ok(real) = std::fs::canonicalize(at) {
            return rest.iter().rev().fold(real, |p, name| p.join(name));
        }
        match (at.parent(), at.file_name()) {
            (Some(parent), Some(name)) => {
                rest.push(name.to_os_string());
                at = if parent.as_os_str().is_empty() {
                    Path::new(".")
                } else {
                    parent
                };
            }
            _ => return path.to_path_buf(),
        }
    }
}

fn passwd_home() -> Option<PathBuf> {
    horch_platform::path::passwd_home().ok().flatten()
}

fn shared<'a>(ledger: &Path, passwd_home: Option<&'a Path>) -> (Option<&'a Path>, PathBuf) {
    let home = passwd_home.filter(|h| canonical(&default_path(h)) == canonical(ledger));
    let shown = home.map_or_else(|| ledger.to_path_buf(), default_path);
    (home, shown)
}

fn open_refusal(
    exe: &Path,
    ledger: &Path,
    passwd_home: Option<&Path>,
    allow: Option<&str>,
) -> Option<String> {
    let (home, shown) = shared(ledger, passwd_home);
    write_refusal(exe, &shown, home, allow)
}

fn this_process() -> Option<(PathBuf, Option<String>)> {
    use crate::runtime::{EnvSource, ProcessEnv};
    let exe = ProcessEnv.current_exe()?;
    Some((exe, ProcessEnv.var(ALLOW_MIGRATE_ENV)))
}

fn guard(ledger: &Path, from: i64) -> Result<()> {
    let Some((exe, allow)) = this_process() else {
        return Ok(());
    };
    let home = passwd_home();
    let (home, shown) = shared(ledger, home.as_deref());
    match migrate_refusal(&exe, &shown, home, allow.as_deref(), from) {
        Some(text) => anyhow::bail!(text),
        None => Ok(()),
    }
}

fn guard_open(ledger: &Path) -> Result<()> {
    let Some((exe, allow)) = this_process() else {
        return Ok(());
    };
    match open_refusal(&exe, ledger, passwd_home().as_deref(), allow.as_deref()) {
        Some(text) => anyhow::bail!(text),
        None => Ok(()),
    }
}

fn journal_mode(conn: &Connection) -> Result<String> {
    conn.query_row("PRAGMA journal_mode", [], |row| row.get(0))
        .context("reading the task ledger journal mode")
}

fn open_lock(path: &Path) -> Result<crate::fsx::DirLockGuard> {
    let dir = path
        .parent()
        .filter(|dir| !dir.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    crate::fsx::DirLock::acquire(dir, OPEN_LOCK_NAME, OPEN_LOCK_STALE_AFTER, BUSY_TIMEOUT)
        .with_context(|| format!("locking task ledger initialization for {}", path.display()))
}

pub const ASSERT_READ_ONLY_ENV: &str = "HORCH_TEST_ASSERT_READ_ONLY";

static FLEETD_WRITER: AtomicBool = AtomicBool::new(false);

fn assert_fleetd_writer(path: &Path) {
    use crate::runtime::{EnvSource, ProcessEnv};
    if FLEETD_WRITER.load(Ordering::SeqCst) {
        return;
    }
    if ProcessEnv.var(ASSERT_READ_ONLY_ENV).as_deref() == Some("1") {
        panic!(
            "{ASSERT_READ_ONLY_ENV}=1: a process other than fleetd opens the task ledger {} for a write",
            path.display()
        );
    }
}

#[derive(Debug)]
pub struct LedgerWriter(());

impl LedgerWriter {
    pub fn fleetd() -> LedgerWriter {
        FLEETD_WRITER.store(true, Ordering::SeqCst);
        LedgerWriter(())
    }

    pub fn open(&self, path: &Path) -> Result<Store> {
        Store::open(path)
    }
}

#[derive(Clone, Debug)]
pub struct ReadOnlyRecord(Arc<ReadOnlyEntry>);

#[derive(Debug)]
struct ReadOnlyEntry {
    path: PathBuf,
    last: Mutex<String>,
}

fn record_statements(conn: &Connection, path: &Path) -> ReadOnlyRecord {
    unsafe extern "C" fn on_statement(
        event: std::os::raw::c_uint,
        ctx: *mut std::os::raw::c_void,
        _stmt: *mut std::os::raw::c_void,
        sql: *mut std::os::raw::c_void,
    ) -> std::os::raw::c_int {
        if event == rusqlite::ffi::SQLITE_TRACE_STMT as std::os::raw::c_uint && !sql.is_null() {
            // SAFETY: `ctx` is the `ReadOnlyEntry` of the `Store` that owns
            // this connection. The `Store` declares `conn` before `record`,
            // so the connection closes, and SQLite stops calling this
            // callback, before the `Store` drops its `Arc` of the entry.
            let entry = unsafe { &*ctx.cast::<ReadOnlyEntry>() };
            // SAFETY: for SQLITE_TRACE_STMT, SQLite passes the SQL text as
            // a NUL-terminated string that lives for this call.
            let text = unsafe { std::ffi::CStr::from_ptr(sql.cast()) };
            if let Ok(mut last) = entry.last.lock() {
                *last = text.to_string_lossy().into_owned();
            }
        }
        0
    }
    let record = ReadOnlyRecord(Arc::new(ReadOnlyEntry {
        path: path.to_path_buf(),
        last: Mutex::new(String::new()),
    }));
    // SAFETY: the handle is the open connection of `conn`. The context
    // pointer stays valid for each call of the callback: see the SAFETY
    // comment in `on_statement`.
    unsafe {
        rusqlite::ffi::sqlite3_trace_v2(
            conn.handle(),
            rusqlite::ffi::SQLITE_TRACE_STMT as std::os::raw::c_uint,
            Some(on_statement),
            Arc::as_ptr(&record.0).cast_mut().cast(),
        );
    }
    record
}

pub fn read_only_failure(
    command: &str,
    record: Option<&ReadOnlyRecord>,
    err: anyhow::Error,
) -> anyhow::Error {
    let read_only = err.chain().any(|e| {
        e.downcast_ref::<rusqlite::Error>()
            .and_then(rusqlite::Error::sqlite_error)
            .is_some_and(|f| f.extended_code & 0xff == rusqlite::ffi::SQLITE_READONLY)
    });
    if !read_only {
        return err;
    }
    let last = record.map(|r| {
        let sql = r.0.last.lock().map(|l| l.clone()).unwrap_or_default();
        (&r.0.path, sql)
    });
    let text = match last {
        Some((path, sql)) if !sql.is_empty() => format!(
            "`{command}` got SQLITE_READONLY on the read-only task ledger {}; the last statement was: {sql}",
            path.display()
        ),
        Some((path, _)) => format!(
            "`{command}` got SQLITE_READONLY on the read-only task ledger {} before its first statement",
            path.display()
        ),
        None => format!("`{command}` got SQLITE_READONLY with no read-only task ledger open"),
    };
    err.context(text)
}

pub const SQL_TRACE_ENV: &str = "HORCH_TEST_SQL_TRACE";

fn trace_writes(conn: &Connection) {
    #[cfg(debug_assertions)]
    {
        use crate::runtime::{EnvSource, ProcessEnv};
        if let Some(file) = ProcessEnv.var(SQL_TRACE_ENV) {
            trace_to(conn, PathBuf::from(file));
        }
    }
    #[cfg(not(debug_assertions))]
    let _ = conn;
}

#[cfg(debug_assertions)]
fn trace_to(conn: &Connection, file: PathBuf) {
    use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
    conn.authorizer(Some(move |ctx: AuthContext<'_>| {
        let read = matches!(
            ctx.action,
            AuthAction::Read { .. } | AuthAction::Select | AuthAction::Function { .. }
        ) || matches!(
            ctx.action,
            AuthAction::Savepoint { savepoint_name, .. } if savepoint_name == SNAPSHOT
        );
        if !read {
            use std::io::Write;
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&file)
            {
                let _ = writeln!(f, "{:?}", ctx.action);
            }
        }
        Authorization::Allow
    }));
}

pub struct Store {
    conn: Connection,
    path: PathBuf,
    record: Option<ReadOnlyRecord>,
}

impl Store {
    pub fn open(path: &Path) -> Result<Store> {
        Store::open_checked(path, &guard_open)
    }

    fn open_checked(path: &Path, check: &dyn Fn(&Path) -> Result<()>) -> Result<Store> {
        assert_fleetd_writer(path);
        check(path)?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        }
        let mut conn = Connection::open(path)
            .with_context(|| format!("opening the task ledger {}", path.display()))?;
        conn.busy_timeout(BUSY_TIMEOUT)?;
        let _open_lock = if journal_mode(&conn)? == "wal" {
            None
        } else {
            Some(open_lock(path)?)
        };
        if journal_mode(&conn)? != "wal" {
            let _: String = conn
                .query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))
                .context("setting the task ledger journal mode to WAL")?;
        }
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        schema::migrate_guarded(&mut conn, &|from| guard(path, from))
            .context("migrating the task ledger schema")?;
        Ok(Store {
            conn,
            path: path.to_path_buf(),
            record: None,
        })
    }

    pub fn open_read_only(path: &Path) -> Result<Store> {
        let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .with_context(|| format!("opening the task ledger {} read-only", path.display()))?;
        conn.busy_timeout(BUSY_TIMEOUT)?;
        let record = record_statements(&conn, path);
        trace_writes(&conn);
        Ok(Store {
            conn,
            path: path.to_path_buf(),
            record: Some(record),
        })
    }

    pub fn read_only_record(&self) -> Option<ReadOnlyRecord> {
        self.record.clone()
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    pub fn second_writer(&self) -> Result<Store> {
        anyhow::ensure!(
            !self.conn.is_readonly(rusqlite::MAIN_DB)?,
            "the task ledger {} is open read-only; only fleetd writes it",
            self.path.display()
        );
        let path: &Path = &self.path;
        Store::open(path)
    }

    pub fn conn_mut(&mut self) -> &mut Connection {
        &mut self.conn
    }

    pub fn immediate<T>(&mut self, f: impl FnOnce(&Transaction) -> Result<T>) -> Result<T> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .context("taking the task ledger's write lock")?;
        match f(&tx) {
            Ok(out) => {
                tx.commit().context("committing to the task ledger")?;
                Ok(out)
            }
            Err(e) => {
                drop(tx);
                super::util::record_refusal_after_rollback(&self.conn, &e);
                Err(e)
            }
        }
    }
}

pub fn one_snapshot<T>(conn: &Connection, f: impl FnOnce() -> Result<T>) -> Result<T> {
    conn.execute_batch(&format!("SAVEPOINT {SNAPSHOT}"))?;
    let out = f();
    let end = if out.is_ok() {
        format!("RELEASE {SNAPSHOT}")
    } else {
        format!("ROLLBACK TO {SNAPSHOT}; RELEASE {SNAPSHOT}")
    };
    conn.execute_batch(&end)?;
    out
}

const SNAPSHOT: &str = "one_snapshot";

pub fn optional<T>(r: rusqlite::Result<T>) -> Result<Option<T>> {
    Ok(r.optional()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn open_creates_the_file_and_the_tables() {
        let dir = temp();
        let path = dir.path().join("sub").join("ledger.sqlite3");
        let store = Store::open(&path).unwrap();
        assert!(path.exists());
        let n: i64 = store
            .conn()
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name IN \
                 ('projects','runs','tasks','task_deps','task_files','workers','checks',\
                  'stops','worktrees','events','task_steps','import_raw','import_findings')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 13);
        let v: i64 = store
            .conn()
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v, schema::VERSION);
        drop(store);
        Store::open(&path).unwrap();
    }

    #[test]
    fn read_only_open_reads_without_accepting_a_write() {
        let dir = temp();
        let path = dir.path().join("ledger.sqlite3");
        drop(Store::open(&path).unwrap());
        let store = Store::open_read_only(&path).unwrap();
        let version: i64 = store
            .conn()
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, schema::VERSION);
        assert!(store.conn().execute("PRAGMA user_version = 0", []).is_err());
    }

    #[cfg(debug_assertions)]
    #[test]
    fn the_trace_records_a_write_of_a_read_only_connection() {
        let dir = temp();
        let path = dir.path().join("ledger.sqlite3");
        drop(Store::open(&path).unwrap());
        let store = Store::open_read_only(&path).unwrap();
        let trace = dir.path().join("trace.txt");
        trace_to(store.conn(), trace.clone());
        let _: i64 = store
            .conn()
            .query_row("SELECT count(*) FROM projects", [], |r| r.get(0))
            .unwrap();
        assert!(!trace.exists(), "a read is not traced");
        let err = store
            .conn()
            .execute("INSERT INTO projects VALUES ('p', '/r', 'now')", [])
            .unwrap_err();
        assert!(err.to_string().contains("readonly"), "{err}");
        let text = std::fs::read_to_string(&trace).unwrap();
        assert!(text.contains("Insert"), "{text}");
    }

    #[cfg(debug_assertions)]
    #[test]
    fn the_trace_counts_only_the_snapshot_savepoint_as_a_read() {
        let dir = temp();
        let path = dir.path().join("ledger.sqlite3");
        drop(Store::open(&path).unwrap());
        let store = Store::open_read_only(&path).unwrap();
        let trace = dir.path().join("trace.txt");
        trace_to(store.conn(), trace.clone());
        let conn = store.conn();
        one_snapshot(conn, || {
            let _: i64 = conn.query_row("SELECT count(*) FROM projects", [], |r| r.get(0))?;
            Ok(())
        })
        .unwrap();
        assert!(one_snapshot(conn, || Err::<(), _>(anyhow::anyhow!("x"))).is_err());
        assert!(!trace.exists(), "a snapshot read is not traced");
        let written = one_snapshot(conn, || {
            Ok(conn.execute("INSERT INTO projects VALUES ('p', '/r', 'now')", [])?)
        });
        assert!(written.is_err(), "the read-only ledger refuses the insert");
        let text = std::fs::read_to_string(&trace).unwrap();
        assert!(text.contains("Insert"), "{text}");
        std::fs::remove_file(&trace).unwrap();
        for name in ["other", "one_snapshot_x"] {
            conn.execute_batch(&format!("SAVEPOINT {name}; RELEASE {name}"))
                .unwrap();
        }
        let text = std::fs::read_to_string(&trace).unwrap();
        assert!(text.contains("\"other\""), "{text}");
        assert!(text.contains("\"one_snapshot_x\""), "{text}");
    }

    #[test]
    fn a_read_only_failure_names_the_command_the_statement_and_the_ledger() {
        let dir = temp();
        let path = dir.path().join("ledger.sqlite3");
        drop(Store::open(&path).unwrap());
        let store = Store::open_read_only(&path).unwrap();
        let err: anyhow::Error = store
            .conn()
            .execute("INSERT INTO projects VALUES ('p', '/r', 'now')", [])
            .unwrap_err()
            .into();
        let text = format!(
            "{:#}",
            read_only_failure(
                "horch ledger status --json",
                store.read_only_record().as_ref(),
                err
            )
        );
        assert!(text.contains("`horch ledger status --json`"), "{text}");
        assert!(
            text.contains("the last statement was: INSERT INTO projects VALUES ('p', '/r', 'now')"),
            "{text}"
        );
        assert!(text.contains(&path.display().to_string()), "{text}");
        assert!(text.contains("Error code 8"), "{text}");
        let other = read_only_failure("horch ledger status", None, anyhow::anyhow!("no run"));
        assert_eq!(format!("{other:#}"), "no run", "another error is unchanged");
    }

    #[test]
    fn a_read_only_failure_names_the_store_that_failed_with_2_stores_open() {
        let dir = temp();
        let first_path = dir.path().join("first.sqlite3");
        let second_path = dir.path().join("second.sqlite3");
        drop(Store::open(&first_path).unwrap());
        drop(Store::open(&second_path).unwrap());
        let first = Store::open_read_only(&first_path).unwrap();
        let second = Store::open_read_only(&second_path).unwrap();
        let err: anyhow::Error = first
            .conn()
            .execute("INSERT INTO projects VALUES ('first', '/r', 'now')", [])
            .unwrap_err()
            .into();
        let _: i64 = second
            .conn()
            .query_row("SELECT count(*) FROM tasks", [], |r| r.get(0))
            .unwrap();
        let text = format!(
            "{:#}",
            read_only_failure(
                "horch ledger status",
                first.read_only_record().as_ref(),
                err
            )
        );
        assert!(
            text.contains(
                "the last statement was: INSERT INTO projects VALUES ('first', '/r', 'now')"
            ),
            "{text}"
        );
        assert!(text.contains(&first_path.display().to_string()), "{text}");
        assert!(!text.contains(&second_path.display().to_string()), "{text}");
    }

    #[test]
    fn a_newer_schema_is_refused() {
        let dir = temp();
        let path = dir.path().join("ledger.sqlite3");
        drop(Store::open(&path).unwrap());
        let conn = Connection::open(&path).unwrap();
        conn.pragma_update(None, "user_version", schema::VERSION + 1)
            .unwrap();
        drop(conn);
        let err = Store::open(&path).err().unwrap();
        assert!(format!("{err:#}").contains("self-update"), "{err:#}");
    }

    #[test]
    fn two_immediate_writers_both_commit() {
        let dir = temp();
        let path = dir.path().join("ledger.sqlite3");
        drop(Store::open(&path).unwrap());
        let handles: Vec<_> = (0..2)
            .map(|i| {
                let path = path.clone();
                std::thread::spawn(move || {
                    let mut s = Store::open(&path).unwrap();
                    s.immediate(|tx| {
                        tx.execute(
                            "INSERT INTO projects (project_id, root, created_at) VALUES (?1, ?2, 'now')",
                            (format!("p{i}"), format!("/r{i}")),
                        )?;
                        std::thread::sleep(Duration::from_millis(50));
                        Ok(())
                    })
                    .unwrap();
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        let s = Store::open(&path).unwrap();
        let n: i64 = s
            .conn()
            .query_row("SELECT count(*) FROM projects", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 2);
    }

    #[test]
    fn native_rows_need_their_fields_and_imported_rows_do_not() {
        let dir = temp();
        let mut s = Store::open(&dir.path().join("l.sqlite3")).unwrap();
        s.immediate(|tx| {
            tx.execute_batch(
                "INSERT INTO projects VALUES ('p', '/r', 'now');
                 INSERT INTO runs (run_id, project_id, slug, status, started_at)
                   VALUES ('r', 'p', 's', 'active', 'now');",
            )?;
            Ok(())
        })
        .unwrap();
        let native = s.immediate(|tx| {
            tx.execute(
                "INSERT INTO tasks (task_id, run_id, seq, title, status, created_at, updated_at,
                   primary_persona, claimable_by)
                 VALUES ('t1', 'r', 1, 'x', 'ready', 'now', 'now', 'opus', 'opus')",
                [],
            )?;
            Ok(())
        });
        assert!(
            native.is_err(),
            "a native task without a ticket file must be refused"
        );
        s.immediate(|tx| {
            tx.execute(
                "INSERT INTO tasks (task_id, run_id, seq, title, status, created_at, updated_at, origin)
                 VALUES ('t1', 'r', 1, 'x', 'ready', 'now', 'now', 'import')",
                [],
            )?;
            Ok(())
        })
        .unwrap();
    }

    fn check_as<'a>(
        exe: &'a Path,
        home: &'a Path,
        allow: Option<&'a str>,
    ) -> impl Fn(&Path) -> Result<()> + 'a {
        move |ledger: &Path| match open_refusal(exe, ledger, Some(home), allow) {
            Some(text) => anyhow::bail!(text),
            None => Ok(()),
        }
    }

    const DEV: &str = "/w/target/debug/horch";

    #[test]
    fn a_dev_build_refuses_a_same_version_write_of_the_shared_ledger() {
        let home = temp();
        let shared = default_path(home.path());
        drop(Store::open(&shared).unwrap());
        let before = std::fs::read(&shared).unwrap();
        let err = Store::open_checked(&shared, &check_as(Path::new(DEV), home.path(), None))
            .err()
            .expect("a dev build refuses the shared ledger");
        let text = format!("{err:#}");
        assert!(
            text.contains("refuses to open the shared task ledger"),
            "{text}"
        );
        assert!(text.contains(ALLOW_MIGRATE_ENV), "{text}");
        assert_eq!(
            std::fs::read(&shared).unwrap(),
            before,
            "the file is unchanged"
        );
        for opt_out in [Some("0"), Some("")] {
            assert!(
                Store::open_checked(&shared, &check_as(Path::new(DEV), home.path(), opt_out))
                    .is_err(),
                "{opt_out:?}"
            );
        }
    }

    #[test]
    fn a_dev_build_refuses_the_shared_ledger_before_it_touches_the_disk() {
        let home = temp();
        let shared = default_path(home.path());
        let err = Store::open_checked(&shared, &check_as(Path::new(DEV), home.path(), None));
        assert!(err.is_err());
        assert!(
            !home.path().join(".horch").exists(),
            "no folder and no file is created"
        );
    }

    #[test]
    fn a_dev_build_opens_a_scratch_ledger_or_the_shared_one_with_the_opt_in() {
        let home = temp();
        let check = check_as(Path::new(DEV), home.path(), None);
        let scratch = home.path().join("scratch").join("ledger.sqlite3");
        Store::open_checked(&scratch, &check).expect("a scratch ledger opens");
        let shared = default_path(home.path());
        let allowed = check_as(Path::new(DEV), home.path(), Some("1"));
        Store::open_checked(&shared, &allowed).expect("the opt-in opens the shared ledger");
        let installed = check_as(Path::new("/usr/local/bin/horch"), home.path(), None);
        Store::open_checked(&shared, &installed).expect("an installed build opens it");
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_home_still_names_the_shared_ledger() {
        let dir = temp();
        let real = dir.path().join("real");
        std::fs::create_dir_all(&real).unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let via_link = default_path(&link);
        assert_eq!(canonical(&via_link), canonical(&default_path(&real)));
        let err = Store::open_checked(&via_link, &check_as(Path::new(DEV), &real, None));
        assert!(err.is_err(), "the link names the shared ledger");
        assert!(!real.join(".horch").exists());
    }

    #[test]
    fn enum_columns_refuse_unknown_words() {
        let dir = temp();
        let mut s = Store::open(&dir.path().join("l.sqlite3")).unwrap();
        let bad = s.immediate(|tx| {
            tx.execute_batch(
                "INSERT INTO projects VALUES ('p', '/r', 'now');
                 INSERT INTO runs (run_id, project_id, slug, status, started_at)
                   VALUES ('r', 'p', 's', 'sleeping', 'now');",
            )?;
            Ok(())
        });
        assert!(bad.is_err());
    }
}
