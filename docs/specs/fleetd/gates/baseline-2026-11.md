# Baseline of fleet/fleetd-build, 2026-11-19

First executed `fmt`, `clippy` and `test` run of the fleetd build. No code or test changed in this unit (B1, T-3).

## What was tested

| Item | Value |
|---|---|
| Tested commit (worktree HEAD) | `085427fba539a050c92c489c07e3d43615342f55` |
| Branch of the worktree | `fleet/fleetd-finish-T3-b1-baseline-execute-fmt-clippy-and-full`, based on `fleet/fleetd-finish` |
| `fleet/fleetd-build` commit | `d29d549c8bf71bb0735a03116214bb259ff46fde` |
| Difference | The tested commit is `fleet/fleetd-build` plus the 7 A1 commits (isolation: `scripts/dev-sandbox.sh`, ledger guard in `store.rs`, service label, e2e ledger seal, old-watchdog test). `git diff --stat fleet/fleetd-build HEAD` lists 8 files. |
| Host | macOS, aarch64. Load average at the end of the run: 16.54, 22.39, 26.41. Other workers ran builds and tests on the same host during the run. |
| Sandbox | `scripts/dev-sandbox.sh` for build, clippy and test. `FLEET_ROOT`, `HORCH_ALLOW_LEDGER_MIGRATE` and `ANTHROPIC_API_KEY` unset. |

## The 3 commands

| Command | Exit code | Wall time |
|---|---|---|
| `cargo fmt --all --check` | 0 | 2 s |
| `scripts/dev-sandbox.sh cargo build --workspace --bins --locked` (step 2, run first) | 0 | 30 s |
| `cargo clippy --workspace --all-targets --locked` (run as `scripts/dev-sandbox.sh cargo clippy ...`) | 0 | 19 s |
| `scripts/dev-sandbox.sh cargo test --workspace --locked --no-fail-fast` | **1** | **4143 s (69 min 3 s)** |

The sum of the per-binary times that `cargo test` prints is 3793 s. The rest is compile and link time.

## Size of `target/`

| Moment | `du -sh target` |
|---|---|
| After build and clippy, before the tests | 2.7G |
| **After the test run (peak)** | **9.0G** |

`target/` only grows during a run, so the end value is the peak.

## Test totals

| Measure | Count |
|---|---|
| `test result` lines (test binaries and doc-test runs) | 370 |
| `test result` lines with FAILED | 77 (`cargo` prints `error: 76 targets failed`) |
| Tests passed | 4716 |
| Tests failed | 362 `FAILED` lines, 361 unique pairs of binary and test name (1 line repeats) |
| Tests ignored | 25 |
| Failing tests that are on the expected-failures list | 2 (see below). Both fail for another reason than the listed one. |
| Failing tests that are not on the list | 359 |

Every test binary ran to completion (`--no-fail-fast`).

## Clippy

Clippy exits 0 with 50 `warning:` lines (no `error:`). The warnings are in `crates/horch/src/cmd/messaging.rs:250`, `crates/horch/src/cmd/taskledger_spawn.rs:99`, `crates/horch-core/src/fleetd/worktree_custody/adapters/git.rs:823`, and test files of `crates/horch-core/tests/` (`jobs::run_job` takes `&mut` that it does not need, in 12 files) and `crates/horch-e2e/tests/` (`fleet18_messaging.rs:718`, `watchdog_liveness.rs:87`). The CI gate `cargo clippy --workspace --bins --release --locked -- -D warnings` was not run in this unit. This run did not check whether that gate passes. The 2 warnings of the `horch` bin and the 1 warning of the `horch-core` lib would fail it if they also appear in a release build.

## Causes of the failures (summary)

| Count | Cause |
|---|---|
| 160 | The test calls `horch ledger run new`. The command is retired (`horch ledger run new is retired. Add an idea: horch idea add`). |
| 135 | Other (see the table of failing tests). |
| 27 | The test reads the operator token and the token file does not exist (`reading the operator token ... stat failed: No such file or directory`). |
| 20 | A task is added to a run with no idea in flight (FR-12: `a task needs an idea`). |
| 7 | The test times out waiting for a fake agent or a transcript. |
| 6 | A test seed breaks `features.branch` uniqueness. |
| 6 | The test calls `horch spawn`. The command is retired. |

This table is a text match on the first panic line. The table of failing tests holds the exact line of each test. This unit does not diagnose the product cause of each failure.

## Failing tests that are on the expected-failures list

| Test | Binary | Expected-failures file:line | Listed reason | Actual failure |
|---|---|---|---|---|
| `g52_resume_uses_own_session` | `tests/fleet18_observability.rs` | `crates/horch-e2e/expected-failures/fleet18_observability.txt:7` | horch spawn refused: no ready task for persona codex-terra | timed out waiting for two session transcripts |
| `g53_ledger_handoff_compacts_resumes_and_records_progress_once` | `tests/fleet18_observability.rs` | `crates/horch-e2e/expected-failures/fleet18_observability.txt:14` | horch ledger run new is retired | called 'Result::unwrap()' on an 'Err' value: reading the operator token |

## Entries of the list whose test passed

None. No listed test body passed (the wrapper in `crates/horch-e2e/src/expected.rs` panics with `passed but is on the list of expected failures`, and that text appears 0 times in the output). The 9 entries are:

| Entry | File:line | Result of the test |
|---|---|---|
| `g52_resume_uses_own_session` | `fleet18_observability.txt:7` | FAILED (other message) |
| `g50_live_foreign_runs_and_legacy_orchestrators_warn_but_do_not_block` | `fleet18_observability.txt:12` | ok (failed for the listed reason) |
| `g51_sessions_points_to_ledger_status_only_for_the_callers_active_run` | `fleet18_observability.txt:13` | ok (failed for the listed reason) |
| `g53_ledger_handoff_compacts_resumes_and_records_progress_once` | `fleet18_observability.txt:14` | FAILED (other message) |
| `ledger_stop_stops_a_spawned_worker_by_the_ingested_pid` | `fleet18_observability.txt:15` | ok (failed for the listed reason) |
| `the_whole_flow_on_the_fake_herdr` | `taskledger.txt:8` | ok (failed for the listed reason) |
| `stopping_one_of_three_workers_leaves_the_others_running` | `taskledger.txt:10` | ok (failed for the listed reason) |
| `a_worker_with_no_progress_is_stalled_by_the_ladder_and_stopped_by_its_pid` | `taskledger.txt:12` | ok (failed for the listed reason) |

## Failing tests

`Binary wall time` is the `finished in` time of the whole test binary. `cargo test` prints no time per test. All failing tests are **not** on the expected-failures list unless the column says `yes`.

### horch-e2e (249 failing)

