Tip: `895e3848` (`fleet/fleetd-finish`, 2026-11-21, "fleetd-finish 206: restore 2 comments of the sweep on the ready rows reader (T-313)")

# The pre-gate of Phase 4

This file is the pre-gate of Phase 4 (task T-323, ticket 210). It runs the rows of
`docs/tickets/fleetd-p4/60-gate.md` on the tip above, before the merge of `origin/main` (task
E1). The real gate (E2) writes `docs/specs/fleetd/gates/fleetd-p4.md` later.

## The runs

- Host: macOS aarch64, this Mac. Each command ran through `scripts/dev-sandbox.sh`.
- `cargo build --workspace --bins --locked`: passed.
- `cargo test --locked --no-fail-fast -p horch-core --test nfr --test arch_scan --test architecture_doc --test harness_defaults --test sr3_no_agent_push`:
  1 failed (`arch_scan`), the other 4 targets passed. Load average 13 to 19.
- `cargo test --locked --no-fail-fast -p horch-e2e --test p0_dispatch --test p0_landing --test p4_verifier --test p4_no_loss --test p4_landing --test p4_repair --test p4_remote --test p4_rescue_refs --test p4_clock_and_heartbeat --test p4_safety --test p4_release --test expected_failures`:
  40 of 40 passed. Load average 13 at the start and 38 at the end.
- Required items 2, 4 and 5: `cargo test --locked -p horch-core --lib fleet_runtime::domain::health -- --test-threads=4` (8 of 8) and
  `cargo test --locked -p horch-core --test integration_project --test fleetd_jobs -- --test-threads=4` (15 of 15). Load average 35.
- The red test ran 2 more times alone: 3 of 3 red.
- `grep -n "| 4 |" crates/horch-e2e/expected-failures/*.txt`: 2 lines (see Red rows).
- Linux: not run. Windows: not run (pending CI). A row below is green on macOS only.

## The rows

