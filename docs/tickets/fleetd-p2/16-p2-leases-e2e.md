# 16: The e2e tests of Phase 2: a stopped worker is caught within 1 lease, retries are bounded, a cancel stops the process first, and 2 workers never hold 1 task

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** quality
**Review:** full
**Check:** `cargo test -p horch-e2e --test p2_leases && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-e2e/tests/p2_leases.rs`

Spec: `docs/specs/fleetd/design.md`, RR-1, RR-2, RR-6, FM-14, "Supervision, liveness and reconciliation" ("The task state machine", "A cancel", "Stall ladder"), "Migration plan" item 4 (*Gate:* and Tests: "a task in `running`, `blocked_on_user` or `waiting_quota` is cancelled, and the process of a live attempt is gone before the task is `cancelled`").

## Goal

The new file `crates/horch-e2e/tests/p2_leases.rs` proves the rules of the Phase 2 gate end to end, through the `horch` binary and the pass of fleetd, with temporary folders.

## Requirements

- **RR-1**: a worker stopped with `kill -STOP` is caught within 1 lease (120 s of the fleet clock).
- **RR-2**: an expired lease returns the task to `ready` or escalates it. The task never stays assigned.
- **RR-6**: a task whose attempts each fail reaches `escalated` after `max_attempts`, not more attempts.
- The cancel tests of the gate: a task in `running`, in `blocked_on_user` and in `waiting_quota` is cancelled, and the process of a live attempt is gone before the task is `cancelled`.
- **FM-14**: a new attempt of a task never starts while a process of its older attempt is alive, and the `leases` table has at most 1 row for a task.

## Context

- Read `docs/tickets/fleetd-p2/MAP.md`, section "Open questions", first (Q6, Q11, Q15, Q17 and Q22 apply here).
- Use the harness and the helpers of `crates/horch-e2e/tests/p0_liveness.rs` as the model: a `Harness` with its own ledger and git repository, a stand-in agent (a `sleep` process, killed on drop), a worker row with that agent, and `horch ledger watch --once` for 1 pass. Copy the helpers that you need into the new file. Do not change `p0_liveness.rs` (ticket 15 owns it).
- Move the fleet clock with `HORCH_NOW` steps and a large `pass_period_s` in the `fleet.toml` of the test (a pass adds at most 2 pass periods). No test sleeps for a lease.
- `kill -STOP` and `kill -CONT` exist only on Unix. Put the RR-1 test under `#[cfg(unix)]`. On Windows the RR-1 row is shown by the heartbeat rule of ticket 08 (a stale heartbeat stops the renewal); ticket 18 names this in the gate file.
- The tests, each in this file:
  - `rr1_stopped_worker_is_caught_within_1_lease`: claim, `kill -STOP` the agent (its supervisor checkins stop: stop the stand-in supervisor too, or start the worker with no supervisor), passes until the fleet clock is 120 s past the last renewal; the task is `stalled` or past it. `kill -CONT` and stop the process on drop.
  - `rr2_expired_lease_never_leaves_the_task_assigned`: after the expiry, the task is `ready` (or `escalated` at the last attempt), its `leases` row is gone, and its attempt ended.
  - `rr6_task_that_fails_each_attempt_escalates_after_max_attempts`: each attempt exits with no result; after the backoffs of rung 6 (`not_before`) and the 1 replan, the task is `escalated` and 1 needs-you item names it. The attempt count is never above `max_attempts` + the replan.
  - `cancel_of_a_running_task_stops_its_process_before_the_task_is_cancelled`: `horch task cancel`; at the moment the task is `cancelled`, the agent process is gone (read the event times of the attempt end and of the task move, and check the PID with `horch_core::procid::alive`).
  - `cancel_of_a_blocked_on_user_task_is_immediate` and `cancel_of_a_waiting_quota_task_is_immediate`: no process; the task is `cancelled` in the same command.
  - `fm14_no_second_attempt_starts_while_the_old_process_lives`: an attempt is ended (lease expiry) while its process is still alive; the pass stops it first, and the next claim or restart starts only after the process is gone. At no time does `leases` have 2 rows for the task.
- Read the ledger with `Store::open` as `p0_liveness.rs` does. Read the stored task words (MAP.md Q3), not the shown words.

## Steps

1. Write the tests. Run them on the base of this ticket (the code of tickets 08 to 14 is there). Each passes. If a test fails, find the cause: a fault in the code of another ticket goes to the orchestrator as a QUESTION with the test name and the output.
2. Run each test 3 times to find a flaky test. Run the Check and the guard tests. Commit.

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

The Check passes. Each test of the list is in `p2_leases.rs` and passes 3 times in a row on this Mac.
