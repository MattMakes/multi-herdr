# 20: The reap path of task-execution: I1 (process part), I4 and I5

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-core --lib fleetd::task_execution && cargo test -p horch-core --lib taskledger && cargo test -p horch-e2e --test taskledger`
**Own:** `crates/horch-core/src/fleetd/task_execution/domain/task.rs`, `crates/horch-core/src/fleetd/task_execution/application/commands.rs`, `crates/horch-core/src/fleetd/task_execution/adapters/sqlite.rs`, `crates/horch-core/src/taskledger/tasks.rs`, `crates/horch-core/src/taskledger/workers.rs`

Spec: `docs/specs/fleetd/design.md`, "Ledger" (invariants I1, I4, I5), FM-4, FM-22, the part of FM-13 that Phase 1 can do, "Supervision, liveness and reconciliation" (Liveness comes from outside the model: the Process signal); `ddd/08-code/task-execution/aggregate-canvas-task.md` (T4); `docs/specs/fleetd/preservation-04-behavior.md` rows S7 and S8.

## Goal

task-execution decides, for each task that runs and each process under the fleet: an attempt whose process is dead ends, and its task is queued again with the worktree handling of today (I1, process part); a worker never gets a second attempt (I4); a process or pane that a row names and that no live attempt owns is an orphan to stop (I5). Each decision is rows in 1 transaction; each stop is an effect after the commit.

## Requirements

- **FM-4**: an agent process that dies is seen by PID and start time on the next pass, within 30 s.
- **FM-22**: an orphan process or pane is stopped: process-group stop and exact pane close; I5 stops anything not owned by a live attempt.
- **FM-13** (the Phase 1 part): after a herdr restart, the dead PIDs and missing panes end their attempts, and the tasks are queued again with the worktree handling of today. The rescue of worktrees is Phase 3.

## Context

- Read `docs/tickets/fleetd-p1/MAP.md`, section "Open questions", first. It records each choice of this phase (Q1 to Q21), and each ticket follows it.
- Phase P (`docs/tickets/fleetd-pp/`) builds `crates/horch-platform`. Its concern tickets propose function names. The module doc of each concern file in `crates/horch-platform/src/` gives the final name. Read that doc before you call a platform function. A name in this ticket is the proposed name.
- The orphan scope (MAP.md, Q12): a process or pane is "under the fleet" when a `workers`, `attempts` or `jobs` row names it (`agent_pid`, `supervisor_pid`, `pane_id`, `attempts.pid`, `jobs.pid`). A pane or process that no row names is not touched in Phase 1.
- The reap of today: `reap_dead` in `crates/horch/src/cmd/taskledger_watch.rs:305`, `stop::reap` in `crates/horch-core/src/taskledger/stop.rs:1107`, `tasks::requeue` (`tasks.rs:1429`) and the hold at `max_attempts` (preservation 04 L4). Row S7: a reap ends the worker row and requeues the task in 1 transaction before any signal. Row S8: a stop reads PID and start time from the row, sends TERM, waits, then KILL only to the same process, then closes the exact pane. Keep both.
- Use cases in `application/commands.rs` (`ddd/08-code/implementation-plan.md`, Slice 1): `EndAttempt` (the attempt to `ended` with its outcome, the worker to its end status, through `transition()`), `ScheduleRetry` (today: `tasks::requeue`, with the attempt count of today), `EscalateTask` (today: the hold `held_reason` at `max_attempts`). Each returns the effects to do after the commit: `Stop { pid, pid_start, pgid, pane }`.
- I4 at the claim: `tasks::claim` (`tasks.rs:934`) refuses a worker that has a live attempt. Today `workers_one_working_per_task` limits 1 working worker per task; I4 is the other side.
- The claim writes 1 attempt row in `live` for the task and the worker (MAP.md, Q6), with the agent PID and its start time when the worker row has them, through `transition()`.
- The decision is pure in `domain/task.rs`: facts in (rows and process facts), decisions out. The pass of ticket 21 calls it.

## Steps

1. Write the tests first. Each fails before the code exists:
   - `i1_running_task_with_a_dead_process_ends_its_attempt_and_is_queued_again`.
   - `i4_dispatch_refuses_a_worker_with_a_live_attempt` (Slice 1).
   - `i5_orphan_process_or_pane_is_stopped` (Slice 1): a live `sleep` named by an ended worker row gives 1 `Stop` effect.
   - `pres_04_s7_reap_requeues_in_1_transaction_before_the_signal` (Slice 1).
   - `pres_04_s8_stop_signals_only_the_recorded_pid_of_this_host` (Slice 1).
   - `claim_writes_1_live_attempt`.
   - `process_that_no_row_names_is_not_touched`.
2. Write the code. Run the Check.

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

The Check passes. A dead agent ends its attempt and requeues its task in 1 transaction; an orphan named by a row gives 1 stop; a process that no row names is left alone.
