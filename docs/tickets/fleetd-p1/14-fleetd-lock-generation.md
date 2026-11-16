# 14: The fleetd lock, the generation of the fleetd row, and the first start

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-core --lib fleetd::fleet_runtime && cargo test -p horch-core --test fleetd_lock`
**Own:** `crates/horch-core/src/fleetd/fleet_runtime/domain/generation.rs`, `crates/horch-core/src/fleetd/fleet_runtime/adapters/fleetd_row.rs`, `crates/horch-core/tests/fleetd_lock.rs`

Spec: `docs/specs/fleetd/design.md`, "Architecture and roles" (One fleetd for each ledger), "Ledger" (table `fleetd`; rule 1: "Each write transaction first checks that the generation in the `fleetd` row is its own, and fleetd exits when it is not"), FM-11, FM-12, RR-7; "What a release does to a run in flight", row Phase 1 ("The `fleetd` row gets generation 1 and a fleet clock of 0").

## Goal

1 fleetd runs for 1 ledger. At start, fleetd takes the lock `<fleet root>/run/fleetd.lock`, then adds 1 to the generation of the `fleetd` row and writes its PID and start time. A second fleetd that cannot take the lock exits at once. A fleetd whose generation is not the one in the row exits with no effect.

## Requirements

- **FM-12** (part): "Each restart stops the old process by its PID and start time and waits until the fleetd lock is free before it starts the new one, so 2 fleetd processes never run at once." This ticket gives the lock, the wait and the row.
- **RR-7** (part): a restart rebuilds from the ledger; the generation makes an old process harmless.
- The backfill of the release table: the first start writes the row with generation 1 and `fleet_clock_s` 0.

## Context

- Read `docs/tickets/fleetd-p1/MAP.md`, section "Open questions", first. It records each choice of this phase (Q1 to Q21), and each ticket follows it.
- Phase P (`docs/tickets/fleetd-pp/`) builds `crates/horch-platform`. Its concern tickets propose function names. The module doc of each concern file in `crates/horch-platform/src/` gives the final name. Read that doc before you call a platform function. A name in this ticket is the proposed name.
- The platform lock: `lock::FleetdLock::try_take(path)` of ticket 05 (read the module doc of `crates/horch-platform/src/lock.rs` for the final name). The fleet root comes from ticket 04 (`ctx.paths.fleet_root`).
- The start, in this order: take the lock (exit at once with a clear message and exit code 0 when another fleetd holds it, so a service manager does not loop on errors; say so in the module doc), then in 1 transaction read the `fleetd` row, write `generation + 1` (1 when there is no row; `fleet_clock_s` 0 for a new row, else keep it), `pid`, `pid_start` (the start time from the platform process concern), `started_at`. Return the `Generation` value.
- The guard: ticket 11 gives `transition(.., guard: Option<Generation>)` and the error `GenerationLost`. Give the function `check_generation(tx, g)` that the pass calls first in its transaction (part 2 of the pass, "check_generation(tx)"), and the rule: on `GenerationLost`, fleetd exits with no effect (ticket 21 calls the exit).
- `wait_until_free(path, deadline)`: the restart path of ticket 23 stops the old fleetd by its PID and start time (the platform signal concern), then calls this, then starts the new one.
- Tests that need 2 processes start the test binary itself as a child with an env flag. Use temp folders (build rule 1).

## Steps

1. Write the tests first, in the new file `crates/horch-core/tests/fleetd_lock.rs`. Each fails before the code exists:
   - `first_start_writes_generation_1_and_a_fleet_clock_of_0`.
   - `each_start_adds_1_to_the_generation`.
   - `second_fleetd_exits_at_once_while_the_lock_is_held`.
   - `second_fleetd_exits_on_a_generation_mismatch` (from `ddd/08-code/implementation-plan.md`, Slice 1): a process with generation 1 cannot write after a start with generation 2.
   - `lock_is_free_after_the_holder_is_killed_and_a_new_start_takes_it`.
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

The Check passes. 2 fleetd processes of 1 ledger never both pass the start, and an old generation writes nothing.
