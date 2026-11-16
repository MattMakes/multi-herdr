# 28: The e2e tests of Phase 1 for the pass: 10,000 folders, missed file events, a fleetd killed mid-run, a crash after the commit

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** quality
**Review:** full
**Check:** `cargo test -p horch-e2e --test p1_pass`
**Own:** `crates/horch-e2e/tests/p1_pass.rs`

Spec: `docs/specs/fleetd/design.md`, RR-4, RR-7, FM-11, "Migration plan" item 3 (Tests: "the fact scans of 10,000 outboxes and worktree folders keep the heartbeat under 30 s and go on at their cursors after a restart"), "Supervision, liveness and reconciliation" (the work budget, the time budget of a pass).

## Goal

The gate tests of RR-4, RR-7 and FM-11, and the scan test of the gate, at the level of the `horch` binary, with a real fleetd in stand-in mode.

## Requirements

- **RR-4**: drop all file watch events in a test; tasks still complete.
- **RR-7**: kill the supervisor mid-run; compare the outcomes with a clean run.
- **FM-11**: fleetd killed between the commit and the effect: on restart each effect happens once (the unit test is in ticket 21; this is the e2e test).
- The scan test of the gate: 10,000 outboxes and worktree folders keep the heartbeat under 30 s, and the scans go on at their cursors after a restart.

## Context

- Read `docs/tickets/fleetd-p1/MAP.md`, section "Open questions", first. It records each choice of this phase (Q1 to Q21), and each ticket follows it.
- Phase P (`docs/tickets/fleetd-pp/`) builds `crates/horch-platform`. Its concern tickets propose function names. The module doc of each concern file in `crates/horch-platform/src/` gives the final name. Read that doc before you call a platform function. A name in this ticket is the proposed name.
- The code is in place (tickets 18, 21, 23). This ticket adds tests only. If a test fails for a fault in the code, send a QUESTION with the fault and a proposed fix task.
- Fault points (`HORCH_FAULT`, `crates/horch-core/src/runtime/fault.rs`; ticket 21): `drop-watch-events` and `abort-after-commit`. Set them on the fleetd process of the test only.
- The folders (MAP.md, Q4): outboxes `<fleet root>/tasks/<task>/attempts/<attempt>/outbox/`; the worktree folders that the scans of ticket 18 read. Make 10,000 of them in the temp fleet root, with no git work (empty folders and small files are enough for the scan).
- The cursors are rows of `pass_cursors` (ticket 18). After a restart, the first scan starts at the stored cursor, not at the start.
- "Compare with a clean run" (RR-7): run the same small plan (3 tasks of the fake harness) twice, once clean and once with fleetd killed by SIGKILL in the middle and started again. Compare the end state of each task and its count of attempts: the same, with no lost and no duplicated task.
- Start fleetd as the other e2e tests do (stand-in mode, MAP.md, Q8; copy `start_fleetd` and `Fleetds` from `crates/horch-e2e/tests/p0_supervision.rs`). Use the fake herdr and the fake agents of `crates/horch-e2e/src/harness.rs`.
- The whole file must run in less than 20 min.

## Steps

1. Write the tests in the new file `crates/horch-e2e/tests/p1_pass.rs`:
   - `scans_of_10000_folders_keep_the_heartbeat_under_30_s_and_go_on_at_their_cursors_after_a_restart`.
   - `rr4_tasks_complete_with_all_file_watch_events_dropped`.
   - `rr7_fleetd_killed_mid_run_ends_as_a_clean_run`.
   - `fm11_fleetd_aborted_after_the_commit_does_each_effect_once_on_restart`.
2. Run the Check. If a test fails, send a QUESTION.

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

The Check passes in less than 20 min. The 4 tests pass.
