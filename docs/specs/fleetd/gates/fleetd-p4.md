# The gate of Phase 4

The gate: FR-5, SR-2, SR-3, SR-4, PL-4, PL-5, FM-2, FM-6, FM-36, FM-37, FM-38, FM-40 and FM-43
green (`docs/specs/fleetd/design.md`, Migration plan, item 6). The gate also has a part test of
FM-9, 1 test for each of I8 to I12, the named tests of the gate line, and the release row of
Phase 4 in "What a release does to a run in flight".

A rule is green when each of its tests passes and is off the list of expected failures.

## Result

- macOS: each Phase 4 gate row is green at `3de43412`.
- The ticket Check is red at `3de43412`. 2 tests of `p3_refusals`, outside the Phase 4 rows, failed
  in the whole-workspace run (see "The whole workspace on macOS").
- Linux: no green CI run exists. No row is green on Linux.
- Windows: no green CI run exists. No row is green on Windows.

The gate of Phase 4 is not complete until the Check is green and Linux and Windows have green runs.

## The commit

- Tested commit: `3de43412`, the tip of `fleet/fleetd-finish` when this ticket started.
- This file is the only change of the ticket. The ticket changes nothing in `crates/`.

## The runs

- macOS: macOS aarch64, this Mac, on 2026-11-23, through `scripts/dev-sandbox.sh`:
  - `cargo build --workspace --bins --locked`: passed.
  - `cargo test -p horch-core --no-fail-fast --test nfr --test arch_scan --test architecture_doc
    --test harness_defaults --test sr3_no_agent_push`: 79 passed, 0 failed, 3 ignored.
  - `cargo test -p horch-e2e --no-fail-fast --test p0_dispatch --test p0_landing --test p4_verifier
    --test p4_no_loss --test p4_landing --test p4_repair --test p4_remote --test p4_rescue_refs
    --test p4_clock_and_heartbeat --test p4_safety --test p4_release`: 39 passed, 0 failed.
  - The 3 ignored tests are `nfr_02_live_store_rss`, `nfr_02_tick_budget` and the helper
    `network_push_child`, which `horch_old_path_push_with_dev_null_hooks_still_works` runs.
- Linux: not run on this Mac. The latest CI run of `fleet/fleetd-finish` is `44936097033`, at
  `2906f9cc`, an ancestor of `3de43412` by 74 commits. Its job `test (ubuntu-latest)` failed.
- Windows: not run on this Mac. In CI run `44936097033`, `build-windows` passed and `test-windows`
  failed.

A row is green on an OS only with a green run of that OS at the tested code.

## CI run 44936097033

| Job | Result | Failed tests | Failed tests of this gate |
| --- | --- | --- | --- |
| `test (macos-15)` | failure | 5 | none |
| `test (ubuntu-latest)` | failure | 4 | `fm36_fleetd_restart_in_a_bundle_and_its_bisect_builds_nothing_twice` |
| `test-windows` | failure | 476 | `candidate_ref_of_each_ended_bundle_is_deleted_and_verified`, `i8_squash_merge_with_a_conflict_resolution_is_seen_as_landed` |
| `build-windows` | success | - | - |

Commit `1062dc42`, after that run, changes the wait of the FM-36 test. No Linux run proves it.

## The whole workspace on macOS

`cargo clippy --workspace --all-targets --locked`: passed. `cargo fmt --all --check`: passed.

| Record | ok | failed | ignored |
| --- | --- | --- | --- |
| `scripts/dev-sandbox.sh cargo test --workspace --locked --no-fail-fast` at `3de43412` | 5690 | 2 | 27 |

The 2 failures are in `crates/horch-e2e/tests/p3_refusals.rs`:

1. `cleanup_after_land_removes_a_landed_tree_through_the_remove_job`
2. `cleanup_of_a_landed_tree_with_an_untracked_file_is_refused_and_marked_cleanup_failed`

Each failed with "timed out after 120 s waiting for the landing of the branch and of its tree". The
branch was `landed`, and its tree stayed in `landing`. The load average was 144 near the failure.
Alone, both tests passed 3 of 3 runs, in 49 s to 73 s, at a load average of 12 to 17. The link to
load is a measured correlation. The cause is not proved. The ticket rule counts this as a bug, and
the fix is outside this ticket, so it is reported to the orchestrator as `BLOCKED:`.

## The list of expected failures

`grep -n "| 4 |" crates/horch-e2e/expected-failures/*.txt` prints nothing. A search for "phase 4" in
any case also prints nothing. The file `taskledger.txt`, which the audit named, no longer exists.
`expected_failures_list_gates_the_run` passes.

## The rows

Each test passes on macOS at `3de43412`. Each Linux and Windows cell is "no green run": CI run
`44936097033` failed on both OSes.

