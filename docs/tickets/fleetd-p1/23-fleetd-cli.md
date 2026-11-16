# 23: horch fleetd, horch fleetd check, horch job requeue and horch job run; the fleetd check before each command

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch && cargo test -p horch-core --test architecture_doc && cargo test -p horch-e2e --test p0_fleetd_hang --test expected_failures`
**Own:** `crates/horch/src/cmd/fleetd.rs`, `crates/horch/src/cmd/jobcmd.rs`, `crates/horch/src/cmd/mod.rs`, `crates/horch/src/main.rs`, `docs/architecture.md`, `crates/horch-e2e/expected-failures/p0_fleetd_hang.txt`, `crates/horch-e2e/tests/p0_fleetd_hang.rs`

Spec: `docs/specs/fleetd/design.md`, "Architecture and roles" (row fleetd; Supervision tree and restart policy; One fleetd for each ledger), PL-1, PL-5, FM-12, FM-34, OR-6, "Migration plan" item 3 ("Remove the lazy `ensure` start"), "Preservation analysis" (Contract surface: `horch fleetd`); `docs/specs/fleetd/preservation-06-wiring-and-platform.md` row S14; `ddd/08-code/fleet-runtime/design.md` (Composition root `crates/horch/src/cmd/fleetd.rs`).

## Goal

`horch fleetd` runs the daemon: it takes the lock, starts its generation, and runs 1 pass every `pass_period_s`. `horch fleetd check` restarts a hung or older fleetd. `horch job requeue <job>` and the internal `horch job run <id>` exist. Each `horch` command makes the same check as `horch fleetd check` before it runs. The red test of FM-12 turns green. Ticket 25 removes the lazy start of the watchdog.

## Requirements

- **FM-12**: a hung fleetd is restarted: `horch fleetd check` restarts the service when the heartbeat is older than 30 s of awake time, and each `horch` CLI command makes the same check. Each restart stops the old process by its PID and start time and waits until the lock is free.
- **OR-6** (part): a CLI that finds an older daemon restarts it.
- **PL-1** (part): `horch fleetd` builds and runs on macOS, Linux and Windows (CI).
- The red test `fm12_hung_fleetd_is_restarted_on_each_os` in `crates/horch-e2e/tests/p0_fleetd_hang.rs` turns green: remove its line from `crates/horch-e2e/expected-failures/p0_fleetd_hang.txt` (read `crates/horch-e2e/expected-failures/README.md`).

## Context

- Read `docs/tickets/fleetd-p1/MAP.md`, section "Open questions", first. It records each choice of this phase (Q1 to Q21), and each ticket follows it.
- Phase P (`docs/tickets/fleetd-pp/`) builds `crates/horch-platform`. Its concern tickets propose function names. The module doc of each concern file in `crates/horch-platform/src/` gives the final name. Read that doc before you call a platform function. A name in this ticket is the proposed name.
- The service mode (MAP.md, Q8): the service manager of the OS is used only when the ledger is the default ledger of the OS user (`<service::os_user_home()>/.horch/ledger.sqlite3` of ticket 06, never `$HOME`) and `FLEET_ROOT` is not set. Else the mode is stand-in: a restart stops the old fleetd by the PID and start time of the `fleetd` row (platform signal concern, `stop_grace_s`), waits until the lock is free (ticket 14), and starts `horch fleetd` detached through the launcher of ticket 16, with the env of the fleet (`HORCH_LEDGER_DB`, `HORCH_STATE_DIR`, `HORCH_DATA_DIR`, `FLEET_ROOT`). The red test runs in stand-in mode. No test registers a real service (build rule 1). Write the test `service_mode_is_stand_in_for_a_ledger_outside_the_os_user_home`.
- `horch fleetd check` (exit 0 in each case that it handles, as the test needs): no `fleetd` row -> nothing ("Remove the lazy `ensure` start": the check does not start a fleetd that never ran; the service starts it); a row with a dead PID -> start; a hung heartbeat (`is_hung` of ticket 21) or an older binary (`is_older`) -> restart; else nothing. It prints what it did.
- The CLI check: `crates/horch/src/main.rs:822` calls `cmd::taskledger_watch::ensure(&ctx)` after each watched command. Add the check of `horch fleetd check` before the command. Keep the call of `ensure` in this ticket: tests that this ticket does not own pin it (`crates/horch/tests/w3_ledger_cli_pins.rs:549`, `crates/horch-e2e/tests/taskledger.rs:1370`, `crates/horch-e2e/tests/watchdog_liveness.rs:263`). Ticket 25 removes it with the old loop. `HORCH_NO_WATCHDOG=1` keeps its meaning: no check and no start (MAP.md, Q17). The check is quiet unless it restarts.
- `horch fleetd`: the composition root. Wire each adapter of tickets 11 to 22 to its port, and the respawn function of `crates/horch/src/cmd/recipes.rs` to the port of ticket 22. The loop: 1 pass, then sleep until the next period; SIGTERM ends it between passes, never inside a pass (row S7). The changed-binary restart is ticket 24.
- `horch job requeue <job>` calls `requeue` of ticket 17 and prints the new run. `horch job run <id>` calls `run_job` of ticket 17; hide it from help.
- New CLI files need rows in `docs/architecture.md` (`every_cli_file_has_a_row_in_the_architecture_doc`): `fleetd.rs` and `jobcmd.rs`, and the 2 commands in the command list.
- The test file `p0_fleetd_hang.rs` stays as it is. Change it only if its setup is wrong for the interface of this ticket, and say why in the commit.

## Steps

1. Run `cargo test -p horch-e2e --test p0_fleetd_hang` first: the test passes as an expected failure with the reason `unrecognized subcommand 'fleetd'`.
2. Write the tests first, in `crates/horch/src/cmd/fleetd.rs`: `check_with_no_row_starts_nothing`, `check_restarts_a_hung_fleetd_by_pid_and_start_time`, `check_restarts_an_older_fleetd`, `check_leaves_a_fresh_fleetd`, `service_mode_is_stand_in_for_a_ledger_outside_the_os_user_home`. In `crates/horch/src/main.rs` tests: `a_horch_command_runs_the_fleetd_check_before_it_runs`.
3. Write the code. Remove the line of `fm12_hung_fleetd_is_restarted_on_each_os` from `p0_fleetd_hang.txt`.
4. Run the Check. The Windows and Linux results of `fm12` come from CI (the file has no `#![cfg(unix)]`).

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

The Check passes, `fm12_hung_fleetd_is_restarted_on_each_os` passes off the list, and each `horch` command runs the fleetd check.