| Row | Test binary | Test name | Result | First failure line |
| --- | --- | --- | --- | --- |
| FR-5 | `horch-e2e` `p4_safety` | `fr5_worker_never_runs_a_task_search` | passed | - |
| SR-2 | `horch-e2e` `p4_safety` | `sr2_profile_secret_never_reaches_task_yaml_or_start_md` | passed | - |
| SR-3 (push) | `horch-e2e` `p4_safety` | `sr3_agent_git_push_is_refused_and_fleetd_push_works` | passed | - |
| SR-3 (external messages) | `horch-e2e` `p4_safety`, `p4_verifier` | `sr3_agent_launch_denies_external_messages`, `sr3_check_that_connects_off_host_is_refused` | passed | - |
| SR-3 (guard) | `horch-core` `sr3_no_agent_push` | `agent_push_is_refused_by_the_pre_push_hook`, `horch_old_path_push_with_dev_null_hooks_still_works`, 4 more (6 passed, 1 ignored helper `network_push_child` that `horch_old_path_push_with_dev_null_hooks_still_works` runs) | passed | - |
| SR-3 (profiles) | `horch-core` `harness_defaults` | `claude_profile_denies_git_push_and_external_messages_by_default`, `codex_profile_forbids_git_push_and_external_messages_by_default` | passed | - |
| SR-4 | `horch-e2e` `p4_verifier` | `sr4_check_that_writes_outside_its_paths_is_refused` | passed | - |
| PL-4 | `horch-e2e` `p4_clock_and_heartbeat` | `pl4_sleep_past_a_branch_deadline_fires_nothing_on_wake` | passed | - |
| PL-5 | `horch-e2e` `p4_clock_and_heartbeat` | `pl5_fast_gate_and_full_gate_of_5_min_keep_fleetd_healthy` | passed | - |
| FM-2 | `horch-e2e` `p0_dispatch` | `fm2_agent_that_never_claims_has_its_task_bound_at_spawn` | passed | - |
| FM-6 | `horch-e2e` `p0_dispatch` | `fm6_agent_that_exits_without_done_leaves_its_work_to_the_next_attempt` | passed | - |
| FM-6 (Stop hooks) | `horch-core` `harness_defaults` | `claude_push_launch_has_a_stop_hook_that_runs_result_check`, `codex_push_launch_has_a_trusted_stop_hook`, `antigravity_push_launch_has_a_stop_hook_that_runs_result_check`, `claim_attempt_has_no_stop_hook` | passed | - |
| FM-36 | `horch-e2e` `p4_landing` | `fm36_fleetd_restart_in_a_bundle_and_its_bisect_builds_nothing_twice` | passed | - |
| FM-37 | `horch-e2e` `p4_landing` | `fm37_passed_bundle_whose_compare_and_set_fails_is_dropped_and_its_members_are_ready` | passed | - |
| FM-38 | `horch-e2e` `p4_repair` | `fm38_conflicted_branch_gets_a_resolver_attempt_and_lands` | passed | - |
| FM-40 | `horch-e2e` `p4_rescue_refs` | `fm40_unique_rescue_ref_is_kept_until_rescue_drop`, `fm40_superseded_rescue_ref_expires_30_days_after_its_feature_ended`, `fm40_unique_budget_opens_1_item_and_deletes_nothing` | passed | - |
| FM-43 | `horch-e2e` `p4_remote` | `fm43_transient_push_failure_is_followed_by_a_new_run_of_the_push_job` | passed | - |
| FM-9 part (code kind) | `horch-e2e` `p4_verifier` | `fm9_code_task_with_a_failing_check_stays_not_done` | passed | - |
| FM-9 part (structural check) | `horch-e2e` `p4_verifier` | `fm9_docs_task_without_its_section_stays_not_done` | passed | - |
| I8 | `horch-e2e` `p4_no_loss` | `i8_squash_merge_with_a_conflict_resolution_is_seen_as_landed` | passed | - |
| I9 | `horch-e2e` `p4_no_loss` | `i9_gate_result_on_a_stale_tip_moves_no_ref` | passed | - |
| I10 | `horch-e2e` `p4_no_loss` | `i10_crash_before_and_after_the_rescue_of_a_forced_update_loses_no_tip` | passed | - |
| I11 | `horch-e2e` `p4_no_loss` | `i11_orphan_fleet_branch_is_adopted_and_not_deleted` | passed | - |
| I12 | `horch-e2e` `p4_no_loss` | `i12_stash_entry_blocks_the_deletion_of_its_branch` | passed | - |
| Test: dropped bundle | `horch-e2e` `p4_landing` | `fm37_passed_bundle_whose_compare_and_set_fails_is_dropped_and_its_members_are_ready` | passed | - |
| Test: candidate refs deleted | `horch-e2e` `p4_landing` | `candidate_ref_of_each_ended_bundle_is_deleted_and_verified` | passed | - |
| Test: failed resolver, max_attempts | `horch-e2e` `p4_repair` | `failed_resolver_attempt_starts_the_next_and_at_max_attempts_opens_1_item` | passed | - |
| Test: fix attempt | `horch-e2e` `p4_repair` | `gate_failed_branch_gets_a_fix_attempt_with_the_gate_output` | passed | - |
| Test: transient push failure | `horch-e2e` `p4_remote` | `fm43_transient_push_failure_is_followed_by_a_new_run_of_the_push_job` | passed | - |
| Test: 2 fetch periods | `horch-e2e` `p4_remote` | `two_fetch_periods_in_a_row_make_two_fetch_runs` | passed | - |
| Test: push failures, PR | `horch-e2e` `p4_remote` | `three_push_failures_open_1_needs_you_item`, `closed_unmerged_pull_request_opens_1_needs_you_item` | passed | - |
| Test: rescue drop by a worker | `horch-e2e` `p4_rescue_refs` | `rescue_drop_by_a_worker_is_refused` | passed | - |
| Test: verifier chain | `horch-e2e` `p4_verifier` | `passing_code_task_reaches_done_only_through_the_verifier`, `push_dispatched_task_launches_writes_task_files_and_reaches_done_through_verifier` | passed | - |
| Named: `p0_dispatch` | `horch-e2e` `p0_dispatch` | 2 tests (FM-2, FM-6 above) | passed | - |
| Named: `p0_landing` | `horch-e2e` `p0_landing` | `given_3_ready_branches_when_bundle_then_fast_gate_runs_once_and_none_is_forgotten`, `two_conflicting_features_keep_their_tips_and_reach_a_decision` | passed | - |
| Release row (run in flight) | `horch-e2e` `p4_release` | `claimed_attempt_with_detached_finish_lands_once_after_the_update`, `old_finish_that_dies_around_its_land_lands_once_after_the_update`, `each_run_gets_1_feature_row_and_each_task_branch_a_row`, `run_without_gates_holds_dispatch_and_loses_no_task` | passed | - |
| Required item 1 (FM-2 binding at process entry) | `horch-e2e` `p0_dispatch` | `fm2_agent_that_never_claims_has_its_task_bound_at_spawn` (ticket 48, 49) | passed | - |
| Required item 2 (the 2 gates) | `horch-core` `integration_project`; `horch-e2e` `p4_release` | `gates_parse_fast_and_full`, `missing_fast_gate_is_none`, `unknown_gate_key_is_refused`; `run_without_gates_holds_dispatch_and_loses_no_task` (tickets 04, 28) | passed | - |
| Required item 3 (check sandbox, SR-4) | `horch-e2e` `p4_verifier` | `sr4_check_that_writes_outside_its_paths_is_refused` (tickets 17, 18, 51) | passed | - |
| Required item 4 (health scope) | `horch-core` `--lib` | `fleet_runtime::domain::health::tests::open_file_guard_compares_system_count_with_system_limit`, `process_guard_compares_user_count_with_user_limit`, `not_supported_fact_gives_unknown_and_no_hold` (ticket 61) | passed | - |
| Required item 5 (a time limit for each job kind) | `horch-core` `fleetd_jobs` | `each_job_kind_has_a_time_limit` (ticket 03) | passed | - |
| Guard: `nfr` | `horch-core` `nfr` | 11 passed; `nfr_02_live_store_rss` and `nfr_02_tick_budget` are `#[ignore]` (not rows of Phase 4) | passed | - |
| Guard: `architecture_doc` | `horch-core` `architecture_doc` | 3 tests | passed | - |
| Guard: `arch_scan` | `horch-core` `arch_scan` | `fitness_no_context_touches_foreign_tables` (35 other tests passed) | failed, 3 of 3 | `a context names a table of another context:` (`crates/horch-core/tests/arch_scan.rs:1880`) |
| Guard: `expected_failures` | `horch-e2e` `expected_failures` | `expected_failures_list_gates_the_run` (1 test) | passed | - |
| No Phase 4 expected failure | `crates/horch-e2e/expected-failures/taskledger.txt` | `the_whole_flow_on_the_fake_herdr`, `a_task_finishes_through_review_land_pr_and_cleanup` | failed: 2 lines with phase 4 | `taskledger.txt:9: the_whole_flow_on_the_fake_herdr \| 4 \| timed out waiting for the worktree of T-2 is gone` |
| Linux check sandbox (Q-A) | - | not run on Linux | pending (allowed by Q-A when CI has no `bwrap`) | - |
| Windows | - | not run | pending CI | - |

