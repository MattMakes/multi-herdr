# The ledger schema migrates forward in place from a frozen version 1

`schema::migrate` brings a ledger file to `schema::VERSION` with 1 migration
per version, starting from version 1. Version 1 runs as a migration on
existing files, so its SQL text never changes: `V1_WORKTREE_STATUS` and
`V1_STEP_NAME` keep the words version 1 wrote, and later versions add new
words. The test `v1_text_is_pinned` pins the length and FNV-1a hash of that
text. A file from a newer binary is refused: this binary would not know its
columns.

## Consequences

- `worktrees.status` keeps its version 1 words. Version 5 adds the later
  ones. `task_steps.name` keeps its version 1 words. Version 8 adds the
  finish step `comment-sweep`.
- SQLite cannot change a `CHECK`. A migration that adds a status word builds
  the table again under its own name, copies its rows, drops the old table
  and renames. Version 5 (worktree triage, #308: the statuses `archived` and
  `trashed`, and the columns of a decision) does this for `worktrees`.
  Version 8 does it for `task_steps`, which no table points at.
- Foreign keys are off while `migrate` runs: the rows of `worktree_modes`
  point at `worktrees`, which version 5 drops and builds again. The keys are
  checked before the commit and turned back on after it. `PRAGMA
  foreign_keys` is a no-op inside a transaction, so it is set before one.
- `schema::migrate_unchecked` takes the write lock (`BEGIN IMMEDIATE`)
  before it reads `PRAGMA user_version`: 2 processes that open a fresh file
  at once must not both create the tables.
- Before a migration, `schema::backup` copies the ledger at version `from` to
  `<dir>/ledger.v<from>-backup-<UTC yyyymmddTHHMMSSZ>.sqlite3` and keeps the
  newest `BACKUPS_KEPT` (3). The caller holds the write lock, so the copy is
  the state the migration starts from. `VACUUM INTO` cannot run inside a
  transaction, so the copy runs on a second connection.
