# 22: Move the duties of today's watchdog into the pass of fleetd

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-core --lib taskledger && cargo test -p horch-core --lib fleetd && cargo test -p horch-core --test arch_scan && cargo test -p horch && cargo test -p horch-e2e --test taskledger --test watchdog_liveness`
**Own:** `crates/horch-core/src/fleetd/fleet_runtime/adapters/watchdog_duties.rs`, `crates/horch-core/src/taskledger/watchdog.rs`, `crates/horch/src/cmd/taskledger_watch.rs`, `crates/horch/src/cmd/recipes.rs`, `crates/horch-core/tests/arch_scan.rs`

Spec: `docs/specs/fleetd/design.md`, "Supervision, liveness and reconciliation" (introduction: the 3 reasons why the watchdog misses things), "Migration plan" item 3 ("Move the watchdog's pass into a long-lived daemon"; "It acts in code (stop by recorded PID, requeue, respawn) instead of only mailing the orchestrator"), FM-7; `docs/specs/fleetd/preservation-04-behavior.md` (the watchdog rows S-series and L-series that name `watchdog.rs`).

## Goal

Each duty of the pass of today's watchdog runs in the pass of fleetd, for each active run: the check of each worker (`check_run`), the reap of a stalled worker by its stall rounds, the flag of an unclaimed spawn, the re-wake of startable tasks, the wake for reviews that are in, and the respawn of a gone orchestrator. Each duty writes rows in part 2 and does its effect in part 3.

## Requirements

- **FM-7** (part): the duties act in code on each pass; none depends on a model reading mail. The wake mail to the orchestrator stays where today's behavior needs it (the outbox of `crates/horch-core/src/taskledger/outbox.rs`), and the actions (stop by recorded PID, requeue, respawn) are code.

## Context

- Read `docs/tickets/fleetd-p1/MAP.md`, section "Open questions", first. It records each choice of this phase (Q1 to Q21), and each ticket follows it.
- Phase P (`docs/tickets/fleetd-pp/`) builds `crates/horch-platform`. Its concern tickets propose function names. The module doc of each concern file in `crates/horch-platform/src/` gives the final name. Read that doc before you call a platform function. A name in this ticket is the proposed name.
- Today: `pass` (`crates/horch/src/cmd/taskledger_watch.rs:123`), `reconcile_at` (`:187`: unclaimed spawns, startable re-wake), `orchestrator_at` (`:220`: the respawn through `recipes::respawn_orchestrator`, `crates/horch/src/cmd/recipes.rs`), `act_on` (`:275`) and `reap_dead` (`:305`), with the rules in `crates/horch-core/src/taskledger/watchdog.rs` (`check_run`, `unclaimed_spawns`, `flag_unclaimed`, `startable_rewake`, `reviews_waiting`, `stalled_blockers`, `attempt_held`).
- The pass of ticket 21 has the port `RunDuties`. Implement it in `adapters/watchdog_duties.rs` with the rules of `watchdog.rs`. The respawn of the orchestrator is in the `horch` crate, so the port takes it as an effect: part 2 writes an attempt in `launching` with `task_id` null and the orchestrator worker (MAP.md, Q6), and part 3 calls the respawn function that the composition root of ticket 23 gives. Move the code that both the old loop and the pass need out of `taskledger_watch.rs` into `watchdog_duties.rs` or `watchdog.rs`; ticket 25 removes the old loop.
- The reap of a dead process is ticket 20 (`finish_exited`). The reap of a stalled worker (stall rounds of today, `check_every_s`, `stall_rounds` of the run) stays until Phase 2 replaces it with leases: keep its numbers.
- The status writes of `watchdog.rs` go through `transition()`. Remove the exception of `watchdog.rs` from `fitness_status_writes_go_through_transition` in `crates/horch-core/tests/arch_scan.rs` (ticket 13 named it).
- Keep the behavior that the e2e tests of today prove: `crates/horch-e2e/tests/taskledger.rs`, `watchdog_liveness.rs`, `doctor_extras_watchdog.rs`. The old `horch ledger watch` still runs in this ticket; ticket 25 points it at fleetd.

## Steps

1. Write the tests first, in `adapters/watchdog_duties.rs`. Each fails before the code exists:
   - `pass_flags_an_unclaimed_spawn_once`.
   - `pass_reaps_a_worker_past_its_stall_rounds_and_requeues_its_task`.
   - `pass_writes_a_launching_attempt_for_a_gone_orchestrator_and_respawns_it_after_the_commit`.
   - `pass_rewakes_startable_tasks_once_for_each_board_state`.
2. Move the duties. Remove the `watchdog.rs` exception of the rule.
3. Run the Check.

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

The Check passes. Each duty of today's watchdog runs in the pass, writes rows in 1 transaction, and does its effect after the commit.
