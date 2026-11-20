//! Opening the task ledger: 1 SQLite file per machine (ADR 0001).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::{Context, Result};
use rusqlite::{Connection, OpenFlags, OptionalExtension, Transaction, TransactionBehavior};

use super::schema;

/// How long a writer waits for another writer's lock before it fails.
/// A claim or a checkin holds the lock for milliseconds; 30 s is room for
/// many workers at once on a slow disk.
pub const BUSY_TIMEOUT: Duration = Duration::from_secs(30);

/// A live opener keeps its initialization lock for at most this long before
/// another machine may break it. Dead local processes are detected at once.
const OPEN_LOCK_STALE_AFTER: Duration = Duration::from_secs(5 * 60);

/// The cross-process lock used only while a ledger switches to WAL and runs
/// its first migration. Ledgers already in WAL mode do not take this lock.
const OPEN_LOCK_NAME: &str = "task-ledger-open";

/// The ledger file under a home directory: `<home>/.horch/ledger.sqlite3`.
pub fn default_path(home: &Path) -> PathBuf {
    home.join(".horch").join("ledger.sqlite3")
}

/// The opt-in that lets a development build migrate the shared ledger.
pub const ALLOW_MIGRATE_ENV: &str = "HORCH_ALLOW_LEDGER_MIGRATE";

/// True when `exe` is a cargo build output: a `target` component followed
/// by `debug` or `release`, or by 1 more component and then `debug`,
/// `release` or `deps` (`target/<triple>/debug`, `target/<profile>/deps`).
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

/// True when `ledger` is the operator's shared ledger,
/// `<passwd_home>/.horch/ledger.sqlite3`.
pub fn is_shared_ledger(ledger: &Path, passwd_home: &Path) -> bool {
    ledger == default_path(passwd_home)
}

/// Why this horch must not migrate `ledger` from version `from`, or `None`
/// when it may. A development build refuses the shared ledger unless
/// `allow` (the value of [`ALLOW_MIGRATE_ENV`]) is `1`.
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

/// Whether a development build `exe` must keep off `ledger`: it is the
/// shared ledger of `passwd_home`, and `allow` (the value of
/// [`ALLOW_MIGRATE_ENV`]) is not `1`.
fn refused(exe: &Path, ledger: &Path, passwd_home: Option<&Path>, allow: Option<&str>) -> bool {
    let shared = passwd_home.is_some_and(|home| is_shared_ledger(ledger, home));
    shared && is_dev_build(exe) && allow != Some("1")
}

/// Why this horch must not open `ledger` for a write, or `None` when it
/// may. A development build refuses each writable open of the shared
/// ledger, also when its schema version is current, unless `allow` (the
/// value of [`ALLOW_MIGRATE_ENV`]) is `1`: its rows name live runs and PIDs.
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

/// The real path of `path`: the canonical path of its nearest ancestor that
/// exists, with the rest of `path` after it. A ledger that does not exist
/// yet still matches through a symlinked home.
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

/// The passwd home of this user, not `$HOME`: a test or a fleet can point
/// `$HOME` anywhere, but the shared ledger stays. Where the OS has no passwd
/// database, no ledger counts as shared.
fn passwd_home() -> Option<PathBuf> {
    horch_platform::path::passwd_home().ok().flatten()
}

/// `passwd_home` when `ledger` is its shared ledger by real path ([`canonical`]),
/// and the path to show for `ledger`. `is_shared_ledger` checks
/// `<home>/.horch/ledger.sqlite3`, so the home is passed on only when the
/// real paths match.
fn shared<'a>(ledger: &Path, passwd_home: Option<&'a Path>) -> (Option<&'a Path>, PathBuf) {
    let home = passwd_home.filter(|h| canonical(&default_path(h)) == canonical(ledger));
    let shown = home.map_or_else(|| ledger.to_path_buf(), default_path);
    (home, shown)
}

/// [`write_refusal`] for a `ledger` given by any path: a symlinked or
/// relative path of the shared ledger is the shared ledger.
fn open_refusal(
    exe: &Path,
    ledger: &Path,
    passwd_home: Option<&Path>,
    allow: Option<&str>,
) -> Option<String> {
    let (home, shown) = shared(ledger, passwd_home);
    write_refusal(exe, &shown, home, allow)
}

/// The executable of this process and the value of [`ALLOW_MIGRATE_ENV`].
fn this_process() -> Option<(PathBuf, Option<String>)> {
    use crate::runtime::{EnvSource, ProcessEnv};
    let exe = ProcessEnv.current_exe()?;
    Some((exe, ProcessEnv.var(ALLOW_MIGRATE_ENV)))
}

/// [`migrate_refusal`] for this process and `ledger`.
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

/// [`write_refusal`] for this process and `ledger`: the first check of
/// [`Store::open`], before it creates a folder or a file.
fn guard_open(ledger: &Path) -> Result<()> {
    let Some((exe, allow)) = this_process() else {
        return Ok(());
    };
    match open_refusal(&exe, ledger, passwd_home().as_deref(), allow.as_deref()) {
        Some(text) => anyhow::bail!(text),
        None => Ok(()),
    }
}

/// The current SQLite journal mode, in lower-case ASCII.
fn journal_mode(conn: &Connection) -> Result<String> {
    conn.query_row("PRAGMA journal_mode", [], |row| row.get(0))
        .context("reading the task ledger journal mode")
}

