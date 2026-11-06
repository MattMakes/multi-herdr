//! The wake outbox (trace-fixes C3, D7). A wake is a row first and a send
//! second: [`write`] adds the row inside a ledger transaction, and [`drain`]
//! gives each undelivered row to a sender and sets `delivered_at` only when
//! the send took it. A failed send leaves the row for the next drain.

use std::fs::{OpenOptions, TryLockError};

use anyhow::{Context, Result};
use rusqlite::{params, Connection, Transaction, TransactionBehavior};
use sha2::{Digest, Sha256};

use super::model::OutboxEvent;
use super::store::Store;
use super::util;

/// 1 wake to write: what happened, what it is about, and its line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Wake {
    pub event: OutboxEvent,
    /// The task, worker or other thing the wake is about.
    pub subject: String,
    /// The board state the wake answers ([`board`]), hashed into the key.
    /// The same subject in a new board state is a new wake.
    pub board: Option<String>,
    /// The line for the orchestrator.
    pub text: String,
}

impl Wake {
    pub fn new(event: OutboxEvent, subject: impl Into<String>, text: impl Into<String>) -> Self {
        Wake {
            event,
            subject: subject.into(),
            board: None,
            text: text.into(),
        }
    }

    /// The same wake, for the board state `board`.
    pub fn on_board(self, board: String) -> Self {
        Wake {
            board: Some(board),
            ..self
        }
    }
}

/// The dedup key of a wake: `<run>:<event>:<subject>[:<board-hash>]`.
pub fn key(run_id: &str, wake: &Wake) -> String {
    let base = format!("{run_id}:{}:{}", wake.event.as_str(), wake.subject);
    match &wake.board {
        Some(board) => format!("{base}:{}", short_hash(board)),
        None => base,
    }
}

/// The first 16 hex characters of the sha256 of `text`.
fn short_hash(text: &str) -> String {
    let digest = Sha256::digest(text.as_bytes());
    digest.iter().take(8).map(|b| format!("{b:02x}")).collect()
}

/// Write `wake` in `tx`, the transaction of the ledger change that causes
/// it. Do not send inside the transaction: [`drain`] sends after the commit.
/// `Some(id)`: a new row. `None`: a row with the same key exists already.
pub fn write(tx: &Transaction, run_id: &str, wake: &Wake) -> Result<Option<String>> {
    let id = util::new_id();
    let n = tx.execute(
        "INSERT INTO outbox (id, run_id, event, key, text, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6) ON CONFLICT(key) DO NOTHING",
        params![
            id,
            run_id,
            wake.event.as_str(),
            key(run_id, wake),
            wake.text,
            util::now()
        ],
    )?;
    Ok((n == 1).then_some(id))
}

/// What [`write_now`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Written {
    /// No wake applies to the board.
    Nothing,
    /// The wake applies, and a row with its key exists already.
    Exists,
    /// A new row with this id.
    New(String),
}

/// Decide and write 1 wake in 1 immediate transaction of its own: `decide`
/// reads the board and returns the wake, or `None`. A caller with only a
/// `&Store` calls it right after its ledger commit and before any send.
/// A crash between that commit and this transaction loses the wake.
pub fn write_now(
    store: &Store,
    run_id: &str,
    decide: impl FnOnce(&Transaction) -> Result<Option<Wake>>,
) -> Result<Written> {
    let tx = Transaction::new_unchecked(store.conn(), TransactionBehavior::Immediate)?;
    let Some(wake) = decide(&tx)? else {
        return Ok(Written::Nothing);
    };
    let written = match write(&tx, run_id, &wake)? {
        Some(id) => Written::New(id),
        None => Written::Exists,
    };
    tx.commit()?;
    Ok(written)
}

/// 1 outbox row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub id: String,
    pub run_id: String,
    pub event: String,
    pub key: String,
    pub text: String,
    pub created_at: String,
    pub delivered_at: Option<String>,
}

