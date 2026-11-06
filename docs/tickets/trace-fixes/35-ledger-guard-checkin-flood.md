# 35: A dev build cannot migrate the shared ledger, and a failing checkin does not flood the pane

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `opus` · **Parallel:** yes
**Kind:** quality
**Review:** none
**Check:** `cargo check --workspace --all-targets && cargo clippy --workspace --all-targets -- -D warnings && cargo test -p horch-core -- trf_48 trf_49 taskledger`
**Own:** `crates/horch-core/src/taskledger/store.rs`, `crates/horch-core/src/taskledger/schema.rs`, `crates/horch-core/src/taskledger/supervisor.rs`, `crates/horch-core/tests/trf_48_ledger_guard.rs`

Spec: `docs/specs/trace-fixes.md` (conventions in section 8). Added by the orchestrator after an incident.
Incident record: `ai_docs/plans/trace-fixes/decisions.md`, section "Incident: shared ledger migrated to v7".
Waits for: nothing.

## Incident

On 2026-11-06 at 00:02Z a worker ran `cargo run -p horch -- ledger add-map --dry-run` in its worktree,
with no `HORCH_LEDGER_DB`. `Store::open` (`store.rs:29`) opened the shared `~/.horch/ledger.sqlite3`
and `schema::migrate` moved it to version 7. The installed horch 0.4.5 knows only version 6, so it
refused the ledger for every fleet on the host. Every worker pane then printed
`horch worker: checkin failed: the task ledger is schema version 7; ...` once per checkin interval,
forever (`supervisor.rs:110`). The operator has seen this flood more than once. The error told the
reader to run `horch self-update`, which cannot help when the installed horch is already the latest.
The v7 migration is plain `ALTER TABLE ADD COLUMN`, so it is not safe to run twice; recovery needed
a hand-written downgrade (`ai_docs/plans/trace-fixes/ledger-downgrade-v7-to-v6.sql`).

## Requirements

- **TRF-48**: A horch run from a cargo build output (its executable path has a `target/debug/` or
  `target/release/` component, including `target/*/deps/`) does not migrate the operator's shared
  ledger. The shared ledger is `<passwd home>/.horch/ledger.sqlite3`, where the home comes from
  `libc::getpwuid(getuid())`, not from `$HOME`. It refuses with 1 error that names the executable,
  the ledger path, `HORCH_LEDGER_DB=<scratch file>` and the opt-in `HORCH_ALLOW_LEDGER_MIGRATE=1`.
  Opening without migration (version already current) is allowed. A ledger anywhere else (temp
  dirs, `HORCH_LEDGER_DB`, a test `HOME`) migrates as today. Every migration of an existing file
  with `user_version > 0` first writes a copy with the SQLite backup API to
  `<db dir>/ledger.v<old>-backup-<UTC yyyymmddTHHMMSSZ>.sqlite3`, and keeps the newest 3 such
  copies. The "newer schema" error names the newest backup when one exists, and says a newer or
  development horch migrated the file; it keeps `horch self-update` as 1 option, not the only one.
  Test names start with `trf_48_`.
- **TRF-49**: The checkin thread prints a failure the first time it happens and when its text
  changes. While the same error repeats, it prints at most 1 line per 10 min, naming the repeat
  count. On the first success after failures, it prints 1 line `horch worker: checkin recovered
  after <n> failures`. Test names start with `trf_49_`.

## Steps

1. Read `store.rs` `Store::open`, `schema.rs` `migrate` and `migrate_unchecked`, and `supervisor.rs`
   `start_checkins` and `beat`.
2. TRF-48 guard: decide "dev build" and "shared ledger" in small pure functions that take the exe
   path, the ledger path, the passwd home and the env value as arguments, so tests need no real
   paths. Apply the guard only when `current < VERSION` and `current > 0`. A fresh file
   (`user_version = 0`) is created as today.
3. TRF-48 backup: take it inside `migrate` after the version read and before the first statement.
   Use `rusqlite`'s backup API (feature `backup` if needed; say so in the report) or `VACUUM INTO`.
   Prune to the newest 3 by file name.
4. TRF-48 error text: change the `current > VERSION` message as described. Keep it 1 sentence plus
   1 instruction line.
5. TRF-49: keep the state (last error text, repeat count, last print time) in the thread. Put the
   decision "print this line or not" in a pure function with an injected clock, and test it.
6. Tests in `crates/horch-core/tests/trf_48_ledger_guard.rs` and unit tests in `supervisor.rs`:
   - `trf_48_dev_build_refuses_to_migrate_the_shared_ledger` (pure functions);
   - `trf_48_opt_in_and_scratch_ledgers_migrate`;
   - `trf_48_migration_writes_a_backup_and_keeps_3` (real temp v6 file built by the v6 DDL path, or
     a file with `user_version = 6` made by the test);
   - `trf_48_newer_schema_error_names_the_backup`;
   - `trf_49_same_error_prints_once_then_every_10_min`, `trf_49_changed_error_prints`,
     `trf_49_recovery_prints_1_line`.
7. Run the Check. Then run `cargo test --workspace --no-run` to confirm every test target still
   builds.

Warning: never run a branch-built horch against the shared ledger. Set
`HORCH_LEDGER_DB=$(mktemp -d)/ledger.sqlite3` for any manual run.

Out of scope: briefing text (ticket 30 adds the worker rule). The add-map dry-run fix (ticket 10).

## Done when

The check passes, and report 35 shows the exact refusal text and the exact flood lines.

## Report

Write `/Users/mascott/projects/multi-herdr/ai_docs/reports/trace-fixes/35.md` (spec section 8). Then run `horch ledger finish`; when every step passed, run `horch ledger done "<summary>"`.
