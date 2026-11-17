# 08: Leases: 1 lease for each held task, renewed only on earned activity, and the lease part of I1

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-core --lib fleetd::task_execution && cargo test -p horch-core --lib taskledger::tasks && cargo test -p horch-e2e --test taskledger && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/task_execution/domain/lease.rs`, `crates/horch-core/src/fleetd/task_execution/application/lease.rs`, `crates/horch-core/src/fleetd/task_execution/adapters/leases_store.rs`, `crates/horch-core/src/fleetd/task_execution/application/commands.rs`, `crates/horch-core/src/fleetd/task_execution/adapters/sqlite.rs`

Spec: `docs/specs/fleetd/design.md`, "Ledger" (rule 2 "Leases, not claims"; invariant I1), "Supervision, liveness and reconciliation" ("Liveness comes from outside the model", "Lease renewal is earned, not claimed", "One fleet clock for each timer", "Order inside 1 pass"), FM-3, FM-14; `ddd/08-code/task-execution/aggregate-canvas-task.md` (`renew-lease`).

## Goal

A task in `leased` or `running` always has 1 lease row with an expiry on the fleet clock. Only a pass renews it, and only from outside signals: the process is alive, the wrapper heartbeat is under `heartbeat_max_age_s` old, and there was activity within `stall_after_s`. When the fleet clock passes `expires_at`, the task moves to `stalled` in the same pass. A task in `waiting_quota` has its lease paused. The pass checks the lease part of I1 and repairs or escalates a breach.

## Requirements

- **RR-2**: a task is held by a lease with an expiry. An expired lease returns the task to ready or escalates (through `stalled` and rung 6 of ticket 10). It never stays assigned. Verified by a property test over the state machine.
- **FM-3** (the lease part): a worker that holds a task and does nothing stops earning renewals, and its lease expires within `lease_s` of the stall threshold.
- **FM-14**: the lease row is unique for each task, and a second attempt of 1 task cannot hold a lease while the first is live.
- **I1** (the lease part): each live attempt of a task has exactly 1 unexpired lease. Each `leased` or `running` task has exactly 1 attempt with a live process or pane (the process part is Phase 1, ticket 20 of `docs/tickets/fleetd-p1/`).
- **PL-4** (the lease part): a sleep does not expire a lease; the lease is a deadline on the fleet clock.

## Context

- Read `docs/tickets/fleetd-p2/MAP.md`, section "Open questions", first (Q5, Q6, Q7, Q20 and Q21 apply here).
- The tables are from ticket 02: `leases` and `attempts.active_at`. The fleet clock is `fleetd.fleet_clock_s` (Phase 1, ticket 15: `crates/horch-core/src/fleetd/fleet_runtime/adapters/clock_store.rs`).
- The numbers: `lease_s` and `stall_after_s` of the profile of the attempt's worker (ticket 06, `crates/horch-core/src/fleetd/worker_profiles/domain/profile.rs`); `heartbeat_max_age_s` of `fleet.toml` (ticket 05). The domain gets them as values.
- The signals of 1 live attempt, as facts that part 1 of the pass reads (MAP.md, Q6 and Q7):
  - the process: the `Seen` of Phase 1 (`crates/horch-core/src/fleetd/task_execution/domain/task.rs`);
  - the wrapper heartbeat: the age of `workers.last_checkin_at` (ticket 07);
  - activity: the signals of today's watchdog, `Observation` in `crates/horch-core/src/taskledger/watchdog.rs:44` (progress recorded by the agent, transcript growth, a move of the herdr state counter, the working timer). When a pass sees activity, it sets `attempts.active_at` to the fleet clock.
- The rules, pure in `domain/lease.rs`:
  - The claim (`start_attempt` in `application/commands.rs`, Phase 1 ticket 20) writes the lease in the same transaction: `expires_at` = the fleet clock + `lease_s`, `renew_basis` `claim`, and `active_at` = the fleet clock.
  - Renew: process alive, heartbeat age at most `heartbeat_max_age_s`, and `fleet clock - active_at` at most `stall_after_s` -> `expires_at` = the fleet clock + `lease_s`, `renew_basis` names the signals.
  - Expire: the fleet clock past `expires_at` -> the task moves to `stalled` through `transition()`, cause `lease_expired`. Ticket 10 (rung 6) and ticket 09 (rung 3, the stop of the process group) act on `stalled`.
  - The result wins: when the task left `running` in this pass (for example to `verifying` through `horch ledger finish`), there is no expiry. In Phase 2 a "result" is the move of the worker to `verifying` (MAP.md, Q20).
  - Paused: a task in `waiting_quota` keeps its lease row and does not expire. On its exit to `ready` the lease row goes.
  - A lease belongs only to a task in `leased`, `running`, `stalled` or `waiting_quota`. Each command of `task_execution` that moves a task out of these states deletes its lease in the same transaction. A move by another writer of Phases 1 to 4 (for example `horch ledger finish` to `verifying`) leaves a stale row, and the I1 check of the next pass deletes it. A `verifying` task has no lease (MAP.md, Q20).
  - I1 check: a live attempt of a task with no lease, or with 2, or a lease whose attempt is not live -> repair (write the missing lease with a full time, or delete the stale one) and record an event with the cause `i1_repaired`. A breach that cannot be repaired (2 live attempts for 1 task) -> 1 needs-you item (the needs-you list of Phase 1, `crates/horch-core/src/fleetd/ideas/application/needs_you.rs`).
- The application function, for example `renew_or_expire_leases(tx, clock, facts, numbers) -> Result<Vec<Moved>>`, is what ticket 14 calls in the pass after `finish_exited` ("Order inside 1 pass").
- The store: `adapters/leases_store.rs` holds the SQL of `leases` and of `attempts.active_at`. `fitness_no_context_touches_foreign_tables` has `leases` under `task_execution` (ticket 02).
- The repository has no `proptest` crate (`nfr_11_no_proptest`). Write the property test as an exhaustive walk over a small state space, or as a loop over a fixed seed with `std` only.

## Steps

1. Write the tests first. Each fails before the code exists:
   - In `domain/lease.rs`: `renews_only_with_process_heartbeat_and_activity` (each of the 3 missing gives no renewal), `expires_on_the_fleet_clock_not_the_wall_clock`, `sleep_does_not_expire_a_lease` (`ddd/08-code/implementation-plan.md`, Slice 2), `waiting_quota_pauses_the_lease`, `idle_worker_lease_expires_within_lease_s_of_the_stall_threshold` (FM-3), `rr2_no_sequence_of_facts_leaves_a_task_held_with_an_expired_lease` (the property test).
   - In `application/lease.rs`: `claim_writes_1_lease_on_the_fleet_clock`, `expired_lease_moves_the_task_to_stalled_in_the_same_pass`, `task_that_left_running_in_the_pass_is_not_expired`, `fm14_second_lease_for_1_task_is_refused`, `i1_live_attempt_with_no_lease_is_repaired_with_an_event`, `i1_lease_of_a_task_that_is_not_held_is_deleted`, `i1_two_live_attempts_of_1_task_open_1_needs_you_item`.
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

The Check passes. A claim writes 1 lease, a pass renews it only on earned activity, an expiry moves the task to `stalled`, and a sleep of the machine expires nothing.
