# 24: horch install and horch self-update restart fleetd; fleetd restarts itself on a changed binary

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch && cargo test -p horch-e2e --test p0_supervision --test update --test expected_failures`
**Own:** `crates/horch/src/cmd/install.rs`, `crates/horch/src/cmd/selfupdate.rs`, `crates/horch/src/cmd/fleetd.rs`, `crates/horch-e2e/expected-failures/p0_supervision.txt`

Spec: `docs/specs/fleetd/design.md`, OR-6, FM-34, "Architecture and roles" (Supervision tree and restart policy), "Migration plan" (What each gate needs: "the quota probes and the self-update restart (today)"); `docs/specs/fleetd/preservation-04-behavior.md` row S10; `docs/specs/fleetd/preservation-06-wiring-and-platform.md` row S14.

## Goal

`horch self-update` restarts the running fleetd on the new binary at once. `horch install` installs the service definition (system mode only) and restarts a fleetd that runs another binary. A running fleetd whose binary file changed starts the new binary between 2 passes and exits; the new one takes the lock. The red test of FM-34 turns green.

## Requirements

- **OR-6** (part): `horch self-update` restarts the running daemon on the new binary at once. The CLI part (an older daemon is restarted) is ticket 23; the schema part is ticket 03.
- **FM-34**: daemon and CLI of different versions: prevented by the restart on the new binary. The red test `fm34_daemon_restart_on_a_new_binary_keeps_the_run` in `crates/horch-e2e/tests/p0_supervision.rs` turns green: remove its line, and only its line, from `crates/horch-e2e/expected-failures/p0_supervision.txt` (read `crates/horch-e2e/expected-failures/README.md`). The line of `fm7_killed_watchdog_does_not_stop_the_reconcile` stays for ticket 25.
- Preservation 04 row S10: a supervisor whose binary file changed is stopped by its exact PID and restarted on the new binary, and `horch install` restarts it. Today this is `taskledger_watch.rs:450` and `:490` for the watchdog.

## Context

- Read `docs/tickets/fleetd-p1/MAP.md`, section "Open questions", first. It records each choice of this phase (Q1 to Q21), and each ticket follows it.
- Phase P (`docs/tickets/fleetd-pp/`) builds `crates/horch-platform`. Its concern tickets propose function names. The module doc of each concern file in `crates/horch-platform/src/` gives the final name. Read that doc before you call a platform function. A name in this ticket is the proposed name.
- Today: `install` (`crates/horch/src/cmd/install.rs:29`) calls `super::taskledger_watch::restart_if_stale(ctx, &target)` at `:49`. `self_update` (`crates/horch/src/cmd/selfupdate.rs:404`) ends with `restart` (`:370`), which replaces the process of the CLI. `crates/horch-core/src/taskledger/watchdog.rs:649` `runs_other_binary` and `:661` `binary_replaced` compare `ExeIdentity` values (`crates/horch-core/src/exeid.rs`).
- Keep the call of `restart_if_stale` at `install.rs:49` in this ticket: old watchdogs still run until ticket 25, and `install_restarts_the_watchdog_of_an_active_run_on_the_installed_binary` in `crates/horch-e2e/tests/watchdog_liveness.rs` pins it. Ticket 25 removes it. Add the fleetd restart beside it.
- The restart of fleetd is the restart function of ticket 23 in `crates/horch/src/cmd/fleetd.rs` (service mode or stand-in mode, MAP.md, Q8). Use it in each place. Do not write a second restart path.
- `horch install`: in system mode, it installs the service definition through `service::install` of ticket 06, and then restarts fleetd when the `fleetd` row names a live fleetd that runs another binary (`is_older` of ticket 21). In stand-in mode, it writes no service and only restarts. No test registers a real service (build rule 1): a test uses a temp ledger, so the mode is stand-in.
- `horch self-update`: after the new binary is in place, and before `restart` replaces the CLI process, restart the running fleetd on the new binary. If no fleetd row names a live fleetd, do nothing.
- The changed binary inside fleetd: at the end of each pass (after the heartbeat), fleetd compares its own `ExeIdentity` with the file at its path. When the file changed, fleetd starts the new binary through the restart path (the launcher of ticket 16, detached), and exits between passes, never inside a pass. The new fleetd waits for the lock (ticket 14) and takes a new generation. The test of FM-34 waits 10 s (2 passes) for the new PID in the `fleetd` row.
- The test of FM-34 uses a changed binary as its trigger (`install_copy` writes a new copy with a new mtime). Keep that trigger. Do not change the test file; it is not in `Own`.

## Steps

1. Run `cargo test -p horch-e2e --test p0_supervision` first: `fm34_daemon_restart_on_a_new_binary_keeps_the_run` passes as an expected failure.
2. Write the tests first. Each fails before the code exists:
   - In `crates/horch/src/cmd/fleetd.rs`: `pres_04_s10_changed_binary_restarts_between_passes` (a fake pass loop with a changed `ExeIdentity` starts the new binary once, after the heartbeat, and ends the loop).
   - In `crates/horch/src/cmd/install.rs`: `install_writes_no_service_in_stand_in_mode` and `install_restarts_a_fleetd_of_another_binary`.
   - In `crates/horch/src/cmd/selfupdate.rs`: `self_update_restarts_the_running_fleetd_before_it_replaces_itself` and `self_update_with_no_live_fleetd_restarts_nothing`.
3. Write the code. Remove the line of `fm34_daemon_restart_on_a_new_binary_keeps_the_run` from `p0_supervision.txt`.
4. Run the Check.

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

The Check passes. `fm34_daemon_restart_on_a_new_binary_keeps_the_run` passes off the list. `horch self-update`, `horch install` and a changed binary each lead to 1 fleetd on the new binary.