| Test | Binary | Binary wall time | In expected-failures | Expected-failures file:line | Failure summary |
|---|---|---|---|---|---|
| `arc_07_e2e_bin_overrides_reach_worker` | `tests/brief.rs` | 5.9 s | no | - | stdout: |
| `done_finishes_in_time_against_a_stuck_orchestrator` | `tests/done_budget.rs` | 5.3 s | no | - | called 'Result::unwrap()' on an 'Err' value: reading the operator token |
| `done_retried_after_a_kill_during_the_push_records_one_done` | `tests/done_budget.rs` | 5.3 s | no | - | called 'Result::unwrap()' on an 'Err' value: reading the operator token |
| `done_returns_in_time_when_herdr_hangs_on_pane_get` | `tests/done_budget.rs` | 5.3 s | no | - | called 'Result::unwrap()' on an 'Err' value: reading the operator token |
| `ledger_done_finishes_in_time_against_a_stuck_orchestrator` | `tests/done_budget.rs` | 5.3 s | no | - | called 'Result::unwrap()' on an 'Err' value: reading the operator token |
| `ledger_done_killed_during_the_wake_push_has_recorded_the_report` | `tests/done_budget.rs` | 5.3 s | no | - | called 'Result::unwrap()' on an 'Err' value: reading the operator token |
| `bal_04_exit_code_3` | `tests/e2e.rs` | 126.6 s | no | - | --force spawns: horch: horch spawn is retired. fleetd launches each attempt. To pair with a persona, plan its task with an interactive persona |
| `bal_04_gate_before_side_effects` | `tests/e2e.rs` | 126.6 s | no | - | assertion 'left == right' failed: horch: horch spawn is retired. fleetd launches each attempt. To pair with a persona, plan its task with an interacti |
| `bal_05_resume_uses_via` | `tests/e2e.rs` | 126.6 s | no | - | assertion failed: h.run(&["spawn", "researcher", "--from-pane", "w1:p1", |
| `bal_05_substituted_record_fields` | `tests/e2e.rs` | 126.6 s | no | - | horch: horch spawn is retired. fleetd launches each attempt. To pair with a persona, plan its task with an interactive persona |
| `bal_06_route_matches_gate` | `tests/e2e.rs` | 126.6 s | no | - | assertion 'left == right' failed: all-exhausted opus |
| `bal_08_fleet_auto_choice_all_exhausted` | `tests/e2e.rs` | 126.6 s | no | - | called 'Option::unwrap()' on a 'None' value |
| `bal_08_fleet_auto_choice_all_ok` | `tests/e2e.rs` | 126.6 s | no | - | called 'Option::unwrap()' on a 'None' value |
| `bal_08_fleet_auto_choice_claude_exhausted_codex_ok` | `tests/e2e.rs` | 126.6 s | no | - | called 'Option::unwrap()' on a 'None' value |
| `bal_08_fleet_auto_choice_claude_unknown_codex_ok` | `tests/e2e.rs` | 126.6 s | no | - | called 'Option::unwrap()' on a 'None' value |
| `tel_02_fleet_records_the_orchestrators_plugin_skills` | `tests/e2e.rs` | 126.6 s | no | - | timed out after 120 s waiting for the orchestrator's claude |
| `tel_02_fleet_writes_orchestrator_record` | `tests/e2e.rs` | 126.6 s | no | - | timed out after 120 s waiting for the orchestrator's claude |
| `tel_02_resume_refuses_orchestrator` | `tests/e2e.rs` | 126.6 s | no | - | horch: horch spawn is retired. fleetd launches each attempt. To pair with a persona, plan its task with an interactive persona |
| `phase4::ask_scenario_sends_one_question` | `tests/fakes.rs` | 13.4 s | no | - | horch ["ledger", "run", "new", "r1"]: horch ledger run new is retired. Add an idea: horch idea add "<idea>" |
| `phase4::bound_scenario_writes_entry_json_at_entry` | `tests/fakes.rs` | 13.4 s | no | - | horch ["ledger", "run", "new", "r1"]: horch ledger run new is retired. Add an idea: horch idea add "<idea>" |
| `phase4::noresult_scenario_writes_nothing` | `tests/fakes.rs` | 13.4 s | no | - | horch ["ledger", "run", "new", "r1"]: horch ledger run new is retired. Add an idea: horch idea add "<idea>" |
| `phase4::result_scenario_writes_result_json` | `tests/fakes.rs` | 13.4 s | no | - | horch ["ledger", "run", "new", "r1"]: horch ledger run new is retired. Add an idea: horch idea add "<idea>" |
| `g56_facts_reach_the_launch_context_and_are_project_scoped` | `tests/fleet18_brief_worktree.rs` | 23.6 s | no | - | ["ledger", "run", "new", "review"]: Output { status: ExitStatus(unix_wait_status(256)), stdout: "", stderr: "horch ledger run new is retired. Add an i |
| `g56_later_spawn_receives_recorded_fact` | `tests/fleet18_brief_worktree.rs` | 23.6 s | no | - | ["spawn", "sonnet", "--from-pane", "w1:p1", "--force", "--no-tile"]: Output { status: ExitStatus(unix_wait_status(256)), stdout: "", stderr: "horch: h |
| `g57_compacting_orchestrator_types_its_command_card` | `tests/fleet18_brief_worktree.rs` | 23.6 s | no | - | called 'Result::unwrap()' on an 'Err' value: reading the operator token |
| `g64_skill_states_each_delegation_and_reporting_rule_within_budget` | `tests/fleet18_brief_worktree.rs` | 23.6 s | no | - | missing verified (with evidence) or not verified |
| `g69_drop_keep_branch_refuses_detached_unique_commits` | `tests/fleet18_brief_worktree.rs` | 23.6 s | no | - | ["ledger", "run", "new", "review"]: Output { status: ExitStatus(unix_wait_status(256)), stdout: "", stderr: "horch ledger run new is retired. Add an i |
| `g69_drop_refuses_changed_checked_out_branch` | `tests/fleet18_brief_worktree.rs` | 23.6 s | no | - | ["ledger", "run", "new", "review"]: Output { status: ExitStatus(unix_wait_status(256)), stdout: "", stderr: "horch ledger run new is retired. Add an i |
| `g69_drop_refuses_detached_unique_commits` | `tests/fleet18_brief_worktree.rs` | 23.6 s | no | - | ["ledger", "run", "new", "review"]: Output { status: ExitStatus(unix_wait_status(256)), stdout: "", stderr: "horch ledger run new is retired. Add an i |
| `g69_from_scratch_and_drop_preserve_unpublished_work` | `tests/fleet18_brief_worktree.rs` | 23.6 s | no | - | ["ledger", "run", "new", "review"]: Output { status: ExitStatus(unix_wait_status(256)), stdout: "", stderr: "horch ledger run new is retired. Add an i |
| `g69_open_scratch_does_not_block_finish_or_done` | `tests/fleet18_brief_worktree.rs` | 23.6 s | no | - | ["ledger", "run", "new", "review"]: Output { status: ExitStatus(unix_wait_status(256)), stdout: "", stderr: "horch ledger run new is retired. Add an i |
| `g61_fleet_confirms_only_after_orchestrator_runs_inbox` | `tests/fleet18_delivery_probe.rs` | 7.3 s | no | - | horch 0.4.8 |
| `g61_fleet_probe_never_falls_back_to_separate_text_and_enter` | `tests/fleet18_delivery_probe.rs` | 7.3 s | no | - | horch 0.4.8 |
| `g61_fleet_without_ack_warns_and_keeps_probe_pending` | `tests/fleet18_delivery_probe.rs` | 7.3 s | no | - | horch 0.4.8 |
| `a_pass_with_an_unknown_orchestrator_ends_within_the_push_budget` | `tests/fleet18_messaging.rs` | 9.2 s | no | - | called 'Result::unwrap()' on an 'Err' value: run pinned has no idea in flight of its feature F-0001, and a task needs an idea (FR-12). Add one: horch  |
| `g60_codex_queue_confirmation_stays_pending_until_inbox` | `tests/fleet18_messaging.rs` | 9.2 s | no | - | called 'Result::unwrap()' on an 'Err' value: reading the operator token |
| `g60_plain_done_and_tell_without_registration_arrive_after_late_registration` | `tests/fleet18_messaging.rs` | 9.2 s | no | - | called 'Result::unwrap()' on an 'Err' value: reading the operator token |
| `g70_long_multiline_questions_and_blocked_survive_transport_ack_and_reply` | `tests/fleet18_messaging.rs` | 9.2 s | no | - | called 'Result::unwrap()' on an 'Err' value: run pinned has no idea in flight of its feature F-0001, and a task needs an idea (FR-12). Add one: horch  |
| `g70_pinned_cli_question_reply_resets_stalls` | `tests/fleet18_messaging.rs` | 9.2 s | no | - | called 'Result::unwrap()' on an 'Err' value: run pinned has no idea in flight of its feature F-0001, and a task needs an idea (FR-12). Add one: horch  |
| `g70_pinned_cli_sixty_minute_review_wait_and_normal_reap_after_completion` | `tests/fleet18_messaging.rs` | 9.2 s | no | - | called 'Result::unwrap()' on an 'Err' value: run pinned has no idea in flight of its feature F-0001, and a task needs an idea (FR-12). Add one: horch  |
| `g70_pinned_cli_wait_expires_and_dead_agent_is_immediate` | `tests/fleet18_messaging.rs` | 9.2 s | no | - | called 'Result::unwrap()' on an 'Err' value: run pinned has no idea in flight of its feature F-0001, and a task needs an idea (FR-12). Add one: horch  |
| `g70_question_wait_clears_on_orchestrator_answer_and_stalls_restart` | `tests/fleet18_messaging.rs` | 9.2 s | no | - | called 'Result::unwrap()' on an 'Err' value: run waits has no idea in flight of its feature F-0001, and a task needs an idea (FR-12). Add one: horch i |
| `g70_reviewer_wait_holds_sixty_minutes_then_three_idle_checks_reap` | `tests/fleet18_messaging.rs` | 9.2 s | no | - | called 'Result::unwrap()' on an 'Err' value: run waits has no idea in flight of its feature F-0001, and a task needs an idea (FR-12). Add one: horch i |
| `g70_waiting_background_deadline_and_dead_process` | `tests/fleet18_messaging.rs` | 9.2 s | no | - | called 'Result::unwrap()' on an 'Err' value: run waits has no idea in flight of its feature F-0001, and a task needs an idea (FR-12). Add one: horch i |
| `g49_spawn_doctor_and_inbox_warn_once_and_name_installed_binary` | `tests/fleet18_observability.rs` | 36.0 s | no | - | horch: WARNING: stale horch: running v0.4.8; cached latest v99.0.0; running <path> differs from installed <path> |
| `g50_legacy_orchestrator_and_host_load_are_advisory` | `tests/fleet18_observability.rs` | 36.0 s | no | - | called 'Result::unwrap()' on an 'Err' value: reading the operator token |
| `g51_pane_gone_context_filters_and_completed_summaries` | `tests/fleet18_observability.rs` | 36.0 s | no | - | called 'Result::unwrap()' on an 'Err' value: reading the operator token |
| `g51_sessions_filter_workspace_live_role_and_indent_summary` | `tests/fleet18_observability.rs` | 36.0 s | no | - | called 'Result::unwrap()' on an 'Err' value: reading the operator token |
| `g52_concurrent_workers_get_their_own_session_and_transcript` | `tests/fleet18_observability.rs` | 36.0 s | no | - | timed out waiting for two session transcripts |
| `g52_done_discovery_uses_the_same_launch_token` | `tests/fleet18_observability.rs` | 36.0 s | no | - | horch: horch spawn is retired. fleetd launches each attempt. To pair with a persona, plan its task with an interactive persona |
| `g52_resume_uses_own_session` | `tests/fleet18_observability.rs` | 36.0 s | yes | `fleet18_observability.txt:7` | timed out waiting for two session transcripts |
| `g53_below_threshold_handoff_does_not_schedule` | `tests/fleet18_observability.rs` | 36.0 s | no | - | called 'Result::unwrap()' on an 'Err' value: reading the operator token |
| `g53_first_handoff_without_prior_warning_does_not_schedule` | `tests/fleet18_observability.rs` | 36.0 s | no | - | called 'Result::unwrap()' on an 'Err' value: reading the operator token |
| `g53_ledger_handoff_compacts_resumes_and_records_progress_once` | `tests/fleet18_observability.rs` | 36.0 s | yes | `fleet18_observability.txt:14` | called 'Result::unwrap()' on an 'Err' value: reading the operator token |
| `g53_progress_warns_once_and_missing_handoff_is_refused` | `tests/fleet18_observability.rs` | 36.0 s | no | - | called 'Result::unwrap()' on an 'Err' value: reading the operator token |
| `a_scratch_fleet_warns_about_the_shared_ledger_and_does_not_create_it` | `tests/fleet_start.rs` | 6.3 s | no | - | horch: horch spawn is retired. fleetd launches each attempt. To pair with a persona, plan its task with an interactive persona |
| `a_fleet_with_an_abandoned_worktree_spawns_the_worktree_steward` | `tests/i308_fleet_steward.rs` | 6.1 s | no | - | horch ledger run new alpha: status: exit status: 1 |
| `a_fleet_with_no_parked_worktree_spawns_no_steward` | `tests/i308_fleet_steward.rs` | 6.1 s | no | - | assertion 'left == right' failed: only the orchestrator: [] |
| `a_worker_can_drop_a_clean_worktree_from_an_explicit_other_run` | `tests/i308_fleet_steward.rs` | 6.1 s | no | - | horch ledger run new alpha: status: exit status: 1 |
| `an_exhausted_claude_pool_starts_no_codex_steward` | `tests/i308_fleet_steward.rs` | 6.1 s | no | - | horch ledger run new alpha: status: exit status: 1 |
| `the_no_change_steward_task_finishes_without_a_worktree` | `tests/i308_fleet_steward.rs` | 6.1 s | no | - | horch ledger run new alpha: status: exit status: 1 |
| `triage_delete_accepts_a_clean_detached_head_reachable_from_a_branch` | `tests/i308_fleet_steward.rs` | 6.1 s | no | - | horch ledger run new alpha: status: exit status: 1 |
| `triage_delete_refuses_a_detached_unique_commit` | `tests/i308_fleet_steward.rs` | 6.1 s | no | - | horch ledger run new alpha: status: exit status: 1 |
| `triage_lists_rows_from_a_ledger_it_cannot_write` | `tests/i308_fleet_steward.rs` | 6.1 s | no | - | horch ledger run new alpha: status: exit status: 1 |
| `archive_restore_and_gc_through_the_cli` | `tests/i308_triage.rs` | 9.6 s | no | - | W1 feat ARCHIVE: queued in the triage_apply job 8; horch worktree triage shows the result |
| `finish_queues_a_task_that_inherits_the_worktree` | `tests/i308_triage.rs` | 9.6 s | no | - | horch ledger run new alpha: status: exit status: 1 |
| `run_close_abandon_cancels_then_refuses_until_the_worktree_is_decided` | `tests/i308_triage.rs` | 9.6 s | no | - | horch ledger run new alpha: status: exit status: 1 |
| `run_close_refuses_while_a_parked_worktree_has_no_decision` | `tests/i308_triage.rs` | 9.6 s | no | - | horch ledger run new alpha: status: exit status: 1 |
| `antigravity_child_never_receives_a_forbidden_key` | `tests/lifecycle.rs` | 6.3 s | no | - | status: exit status: 1 |
| `arc_16_crash_after_insert_not_live` | `tests/lifecycle.rs` | 6.3 s | no | - | assertion 'left == right' failed: status: exit status: 1 |
| `arc_16_e2e_fail_split` | `tests/lifecycle.rs` | 6.3 s | no | - | assertion 'left == right' failed: [] |
| `arc_26_e2e_lifecycle_matrix_antigravity` | `tests/lifecycle.rs` | 6.3 s | no | - | status: exit status: 1 |
| `arc_26_e2e_lifecycle_matrix_claude` | `tests/lifecycle.rs` | 6.3 s | no | - | status: exit status: 1 |
| `arc_26_e2e_lifecycle_matrix_codex` | `tests/lifecycle.rs` | 6.3 s | no | - | status: exit status: 1 |
| `arc_26_e2e_lifecycle_matrix_opencode` | `tests/lifecycle.rs` | 6.3 s | no | - | status: exit status: 1 |
| `arc_26_e2e_lifecycle_matrix_pi` | `tests/lifecycle.rs` | 6.3 s | no | - | status: exit status: 1 |
| `arc_26_e2e_lifecycle_matrix_prime` | `tests/lifecycle.rs` | 6.3 s | no | - | status: exit status: 1 |
| `pane_command_removes_a_forbidden_key_before_horch_starts` | `tests/lifecycle.rs` | 6.3 s | no | - | status: exit status: 1 |
| `spawn_model_fable_reaches_the_claude_command_line_and_the_resume` | `tests/lifecycle.rs` | 6.3 s | no | - | status: exit status: 1 |
| `tel_session_recorded_when_agent_ends_fast` | `tests/lifecycle.rs` | 6.3 s | no | - | status: exit status: 1 |
| `tel_session_recorded_when_prime_ends_fast` | `tests/lifecycle.rs` | 6.3 s | no | - | status: exit status: 1 |
| `worker_warns_once_about_a_teammate_file_that_does_not_load` | `tests/lifecycle.rs` | 6.3 s | no | - | status: exit status: 1 |
| `fm3_idle_claim_is_caught_in_under_10_min` | `tests/p0_liveness.rs` | 14.5 s | no | - | horch ledger run new eps: status: exit status: 1 |
| `fm4_dead_agent_ends_its_attempt_within_30_s` | `tests/p0_liveness.rs` | 14.5 s | no | - | horch ledger run new eps: status: exit status: 1 |
| `fm5_usage_limited_worker_is_not_reaped_and_burns_no_attempt` | `tests/p0_liveness.rs` | 14.5 s | no | - | horch ledger run new eps: status: exit status: 1 |
| `sleep_does_not_expire_a_lease` | `tests/p0_liveness.rs` | 14.5 s | no | - | horch ledger run new eps: status: exit status: 1 |
| `fm13_herdr_restart_ends_attempts_and_queues_tasks_again` | `tests/p0_rescue.rs` | 14.3 s | no | - | horch ledger run new fm13: status: exit status: 1 |
| `fm13_herdr_restart_rescues_each_worktree_and_the_next_attempt_reuses_it` | `tests/p0_rescue.rs` | 14.3 s | no | - | horch ledger run new fm13: status: exit status: 1 |
| `fm25_killed_worker_with_dirty_and_untracked_files_loses_nothing` | `tests/p0_rescue.rs` | 14.3 s | no | - | horch ledger run new fm25 --stall-rounds 1: status: exit status: 1 |
| `fm34_daemon_restart_on_a_new_binary_keeps_the_run` | `tests/p0_supervision.rs` | 15.8 s | no | - | horch ledger run new zeta --check-every 1 --stall-rounds 1000: status: exit status: 1 |
| `fm7_killed_watchdog_does_not_stop_the_reconcile` | `tests/p0_supervision.rs` | 15.8 s | no | - | horch ledger run new zeta --check-every 1 --stall-rounds 1000: status: exit status: 1 |
| `job_of_5_min_runs_while_fleetd_stays_healthy_and_is_not_restarted` | `tests/p1_jobs.rs` | 9.4 s | no | - | horch ledger run new jobs --check /bin/sleep 300: status: exit status: 1 |
| `job_past_its_limit_runs_again_after_its_backoff_and_after_3_abandoned_runs_goes_to_you` | `tests/p1_jobs.rs` | 9.4 s | no | - | horch ledger run new jobs --check /bin/sleep 600: status: exit status: 1 |
| `fm11_fleetd_aborted_after_the_commit_does_each_effect_once_on_restart` | `tests/p1_pass.rs` | 9.0 s | no | - | horch ledger run new alpha --steps check --check true --check-every 3600 --stall-rounds 1000: status: exit status: 1 |
| `rr4_tasks_complete_with_all_file_watch_events_dropped` | `tests/p1_pass.rs` | 9.0 s | no | - | the test fleetd exited (Some(ExitStatus(unix_wait_status(0)))): |
| `rr7_fleetd_killed_mid_run_ends_as_a_clean_run` | `tests/p1_pass.rs` | 9.0 s | no | - | the test fleetd exited (Some(ExitStatus(unix_wait_status(0)))): |
| `scans_of_10000_folders_keep_the_heartbeat_under_30_s_and_go_on_at_their_cursors_after_a_restart` | `tests/p1_pass.rs` | 9.0 s | no | - | horch ledger run new alpha --steps check --check true --check-every 3600 --stall-rounds 1000: status: exit status: 1 |
| `fleetd_runs_no_pass_while_an_old_watchdog_may_still_run` | `tests/p1_release.rs` | 6.6 s | no | - | horch ledger run new zeta --check-every 1 --stall-rounds 1000: status: exit status: 1 |
| `update_with_a_live_worker_and_a_live_old_watchdog_leaves_1_control_path` | `tests/p1_release.rs` | 6.6 s | no | - | horch ledger run new zeta --check-every 1 --stall-rounds 1000: status: exit status: 1 |
| `cancel_of_a_blocked_on_user_task_is_immediate` | `tests/p2_leases.rs` | 24.0 s | no | - | horch ledger run new p2: status: exit status: 1 |
| `cancel_of_a_running_task_stops_its_process_before_the_task_is_cancelled` | `tests/p2_leases.rs` | 24.0 s | no | - | horch ledger run new p2: status: exit status: 1 |
| `cancel_of_a_waiting_quota_task_is_immediate` | `tests/p2_leases.rs` | 24.0 s | no | - | horch ledger run new p2: status: exit status: 1 |
| `fm14_no_second_attempt_starts_while_the_old_process_lives` | `tests/p2_leases.rs` | 24.0 s | no | - | horch ledger run new p2: status: exit status: 1 |
| `rr1_stopped_worker_is_caught_within_1_lease` | `tests/p2_leases.rs` | 24.0 s | no | - | horch ledger run new p2: status: exit status: 1 |
| `rr2_expired_lease_never_leaves_the_task_assigned` | `tests/p2_leases.rs` | 24.0 s | no | - | horch ledger run new p2: status: exit status: 1 |
| `rr6_task_that_fails_each_attempt_escalates_after_max_attempts` | `tests/p2_leases.rs` | 24.0 s | no | - | horch ledger run new p2 --max-attempts 2: status: exit status: 1 |
| `held_task_with_a_live_worker_on_phase_1_is_not_reaped_after_the_update_and_has_1_lease` | `tests/p2_release.rs` | 10.6 s | no | - | horch ledger show T-1 --json: status: exit status: 1 |
| `cleanup_after_land_removes_a_landed_tree_through_the_remove_job` | `tests/p3_refusals.rs` | 19.1 s | no | - | horch ledger run new alpha --steps land,cleanup: status: exit status: 1 |
| `cleanup_of_a_landed_tree_with_an_untracked_file_is_refused_and_marked_cleanup_failed` | `tests/p3_refusals.rs` | 19.1 s | no | - | horch ledger run new alpha --steps land,cleanup: status: exit status: 1 |
| `dismiss_cleanup_rescues_then_removes_only_the_recorded_path` | `tests/p3_refusals.rs` | 19.1 s | no | - | horch ledger run new alpha --steps land: status: exit status: 1 |
| `drop_of_a_clean_tree_writes_a_verified_rescue_ref_before_the_removal` | `tests/p3_refusals.rs` | 19.1 s | no | - | horch ledger run new alpha --steps land: status: exit status: 1 |
| `drop_of_a_merged_branch_deletes_it_and_keep_branch_keeps_it` | `tests/p3_refusals.rs` | 19.1 s | no | - | horch ledger run new alpha --steps land: status: exit status: 1 |
| `rr13_drop_of_a_branch_with_a_stash_entry_is_refused_and_the_tree_stays` | `tests/p3_refusals.rs` | 19.1 s | no | - | horch ledger run new alpha --steps land: status: exit status: 1 |
| `rr13_drop_of_a_dirty_tree_is_refused_and_the_tree_stays` | `tests/p3_refusals.rs` | 19.1 s | no | - | horch ledger run new alpha --steps land: status: exit status: 1 |
| `rr13_drop_of_unpublished_unique_commits_is_refused_and_the_tree_stays` | `tests/p3_refusals.rs` | 19.1 s | no | - | horch ledger run new alpha --steps land: status: exit status: 1 |
| `rr13_drop_with_a_head_that_moved_during_the_checks_is_refused_and_the_tree_stays` | `tests/p3_refusals.rs` | 19.1 s | no | - | horch ledger run new alpha --steps land: status: exit status: 1 |
| `run_gate_folder_is_added_and_removed_through_the_jobs` | `tests/p3_refusals.rs` | 19.1 s | no | - | horch ledger run new alpha --steps land: status: exit status: 1 |
| `live_tree_and_dirty_orphan_on_phase_2_are_active_and_listed_and_nothing_is_removed_after_the_update` | `tests/p3_release.rs` | 14.6 s | no | - | assertion 'left == right' failed: status: exit status: 0 |
| `pres_04_l5_unlanded_task_keeps_worktree_and_blocks_dependents` | `tests/p3_unlanded.rs` | 11.8 s | no | - | horch ledger run new alpha --steps land: status: exit status: 1 |
| `pres_04_l9_unlanded_work_is_listed_and_decided_once` | `tests/p3_unlanded.rs` | 11.8 s | no | - | horch ledger run new alpha --steps land: status: exit status: 1 |
| `rr14_requeue_of_a_parked_task_runs_in_the_same_tree` | `tests/p3_unlanded.rs` | 11.8 s | no | - | horch ledger run new alpha --steps land: status: exit status: 1 |
| `rr15_landed_work_of_a_done_task_is_not_listed` | `tests/p3_unlanded.rs` | 11.8 s | no | - | horch ledger run new alpha --steps land: status: exit status: 1 |
| `rr15_orphaned_and_rescued_trees_are_listed_with_their_rescue_ref` | `tests/p3_unlanded.rs` | 11.8 s | no | - | horch ledger run new alpha --steps land: status: exit status: 1 |
| `rr15_rescued_work_that_did_not_land_stays_listed_after_the_task_is_done` | `tests/p3_unlanded.rs` | 11.8 s | no | - | horch ledger run new alpha --steps land: status: exit status: 1 |
| `rr18_requeue_of_an_escalated_task_sets_its_attempt_count_to_0` | `tests/p3_unlanded.rs` | 11.8 s | no | - | horch ledger run new alpha --steps land --max-attempts 2: status: exit status: 1 |
| `rr18_review_requeue_keeps_the_attempt_count` | `tests/p3_unlanded.rs` | 11.8 s | no | - | horch ledger run new alpha --steps code-sweep: status: exit status: 1 |
| `fm26_tree_made_outside_fleetd_is_rescued_and_its_files_are_on_the_ref` | `tests/p3_worktrees.rs` | 15.4 s | no | - | horch ledger run new fm26: status: exit status: 1 |
| `fm27_only_landed_or_rescued_trees_are_removed` | `tests/p3_worktrees.rs` | 15.4 s | no | - | horch ledger run new fm27 --steps land: status: exit status: 1 |
| `fm27_vanished_tree_opens_1_needs_you_item_within_60_s` | `tests/p3_worktrees.rs` | 15.4 s | no | - | horch ledger run new fm27: status: exit status: 1 |
| `fm28_over_budget_removes_finished_trees_and_holds_dispatch_before_an_active_tree` | `tests/p3_worktrees.rs` | 15.4 s | no | - | horch ledger run new old: status: exit status: 1 |
| `fr11_worker_own_git_worktree_add_is_flagged_on_the_next_pass` | `tests/p3_worktrees.rs` | 15.4 s | no | - | horch ledger run new fr11: status: exit status: 1 |
| `rr10_hand_made_worktree_is_adopted_within_1_list_period` | `tests/p3_worktrees.rs` | 15.4 s | no | - | horch ledger run new rr10: status: exit status: 1 |
| `slow_worktree_add_of_60_s_keeps_the_heartbeat_and_the_tree_is_used_only_after_its_job_passed` | `tests/p3_worktrees.rs` | 15.4 s | no | - | horch ledger run new slow: status: exit status: 1 |
| `fr5_worker_never_runs_a_task_search` | `tests/p4_safety.rs` | 40.4 s | no | - | This is a conformance check of horch. Do each step, then give 1 JSON object as your last message, with no other text. |
| `fm9_code_task_with_a_failing_check_stays_not_done` | `tests/p4_verifier.rs` | 138.3 s | no | - | timed out after 120 s waiting for the fake claude to write result.json |
| `fm9_docs_task_without_its_section_stays_not_done` | `tests/p4_verifier.rs` | 138.3 s | no | - | git ["add", "docs/guide.md"]: status: exit status: 128 |
| `passing_code_task_reaches_done_only_through_the_verifier` | `tests/p4_verifier.rs` | 138.3 s | no | - | timed out after 120 s waiting for the fake claude to write result.json |
| `sr4_check_that_writes_outside_its_paths_is_refused` | `tests/p4_verifier.rs` | 138.3 s | no | - | timed out after 120 s waiting for the fake claude to write result.json |
| `operator_request_from_a_test_in_a_gate_is_refused_for_each_act` | `tests/p5_operator_boundary.rs` | 36.1 s | no | - | assertion 'left == right' failed: the gate changes no state of the ledger |
| `fm17_codex_worker_needs_no_ledger_allow_rule` | `tests/p5_read_only_worker.rs` | 8.8 s | no | - | assertion 'left == right' failed: report_progress: {"exit":1,"sandboxed":true,"stderr":"horch: unable to open database file: Error code 14: Unable to  |
| `fm17_codex_worker_writes_its_result_in_the_task_folder` | `tests/p5_read_only_worker.rs` | 8.8 s | no | - | assertion 'left == right' failed: report_result: {"exit":1,"sandboxed":true,"stderr":"horch result: unable to open database file: Error code 14: Unabl |
| `worker_with_a_read_only_ledger_reports_progress_and_done_with_no_error` | `tests/p5_read_only_worker.rs` | 8.8 s | no | - | assertion 'left == right' failed: report_progress: {"exit":1,"sandboxed":true,"stderr":"horch: unable to open database file: Error code 14: Unable to  |
| `old_claim_worker_runs_progress_and_done_after_the_update_with_1_effect_each` | `tests/p5_release.rs` | 31.7 s | no | - | the test leaked 3 process(es) that outlived teardown by 5s; teardown killed them: |
| `old_push_worker_runs_progress_and_done_after_the_update_with_1_effect_each` | `tests/p5_release.rs` | 31.7 s | no | - | the test leaked 3 process(es) that outlived teardown by 5s; teardown killed them: |
| `update_stops_no_run` | `tests/p5_release.rs` | 31.7 s | no | - | the test leaked 3 process(es) that outlived teardown by 5s; teardown killed them: |
| `new_fake_cli_version_fails_a_check_marks_its_profiles_unhealthy_and_adds_1_idea` | `tests/p7_docs_sync.rs` | 166.6 s | no | - | assertion 'left == right' failed: Job { id: 7, subject: "claude de8a6c422c364c6c", run: 1, state: "passed", repo: None } |
| `passed_conformance_marks_the_profiles_healthy_and_dispatch_resumes` | `tests/p7_docs_sync.rs` | 166.6 s | no | - | assertion 'left == right' failed: Job { id: 7, subject: "claude 6ebaebbf126df5df", run: 1, state: "passed", repo: None } |
| `unhealthy_profile_gets_no_dispatch_and_status_names_the_hold` | `tests/p7_docs_sync.rs` | 166.6 s | no | - | assertion 'left == right' failed: Job { id: 7, subject: "claude d3ed9a0238641a0a", run: 1, state: "passed", repo: None } |
| `a_live_relay_gets_the_fleet_probe_and_the_idle_pane_gets_no_text` | `tests/relay_first_delivery.rs` | 26.1 s | no | - | horch 0.4.8 |
| `a_restored_worker_pane_tells_as_its_role_peeks_its_inbox_and_finishes` | `tests/restored_pane.rs` | 24.9 s | no | - | status: exit status: 1 |
| `arc_14_provenance_on_spawn_substitute_resume` | `tests/routing_provenance.rs` | 10.1 s | no | - | ["spawn", "codex-sol", "--from-pane", "w1:p1", "--no-tile"] |
| `the_hermetic_story` | `tests/scenario.rs` | 24.1 s | no | - | assertion 'left == right' failed |
| `the_hermetic_story_with_an_ollama_that_starts_in_7_s` | `tests/scenario.rs` | 24.1 s | no | - | assertion 'left == right' failed |
| `a_controller_in_a_run_spawns_a_role_that_joins_the_run` | `tests/skill_intake_spawn.rs` | 22.3 s | no | - | status: exit status: 1 |
| `a_report_role_with_a_row_and_no_task_runs_to_done` | `tests/skill_intake_spawn.rs` | 22.3 s | no | - | status: exit status: 1 |
| `trf_43_a_controller_outside_every_run_cannot_send_a_brief` | `tests/skill_intake_spawn.rs` | 22.3 s | no | - | status: exit status: 1 |
| `trf_43_a_spoofed_pane_id_cannot_send_a_brief` | `tests/skill_intake_spawn.rs` | 22.3 s | no | - | status: exit status: 1 |
| `trf_43_a_worker_without_spawn_only_cannot_send_a_brief` | `tests/skill_intake_spawn.rs` | 22.3 s | no | - | status: exit status: 1 |
| `git_env_cannot_reach_another_repository` | `tests/skills_exposure.rs` | 30.1 s | no | - | stdout: |
| `operator_skills_e2e_changed_since_spawn_fails_the_launch` | `tests/skills_exposure.rs` | 30.1 s | no | - | spawn op-drift: stdout: |
| `operator_skills_e2e_check_warns_on_a_missing_name` | `tests/skills_exposure.rs` | 30.1 s | no | - | stdout: |
| `operator_skills_e2e_deleted_since_spawn_fails_the_launch` | `tests/skills_exposure.rs` | 30.1 s | no | - | spawn op-gone: stdout: |
| `operator_skills_e2e_exposure_claude` | `tests/skills_exposure.rs` | 30.1 s | no | - | stdout: |
| `operator_skills_e2e_exposure_codex` | `tests/skills_exposure.rs` | 30.1 s | no | - | spawn op-codex: stdout: |
| `operator_skills_e2e_ledger_record_lists_them` | `tests/skills_exposure.rs` | 30.1 s | no | - | spawn op-record: stdout: |
| `plugin_skills_e2e_changed_since_spawn_fails_the_launch` | `tests/skills_exposure.rs` | 30.1 s | no | - | spawn plug-drift: stdout: |
| `plugin_skills_e2e_ledger_record_lists_them` | `tests/skills_exposure.rs` | 30.1 s | no | - | spawn plug-record: stdout: |
| `skl_06_e2e_exposure_antigravity` | `tests/skills_exposure.rs` | 30.1 s | no | - | stdout: |
| `skl_06_e2e_exposure_claude` | `tests/skills_exposure.rs` | 30.1 s | no | - | spawn skl-claude: stdout: |
| `skl_06_e2e_exposure_codex` | `tests/skills_exposure.rs` | 30.1 s | no | - | spawn skl-codex: stdout: |
| `skl_06_e2e_exposure_opencode` | `tests/skills_exposure.rs` | 30.1 s | no | - | spawn skl-opencode: stdout: |
| `skl_06_e2e_exposure_pi` | `tests/skills_exposure.rs` | 30.1 s | no | - | spawn skl-pi: stdout: |
| `skl_06_e2e_exposure_prime` | `tests/skills_exposure.rs` | 30.1 s | no | - | spawn skl-prime: stdout: |
| `skl_06_e2e_marketplace_offline` | `tests/skills_exposure.rs` | 30.1 s | no | - | spawn skl-market: stdout: |
| `worker_reads_the_skill_store_its_spawner_read` | `tests/skills_exposure.rs` | 30.1 s | no | - | spawn: stdout: |
| `a_worker_that_registers_after_the_wait_still_works` | `tests/startup_error.rs` | 8.9 s | no | - | status: exit status: 1 |
| `spawn_fails_with_the_error_of_a_worker_whose_brief_is_missing` | `tests/startup_error.rs` | 8.9 s | no | - | spawn's stderr must hold the worker's error: status: exit status: 1 |
| `spawn_waits_for_a_worker_that_registers_and_prints_only_the_pane` | `tests/startup_error.rs` | 8.9 s | no | - | status: exit status: 1 |
| `a_cancel_during_the_land_lands_nothing` | `tests/taskledger.rs` | 247.1 s | no | - | horch ledger run new r2 --steps land,cleanup: status: exit status: 1 |
| `a_cancel_names_the_tasks_that_wait_for_it` | `tests/taskledger.rs` | 247.1 s | no | - | horch ledger run new a: status: exit status: 1 |
| `a_cancelled_review_is_queued_again_and_the_sweep_then_passes` | `tests/taskledger.rs` | 247.1 s | no | - | horch ledger run new alpha --steps code-sweep: status: exit status: 1 |
| `a_cancelled_task_does_not_land_or_turn_done` | `tests/taskledger.rs` | 247.1 s | no | - | horch ledger run new r2 --steps land,cleanup: status: exit status: 1 |
| `a_check_past_its_timeout_is_killed_with_its_children` | `tests/taskledger.rs` | 247.1 s | no | - | horch ledger run new alpha --steps check: status: exit status: 1 |
| `a_closed_run_frees_the_project_for_the_next_run` | `tests/taskledger.rs` | 247.1 s | no | - | horch ledger run new a: status: exit status: 1 |
| `a_codex_workers_check_cannot_write_outside_its_worktree` | `tests/taskledger.rs` | 247.1 s | no | - | horch ledger run new alpha --steps check: status: exit status: 1 |
| `a_codex_workers_sandboxed_check_past_its_timeout_leaves_no_process` | `tests/taskledger.rs` | 247.1 s | no | - | horch ledger run new alpha --steps check: status: exit status: 1 |
| `a_command_from_another_workspace_says_whose_run_it_uses` | `tests/taskledger.rs` | 247.1 s | no | - | horch ledger run new a: status: exit status: 1 |
| `a_command_means_the_run_it_names_before_the_run_of_its_worker` | `tests/taskledger.rs` | 247.1 s | no | - | horch ledger run new a: status: exit status: 1 |
| `a_commit_after_the_land_is_checked_and_reviewed_again` | `tests/taskledger.rs` | 247.1 s | no | - | horch ledger run new alpha --steps check,code-sweep,land,cleanup: status: exit status: 1 |
| `a_complexity_report_past_64_kib_is_read_whole` | `tests/taskledger.rs` | 247.1 s | no | - | horch ledger run new alpha --steps complexity-sweep: status: exit status: 1 |
| `a_critical_finding_fails_until_a_fix_is_reviewed_again` | `tests/taskledger.rs` | 247.1 s | no | - | horch ledger run new alpha --steps code-sweep: status: exit status: 1 |
| `a_dead_fleetd_is_restarted_by_the_next_command` | `tests/taskledger.rs` | 247.1 s | no | - | horch ledger run new zeta --check-every 1: status: exit status: 1 |
| `a_detached_finish_returns_at_once_and_mails_the_result` | `tests/taskledger.rs` | 247.1 s | no | - | horch ledger run new alpha --steps check: status: exit status: 1 |
| `a_failed_push_after_a_land_is_retried_by_the_next_finish` | `tests/taskledger.rs` | 247.1 s | no | - | horch ledger run new alpha --steps land,cleanup: status: exit status: 1 |
| `a_failed_reviews_in_push_is_redelivered_until_the_author_runs_finish` | `tests/taskledger.rs` | 247.1 s | no | - | horch ledger run new alpha --steps code-sweep: status: exit status: 1 |
| `a_failed_reviews_in_tell_wakes_the_orchestrator_once` | `tests/taskledger.rs` | 247.1 s | no | - | horch ledger run new alpha --steps code-sweep: status: exit status: 1 |
| `a_killed_detached_finish_is_reported_by_the_next_finish` | `tests/taskledger.rs` | 247.1 s | no | - | horch ledger run new alpha --steps check: status: exit status: 1 |
| `a_land_checks_outside_the_lock_and_lands_after_a_faster_task` | `tests/taskledger.rs` | 247.1 s | no | - | horch ledger run new alpha --steps land: status: exit status: 1 |
| `a_landed_worktree_passes_land_at_once_in_a_run_with_no_remote` | `tests/taskledger.rs` | 247.1 s | no | - | horch ledger run new alpha --steps land,cleanup: status: exit status: 1 |
| `a_landed_worktree_with_no_new_commit_passes_land_without_a_push` | `tests/taskledger.rs` | 247.1 s | no | - | horch ledger run new alpha --steps land,cleanup: status: exit status: 1 |
| `a_live_worker_that_does_not_claim_is_reported_once` | `tests/taskledger.rs` | 247.1 s | no | - | horch ledger run new a --heartbeat-stale 100000: status: exit status: 1 |
| `a_missing_reviewer_prompt_queues_no_review` | `tests/taskledger.rs` | 247.1 s | no | - | horch ledger run new alpha --steps code-sweep: status: exit status: 1 |
| `a_new_tree_supersedes_the_open_reviews_of_the_old_round` | `tests/taskledger.rs` | 247.1 s | no | - | horch ledger run new alpha --steps code-sweep: status: exit status: 1 |
| `a_queued_review_moves_to_its_fallback_persona` | `tests/taskledger.rs` | 247.1 s | no | - | horch ledger run new alpha --steps code-sweep: status: exit status: 1 |
| `a_refused_spawn_ends_its_worker_row` | `tests/taskledger.rs` | 247.1 s | no | - | horch ledger run new a: status: exit status: 1 |
| `a_replacement_commit_after_the_land_lands_and_is_pushed` | `tests/taskledger.rs` | 247.1 s | no | - | horch ledger run new alpha --steps land,cleanup: status: exit status: 1 |
| `a_replacement_with_the_same_tree_reuses_the_finished_round` | `tests/taskledger.rs` | 247.1 s | no | - | horch ledger run new alpha --steps code-sweep: status: exit status: 1 |
| `a_reviewer_that_cannot_run_hands_the_round_to_its_fallback` | `tests/taskledger.rs` | 247.1 s | no | - | horch ledger run new alpha --steps code-sweep: status: exit status: 1 |
| `a_rust_only_change_is_not_sent_to_the_complexity_analyser` | `tests/taskledger.rs` | 247.1 s | no | - | horch ledger run new alpha --steps complexity-sweep: status: exit status: 1 |
| `a_shared_task_claims_on_the_run_branch_and_lands_only_new_clean_commits` | `tests/taskledger.rs` | 247.1 s | no | - | horch ledger run new alpha --steps check,land: status: exit status: 1 |
| `a_spawn_joins_the_run_of_its_own_workspace` | `tests/taskledger.rs` | 247.1 s | no | - | horch ledger run new a: status: exit status: 1 |
| `a_spawn_of_an_unknown_teammate_leaves_no_live_row` | `tests/taskledger.rs` | 247.1 s | no | - | horch ledger run new a: status: exit status: 1 |
| `a_stuck_run_wakes_from_the_watchdog_when_no_task_finishes` | `tests/taskledger.rs` | 247.1 s | no | - | horch ledger run new a: status: exit status: 1 |
| `a_task_finishes_through_review_land_pr_and_cleanup` | `tests/taskledger.rs` | 247.1 s | no | - | horch ledger run new alpha --verify full: status: exit status: 1 |
| `a_task_reaped_max_attempts_times_fails_and_its_replan_requeues_it` | `tests/taskledger.rs` | 247.1 s | no | - | horch ledger run new a --max-attempts 2: status: exit status: 1 |
| `add_map_queues_a_ticket_map_in_one_step` | `tests/taskledger.rs` | 247.1 s | no | - | horch ledger run new beta: status: exit status: 1 |
| `an_outer_git_dir_reaches_no_child_of_finish` | `tests/taskledger.rs` | 247.1 s | no | - | horch ledger run new alpha --steps check,complexity-sweep,land,pr: status: exit status: 1 |
| `an_unlanded_done_names_its_dependents_and_a_stuck_run_wakes_once` | `tests/taskledger.rs` | 247.1 s | no | - | horch ledger run new a: status: exit status: 1 |
| `close_abandon_cancels_the_open_tasks` | `tests/taskledger.rs` | 247.1 s | no | - | horch ledger run new a: status: exit status: 1 |
| `finish_finds_the_code_sweep_skill_in_the_plugins_of_the_home` | `tests/taskledger.rs` | 247.1 s | no | - | horch ledger run new alpha --steps code-sweep: status: exit status: 1 |
| `finish_finds_the_code_sweep_skill_in_the_skill_store` | `tests/taskledger.rs` | 247.1 s | no | - | horch ledger run new alpha --steps code-sweep: status: exit status: 1 |
| `finish_finds_the_complexity_analyser_in_a_local_marketplace_of_the_store` | `tests/taskledger.rs` | 247.1 s | no | - | horch ledger run new alpha --steps complexity-sweep: status: exit status: 1 |
| `finish_finds_the_complexity_analyser_in_the_plugins_of_the_home` | `tests/taskledger.rs` | 247.1 s | no | - | horch ledger run new alpha --steps complexity-sweep: status: exit status: 1 |
| `finish_finds_the_complexity_analyser_in_the_skill_store` | `tests/taskledger.rs` | 247.1 s | no | - | horch ledger run new alpha --steps complexity-sweep: status: exit status: 1 |
| `ledger_watch_points_at_fleetd` | `tests/taskledger.rs` | 247.1 s | no | - | horch ledger run new a --check-every 10: status: exit status: 1 |
| `legacy_plumbing_lives_under_ledger_legacy` | `tests/taskledger.rs` | 247.1 s | no | - | assertion 'left == right' failed: <path> |
| `only_the_orchestrator_waives_a_critical_finding` | `tests/taskledger.rs` | 247.1 s | no | - | horch ledger run new alpha --steps code-sweep: status: exit status: 1 |
| `queue_claim_and_status_through_the_cli` | `tests/taskledger.rs` | 247.1 s | no | - | horch ledger run new alpha: status: exit status: 1 |
| `replacing_a_live_worker_names_the_stop_command` | `tests/taskledger.rs` | 247.1 s | no | - | horch ledger run new a: status: exit status: 1 |
| `requeue_refuses_a_task_that_is_neither_held_nor_cancelled` | `tests/taskledger.rs` | 247.1 s | no | - | horch ledger run new a: status: exit status: 1 |
| `run_new_refuses_an_old_run_branch_behind_the_main_branch` | `tests/taskledger.rs` | 247.1 s | no | - | horch ledger run new a: status: exit status: 1 |
| `spawn_run_names_the_run_the_worker_joins` | `tests/taskledger.rs` | 247.1 s | no | - | status: exit status: 1 |
| `status_on_a_fresh_ledger_creates_it_and_says_no_runs` | `tests/taskledger.rs` | 247.1 s | no | - | status: exit status: 1 |
| `the_import_finds_work_that_never_landed` | `tests/taskledger.rs` | 247.1 s | no | - | status: exit status: 1 |
| `the_no_watchdog_setting_turns_off_the_fleetd_check` | `tests/taskledger.rs` | 247.1 s | no | - | horch ledger run new zeta: status: exit status: 1 |
| `the_watchdog_ends_pre_claim_rows_whose_process_is_gone` | `tests/taskledger.rs` | 247.1 s | no | - | horch ledger run new a: status: exit status: 1 |
| `trf_43_a_spawn_with_task_text_refuses_with_the_ledger_task_command` | `tests/taskledger.rs` | 247.1 s | no | - | horch ledger run new a: status: exit status: 1 |
| `trf_43_assign_is_gone` | `tests/taskledger.rs` | 247.1 s | no | - | horch ledger run new eta: status: exit status: 1 |
| `two_fleetds_started_at_once_leave_1` | `tests/taskledger.rs` | 247.1 s | no | - | horch ledger run new zeta --check-every 1: status: exit status: 1 |
| `worker_done_reports_without_the_default_tell_grace` | `tests/tell_grace.rs` | 8.6 s | no | - | status: exit status: 1 |
| `a_busy_pi_worker_with_no_timer_is_kept_until_stall_after_then_stopped_at_rung_3` | `tests/watchdog_liveness.rs` | 9.9 s | no | - | called 'Result::unwrap()' on an 'Err' value: run pinned has no idea in flight of its feature F-0001, and a task needs an idea (FR-12). Add one: horch  |
| `a_detached_fleetd_does_not_inherit_the_starting_panes_workspace` | `tests/watchdog_liveness.rs` | 9.9 s | no | - | called 'Result::unwrap()' on an 'Err' value: run pinned has no idea in flight of its feature F-0001, and a task needs an idea (FR-12). Add one: horch  |
| `a_fleetd_started_from_another_workspace_wakes_the_runs_workspace` | `tests/watchdog_liveness.rs` | 9.9 s | no | - | called 'Result::unwrap()' on an 'Err' value: run pinned has no idea in flight of its feature F-0001, and a task needs an idea (FR-12). Add one: horch  |
| `a_run_made_outside_herdr_warns_and_the_first_pane_plan_records_its_workspace` | `tests/watchdog_liveness.rs` | 9.9 s | no | - | horch ledger run new is retired. Add an idea: horch idea add "<idea>" |
| `a_sigterm_to_fleetd_during_a_reap_leaves_the_reap_to_the_next_fleetd` | `tests/watchdog_liveness.rs` | 9.9 s | no | - | called 'Result::unwrap()' on an 'Err' value: run pinned has no idea in flight of its feature F-0001, and a task needs an idea (FR-12). Add one: horch  |
| `a_wake_from_another_workspace_goes_to_the_runs_workspace` | `tests/watchdog_liveness.rs` | 9.9 s | no | - | called 'Result::unwrap()' on an 'Err' value: run pinned has no idea in flight of its feature F-0001, and a task needs an idea (FR-12). Add one: horch  |
| `arc_31_e2e_a_ledger_worktree_is_not_unregistered` | `tests/worktree.rs` | 22.3 s | no | - | horch ledger run new is retired. Add an idea: horch idea add "<idea>" |
| `arc_31_e2e_done_refuses_then_unlanded_reports` | `tests/worktree.rs` | 22.3 s | no | - | called 'Result::unwrap()' on an 'Err' value: reading the operator token |
| `arc_31_e2e_list_and_done_survive_a_deleted_worktree_folder` | `tests/worktree.rs` | 22.3 s | no | - | called 'Result::unwrap()' on an 'Err' value: reading the operator token |

### horch (56 failing)

| Test | Binary | Binary wall time | In expected-failures | Expected-failures file:line | Failure summary |
|---|---|---|---|---|---|
| `ctx_01_cli_table_from_fixture_home` | `tests/context_cli.rs` | 0.3 s | no | - | horch ["note", "step 1 done"]: horch: reading the operator token: token file <path> is refused: stat failed: No such file or directory (os error 2) |
| `ctx_21_compact_at_sets_base` | `tests/context_cli.rs` | 0.3 s | no | - | horch ["note", "step 1 done"]: horch: reading the operator token: token file <path> is refused: stat failed: No such file or directory (os error 2) |
| `ctx_21_substituted_launch_takes_the_fallback_compact_at` | `tests/context_cli.rs` | 0.3 s | no | - | horch ["note", "step 1 done"]: horch: reading the operator token: token file <path> is refused: stat failed: No such file or directory (os error 2) |
| `check_none_flag_on_add_stores_no_check` | `tests/ledger_cli.rs` | 1.3 s | no | - | ["ledger", "run", "new", "r1"]: horch ledger run new is retired. Add an idea: horch idea add "<idea>" |
| `check_none_in_ticket_on_add_without_flag_stores_no_check` | `tests/ledger_cli.rs` | 1.3 s | no | - | ["ledger", "run", "new", "r1"]: horch ledger run new is retired. Add an idea: horch idea add "<idea>" |
| `ledger_status_shows_the_total_size_of_all_trees` | `tests/p3_worktrees_cli.rs` | 3.1 s | no | - | assertion 'left == right' failed: ["ledger", "run", "new", "r1"]: horch ledger run new is retired. Add an idea: horch idea add "<idea>" |
| `restore_of_a_task_makes_branch_rescue_t_seq_n_and_its_tree_with_each_rescued_file` | `tests/p3_worktrees_cli.rs` | 3.1 s | no | - | assertion 'left == right' failed: ["ledger", "run", "new", "r1"]: horch ledger run new is retired. Add an idea: horch idea add "<idea>" |
| `restore_of_a_task_whose_tree_is_removed_uses_its_rescue_ref` | `tests/p3_worktrees_cli.rs` | 3.1 s | no | - | assertion 'left == right' failed: ["ledger", "run", "new", "r1"]: horch ledger run new is retired. Add an idea: horch idea add "<idea>" |
| `restore_of_an_archive_name_keeps_today_s_behavior` | `tests/p3_worktrees_cli.rs` | 3.1 s | no | - | assertion 'left == right' failed: ["ledger", "run", "new", "r1"]: horch ledger run new is retired. Add an idea: horch idea add "<idea>" |
| `restore_refuses_when_the_branch_or_the_folder_exists_and_changes_nothing` | `tests/p3_worktrees_cli.rs` | 3.1 s | no | - | assertion 'left == right' failed: ["ledger", "run", "new", "r1"]: horch ledger run new is retired. Add an idea: horch idea add "<idea>" |
| `restore_runs_no_hook_of_the_project` | `tests/p3_worktrees_cli.rs` | 3.1 s | no | - | assertion 'left == right' failed: ["ledger", "run", "new", "r1"]: horch ledger run new is retired. Add an idea: horch idea add "<idea>" |
| `worktrees_json_has_each_field` | `tests/p3_worktrees_cli.rs` | 3.1 s | no | - | assertion 'left == right' failed: ["ledger", "run", "new", "r1"]: horch ledger run new is retired. Add an idea: horch idea add "<idea>" |
| `worktrees_lists_each_tree_with_state_task_owner_age_and_size` | `tests/p3_worktrees_cli.rs` | 3.1 s | no | - | assertion 'left == right' failed: ["ledger", "run", "new", "r1"]: horch ledger run new is retired. Add an idea: horch idea add "<idea>" |
| `worktrees_shows_the_rescue_job_that_the_reconciler_queued_for_an_orphan` | `tests/p3_worktrees_cli.rs` | 3.1 s | no | - | assertion 'left == right' failed: ["ledger", "run", "new", "r1"]: horch ledger run new is retired. Add an idea: horch idea add "<idea>" |
| `a_long_question_file_stays_under_64_kib` | `tests/p4_question_file.rs` | 1.6 s | no | - | assertion 'left == right' failed: horch ["ledger", "run", "new", "r1"]: horch ledger run new is retired. Add an idea: horch idea add "<idea>" |
| `question_of_a_claim_attempt_writes_no_file` | `tests/p4_question_file.rs` | 1.6 s | no | - | assertion 'left == right' failed: horch ["ledger", "run", "new", "r1"]: horch ledger run new is retired. Add an idea: horch idea add "<idea>" |
| `question_of_a_push_attempt_writes_1_question_file` | `tests/p4_question_file.rs` | 1.6 s | no | - | assertion 'left == right' failed: horch ["ledger", "run", "new", "r1"]: horch ledger run new is retired. Add an idea: horch idea add "<idea>" |
| `two_questions_write_2_files` | `tests/p4_question_file.rs` | 1.6 s | no | - | assertion 'left == right' failed: horch ["ledger", "run", "new", "r1"]: horch ledger run new is retired. Add an idea: horch idea add "<idea>" |
| `check_exits_2_with_neither_file_and_0_with_one` | `tests/p4_result_cli.rs` | 0.4 s | no | - | assertion 'left == right' failed: horch ["ledger", "run", "new", "r1"]: horch ledger run new is retired. Add an idea: horch idea add "<idea>" |
| `oversized_result_is_refused` | `tests/p4_result_cli.rs` | 0.4 s | no | - | assertion 'left == right' failed: horch ["ledger", "run", "new", "r1"]: horch ledger run new is retired. Add an idea: horch idea add "<idea>" |
| `result_json_with_another_status_is_refused` | `tests/p4_result_cli.rs` | 0.4 s | no | - | assertion 'left == right' failed: horch ["ledger", "run", "new", "r1"]: horch ledger run new is retired. Add an idea: horch idea add "<idea>" |
| `result_outside_a_push_attempt_is_refused` | `tests/p4_result_cli.rs` | 0.4 s | no | - | assertion 'left == right' failed: horch ["ledger", "run", "new", "r1"]: horch ledger run new is retired. Add an idea: horch idea add "<idea>" |
| `result_writes_a_valid_file_once` | `tests/p4_result_cli.rs` | 0.4 s | no | - | assertion 'left == right' failed: horch ["ledger", "run", "new", "r1"]: horch ledger run new is retired. Add an idea: horch idea add "<idea>" |
| `second_result_of_1_attempt_is_refused` | `tests/p4_result_cli.rs` | 0.4 s | no | - | assertion 'left == right' failed: horch ["ledger", "run", "new", "r1"]: horch ledger run new is retired. Add an idea: horch idea add "<idea>" |
| `triage_archive_is_refused_with_a_later_task` | `tests/p5_cli_writers.rs` | 3.0 s | no | - | assertion 'left == right' failed: W1 feat DELETE: queued in the triage_apply job 1; horch worktree triage shows the result |
| `add_map_sends_paths_and_sha256_not_texts` | `tests/p5_route_ledger_run.rs` | 1.1 s | no | - | assertion 'left == right' failed: ["ledger", "run", "new", "r1"] |
| `fleetd_down_queues_a_request_file` | `tests/p5_route_ledger_run.rs` | 1.1 s | no | - | assertion 'left == right' failed: ["ledger", "run", "new", "r1"] |
| `run_new_sends_1_run_new_request` | `tests/p5_route_ledger_run.rs` | 1.1 s | no | - | assertion 'left == right' failed: ["ledger", "run", "new", "r1", "--check", "true"] |
| `task_add_and_task_solo_send_their_kinds` | `tests/p5_route_ledger_run.rs` | 1.1 s | no | - | assertion 'left == right' failed: ["ledger", "run", "new", "r1"] |
| `requeue_cancel_and_waive_send_their_kinds` | `tests/p5_route_ledger_task.rs` | 1.3 s | no | - | assertion 'left == right' failed: ["ledger", "run", "new", "r1"] |
| `worker_that_removes_horch_role_is_refused_by_fleetd` | `tests/p5_route_ledger_task.rs` | 1.3 s | no | - | assertion 'left == right' failed: ["ledger", "run", "new", "r1"] |
| `worker_that_sets_horch_role_orchestrator_is_refused_by_fleetd` | `tests/p5_route_ledger_task.rs` | 1.3 s | no | - | assertion 'left == right' failed: ["ledger", "run", "new", "r1"] |
| `worktree_drop_sends_worktree_drop` | `tests/p5_route_ledger_task.rs` | 1.3 s | no | - | assertion 'left == right' failed: ["ledger", "run", "new", "r1"] |
| `each_operator_command_that_sets_horch_role_is_still_decided_by_fleetd` | `tests/p5_route_operator_commands.rs` | 1.1 s | no | - | called 'Result::unwrap()' on an 'Err' value: UNIQUE constraint failed: features.branch |
| `feature_branch_and_rescue_commands_send_their_kinds` | `tests/p5_route_operator_commands.rs` | 1.1 s | no | - | called 'Result::unwrap()' on an 'Err' value: UNIQUE constraint failed: features.branch |
| `job_requeue_sends_job_requeue` | `tests/p5_route_operator_commands.rs` | 1.1 s | no | - | called 'Result::unwrap()' on an 'Err' value: UNIQUE constraint failed: features.branch |
| `task_requeue_and_cancel_send_their_kinds` | `tests/p5_route_operator_commands.rs` | 1.1 s | no | - | called 'Result::unwrap()' on an 'Err' value: UNIQUE constraint failed: features.branch |
| `worktree_restore_and_triage_send_their_kinds` | `tests/p5_route_operator_commands.rs` | 1.1 s | no | - | called 'Result::unwrap()' on an 'Err' value: UNIQUE constraint failed: features.branch |
| `worktree_triage_archive_sends_1_request_and_queues_1_job` | `tests/p5_route_operator_commands.rs` | 1.1 s | no | - | called 'Result::unwrap()' on an 'Err' value: UNIQUE constraint failed: features.branch |
| `no_code_path_creates_a_task_without_an_idea` | `tests/p6_retired_run_commands.rs` | 0.9 s | no | - | a code path writes the idea of a run for its task: ["horch-e2e/src/ledger_seed.rs", "horch/src/cmd/worker.rs"] |
| `kind_catalog_is_the_table_of_the_design_and_the_kept_kinds` | `tests/p6_retired_worker_commands.rs` | 1.5 s | no | - | assertion 'left == right' failed: Kind::ALL |
| `the_legacy_resume_and_assign_arms_run` | `tests/r_cli_cmds_ledger.rs` | 0.0 s | no | - | ["add", "--record-id", "r1", "--agent", "claude", "--tier", "t", "--model", "m", "--role", "dev", "--task", "first"]: Output { status: ExitStatus(unix |
| `the_legacy_subcommands_write_read_and_report_a_session` | `tests/r_cli_cmds_ledger.rs` | 0.0 s | no | - | ["add", "--record-id", "r1", "--agent", "claude", "--tier", "t", "--model", "m", "--role", "dev", "--task", "do it"]: Output { status: ExitStatus(unix |
| `trf_11_dry_run_names_each_problem_and_writes_nothing` | `tests/trf_11_add_map_cli.rs` | 0.3 s | no | - | assertion 'left == right' failed: ["ledger", "run", "new", "r1"]: horch ledger run new is retired. Add an idea: horch idea add "<idea>" |
| `trf_13_add_map_refuses_a_ticket_already_queued_unless_again` | `tests/trf_11_add_map_cli.rs` | 0.3 s | no | - | assertion 'left == right' failed: ["ledger", "run", "new", "r1"]: horch ledger run new is retired. Add an idea: horch idea add "<idea>" |
| `trf_14_add_map_stores_kind_review_and_check_none` | `tests/trf_11_add_map_cli.rs` | 0.3 s | no | - | assertion 'left == right' failed: ["ledger", "run", "new", "r1"]: horch ledger run new is retired. Add an idea: horch idea add "<idea>" |
| `a_command_with_no_fleetd_row_starts_no_watchdog` | `tests/w3_ledger_cli_pins.rs` | 0.7 s | no | - | assertion 'left == right' failed: ["ledger", "run", "new", "r1", "--check-every", "10"]: horch ledger run new is retired. Add an idea: horch idea add  |
| `add_map_prints_each_task_and_the_path_of_the_map_it_read` | `tests/w3_ledger_cli_pins.rs` | 0.7 s | no | - | assertion 'left == right' failed: ["ledger", "run", "new", "r1"]: horch ledger run new is retired. Add an idea: horch idea add "<idea>" |
| `add_refuses_a_check_timeout_of_zero_after_it_queued_the_task` | `tests/w3_ledger_cli_pins.rs` | 0.7 s | no | - | assertion 'left == right' failed: ["ledger", "run", "new", "r1"]: horch ledger run new is retired. Add an idea: horch idea add "<idea>" |
| `add_takes_the_title_check_and_files_from_the_ticket_unless_a_flag_names_them` | `tests/w3_ledger_cli_pins.rs` | 0.7 s | no | - | assertion 'left == right' failed: ["ledger", "run", "new", "r1"]: horch ledger run new is retired. Add an idea: horch idea add "<idea>" |
| `ensure_starts_nothing_with_the_watchdog_off_or_without_a_ledger` | `tests/w3_ledger_cli_pins.rs` | 0.7 s | no | - | assertion 'left == right' failed: ["ledger", "run", "new", "r1", "--check-every", "10"]: horch ledger run new is retired. Add an idea: horch idea add  |
| `run_new_creates_the_branch_and_reports_it` | `tests/w3_ledger_cli_pins.rs` | 0.7 s | no | - | assertion 'left == right' failed: horch ledger run new is retired. Add an idea: horch idea add "<idea>" |
| `the_board_commands_print_their_pinned_text` | `tests/w3_ledger_cli_pins.rs` | 0.7 s | no | - | assertion 'left == right' failed: ["ledger", "run", "new", "r1", "--steps", "check,land"]: horch ledger run new is retired. Add an idea: horch idea ad |
| `the_unfinished_import_and_refusal_arms_print_their_pinned_text` | `tests/w3_ledger_cli_pins.rs` | 0.7 s | no | - | assertion 'left == right' failed: ["ledger", "run", "new", "r1"]: horch ledger run new is retired. Add an idea: horch idea add "<idea>" |
| `watch_once_json_lists_each_checked_worker` | `tests/w3_ledger_cli_pins.rs` | 0.7 s | no | - | assertion 'left == right' failed: ["ledger", "run", "new", "r1", "--check-every", "10", "--stall-rounds", "1000"]: horch ledger run new is retired. Ad |
| `watch_once_reports_in_text_and_json` | `tests/w3_ledger_cli_pins.rs` | 0.7 s | no | - | assertion 'left == right' failed: ["ledger", "run", "new", "r1", "--check-every", "10"]: horch ledger run new is retired. Add an idea: horch idea add  |

### horch-core (41 failing)

| Test | Binary | Binary wall time | In expected-failures | Expected-failures file:line | Failure summary |
|---|---|---|---|---|---|
| `arc_09_argv_matches_baseline` | `tests/baseline_oracles.rs` | 0.9 s | no | - | assertion 'left == right' failed: architect-reviewer: adapter argv differs from the A0 oracle |
| `oracle_launch_matches` | `tests/baseline_oracles.rs` | 0.9 s | no | - | launch/architect-reviewer.json differs from the A0 oracle at line 48: |
| `check_job_has_no_forbidden_env` | `tests/check_sandbox.rs` | 0.9 s | no | - | called 'Option::unwrap()' on a 'None' value |
| `check_runs_in_the_worker_sandbox_not_with_fleetd_rights` | `tests/check_sandbox.rs` | 0.9 s | no | - | called 'Option::unwrap()' on a 'None' value |
| `claude_worker_check_runs_in_the_platform_sandbox` | `tests/check_sandbox.rs` | 0.9 s | no | - | called 'Option::unwrap()' on a 'None' value |
| `codex_worker_check_keeps_codex_sandbox` | `tests/check_sandbox.rs` | 0.9 s | no | - | called 'Option::unwrap()' on a 'None' value |
| `ctx_05_prime_launch_records_the_applied_window` | `tests/context_prime.rs` | 3.6 s | no | - | called 'Result::unwrap()' on an 'Err' value: reading the operator token |
| `worktree_triage_queues_its_jobs` | `tests/fleetd_handlers_custody.rs` | 1.0 s | no | - | applied: {"applied":[],"failed":0,"job":2,"queued":["W1 _scratch/t1 ARCHIVE: queued in the triage_apply job 2; horch worktree triage shows the result" |
| `pass_adopts_a_hand_made_worktree_within_1_list_period` | `tests/fleetd_pass_p3.rs` | 32.4 s | no | - | job 1 ended: JobRow { id: 1, kind: WorktreeList, subject: "<path> repo: Some("<path> input_key: "0", run: 1, abandoned_runs: 0, not_before: None, stat |
| `worktree_list_job_is_queued_once_per_period_per_repository` | `tests/fleetd_pass_p3.rs` | 32.4 s | no | - | job 1 ended: JobRow { id: 1, kind: WorktreeList, subject: "<path> repo: Some("<path> input_key: "0", run: 1, abandoned_runs: 0, not_before: None, stat |
| `worktree_list_job_writes_no_index_of_a_tree` | `tests/fleetd_pass_p3.rs` | 32.4 s | no | - | job 1 ended: JobRow { id: 1, kind: WorktreeList, subject: "<path> repo: Some("<path> input_key: "0", run: 1, abandoned_runs: 0, not_before: None, stat |
| `only_the_bind_and_its_dispatch_points_choose_a_task` | `tests/fr5_push_only.rs` | 0.3 s | no | - | FR-5: only the bind module, fleetd and horch spawn choose a task: |
| `harness_codex_rules_snapshot` | `tests/harness.rs` | 0.1 s | no | - | assertion 'left == right' failed: codex_rules_orchestrator.rules differs from the pre-A4 snapshot |
| `t27_every_intake_teammate_reads_but_does_not_write_its_bundle` | `tests/intake_roster.rs` | 3.0 s | no | - | assertion 'left == right' failed: intake-auditor |
| `t28_a_full_sandboxed_teammate_writes_no_skills_bundle` | `tests/intake_roster.rs` | 3.0 s | no | - | skill-importer |
| `an_old_registry_others_can_write_is_not_copied_and_inbox_says_why` | `tests/messaging.rs` | 11.3 s | no | - | status: exit status: 1 |
| `done_outside_a_git_work_tree_reports_no_scan_error` | `tests/messaging.rs` | 11.3 s | no | - | status: exit status: 1 |
| `g5_done_without_an_orchestrator` | `tests/messaging.rs` | 11.3 s | no | - | status: exit status: 1 |
| `g5_resume_after_a_closed_pane` | `tests/messaging.rs` | 11.3 s | no | - | status: exit status: 1 |
| `mail_u1_done_push_failure_records_queued_and_closes` | `tests/messaging.rs` | 11.3 s | no | - | status: exit status: 1 |
| `mail_u1_done_stays_open_when_enqueue_fails` | `tests/messaging.rs` | 11.3 s | no | - | status: exit status: 1 |
| `p09_registry_in_the_old_temp_location_still_resolves` | `tests/messaging.rs` | 11.3 s | no | - | status: exit status: 1 |
| `p09_registry_lives_in_the_state_folder` | `tests/messaging.rs` | 11.3 s | no | - | status: exit status: 1 |
| `t11_followup_done_warns_when_the_registration_stays` | `tests/messaging.rs` | 11.3 s | no | - | status: exit status: 1 |
| `t11_followup_orchestrator_registers_under_a_custom_state_dir` | `tests/messaging.rs` | 11.3 s | no | - | horch fleet ran a pane-launch command |
| `w2_agent_exit_before_done_tells_the_orchestrator` | `tests/messaging.rs` | 11.3 s | no | - | status: exit status: 1 |
| `builtin_roster_has_no_refused_worker_profile_on_macos` | `tests/p5_profile_validation.rs` | 0.6 s | no | - | refused worker profiles: |
| `hooks_fire_fails_when_no_stop_reaches_the_wrapper` | `tests/p7_conformance.rs` | 61.2 s | no | - | assertion 'left == right' failed: CheckResult { check: HooksFire, verdict: Pass, evidence: "tool start, tool end and stop reached the wrapper" } |
| `shared_write_fails_when_the_agent_ends_without_the_write` | `tests/p7_conformance.rs` | 61.2 s | no | - | assertion 'left == right' failed: CheckResult { check: SharedWrite, verdict: Pass, evidence: "outbox/result.json is in the task folder" } |
| `interactive_attempt_is_unchanged` | `tests/p7_wrapper_exec.rs` | 2.2 s | no | - | assertion 'left == right' failed: claude |
| `both_gates_run_again_after_the_rebase` | `tests/stack_edges.rs` | 4.9 s | no | - | called 'Result::unwrap()' on an 'Err' value: run F-0002 has no idea in flight of its feature F-0002, and a task needs an idea (FR-12). Add one: horch  |
| `trf_02_template_example_map_parses_with_the_ticket` | `tests/trf_02_ticket_template.rs` | 0.0 s | no | - | the template has no '== Example MAP.md ==' line |
| `conflict_is_recorded_with_its_files` | `tests/trial_merges.rs` | 4.6 s | no | - | assertion 'left == right' failed: a conflict is a failed run |
| `trial_merge_job_does_not_check_out` | `tests/trial_merges.rs` | 4.6 s | no | - | assertion 'left == right' failed |
| `trial_merge_of_2_tips_runs_once` | `tests/trial_merges.rs` | 4.6 s | no | - | assertion 'left == right' failed |
| `worktree_new_refuses_a_shared_task_and_returns_an_existing_worktree` | `tests/w3_finish_steps.rs` | 11.4 s | no | - | called 'Result::unwrap()' on an 'Err' value: T-2 has the attempt 2bf2bd23-8694-45fd-83b1-172c7b64f6ef/A1 that is not ended: 1 task has 1 live attempt  |
| `an_empty_run_renders_no_tasks_and_is_complete` | `tests/w3_taskledger_core_status.rs` | 1.0 s | no | - | assertion 'left == right' failed |
| `render_prints_each_section` | `tests/w3_taskledger_core_status.rs` | 1.0 s | no | - | assertion 'left == right' failed |
| `a_tree_whose_removal_failed_frees_no_space` | `tests/worktree_disk.rs` | 2.1 s | no | - | called 'Result::unwrap()' on an 'Err' value: run r1 has no idea in flight of its feature F-0001, and a task needs an idea (FR-12). Add one: horch idea |
| `over_budget_never_removes_an_active_tree_and_holds_dispatch` | `tests/worktree_disk.rs` | 2.1 s | no | - | called 'Result::unwrap()' on an 'Err' value: run r1 has no idea in flight of its feature F-0001, and a task needs an idea (FR-12). Add one: horch idea |
| `over_budget_removes_landed_and_rescued_trees_of_finished_runs_oldest_first` | `tests/worktree_disk.rs` | 2.1 s | no | - | called 'Result::unwrap()' on an 'Err' value: run r1 has no idea in flight of its feature F-0001, and a task needs an idea (FR-12). Add one: horch idea |

### horch (unit) (7 failing)

| Test | Binary | Binary wall time | In expected-failures | Expected-failures file:line | Failure summary |
|---|---|---|---|---|---|
| `cmd::context::tests::ctx_06_windows_view_lists_claude_and_codex_teammates` | `unittests src/main.rs` | 14.7 s | no | - | codex-sol                    codex     gpt-5.6-sol                           256,000  fleet     256,000     160,000  teammate    160,000    96,000  te |
| `cmd::messaging::tests::trf_45_note_and_ledger_progress_are_1_command_and_write_no_json` | `unittests src/main.rs` | 14.7 s | no | - | called 'Result::unwrap()' on an 'Err' value: refused: not handled in this version |
| `cmd::recipes::tests::trf_43_the_steward_task_is_queued_and_its_spawn_has_no_task_text` | `unittests src/main.rs` | 14.7 s | no | - | called 'Result::unwrap()' on an 'Err' value: horch ledger task is retired. Add a ticket that you wrote: horch idea add --direct <ticket file> [--to <f |
| `cmd::rescuecmd::tests::drop_deletes_the_ref_and_verifies_it` | `unittests src/main.rs` | 14.7 s | no | - | called 'Result::unwrap()' on an 'Err' value: reading the operator token |
| `cmd::rescuecmd::tests::worker_caller_is_refused` | `unittests src/main.rs` | 14.7 s | no | - | called 'Result::unwrap()' on an 'Err' value: reading the operator token |
| `cmd::sessionscmd::tests::trf_44_sessions_shows_a_ledger_worker_with_its_claimed_task` | `unittests src/main.rs` | 14.7 s | no | - | called 'Result::unwrap()' on an 'Err' value: run r has no idea in flight of its feature F-0001, and a task needs an idea (FR-12). Add one: horch idea  |
| `cmd::taskledger::tests::no_allowed_ledger_subcommand_takes_a_check_command` | `unittests src/main.rs` | 14.7 s | no | - | ["reopen"] |

### horch-core (unit) (4 failing)

| Test | Binary | Binary wall time | In expected-failures | Expected-failures file:line | Failure summary |
|---|---|---|---|---|---|
| `harness::codex::tests::each_launch_gets_only_its_own_rules` | `unittests src/lib.rs` | 170.2 s | no | - | assertion failed: read(&orchestrator).contains(r#"pattern = ["horch", "spawn"]"#) |
| `taskledger::schema::v7_tests::trf_48_every_old_version_migrates_to_current` | `unittests src/lib.rs` | 170.2 s | no | - | assertion 'left == right' failed: version 14 |
| `taskledger::stop::tests::trf_36_a_cancel_stops_the_worker_by_its_pids_and_queues_nothing` | `unittests src/lib.rs` | 170.2 s | no | - | called 'Result::unwrap()' on an 'Err' value: run s has no idea in flight of its feature F-0001, and a task needs an idea (FR-12). Add one: horch idea  |
| `taskledger::stop::tests::trf_36_a_worker_that_cannot_be_stopped_leaves_the_cancel_standing` | `unittests src/lib.rs` | 170.2 s | no | - | called 'Result::unwrap()' on an 'Err' value: run s has no idea in flight of its feature F-0001, and a task needs an idea (FR-12). Add one: horch idea  |

### horch-platform (4 failing)

| Test | Binary | Binary wall time | In expected-failures | Expected-failures file:line | Failure summary |
|---|---|---|---|---|---|
| `arc_30_e2e_a_live_relay_that_fails_leaves_the_tell_for_the_courier` | `tests/channel.rs` | 6.0 s | no | - | called 'Result::unwrap()' on an 'Err' value: reading the operator token |
| `arc_30_e2e_codex_relay_runs_codex_queue` | `tests/channel.rs` | 6.0 s | no | - | called 'Result::unwrap()' on an 'Err' value: reading the operator token |
| `w2_e2e_done_reports_through_a_relay_without_a_registered_pane` | `tests/channel.rs` | 6.0 s | no | - | called 'Result::unwrap()' on an 'Err' value: reading the operator token |
| `w2_e2e_refused_tell_without_a_pane_does_not_claim_no_orchestrator` | `tests/channel.rs` | 6.0 s | no | - | called 'Result::unwrap()' on an 'Err' value: reading the operator token |

## Notes

- The host was loaded (load average 16 to 26) by other workers. The timeouts of 120 s (7 tests) may come from this load. A rerun on a quiet host is needed to say so. This unit did not rerun any test.
- No process of this unit is alive after the run. The sandbox folder `.dev-sandbox` is removed by the script.
- Each unexpected failure is a candidate ticket. This unit fixes none.