/// Which undelivered rows a drain takes.
#[derive(Debug, Clone, Copy)]
pub enum Scope<'a> {
    /// The rows of 1 run.
    Run(&'a str),
    /// The rows of the runs that belong to 1 herdr workspace.
    Workspace(&'a str),
}

/// The undelivered rows of `scope`, oldest first.
pub fn undelivered(conn: &Connection, scope: Scope) -> Result<Vec<Row>> {
    let (filter, value) = match scope {
        Scope::Run(run) => ("o.run_id = ?1", run),
        Scope::Workspace(ws) => ("r.workspace_id = ?1", ws),
    };
    let sql = format!(
        "SELECT o.id, o.run_id, o.event, o.key, o.text, o.created_at, o.delivered_at
           FROM outbox o JOIN runs r ON r.run_id = o.run_id
          WHERE o.delivered_at IS NULL AND {filter}
          ORDER BY o.created_at, o.rowid"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map([value], |r| {
            Ok(Row {
                id: r.get(0)?,
                run_id: r.get(1)?,
                event: r.get(2)?,
                key: r.get(3)?,
                text: r.get(4)?,
                created_at: r.get(5)?,
                delivered_at: r.get(6)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// The text of the newest row of `run` whose key starts with `prefix`.
pub fn last_text(conn: &Connection, run_id: &str, prefix: &str) -> Result<Option<String>> {
    let text = conn.query_row(
        "SELECT text FROM outbox WHERE run_id = ?1 AND substr(key, 1, length(?2)) = ?2
          ORDER BY created_at DESC, rowid DESC LIMIT 1",
        params![run_id, prefix],
        |r| r.get(0),
    );
    super::store::optional(text)
}

/// The board state of a run as 1 line: every task with its status, hold and
/// attempts, and every worktree with its status. With `workers`, every
/// worker with its status too. 2 equal lines are 1 board state.
pub fn board(conn: &Connection, run_id: &str, workers: bool) -> Result<String> {
    let tasks: String = conn.query_row(
        "SELECT coalesce(group_concat(x, ','), '') FROM (
           SELECT task_id || '=' || status || '/' || coalesce(held_reason, '') || '/' || attempts AS x
             FROM tasks WHERE run_id = ?1 ORDER BY task_id)",
        [run_id],
        |r| r.get(0),
    )?;
    let worktrees: String = conn.query_row(
        "SELECT coalesce(group_concat(x, ','), '') FROM (
           SELECT w.worktree_id || '=' || w.status AS x
             FROM worktrees w JOIN tasks t ON t.task_id = w.task_id
            WHERE t.run_id = ?1 ORDER BY w.worktree_id)",
        [run_id],
        |r| r.get(0),
    )?;
    let mut line = format!("tasks {tasks}; worktrees {worktrees}");
    if workers {
        let live: String = conn.query_row(
            "SELECT coalesce(group_concat(x, ','), '') FROM (
               SELECT worker_id || '=' || status AS x
                 FROM workers WHERE run_id = ?1 ORDER BY worker_id)",
            [run_id],
            |r| r.get(0),
        )?;
        line.push_str(&format!("; workers {live}"));
    }
    Ok(line)
}

/// Set `delivered_at` of row `id`. `false`: another drain set it first.
pub fn mark_delivered(conn: &Connection, id: &str) -> Result<bool> {
    let n = conn.execute(
        "UPDATE outbox SET delivered_at = ?2 WHERE id = ?1 AND delivered_at IS NULL",
        params![id, util::now()],
    )?;
    Ok(n == 1)
}

/// What 1 drain did.
#[derive(Debug, Default)]
pub struct Drained {
    /// The ids of the rows that the sender took.
    pub delivered: Vec<String>,
    /// The ids of the rows that the sender refused, with the error.
    pub failed: Vec<(String, String)>,
    /// Another process drained the outbox at the same time, and this drain
    /// sent nothing. That process sends the rows.
    pub busy: bool,
}

/// Give each undelivered row of `scope` to `send`, oldest first. A row that
/// `send` takes gets `delivered_at`; a row that it refuses stays. 1 drain
/// runs at a time per ledger file: 2 drains would send the same row twice.
pub fn drain(
    store: &Store,
    scope: Scope,
    mut send: impl FnMut(&Row) -> Result<()>,
) -> Result<Drained> {
    let lock_path = {
        let mut p = store.path().as_os_str().to_owned();
        p.push(".outbox.lock");
        std::path::PathBuf::from(p)
    };
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)
        .with_context(|| format!("opening {}", lock_path.display()))?;
    let _held = match lock.try_lock() {
        Ok(()) => Held(lock),
        Err(TryLockError::WouldBlock) => {
            return Ok(Drained {
                busy: true,
                ..Default::default()
            })
        }
        Err(TryLockError::Error(e)) => {
            return Err(e).with_context(|| format!("locking {}", lock_path.display()))
        }
    };
    let mut out = Drained::default();
    for row in undelivered(store.conn(), scope)? {
        match send(&row) {
            Ok(()) => {
                mark_delivered(store.conn(), &row.id)?;
                out.delivered.push(row.id);
            }
            Err(e) => out.failed.push((row.id, format!("{e:#}"))),
        }
    }
    Ok(out)
}

/// The held drain lock. Its drop unlocks explicitly: a flock belongs to the
/// open file description, and a child forked by another thread holds a copy
/// of it until its exec. Only an unlock, not a close, frees it at once.
struct Held(std::fs::File);

impl Drop for Held {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::taskledger::model::Actor;
    use crate::taskledger::runs::{self, NewRun, Run};
    use crate::taskledger::tasks::{self, NewTask};

    fn ledger() -> (tempfile::TempDir, Store, Run) {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("ledger.sqlite3")).unwrap();
        let run = store
            .immediate(|tx| {
                runs::create(
                    tx,
                    Path::new("/repo"),
                    &NewRun {
                        slug: "r1".into(),
                        ..Default::default()
                    },
                )
            })
            .unwrap();
        (dir, store, run)
    }

    fn wake(subject: &str, text: &str) -> Wake {
        Wake::new(OutboxEvent::Startable, subject, text)
    }

    fn count(store: &Store) -> i64 {
        store
            .conn()
            .query_row("SELECT count(*) FROM outbox", [], |r| r.get(0))
            .unwrap()
    }

    #[test]
    fn trf_21_the_wake_row_commits_and_rolls_back_with_its_ledger_change() {
        let (_dir, mut store, run) = ledger();
        // The ledger change fails after the row is written: neither stays.
        let failed: Result<()> = store.immediate(|tx| {
            tasks::add(
                tx,
                &run,
                &NewTask::work("alpha", "t/alpha.md", "dev"),
                Actor::Orchestrator,
            )?;
            write(tx, &run.run_id, &wake("T-1", "NOTE: T-1 can start."))?;
            anyhow::bail!("the ledger change fails")
        });
        assert!(failed.is_err());
        assert_eq!(count(&store), 0, "the wake rolled back with the change");
        assert!(tasks::of_run(store.conn(), &run.run_id).unwrap().is_empty());

        // The same change commits: the task and the row are both there.
        store
            .immediate(|tx| {
                tasks::add(
                    tx,
                    &run,
                    &NewTask::work("alpha", "t/alpha.md", "dev"),
                    Actor::Orchestrator,
                )?;
                write(tx, &run.run_id, &wake("T-1", "NOTE: T-1 can start."))
            })
            .unwrap()
            .expect("a new row");
        assert_eq!(count(&store), 1);
        assert_eq!(tasks::of_run(store.conn(), &run.run_id).unwrap().len(), 1);
    }

    #[test]
    fn trf_21_write_now_decides_and_writes_in_1_transaction() {
        let (_dir, store, run) = ledger();
        let none = write_now(&store, &run.run_id, |_| Ok(None)).unwrap();
        assert_eq!(none, Written::Nothing);
        let first = write_now(&store, &run.run_id, |_| Ok(Some(wake("T-1", "a")))).unwrap();
        assert!(matches!(first, Written::New(_)), "{first:?}");
        let again = write_now(&store, &run.run_id, |_| Ok(Some(wake("T-1", "a")))).unwrap();
        assert_eq!(again, Written::Exists);
        // A decision that fails writes nothing.
        assert!(write_now(&store, &run.run_id, |_| anyhow::bail!("no board")).is_err());
        assert_eq!(count(&store), 1);
    }

    #[test]
    fn trf_21_a_repeated_key_writes_no_new_row() {
        let (_dir, mut store, run) = ledger();
        let first = store
            .immediate(|tx| write(tx, &run.run_id, &wake("T-1", "a")))
            .unwrap();
        let again = store
            .immediate(|tx| write(tx, &run.run_id, &wake("T-1", "b")))
            .unwrap();
        assert!(first.is_some());
        assert_eq!(again, None);
        assert_eq!(count(&store), 1);
        // A new board state is a new key.
        let board = wake("T-1", "c").on_board("tasks T-1=queued".into());
        assert!(store
            .immediate(|tx| write(tx, &run.run_id, &board))
            .unwrap()
            .is_some());
        assert_eq!(
            key(&run.run_id, &wake("T-1", "x")),
            format!("{}:startable:T-1", run.run_id)
        );
        assert!(key(&run.run_id, &board).starts_with(&format!("{}:startable:T-1:", run.run_id)));
    }

    #[test]
    fn trf_21_a_failed_first_send_leaves_the_row_and_a_later_drain_delivers_it() {
        let (_dir, mut store, run) = ledger();
        let id = store
            .immediate(|tx| write(tx, &run.run_id, &wake("T-1", "NOTE: T-1 can start.")))
            .unwrap()
            .unwrap();

        let first = drain(&store, Scope::Run(&run.run_id), |_| {
            anyhow::bail!("no orchestrator pane")
        })
        .unwrap();
        assert!(first.delivered.is_empty());
        assert_eq!(first.failed.len(), 1);
        assert!(first.failed[0].1.contains("no orchestrator pane"));
        let left = undelivered(store.conn(), Scope::Run(&run.run_id)).unwrap();
        assert_eq!(left.len(), 1, "the failed send left the row");
        assert_eq!(left[0].id, id);

        let mut sent = Vec::new();
        let second = drain(&store, Scope::Run(&run.run_id), |row| {
            sent.push(row.text.clone());
            Ok(())
        })
        .unwrap();
        assert_eq!(second.delivered, vec![id.clone()]);
        assert_eq!(sent, vec!["NOTE: T-1 can start.".to_string()]);
        let delivered: Option<String> = store
            .conn()
            .query_row(
                "SELECT delivered_at FROM outbox WHERE id = ?1",
                [&id],
                |r| r.get(0),
            )
            .unwrap();
        assert!(delivered.is_some());

        // A third drain sends nothing again.
        let third = drain(&store, Scope::Run(&run.run_id), |_| {
            panic!("no row is left")
        })
        .unwrap();
        assert!(third.delivered.is_empty() && third.failed.is_empty());
    }

    #[test]
    fn trf_21_a_drain_takes_only_its_workspace_and_waits_for_a_running_drain() {
        let (_dir, mut store, run) = ledger();
        store
            .conn()
            .execute(
                "UPDATE runs SET workspace_id = 'w1' WHERE run_id = ?1",
                [&run.run_id],
            )
            .unwrap();
        store
            .immediate(|tx| write(tx, &run.run_id, &wake("T-1", "a")))
            .unwrap();
        let other = drain(&store, Scope::Workspace("w2"), |_| panic!("not w2")).unwrap();
        assert!(other.delivered.is_empty());

        // A drain that runs while another holds the lock sends nothing.
        let nested = drain(&store, Scope::Workspace("w1"), |_| {
            let inner = drain(&store, Scope::Workspace("w1"), |_| panic!("busy")).unwrap();
            assert!(inner.busy);
            Ok(())
        })
        .unwrap();
        assert_eq!(nested.delivered.len(), 1);
    }

    /// A forked child that lives until [`Child::release`]. It holds a copy of
    /// every descriptor of this process, as a child of a concurrent spawn
    /// does between its fork and its exec.
    struct Child {
        pid: libc::pid_t,
        hold: libc::c_int,
    }

    impl Child {
        fn fork() -> Child {
            let mut fds = [0 as libc::c_int; 2];
            // SAFETY: `fds` is a writable array of 2 descriptors.
            assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0);
            let [read, hold] = fds;
            // SAFETY: the child makes only async-signal-safe calls.
            let pid = unsafe { libc::fork() };
            assert!(pid >= 0, "fork failed");
            if pid == 0 {
                // SAFETY: close, read and _exit are async-signal-safe, and
                // `b` is 1 writable byte. The child blocks until the parent
                // closes `hold`, then exits.
                unsafe {
                    libc::close(hold);
                    let mut b = 0u8;
                    libc::read(read, (&mut b as *mut u8).cast(), 1);
                    libc::_exit(0);
                }
            }
            // SAFETY: `read` is a pipe descriptor that this process owns.
            unsafe { libc::close(read) };
            Child { pid, hold }
        }

        fn release(self) {
            // SAFETY: `hold` is owned here, and `pid` is a child of this process.
            unsafe {
                libc::close(self.hold);
                let mut status = 0;
                libc::waitpid(self.pid, &mut status, 0);
            }
        }
    }

    #[test]
    fn trf_21_a_drain_lock_is_free_while_a_child_process_lives() {
        let (_dir, mut store, run) = ledger();
        store
            .immediate(|tx| write(tx, &run.run_id, &wake("T-1", "a")))
            .unwrap();
        let mut child = None;
        let first = drain(&store, Scope::Run(&run.run_id), |_| {
            child = Some(Child::fork());
            anyhow::bail!("the send fails")
        })
        .unwrap();
        assert_eq!(first.failed.len(), 1);

        // The child still holds a copy of the lock descriptor.
        let second = drain(&store, Scope::Run(&run.run_id), |_| Ok(()));
        child.expect("the send forked").release();
        let second = second.unwrap();
        assert!(!second.busy, "the first drain released its lock");
        assert_eq!(second.delivered.len(), 1);
    }

    #[test]
    fn trf_21_the_board_line_changes_with_the_board() {
        let (_dir, mut store, run) = ledger();
        let empty = board(store.conn(), &run.run_id, true).unwrap();
        let t = store
            .immediate(|tx| {
                tasks::add(
                    tx,
                    &run,
                    &NewTask::work("alpha", "t/alpha.md", "dev"),
                    Actor::Orchestrator,
                )
            })
            .unwrap();
        let queued = board(store.conn(), &run.run_id, true).unwrap();
        assert_ne!(empty, queued);
        assert_eq!(queued, board(store.conn(), &run.run_id, true).unwrap());
        store
            .conn()
            .execute(
                "UPDATE tasks SET status = 'done' WHERE task_id = ?1",
                [&t.task_id],
            )
            .unwrap();
        assert_ne!(queued, board(store.conn(), &run.run_id, true).unwrap());
    }
}
