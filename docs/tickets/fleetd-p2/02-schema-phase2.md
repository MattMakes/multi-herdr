# 02: The next schema version: the leases and questions tables, the new task and wait columns, and their backfill

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-core --test taskledger_schema_p2 && cargo test -p horch-core --lib taskledger::schema && cargo test -p horch-core --test taskledger_schema_v8 && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/taskledger/schema.rs`, `crates/horch-core/tests/taskledger_schema_p2.rs`, `crates/horch-core/tests/arch_scan.rs`

Spec: `docs/specs/fleetd/design.md`, "Ledger" (the `CREATE TABLE` of `tasks`, `leases` and `questions`; rule 2 "Leases, not claims"), "Migration plan" item 4 and the row Phase 2 of "What a release does to a run in flight"; `docs/specs/fleetd/preservation-05-contracts-and-values.md` rows L-3, L-4 and L-5.

## Goal

1 new schema version adds what Phase 2 stores: the `leases` table, the `questions` table, the task columns of the retry backoff, the replan and the cancel, and the wait columns of `max_wait_s`. The migration backfills a lease for each held task and a fleet-clock time limit for each open wait, so a run in flight goes on after the update.

## Requirements

- **RR-2** (the table): a task is held by a lease with an expiry on the fleet clock. 1 lease row for each task at most (`task_id` is the key).
- **FM-14** (the table): the lease row is unique for each task.
- The release row of Phase 2: "Each held task gets a lease with a full time. An open `worker_waits` row of an interactive persona gets `max_wait_s` from the release."
- **OR-6** (kept): a newer schema is refused, a backup is written before the migration, and a development build does not migrate the shared ledger.

## Context

- Read `docs/tickets/fleetd-p2/MAP.md`, section "Open questions", first (Q2, Q4, Q11, Q13, Q21 apply here).
- The schema number. Do not write a number in this ticket's code comments or tests other than through the constant. Read `VERSION` in `crates/horch-core/src/taskledger/schema.rs` (8 today on the branch: the fleetd schema of Phase 1, `docs/tickets/fleetd-p1/03-schema-v8.md`). Phase 2 is the next version, `VERSION + 1`. A later merge task can renumber the fleetd versions (the main branch also has a version 8): keep each number in 1 constant. Ticket 03 of this map adds its changes to the same new version (nothing is released between 02 and 03).
- The Phase 1 migration is the model: 1 helper for each table, a test file that builds the old version with `schema::migrate_to` and checks the rows after (`crates/horch-core/tests/taskledger_schema_v8.rs`), and the 3 safety rules of OR-6 (the backup, `migrate_refusal` in `crates/horch-core/src/taskledger/store.rs`).
- The tables, as the design writes them, with the changes of MAP.md:
  - `leases (task_id TEXT PRIMARY KEY REFERENCES tasks(task_id), attempt_id TEXT NOT NULL REFERENCES attempts(id), expires_at INTEGER NOT NULL, renewed_at TEXT NOT NULL, renew_basis TEXT NOT NULL)`. `expires_at` is a deadline on the fleet clock (`fleetd.fleet_clock_s`, seconds).
  - `questions (id TEXT PRIMARY KEY, task_id TEXT NOT NULL REFERENCES tasks(task_id), feature_id TEXT NOT NULL, owner TEXT NOT NULL, state TEXT NOT NULL CHECK (state IN ('open','answered','escalated')), answer_case TEXT CHECK (answer_case IN ('covered','revise','replan','own_decision')), citation TEXT, due_at INTEGER NOT NULL, asked_at TEXT NOT NULL, answered_at TEXT)`. `feature_id` has no `REFERENCES`: the `features` table is Phase 4, and before then it holds the run id (MAP.md, Q13).
  - `tasks`: add `not_before INTEGER` (fleet clock: the end of a retry backoff), `due_at INTEGER` (fleet clock: the time limit of the current state, for `failed`), `replans INTEGER NOT NULL DEFAULT 0`, `cancel_requested TEXT` (the reason of a cancel that waits for the stop of a live attempt, MAP.md Q11). Today's `attempts`, `attempt_floor` and `held_reason` stay.
  - `attempts`: add `active_at INTEGER` (fleet clock: the last time that a pass saw activity of the attempt; the stall ladder and the lease measure "no activity" on the fleet clock from it, PL-4; MAP.md Q7).
  - `worker_waits` (today: `V2` in `schema.rs`): add `due_at INTEGER` (fleet clock: the cap `max_wait_s`) and `question_id TEXT` (the live `questions` row behind a question wait).
- The backfill (MAP.md, Q21):
  - Each task in `running` whose live attempt exists (the `attempts` row in `live` of Phase 1) gets 1 lease: `attempt_id` that attempt, `expires_at` = the fleet clock of the `fleetd` row + 120 (the default `liveness.lease_s`), `renew_basis` `backfill`.
  - Each live attempt gets `active_at` = the fleet clock, so no attempt is judged idle for the time before the update.
  - Each `worker_waits` row gets `due_at` = the fleet clock + 3600 (the default `liveness.max_wait_s`). Each worker of Phase 2 is interactive (MAP.md, Q10).
  - The task words do not change in this ticket. Ticket 03 changes them.
- The owners of the new tables for the fitness test `fitness_no_context_touches_foreign_tables` (`FLEETD_TABLE_OWNERS`, `crates/horch-core/tests/arch_scan.rs:1351`): `leases` and `worker_waits` to `task_execution`, `questions` to `ideas` (`ddd/08-code/task-execution/design.md`, `ddd/08-code/ideas/design.md`).

## Steps

1. Write the tests first, in the new file `crates/horch-core/tests/taskledger_schema_p2.rs`. Each fails before the code exists:
   - `phase2_migration_adds_leases_and_questions` (the tables and their CHECK lists).
   - `held_task_with_a_live_attempt_gets_1_lease_with_a_full_time` (the release row).
   - `task_with_no_live_attempt_gets_no_lease`.
   - `open_wait_gets_a_due_at_on_the_fleet_clock`.
   - `live_attempt_gets_active_at_on_the_fleet_clock`.
   - `lease_is_unique_for_each_task` (a second insert for 1 `task_id` fails: FM-14).
   - `newer_schema_is_refused_and_backup_precedes_migration` (`ddd/08-code/implementation-plan.md`, Slice 2).
   - `migration_from_the_phase1_version_keeps_each_row` (a dump before and after, as `taskledger_schema_v8.rs` does).
2. Write the migration step and the helpers. Bump `VERSION` by 1. Add the 3 owners to `FLEETD_TABLE_OWNERS`.
3. Run the Check and the guard tests. Commit.

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

The Check passes. A ledger of the Phase 1 version migrates to the new version with a backup first, keeps each row, and has 1 lease for each held task with a live attempt.
