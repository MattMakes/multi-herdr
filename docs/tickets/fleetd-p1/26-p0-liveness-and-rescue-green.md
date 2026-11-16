# 26: Make the Phase 1 red tests of liveness and of the herdr restart green, and add the e2e test of orphans

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** quality
**Review:** full
**Check:** `cargo test -p horch-e2e --test p0_liveness --test p0_rescue --test p1_orphans --test expected_failures`
**Own:** `crates/horch-e2e/tests/p0_liveness.rs`, `crates/horch-e2e/expected-failures/p0_liveness.txt`, `crates/horch-e2e/tests/p0_rescue.rs`, `crates/horch-e2e/expected-failures/p0_rescue.txt`, `crates/horch-e2e/tests/p1_orphans.rs`

Spec: `docs/specs/fleetd/design.md`, FM-4, FM-13, FM-22, "Migration plan" item 3 (Tests: "after a herdr restart, fleetd sees the dead processes, ends the attempts and queues the tasks again with the worktree handling of today (the part of FM-13 that Phase 1 can do)"), "Ledger" (I1, I5).

## Goal

The red tests `fm4_dead_agent_ends_its_attempt_within_30_s` and `fm13_herdr_restart_ends_attempts_and_queues_tasks_again` run against fleetd and pass. A new e2e test shows that an orphan process or pane after a reap is stopped within 1 pass (FM-22).

## Requirements

- **FM-4**: a dead agent process ends its attempt within 30 s (PID and start time checked on each 5 s pass). Test: `fm4_dead_agent_ends_its_attempt_within_30_s` in `crates/horch-e2e/tests/p0_liveness.rs`.
- **FM-13** (the part of Phase 1): after a herdr restart, fleetd sees the dead processes, ends the attempts and queues the tasks again. Test: `fm13_herdr_restart_ends_attempts_and_queues_tasks_again` in `crates/horch-e2e/tests/p0_rescue.rs`. The rescue tests (`fm13_herdr_restart_rescues_each_worktree_and_the_next_attempt_reuses_it`, `fm25_killed_worker_with_dirty_and_untracked_files_loses_nothing`) are Phase 3: their lines stay.
- **FM-22**: orphan processes or panes after a reap are stopped by the process-group kill and the exact pane close; I5 stops anything that a live attempt does not own.
- Remove the line of each test that turns green, and only those 2 lines: `fm4_...` from `crates/horch-e2e/expected-failures/p0_liveness.txt` (`fm3_...`, `fm5_...` and `sleep_does_not_expire_a_lease` are Phase 2) and `fm13_herdr_restart_ends_attempts_...` from `crates/horch-e2e/expected-failures/p0_rescue.txt`. Read `crates/horch-e2e/expected-failures/README.md`.

## Context

- Read `docs/tickets/fleetd-p1/MAP.md`, section "Open questions", first. It records each choice of this phase (Q1 to Q21), and each ticket follows it.
- Phase P (`docs/tickets/fleetd-pp/`) builds `crates/horch-platform`. Its concern tickets propose function names. The module doc of each concern file in `crates/horch-platform/src/` gives the final name. Read that doc before you call a platform function. A name in this ticket is the proposed name.
- The code is in place (tickets 20, 21, 22, 23, 25). This ticket changes the test setups to the fleetd interface and adds 1 test. If a test fails for a fault in the code, send a QUESTION with the fault and a proposed fix task. Do not edit a file outside `Own`.
- The setup of FM-4: it asserts that the claim "started it" (a watchdog, `p0_liveness.rs:207`). After ticket 25 no claim starts a watchdog. Start fleetd instead (stand-in mode, MAP.md, Q8; copy `start_fleetd` and `Fleetds` from `crates/horch-e2e/tests/p0_supervision.rs:141` and `:101`; each test file of `crates/horch-e2e/tests/` is its own crate). The helper `watch_once` (`p0_liveness.rs:68`) still works: `horch ledger watch --once` runs 1 fleetd pass (MAP.md, Q11).
- The setup of FM-13 (required by the plan of this phase): `fm13_herdr_restart_ends_attempts_and_queues_tasks_again` (`p0_rescue.rs:258`) unsets `HORCH_NO_WATCHDOG` and waits on the watchdog lock (`watchdog::holder`, `:275` and `:292`). Change it to start fleetd and to wait on, and check, the `fleetd` row (a live PID with its start time). Keep the rest: `restart_herdr`, no `horch` call after it, `CAUGHT_S`, and the asserts on the tasks and the workers. Say why in the commit.
- The orphan test, in the new file `crates/horch-e2e/tests/p1_orphans.rs`: `fm22_orphan_process_and_pane_after_a_reap_are_stopped`. A reaped worker leaves a child process in its process group and its pane; fleetd stops both within 1 pass of 5 s plus `stop_grace_s`. A process that no row names is not touched (MAP.md, Q12). Use the fake herdr of `crates/horch-e2e/src/harness.rs`.

## Steps

1. Run `cargo test -p horch-e2e --test p0_liveness --test p0_rescue` first: the 2 tests pass as expected failures.
2. Write `fm22_orphan_process_and_pane_after_a_reap_are_stopped` first and see it fail before the setup is right.
3. Change the setups of FM-4 and FM-13. Remove the 2 lines.
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

The Check passes. `fm4_dead_agent_ends_its_attempt_within_30_s`, `fm13_herdr_restart_ends_attempts_and_queues_tasks_again` and `fm22_orphan_process_and_pane_after_a_reap_are_stopped` pass off the list. The 5 lines of Phase 2 and Phase 3 stay.
