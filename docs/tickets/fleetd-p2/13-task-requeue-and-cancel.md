# 13: horch task requeue and horch task cancel; a cancel stops a live attempt before the task is cancelled

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-core --lib fleetd::task_execution && cargo test -p horch --lib && cargo test -p horch --test w3_ledger_cli_pins && cargo test -p horch-e2e --test taskledger && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/task_execution/domain/cancel.rs`, `crates/horch-core/src/fleetd/task_execution/application/cancel.rs`, `crates/horch/src/cmd/taskcmd.rs`, `crates/horch/src/cmd/mod.rs`, `crates/horch/src/main.rs`, `crates/horch/src/cmd/taskledger.rs`, `crates/horch/tests/w3_ledger_cli_pins.rs`, `docs/architecture.md`

Spec: `docs/specs/fleetd/design.md`, "Supervision, liveness and reconciliation" ("The task state machine": the rows `escalated`, `failed`, `cancelled`; "A cancel"), "Long work is a job" (the last sentence of "The kinds of job": `horch task requeue` and the `task_check` of its task), RR-18, "Migration plan" item 4 ("`horch task requeue` and `horch task cancel`, and the exit to `cancelled` from each task state that is not terminal"); `docs/specs/fleetd/preservation-04-behavior.md` rows L5, L8 and L10; `ddd/08-code/task-execution/aggregate-canvas-task.md` (`cancel-task`, `requeue-task`).

## Goal

You have 2 commands for a task: `horch task requeue <task> "<reason>"` and `horch task cancel <task> "<reason>"`. A cancel works from each state that is not terminal. When the task has a live attempt, the stop of its process group is recorded first, and the task is `cancelled` only after the process is gone. `horch ledger requeue` and `horch ledger cancel` of today call the same code and print the same lines.

## Requirements

- The exit to `cancelled` from each task state that is not terminal, with the stop of a live attempt first (the Phase 2 row of "What each gate needs").
- **RR-6** (the exit): `horch task requeue` of an `escalated` task sets its attempt count to 0 and moves it to `ready`. It also queues 1 more run of the `task_check` job of the task and sets its abandoned count to 0 (Phase 1, `horch job requeue`).
- **FM-14** (the cancel part): a new attempt never starts while a process of an older attempt of the task is alive.

## Context

- Read `docs/tickets/fleetd-p2/MAP.md`, section "Open questions", first (Q11, Q12, Q22 and Q24 apply here).
- Today: `cancel_cmd` (`crates/horch/src/cmd/taskledger.rs:919`) cancels first and then stops the holder (`cancel_and_stop`, TRF-36; `stop::stop_cancelled`). `requeue_cmd` (`taskledger.rs:1034`) calls `tasks::requeue_by_operator` (`crates/horch-core/src/taskledger/tasks.rs:1515`), which also resumes a parked worktree (row L5, `resume_parked_worktree`). The design orders a cancel the other way: the stop first, then `cancelled`.
- The cancel, in `application/cancel.rs` (pure rules in `domain/cancel.rs`):
  - A state that is not terminal and no live attempt: the task moves to `cancelled` at once, its lease row goes (ticket 08), and today's wake of the dependents stays (`wake_dependents`, `taskledger.rs`).
  - A live attempt: 1 transaction sets `tasks.cancel_requested` to the reason (ticket 02), ends the attempt with the outcome `killed` and the cause `task_cancelled`, and returns its `Stop` (Phase 1, `application/commands.rs`). After the commit the command does the stop with today's stop code (by recorded PID and start time, then the exact pane). When the command sees the process gone, a second transaction moves the task to `cancelled` and clears `cancel_requested`. When it does not (a timeout, or the command dies), the next pass does it: ticket 14 calls a function of this ticket, for example `complete_cancels(tx, facts)`, for each task with `cancel_requested` and no live process of its attempts.
  - A task with `cancel_requested` is never retried, re-leased or claimed: rung 6 (ticket 10) and the claim skip it.
  - `done` and `cancelled` refuse with the reason of the door. A cancel in Phase 2 keeps today's worktree handling: the rescue ref and the abandon of the task branch are Phases 3 and 4.
- The requeue, in `application/cancel.rs` with the cancel (both are commands of the operator on 1 task):
  - `escalated` -> `ready` (today's `requeue_by_operator`, with the park of row L5): `attempt_floor` = `attempts` (row L8), `held_reason` cleared, its needs-you item `task_escalated:<task id>` resolved, and the `task_check` job of the task requeued (the use case of `horch job requeue`, Phase 1 ticket 23: `crates/horch/src/cmd/jobcmd.rs`).
  - `failed` with `replans` 0 -> the 1 replan of ticket 10 (`ready`, attempts at 0, `replans` 1). With `replans` 1, refuse: the task escalates.
  - Each other state refuses with a reason that names the state in today's word (`TaskStatus::shown`, ticket 03).
  - A requeue by a code-sweep review keeps its own counter (05 V-14). Do not change it.
- The CLI: a new file `crates/horch/src/cmd/taskcmd.rs` with the subcommand `horch task` and its 2 commands, and 1 line in `crates/horch/src/cmd/mod.rs` and in `crates/horch/src/main.rs`. The arguments of `horch ledger requeue` and `cancel` of today (`<task> "<reason>"`, `--run`). `docs/architecture.md` gets 1 row for `taskcmd.rs` (`every_command_file_has_a_row_in_the_architecture_doc`). `horch ledger requeue` and `horch ledger cancel` keep their arguments, their output and their exit codes (`crates/horch/tests/w3_ledger_cli_pins.rs` pins them). Change a pin only where the cancel order changes a printed line, and name it in the commit message.
- Before Phase 5, the commands write the ledger directly through `transition()` ("Who writes the ledger before Phase 5").

## Steps

1. Write the tests first. Each fails before the code exists:
   - In `application/cancel.rs`: `cancel_from_each_non_terminal_state_stops_the_process_group_before_the_move` (`ddd/08-code/implementation-plan.md`, Slice 2), `cancel_with_no_live_attempt_is_immediate`, `cancel_requested_task_is_not_retried_or_claimed`, `pass_completes_a_cancel_when_the_process_is_gone`, `cancel_of_done_or_cancelled_is_refused`, `pres_04_l8_operator_requeue_resets_the_attempt_count`, `requeue_of_escalated_requeues_its_task_check_job`, `requeue_of_failed_is_the_1_replan`, `requeue_of_a_failed_task_after_its_replan_is_refused`.
   - In `taskcmd.rs`: `task_cancel_and_ledger_cancel_give_the_same_rows`, `task_requeue_and_ledger_requeue_give_the_same_rows`.
2. Write the code. Run the Check and the guard tests. Commit.

## Constraints

@@RULES@@

## Done when

The Check passes. `horch task cancel` of a task with a live attempt stops the process first and then marks the task `cancelled`, and `horch task requeue` of an escalated task makes it ready with an attempt count of 0.
