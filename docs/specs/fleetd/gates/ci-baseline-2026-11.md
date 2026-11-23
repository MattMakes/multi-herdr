# CI baseline of fleetd, 2026-11-20

- Run: 44680906908, https://github.com/MattMakes/multi-herdr/actions/runs/44680906908
- Trigger: `gh workflow run ci.yml --ref fleet/fleetd-finish` (`workflow_dispatch`).
- Head commit: `085427fba539a050c92c489c07e3d43615342f55`, the tip of `origin/fleet/fleetd-finish` (checked with `git ls-remote`).
- Toolchain: `stable` = rustc 1.100.0 (0ef1bc386 2026-11-09). `rust-version` of the workspace is 1.89.
- Run result: failure. 1 of 6 jobs passed.

| Job | OS | Result | Time |
|---|---|---|---|
| `test (macos-15)` | macOS | failure | 18 min 41 s |
| `test (ubuntu-latest)` | Linux | failure | 11 min 55 s |
| `docs` | Linux | failure | 54 s |
| `release-lint` | Linux | failure | 1 min 42 s |
| `build-windows` | Windows | success | 2 min 58 s |
| `test-windows` | Windows | failure (the job has `continue-on-error: true`) | 2 min 3 s |

## Classes

- (a) a test that is on `crates/horch-e2e/expected-failures/`
- (b) a compile error on that OS
- (c) a test that is unix-only by its code but runs on Windows
- (d) a real fault of the product (or of the test harness) on that OS
- (e) a fault of the CI setup
- (f) not one of the classes of the ticket: the test fails with the same message on macOS and Linux. The test code is stale. It is not OS-specific. `f?` means the cause is not read yet.

## How to read the tables

- A run of 4400 tests gives about 700 failures. The tables group failures by cause. The group of the e2e build has 1 row for about 560 tests, because each test fails with the same first error line.
- The counts of passed and failed tests are the sums of the lines `test result:` of the job log.
- The column `file:line` is the place of the panic or the error. It is not always the place of the cause.
- Nothing is run locally. Each cause below comes from the CI log. Class `f` and `f?` rows are not checked on a local macOS build.

## macOS

Job `test (macos-15)`: failure. 4411 tests passed, 666 tests failed.

### Groups

| Test or step | Class | First error line | file:line |
|---|---|---|---|
| About 528 tests of `horch-e2e` and of the `horch` crate that start the e2e harness (all tests of the suites `p0_*`, `p1_*` … `p7_*`, `fleet18_*`, `mail`, `taskledger`, `telemetry_start_handshake`, `watchdog_liveness`, `worktree`, …) | d | `the build of horch and the fakes failed: cargo build --locked -p horch -p horch-e2e --bins --profile out …` then `error: profile `out` is not defined` | `crates/horch-e2e/src/harness.rs:1012-1016` (the profile comes from the name of the folder of the test binary) |
| 38 tests of `horch-e2e` that build only the fakes (same cause) | d | `the build of the fakes failed: … --profile out` | `crates/horch-e2e/src/harness.rs:963` |

Cause of the first 2 rows: rustc 1.100 (cargo) puts the test binaries in `target/debug/build/<package>/<hash>/out/`. `build_command` takes the last folder name of the path of the binary as the cargo profile. The name is `out`, so cargo is called with `--profile out`. The same cause hits Linux. A run with the old layout (`target/debug/deps/`) is not affected, so a local run on an older cargo is green on this point.

### Failures that are not caused by the build

107 tests. Rows with the OS `macOS` fail on macOS only. Rows with `Linux + macOS` fail on both.