/// Serialize the WAL switch and first migration across threads and processes.
fn open_lock(path: &Path) -> Result<crate::fsx::DirLockGuard> {
    let dir = path
        .parent()
        .filter(|dir| !dir.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    crate::fsx::DirLock::acquire(dir, OPEN_LOCK_NAME, OPEN_LOCK_STALE_AFTER, BUSY_TIMEOUT)
        .with_context(|| format!("locking task ledger initialization for {}", path.display()))
}

/// The test seam of RR-5: with this variable set to `1`, a writable open of
/// the ledger in a process that made no [`LedgerWriter`] panics. The e2e
/// tests set it for each CLI process.
pub const ASSERT_READ_ONLY_ENV: &str = "HORCH_TEST_ASSERT_READ_ONLY";

/// True after this process made the [`LedgerWriter`] of fleetd.
static FLEETD_WRITER: AtomicBool = AtomicBool::new(false);

/// Panic under the seam ([`ASSERT_READ_ONLY_ENV`]) when this process is not
/// fleetd.
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

/// The right of this process to write the ledger (RR-5, FM-24). From
/// Phase 5 fleetd is the 1 writer: each other process opens the ledger with
/// [`Store::open_read_only`] and sends a request or writes a file.
///
/// Only the composition root of fleetd (`crates/horch/src/cmd/fleetd.rs`)
/// makes it, for `horch fleetd` and for `horch ledger watch --once`, which
/// takes the fleetd lock. The arch rule
/// `fitness_only_fleet_runtime_opens_the_ledger_writable` holds that.
#[derive(Debug)]
pub struct LedgerWriter(());

impl LedgerWriter {
    /// The writer of fleetd. After this call, the seam lets this process
    /// open the ledger for a write.
    pub fn fleetd() -> LedgerWriter {
        FLEETD_WRITER.store(true, Ordering::SeqCst);
        LedgerWriter(())
    }

    /// Open (and create, and migrate) the ledger at `path` for a write.
    pub fn open(&self, path: &Path) -> Result<Store> {
        Store::open(path)
    }
}

/// An open task ledger.
pub struct Store {
    conn: Connection,
    path: PathBuf,
}

impl Store {
    /// Open (and create, and migrate) the ledger at `path`. A development
    /// build does not open the shared ledger for a write
    /// ([`write_refusal`]), and does not migrate it ([`migrate_refusal`]).
    ///
    /// A writable open: production code calls it through [`LedgerWriter`],
    /// and under [`ASSERT_READ_ONLY_ENV`] a process that is not fleetd
    /// panics here, before it touches the file.
    pub fn open(path: &Path) -> Result<Store> {
        Store::open_checked(path, &guard_open)
    }

    /// [`Store::open`] with `check` as its refusal of the file, which runs
    /// before the first change on disk. A test passes its own check.
    fn open_checked(path: &Path, check: &dyn Fn(&Path) -> Result<()>) -> Result<Store> {
        assert_fleetd_writer(path);
        check(path)?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        }
        let mut conn = Connection::open(path)
            .with_context(|| format!("opening the task ledger {}", path.display()))?;
        conn.busy_timeout(BUSY_TIMEOUT)?;
        // Switching journal mode is not covered by SQLite's busy timeout. A
        // first opener holds a cross-process lock until migration completes;
        // later openers see WAL and rely on SQLite's writer lock as usual.
        let _open_lock = if journal_mode(&conn)? == "wal" {
            None
        } else {
            Some(open_lock(path)?)
        };
        if journal_mode(&conn)? != "wal" {
            // WAL: readers never block the 1 writer, and a crash mid-write
            // keeps the last committed state.
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
        })
    }

    /// Open an existing ledger without creating, migrating, or changing its
    /// journal settings. Read-only commands use this in a sandbox.
    pub fn open_read_only(path: &Path) -> Result<Store> {
        let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .with_context(|| format!("opening the task ledger {} read-only", path.display()))?;
        conn.busy_timeout(BUSY_TIMEOUT)?;
        Ok(Store {
            conn,
            path: path.to_path_buf(),
        })
    }

    /// The file this store reads and writes.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The connection, for reads.
    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    /// A second writable store of this writable ledger: the own connection
    /// of the custody of a caller (`finish::with_custody`). A read-only
    /// store gives an error. It opens through [`Store::open`], so the seam
    /// checks the process again (T-885).
    pub fn second_writer(&self) -> Result<Store> {
        anyhow::ensure!(
            !self.conn.is_readonly(rusqlite::MAIN_DB)?,
            "the task ledger {} is open read-only; only fleetd writes it",
            self.path.display()
        );
        let path: &Path = &self.path;
        Store::open(path)
    }

    /// The connection, for a call that takes `&mut Connection`: a custody
    /// call that runs a job between its transactions ([`Store::second_writer`]).
    pub fn conn_mut(&mut self) -> &mut Connection {
        &mut self.conn
    }

    /// Run `f` in a `BEGIN IMMEDIATE` transaction: the write lock is taken
    /// before the first read, so a read-then-write (claim, reap, land) cannot
    /// interleave with another writer. Commits when `f` returns `Ok`. When
    /// `f` fails, the transaction rolls back, and a refused move of the
    /// error is recorded after it
    /// ([`super::util::record_refusal_after_rollback`]).
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

/// 1 optional value of a query.
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
        // Reopening is a no-op migration.
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

    /// The check of [`guard_open`] for the dev build `exe` and the passwd
    /// home `home`, with [`ALLOW_MIGRATE_ENV`] as `allow`.
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
        // The shared ledger exists with the current schema: no migration runs.
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