| Row | Tests | macOS | Linux | Windows |
| --- | --- | --- | --- | --- |
| FR-5 | `fr5_worker_never_runs_a_task_search` (`p4_safety`) | green | no green run | no green run |
| SR-2 | `sr2_profile_secret_never_reaches_task_yaml_or_start_md` (`p4_safety`) | green | no green run | no green run |
| SR-3 | `sr3_agent_git_push_is_refused_and_fleetd_push_works`, `sr3_agent_launch_denies_external_messages` (`p4_safety`); `sr3_check_that_connects_off_host_is_refused` (`p4_verifier`); `agent_push_is_refused_by_the_pre_push_hook`, `horch_old_path_push_with_dev_null_hooks_still_works`, `claude_profile_denies_git_push_and_external_messages_by_default`, `codex_profile_forbids_git_push_and_external_messages_by_default`, `verifier_check_sends_no_message_outside_the_fleet`, `a_failing_project_pre_commit_hook_stops_an_agent_commit` (`sr3_no_agent_push`) | green | no green run | no green run |
| SR-4 | `sr4_check_that_writes_outside_its_paths_is_refused` (`p4_verifier`) | green | no green run | no green run |
| PL-4 | `pl4_sleep_past_a_branch_deadline_fires_nothing_on_wake` (`p4_clock_and_heartbeat`) | green | no green run | no green run |
| PL-5 | `pl5_fast_gate_and_full_gate_of_5_min_keep_fleetd_healthy` (`p4_clock_and_heartbeat`) | green | no green run | no green run |
| FM-2 | `fm2_agent_that_never_claims_has_its_task_bound_at_spawn` (`p0_dispatch`) | green | no green run | no green run |
| FM-6 | `fm6_agent_that_exits_without_done_leaves_its_work_to_the_next_attempt` (`p0_dispatch`); `claude_push_launch_has_a_stop_hook_that_runs_result_check`, `codex_push_launch_has_a_trusted_stop_hook`, `antigravity_push_launch_has_a_stop_hook_that_runs_result_check`, `claim_attempt_has_no_stop_hook` (`harness_defaults`) | green | no green run | no green run |
| FM-36 | `fm36_fleetd_restart_in_a_bundle_and_its_bisect_builds_nothing_twice` (`p4_landing`) | green | failed in CI run `44936097033` | no green run |
| FM-37 | `fm37_passed_bundle_whose_compare_and_set_fails_is_dropped_and_its_members_are_ready` (`p4_landing`) | green | no green run | no green run |
| FM-38 | `fm38_conflicted_branch_gets_a_resolver_attempt_and_lands`, `failed_resolver_attempt_starts_the_next_and_at_max_attempts_opens_1_item`, `gate_failed_branch_gets_a_fix_attempt_with_the_gate_output` (`p4_repair`) | green | no green run | no green run |
| FM-40 | `fm40_unique_rescue_ref_is_kept_until_rescue_drop`, `fm40_superseded_rescue_ref_expires_30_days_after_its_feature_ended`, `fm40_unique_budget_opens_1_item_and_deletes_nothing`, `rescue_drop_by_a_worker_is_refused` (`p4_rescue_refs`) | green | no green run | no green run |
| FM-43 | `fm43_transient_push_failure_is_followed_by_a_new_run_of_the_push_job`, `three_push_failures_open_1_needs_you_item` (`p4_remote`) | green | no green run | no green run |
| FM-9 (part: code kind) | `fm9_code_task_with_a_failing_check_stays_not_done` (`p4_verifier`) | green | no green run | no green run |
| FM-9 (part: structural check) | `fm9_docs_task_without_its_section_stays_not_done` (`p4_verifier`) | green | no green run | no green run |
| I8 | `i8_squash_merge_with_a_conflict_resolution_is_seen_as_landed` (`p4_no_loss`) | green | no green run | failed in CI run `44936097033` |
| I9 | `i9_gate_result_on_a_stale_tip_moves_no_ref` (`p4_no_loss`) | green | no green run | no green run |
| I10 | `i10_crash_before_and_after_the_rescue_of_a_forced_update_loses_no_tip` (`p4_no_loss`) | green | no green run | no green run |
| I11 | `i11_orphan_fleet_branch_is_adopted_and_not_deleted` (`p4_no_loss`) | green | no green run | no green run |
| I12 | `i12_stash_entry_blocks_the_deletion_of_its_branch` (`p4_no_loss`) | green | no green run | no green run |
| Test: dropped bundle | `fm37_passed_bundle_whose_compare_and_set_fails_is_dropped_and_its_members_are_ready` (`p4_landing`) | green | no green run | no green run |
| Test: candidate ref deleted | `candidate_ref_of_each_ended_bundle_is_deleted_and_verified` (`p4_landing`) | green | no green run | failed in CI run `44936097033` |
| Test: failed resolver | `failed_resolver_attempt_starts_the_next_and_at_max_attempts_opens_1_item` (`p4_repair`) | green | no green run | no green run |
| Test: transient push failure | `fm43_transient_push_failure_is_followed_by_a_new_run_of_the_push_job` (`p4_remote`) | green | no green run | no green run |
| Test: 2 fetch periods | `two_fetch_periods_in_a_row_make_two_fetch_runs` (`p4_remote`) | green | no green run | no green run |
| Test: bundle of Phase 0 | `given_3_ready_branches_when_bundle_then_fast_gate_runs_once_and_none_is_forgotten` (`p0_landing`) | green | no green run | no green run |
| Run in flight (release row of Phase 4) | `claimed_attempt_with_detached_finish_lands_once_after_the_update`, `old_finish_that_dies_around_its_land_lands_once_after_the_update`, `run_without_gates_holds_dispatch_and_loses_no_task`, `each_run_gets_1_feature_row_and_each_task_branch_a_row` (`p4_release`) | green | no green run | no green run |

