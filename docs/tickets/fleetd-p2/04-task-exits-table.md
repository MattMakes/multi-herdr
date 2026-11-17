# 04: transition() has each exit of the 12-state task table, and I2 promotes pending tasks in the pass

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-core --lib fleetd && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/fleet_runtime/domain/ledger.rs`, `crates/horch-core/src/fleetd/task_execution/domain/task.rs`

Spec: `docs/specs/fleetd/design.md`, "Supervision, liveness and reconciliation" ("The task state machine": the column "Leaves when" of each of the 12 rows, and "A cancel"), "Ledger" (rule 1 "One door"; invariant I2); `ddd/08-code/task-execution/aggregate-canvas-task.md`.

## Goal

The table of allowed transitions of `Entity::Task` is exactly the 12-state table of the design. Each move that the table does not have is refused by the door. The pure rule of I2 (a `pending` task whose dependencies are all met goes to `ready`) is a function of the task domain that the pass calls.

## Requirements

- **RR-2** (the table part): no path leaves a task held with no exit. Each held state has an exit in the table.
- **RR-6** (the table part): the exits `running -> stalled`, `stalled -> ready`, `stalled -> failed`, `failed -> escalated` and `escalated -> ready` exist, so a retry is bounded and ends with you.
- The exit to `cancelled` from each state that is not terminal (`done` and `cancelled` are terminal).

## Context

- Read `docs/tickets/fleetd-p2/MAP.md`, section "Open questions", first (Q5, Q12 and Q20 apply here).
- Ticket 03 of this map renamed the exits of today to the new words. This ticket adds the rest. Read `EXITS` in `crates/horch-core/src/fleetd/fleet_runtime/domain/ledger.rs` first.
- The exits, from the column "Leaves when" of the design table:
  - `pending -> ready`, `pending -> cancelled`.
  - `ready -> leased`, `ready -> running` (the claim of Phase 2, MAP.md Q5), `ready -> cancelled`.
  - `leased -> running`, `leased -> ready` and `leased -> failed` (rung 6 after `timed_out` or `crashed`), `leased -> cancelled`.
  - `running -> verifying`, `running -> blocked_on_user`, `running -> ready` (rung 6, and the restart on a fallback), `running -> waiting_quota`, `running -> stalled`, `running -> failed` (rung 6 with no attempt left), `running -> cancelled`.
  - `stalled -> ready`, `stalled -> failed`, `stalled -> verifying`, `stalled -> cancelled`.
  - `verifying -> done`, `verifying -> ready` (outcome `rejected`, rung 6, and a fix round), `verifying -> failed`, `verifying -> cancelled`.
  - `blocked_on_user -> ready`, `blocked_on_user -> cancelled`.
  - `waiting_quota -> ready`, `waiting_quota -> cancelled`.
  - `failed -> ready` (the 1 replan), `failed -> escalated`, `failed -> cancelled`.
  - `escalated -> ready`, `escalated -> cancelled`.
  - Remove today's exit `cancelled -> ready` (a requeue of a cancelled task, renamed by ticket 03). The design has `cancelled` as terminal (MAP.md, Q22). `horch ledger requeue` of a cancelled task is then refused with the reason of the door. Change the test that pins it.
- I2 in Phase 2: a normal dependency is met when its task is `done` (the task branch state is Phase 4). Write the pure function in `domain/task.rs`, for example `fn promotable(tasks: &[TaskRow], deps: &[(String, String)]) -> Vec<String>`. Ticket 14 calls it in the pass (`promote_pending`, after `finish_exited`). Ticket 03 already promotes in the transaction that moves a task to `done`. The pass is the level-triggered repair.

## Steps

1. Write the tests first. Each fails before the code exists:
   - `allowed_transitions_of_task_equal_the_12_state_table` (`ddd/08-code/implementation-plan.md`, Slice 2): the set of task exits is exactly the list above.
   - `each_state_that_is_not_terminal_has_an_exit_to_cancelled`.
   - `done_and_cancelled_have_no_exit`.
   - `every_held_state_has_an_exit_that_frees_the_task` (RR-2: `leased`, `running`, `stalled`, `blocked_on_user`, `waiting_quota` each reach `ready` or `cancelled` through the table).
   - `pending_task_with_each_dependency_done_is_promotable` and `pending_task_with_a_dependency_not_done_is_not_promotable`.
2. Write the code. Run the Check and the guard tests. Commit.

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

The Check passes. The task exits of `EXITS` are exactly the table of the design, and a move outside it is refused by `transition()`.
