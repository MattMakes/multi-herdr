Tip: `895e384850230f5b2a5a6eaa824b0ba9b4f4d1f3` (`fleet/fleetd-finish`, 2026-11-21)

# Pre-gate of Phase 6

This is the pre-gate of Phase 6 (task T-325, ticket `docs/tickets/fleetd-finish/212-pre-gate-p6.md`).
The real gate runs after task E1 and writes `docs/specs/fleetd/gates/fleetd-p6.md`.

The gate line (`docs/specs/fleetd/design.md`, Migration plan, item 8): FR-1, FR-2, FR-3, FR-4,
FR-10, FR-12, RR-3, RR-12, OR-3, FM-1, FM-8, FM-9, FM-10, FM-16, FM-21, FM-29 to FM-33, FM-35,
FM-39, FM-41, FM-42, 2 gate tests, the release test, and REQUIRED items 1 to 5 of
`docs/tickets/fleetd-p6/MAP.md`.

## The runs

Each command ran on macOS aarch64 (this Mac) through `scripts/dev-sandbox.sh`, with
`--locked --no-fail-fast`, 1 `cargo test` command at a time. The 1-minute load was 11 to 34.

| Run | Binaries | Tests passed | Failed | Ignored |
| --- | --- | --- | --- | --- |
| `cargo build --workspace --bins` | - | build ok | - | - |
| `-p horch-core` gate suites | 26 | 208 | 1 | 2 |
| `-p horch-core --test plan_rules_profiles --test plan_rules_model_guide` | 2 | 11 | 0 | 0 |
| `-p horch --test p6_retired_run_commands --test p6_retired_worker_commands --test p6_fleet_start` | 3 | 28 | 0 | 0 |
| `-p horch-e2e` Phase 6 suites and `expected_failures` | 13 | 59 | 0 | 0 |

The 26 horch-core binaries: `nfr`, `arch_scan`, `architecture_doc`, `plan_validator`,
`plan_rules_judges`, `judge_pair`, `judge_branch`, `idea_direct`, `question_flow`,
`question_answers`, `p6_authorize`, `idea_rollup`, `feature_states_p6`, `judge_cancel`,
`merged_with_ideas`, `idea_intake`, `idea_dispatcher`, `sub_orchestrators`, `plan_commit`,
`idea_approval`, `taskledger_schema_p6`, `needs_you_folder`, `replan`, `idea_waiting`,
`plan_rules_coverage`, `idea_notes`.

The 2 ignored tests are `nfr::fr_02_live_store_rss` and `nfr::fr_02_tick_budget`. They are
`#[ignore]` in the source.

Expected failures: `grep -n "| 6 |" crates/horch-e2e/expected-failures/*.txt` is empty. The
active entries are of Phases 3, 4 and 7 only. `horch-e2e --test expected_failures` passes.

Linux and Windows: no CI run of this tip exists. Each row is pending CI on Linux. Each e2e file
of the gate has `#![cfg(unix)]`, so a Windows build has no e2e test for those rows.

## The rows