| Test | OS | Class | First error line | file:line |
|---|---|---|---|---|
| `a_command_with_no_fleetd_row_starts_no_watchdog` | Linux + macOS | f | assert_eq failed: ["ledger", "run", "new", "r1", "--check-every", "10"]: horch ledger run new is retired. Add an idea: horch idea  | `horch/tests/w3_ledger_cli_pins.rs:138` |
| `a_long_question_file_stays_under_64_kib` | Linux + macOS | f | assert_eq failed: horch ["ledger", "run", "new", "r1"]: horch ledger run new is retired. Add an idea: horch idea add "<idea>" | `horch/tests/p4_question_file.rs:186` |
| `a_tree_whose_removal_failed_frees_no_space` | Linux + macOS | f | Err: run r1 has no idea in flight of its feature F-0001, and a task needs an idea (FR-12). Add one: horch idea add | `horch-core/tests/worktree_disk.rs:191` |
| `a_worker_with_no_progress_is_stalled_by_the_ladder_and_stopped_by_its_` | Linux + macOS | a | `a_worker_with_no_progress_is_stalled_by_the_ladder_and_stopped_by_its_pid` is on the list of expected failures with the reason `i | `horch-e2e/src/expected.rs:108` |
| `adapter_hash_changes_when_the_rendered_launch_changes` | Linux | e | Err: conformance-claude: refusing to launch without a sandbox: bwrap and socat not on PATH; install bubblewrap and socat | `horch-core/tests/p7_conformance_job.rs:346` |
| `add_map_prints_each_task_and_the_path_of_the_map_it_read` | Linux + macOS | f | assert_eq failed: ["ledger", "run", "new", "r1"]: horch ledger run new is retired. Add an idea: horch idea add "<idea>" | `horch/tests/w3_ledger_cli_pins.rs:138` |
| `add_map_sends_paths_and_sha256_not_texts` | Linux + macOS | f? | assert_eq failed: ["ledger", "run", "new", "r1"] | `horch/tests/p5_route_ledger_run.rs:140` |
| `add_refuses_a_check_timeout_of_zero_after_it_queued_the_task` | Linux + macOS | f | assert_eq failed: ["ledger", "run", "new", "r1"]: horch ledger run new is retired. Add an idea: horch idea add "<idea>" | `horch/tests/w3_ledger_cli_pins.rs:138` |
| `add_takes_the_title_check_and_files_from_the_ticket_unless_a_flag_name` | Linux + macOS | f | assert_eq failed: ["ledger", "run", "new", "r1"]: horch ledger run new is retired. Add an idea: horch idea add "<idea>" | `horch/tests/w3_ledger_cli_pins.rs:138` |
| `an_empty_run_renders_no_tasks_and_is_complete` | Linux + macOS | f? | assert_eq failed | `horch-core/tests/w3_taskledger_core_status.rs:163` |
| `arc_09_argv_matches_baseline` | Linux | e | assert_eq failed: architect-reviewer: adapter argv differs from the A0 oracle | `horch-core/tests/baseline_oracles.rs:498` |
| `arc_31_e2e_a_ledger_worktree_is_not_unregistered` | Linux | f? | error: test failed, to rerun pass `-p horch-e2e --test worktree` | `horch-e2e/src/harness.rs:963` |
| `both_gates_run_again_after_the_rebase` | Linux + macOS | f | Err: run F-0002 has no idea in flight of its feature F-0002, and a task needs an idea (FR-12). Add one: horch idea add | `horch-core/tests/stack_edges.rs:565` |
| `builtin_roster_has_no_refused_worker_profile_on_macos` | Linux + macOS | f? | refused worker profiles: | `horch-core/tests/p5_profile_validation.rs:356` |
| `check_exits_2_with_neither_file_and_0_with_one` | Linux + macOS | f | assert_eq failed: horch ["ledger", "run", "new", "r1"]: horch ledger run new is retired. Add an idea: horch idea add "<idea>" | `horch/tests/p4_result_cli.rs:194` |
| `check_job_has_no_forbidden_env` | Linux + macOS | f? | called `Option::unwrap()` on a `None` value | `horch-core/tests/check_sandbox.rs:254` |
| `check_none_flag_on_add_stores_no_check` | Linux + macOS | f | ["ledger", "run", "new", "r1"]: horch ledger run new is retired. Add an idea: horch idea add "<idea>" | `horch/tests/ledger_cli.rs:63` |
| `check_none_in_ticket_on_add_without_flag_stores_no_check` | Linux + macOS | f | ["ledger", "run", "new", "r1"]: horch ledger run new is retired. Add an idea: horch idea add "<idea>" | `horch/tests/ledger_cli.rs:63` |
| `check_runs_in_the_worker_sandbox_not_with_fleetd_rights` | Linux + macOS | f? | called `Option::unwrap()` on a `None` value | `horch-core/tests/check_sandbox.rs:254` |
| `claude_worker_check_runs_in_the_platform_sandbox` | Linux + macOS | f? | called `Option::unwrap()` on a `None` value | `horch-core/tests/check_sandbox.rs:254` |
| `cmd::context::tests::ctx_06_windows_view_lists_claude_and_codex_teamma` | Linux + macOS | f? | codex-sol                    codex     gpt-5.6-sol                           256,000  fleet     256,000     160,000  teammate    1 | `horch/src/cmd/context.rs:1267` |
| `cmd::messaging::tests::trf_45_note_and_ledger_progress_are_1_command_a` | Linux + macOS | f? | Err: refused: not handled in this version | `horch/src/cmd/messaging.rs:1532` |
| `cmd::recipes::tests::trf_43_the_steward_task_is_queued_and_its_spawn_h` | Linux + macOS | f | Err: horch ledger task is retired. Add a ticket that you wrote: horch idea add --direct <ticket file> [--to <feature>] | `horch/src/cmd/recipes.rs:1152` |
| `cmd::rescuecmd::tests::drop_deletes_the_ref_and_verifies_it` | Linux + macOS | f | Err: reading the operator token | `horch/src/cmd/rescuecmd.rs:290` |
| `cmd::rescuecmd::tests::worker_caller_is_refused` | Linux + macOS | f | Err: reading the operator token | `horch/src/cmd/rescuecmd.rs:329` |
| `cmd::sessionscmd::tests::trf_44_sessions_shows_a_ledger_worker_with_it` | Linux + macOS | f | Err: run r has no idea in flight of its feature F-0001, and a task needs an idea (FR-12). Add one: horch idea add | `horch/src/cmd/sessionscmd.rs:438` |
| `cmd::taskledger::tests::no_allowed_ledger_subcommand_takes_a_check_com` | Linux + macOS | f? | ["reopen"] | `horch/src/cmd/taskledger.rs:2151` |
| `codex_worker_check_keeps_codex_sandbox` | Linux + macOS | f? | called `Option::unwrap()` on a `None` value | `horch-core/tests/check_sandbox.rs:254` |
| `conflict_is_recorded_with_its_files` | Linux + macOS | f? | assert_eq failed: a conflict is a failed run | `horch-core/tests/trial_merges.rs:367` |
| `ctx_01_cli_table_from_fixture_home` | Linux + macOS | f | horch ["note", "step 1 done"]: horch: reading the operator token: token file <path> is refused: stat failed: No such file or direc | `horch/tests/context_cli.rs:129` |
| `ctx_05_claude_overlay_gets_fleet_window_only_without_operator_value` | Linux | e | Err: opus: refusing to launch without a sandbox: bwrap and socat not on PATH; install bubblewrap and socat | `horch-core/tests/context_launch.rs:157` |
| `ctx_05_prime_launch_records_the_applied_window` | Linux + macOS | f | Err: reading the operator token | `horch-core/tests/context_prime.rs:503` |
| `ctx_05_recorded_applied_matches_the_command` | Linux | e | Err: opus: refusing to launch without a sandbox: bwrap and socat not on PATH; install bubblewrap and socat | `horch-core/tests/context_launch.rs:157` |
| `ctx_21_compact_at_sets_base` | Linux + macOS | f | horch ["note", "step 1 done"]: horch: reading the operator token: token file <path> is refused: stat failed: No such file or direc | `horch/tests/context_cli.rs:129` |
| `ctx_21_substituted_launch_takes_the_fallback_compact_at` | Linux + macOS | f | horch ["note", "step 1 done"]: horch: reading the operator token: token file <path> is refused: stat failed: No such file or direc | `horch/tests/context_cli.rs:129` |
| `each_operator_command_that_sets_horch_role_is_still_decided_by_fleetd` | Linux + macOS | f | Err: UNIQUE constraint failed: features.branch | `horch/tests/p5_route_operator_commands.rs:119` |
| `ensure_starts_nothing_with_the_watchdog_off_or_without_a_ledger` | Linux + macOS | f | assert_eq failed: ["ledger", "run", "new", "r1", "--check-every", "10"]: horch ledger run new is retired. Add an idea: horch idea  | `horch/tests/w3_ledger_cli_pins.rs:138` |
| `execution::lifecycle::tests::arc_06_transport_env_applied_to_child` | Linux | e | Err: sonnet: refusing to launch without a sandbox: socat not on PATH; install bubblewrap and socat | `horch-core/src/execution/lifecycle.rs:2106` |
| `feature_branch_and_rescue_commands_send_their_kinds` | Linux + macOS | f | Err: UNIQUE constraint failed: features.branch | `horch/tests/p5_route_operator_commands.rs:119` |
| `fleetd_down_queues_a_request_file` | Linux + macOS | f? | assert_eq failed: ["ledger", "run", "new", "r1"] | `horch/tests/p5_route_ledger_run.rs:140` |
| `fleetd_review_spawn_does_not_use_the_spawn_kind` | macOS | f? | the spawn wrote the task files of its attempt | `horch/tests/p6_retired_worker_commands.rs:594` |
| `fr5_worker_never_runs_a_task_search` | Linux | f? | error: test failed, to rerun pass `-p horch-e2e --test p4_safety` | `horch-e2e/src/harness.rs:963` |
| `g50_live_foreign_runs_and_legacy_orchestrators_warn_but_do_not_block` | Linux + macOS | a | `g50_live_foreign_runs_and_legacy_orchestrators_warn_but_do_not_block` is on the list of expected failures with the reason `horch  | `horch-e2e/src/expected.rs:108` |
| `g51_sessions_points_to_ledger_status_only_for_the_callers_active_run` | Linux + macOS | a | `g51_sessions_points_to_ledger_status_only_for_the_callers_active_run` is on the list of expected failures with the reason `horch  | `horch-e2e/src/expected.rs:108` |
| `g52_resume_uses_own_session` | Linux + macOS | a | `g52_resume_uses_own_session` is on the list of expected failures with the reason `horch spawn refused: no ready task for persona  | `horch-e2e/src/expected.rs:108` |
| `g53_ledger_handoff_compacts_resumes_and_records_progress_once` | Linux + macOS | a | `g53_ledger_handoff_compacts_resumes_and_records_progress_once` is on the list of expected failures with the reason `horch ledger  | `horch-e2e/src/expected.rs:108` |
| `g64_skill_states_each_delegation_and_reporting_rule_within_budget` | Linux + macOS | f? | missing verified (with evidence) or not verified | `horch-e2e/tests/fleet18_brief_worktree.rs:391` |
| `g70_question_wait_clears_on_orchestrator_answer_and_stalls_restart` | Linux + macOS | f | Err: run waits has no idea in flight of its feature F-0001, and a task needs an idea (FR-12). Add one: horch idea add | `horch-e2e/tests/fleet18_messaging.rs:311` |
| `g70_reviewer_wait_holds_sixty_minutes_then_three_idle_checks_reap` | Linux + macOS | f | Err: run waits has no idea in flight of its feature F-0001, and a task needs an idea (FR-12). Add one: horch idea add | `horch-e2e/tests/fleet18_messaging.rs:311` |
| `g70_waiting_background_deadline_and_dead_process` | Linux + macOS | f | Err: run waits has no idea in flight of its feature F-0001, and a task needs an idea (FR-12). Add one: horch idea add | `horch-e2e/tests/fleet18_messaging.rs:311` |
| `gate_at_its_limit_is_abandoned_and_its_group_killed` | Linux | f? | the gate group was killed | `horch-core/tests/gate_jobs.rs:958` |
| `harness::claude::tests::worker_settings_deny_git_worktree_add_remove_a` | Linux | e | Err: opus: refusing to launch without a sandbox: bwrap and socat not on PATH; install bubblewrap and socat | `horch-core/src/harness/claude.rs:2894` |
| `harness::codex::tests::each_launch_gets_only_its_own_rules` | Linux + macOS | f? | assertion failed: read(&orchestrator).contains(r#"pattern = ["horch", "spawn"]"#) | `horch-core/src/harness/codex.rs:2761` |
| `harness::launch::tests::a_haiku_override_launches_without_effort` | Linux | e | Err: opus: refusing to launch without a sandbox: bwrap and socat not on PATH; install bubblewrap and socat | `horch-core/src/harness/launch.rs:2275` |
| `harness::launch::tests::an_agent_launch_drops_the_git_repository_varia` | Linux | f? |  | `horch-core/src/harness/launch.rs:3134` |
| `harness::launch::tests::arc_29_launch_trusts_the_workdir_before_the_ag` | Linux | e | Err: opus: refusing to launch without a sandbox: bwrap and socat not on PATH; install bubblewrap and socat | `horch-core/src/harness/launch.rs:3316` |
| `harness::launch::tests::claude_exec_launch_exposes_its_skills_as_a_plu` | Linux | e | Err: exec-claude: refusing to launch without a sandbox: bwrap and socat not on PATH; install bubblewrap and socat | `horch-core/src/harness/launch.rs:3380` |
| `harness::launch::tests::claude_fresh_carries_session_and_mode` | Linux | e | Err: opus: refusing to launch without a sandbox: bwrap and socat not on PATH; install bubblewrap and socat | `horch-core/src/harness/launch.rs:1777` |
| `harness::launch::tests::claude_resume_swaps_the_session_flag` | Linux | e | Err: sonnet: refusing to launch without a sandbox: bwrap and socat not on PATH; install bubblewrap and socat | `horch-core/src/harness/launch.rs:1816` |
| `harness::launch::tests::declared_mcp_servers_are_strict_and_complete` | Linux | e | Err: frontend-developer: refusing to launch without a sandbox: bwrap and socat not on PATH; install bubblewrap and socat | `horch-core/src/harness/launch.rs:2811` |
| `harness::launch::tests::disabled_skills_become_skill_overrides` | Linux | e | Err: sonnet: refusing to launch without a sandbox: bwrap and socat not on PATH; install bubblewrap and socat | `horch-core/src/harness/launch.rs:3014` |
| `harness::launch::tests::git_env_agent_launch_child` | Linux | e | Err: opus: refusing to launch without a sandbox: bwrap and socat not on PATH; install bubblewrap and socat | `horch-core/src/harness/launch.rs:3173` |
| `harness::launch::tests::operator_only_teammate_with_every_skill_missin` | Linux | e | Err: opus: refusing to launch without a sandbox: bwrap and socat not on PATH; install bubblewrap and socat | `horch-core/src/harness/launch.rs:3228` |
| `harness::launch::tests::opting_in_to_claudeai_skills_leaves_the_switch` | Linux | e | Err: sonnet: refusing to launch without a sandbox: bwrap and socat not on PATH; install bubblewrap and socat | `horch-core/src/harness/launch.rs:2969` |
| `harness::launch::tests::path_of_blender_fills_blender_path_or_leaves_i` | Linux | e | Err: blender-artist: refusing to launch without a sandbox: bwrap and socat not on PATH; install bubblewrap and socat | `horch-core/src/harness/launch.rs:1612` |
| `harness::launch::tests::phase_skills_are_native_on_all_harnesses_and_r` | Linux | e | Err: sonnet: refusing to launch without a sandbox: bwrap and socat not on PATH; install bubblewrap and socat | `horch-core/src/harness/launch.rs:1657` |
| `harness::launch::tests::plugin_paths_are_expanded_before_launch` | Linux | e | Err: staff-engineer: refusing to launch without a sandbox: bwrap and socat not on PATH; install bubblewrap and socat | `horch-core/src/harness/launch.rs:2874` |
| `harness::launch::tests::plugin_skills_load_a_filtered_copy_of_the_plug` | Linux | e | Err: opus: refusing to launch without a sandbox: bwrap and socat not on PATH; install bubblewrap and socat | `horch-core/src/harness/launch.rs:2141` |
| `harness::launch::tests::restricted_settings_keep_the_operator_status_l` | Linux | e | Err: sonnet: refusing to launch without a sandbox: bwrap and socat not on PATH; install bubblewrap and socat | `horch-core/src/harness/launch.rs:3073` |
| `harness::launch::tests::unset_isolation_fields_add_no_flags` | Linux | e | Err: sonnet: refusing to launch without a sandbox: bwrap and socat not on PATH; install bubblewrap and socat | `horch-core/src/harness/launch.rs:2892` |
| `harness::launch::tests::variadic_flags_are_fenced_off_from_the_prompt_` | Linux | e | Err: frontend-developer: refusing to launch without a sandbox: bwrap and socat not on PATH; install bubblewrap and socat | `horch-core/src/harness/launch.rs:2841` |
| `harness_codex_rules_snapshot` | Linux + macOS | f? | assert_eq failed: codex_rules_orchestrator.rules differs from the pre-A4 snapshot | `horch-core/tests/harness.rs:25` |
| `hdf_04_migrated_launch_env_and_args_match_w2` | Linux | e | Err: opus: refusing to launch without a sandbox: bwrap and socat not on PATH; install bubblewrap and socat | `horch-core/tests/harness_defaults.rs:82` |
| `interactive_attempt_is_unchanged` | Linux + macOS | f? | assert_eq failed: claude | `horch-core/tests/p7_wrapper_exec.rs:457` |
| `job_requeue_sends_job_requeue` | Linux + macOS | f | Err: UNIQUE constraint failed: features.branch | `horch/tests/p5_route_operator_commands.rs:119` |
| `kind_catalog_is_the_table_of_the_design_and_the_kept_kinds` | Linux + macOS | f | assert_eq failed: Kind::ALL | `horch/tests/p6_retired_worker_commands.rs:375` |
| `launch_environment_sets_256k_context_limit` | Linux | e | Err: app-release-preparer: refusing to launch without a sandbox: bwrap and socat not on PATH; install bubblewrap and socat | `horch-core/tests/p7_context_256k.rs:79` |
| `launching_attempt_with_no_process_past_the_start_timeout_ends_timed_ou` | macOS | f? | error: test failed, to rerun pass `-p horch-core --test fleetd_pass` | `horch-core/tests/fleetd_pass.rs:613` |
| `ledger_status_shows_the_total_size_of_all_trees` | Linux + macOS | f | assert_eq failed: ["ledger", "run", "new", "r1"]: horch ledger run new is retired. Add an idea: horch idea add "<idea>" | `horch/tests/p3_worktrees_cli.rs:178` |
| `ledger_stop_stops_a_spawned_worker_by_the_ingested_pid` | Linux + macOS | a | `ledger_stop_stops_a_spawned_worker_by_the_ingested_pid` is on the list of expected failures with the reason `horch ledger run new | `horch-e2e/src/expected.rs:108` |
| `line_over_the_limit_is_refused` | Linux | f? | Err: Os { code: 104, kind: ConnectionReset, message: "Connection reset by peer" } | `horch-core/tests/fleetd_channel.rs:157` |
| `next_attempt_of_its_task_runs_in_exec_mode` | Linux | f? | Running tests/relay_first_delivery.rs (target/debug/build/horch-e2e/b48eacb3e51948ff/out/relay_first_delivery-b48eacb3e51948ff) | `horch-e2e/src/harness.rs:963` |
| `no_code_path_creates_a_task_without_an_idea` | Linux + macOS | f | a code path writes the idea of a run for its task: ["horch/src/cmd/worker.rs", "horch-e2e/src/ledger_seed.rs"] | `horch/tests/p6_retired_run_commands.rs:414` |
| `only_the_bind_and_its_dispatch_points_choose_a_task` | Linux + macOS | f? | FR-5: only the bind module, fleetd and horch spawn choose a task: | `horch-core/tests/fr5_push_only.rs:136` |
| `operator_boundary_passes_for_each_adapter` | macOS | f? | error: test failed, to rerun pass `-p horch-e2e --test p7_launcher` | `horch-e2e/src/harness.rs:963` |
| `oracle_launch_matches` | Linux | f? | launch/architect-reviewer.json differs from the A0 oracle at line 31: | `horch-core/tests/baseline_oracles.rs:57` |
| `over_budget_never_removes_an_active_tree_and_holds_dispatch` | Linux + macOS | f | Err: run r1 has no idea in flight of its feature F-0001, and a task needs an idea (FR-12). Add one: horch idea add | `horch-core/tests/worktree_disk.rs:191` |
| `over_budget_removes_landed_and_rescued_trees_of_finished_runs_oldest_f` | Linux + macOS | f | Err: run r1 has no idea in flight of its feature F-0001, and a task needs an idea (FR-12). Add one: horch idea add | `horch-core/tests/worktree_disk.rs:191` |
| `oversized_result_is_refused` | Linux + macOS | f | assert_eq failed: horch ["ledger", "run", "new", "r1"]: horch ledger run new is retired. Add an idea: horch idea add "<idea>" | `horch/tests/p4_result_cli.rs:194` |
| `pass_adopts_a_hand_made_worktree_within_1_list_period` | Linux + macOS | f? | job 1 ended: JobRow { id: 1, kind: WorktreeList, subject: "<path> repo: Some("<path> input_key: "0", run: 1, abandoned_runs: 0, no | `horch-core/tests/fleetd_pass_p3.rs:325` |
| `question_of_a_claim_attempt_writes_no_file` | Linux + macOS | f | assert_eq failed: horch ["ledger", "run", "new", "r1"]: horch ledger run new is retired. Add an idea: horch idea add "<idea>" | `horch/tests/p4_question_file.rs:186` |
| `question_of_a_push_attempt_writes_1_question_file` | Linux + macOS | f? | Running tests/p4_result_cli.rs (target/debug/build/horch/b26bdedac14b33f8/out/p4_result_cli-b26bdedac14b33f8) | `horch/tests/p4_question_file.rs:186` |
| `render_prints_each_section` | Linux + macOS | f? | assert_eq failed | `horch-core/tests/w3_taskledger_core_status.rs:140` |
| `requeue_cancel_and_waive_send_their_kinds` | Linux + macOS | f? | assert_eq failed: ["ledger", "run", "new", "r1"] | `horch/tests/p5_route_ledger_task.rs:177` |
| `restore_of_a_task_makes_branch_rescue_t_seq_n_and_its_tree_with_each_r` | Linux + macOS | f | assert_eq failed: ["ledger", "run", "new", "r1"]: horch ledger run new is retired. Add an idea: horch idea add "<idea>" | `horch/tests/p3_worktrees_cli.rs:178` |
| `restore_of_a_task_whose_tree_is_removed_uses_its_rescue_ref` | Linux + macOS | f | assert_eq failed: ["ledger", "run", "new", "r1"]: horch ledger run new is retired. Add an idea: horch idea add "<idea>" | `horch/tests/p3_worktrees_cli.rs:178` |
| `restore_of_an_archive_name_keeps_today_s_behavior` | Linux + macOS | f | assert_eq failed: ["ledger", "run", "new", "r1"]: horch ledger run new is retired. Add an idea: horch idea add "<idea>" | `horch/tests/p3_worktrees_cli.rs:178` |
| `restore_refuses_when_the_branch_or_the_folder_exists_and_changes_nothi` | Linux + macOS | f | assert_eq failed: ["ledger", "run", "new", "r1"]: horch ledger run new is retired. Add an idea: horch idea add "<idea>" | `horch/tests/p3_worktrees_cli.rs:178` |
| `restore_runs_no_hook_of_the_project` | Linux + macOS | f | assert_eq failed: ["ledger", "run", "new", "r1"]: horch ledger run new is retired. Add an idea: horch idea add "<idea>" | `horch/tests/p3_worktrees_cli.rs:178` |
| `result_json_with_another_status_is_refused` | Linux + macOS | f | assert_eq failed: horch ["ledger", "run", "new", "r1"]: horch ledger run new is retired. Add an idea: horch idea add "<idea>" | `horch/tests/p4_result_cli.rs:194` |
| `result_outside_a_push_attempt_is_refused` | Linux + macOS | f | assert_eq failed: horch ["ledger", "run", "new", "r1"]: horch ledger run new is retired. Add an idea: horch idea add "<idea>" | `horch/tests/p4_result_cli.rs:194` |
| `result_writes_a_valid_file_once` | Linux + macOS | f | assert_eq failed: horch ["ledger", "run", "new", "r1"]: horch ledger run new is retired. Add an idea: horch idea add "<idea>" | `horch/tests/p4_result_cli.rs:194` |
| `resume_keeps_an_override_and_refuses_a_hand_edited_record` | Linux | e | Err: sonnet: refusing to launch without a sandbox: bwrap and socat not on PATH; install bubblewrap and socat | `horch-core/tests/execution_plan.rs:1418` |
| `run_new_creates_the_branch_and_reports_it` | Linux + macOS | f | assert_eq failed: horch ledger run new is retired. Add an idea: horch idea add "<idea>" | `horch/tests/w3_ledger_cli_pins.rs:158` |
| `run_new_sends_1_run_new_request` | Linux + macOS | f? | assert_eq failed: ["ledger", "run", "new", "r1", "--check", "true"] | `horch/tests/p5_route_ledger_run.rs:140` |
| `second_result_of_1_attempt_is_refused` | Linux + macOS | f | assert_eq failed: horch ["ledger", "run", "new", "r1"]: horch ledger run new is retired. Add an idea: horch idea add "<idea>" | `horch/tests/p4_result_cli.rs:194` |
| `spawn_model_fable_is_allowed_for_an_ordinary_claude_teammate` | Linux | e | Err: sonnet: refusing to launch without a sandbox: bwrap and socat not on PATH; install bubblewrap and socat | `horch-core/tests/execution_plan.rs:1418` |
| `stopping_one_of_three_workers_leaves_the_others_running` | Linux + macOS | a | `stopping_one_of_three_workers_leaves_the_others_running` is on the list of expected failures with the reason `timed out waiting f | `horch-e2e/src/expected.rs:108` |
| `store_dirlock_excludes_concurrent_writers` | Linux | f? | Err: Any { .. } | `horch-core/tests/execution_store.rs:343` |
| `t27_every_intake_teammate_reads_but_does_not_write_its_bundle` | Linux + macOS | f? | assert_eq failed: intake-auditor | `horch-core/tests/intake_roster.rs:454` |
| `t28_a_full_sandboxed_teammate_writes_no_skills_bundle` | Linux + macOS | f? | skill-importer | `horch-core/tests/intake_roster.rs:506` |
| `task_add_and_task_solo_send_their_kinds` | Linux + macOS | f? | assert_eq failed: ["ledger", "run", "new", "r1"] | `horch/tests/p5_route_ledger_run.rs:140` |
| `task_requeue_and_cancel_send_their_kinds` | Linux + macOS | f | Err: UNIQUE constraint failed: features.branch | `horch/tests/p5_route_operator_commands.rs:119` |
| `taskledger::schema::v7_tests::trf_48_every_old_version_migrates_to_cur` | Linux + macOS | f? | assert_eq failed: version 14 | `horch-core/src/taskledger/schema.rs:2279` |
| `taskledger::stop::tests::trf_36_a_cancel_stops_the_worker_by_its_pids_` | Linux + macOS | f | Err: run s has no idea in flight of its feature F-0001, and a task needs an idea (FR-12). Add one: horch idea add | `horch-core/src/taskledger/stop.rs:1043` |
| `taskledger::stop::tests::trf_36_a_worker_that_cannot_be_stopped_leaves` | Linux + macOS | f | Err: run s has no idea in flight of its feature F-0001, and a task needs an idea (FR-12). Add one: horch idea add | `horch-core/src/taskledger/stop.rs:1043` |
| `the_board_commands_print_their_pinned_text` | Linux + macOS | f | assert_eq failed: ["ledger", "run", "new", "r1", "--steps", "check,land"]: horch ledger run new is retired. Add an idea: horch ide | `horch/tests/w3_ledger_cli_pins.rs:138` |
| `the_legacy_resume_and_assign_arms_run` | Linux + macOS | f? | ["add", "--record-id", "r1", "--agent", "claude", "--tier", "t", "--model", "m", "--role", "dev", "--task", "first"]: Output { sta | `horch/tests/r_cli_cmds_ledger.rs:29` |
| `the_legacy_subcommands_write_read_and_report_a_session` | Linux + macOS | f? | ["add", "--record-id", "r1", "--agent", "claude", "--tier", "t", "--model", "m", "--role", "dev", "--task", "do it"]: Output { sta | `horch/tests/r_cli_cmds_ledger.rs:29` |
| `the_unfinished_import_and_refusal_arms_print_their_pinned_text` | Linux + macOS | f | assert_eq failed: ["ledger", "run", "new", "r1"]: horch ledger run new is retired. Add an idea: horch idea add "<idea>" | `horch/tests/w3_ledger_cli_pins.rs:138` |
| `trf_02_template_example_map_parses_with_the_ticket` | Linux + macOS | f? | the template has no '== Example MAP.md ==' line | `horch-core/tests/trf_02_ticket_template.rs:16` |
| `trf_11_dry_run_names_each_problem_and_writes_nothing` | Linux + macOS | f | assert_eq failed: ["ledger", "run", "new", "r1"]: horch ledger run new is retired. Add an idea: horch idea add "<idea>" | `horch/tests/trf_11_add_map_cli.rs:110` |
| `trf_13_add_map_refuses_a_ticket_already_queued_unless_again` | Linux + macOS | f | assert_eq failed: ["ledger", "run", "new", "r1"]: horch ledger run new is retired. Add an idea: horch idea add "<idea>" | `horch/tests/trf_11_add_map_cli.rs:110` |
| `trf_14_add_map_stores_kind_review_and_check_none` | Linux + macOS | f | assert_eq failed: ["ledger", "run", "new", "r1"]: horch ledger run new is retired. Add an idea: horch idea add "<idea>" | `horch/tests/trf_11_add_map_cli.rs:110` |
| `triage_archive_is_refused_with_a_later_task` | Linux + macOS | f? | assert_eq failed: W1 feat DELETE: queued in the triage_apply job 1; horch worktree triage shows the result | `horch/tests/p5_cli_writers.rs:403` |
| `trial_merge_job_does_not_check_out` | Linux + macOS | f? | assert_eq failed | `horch-core/tests/trial_merges.rs:412` |
| `trial_merge_of_2_tips_runs_once` | Linux + macOS | f? | assert_eq failed | `horch-core/tests/trial_merges.rs:329` |
| `two_questions_write_2_files` | Linux + macOS | f | assert_eq failed: horch ["ledger", "run", "new", "r1"]: horch ledger run new is retired. Add an idea: horch idea add "<idea>" | `horch/tests/p4_question_file.rs:186` |
| `watch_once_json_lists_each_checked_worker` | Linux + macOS | f | assert_eq failed: ["ledger", "run", "new", "r1", "--check-every", "10", "--stall-rounds", "1000"]: horch ledger run new is retired | `horch/tests/w3_ledger_cli_pins.rs:138` |
| `watch_once_reports_in_text_and_json` | Linux + macOS | f | assert_eq failed: ["ledger", "run", "new", "r1", "--check-every", "10"]: horch ledger run new is retired. Add an idea: horch idea  | `horch/tests/w3_ledger_cli_pins.rs:138` |
| `worker_that_removes_horch_role_is_refused_by_fleetd` | Linux + macOS | f? | assert_eq failed: ["ledger", "run", "new", "r1"] | `horch/tests/p5_route_ledger_task.rs:177` |
| `worker_that_sets_horch_role_orchestrator_is_refused_by_fleetd` | Linux + macOS | f? | assert_eq failed: ["ledger", "run", "new", "r1"] | `horch/tests/p5_route_ledger_task.rs:177` |
| `worktree_drop_sends_worktree_drop` | Linux + macOS | f? | assert_eq failed: ["ledger", "run", "new", "r1"] | `horch/tests/p5_route_ledger_task.rs:177` |
| `worktree_list_job_is_queued_once_per_period_per_repository` | Linux + macOS | f? | job 1 ended: JobRow { id: 1, kind: WorktreeList, subject: "<path> repo: Some("<path> input_key: "0", run: 1, abandoned_runs: 0, no | `horch-core/tests/fleetd_pass_p3.rs:325` |
| `worktree_list_job_writes_no_index_of_a_tree` | Linux + macOS | f? | job 1 ended: JobRow { id: 1, kind: WorktreeList, subject: "<path> repo: Some("<path> input_key: "0", run: 1, abandoned_runs: 0, no | `horch-core/tests/fleetd_pass_p3.rs:325` |
| `worktree_new_refuses_a_shared_task_and_returns_an_existing_worktree` | Linux + macOS | f? | Err: T-2 has the attempt 430556fa-70e5-4647-b4e2-1aa0613234de/A1 that is not ended: 1 task has 1 live attempt (I1) | `horch-core/tests/w3_finish_steps.rs:1535` |
| `worktree_restore_and_triage_send_their_kinds` | Linux + macOS | f | Err: UNIQUE constraint failed: features.branch | `horch/tests/p5_route_operator_commands.rs:119` |
| `worktree_triage_archive_sends_1_request_and_queues_1_job` | Linux + macOS | f | Err: UNIQUE constraint failed: features.branch | `horch/tests/p5_route_operator_commands.rs:119` |
| `worktree_triage_queues_its_jobs` | Linux + macOS | f? | applied: {"applied":[],"failed":0,"job":2,"queued":["W1 _scratch/t1 ARCHIVE: queued in the triage_apply job 2; horch worktree tria | `horch-core/tests/fleetd_handlers_custody.rs:153` |
| `worktrees_json_has_each_field` | Linux + macOS | f | assert_eq failed: ["ledger", "run", "new", "r1"]: horch ledger run new is retired. Add an idea: horch idea add "<idea>" | `horch/tests/p3_worktrees_cli.rs:178` |
| `worktrees_lists_each_tree_with_state_task_owner_age_and_size` | Linux + macOS | f | assert_eq failed: ["ledger", "run", "new", "r1"]: horch ledger run new is retired. Add an idea: horch idea add "<idea>" | `horch/tests/p3_worktrees_cli.rs:178` |
| `worktrees_shows_the_rescue_job_that_the_reconciler_queued_for_an_orpha` | Linux + macOS | f | assert_eq failed: ["ledger", "run", "new", "r1"]: horch ledger run new is retired. Add an idea: horch idea add "<idea>" | `horch/tests/p3_worktrees_cli.rs:178` |

## Linux

Job `test (ubuntu-latest)`: failure. 4384 tests passed, 693 tests failed.
Job `docs`: failure. Job `release-lint`: failure.

### Groups of `test (ubuntu-latest)`

| Test or step | Class | First error line | file:line |
|---|---|---|---|
| About 523 tests of `horch-e2e` that start the harness | d | `the build of horch and the fakes failed … error: profile `out` is not defined` | `crates/horch-e2e/src/harness.rs:1012-1016` |
| 38 tests of `horch-e2e` that build only the fakes | d | `the build of the fakes failed … --profile out` | `crates/horch-e2e/src/harness.rs:963` |
| 27 tests of `horch-core` (see the table below, class e) | e | `refusing to launch without a sandbox: bwrap and socat not on PATH; install bubblewrap and socat` | `crates/horch-core/src/harness/launch.rs` (many tests), `crates/horch-core/tests/context_launch.rs:157`, `execution_plan.rs:1418`, `p7_conformance_job.rs:346`, `baseline_oracles.rs:57,498` |

The step `Install bubblewrap` of `ci.yml` installs `bubblewrap` only. The message says that `socat` is also needed. `crates/horch-platform/src/linux/sandbox.rs` is the place that checks both.

### Failures that are not caused by the build

The table of the section macOS holds all the failures that are not caused by the build, for both OSes. A row with the OS `Linux` fails on Linux only.

### Job `docs` (`cargo doc --workspace --no-deps --all-features --locked`, `RUSTDOCFLAGS=-D warnings`)

33 rustdoc errors, all in `horch-core`. First error line of each group is in the column.

| Group | Count | Class | First error line | file:line |
|---|---|---|---|---|
| public doc links to a private item | 16 | d | `public documentation for `exec_push` links to private item `SAFE_CONFIG`` | `crates/horch-core/src/fleetd/integration/adapters/exec_push.rs:12` and 15 more (`new_features.rs:130`, `task_execution/application/lease.rs:11`, `quota.rs:337`, `worktree_custody/adapters/git.rs:509,666`, `worktree_custody/application/create.rs:234`, `harness/codex.rs:1906`, `roster/validation.rs:1842`, `taskledger/import.rs:1214`, `taskledger/schema.rs:1447,1454`, `taskledger/tasks.rs:61`, `taskledger/workers.rs:782`) |
| name is both a function and a module | 9 | d | `` `cancel` is both a function and a module`` | `fleetd/ideas/application/park_cancel.rs:17,70,327`, `integration/application/commands.rs:9,141`, `task_execution/application/outbox_ingest.rs:11`, `verifier.rs:25,27,466` |
| unclosed HTML tag | 3 | d | ``unclosed HTML tag `n` `` | `fleetd/fleet_runtime/application/jobs.rs:673`, `worker_profiles/application/drift_idea.rs:5,6` |
| redundant explicit link target | 3 | d | `redundant explicit link target` | `task_execution/application/quota.rs:2,3`, `worker_profiles/adapters/launcher.rs:6` |
| unresolved link | 2 | d | ``unresolved link to `create_and_wait` `` | `taskledger/finish/mod.rs:546`, `taskledger/wakes.rs:535` (`held_state`) |

These are doc faults of the code of fleetd. They are the same on every OS. The paths above are under `crates/horch-core/src/`. They are class (d) in the sense of a real fault of the product source, not of an OS.

### Job `release-lint` (`cargo clippy --workspace --bins --release --locked -- -D warnings`)

1 error, class (d). The `clippy` of rustc 1.100 is stricter than the one that the code was written for.

| Step | Class | First error line | file:line |
|---|---|---|---|
| `cargo clippy` | d | ``error: using `chunks_exact` with a constant chunk size`` (`-D clippy::chunks-exact-to-as-chunks`) | `crates/horch-core/src/fleetd/worktree_custody/adapters/git.rs:823` (`parts.chunks_exact(3)` in `project_program`) |

## Windows

Job `build-windows` (`cargo build --workspace --locked`): success.

Job `test-windows` (`cargo test --workspace --locked --no-fail-fast`): failure. **0 tests ran**: the build of the tests did not compile, so the counts are 0 passed and 0 failed. Cargo stops at a compile error, even with `--no-fail-fast`. More compile errors can show up in other crates after these are fixed.

| Test or step | Class | First error line | file:line |
|---|---|---|---|
| `horch-marketplace` test `marketplace` | c | ``error[E0433]: cannot find `unix` in `os``` (`std::os::unix::fs::symlink`) | `crates/horch-marketplace/tests/marketplace.rs:445`, `:459` |
| `horch-core` (lib test), test of `land` | c | ``error[E0433]: cannot find `unix` in `os``` (`use std::os::unix::fs::PermissionsExt`) | `crates/horch-core/src/taskledger/finish/land.rs:1446` |
| same test | c | ``error[E0599]: no method named `set_mode` found for struct `Permissions` `` | `crates/horch-core/src/taskledger/finish/land.rs:1471` |
| `horch-core` (lib test), test of `pr` | c | ``error[E0433]: cannot find `test_support` in the crate root`` (the module has `#[cfg(all(test, unix))]`) | `crates/horch-core/src/taskledger/finish/pr.rs:700` (module at `crates/horch-core/src/lib.rs:72-73`) |
| `horch-core` (lib test), tests of `codex` | b | ``error[E0425]: cannot find function `prepare_in` in this scope`` (2 uses; the function is unix-only) | `crates/horch-core/src/harness/codex.rs:3312`, `:3417` |
| `horch-core` (lib test), test of the outbox lock | c | ``error[E0433]: cannot find module or crate `libc` in this scope`` (13 uses of `libc::pipe`, `fork`, `close`, `read`, `waitpid`, `_exit`, `pid_t`, `c_int`) | `crates/horch-core/src/taskledger/outbox.rs:504-538` |

Total: 16 compile errors in `horch-core` (lib test) and 2 in `horch-marketplace` (test `marketplace`). 3 warnings (unused import `FORBIDDEN_ENV` at `fleetd/worker_profiles/adapters/launcher.rs:112`, unused import `std::process::Command` at `workspace/herdr.rs:870`, unused variable `round` at `fsx.rs:879`) do not fail the job.

## Fix units

Each unit is for 1 worker. The order is the order of the value: unit 1 un-blocks about 560 of the failures on each of the 2 OSes.

| Unit | Fixes | Files it touches |
|---|---|---|
| FIX-1: the e2e harness profile | the 561 tests of the build of the e2e harness, on macOS and Linux | `crates/horch-e2e/src/harness.rs` (`build_command`, lines about 960-1030, and its unit test near line 1264). Read the profile from `--profile`/`CARGO_BUILD_*` or from `OUT_DIR`, or build to a fixed target folder. Do not take the profile from the name of the last folder. |
| FIX-2: socat on Linux CI | the 27 Linux-only sandbox failures | `.github/workflows/ci.yml` (the step `Install bubblewrap`: add `socat`). This unit needs the operator, because the ticket CI0 forbids the edit of `ci.yml`. |
| FIX-3: rustdoc | the job `docs` (33 errors) | the 19 files named in the table of the job `docs`, all under `crates/horch-core/src/` |
| FIX-4: clippy | the job `release-lint` (1 error) | `crates/horch-core/src/fleetd/worktree_custody/adapters/git.rs` (line 823) |
| FIX-5: Windows test build, `horch-core` | the 16 errors of the lib test | `crates/horch-core/src/taskledger/finish/land.rs`, `crates/horch-core/src/taskledger/finish/pr.rs`, `crates/horch-core/src/taskledger/outbox.rs`, `crates/horch-core/src/harness/codex.rs`, `crates/horch-core/src/lib.rs` (the gate `#[cfg(all(test, unix))]`). Gate each test with `#[cfg(unix)]`, or write a Windows form. |
| FIX-6: Windows test build, `horch-marketplace` | the 2 errors of the test `marketplace` | `crates/horch-marketplace/tests/marketplace.rs` (lines 445 and 459: gate the 2 symlink tests with `#[cfg(unix)]`) |
| FIX-7: Windows next errors | run `cargo test --workspace --locked --no-fail-fast` on Windows after FIX-5 and FIX-6, and record the new errors | no file known yet. A run needs `workflow_dispatch` again, or a Windows host. |
| FIX-8: stale tests of a retired command | the tests in the group `f` (58 rows): `horch ledger run new is retired`, `horch ledger task is retired`, `run … has no idea in flight`, `UNIQUE constraint failed: features.branch`, `reading the operator token`, `Kind::ALL` | the test files named in the column `file:line` of the table: `crates/horch/tests/p3_worktrees_cli.rs`, `p4_result_cli.rs`, `p4_question_file.rs`, `p5_route_ledger_task.rs`, `p5_route_ledger_run.rs`, `p5_route_operator_commands.rs`, `w3_ledger_cli_pins.rs`, `trf_11_add_map_cli.rs`, `ledger_cli.rs`, `context_cli.rs`, `p6_retired_worker_commands.rs`, `p6_retired_run_commands.rs`, `crates/horch-core/tests/worktree_disk.rs`, `crates/horch-e2e/tests/fleet18_messaging.rs`, and the `src/cmd/*.rs` unit tests in the table. These tests fail on every OS, so a local macOS run shows them too. Check this first on the worker side. |
| FIX-9: expected-failures list | the 7 rows of class (a): a listed test now fails for another reason, or passes | `crates/horch-e2e/src/expected.rs` check, and the lists in `crates/horch-e2e/expected-failures/*.txt` (`p0_*.txt`, `taskledger.txt`, `fleet18_observability.txt`). Rule 7: run each changed test 3 times. |
| FIX-10: unexplained failures | the 50 rows of class `f?`, among them the rows that fail on 1 OS only (`gate_at_its_limit_is_abandoned_and_its_group_killed`, `store_dirlock_excludes_concurrent_writers`, `line_over_the_limit_is_refused`, `launching_attempt_with_no_process_past_the_start_timeout_ends_timed_out`) | the files named in the table. The 4 named tests look like timing or process tests. Rule 6 applies: run each 3 times, and find the cause. Do not accept a flake. |

## Notes for the next run

- Do not start a second run only to see the same logs. Run again after FIX-1, FIX-3, FIX-4, FIX-5 and FIX-6 are on the branch.
- The full logs are in the run: `gh run view 44680906908 --log-failed`. The job ids are `136103798948` (macOS), `136103798920` (Linux), `136103798969` (`test-windows`), `136103798982` (`docs`), `136103798898` (`release-lint`).
