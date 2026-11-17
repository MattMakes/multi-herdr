# 03: The 12 task states as the stored words of tasks.status, with today's words in what you read

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** no
**Kind:** refactor
**Review:** full
**Check:** `cargo test --workspace --locked`
**Own:** `crates/horch-core/src/taskledger/model.rs`, `crates/horch-core/src/taskledger/schema.rs`, `crates/horch-core/src/taskledger/sql_words.rs`, `crates/horch-core/src/taskledger/tasks.rs`, `crates/horch-core/src/taskledger/status.rs`, `crates/horch-core/src/taskledger/runs.rs`, `crates/horch-core/src/taskledger/store.rs`, `crates/horch-core/src/taskledger/stop.rs`, `crates/horch-core/src/taskledger/supervisor.rs`, `crates/horch-core/src/taskledger/watchdog.rs`, `crates/horch-core/src/taskledger/import.rs`, `crates/horch-core/src/taskledger/workers.rs`, `crates/horch-core/src/taskledger/finish/mod.rs`, `crates/horch-core/src/taskledger/finish/land.rs`, `crates/horch-core/src/taskledger/triage/mod.rs`, `crates/horch-core/src/fleetd/fleet_runtime/domain/ledger.rs`, `crates/horch-core/src/fleetd/fleet_runtime/domain/jobs.rs`, `crates/horch-core/src/fleetd/fleet_runtime/application/jobs.rs`, `crates/horch-core/src/fleetd/fleet_runtime/adapters/sqlite.rs`, `crates/horch-core/src/fleetd/fleet_runtime/adapters/jobs_store.rs`, `crates/horch-core/src/fleetd/task_execution/domain/task.rs`, `crates/horch-core/src/fleetd/task_execution/application/commands.rs`, `crates/horch-core/src/fleetd/task_execution/adapters/sqlite.rs`, `crates/horch/src/cmd/taskledger.rs`, `crates/horch/src/cmd/taskledger_spawn.rs`, `crates/horch/src/cmd/taskledger_wake.rs`, `crates/horch/src/cmd/taskledger_watch.rs`, `crates/horch/src/cmd/recipes.rs`, `crates/horch-core/tests/taskledger_schema_p2.rs`, `crates/horch-core/tests/taskledger_schema_v8.rs`, `crates/horch-core/tests/w5_status_words_scan.rs`, `crates/horch-core/tests/i308_triage_apply.rs`, `crates/horch-core/tests/taskledger_queue.rs`, `crates/horch-core/tests/trf_35_park_unlanded.rs`, `crates/horch-core/tests/w3_finish_steps.rs`, `crates/horch-core/tests/w3_taskledger_core_status.rs`, `crates/horch-core/tests/w3_taskledger_core_tasks.rs`, `crates/horch-core/tests/w3_taskledger_core_import.rs`, `crates/horch/tests/trf_37_ledger_handoff_cli.rs`, `crates/horch/tests/w3_ledger_cli_pins.rs`, `crates/horch-e2e/tests/taskledger.rs`, `crates/horch-e2e/tests/i308_triage.rs`, `crates/horch-e2e/tests/p0_dispatch.rs`, `crates/horch-e2e/tests/p0_rescue.rs`, `crates/horch-e2e/tests/p0_supervision.rs`

Spec: `docs/specs/fleetd/design.md`, "Ledger" (the `tasks` table, its `CHECK` list of 12 states), "Supervision, liveness and reconciliation" ("The task state machine"); `ddd/08-code/task-execution/aggregate-canvas-task.md`.

## Goal

`tasks.status` stores the 12 states of the design: `pending`, `ready`, `leased`, `running`, `stalled`, `verifying`, `blocked_on_user`, `waiting_quota`, `done`, `failed`, `escalated`, `cancelled`. Each task of today moves to its design state in the migration. The behavior of today does not change. What you read in `horch ledger show`, `horch ledger status`, `horch ledger events` and their `--json` keeps the words of today.

## Requirements

- The 12-state table of the design is the stored `CHECK` list of `tasks.status` (the enum `TaskStatus` in `crates/horch-core/src/taskledger/model.rs` is the 1 source of the list, as today).
- The operator's rule for Phase 2: what you read keeps today's words, and a new state has a word that says what it is (MAP.md, Q3).
- No change of behavior: each test that passes before this ticket passes after it, with a changed fixture word only where the test writes or reads `tasks.status` directly.

## Context