| Row | Binary | Test | Result | First failure line |
| --- | --- | --- | --- | --- |
| FR-1 | `horch-e2e/p6_intake` | `fr1_two_ideas_make_two_files_and_two_rows` | passed | - |
| FR-1 | `horch-core/idea_intake` | `intake_test_two_ideas_added_back_to_back_produce_two_files_and_two_rows` | passed | - |
| FR-2 | `horch-e2e/p6_sub_orchestrators` | `fr2_each_sub_orchestrator_attempt_has_1_idea` | passed | - |
| FR-2 | `horch-core/idea_dispatcher` | `property_no_sequence_of_passes_puts_2_ideas_of_1_feature_in_planning` | passed | - |
| FR-3 | `horch-e2e/p6_plans` | `fr3_valid_plan_commits_each_task`, `fr3_plan_missing_a_requirement_is_rejected` | passed | - |
| FR-3 | `horch-core/plan_commit` | `valid_plan_commits_each_task_in_1_transaction` | passed | - |
| FR-4 | `horch-e2e/p6_plans` | `fr4_judgment_task_without_judge_is_refused` | passed | - |
| FR-4 | `horch-core/plan_rules_judges` | `plan_without_paired_judge_is_refused` | passed | - |
| FR-10 | `horch-e2e/p6_fr10_approval` | `fr10_a_` to `fr10_f_` (6 tests), `fr10_sub_orchestrator_approval_leaves_awaiting_approval` | passed | - |
| FR-10 | `horch-core/idea_approval` | `fm32_forged_approval_is_ignored`, `approve_with_a_stale_hash_is_refused` | passed | - |
| FR-12 | `horch-e2e/p6_intake` | `fr12_direct_idea_makes_1_validated_plan`, `fr12_direct_judgment_idea_gets_its_judge` | passed | - |
| FR-12 | `horch-core/taskledger_schema_p6` | `task_without_an_idea_is_refused_by_the_trigger` | passed | - |
| FR-12 | `horch/p6_retired_run_commands` | `no_code_path_creates_a_task_without_an_idea` | passed | - |
| RR-3 | `horch-e2e/p6_judge_pair` | `rr3_judgment_and_judge_reach_done_with_no_plan_change`, `rr3_critical_gives_1_fix_round` | passed | - |
| RR-3 | `horch-core/judge_pair` | `rr3_judgment_task_and_its_judge_reach_done_with_no_plan_change`, `critical_item_gives_1_fix_round_not_counted_in_attempts` | passed | - |
| RR-12 | `horch-e2e/p6_questions` | `rr12_question_owner_is_started_and_answers`, `rr12_unanswered_question_reaches_needs_you_at_its_limit` | passed | - |
| RR-12 | `horch-core/question_flow` | `question_whose_owner_is_not_running_is_answered_after_fleetd_starts_it` | passed | - |
| OR-3 | `horch-core/needs_you_folder` | `open_item_has_1_file`, `same_escalation_on_2_passes_gives_1_file`, `resolved_item_file_is_removed` | passed | - |
| FM-1 | `horch-e2e/p0_ideas` | `fm1_plan_with_an_uncovered_requirement_is_refused` | passed | - |
| FM-8 | `horch-e2e/p0_ideas` | `fm8_two_ideas_sent_back_to_back_stay_2_ideas` | passed | - |
| FM-9 | `horch-e2e/p6_judge_pair` | `fm9_judgment_task_is_done_only_after_its_judge` | passed | - |
| FM-10 | `horch-e2e/p6_plans` | `fm10_cycle_is_refused_and_ticketing_reruns` | passed | - |
| FM-16 | `horch-e2e/p6_sub_orchestrators` | `fm16_no_orchestrator_and_questions_stay_in_their_feature` | passed | - |
| FM-16 | `horch/p6_fleet_start` | `fleet_start_launches_no_orchestrator` | passed | - |
| FM-16 | `horch-core/sub_orchestrators` | `i13_open_feature_without_sub_orchestrator_gets_one` | passed | - |
| FM-21 | `horch-e2e/p6_questions` | `fm21_one_replan_then_escalation` | passed | - |
| FM-21 | `horch-core/replan` | `second_failure_after_a_replan_escalates`, `failed_task_past_replan_limit_escalates` | passed | - |
| FM-29 | `horch-e2e/p6_sub_orchestrators` | `fm29_idle_stop_and_park` | passed | - |
| FM-29 | `horch-core/idea_waiting` | `idle_sub_orchestrator_stops_after_30_min_of_fleet_clock`, `reminder_only_in_working_hours`, `idea_parks_after_7_calendar_days` | passed | - |
| FM-30 | `horch-e2e/p6_intake` | `fm30_no_spec_before_approval` | passed | - |
| FM-30 | `horch-core/plan_rules_coverage` | `requirement_with_no_approved_citation_is_refused` | passed | - |
| FM-31 | `horch-e2e/p6_sub_orchestrators` | `fm31_killed_sub_orchestrator_resumes_from_its_folder` | passed | - |
| FM-31 | `horch-core/idea_notes`, `horch-core/sub_orchestrators` | `sub_orchestrator_answer_arrives_as_an_idea_note_file`, `dead_sub_orchestrator_restarts_in_the_same_phase` | passed | - |
| FM-32 | `horch-e2e/p6_intake` | `fm32_sub_orchestrator_operator_request_is_refused` | passed | - |
| FM-32 | `horch-core/p6_authorize` | `orchestrator_seat_has_no_operator_right` | passed | - |
| FM-33 | `horch-e2e/p6_judge_pair` | `fm33_two_fix_rounds_escalate` | passed | - |
| FM-35 | `horch-e2e/p6_questions` | `fm35_delivery_repeats_until_an_answer` | passed | - |
| FM-35 | `horch-core/question_flow` | `question_escalates_after_15_min_and_operator_wait_has_no_limit` | passed | - |
| FM-39 | `horch-e2e/p6_features` | `fm39_cancelled_task_does_not_block_its_idea` | passed | - |
| FM-39 | `horch-core/idea_rollup` | `cancelled_task_does_not_block_its_idea` | passed | - |
| FM-41 | `horch-e2e/p6_features` | `fm41_merge_with_an_idea_in_flight_moves_it_to_a_new_feature` | passed | - |
| FM-42 | `horch-e2e/p6_features` | `fm42_lower_feature_merged_rebases_the_upper`, `fm42_lower_feature_abandoned_rebases_the_upper` | passed | - |
| Gate test 1: merge of an `open` feature | `horch-e2e/p6_features`, `horch-core/merged_with_ideas` | `fm41_merge_with_an_idea_in_flight_moves_it_to_a_new_feature`, `open_feature_merged_by_content_moves_ideas_in_flight_to_a_new_feature` | passed | - |
| Gate test 2: judge cancel | `horch-e2e/p6_judge_pair`, `horch-core/judge_cancel` | `cancel_of_a_judged_task_cancels_its_judge_task`, `cancel_of_a_judge_task_alone_is_refused` | passed | - |
| Release test | `horch-e2e/p6_release` | `p6_release_loses_no_task`, `p6_release_run_with_an_open_question_gets_1_idea_1_feature_and_1_owner` | passed | - |
| REQUIRED 1: no agent has operator rights | `horch-e2e/p6_no_orchestrator_seat`, `horch-core/p6_authorize` | `old_seat_operator_request_on_the_channel_is_refused`, `old_seat_operator_request_file_is_refused`, `orchestrator_seat_has_no_operator_right` | passed | - |
| REQUIRED 2: retired run, plan and worker kinds | `horch/p6_retired_run_commands`, `horch/p6_retired_worker_commands` | 25 tests, with `retired_kind_request_file_is_refused_with_the_new_command`, `kind_catalog_is_the_table_of_the_design_and_the_kept_kinds` | passed | - |
| REQUIRED 3: FM-9 with the judge pair | `horch-core/judge_pair`, `horch-e2e/p6_judge_pair` | `rr3_judgment_task_and_its_judge_reach_done_with_no_plan_change`, `fm9_judgment_task_is_done_only_after_its_judge` | passed | - |
| REQUIRED 4: ideas finish clean | `horch-core/idea_rollup` | 5 tests, with `idea_is_merged_only_when_each_tree_is_removed_or_rescued` | passed | - |
| REQUIRED 5: feature ready by idea state | `horch-core/feature_states_p6` | `feature_with_an_idea_in_progress_is_not_ready`, `feature_with_ideas_integrating_cancelled_or_parked_is_ready_after_the_full_gate`, `open_feature_merged_by_content_is_merged` | passed | - |
| Tickets 08 to 12 | `horch-core/plan_validator`, `plan_rules_coverage`, `plan_rules_profiles`, `plan_rules_model_guide`, `plan_rules_judges` | 36 tests | passed | - |
| Guard: `nfr` | `horch-core/nfr` | 11 tests | passed | - |
| Guard: `architecture_doc` | `horch-core/architecture_doc` | 3 tests | passed | - |
| Guard: `arch_scan` | `horch-core/arch_scan` | `fitness_no_context_touches_foreign_tables` | failed, 3 of 3 | `a context names a table of another context:` (`crates/horch-core/tests/arch_scan.rs:1880`) |
| Expected failures of Phase 6 | `horch-e2e/expected_failures`, grep `"| 6 |"` | `expected_failures` | passed, 0 entries | - |

