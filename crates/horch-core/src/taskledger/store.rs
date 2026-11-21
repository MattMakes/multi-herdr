use std::path::{Path, PathBuf};
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
    let shared = passwd_home.is_some_and(|home| is_shared_ledger(ledger, home));
    if !shared || !is_dev_build(exe) || allow == Some("1") {
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

#[cfg(unix)]
pub fn passwd_home() -> Option<PathBuf> {
    use std::ffi::{CStr, OsStr};
    use std::os::unix::ffi::OsStrExt;
    let mut buf = vec![0 as libc::c_char; 16 * 1024];
    // SAFETY: `passwd` is plain data; all-zero bytes are a valid value.
    let mut pw: libc::passwd = unsafe { std::mem::zeroed() };
    let mut out: *mut libc::passwd = std::ptr::null_mut();
    // SAFETY: every pointer is valid for the call, and `buf.len()` is the
    // size of `buf`. The strings in `pw` point into `buf`, which outlives them.
    let rc = unsafe {
        libc::getpwuid_r(
            libc::getuid(),
            &mut pw,
            buf.as_mut_ptr(),
            buf.len(),
            &mut out,
        )
    };
    if rc != 0 || out.is_null() || pw.pw_dir.is_null() {
        return None;
    }
    // SAFETY: getpwuid_r succeeded, so `pw_dir` is a NUL-terminated string in `buf`.
    let dir = unsafe { CStr::from_ptr(pw.pw_dir) };
    Some(PathBuf::from(OsStr::from_bytes(dir.to_bytes())))
}

#[cfg(not(unix))]
pub fn passwd_home() -> Option<PathBuf> {
    None
}

fn guard(ledger: &Path, from: i64) -> Result<()> {
    use crate::runtime::{EnvSource, ProcessEnv};
    let Some(exe) = ProcessEnv.current_exe() else {
        return Ok(());
    };
    let real = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let home = passwd_home();
    let shared = home.as_deref().map(|h| real(&default_path(h)));
    let ledger = real(ledger);
    let allow = ProcessEnv.var(ALLOW_MIGRATE_ENV);
    let home = home.filter(|_| shared.as_deref() == Some(ledger.as_path()));
    let shown = home.as_deref().map(default_path).unwrap_or(ledger);
    match migrate_refusal(&exe, &shown, home.as_deref(), allow.as_deref(), from) {
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

pub struct Store {
    conn: Connection,
    path: PathBuf,
}

impl Store {
    pub fn open(path: &Path) -> Result<Store> {
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
        })
    }

    pub fn open_read_only(path: &Path) -> Result<Store> {
        let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .with_context(|| format!("opening the task ledger {} read-only", path.display()))?;
        conn.busy_timeout(BUSY_TIMEOUT)?;
        Ok(Store {
            conn,
            path: path.to_path_buf(),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    pub fn immediate<T>(&mut self, f: impl FnOnce(&Transaction) -> Result<T>) -> Result<T> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .context("taking the task ledger's write lock")?;
        let out = f(&tx)?;
        tx.commit().context("committing to the task ledger")?;
        Ok(out)
    }
}

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
                 VALUES ('t1', 'r', 1, 'x', 'queued', 'now', 'now', 'opus', 'opus')",
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
                 VALUES ('t1', 'r', 1, 'x', 'queued', 'now', 'now', 'import')",
                [],
            )?;
            Ok(())
        })
        .unwrap();
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
