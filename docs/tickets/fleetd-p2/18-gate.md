# 18: Gate of Phase 2: each rule of the gate green, and the gate file

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** no
**Kind:** quality
**Review:** light
**Check:** `cargo test -p horch-e2e --test expected_failures && test -s docs/specs/fleetd/gates/fleetd-p2.md`
**Own:** `docs/specs/fleetd/gates/fleetd-p2.md`

Spec: `docs/specs/fleetd/design.md`, "Migration plan" item 4 (*Gate:* and Tests), the tables "What each gate needs", "Where each rule becomes green" and "What a release does to a run in flight" (row Phase 2).

## Goal

Show that the gate of Phase 2 is green: each requirement and failure mode of the gate line has a test that passes and is off the list of expected failures, and each test of the gate passes. Write `docs/specs/fleetd/gates/fleetd-p2.md`.

## Requirements

- The gate line: RR-1, RR-2, RR-6, RR-11, FM-3, FM-5, FM-14.
- The tests of the gate: a usage-limited worker is not reaped and burns no attempt; the idle-claim scenario is caught in under 10 min; a task in `running`, `blocked_on_user` or `waiting_quota` is cancelled, and the process of a live attempt is gone before the task is `cancelled`. The release row of Phase 2.
- A rule is green when each of its tests passes and is off the list of expected failures ("Each gate names its rows").

## Context

- Read `docs/tickets/fleetd-p2/MAP.md`, section "Open questions", first. It records each choice of this phase (Q1 to Q24), and each ticket follows it. Q1 and Q23 are open for the operator: name their state in the gate file.
- The rows and their tests (the ticket that wrote each; a worker may have changed a name: find it in the ticket and the file):

| Row | Test | Ticket |
| --- | --- | --- |
| RR-1 | `rr1_stopped_worker_is_caught_within_1_lease` (`crates/horch-e2e/tests/p2_leases.rs`, Unix only), `rr1_stopped_worker_is_hung_after_stall_after_s_with_no_activity`, `renews_only_with_process_heartbeat_and_activity` | 16, 09, 08 |
| RR-2 | `rr2_no_sequence_of_facts_leaves_a_task_held_with_an_expired_lease`, `rr2_expired_lease_never_leaves_the_task_assigned` (`p2_leases.rs`), `every_held_state_has_an_exit_that_frees_the_task` | 08, 16, 04 |
| RR-6 | `rr6_no_sequence_of_outcomes_retries_forever`, `retry_backoff_30s_2m_8m_then_replan_then_escalate`, `rr6_task_that_fails_each_attempt_escalates_after_max_attempts` (`p2_leases.rs`) | 10, 16 |
| RR-11 | `fm5_usage_limit_burns_no_attempt_and_moves_to_fallback`, `both_pools_limited_waits`, `usage_limit_without_reset_at_waits_for_pool_probe_ok` | 11 |
| FM-3 | `fm3_idle_claim_is_caught_in_under_10_min` (`crates/horch-e2e/tests/p0_liveness.rs`), `fm3_idle_claim_is_caught_within_stall_after_plus_lease`, `background_wait_holds_while_the_agent_child_runs_and_ends_when_it_exits` | 15, 09, 14 |
| FM-5 | `fm5_usage_limited_worker_is_not_reaped_and_burns_no_attempt` (`p0_liveness.rs`), `usage_limited_is_never_hung`, `waiting_approval_ends_the_attempt_and_opens_1_dialog_item_with_the_excerpt` | 15, 09 |
| FM-14 | `fm14_no_second_attempt_starts_while_the_old_process_lives` (`p2_leases.rs`), `fm14_second_lease_for_1_task_is_refused`, `stop_of_the_old_attempt_comes_before_the_spawn_on_the_fallback` | 16, 08, 14 |
| Test: usage limit | `fm5_usage_limited_worker_is_not_reaped_and_burns_no_attempt` | 15 |
| Test: idle claim | `fm3_idle_claim_is_caught_in_under_10_min` | 15 |
| Test: cancel | `cancel_of_a_running_task_stops_its_process_before_the_task_is_cancelled`, `cancel_of_a_blocked_on_user_task_is_immediate`, `cancel_of_a_waiting_quota_task_is_immediate` (`p2_leases.rs`) | 16 |
| Test: fleet clock | `sleep_does_not_expire_a_lease` (`p0_liveness.rs`) | 15 |
| Run in flight (release row) | `held_task_with_a_live_worker_on_phase_1_is_not_reaped_after_the_update_and_has_1_lease` (`crates/horch-e2e/tests/p2_release.rs`) | 17 |

- The lines of Phase 2 in `crates/horch-e2e/expected-failures/p0_liveness.txt` (`fm3_...`, `fm5_...`, `sleep_does_not_expire_a_lease`) are gone after ticket 15. Each line of a later phase stays.
- The runs, as in Phase P (`docs/specs/fleetd/gates/fleetd-pp.md`, a decision of the orchestrator): macOS runs on this Mac; Linux runs in Docker on this Mac (the image and the command of that file); Windows is "pending CI" unless a pull request of `fleet/fleetd-build` has a CI run. You can read CI and not start it: `gh run list --repo MattMakes/multi-herdr --branch fleet/fleetd-build`, `gh run view <id> --repo MattMakes/multi-herdr --log-failed`. A row is green on an OS only with a run of that OS. The RR-1 e2e test is Unix only: on Windows the RR-1 row rests on `renews_only_with_process_heartbeat_and_activity` and `rr1_stopped_worker_is_hung_after_stall_after_s_with_no_activity`. Say so in the row.
- `docs/specs/fleetd/gates/fleetd-pp.md` shows the form of a gate file.

## Steps

1. Run `cargo test --workspace --locked` on this Mac. Run each test of the table by name and see it pass. Check that no line of Phase 2 is left in `crates/horch-e2e/expected-failures/`.
2. Run the same in Docker for Linux. Find a CI run of the tip of `fleet/fleetd-build`, if one exists, and read the result of each test on Windows. Do not guess a result.
3. Correct each small fault. A fault in a file outside `Own`, or a fault that is not small (more than about 50 lines, or a change of behavior), goes to the orchestrator as a QUESTION with a proposed fix task.
4. Write `docs/specs/fleetd/gates/fleetd-p2.md`: 1 row for each row of the table, with its test names, the commit, the run (this Mac, Docker, or the CI run id) and the result on macOS, Linux and Windows. Add each flaky test that you saw, and the state of MAP.md Q1 and Q23.
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

The Check passes. Each row of the gate file is green on macOS and Linux, Windows is green or "pending CI", or the file names the row that is not green and the QUESTION or BLOCKED that you sent for it.
