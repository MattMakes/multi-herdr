# 02: The task ledger store and `horch ledger status`

**What to build:** A SQLite task ledger that every `horch` command can open,
and a first command that reads it. `horch ledger status` on a fresh machine
creates `~/.horch/ledger.sqlite3` (`%USERPROFILE%\.horch\` on Windows,
`$HORCH_LEDGER_DB` overrides) and prints that there are no runs.
`horch ledger status --json` prints the same as JSON.

The store holds every table of spec section 4 and the import columns of
section 11.2 (`origin`, `import_raw_id`, the `import_raw` and
`import_findings` tables), with the `CHECK (origin = 'import' OR ...)` rule
for required fields. It opens in WAL mode with a 5 s busy timeout, gives
callers a `BEGIN IMMEDIATE` transaction helper, and migrates forward by
`PRAGMA user_version`.

The name `horch ledger` moves to the new ledger. The old low-level
subcommands (`add`, `set-session`, `has-session`, `resume`, `assign`,
`note`, `done`, `get`, `list`, `path`) move under `horch ledger legacy`,
unchanged. Every message, test and doc that names an old subcommand is
updated, for example the "run: horch ledger done <id>" hint in execution
planning.

**Blocked by:** None (can start immediately)

**Status:** ready-for-agent

**Persona:** primary `backend-developer`, fallback `opus`

**Parallel:** yes

**Spec:** `docs/specs/task-ledger.md` sections 2, 3, 4, 11.2

- [ ] `rusqlite` with the `bundled` feature is a workspace dependency; no
      system SQLite is needed.
- [ ] `horch ledger status` on an empty `HORCH_LEDGER_DB` creates the file,
      prints `No runs.` and exits 0.
- [ ] A test opens the store twice from 2 threads and runs 2 `BEGIN
      IMMEDIATE` writes; both succeed, 1 after the other.
- [ ] A test inserts a native `tasks` row without `task_file` and expects a
      constraint error, then inserts the same row with `origin = 'import'`
      and expects success.
- [ ] Every enum column (`tasks.status`, `workers.status`,
      `worktrees.status`, `checks.verdict`, `stops.result`, `events.kind`)
      has a `CHECK` that lists its values from the spec.
- [ ] `horch ledger legacy <old subcommand>` behaves exactly as
      `horch ledger <old subcommand>` did; the e2e messaging test that
      expects the old hint is updated.
- [ ] The store never reads or writes `ANTHROPIC_API_KEY`.
