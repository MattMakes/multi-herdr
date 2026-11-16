# 25: Phase 1 stops the old watchdog: fleetd takes its runs, horch ledger watch points at fleetd

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch && cargo test -p horch-core --test arch_scan && cargo test -p horch-e2e --test p1_release --test p0_supervision --test taskledger --test watchdog_liveness --test expected_failures`
**Own:** `crates/horch/src/cmd/taskledger_watch.rs`, `crates/horch/src/cmd/taskledger.rs`, `crates/horch/src/cmd/install.rs`, `crates/horch/src/main.rs`, `crates/horch/tests/w3_ledger_cli_pins.rs`, `crates/horch-core/src/fleetd/fleet_runtime/adapters/mod.rs`, `crates/horch-core/src/fleetd/fleet_runtime/adapters/old_watchdog.rs`, `crates/horch-core/tests/arch_scan.rs`, `crates/horch-e2e/tests/p0_supervision.rs`, `crates/horch-e2e/tests/taskledger.rs`, `crates/horch-e2e/tests/watchdog_liveness.rs`, `crates/horch-e2e/tests/p1_release.rs`, `crates/horch-e2e/expected-failures/p0_supervision.txt`

Spec: `docs/specs/fleetd/design.md`, "Migration plan" item 3 ("**Phase 1 stops the old watchdog.** At its first start, fleetd lists each recorded watchdog, stops each one by its PID and start time, and takes its runs. `horch ledger watch` then only points at fleetd."; "Remove the lazy `ensure` start"), the table "What a release does to a run in flight" (row Phase 1), FM-7.

## Goal

At its first start, fleetd stops each recorded watchdog by its PID and start time and takes its runs. The old watchdog loop and the lazy start are gone. `horch ledger watch --once` runs 1 fleetd pass. The release test of Phase 1 passes: an update with a live worker and a live old watchdog leaves 1 control path, and the task has 1 attempt. The red test of FM-7 turns green.

## Requirements

- The release row of Phase 1 ("What a release does to a run in flight"): "fleetd stops each recorded watchdog by its PID and start time and takes its runs. A live worker goes on." Test: "A run with a live worker and a live old watchdog starts on Phase P. After the update to Phase 1, 1 control path acts, and the task has 1 attempt."
- **FM-7**: a killed supervisor does not stop the reconcile. The red test `fm7_killed_watchdog_does_not_stop_the_reconcile` in `crates/horch-e2e/tests/p0_supervision.rs` turns green: remove its line from `crates/horch-e2e/expected-failures/p0_supervision.txt`.

## Context

- Read `docs/tickets/fleetd-p1/MAP.md`, section "Open questions", first. It records each choice of this phase (Q1 to Q21), and each ticket follows it.
- Phase P (`docs/tickets/fleetd-pp/`) builds `crates/horch-platform`. Its concern tickets propose function names. The module doc of each concern file in `crates/horch-platform/src/` gives the final name. Read that doc before you call a platform function. A name in this ticket is the proposed name.
- The recorded watchdogs of today: the columns `runs.watchdog_pid` and `runs.watchdog_started` (`record_start`, `crates/horch-core/src/taskledger/watchdog.rs:766`), and the lock folder `<ledger dir>/locks/watchdog-<run>.lock` with its owner file (`watchdog::lock` `:592`, `watchdog::holder` `:713`, `LockHolder::stop` `:674`).
- The takeover goes in the new file `fleetd/fleet_runtime/adapters/old_watchdog.rs` (add its `pub mod` line to `adapters/mod.rs`). At the first start of a generation, before the first pass: for each active run, find the watchdog from the lock owner and from the `runs` columns; stop it only when the PID and the start time match (platform stop with `stop_grace_s`); clear the record; write 1 event with the cause `old_watchdog_stopped`. A recorded watchdog that is already dead is cleared with no signal. The pass of ticket 22 then does its duties for that run.
- The old loop (MAP.md, Q11): remove `watch` (`crates/horch/src/cmd/taskledger_watch.rs:324`), `ensure` (`:413`), `restart_if_stale` (`:490`) and the helpers that only they use. Keep the code that the pass of ticket 22 uses (it moved to `watchdog_duties.rs` or `watchdog.rs`).
  - `horch ledger watch --once [--force] [--json]` (`crates/horch/src/cmd/taskledger.rs:860`): when no fleetd holds the lock, it runs 1 fleetd pass in the foreground with the JSON of today. Many e2e tests use it (`crates/horch-e2e/tests/taskledger.rs`, `p0_liveness.rs:68`, `p0_rescue.rs:185`, `p0_dispatch.rs`, `watchdog_liveness.rs`). When a fleetd holds the lock, it prints that fleetd does the watch and exits 0.
  - `horch ledger watch` with no `--once` prints that fleetd does the watch, runs the fleetd check of ticket 23, and exits 0.
- Remove the call of `ensure` at `crates/horch/src/main.rs:822` (ticket 23 added the fleetd check before it), and the call of `restart_if_stale` at `crates/horch/src/cmd/install.rs:49` (ticket 24 added the fleetd restart beside it).
- Change the tests that pin the old lazy start or the old loop to the fleetd interface, with the same intent: `crates/horch/tests/w3_ledger_cli_pins.rs:549` ("started it"), `crates/horch-e2e/tests/taskledger.rs:1360` to `:1450` (the claim starts a watchdog, a watchdog is not started twice, a dead watchdog is restarted), and the tests of `crates/horch-e2e/tests/watchdog_liveness.rs` that start or replace a watchdog (`start_watchdog` `:221`, `a_watchdog_of_another_build_is_stopped_by_pid_and_replaced` `:263`, `a_watchdog_of_this_build_is_left_alone` `:299`, `a_watchdog_of_another_path_stays_until_its_own_binary_is_replaced` `:339`, `a_sigterm_during_a_reap_lets_the_reap_finish` `:388`, `install_restarts_the_watchdog_of_an_active_run_on_the_installed_binary` `:479`, and the workspace tests from `:573`). A behavior that fleetd keeps (a reap that a SIGTERM does not cut, the wake to the workspace of the run, the restart on another binary) keeps a test against fleetd. A test of a behavior that is gone (a second watchdog is refused) is removed, and the commit message names it and the reason.
- The FM-7 test: its setup `claimed_run` asserts that the claim "started" a watchdog (`p0_supervision.rs:258`) and then kills the watchdog by its lock. Change the setup to the fleetd interface: start fleetd with `start_fleetd` (`p0_supervision.rs:141`) and kill the fleetd that the `fleetd` row names. Keep the rest of the test: only `horch fleetd check` runs every 30 s, and the task is queued again by a live fleetd. Say why in the commit.
- The release test (MAP.md, Q10 and Q21), in the new file `crates/horch-e2e/tests/p1_release.rs`: build a run with 1 live fake worker and 1 stand-in old watchdog (a `sleep` child that the test writes as the lock owner through `watchdog::lock` and as `runs.watchdog_pid` through `watchdog::record_start`). Start fleetd in stand-in mode. Then: the stand-in is stopped by its PID and start time, the `fleetd` row names the only live supervisor, the worker goes on, and the task has 1 attempt.
- Remove the exception of `taskledger_watch.rs` from `fitness_status_writes_go_through_transition` in `crates/horch-core/tests/arch_scan.rs` (ticket 13 named it), if the file still has 1.

## Steps

1. Run `cargo test -p horch-e2e --test p0_supervision` first: `fm7_killed_watchdog_does_not_stop_the_reconcile` passes as an expected failure.
2. Write the tests first. Each fails before the code exists:
   - In `old_watchdog.rs`: `first_start_stops_a_recorded_watchdog_by_pid_and_start_time`, `a_pid_with_another_start_time_is_not_signalled`, `a_dead_recorded_watchdog_is_cleared_with_no_signal`.
   - In `p1_release.rs`: `update_with_a_live_worker_and_a_live_old_watchdog_leaves_1_control_path`.
   - In `crates/horch/src/cmd/taskledger_watch.rs` or `taskledger.rs`: `watch_once_runs_1_fleetd_pass_with_the_json_of_today` and `watch_once_defers_to_a_fleetd_that_holds_the_lock`.
3. Write the code. Remove the old loop, `ensure` and `restart_if_stale`. Change the pinned tests. Change the setup of the FM-7 test. Remove its line from `p0_supervision.txt`.
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

The Check passes. `fm7_killed_watchdog_does_not_stop_the_reconcile` passes off the list. `update_with_a_live_worker_and_a_live_old_watchdog_leaves_1_control_path` passes. No code starts a watchdog.
