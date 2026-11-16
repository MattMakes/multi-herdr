# 29: Gate of Phase 1: each rule of the gate green, and the gate file

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** no
**Kind:** quality
**Review:** light
**Check:** `cargo test -p horch-e2e --test expected_failures && test -s docs/specs/fleetd/gates/fleetd-p1.md`
**Own:** `docs/specs/fleetd/gates/fleetd-p1.md`

Spec: `docs/specs/fleetd/design.md`, "Migration plan" item 3 (*Gate:* and Tests), the tables "What each gate needs", "Where each rule becomes green" and "What a release does to a run in flight" (row Phase 1).

## Goal

Show that the gate of Phase 1 is green: each requirement and failure mode of the gate line has a test that passes and is off the list of expected failures, and each test of the gate passes, on macOS, Linux and Windows. Write `docs/specs/fleetd/gates/fleetd-p1.md`.

## Requirements

- The gate line: RR-4, RR-7, OR-2, OR-4, OR-5, OR-6, PL-1, PL-3, FM-4, FM-7, FM-11, FM-12, FM-22, FM-34, FM-44; the part of FM-13 that Phase 1 can do; the old watchdog test (the release row of Phase 1).
- A rule is green when each of its tests passes and is off the list of expected failures ("Each gate names its rows").

## Context

- Read `docs/tickets/fleetd-p1/MAP.md`, section "Open questions", first. It records each choice of this phase (Q1 to Q21), and each ticket follows it.
- Phase P (`docs/tickets/fleetd-pp/`) builds `crates/horch-platform`. Its concern tickets propose function names. The module doc of each concern file in `crates/horch-platform/src/` gives the final name. Read that doc before you call a platform function. A name in this ticket is the proposed name.
- The rows and their tests (the ticket that wrote each; a worker may have changed a name: find it in the ticket and the file):

| Row | Test | Ticket |
| --- | --- | --- |
| RR-4 | `rr4_tasks_complete_with_all_file_watch_events_dropped` (`crates/horch-e2e/tests/p1_pass.rs`) | 28 |
| RR-7 | `rr7_fleetd_killed_mid_run_ends_as_a_clean_run` (`p1_pass.rs`) | 28 |
| OR-2 | `every_status_change_has_an_event_with_a_cause`, `fitness_status_writes_go_through_transition` (`crates/horch-core/tests/arch_scan.rs`) | 13 |
| OR-4 | `pacing_holds_a_pool_ahead_of_its_window_and_frees_one_on_pace` | 19 |
| OR-5 | `low_disk_probe_pauses_dispatch_and_normal_probe_resumes`, `pres_06_s12_unknown_probe_neither_pauses_nor_passes` | 19 |
| OR-6 | `self_update_restarts_the_running_fleetd_before_it_replaces_itself` (24), `check_restarts_an_older_fleetd` (23), the refusal of a newer schema (03) | 03, 23, 24 |
| PL-1 | `cargo test --workspace --locked` passes in CI on macOS, Linux and Windows | all |
| PL-3 | the `NotSupported` tests of each concern, for example `watchdog_ping_is_not_supported_off_linux`, `counts_are_not_supported_on_windows` | 05 to 09 |
| FM-4 | `fm4_dead_agent_ends_its_attempt_within_30_s` (`p0_liveness.rs`) | 26 |
| FM-7 | `fm7_killed_watchdog_does_not_stop_the_reconcile` (`p0_supervision.rs`) | 25 |
| FM-11 | `fm11_fleetd_aborted_after_the_commit_does_each_effect_once_on_restart` (`p1_pass.rs`), `fleetd_killed_between_commit_and_effect_does_the_effect_once_on_restart` | 28, 21 |
| FM-12 | `fm12_hung_fleetd_is_restarted_on_each_os` (`p0_fleetd_hang.rs`), `job_of_5_min_runs_while_fleetd_stays_healthy_and_is_not_restarted` | 23, 27 |
| FM-13 (part) | `fm13_herdr_restart_ends_attempts_and_queues_tasks_again` (`p0_rescue.rs`) | 26 |
| FM-22 | `fm22_orphan_process_and_pane_after_a_reap_are_stopped` (`p1_orphans.rs`), `i5_orphan_process_or_pane_is_stopped` | 26, 20 |
| FM-34 | `fm34_daemon_restart_on_a_new_binary_keeps_the_run` (`p0_supervision.rs`) | 24 |
| FM-44 | `needs_you_item_written_twice_by_key_is_1_item_and_resolves_with_its_row`, `item_not_open_is_removed_30_days_after_it_ended` | 10 |
| Old watchdog (release row) | `update_with_a_live_worker_and_a_live_old_watchdog_leaves_1_control_path` (`p1_release.rs`) | 25 |
| Test: 5 min job | `job_of_5_min_runs_while_fleetd_stays_healthy_and_is_not_restarted` (`p1_jobs.rs`) | 27 |
| Test: 10,000 folders | `scans_of_10000_folders_keep_the_heartbeat_under_30_s_and_go_on_at_their_cursors_after_a_restart` | 28 |
| Test: job backoff and requeue | `job_past_its_limit_runs_again_after_its_backoff_and_after_3_abandoned_runs_goes_to_you` | 27 |

- The lines of Phase 1 in `crates/horch-e2e/expected-failures/` today: `fm12_...` (`p0_fleetd_hang.txt`), `fm7_...` and `fm34_...` (`p0_supervision.txt`), `fm4_...` (`p0_liveness.txt`), `fm13_herdr_restart_ends_attempts_...` (`p0_rescue.txt`). After this phase none is left. Each line of a later phase stays.
- CI runs on the draft pull request of `fleet/fleetd-build` at each push, on Linux, macOS and Windows. You can read CI and not start it: `gh run list --repo MattMakes/multi-herdr --branch fleet/fleetd-build`, `gh run view <id> --repo MattMakes/multi-herdr --log-failed`. The handling of CI is as in Phase P (`docs/tickets/fleetd-pp/MAP.md`, Q10 to Q12): a fix needs a new CI run; commit it, run `horch ledger progress "waiting: CI of <commit>"`, send a NOTE to the orchestrator that names the commit, and end your turn.
- `docs/specs/fleetd/gates/fleetd-pp.md` (the gate file of Phase P) shows the form of a gate file.

## Steps

1. Run `cargo test --workspace --locked` on this Mac. Run each test of the table by name and see it pass. Check that no line of Phase 1 is left in `crates/horch-e2e/expected-failures/`.
2. Find the CI run of the tip of `fleet/fleetd-build`. Read the result of each test of the table on Linux and on Windows. If the run does not exist or its log is not complete for an OS, send BLOCKED to the orchestrator with the commit and the OS. Do not guess a result.
3. Correct each small fault. A fault in a file outside `Own`, or a fault that is not small (more than about 50 lines, or a change of behavior), goes to the orchestrator as a QUESTION with a proposed fix task.
4. Write `docs/specs/fleetd/gates/fleetd-p1.md`: 1 row for each row of the table, with its test name, the commit, the CI run id, and the result on macOS, Linux and Windows. Add each flaky test that you saw.
5. Run the Check. Commit.

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

The Check passes. Each row of the gate file is green on the 3 OSes, or the file names the row that is not green and the QUESTION or BLOCKED that you sent for it.