## Red rows

1. Guard `arch_scan`: `fitness_no_context_touches_foreign_tables` fails 3 of 3.
   - Failure text: `a context names a table of another context:`, then 14 lines:
     - `fleetd/fleet_runtime/application/pass.rs:3178`: table `workers` is not a table of fleet_runtime
     - `fleetd/task_execution/adapters/sqlite.rs:205`: table `runs` is not a table of task_execution
     - `fleetd/task_execution/application/dispatch.rs:498`: table `ideas`
     - `fleetd/task_execution/application/dispatch.rs:509`: table `task_files`
     - `fleetd/task_execution/application/dispatch.rs:589`: table `worktrees`
     - `fleetd/task_execution/application/dispatch.rs:619`: tables `runs` and `projects`
     - `fleetd/task_execution/application/dispatch.rs:644`: table `task_files`
     - `fleetd/task_execution/application/launch.rs:428`: table `questions`
     - `fleetd/ideas/application/sub_orchestrators.rs:699`: table `tasks` is not a table of ideas
     - `fleetd/worktree_custody/application/disk.rs:144`: tables `jobs`, `tasks` and `ideas`
     - `fleetd/worktree_custody/application/reconcile.rs:603`: table `jobs`
   - File and line: `crates/horch-core/tests/arch_scan.rs:1880`.
   - Cause (verified by `git blame` of the lines): new SQL that reads tables of other contexts.
     T-292 (fleetd-finish 42 stage 2) added `pass.rs:3178`, `disk.rs:144` and
     `sub_orchestrators.rs:699`. T-313 (fleetd-finish 206) added `dispatch.rs:498`. T-300
     (fleetd-finish 199) added `launch.rs:428`. The other lines were not blamed one by one.
     The test is not wrong: each line is a read of a foreign table outside the 1 door.
   - Fix task: none when this run started. T-249 (CI FIX-21) fixed only the earlier lines of
     `ideas/application/answers.rs`. The orchestrator answered the NOTE of this task: it queues a
     new fix task for these 14 lines.
2. Expected failures of Phase 4: `crates/horch-e2e/expected-failures/taskledger.txt:9` and `:10`.
   - Failure text: `the_whole_flow_on_the_fake_herdr | 4 | timed out waiting for the worktree of T-2 is gone`
     and `a_task_finishes_through_review_land_pr_and_cleanup | 4 | timed out waiting for the worktree of T-1 is gone`.
   - File and line: `crates/horch-e2e/expected-failures/taskledger.txt:9-10`.
   - Cause (not verified, from the comment of the file): the worktree states `landing` and
     `landed` do not exist, so no code removes the tree of landed work.
   - Fix task: T-286 (ticket 168, "Worktree lifecycle: the states `landing` and `landed`"),
     state `landing` when this file was written. It replaces T-207 (cancelled).

## Rules with no test

none. Each rule of the gate line of `docs/tickets/fleetd-p4/60-gate.md` has a test above that
proves it on macOS. 2 gaps of the audit (`ai_docs/plans/fleetd-finish/audit/phase-4.md`) are now
closed by tests:

- The Antigravity Stop hook (D1, T-16): `antigravity_push_launch_has_a_stop_hook_that_runs_result_check`.
- The SR-3 external-message boundary (D2, T-17): `sr3_agent_launch_denies_external_messages` and
  `sr3_check_that_connects_off_host_is_refused`.

No row has a Linux or Windows run in this pre-gate.
