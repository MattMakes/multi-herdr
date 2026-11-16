# The gate of Phase 0

The gate: FM-1 to FM-8 and FM-25 each have a red test, and each new scenario
of Phase 0 has a red test. Each red test is on the list of expected failures
(`crates/horch-e2e/expected-failures/`) with the phase that makes it green.

## The run

- Command: `cargo test --workspace --locked`
- Commit tested: `f8c0b7a68725578e6526139d589b8bbcec96d15e` (branch `fleet/fleetd-build`)
- Host: macOS, aarch64
- Result: pass. 2866 tests pass, 0 fail, 11 are ignored.
- Duration of the full run: 505 s (wall clock, with the workspace binaries
  already built).
- `cargo test -p horch-e2e --test expected_failures` passes: no stale entry,
  and each entry has a phase.

Each red test was also run alone. Each fails with its listed reason, and the
reason is a missing feature of the design, not a fault of the test or of the
harness.

## The rows

| Gate row | Test | Green in | Result |
| --- | --- | --- | --- |
| FM-1 | `fm1_plan_with_an_uncovered_requirement_is_refused` | Phase 6 | red, listed |
| FM-2, agent that never claims | `fm2_agent_that_never_claims_has_its_task_bound_at_spawn` | Phase 4 | red, listed |
| FM-3, claims and stops | `fm3_idle_claim_is_caught_in_under_10_min` | Phase 2 | red, listed |
| FM-4 | `fm4_dead_agent_ends_its_attempt_within_30_s` | Phase 1 | red, listed |
| FM-5, usage-limited worker not reaped | `fm5_usage_limited_worker_is_not_reaped_and_burns_no_attempt` | Phase 2 | red, listed |
| FM-6, exits without done | `fm6_agent_that_exits_without_done_leaves_its_work_to_the_next_attempt` | Phase 4 | red, listed |
| FM-7, killed watchdog | `fm7_killed_watchdog_does_not_stop_the_reconcile` | Phase 1 | red, listed |
| FM-8, 2 ideas back to back | `fm8_two_ideas_sent_back_to_back_stay_2_ideas` | Phase 6 | red, listed |
| FM-25, killed with dirty and untracked files | `fm25_killed_worker_with_dirty_and_untracked_files_loses_nothing` | Phase 3 | red, listed |
| herdr server restart (FM-13, Phase 1 part) | `fm13_herdr_restart_ends_attempts_and_queues_tasks_again` | Phase 1 | red, listed |
| herdr server restart (FM-13, full) | `fm13_herdr_restart_rescues_each_worktree_and_the_next_attempt_reuses_it` | Phase 3 | red, listed |
| several ready task branches | `given_3_ready_branches_when_bundle_then_fast_gate_runs_once_and_none_is_forgotten` | Phase 4 | red, listed |
| 2 features that conflict | `two_conflicting_features_keep_their_tips_and_reach_a_decision` | Phase 6 | red, listed |
| machine sleep | `sleep_does_not_expire_a_lease` | Phase 2 | red, listed |
| daemon restart on a new binary | `fm34_daemon_restart_on_a_new_binary_keeps_the_run` | Phase 1 | red, listed |
| hung fleetd, each OS | `fm12_hung_fleetd_is_restarted_on_each_os` | Phase 1 | red, listed |
| list of expected failures | `expected_failures_list_gates_the_run` | Phase 0 | passes |
| Windows test job | job `test-windows` in `.github/workflows/ci.yml` | CI only | see below |

## Windows

The job `test-windows` is in `.github/workflows/ci.yml`. This Mac cannot run
it. Its result comes from CI on the next pull request to `main`.
