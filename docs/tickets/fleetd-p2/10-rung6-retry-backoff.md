# 10: Rung 6: a burned attempt waits for its backoff on the fleet clock, then failed, 1 replan, escalated

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-core --lib fleetd::task_execution && cargo test -p horch-core --lib taskledger && cargo test -p horch-e2e --test taskledger && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/task_execution/domain/retry.rs`, `crates/horch-core/src/fleetd/task_execution/application/retry.rs`, `crates/horch-core/src/fleetd/task_execution/application/commands.rs`, `crates/horch-core/src/taskledger/tasks.rs`

Spec: `docs/specs/fleetd/design.md`, "Supervision, liveness and reconciliation" ("Stall ladder", rung 6; "The task state machine": the rows `stalled`, `failed`, `escalated`; the last paragraph "Only the verifier moves a task to done"), "Migration plan" ("Before Phase 6, a run stands for its idea and its feature"), "The numbers of the design" (retry backoff, attempts, replan limit); `docs/specs/fleetd/preservation-04-behavior.md` rows L3 and L4; `ddd/08-code/task-execution/aggregate-canvas-task.md` (`promote-task`, rung 6).

## Goal

Each outcome that is not verified and is not `asked` or `quota` burns 1 attempt. With attempts left, the task goes to `ready` with `not_before` at the end of its backoff on the fleet clock, and the claim does not take it before then. With no attempt left, the task goes to `failed`; the orchestrator of the run replans it 1 time within `replan_limit_s`; after that, or past the limit, it goes to `escalated` with 1 item on the needs-you list. No path leaves a task held and forgotten.

## Requirements

- **RR-6**: every retry is bounded. After the max attempts, the task is escalated to the user, not retried forever. Verified by a state machine test.
- **PL-4** (the backoff part): each retry backoff is a deadline on the fleet clock. A sleep past it fires it only after the rest of its fleet time.
- Preservation 04 L3 (a retry goes first in line) and L4 (the limit counts the attempts since the last operator requeue, `attempt_floor`) stay.

## Context

- Read `docs/tickets/fleetd-p2/MAP.md`, section "Open questions", first (Q10 and Q12 apply here).
- Today: `tasks::requeue` (`crates/horch-core/src/taskledger/tasks.rs`, after the enum `Requeue`) calls `requeue_in`: `retry_in` (queued again, priority minus `RETRY_PRIORITY_BOOST`) or `hold_in` (held with `held_reason` "N attempts failed") through `after_failure`. Phase 1 ticket 20 added `schedule_retry` and `escalate` (`tasks.rs`) and the use cases `schedule_retry` and `escalate_task` in `crates/horch-core/src/fleetd/task_execution/application/commands.rs`. Ticket 03 of this map renamed the words (`ready`, `escalated`).
- The rules, pure in `domain/retry.rs`:
  - The outcomes that burn 1 attempt: `rejected`, `crashed`, `timed_out`, `stalled`, `killed`, `no_result`. `asked`, `quota` and `critical_open` burn none.
  - Attempts left (`attempts - attempt_floor` below the limit; the limit is `retry.max_attempts` of the profile, else the run's `max_attempts`, ticket 06): `ready` with `not_before` = the fleet clock + `backoff_s[n]` (`n` = the attempts used minus 1; the last value repeats). L3 stays: the retry is first in line.
  - No attempt left: `failed`, with `due_at` = the fleet clock + `replan_limit_s` (ticket 05). The owner of a `failed` task before Phase 6 is the orchestrator of the run (MAP.md, Q12): fleetd sends it 1 wake through today's mail path (`crates/horch-core/src/taskledger/outbox.rs`), with the text "replan <task>: requeue it 1 time, or cancel it" and the commands.
  - The 1 replan: a requeue of a `failed` task with `replans` 0 moves it to `ready`, with its attempt count at 0 (`attempt_floor` = `attempts`) and `replans` 1. Ticket 13 writes the command. A cancel of it is the replan that replaces the task.
  - `failed` with `replans` 1, or a `failed` task past its `due_at`: `escalated`, with `held_reason` (today's text "N attempts failed") and 1 needs-you item with the key `task_escalated:<task id>`. Your requeue (ticket 13) is the exit.
  - The claim (`pick_task`, `tasks.rs:995`) takes a `ready` task only when the fleet clock is at or past its `not_before`. A ready task in its backoff shows the cause in `horch ledger status` with the words of today's wait list (`tasks::waits`, `tasks.rs:874`): "retry after <n> s".
- The pass (ticket 14) calls the function that moves `failed` tasks past their limit, for example `escalate_overdue(tx, clock)` in `application/retry.rs`.
- A resolver attempt or a fix attempt of a `done` task is Phase 4. Do not add `repair_attempts`.

## Steps

1. Write the tests first. Each fails before the code exists:
   - In `domain/retry.rs`: `retry_backoff_30s_2m_8m_then_replan_then_escalate` (`ddd/08-code/implementation-plan.md`, Slice 2), `asked_quota_and_critical_open_burn_no_attempt`, `last_backoff_value_repeats`, `limit_counts_attempts_since_the_attempt_floor`.
   - In `application/retry.rs` or `tasks.rs`: `ready_task_is_not_claimed_before_not_before`, `sleep_past_a_backoff_fires_it_only_after_the_rest_of_its_fleet_time`, `failed_task_wakes_the_orchestrator_once`, `requeue_of_a_failed_task_is_the_1_replan`, `second_failure_after_the_replan_escalates_with_1_needs_you_item`, `failed_task_past_replan_limit_escalates`, `rr6_no_sequence_of_outcomes_retries_forever` (an exhaustive walk with `std` only; the repository has no `proptest`).
2. Write the code. Run the Check and the guard tests. Commit.

## Constraints

@@RULES@@

## Done when

The Check passes. A burned attempt waits for its backoff on the fleet clock, and the last one leads to `failed`, 1 replan, then `escalated` with 1 needs-you item.
