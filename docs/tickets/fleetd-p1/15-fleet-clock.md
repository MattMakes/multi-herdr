# 15: The fleet clock: 1 count of seconds that stops while the machine sleeps and while fleetd is down

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** light
**Check:** `cargo test -p horch-core --lib fleetd::fleet_runtime`
**Own:** `crates/horch-core/src/fleetd/fleet_runtime/domain/clock.rs`, `crates/horch-core/src/fleetd/fleet_runtime/adapters/clock_store.rs`

Spec: `docs/specs/fleetd/design.md`, "Supervision, liveness and reconciliation" (One fleet clock for each timer. A sleep fires no deadline), "Ledger" (table `fleetd`: `fleet_clock_s`, `clock_ticked_at`).

## Goal

fleetd keeps 1 fleet clock in the `fleetd` row. Each pass adds the time since the last pass, at most 2 pass periods (10 s). Each deadline of Phase 1 (the time limit of a job, the backoff of a job, the start timeout of an attempt) is a value on this clock.

## Requirements

- The fleet clock of the design (amendment A25). It is a mechanism of the gate row of Phase 1 ("the fleet clock (1)"); Phase 2 uses it for leases.

## Context

- Read `docs/tickets/fleetd-p1/MAP.md`, section "Open questions", first. It records each choice of this phase (Q1 to Q21), and each ticket follows it.
- Phase P (`docs/tickets/fleetd-pp/`) builds `crates/horch-platform`. Its concern tickets propose function names. The module doc of each concern file in `crates/horch-platform/src/` gives the final name. Read that doc before you call a platform function. A name in this ticket is the proposed name.
- The rule: `tick(clock_s, last_awake, now_awake, pass_period_s) -> clock_s + min(elapsed, 2 * pass_period_s)`. The elapsed time is awake time (`awake::now()` of ticket 05), so a sleep adds nothing; a restart adds at most 2 periods for the downtime. Readings of different boots add 0.
- `domain/clock.rs` holds the pure rule and the type `FleetSeconds`. `adapters/clock_store.rs` reads and writes `fleet_clock_s` and `clock_ticked_at` of the `fleetd` row in the transaction of the pass, and keeps the last awake reading in memory of the process (a restart starts with no reading and adds 0 on its first tick).
- The row exists after the first start (ticket 14).

## Steps

1. Write the tests first, in `domain/clock.rs` and `adapters/clock_store.rs`. Each fails before the code exists:
   - `fleet_clock_does_not_advance_during_sleep_or_downtime` (from `ddd/08-code/implementation-plan.md`, Slice 1): a gap of 1 h adds at most 10 s.
   - `tick_adds_the_elapsed_awake_time_below_2_periods`.
   - `readings_of_different_boots_add_0`.
   - `first_tick_after_a_restart_adds_0`.
   - `tick_writes_the_clock_in_the_transaction_of_the_pass`.
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

The Check passes. A sleep and a downtime add at most 2 pass periods to the fleet clock.
