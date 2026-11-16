# 03: Schema v8: the fleetd, attempts, jobs, pass_cursors and needs_you tables, the events columns, and the backfill of attempts

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-core --lib taskledger::schema && cargo test -p horch-core --test taskledger_schema_v8`
**Own:** `crates/horch-core/src/taskledger/schema.rs`, `crates/horch-core/src/taskledger/model.rs`, `crates/horch-core/tests/taskledger_schema_v8.rs`

Spec: `docs/specs/fleetd/design.md`, "Ledger" (Schema: the tables `fleetd`, `attempts`, `jobs`, `pass_cursors`, `needs_you`, `events`), "Migration plan" (Schema migration is forward-only; the table "What a release does to a run in flight", row Phase 1), OR-6, and `docs/specs/fleetd/preservation-05-contracts-and-values.md` rows L-3, L-4 and L-5.

## Goal

The ledger moves from schema v7 to v8 in 1 forward-only migration. v8 has the Phase 1 tables of the design and the new columns of `events`. Each live worker row gets 1 attempt row. The safety of today stays: a backup before the migration, a newer schema refused, and no migration of the shared ledger by a development build.

## Requirements

- **OR-6** (part): schema migration is forward-only at daemon start; a newer schema is refused; a backup is written before each migration; a development build does not migrate the shared ledger. The code of today has these 3 rules (`migrate_guarded`, `backup`, `newer_schema` in `crates/horch-core/src/taskledger/schema.rs`); this ticket keeps them for v8 and proves them with tests.
- The backfill of "What a release does to a run in flight", row Phase 1: "Each live worker row gets an attempt row" (MAP.md, Q6: in the state `live`). The `fleetd` row with generation 1 is written at the first start of fleetd (ticket 14), not here.

## Context

- Read `docs/tickets/fleetd-p1/MAP.md`, section "Open questions", first. It records each choice of this phase (Q1 to Q21), and each ticket follows it.
- Phase P (`docs/tickets/fleetd-pp/`) builds `crates/horch-platform`. Its concern tickets propose function names. The module doc of each concern file in `crates/horch-platform/src/` gives the final name. Read that doc before you call a platform function. A name in this ticket is the proposed name.
- Schema numbers: Phase 1 is schema v8, and each later phase takes the next number. The design text names v8 for Phase 2 and v9 for Phase 6: from Phase 2 the numbers of the design text are 1 lower than the code (MAP.md, Q3).
- Today: `pub const VERSION: i64 = 7;` and `migrate_unchecked` in `schema.rs`, with the version steps `v1()` to `v7()`. `v5()` shows how to build a table again when a `CHECK` list changes (SQLite cannot change a `CHECK`).
- The v8 tables, from the design SQL, with these changes (MAP.md, Q5 to Q7):
  - `fleetd`: as the design (`id` = 1, `generation`, `pid`, `pid_start`, `started_at`, `fleet_clock_s`, `clock_ticked_at`). No row yet.
  - `attempts`: `id`, `task_id TEXT REFERENCES tasks(task_id)` (null for an orchestrator attempt), `worker_id TEXT NOT NULL REFERENCES workers(worker_id)`, `mux_handle`, `pid`, `pid_start`, `state` with `CHECK (state IN ('launching','live','ended'))`, `heartbeat_seq`, `started_at`, `ended_at`, `exit_code`, `outcome`. No `idea_id`: the `ideas` table comes in Phase 6.
  - `jobs`: as the design, with the `CHECK` list of the 14 kinds, the `UNIQUE (kind, subject, input_key, run)` and the index `jobs_one_active`.
  - `pass_cursors` and `needs_you`: as the design.
  - `events`: build the table again (as `v5()` does) with the old columns and the new columns `entity`, `entity_id`, `from_state`, `to_state`, `cause` and `data_json`, all nullable for old rows. Add the kind `transition` to `EventKind` in `model.rs`, so the `CHECK` list has it. Copy each old row.
- The backfill: 1 `attempts` row for each `workers` row whose status is a live status of `WorkerStatus` in `model.rs` and that has a task, with `state = 'live'`, the `agent_pid` and `agent_started` of the worker as `pid` and `pid_start`, and the id `<task id>/A<attempt>` (the `attempt` column of the worker). Write it in the same migration transaction.

## Steps

1. Write the tests first, in the new file `crates/horch-core/tests/taskledger_schema_v8.rs`. Each fails before v8 because the tables do not exist or the version is 7:
   - `migrate_from_version_7_adds_the_phase_1_tables_and_keeps_rows`: build a v7 file with rows in `tasks`, `workers` and `events` (use the migration of today to reach v7, then insert), migrate, and check the 5 new tables, the new `events` columns, the old rows and `PRAGMA user_version` = 8.
   - `migrate_7_to_8_writes_a_backup_first`: the backup `ledger.v7-backup-<stamp>.sqlite3` exists after the migration and has version 7 (`backups_in`).
   - `newer_schema_9_is_refused`: a file at version 9 is refused with the text of `newer_schema`.
   - `development_build_does_not_migrate_the_shared_ledger`: `migrate_guarded` with a guard that refuses leaves the file at version 7. Use the guard of today (read `crates/horch-core/src/taskledger/store.rs` for the caller that passes it).
   - `live_worker_gets_1_live_attempt_and_an_ended_worker_gets_none`.
   - `jobs_one_active_refuses_a_second_queued_run_of_1_kind_and_subject`.
2. Write `v8()` and the step `if current < 8` in `migrate_unchecked`. Set `VERSION` to 8. Update the pin tests of `schema.rs` only where they name the version.
3. Run `cargo test -p horch-core --lib taskledger` to see that the tests of today still pass. Run the Check.

## Constraints

1. This fleet runs on the installed `horch` binary and on the live ledger
   `/Users/MASCOTT/.horch/ledger.sqlite3`. The code that you change is the code of that tool.
   Do not break the fleet that builds it:
   - Each test, and each `horch` or `fleetd` process that you build and start, uses temporary
     folders for `HORCH_DATA_DIR`, `HORCH_STATE_DIR` and `HORCH_LEDGER_DB`. Never point a
     development build at the live ledger or at the live state folders.
   - Never install a development build. Never run `horch self-update`, `cargo install` or a
     release script from a worktree.
   - Never start a development `fleetd`, watchdog or service unit on this machine outside a test
     that uses temporary folders. Never register a launchd job, a systemd unit or a scheduled task
     on this machine.
   - The only `horch` commands that you run on the live fleet are the worker commands of your own
     task (`horch ledger ...`, `horch tell orchestrator ...`).
2. NEVER use `ANTHROPIC_API_KEY`. Never read, print, set, export, pass or depend on it. Never
   write code, config, docs or teammate files that use it. Each `claude` run uses the claude.ai
   subscription login. Remove the variable from each `claude` command:
   `env -u ANTHROPIC_API_KEY claude ...`. If a task seems to need an API key, stop and ask.
3. Do not use a shared `CARGO_TARGET_DIR`. Build in the `target/` folder of your worktree. Run
   `cargo clean` in your worktree before `horch ledger finish` only if the disk has less than
   20 GB free (`df -h /`).
4. Before the first commit, run `git remote -v` and `git config user.email`. The remote must be
   `git@git-alt:MattMakes/multi-herdr.git` and the email must be `m4ttplu5@gmail.com`. If not,
   stop and send BLOCKED.
5. Commit messages have no co-author line. Never write the em dash character; use `-`. Never edit
   `CHANGELOG.md` or a generated file by hand.
6. Write the tests first and see them fail for the correct reason. Then write the code.
7. The source of truth is `docs/specs/fleetd/design.md` (FINAL). If the code of today and the
   design do not agree on a point that the ticket does not settle, stop and send QUESTION. Do not
   change `docs/specs/fleetd/` or `ddd/`.
8. Change only the files of `Own`. If the work needs another file, send QUESTION.
9. Prefer quality, simplicity and robustness to speed. No feature that the design does not name.
10. If the GitHub SSH connection times out at `horch ledger finish`, run it again, 3 times maximum.

## Done when

The Check passes. A v7 ledger with live workers reaches v8 with 1 attempt for each, a backup is written first, and a v9 file is refused.