- Read `docs/tickets/fleetd-p2/MAP.md`, section "Open questions", first (Q3, Q4, Q5 and Q20 apply here).
- Ticket 02 of this map adds the new schema version. This ticket adds its changes to the same new version (nothing is released between 02 and 03). Read `VERSION` in `crates/horch-core/src/taskledger/schema.rs`. Do not write a schema number anywhere else.
- Today `TaskStatus` has 5 words: `queued`, `running`, `landing`, `done`, `cancelled` (`model.rs:66`). "Blocked" is derived from `task_deps`, and a held task is `queued` with `held_reason` (`schema.rs`, the comment of `V3`; preservation 04 row L4). The migration maps each row (MAP.md, Q4):
  - `queued` with `held_reason` not null (the hold at `max_attempts`, and the park `unlanded` of row L5) -> `escalated`. `held_reason` stays, so the 2 kinds stay apart.
  - `queued` with a dependency that is not met (today's rule of `tasks::blockers`, `tasks.rs:298`) -> `pending`.
  - each other `queued` -> `ready`.
  - `running` -> `running`. `landing` -> `verifying`. `done` -> `done`. `cancelled` -> `cancelled`.
- The `CHECK` of a column changes only with a rebuild of the table in SQLite. `tasks` is referenced by other tables. Phase 1 rebuilt `events` (`docs/tickets/fleetd-p1/03-schema-v8.md`): use the same safe pattern (foreign keys off for the rebuild, `PRAGMA foreign_key_check` after, 1 transaction).
- Each writer of today maps as follows. Do not add a new path:
  - Each write of `queued` for a new task writes `pending` when a dependency is not met, else `ready`. A requeue of a task that ran writes `ready`: its dependencies were met at its claim, and a `done` task does not move back. The claim (`tasks::claim`, `tasks.rs:937`; `take_task`, `tasks.rs:1120`) takes only `ready`, with today's other rules.
  - Each read of `queued` reads `pending` or `ready` (or `escalated` where today's code reads `queued` with `held_reason`).
  - The claim writes `running` (MAP.md, Q5). `leased` is in the table, and Phase 4 writes it.
  - `landing` becomes `verifying`, in each writer and reader.
  - The 3 states of today's hold: the hold at `max_attempts` and the park `unlanded` write `escalated`; the operator requeue (`tasks::requeue_by_operator`, `tasks.rs:1515`) moves `escalated` to `ready`.
  - When a task moves to `done`, its dependents in `pending` whose dependencies are all met move to `ready` in the same transaction. Today's derived "blocked" stays where it is shown.
- `transition()` (`crates/horch-core/src/fleetd/fleet_runtime/domain/ledger.rs`, `EXITS`): rename the exits of `Entity::Task` to the new words with the same meaning (for example `queued -> running` becomes `ready -> running`, `running -> queued` becomes `running -> ready`, and `cancelled -> queued` becomes `cancelled -> ready`). Ticket 04 adds the other exits of the 12-state table. Keep the test that pins the exits of today, with the new words.
- The words that you read. Add 1 function in `model.rs`, for example `TaskStatus::shown(self, held: bool) -> &'static str`: `pending` and `ready` -> `queued`; `leased` and `running` -> `running`; `verifying` -> `landing`; `escalated` -> `queued` (the display of a held task stays as today, with its held reason); `done`, `cancelled`, `stalled`, `blocked_on_user`, `waiting_quota` and `failed` keep their own word. Use it in `status.rs` (`render_tasks`, `task_line`), in `show_task` (`crates/horch/src/cmd/taskledger.rs:1580`) and in each `--json` field `status` of a task. Each `--json` object of a task gets 1 new field `state` with the stored word.
- Events. The `events` rows of Phase 1 have `from_state` and `to_state`. New rows store the design words. `horch ledger events` (`status::events`, `status.rs:764`) prints the event kind of today, so its text does not change. Do not rewrite old event rows.
- `crates/horch-core/tests/w5_status_words_scan.rs` checks that SQL builds its words from the enums. Keep it green.
- `'queued'` is also a word of the `jobs` table (Phase 1). Change only task words. A `jobs` file is in `Own` only for the case that it reads `tasks.status`.
- The run check is the Check of this ticket. It takes about 15 min. Run `horch ledger progress` each 5 min while it runs.

## Steps

1. Run `cargo test --workspace --locked` first, and record the count of passed tests and the list of expected failures.
2. Write the tests first. Each fails before the code exists:
   - In `crates/horch-core/tests/taskledger_schema_p2.rs`: `migration_maps_each_task_word_of_today_to_its_design_state` (1 row for each case of the map above), `migration_keeps_held_reason_and_each_foreign_key`.
   - In `model.rs`: `task_status_has_the_12_states_of_the_design`, `shown_word_of_each_state_is_the_word_of_today`.
   - In `tasks.rs`: `new_task_with_an_unmet_dependency_is_pending`, `done_moves_each_pending_dependent_with_all_dependencies_met_to_ready`, `claim_takes_only_a_ready_task`.
3. Change the enum, the migration, and each site. Let the compiler find each site of `TaskStatus`. Find each raw word with `rg "'queued'|\"queued\"|'landing'|\"landing\"" crates`.
4. Run the Check. The count of passed tests is the count of step 1 plus the new tests. The list of expected failures is the same. Run the guard tests. Commit.

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

The Check passes with the same results as before plus the new tests. `tasks.status` has the `CHECK` list of the 12 states. `horch ledger show T-1` and `horch ledger status` print the words of today for a task of today.
