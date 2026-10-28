//! Opening the task ledger: 1 SQLite file per machine (ADR 0001).

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior};

use super::schema;

/// How long a writer waits for another writer's lock before it fails.
/// A claim or a checkin holds the lock for milliseconds; 30 s is room for
/// many workers at once on a slow disk.
pub const BUSY_TIMEOUT: Duration = Duration::from_secs(30);

/// The ledger file under a home directory: `<home>/.horch/ledger.sqlite3`.
pub fn default_path(home: &Path) -> PathBuf {
    home.join(".horch").join("ledger.sqlite3")
}

/// An open task ledger.
pub struct Store {
    conn: Connection,
    path: PathBuf,
}

impl Store {
    /// Open (and create, and migrate) the ledger at `path`.
    pub fn open(path: &Path) -> Result<Store> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        }
        let mut conn = Connection::open(path)
            .with_context(|| format!("opening the task ledger {}", path.display()))?;
        conn.busy_timeout(BUSY_TIMEOUT)?;
        // WAL: readers never block the 1 writer, and a crash mid-write keeps
        // the last committed state.
        let _: String = conn.query_row("PRAGMA journal_mode = WAL", [], |r| r.get(0))?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        schema::migrate(&mut conn)?;
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

    /// Run `f` in a `BEGIN IMMEDIATE` transaction: the write lock is taken
    /// before the first read, so a read-then-write (claim, reap, land) cannot
    /// interleave with another writer. Commits when `f` returns `Ok`.
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