## The suites of the gate

| Suite | Passed | Failed |
| --- | --- | --- |
| `p0_dispatch` | 2 | 0 |
| `p0_landing` | 2 | 0 |
| `p4_verifier` | 6 | 0 |
| `p4_no_loss` | 5 | 0 |
| `p4_landing` | 3 | 0 |
| `p4_repair` | 3 | 0 |
| `p4_remote` | 4 | 0 |
| `p4_rescue_refs` | 4 | 0 |
| `p4_clock_and_heartbeat` | 2 | 0 |
| `p4_safety` | 4 | 0 |
| `p4_release` | 4 | 0 |
| `arch_scan` | 37 | 0 |
| `architecture_doc` | 3 | 0 |
| `harness_defaults` | 22 | 0 |
| `nfr` | 11 (2 ignored) | 0 |
| `sr3_no_agent_push` | 6 (1 ignored helper) | 0 |

## The required items of the phase

Each test passed on macOS in the whole-workspace run at `3de43412`.

| Item | Rule | Tickets | Tests |
| --- | --- | --- | --- |
| 1 | The FM-2 test records at process entry that the task is bound and `task.yaml` exists | 48, 49 | `fm2_agent_that_never_claims_has_its_task_bound_at_spawn` |
| 2 | The project declares its 2 gates in `[gates]` of its `fleet.toml` | 04, 28 | `gates_parse_fast_and_full`, `missing_fast_gate_is_none`, `project_with_no_fast_gate_holds_its_dispatch_with_its_cause`, `status_shows_missing_fast_gate_hold` |
| 3 | The check runs in the sandbox of the worker | 17, 18, 51 | `sr4_check_that_writes_outside_its_paths_is_refused` |
| 4 | The health guards compare numbers of the same scope, and a "not supported" fact gives no hold | 61 | `open_file_guard_compares_system_count_with_system_limit`, `process_guard_compares_user_count_with_user_limit`, `not_supported_fact_gives_unknown_and_no_hold` |
| 5 | Each job kind has a time limit in `job_limit_s` | 03 | `each_job_kind_has_a_time_limit` |

## The gaps of the audit

`ai_docs/plans/fleetd-finish/audit/phase-4.md` named 2 gaps. Each now has a passing test on macOS:

- The Antigravity Stop hook (L8): `antigravity_push_launch_has_a_stop_hook_that_runs_result_check`.
- The SR-3 external-message boundary (L9): `sr3_agent_launch_denies_external_messages`,
  `sr3_check_that_connects_off_host_is_refused`, `verifier_check_sends_no_message_outside_the_fleet`.
  These tests cover a finite list of destinations and transports
  (`docs/specs/fleetd/gates/operator-decisions.md`).

## Linux and Windows

- Linux pending row (MAP Q-A): the check sandbox tests need `bwrap`. `.github/workflows/ci.yml`
  installs `bubblewrap`, so the allowed pending reason does not explain a red Linux job.
- Windows risks, from `docs/specs/fleetd/gates/operator-decisions.md` (OD on the Windows target):
  - The breaker guard of the folder lock holds nothing on Windows.
  - The hold of the termination handler for check and gate groups does nothing on Windows.
  - SR-3 for git has no Windows form.
  - Protected-file reads by ACL are not verified as landed.
  - Profile validation refuses a worker profile on Windows.

## Flaky tests

- `cleanup_after_land_removes_a_landed_tree_through_the_remove_job` and
  `cleanup_of_a_landed_tree_with_an_untracked_file_is_refused_and_marked_cleanup_failed`: 1 failure
  each in the whole-workspace run and 3 passes alone. The cause is open.
- Each test of the Phase 4 gate passed in its 1 run at `3de43412` on macOS. This ticket did not run
  them 3 times.
