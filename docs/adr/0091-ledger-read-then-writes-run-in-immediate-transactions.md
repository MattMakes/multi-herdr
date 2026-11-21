# Every ledger read-then-write runs in a BEGIN IMMEDIATE transaction

The task ledger (`crates/horch-core/src/taskledger/`, docs/specs/task-ledger.md,
#28) is where orchestrators queue tasks, workers claim them, and every finish,
checkin and stop is recorded. It is the 1 SQLite file per machine of ADR 0001
(`store::default_path`, overridden by `HORCH_LEDGER_DB`). Every read-then-write
(claim, reap, land) runs in `store::Store::immediate`, a `BEGIN IMMEDIATE`
transaction that takes the write lock before the first read and commits when
the closure returns `Ok`. So `tasks::claim` reads the board and writes the
claim with no other writer in between.

## Consequences

- `store::BUSY_TIMEOUT` is 30 s: how long a writer waits for another writer's
  lock before it fails. A claim or a checkin holds the lock for milliseconds,
  and 30 s is room for many workers at once on a slow disk.
- The ledger runs in WAL journal mode: readers never block the 1 writer, and a
  crash mid-write keeps the last committed state.
- SQLite's busy timeout does not cover switching the journal mode. While a
  ledger is not yet in WAL mode, `Store::open` takes the cross-process
  directory lock `task-ledger-open` (stale after 5 min) until its first
  migration completes.
  Later openers see WAL and rely on SQLite's writer lock as usual.
- A command that only reads never creates the ledger (#199).
  `taskledger::open_existing` and `open_existing_read_only` return `None`
  when the file does not exist: a ledger that does not exist has no runs.
  `open_existing_read_only` does not create, migrate or change the journal
  mode of the database.
- `store::passwd_home` reads the user's home from the passwd database, not
  `$HOME`: a test or a fleet can point `$HOME` anywhere, but the shared
  ledger stays at the real home. A development build refuses to migrate that
  shared ledger unless `HORCH_ALLOW_LEDGER_MIGRATE=1`.
- `worktree_ops::create` runs `git worktree add` inside an immediate
  transaction, so the SQLite writer lock serializes concurrent creations of
  the same worktree name.
- Ledger time stamps (`util::now`, RFC 3339 UTC in a fixed format) sort as
  strings in time order. `status::Parked::of` and other code compare them as
  text without parsing, so the format must stay fixed-width and UTC.
