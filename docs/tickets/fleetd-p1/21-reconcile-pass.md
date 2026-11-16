# 21: The reconcile pass in 3 parts: facts, 1 transaction of rows, then the effects and the heartbeat

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-core --lib fleetd && cargo test -p horch-core --test fleetd_pass`
**Own:** `crates/horch-core/src/fleetd/fleet_runtime/domain/pass.rs`, `crates/horch-core/src/fleetd/fleet_runtime/domain/heartbeat.rs`, `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs`, `crates/horch-core/src/fleetd/fleet_runtime/adapters/effects.rs`, `crates/horch-core/tests/fleetd_pass.rs`

Spec: `docs/specs/fleetd/design.md`, "Supervision, liveness and reconciliation" (The reconcile pass: the pseudocode and "A pass has 3 parts"; Each effect is safe to repeat; Stops come before launches; An attempt id is never used again; the time budget of a pass), RR-4, RR-7, FM-7, FM-11, FM-12, PL-5 (the heartbeat), "Migration plan" item 3.

## Goal

1 function runs 1 pass of fleetd. Part 1 reads the facts with no transaction, within the scan budgets. Part 2 is 1 transaction: it checks the generation, ticks the fleet clock, and calls each Phase 1 step in the order of the design, so each effect is a row in an intended state. Part 3, after the commit, does the effects (stops before launches) within the time budget, then writes the heartbeat and pings the watchdog of the OS.

## Requirements

- **RR-4**: the pass reconciles on a fixed period as well as on file events; a missed event is caught by the next pass.
- **RR-7**: a restart rebuilds from the ledger and the files: no lost and no duplicated task.
- **FM-7**: 1 level-triggered pass every 5 s that acts in code.
- **FM-11**: the ledger changes of a pass are 1 transaction with no effect inside; each effect is a committed intended row first, keyed by its row id and safe to repeat; before a launch, fleetd looks for a live process of the attempt id; stops come before launches.
- **FM-12** and **PL-5** (part): the heartbeat after each full pass; a pass never waits for a job; the pass ends at its time budget.

## Context

- Read `docs/tickets/fleetd-p1/MAP.md`, section "Open questions", first. It records each choice of this phase (Q1 to Q21), and each ticket follows it.
- Phase P (`docs/tickets/fleetd-pp/`) builds `crates/horch-platform`. Its concern tickets propose function names. The module doc of each concern file in `crates/horch-platform/src/` gives the final name. Read that doc before you call a platform function. A name in this ticket is the proposed name.
- The steps of Phase 1 inside part 2, in this order (the pseudocode of the design, with the steps of later phases left out): `check_generation` (ticket 14), `tick_fleet_clock` (ticket 15), `observe_intents` (attempts in `launching`: a live process of that attempt id gives `live`; no process past the start timeout of 90 s on the fleet clock gives `ended` with `timed_out`), `mark_orphans` (ticket 20, I5), `finish_exited` (ticket 20, I1: dead process -> `EndAttempt` and `ScheduleRetry` or `EscalateTask`), the run duties of today (ticket 22 fills this step; leave a call to an empty port `RunDuties` here), `observe_jobs` (ticket 17), `check_invariants` (I1 process part, I4, I5; a breach that the step cannot repair goes to the needs-you list), `record_hold_changes` (ticket 19), `intended_effects`.
- The steps of later phases (`apply_requests`, `ingest`, `advance_ideas`, `keep_sub_orchestrators`, `promote_pending`, `dispatch`, leases, `queue_due_jobs` of other contexts, worktrees, landing) are not in Phase 1. Do not add them.
- Part 3 (`adapters/effects.rs`): each effect is keyed by its row id. A stop uses the platform: stop the process group by PID and start time with `stop_grace_s` (20 s) as the input, then close the exact pane through the herdr client of today (`crates/horch-core/src/workspace/client.rs`). A stop that waits its grace must not hold the pass: the stop sends the polite signal and records the time; the next pass sends the kill when the grace has passed on the fleet clock. A launch goes through the launcher of ticket 16, after a check that no live process has that attempt id. A job start runs `horch job run <id>` through the launcher. Stops run before launches. When `pass_time_budget_s` ends, the rest stay intended rows.
- The heartbeat (`domain/heartbeat.rs`, MAP.md, Q20): the file `<fleet root>/run/heartbeat.json`, written atomically after each full pass: `{ "pid", "pid_start", "generation", "awake": { "boot_id", "awake_ms" }, "at" (calendar, for status only), "exe" (the `ExeIdentity` of `crates/horch-core/src/exeid.rs`), "version" }`. Give the rule `is_hung(heartbeat, now_awake, limit 30 s) -> bool` (a reading of another boot is hung) and `is_older(heartbeat, this exe) -> bool`. Ticket 23 uses both in `horch fleetd check`.
- The watchdog ping: `service::watchdog_ping()` of ticket 06 after the heartbeat; `NotSupported` is normal on macOS and Windows.
- File watching (ticket 07) puts a changed folder first in the scans of ticket 18.
- Fault points (`HORCH_FAULT`, `crates/horch-core/src/runtime/fault.rs`; each point is a name only): `hang-pass` (after the first full pass, the next pass sleeps for ever before its heartbeat: `p0_fleetd_hang.rs` uses it), `abort-after-commit` (abort after the commit of part 2, before part 3), `drop-watch-events` (the watcher reports nothing). Each point that is not armed costs nothing.
- Tests use a temp ledger and temp folders (build rule 1). A test runs the pass as a library call, with fake herdr facts.

## Steps

1. Write the tests first, in the new file `crates/horch-core/tests/fleetd_pass.rs`. Each fails before the code exists:
   - `fleetd_killed_between_commit_and_effect_does_the_effect_once_on_restart` (Slice 1): `abort-after-commit` in a child process, then 1 more pass: 1 stop, 1 launch, no second.
   - `fm_fleetd_killed_mid_reap_requeues_exactly_once` (Slice 1).
   - `stops_come_before_launches_in_part_3`.
   - `launching_attempt_with_no_process_past_the_start_timeout_ends_timed_out`.
   - `launch_is_skipped_when_a_live_process_has_the_attempt_id`.
   - `pass_ends_at_its_time_budget_and_the_rest_stay_intended`.
   - `heartbeat_is_written_after_each_full_pass_and_not_by_a_hung_pass`.
   - `is_hung_after_30_s_of_awake_time_and_not_after_a_sleep`.
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

The Check passes. A crash between the commit and the effects leads to each effect exactly once; the heartbeat is written only after a full pass.