## Red rows

1. Guard `arch_scan`: `fitness_no_context_touches_foreign_tables` fails 3 of 3 (1 run in the
   suite, 2 runs alone). The other 35 tests of `arch_scan` pass.
   - Failure text at `crates/horch-core/tests/arch_scan.rs:1880:5`: "a context names a table of
     another context", with 14 lines:
     - `fleetd/fleet_runtime/application/pass.rs:3178`: table `workers`
     - `fleetd/task_execution/adapters/sqlite.rs:205`: table `runs`
     - `fleetd/task_execution/application/dispatch.rs:498`: table `ideas`
     - `fleetd/task_execution/application/dispatch.rs:509`: table `task_files`
     - `fleetd/task_execution/application/dispatch.rs:589`: table `worktrees`
     - `fleetd/task_execution/application/dispatch.rs:619`: tables `runs` and `projects`
     - `fleetd/task_execution/application/dispatch.rs:644`: table `task_files`
     - `fleetd/task_execution/application/launch.rs:428`: table `questions`
     - `fleetd/ideas/application/sub_orchestrators.rs:699`: table `tasks`
     - `fleetd/worktree_custody/application/disk.rs:144`: tables `jobs`, `tasks` and `ideas`
     - `fleetd/worktree_custody/application/reconcile.rs:603`: table `jobs`
   - Cause (verified from the test output and from ticket 213): production SQL in these files names
     a table that another bounded context owns. The test is a guard, not a gate rule. The Phase 6
     rule rows above stay green.
   - Owner: T-327, ticket `docs/tickets/fleetd-finish/213-arch-scan-foreign-table-reads.md`
     (queued).

## Rules with no test

none. Each rule of the gate line, the 2 gate tests, the release test and REQUIRED items 1 to 5
have at least 1 passing test above.
