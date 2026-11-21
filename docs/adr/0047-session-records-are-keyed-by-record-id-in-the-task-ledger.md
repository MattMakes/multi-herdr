# Session records are keyed by record id in the task ledger

Each project dir has 1 set of session records, so "what has been worked on
here, by which session" survives pane close and workspace restarts. That is
what lets the orchestrator resume an old session id instead of starting
cold. Records are keyed by `record_id`, minted at spawn and always known,
with `session_id` as a separate field, because codex only reveals its
session id after launch. The records live in the `session_records` table of
a task ledger file (TRF-45, D10), 1 row per `LedgerRecordV1`, the record as
JSON. A read-modify-write is 1 immediate SQLite transaction. Every read and
write goes through `ExecutionStore` in
`crates/horch-core/src/execution/store.rs`. The lifecycle rules live in
`records.rs`.

## Consequences

- The old JSON ledger at `<state_root>/<slug>.json` (the path and slug rule
  of every earlier version) is read and stays resumable, but nothing writes
  it. `horch ledger import` copies it, and a record only it holds moves into
  the table at the next write.
- `session_db` picks the file: the task ledger, unless the state dir moved.
  A state dir moved with `HORCH_STATE_DIR` alone (a test, a smoke run) keeps
  its sessions in `<state_root>/ledger.sqlite3`, as it kept its JSON ledgers
  in the state dir before, so it never writes another ledger's sessions. A
  worker pane gets the default state dir by name, which is no move.
- `plan::fresh_session` mints a session id before launch only when the
  harness accepts a caller-chosen one. Codex and OpenCode reveal theirs
  after launch, so the record has `None` and the worker discovers it.
- `SessionLedger::has_session` is true when any record already claims a
  session id. It stops 2 concurrently spawned codex workers from harvesting
  the same rollout file as their session.
