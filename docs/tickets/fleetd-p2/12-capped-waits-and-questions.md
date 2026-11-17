# 12: Capped waits: max_wait_s on the fleet clock, a question wait points at a live questions row, and an answer frees a blocked task

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-core --lib taskledger && cargo test -p horch-core --lib fleetd::ideas && cargo test -p horch-core --lib fleetd::task_execution && cargo test -p horch-e2e --test taskledger && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/taskledger/workers.rs`, `crates/horch-core/src/taskledger/tasks.rs`, `crates/horch-core/src/fleetd/task_execution/domain/waits.rs`, `crates/horch-core/src/fleetd/task_execution/adapters/waits_store.rs`, `crates/horch-core/src/fleetd/ideas/domain/questions.rs`, `crates/horch-core/src/fleetd/ideas/application/questions.rs`, `crates/horch-core/src/fleetd/ideas/adapters/questions_store.rs`

Spec: `docs/specs/fleetd/design.md`, "Migration plan" item 4 ("`worker_waits` rows of an interactive persona get `max_wait_s` (default 1 h) and must point at a live question or review; an exec worker that asks a question exits, and its task moves to `blocked_on_user` with the outcome `asked`"), "Ideas: the dispatcher and sub-orchestrators" ("A worker that waits holds no process"), "Ledger" (the `questions` table), "The task state machine" (the row `blocked_on_user`), FM-3; `docs/specs/fleetd/preservation-04-behavior.md` (the wait rows).

## Goal

A declared wait no longer holds a task for days. Each wait of an interactive persona has a time limit `max_wait_s` (1 h by default) on the fleet clock. A question wait points at a live row of the new `questions` table, and a review wait points at a live review task. A wait past its limit, or with no live row behind it, ends, and the stall ladder judges the worker again. An answer to the question of a `blocked_on_user` task moves it to `ready`, and the next claim gets the question and the answer.

## Requirements

- **FM-3** (the wait part): an interactive persona's waits are capped by `max_wait_s` and need something live behind them: a question row, a review task, or (the background wait, MAP.md Q23) a child process of the agent. Today a question wait lasts 7 days and a background wait 24 h.
- The `questions` table of the Phase 2 gate: 1 row for each worker question, with an owner, a state and a time limit `due_at` on the fleet clock (`question_limit_s`). The escalation of a question past its limit, and the question flow of the sub-orchestrator, are RR-12 (Phase 6).

## Context

- Read `docs/tickets/fleetd-p2/MAP.md`, section "Open questions", first (Q10, Q13 and Q23 apply here).
- Today (`crates/horch-core/src/taskledger/workers.rs`): `set_wait` (`:729`), `clear_wait` (`:750`), `waiting` (`:765`, which ends a wait past its `deadline`), the reviewers wait (`:683` to `:727`), the background wait of `horch ledger progress "waiting: ..."` (24 h, `:418`), and `sync_questions` (`:881`), which projects the open QUESTION mail of a role into a question wait of 7 days. The watchdog counts each wait as progress (`watchdog.rs`, `waiting_note`).
- The columns are from ticket 02: `worker_waits.due_at` and `worker_waits.question_id`; the `questions` table.
- The rules (each worker of Phase 2 is interactive, MAP.md Q10):
  - Each `set_wait` writes `due_at` = the fleet clock + `max_wait_s` of the worker's profile (ticket 06). A wait keeps the earlier of its `deadline` of today and its `due_at`. The stall ladder (ticket 09) counts a wait as activity only until `due_at` (`waiting_until`).
  - A question wait: `sync_questions` writes or keeps 1 `questions` row for each open QUESTION of the worker, and sets `question_id`. The row: `id` = `<attempt id>/Q<n>`, `task_id` = the worker's task, `feature_id` = the run id (MAP.md Q13), `owner` = the orchestrator of the run (the owner before Phase 6, "Before Phase 6, a run stands for its idea and its feature"), `state` `open`, `due_at` = the fleet clock + `question_limit_s` (ticket 05), `asked_at`. When the mail question is answered (today's `close_acted_questions` and the answer by id), the row goes to `answered` with `answered_at`, and the wait ends.
  - A reviewers wait points at its live review tasks (today's `task_ids`). With no live review task, it ends (today's rule `holds_for_finish` stays).
  - A background wait (`horch ledger progress "waiting: ..."`; MAP.md, Q23, the orchestrator's answer): it stays, because an interactive worker of today ends its turn while a long command runs. It is valid only while a child process of the worker's agent is alive, and at most `max_wait_s` on the fleet clock. When the child process is gone, the wait ends at once and the stall ladder judges the worker. The fact: a member of the process group of the recorded agent, other than the agent itself, is alive (`group_members` in `crates/horch-platform/src/process.rs:57`; part 1 of the pass reads it, ticket 14). `NotSupported` (Windows) is an unknown fact: it does not end the wait, and the cap `max_wait_s` still applies. Phase 7 removes the background wait with exec mode.
  - The pure rule of wait validity is in `task_execution/domain/waits.rs`, for example `fn waiting_until(wait, facts) -> Option<u64>`: the fleet clock at which the wait stops counting as activity, or none when it is not valid now. Ticket 09 reads it as `waiting_until`.
  - A wait past `due_at`, or a question wait whose row is not `open`, is cleared on the next read, with an event of kind `wait_ended` and the cause (`max_wait_reached` or `question_closed`).
- `blocked_on_user` (rung 5 of ticket 09 moves a task there when its attempt exits with an open question): when its question row goes to `answered`, the task moves to `ready` through `transition()`, cause `question_answered`. The claim of that task (`claimed`, `crates/horch-core/src/taskledger/tasks.rs:1178`) prints the question and its answer after the ticket snapshot, so a fresh worker starts from the question, the answer and the committed work in the same worktree.
- The SQL of `questions` is in `ideas/adapters/questions_store.rs` (`questions` is a table of `ideas`, ticket 02). The SQL of `worker_waits` that fleetd reads is in `task_execution/adapters/waits_store.rs`. `workers.rs` (the taskledger of today) calls the `ideas` application function to write a question row.
- The fleet clock: `fleetd.fleet_clock_s` (Phase 1). A command that runs with no `fleetd` row reads 0 (the schema of Phase 1 writes the row).

## Steps

1. Write the tests first. Each fails before the code exists:
   - In `workers.rs`: `wait_gets_due_at_max_wait_s_on_the_fleet_clock`, `wait_past_due_at_ends_with_an_event`, `question_wait_points_at_its_open_questions_row`, `question_wait_ends_when_its_row_is_answered`, `reviewers_wait_with_no_live_review_ends`, `background_wait_is_capped_at_max_wait_s`.
   - In `domain/waits.rs`: `background_wait_ends_when_the_agent_child_is_gone`, `background_wait_holds_while_a_child_is_alive_up_to_max_wait_s`, `unknown_child_fact_keeps_the_wait_to_its_cap`.
   - In `ideas/application/questions.rs`: `open_question_mail_writes_1_row_with_owner_and_due_at`, `second_sync_keeps_1_row_for_1_question`, `answer_moves_the_row_to_answered`.
   - In `tasks.rs`: `answer_moves_a_blocked_on_user_task_to_ready`, `claim_of_an_answered_task_prints_the_question_and_the_answer`.
2. Write the code. Run the Check and the guard tests. Commit.

## Constraints

@@RULES@@

## Done when

The Check passes. No wait holds a worker past `max_wait_s` of fleet time, each question wait has 1 live `questions` row behind it, and an answer frees a `blocked_on_user` task.
