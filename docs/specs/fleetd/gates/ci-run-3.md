# CI run 3 of fleetd, 2026-11-20

- Run: 44784709135, https://github.com/MattMakes/multi-herdr/actions/runs/44784709135
- Trigger: `workflow_dispatch` on `fleet/fleetd-finish`, started 2026-11-20 17:35Z, ended 19:33Z.
- Head commit of the run: `001e73c290b950662fe5f34ad76d017d1bfc72ab` (T-209 comment sweep). The tip of `origin/fleet/fleetd-finish` at the start was the same commit. The check "headSha equals the tip" holds.
- Toolchain: `stable` = rustc 1.100.0 (0ef1bc386 2026-11-09), the same as CI0 and CI1.
- Compared with: CI1, `docs/specs/fleetd/gates/ci-run-2.md` (run 44701297167, head `7fbdc0d3`). 205 commits landed between the 2 heads.
- Source of the counts: the sums of the lines `test result:` of each job log, and the blocks `---- <test> stdout ----` of the failed tests. Nothing is run locally. The logs of CI1 were read again, so each CI3 failure is compared by test name with CI1.
- Run result: failure. 2 of 6 jobs passed (CI1: 1 of 6).

| Job | OS | Result | Time |
|---|---|---|---|
| `test (macos-15)` | macOS | failure: 5002 passed, 188 failed, 26 ignored | 106 min 24 s |
| `test (ubuntu-latest)` | Linux | failure: 4821 passed, 368 failed, 26 ignored | 117 min 41 s |
| `docs` | Linux | **success** | 1 min 14 s |
| `release-lint` | Linux | failure, 2 errors | 1 min 41 s |
| `build-windows` | Windows | success | 1 min 47 s |
| `test-windows` | Windows | failure (the job has `continue-on-error: true`): 0 tests ran, 1 compile error | 3 min 18 s |

## Classes

- (a) a test that is on `crates/horch-e2e/expected-failures/`
- (b) a compile error on that OS
- (c) a test that is unix-only by its code but runs on Windows
- (d) a real fault of the product (or of the test harness) on that OS
- (e) a fault of the CI setup
- (f) not one of the classes of the ticket: the test is stale. It calls a command that fleetd retired, or it lacks a setup that the product needs now. It fails the same way on each OS. CI0 and CI1 used the same class.

A cause that the log does not name in the failure line is marked "probable".

## Jobs of `ci.yml`

| Job | OS |
|---|---|
| `test` (matrix) | `ubuntu-latest`, `macos-15` |
| `docs` | `ubuntu-latest` |
| `release-lint` | `ubuntu-latest` |
| `build-windows` | `windows-latest` |
| `test-windows` | `windows-latest`, `continue-on-error: true` |

The file has no `timeout-minutes`. The jobs `test` took 106 min and 118 min on this run, up from 55 min and 72 min in CI1. CI3 runs more tests (macOS 5190 against 5066) and its e2e tests wait longer. This is a note: the ticket forbids an edit of `ci.yml`.

## macOS

Job `test (macos-15)`: failure. 5002 tests passed, 188 failed, 26 ignored. In CI1: 4796 passed, 270 failed.

### Groups

| Group | Count | Class | First error line | file:line |
|---|---|---|---|---|
| Tests of the retired command `horch ledger run new` | 90 | f | `horch ledger run new is retired. Add an idea: horch idea add "<idea>"` | `crates/horch-e2e/tests/taskledger.rs:195` (58), `p3_refusals.rs:52` (10), `p2_leases.rs:107` (7), `p3_unlanded.rs:50` (7), `p3_worktrees.rs:90` (7), `p4_clock_and_heartbeat.rs` (1, `slow_worktree_add_of_60_s_keeps_the_heartbeat_and_the_tree_i...`) |
| The fakes of `horch-core` are built with `--profile out` | 42 | d | ``error: profile `out` is not defined`` after `the build of the fakes failed: cd ".../crates/horch-core/../.." && env -u CARGO_MANIFEST_DIR ... cargo build --locked -p horch-e2e --bin fake-claude ... --profile out` | `crates/horch-core/tests/p7_conformance.rs:86` (20), `p7_operator_boundary.rs:102` (10), `p7_context_limit.rs:108` (6), `p7_wrapper_exec.rs:86` (6). The cause is the `match dir.file_name()` block in the same files (lines 75-81, 90-96, 97-103, 75-81). |
| The profile `antigravity` is unhealthy at boot, so dispatch to it is held (probable cause of the timeout) | 11 | d | `timed out after 120 s waiting for the judged task and its judge task` with the profile fact `antigravity profile_unhealthy adapter-conformance-failed: agy 1.1.3: Private home honoured, Exact MCP set, Sandbox boundary` | `crates/horch-e2e/src/harness.rs:1145` (the wait); the failed conformance is `AGY_FAILS` at `crates/horch-e2e/tests/p7_conformance_binary.rs:338`; the hold is T-48 (R6-F1, fail closed) |
| Other failures | 45 | see below | see below | see below |

Total: 90 + 42 + 11 + 45 = 188.

The 11 tests of the group "profile `antigravity` unhealthy": `arc_26_e2e_lifecycle_matrix_claude`, `arc_26_e2e_lifecycle_matrix_pi` (`lifecycle.rs`), `run_without_gates_holds_dispatch_and_loses_no_task` (`p4_release.rs:1116`), `sr3_check_that_connects_off_host_is_refused` (`p4_verifier.rs:658`), `fr12_direct_judgment_idea_gets_its_judge` (`p6_intake.rs`), and the 6 tests of `p6_judge_pair.rs`: `fm33_two_fix_rounds_escalate`, `cancel_of_a_judge_task_alone_is_refused`, `cancel_of_a_judged_task_cancels_its_judge_task`, `fm9_judgment_task_is_done_only_after_its_judge`, `rr3_critical_gives_1_fix_round`, `rr3_judgment_and_judge_reach_done_with_no_plan_change`.

### Other failures (45 tests)

| Test | Class | First error line | file:line |
|---|---|---|---|
| `an_old_registry_others_can_write_is_not_copied_and_inbox_says_why`, `done_outside_a_git_work_tree_reports_no_scan_error`, `g5_done_without_an_orchestrator`, `g5_inbox_marks_a_role_whose_pane_closed`, `mail_u1_done_push_failure_records_queued_and_closes`, `mail_u1_done_stays_open_when_enqueue_fails`, `p09_registry_in_the_old_temp_location_still_resolves`, `p09_registry_lives_in_the_state_folder`, `t11_followup_done_warns_when_the_registration_stays` (9) | d? | `timed out after 120 s waiting for the running worker`; the record shows the worker `running`; the pane prints `horch did not trust .../work/alpha for codex: the directory of $CODEX_HOME/config.toml (.../home/.codex/config.toml) does not exist`. The cause is not proved. | `crates/horch-e2e/src/harness.rs:1145`, tests of `crates/horch-e2e/tests/messaging.rs` |
| `fitness_no_context_touches_foreign_tables` | d | `a context names a table of another context: fleetd/ideas/application/answers.rs:382: table `events` is not a table of ideas` (also `:660`) | `crates/horch-core/tests/arch_scan.rs:1880`; the cause is `pending_answer_files` of T-44 (commit `93bbb0d6`) at `crates/horch-core/src/fleetd/ideas/application/answers.rs:380` |
| `fleetd_killed_between_commit_and_effect_does_the_effect_once_on_restart`, `launch_is_skipped_when_a_live_process_has_the_attempt_id` | d? | ``assertion `left == right` failed left: "launching"`` | `crates/horch-core/tests/fleetd_pass.rs:511`, `:670` |
| `harness_codex_rules_snapshot` | d? | ``codex_rules_worker.rules differs from the pre-A4 snapshot`` | `crates/horch-core/tests/harness.rs:25` |
| `worker_that_removes_horch_role_is_refused_by_fleetd`, `worker_that_sets_horch_role_orchestrator_is_refused_by_fleetd` | f | `horch: refused inside a fleet attempt: this process is a fleet process of attempt a1, so it sends no operator request (task_requeue)` | `crates/horch/tests/p5_route_ledger_task.rs:371`, `:386` |
| `watch_once_reports_in_text_and_json` | f? | ``assertion `left == right` failed left: "dev1 T-1: dead (round 0): the agent process is gone\n" right: "No worker was due for a check.\n"`` | `crates/horch/tests/w3_ledger_cli_pins.rs:434` |
| `given_3_ready_branches_when_bundle_then_fast_gate_runs_once_and_none_i...` | d? | `timed out after 120 s waiting for 3 running task_check jobs` | `crates/horch-e2e/tests/p0_landing.rs` (wait at `harness.rs:1145`) |
| `fr10_b_agent_after_its_attempt_ended_cannot_approve_and_is_stopped` | d? | `the operator approves: status: exit status: 1` | `crates/horch-e2e/tests/p6_fr10_approval.rs:407` |
| `fr2_each_sub_orchestrator_attempt_has_1_idea` | d? | ``assertion `left == right` failed: the launch names 0 ideas`` | `crates/horch-e2e/tests/p6_sub_orchestrators.rs:353` |
| `a_worker_that_registers_after_the_wait_still_works` | d? | `timed out after 120 s waiting for the late worker ran to its end` | `crates/horch-e2e/tests/startup_error.rs` (wait at `harness.rs:1145`) |
| `g50_legacy_orchestrator_and_host_load_are_advisory` | f? | `horch: NOTE: the health guard free_disk does not hold dispatch: the probe has no reading` (left and right differ) | `crates/horch-e2e/tests/fleet18_observability.rs:806` |
| `a_fleet_with_an_abandoned_worktree_spawns_the_worktree_steward`, `an_exhausted_claude_pool_starts_no_codex_steward` | a | on the list with another reason, failed with another message (`horch ledger cancel T-1 not needed now`, `Creating fleet workspace`) | `crates/horch-e2e/src/expected.rs:108`, `crates/horch-e2e/tests/i308_fleet_steward.rs:41`, `:162` |
| `triage_delete_accepts_a_clean_detached_head_reachable_from_a_branch`, `triage_delete_refuses_a_detached_unique_commit` | f | `horch ledger cancel T-1 not needed now: status: exit status: 1` | `crates/horch-e2e/tests/i308_fleet_steward.rs:41` |
| `triage_lists_rows_from_a_ledger_it_cannot_write` | f? | ``assertion `left == right` failed: { "pr_offered": false,`` | `crates/horch-e2e/tests/i308_fleet_steward.rs:202` |
| `arc_26_e2e_lifecycle_matrix_antigravity`, `arc_26_e2e_lifecycle_matrix_codex`, `arc_26_e2e_lifecycle_matrix_prime` | d? | `timed out after 120 s waiting for the attempt to be live "claude" ["--version"]` (or `to end`, `the session`) | `crates/horch-e2e/tests/lifecycle.rs` (wait at `harness.rs:1145`) |
| `fm3_idle_claim_is_caught_in_under_10_min`, `fm5_usage_limited_worker_is_not_reaped_and_burns_no_attempt` | f | `horch ledger watch: status: exit status: 1`; `EOF while parsing a value at line 1 column 0` | `crates/horch-e2e/tests/p0_liveness.rs:63`, `:34` |
| `held_task_with_a_live_worker_on_phase_1_is_not_reaped_after_the_update_and_has_1_lease` | f | `horch ledger show T-1 --json: status: exit status: 1` | `crates/horch-e2e/tests/p2_release.rs:199` |
| `live_tree_and_dirty_orphan_on_phase_2_are_active_and_listed_and_nothing_is_removed_after_the_update` | f? | ``assertion `left == right` failed: status: exit status: 0`` | `crates/horch-e2e/tests/p3_release.rs:401` |
| `restore_of_a_task_makes_branch_rescue_t_seq_n_and_its_tree_with_each_rescued_file`, `restore_of_a_task_whose_tree_is_removed_uses_its_rescue_ref`, `restore_refuses_when_the_branch_or_the_folder_exists_and_changes_nothing`, `restore_runs_no_hook_of_the_project` | d? | `timed out waiting for fleetd to rescue the tree of T1` | `crates/horch/tests/p3_worktrees_cli.rs:140` |
| `pl4_sleep_past_a_branch_deadline_fires_nothing_on_wake` | f | `horch ledger watch: status: exit status: 1` | `crates/horch-e2e/tests/p4_clock_and_heartbeat.rs:246` |
| `operator_skills_e2e_changed_since_spawn_fails_the_launch`, `plugin_skills_e2e_changed_since_spawn_fails_the_launch`, `skl_06_e2e_exposure_antigravity` | f? | `the agent ran: stdout: stderr: horch: the skills changed since <attempt>`; ``assertion failed: agent_calls(&h, "antigravity").is_empty()`` | `crates/horch-e2e/tests/skills_exposure.rs:664`, `:777`, `:257` |
| `legacy_plumbing_lives_under_ledger_legacy`, `status_on_a_fresh_ledger_creates_it_and_says_no_runs`, `the_import_finds_work_that_never_landed` | f | `.../ledger.sqlite3` against `.../state/ledger.sqlite3`; `status: exit status: 1` | `crates/horch-e2e/tests/taskledger.rs:300`, `:274`, `:1541` |
| `no_status_literal_in_ledger_sql` | d | `status words written as literals; build them from the enums in taskledger/model.rs` (`'landed'` and `'abandoned'`) | `crates/horch-core/tests/w5_status_words_scan.rs:93`; the cause is `crates/horch-core/src/taskledger/triage/mod.rs:201` |

The count of rows above is lower than 45 because a row can name up to 9 tests.

## Linux

Job `test (ubuntu-latest)`: failure. 4821 tests passed, 368 failed, 26 ignored. In CI1: 4691 passed, 375 failed. `bubblewrap` and `socat` are installed by the step `Install bubblewrap` (T-75). The log has 12319 lines `bwrap is not on PATH`.

### Groups

| Group | Count | Class | First error line | file:line |
|---|---|---|---|---|
| Tests of the retired command `horch ledger run new` | 89 | f | `horch ledger run new is retired` | `crates/horch-e2e/tests/taskledger.rs:195` (57), `p3_refusals.rs:52` (10), `p3_unlanded.rs:50` (8), `p3_worktrees.rs:90` (7), `p2_leases.rs:107` (7) |
| The fakes of `horch-core` are built with `--profile out` | 42 | d | ``error: profile `out` is not defined`` | the same 4 files as on macOS (`p7_conformance.rs` 20, `p7_operator_boundary.rs` 10, `p7_context_limit.rs` 6, `p7_wrapper_exec.rs` 6) |
| The sandbox refuses to launch: `bwrap` and `socat` are not on the `PATH` of the test or of fleetd | 134 | d | `refusing to launch without a sandbox: bwrap and socat not on PATH` (unit tests of `horch-core`), `sandbox is not supported on linux: bwrap is not on PATH; install bubblewrap` (fleetd of the e2e tests, 3884 such lines in CI1) | see the table below |
| Other failures | 103 | see below | see below | see below |

Total: 89 + 42 + 134 + 103 = 368.

### The sandbox group (class d, 134 tests)

The cause is the same as in CI1: the tests give the launch another `PATH`. The group is bigger because 61 new tests (the tests of Phase 6 and 7 and `p7_exec.rs`) start fleetd and fail on the same line.

| Cause | Tests | file:line of the cause |
|---|---|---|
| The unit tests build `LaunchEnv::for_test()` with `path: None`. The sandbox check then searches no folder | 27: `harness::launch::tests::*` and `harness::claude::tests::*` in the lib test of `horch-core` (17), `baseline_oracles.rs` (2), `context_launch.rs` (2), `execution_plan.rs` (2), `harness_defaults.rs` (2), `p7_conformance_job.rs` (1), `p7_context_256k.rs` (1) | `crates/horch-core/src/harness/launch.rs:153-159` (`for_test`), `crates/horch-core/src/harness/claude.rs:1637-1658` |
| The e2e harness seals the environment of fleetd and of each CLI with `PATH` = the folder of the fakes only. fleetd cannot find `/usr/bin/bwrap` | 107: `p7_exec.rs` 8, `p6_fr10_approval.rs` 8, `p4_verifier.rs` 6, `p6_judge_pair.rs` 6, `p6_questions.rs` 6, `p6_worker_question.rs` 6, `p4_no_loss.rs` 5, `p6_features.rs` 5, `p6_intake.rs` 5, `p4_remote.rs` 4, `p4_rescue_refs.rs` 4, `p4_safety.rs` 4, `p6_plans.rs` 4, `p6_sub_orchestrators.rs` 4, `p7_docs_sync.rs` 4, `fakes.rs` 3, `p4_release.rs` 3, `p4_repair.rs` 3, `p5_operator_boundary.rs` 3, `skills_exposure.rs` 3, `p0_landing.rs` 2, `p6_idea_flow_binary.rs` 2, `p6_idea_lead_launch.rs` 2, `p7_release.rs` 2, and 1 each in `brief.rs`, `p1_pass.rs`, `p3_triage.rs`, `p7_conformance_binary.rs`, `p7_launcher.rs` | `crates/horch-e2e/src/harness.rs:375` (`env.insert("PATH", self.bin ...)` in `seal`) |

The failure of the unit test `sandbox::tests::local_network_allows_loopback_and_refuses_an_off_host_address` is a different cause (see below).

### Other failures of Linux (103 tests)

The tests that fail on macOS too are in the macOS section with the same cause: `fitness_no_context_touches_foreign_tables`, `harness_codex_rules_snapshot`, `fleetd_killed_between_commit_and_effect...`, `worker_that_*_is_refused_by_fleetd` (2), `watch_once_reports_in_text_and_json`, `no_status_literal_in_ledger_sql`, `messaging.rs` (9, same record, same pane), `p0_liveness.rs` (2), `p2_release.rs`, `p3_release.rs`, `p4_clock_and_heartbeat.rs`, `taskledger.rs` (3 of the macOS rows), `skills_exposure.rs` (`skl_06`), `g50_legacy_orchestrator_and_host_load_are_advisory`, the 3 steward tests of `i308_fleet_steward.rs` with class (a), and the lifecycle matrix tests. The tests that fail on Linux only:

| Test | Class | First error line | file:line |
|---|---|---|---|
| `sandbox::tests::local_network_allows_loopback_and_refuses_an_off_host_address` | d | ``assertion `left == right` failed left: "refused ConnectionRefused Some(111)" right: "connected"`` | `crates/horch-platform/src/sandbox.rs:374` (the `Network::Local` of T-17) |
| `large_reply_to_non_reading_client_times_out_and_does_not_block_the_server` | d | ``called `Result::unwrap_err()` on an `Ok` value: ()`` | `crates/horch-core/tests/channel.rs:252` |
| `gate_at_its_limit_is_abandoned_and_its_group_killed` | d? | `the gate group was killed` | `crates/horch-core/tests/gate_jobs.rs:964` |
| `a_push_review_task_ends_done_by_its_findings_file_and_its_task_lands` | d? | `timed out waiting for T-1 done` | `crates/horch-e2e/tests/taskledger.rs:239` |
| `a_spawned_worker_records_its_pids_session_and_checkins`, `a_worker_with_no_progress_is_stalled_by_the_ladder_and_stopped_by_its_...`, `stopping_one_of_three_workers_leaves_the_others_running`, `the_whole_flow_on_the_fake_herdr` (a) | d? | `timed out waiting for 1 agent PIDs` (3 and `B's agent PID`) | `crates/horch-e2e/tests/taskledger.rs:239` |
| `a_stalled_push_task_stays_stalled_while_its_old_agent_lives`, `a_stop_of_a_push_worker_ends_its_attempt_and_its_agent_only`, `each_stop_that_fleetd_sends_is_recorded_and_a_second_stop_sends_nothing` | d? | `timed out waiting for 1 agent PIDs` | `crates/horch-e2e/tests/p5_push_stop.rs:99` |
| `push_attempt_agent_asks_a_question`, `push_attempt_agent_killed_while_it_works_ends_by_its_exit`, `push_attempt_agent_sends_its_result`, `push_attempt_agent_that_exits_with_no_result_ends_by_its_exit` | d? | `timed out after 120 s waiting for the agent ran no horch result` | `crates/horch-e2e/tests/p5_push_attempt_process.rs` |
| `rr4_file_request_and_job_events_wake_pass_before_period`, `rr4_tasks_complete_with_all_file_watch_events_dropped`, `rr7_fleetd_killed_mid_run_ends_as_a_clean_run` | d? | `after 60 s: the worktree_add job of fleetd makes <folder>` | `crates/horch-e2e/tests/p1_pass.rs:106` |
| `fm13_herdr_restart_rescues_each_worktree_and_the_next_attempt_reuses_it`, `fm25_killed_worker_with_dirty_and_untracked_files_loses_nothing` | d? | `the worktree_add job of <folder> ...` | `crates/horch-e2e/tests/p0_rescue.rs:146` |
| `fm2_agent_that_never_claims_has_its_task_bound_at_spawn`, `fm6_agent_that_exits_without_done_leaves_its_work_to_the_next_attempt` | d? | `timed out after 120 s waiting for the agent is not bound at entry` | `crates/horch-e2e/tests/p0_dispatch.rs` |
| the 10 tests of `p3_triage.rs` and `archive_restore_and_gc_through_the_cli` (`i308_triage.rs:207`) | d | `fleetd refused the facts of the result file: the facts of the result file have the wrong form: unknown field `no_sandbox`, expected `decisions`` | `crates/horch-e2e/tests/p3_triage.rs:214`, `:258`, `:277`, `:314`, `:348`, `:385`, `:418`, `:435`, `:496`, `:548`; the `no_sandbox` result is the sandbox `PATH` cause |
| 6 tests of `fleet18_brief_worktree.rs`, 4 of `fakes.rs`, 7 of `i308_fleet_steward.rs`, 1 of `worktree.rs`, 5 of `fleet18_observability.rs`, 3 of `startup_error.rs`, 2 of `scenario.rs`, 1 of `routing_provenance.rs`, 11 of `lifecycle.rs` | d? | `timed out after 120 s waiting for the worktree of the task` / `the tree of the bind to leave reserved` / `the launch of <cli>` | `crates/horch-e2e/src/harness.rs:1145` |
| `real_conformance_gate_script_runs_isolated_and_retains_reports` | d | `the conformance suite gave 2 wrong runs: claude: the run is failed` | `crates/horch-e2e/tests/p7_conformance_binary.rs:369`, `:465` |
| `operator_request_from_a_worker_sandbox_is_refused_for_each_operator_kind` | d | ``assertion `left == right` failed: no attack changes the state of the ledger`` | `crates/horch-e2e/tests/p5_operator_boundary.rs:802` |
| `p6_release_run_with_an_open_question_gets_1_idea_1_feature_and_1_owner` | d | ``called `Result::unwrap()` on an `Err` value: InvalidColumnType(0, "id \|\| ' ' \|\| name \|\| ' ' \|\| state", Null)`` | `crates/horch-e2e/tests/p6_release.rs:219` (the query reads a NULL name) |

The rows with `d?` have a timeout as their failure line. `bwrap is not on PATH` is in the log of the job, but the block of each test does not name it. The cause "sandbox `PATH`" is probable for these rows.

## Windows

Job `build-windows` (`cargo build --workspace --locked`): success.

Job `test-windows` (`cargo test --workspace --locked --no-fail-fast`): failure. **0 tests ran**: the build of the tests did not compile. The counts are 0 passed and 0 failed. Cargo stops at a compile error, even with `--no-fail-fast`.

| Test or step | Class | First error line | file:line |
|---|---|---|---|
| `horch-platform` test `sandbox` | c | ``error[E0433]: cannot find `unix` in `os``` (`std::os::unix::net::UnixListener::bind`) | `crates/horch-platform/tests/sandbox.rs:232` (test `hiding_sandbox_cannot_connect_to_a_unix_socket_in_a_hidden_folder`, from line 226) |

Total: 1 compile error. The lib test of `horch-core` now compiles (the 16 errors of CI1 are fixed by T-78). 5 warnings do not fail the job: `HANG_LIMIT_S` never used (`crates/horch-platform/src/service.rs:36`), `CONTROL` never used (`crates/agent-docs-sync/tests/changes.rs:244`), unused import `FORBIDDEN_ENV` (`crates/horch-core/src/fleetd/worker_profiles/adapters/launcher.rs:135`), unused import `std::process::Command` (`crates/horch-core/src/workspace/herdr.rs:870`), unused variable `round` (`crates/horch-core/src/fsx.rs:879`).

More compile errors can show up in other crates after this one is fixed. The crate `horch-platform` stopped the build first ("build failed, waiting for other jobs to finish"). The test targets of the crates `horch`, `horch-e2e` and others built after it are not checked on Windows yet.

## Job `docs`

Success. The 33 rustdoc errors of CI1 are fixed (T-76). The crate `horch-e2e` is documented in this run and gives no error.

## Job `release-lint` (`cargo clippy --workspace --bins --release --locked -- -D warnings`)

Failure. 2 errors, both in the crate `horch` (the bin). The error of CI1 (`chunks_exact`) is fixed (T-77). `horch-core` now passes the lint, so cargo checks the crate `horch` next and finds 2 errors that were behind it. The 2 lines are older than CI1 (commit `3b94858a` of 2026-10-28 and `912d5e96` of 2026-11-18).

| Step | Class | First error line | file:line |
|---|---|---|---|
| `cargo clippy` | d | ``error: unused `std::process::ExitCode` that must be used`` (`-D unused-must-use`): `print(outcome);` | `crates/horch/src/cmd/compact.rs:276` |
| `cargo clippy` | d | ``error: unused `std::process::ExitCode` that must be used``: `super::taskledger::write_completed(ctx, summary, notes)?;` | `crates/horch/src/cmd/messaging.rs:906` |

## Difference to CI1

CI1 = run 44701297167 (head `7fbdc0d3`). CI3 = this run (head `001e73c2`).

### Per job

| Job | CI1 | CI3 | Change |
|---|---|---|---|
| `test (macos-15)` | 4796 passed, 270 failed | 5002 passed, 188 failed | better: 82 fewer failures, 206 more passes |
| `test (ubuntu-latest)` | 4691 passed, 375 failed | 4821 passed, 368 failed | a little better: 7 fewer failures, 130 more passes |
| `docs` | failure, 33 errors | success | **fixed** |
| `release-lint` | failure, 1 error | failure, 2 errors | still red, the first error is fixed and 2 errors behind it show |
| `build-windows` | success | success | no change |
| `test-windows` | failure, 17 compile errors, 0 tests | failure, 1 compile error, 0 tests | still red, 16 errors fixed |

By test name: macOS 116 fixed, 154 still red, 34 new. Linux 90 fixed, 284 still red, 84 new.

### Groups of CI1

| Group of CI1 | State in CI3 |
|---|---|
| macOS and Linux: 116 tests of the retired commands `horch ledger run new`, `horch spawn`, `horch ledger task`, the missing operator token and the run with no idea in flight (class f) | macOS: **partly fixed**. The log of CI3 has 0 lines `horch spawn is retired`, 0 lines `reading the operator token` and 0 lines `needs an idea (FR-12)` (CI1: 39, 21 and 17). 26 of the 116 `run new` tests are fixed. **90 `run new` tests are still red** (`taskledger.rs` 58, `p3_refusals.rs` 10, `p2_leases.rs` 7, `p3_unlanded.rs` 7, `p3_worktrees.rs` 7, `p4_clock_and_heartbeat.rs` 1). Linux: 12 fixed, 89 still red. |
| 38 tests of `horch-core` that build the fakes with `--profile out` | **still red**, now 42: `p7_operator_boundary.rs` has 4 tests more. T-59 changed `crates/horch-e2e/src/harness.rs` only. |
| Linux: 82 tests, the sandbox with no `bwrap` on the `PATH` (27 unit tests, about 55 e2e tests) | **still red**, now 134. 3 tests are fixed. 49 tests are new, because they are tests that landed after CI1 and fail on the same line. |
| `docs`: 33 rustdoc errors | **fixed** (T-76). |
| `release-lint`: 1 clippy error (`chunks_exact`) | **fixed** (T-77). 2 new errors show in the crate `horch` (class d, see the section `release-lint`). They were behind the first error. |
| Windows: `horch-core` lib test, 16 compile errors | **fixed** (T-78). |
| Windows: `horch-platform` test `sandbox`, 1 compile error | **still red**, the same line. |
| Windows: `horch-marketplace` test | stays fixed. |
| `no_status_literal_in_ledger_sql` | **still red** at `triage/mod.rs:201`. |
| The timing tests: `store_dirlock_excludes_concurrent_writers`, `line_over_the_limit_is_refused` | **fixed** (T-96, T-97). `gate_at_its_limit_is_abandoned_and_its_group_killed` is **still red** (`gate_jobs.rs:964`, T-95 is done). |
| `check_sandbox.rs` (4 tests), `codex.rs:2761`, `schema.rs:2279`, `cmd::context`, `cmd::messaging`, `cmd::taskledger`, `p5_cli_writers.rs`, `r_cli_cmds_ledger.rs`, `p4_safety.rs`, `p4_verifier.rs` `fm9`, `p5_operator_boundary.rs` (macOS), `scenario.rs`, `routing_provenance.rs`, `restored_pane.rs`, `tell_grace.rs`, `fleet18_*`, `done_budget.rs`, `fakes.rs` (macOS) | **fixed** on macOS. |
| The 7 rows of class (a) | 2 are left on macOS (`a_fleet_with_an_abandoned_worktree_spawns_the_worktree_steward`, `an_exhausted_claude_pool_starts_no_codex_steward`) and 4 on Linux (those 2, `the_no_change_steward_task_finishes_without_a_worktree`, `the_whole_flow_on_the_fake_herdr`). Each fails with another message than the one on the list. |
| `taskledger.rs` `legacy_plumbing_lives_under_ledger_legacy`, `status_on_a_fresh_ledger_creates_it_and_says_no_runs` | **still red**. |
| The 4 `p3_worktrees_cli.rs` restore tests, `p2_release.rs`, `p3_release.rs`, `p4_clock_and_heartbeat.rs` `pl4`, `p0_liveness.rs` (2) | **still red**. |
| Linux: `p3_triage.rs` and `i308_triage.rs` (the `no_sandbox` result) | **still red**. |
| Linux `p4_repair.rs` hang (3 tests, 320 s each) | **still red** (3 tests, `timed out after 320 s waiting for the commit of the fake in the worktree of A1`). |

### New red tests (red in CI3, not red in CI1)

macOS 34 tests, Linux 84 tests. Of these, 12 (macOS) and 61 (Linux) are tests that did not exist at CI1 (landed by T-8, T-9, T-12, T-17, T-65 and others). Those tests that exist on both OSes pass on macOS or fail on a cause that is listed above. The tests that were green in CI1 and are red now, so a regression of a commit between the 2 heads:

| Test | OS | Cause (probable when marked) | Commit |
|---|---|---|---|
| the 9 tests of `messaging.rs` (`an_old_registry_others_can_write_is_not_copied_and_inbox_says_why` and 8 more) | macOS, Linux | the worker record stays `running`, the pane prints `horch did not trust ... for codex: ... $CODEX_HOME/config.toml does not exist`; the test home has no `.codex` folder (probable) | not found |
| `worker_that_removes_horch_role_is_refused_by_fleetd`, `worker_that_sets_horch_role_orchestrator_is_refused_by_fleetd` | macOS, Linux | the test process is now a fleet process and fleetd refuses the operator request (probable) | T-4 (FR-10 boundary) or T-5 |
| `fitness_no_context_touches_foreign_tables` | macOS, Linux | `answers.rs` of the context `ideas` reads the table `events` | T-44 (`93bbb0d6`) |
| `fleetd_killed_between_commit_and_effect_does_the_effect_once_on_restart`, `launch_is_skipped_when_a_live_process_has_the_attempt_id` | macOS (2), Linux (1 of the 2) | the attempt stays `launching` | not found (T-195 or T-163 probable) |
| `harness_codex_rules_snapshot` | macOS, Linux | the rules of a codex worker differ from the snapshot | not found (T-5 or T-17 probable) |
| `watch_once_reports_in_text_and_json` | macOS, Linux | the watch finds a dead worker and the test expects none | not found |
| `given_3_ready_branches_when_bundle_then_fast_gate_runs_once_and_none_i...` | macOS | 3 `task_check` jobs do not run in 120 s | not found |
| `fr10_b_agent_after_its_attempt_ended_cannot_approve_and_is_stopped`, `fr2_each_sub_orchestrator_attempt_has_1_idea`, `a_worker_that_registers_after_the_wait_still_works`, `cancel_of_a_judge_task_alone_is_refused` and 5 more of `p6_judge_pair.rs`, `fr12_direct_judgment_idea_gets_its_judge`, `sr3_check_that_connects_off_host_is_refused` | macOS | the profile `antigravity` is unhealthy (conformance, T-48) for 8 of them; `fr10_b`, `fr2` and `a_worker_that_registers` are not proved | T-48 |
| `p7_operator_boundary.rs` 4 tests (`codex_operator_boundary_conformance_refuses_unproven_version` and 3 more) | macOS, Linux | `--profile out` | T-65 added them |
| `sandbox::tests::local_network_allows_loopback_and_refuses_an_off_host_address` | Linux | the loopback connect is refused inside the sandbox | T-17 (`a87b4423`) |
| `large_reply_to_non_reading_client_times_out_and_does_not_block_the_server` | Linux | the write of a large reply does not time out | T-97 (`7d27c69e` drains the unread bytes) |
| `p6_release_run_with_an_open_question_gets_1_idea_1_feature_and_1_owner` | Linux | the query reads a NULL name | not found |
| `harness_defaults.rs` `exec_launch_gives_the_skills_briefing`, `p7_context_limit.rs` `two_context_limits_with_no_new_commit_open_1_needs_you_item`, `p7_wrapper_exec.rs` `exec_exit_without_result_runs_the_repair_pass` | Linux | sandbox `PATH` and `--profile out` | T-126 and others |

## Tickets for the failures that are still red

The tickets are `FIX-INDEX-8.md` (101-133) and `FIX-INDEX-9.md` (140-146) in `docs/tickets/fleetd-finish/`, the tickets 147 to 172, and the run board of `horch ledger status --run fleetd-finish` (read after the run ended). A ticket is "done" when its task is done. A failure that is red after its ticket is done has no open ticket: it is written as `no ticket`.

| Failure | Task that covers it |
|---|---|
| `taskledger.rs` `run new` tests (58 macOS, 57 Linux) | T-232 (running, part 2, ticket 117), T-233 (queued, part 3, ticket 118), T-234 (queued, part 4, ticket 119). Part 1 (ticket 116, T-90 cancelled): the work is kept on `keep/fleetd-finish-T90` and T-232 lands it. T-232 waits for T-236 (done) for the cancel test. |
| `p3_refusals.rs`, `p2_leases.rs`, `p3_unlanded.rs`, `p3_worktrees.rs` `run new` tests, `p2_release.rs`, `p3_release.rs` | T-89 (landing, ticket 115, in `horch ledger finish`) |
| `p4_clock_and_heartbeat.rs` (`pl4_...`, `slow_worktree_add_...`), `p4_release.rs` `run_without_gates_holds_dispatch_and_loses_no_task` | no ticket (FIX-16 of CI1 was never made a ticket) |
| 42 `--profile out` tests of `horch-core` | no ticket (FIX-11 of CI1; T-125, ticket 146, is done and the 4 files are red) |
| Linux sandbox `PATH` of the unit tests (27 tests, `launch.rs:153`) | no ticket (FIX-12 of CI1; T-193 is done and fixed only the `git_env` test) |
| Linux sandbox `PATH` of the e2e harness (about 107 tests, `harness.rs:375`) | no ticket (FIX-13 of CI1) |
| Windows `horch-platform` `sandbox.rs:232` | no ticket (FIX-14 of CI1) |
| `no_status_literal_in_ledger_sql` | no ticket (FIX-15 of CI1) |
| `release-lint`: the 2 `ExitCode` errors in `horch` | no ticket (T-77 is done and did not cover them) |
| 9 tests of `messaging.rs` | no ticket (T-121, ticket 142, is done; the tests were green in CI1) |
| `p5_route_ledger_task.rs` (2) | no ticket (T-84, ticket 110, is done) |
| `fitness_no_context_touches_foreign_tables` | no ticket |
| `fleetd_pass.rs` (2) `launching` | no ticket. T-172 (running, `pass.rs`, ticket 157) and T-195 (done) change the `launching` step. |
| `harness_codex_rules_snapshot` | no ticket |
| `watch_once_reports_in_text_and_json` | no ticket (T-85, ticket 111, is done) |
| The 11 tests of the unhealthy profile `antigravity` (macOS) and the same cause on Linux | no ticket |
| `given_3_ready_branches_...` (`p0_landing.rs`), `fr10_b_...` (`p6_fr10_approval.rs`), `fr2_each_sub_orchestrator_attempt_has_1_idea`, `a_worker_that_registers_after_the_wait_still_works` | no ticket |
| `i308_fleet_steward.rs`, the 2 steward tests of class (a), `triage_*` (5) | T-94 (queued, ticket 120) for the list; the other tests: no ticket (T-87, ticket 113, is done) |
| `p0_liveness.rs` (2), `p0_rescue.rs`, `p0_dispatch.rs` | no ticket (T-88, ticket 114, is done) |
| `fleet18_observability.rs` `g50_...` and `g52_...`, the 6 tests of `fleet18_brief_worktree.rs` (Linux) | no ticket (T-15 is done, T-86 is done) |
| `p3_worktrees_cli.rs` (4 restore tests) | no ticket (T-83, ticket 109, is done) |
| `skills_exposure.rs` (3 macOS, 4 Linux) | no ticket (T-107, ticket 133, is done) |
| `taskledger.rs` `legacy_plumbing_lives_under_ledger_legacy`, `status_on_a_fresh_ledger_creates_it_and_says_no_runs`, `the_import_finds_work_that_never_landed` | T-232, T-233 and T-234 own the file; none names these 3 tests. T-207 (queued) and T-208 (queued) cover the worktree states, not these tests. |
| Linux `p3_triage.rs`, `i308_triage.rs` (the `no_sandbox` result) | no ticket. The cause is the sandbox `PATH` (FIX-13). |
| Linux `p4_repair.rs` (3 tests), `p4_no_loss.rs`, `p4_remote.rs`, `p4_rescue_refs.rs`, `p6_*.rs`, `p7_exec.rs`, `p7_docs_sync.rs` | no ticket. The cause is the sandbox `PATH` (FIX-13). |
| Linux `p5_push_stop.rs` (3), `p5_push_attempt_process.rs` (4), `p1_pass.rs` (3), `p0_dispatch.rs` (2) | no ticket (T-200 and T-71 are done). Probable cause: the sandbox `PATH`. |
| Linux `gate_jobs.rs:964` | no ticket (T-95 is done) |
| Linux `channel.rs:252` | no ticket (T-97 is done) |
| Linux `sandbox.rs:374` (`local_network...`) | no ticket |
| Linux `p5_operator_boundary.rs:802`, `p7_conformance_binary.rs`, `p6_release.rs:219` | no ticket |
| `T-64`, `T-72` (queued, wait for T-172) | they cover the `pass.rs` work, no test of this run. |

### `no ticket` rows of CI1 and their state now

| Row of CI1 | State now |
|---|---|
| FIX-11 (profile of the fakes in `horch-core` tests) | **no ticket, still red** (42 tests) |
| FIX-12 (sandbox `PATH` of the unit tests) | **no ticket, still red** (27 tests) |
| FIX-13 (sandbox `PATH` of the e2e harness) | **no ticket, still red** (about 107 tests, 61 of them new tests) |
| FIX-14 (Windows `sandbox.rs`) | **no ticket, still red** (1 compile error) |
| FIX-15 (status words in the triage SQL) | **no ticket, still red** |
| FIX-16 (`p4_release.rs` and `p4_clock_and_heartbeat.rs` with 2 active runs) | **no ticket, still red**: `pl4_...` fails the same way, and `run_without_gates_holds_dispatch_and_loses_no_task` now fails because the profile `antigravity` is unhealthy |
| `an_agent_launch_drops_the_git_repository_variables` (`launch.rs:3134`, Linux) | **fixed** (T-193 gave the test a fake `bwrap` and `socat`) |
| Linux `p4_repair` hang, 3 tests of 320 s each | **no ticket, still red** (3 tests, `p4_repair.rs:168`). The cause is probably the sandbox `PATH` (FIX-13). The job took 118 min. |
| The 4 timing tests of FIX-10 (tickets 121, 122, 123) | 2 **fixed** (`store_dirlock...`, `line_over_the_limit...`), `gate_at_its_limit...` **still red**, and the 4th (`fleetd_pass.rs`, T-98, ticket 124) is now red with another message (`launching`, see the new red tests) |

## Fix units

Only failures with no ticket. The units FIX-11 to FIX-16 are the units of CI1 that were never made tickets. FIX-17 to FIX-22 are new. Each unit is for 1 worker.

| Unit | Fixes | Files it touches |
|---|---|---|
| FIX-13: the sandbox `PATH` of the e2e harness | about 107 e2e tests on Linux (the `bwrap` group, the `no_sandbox` result of `p3_triage.rs`, the timeouts that wait for a job) | `crates/horch-e2e/src/harness.rs` (`seal`, line 375: add the folder of `bwrap` and `socat` after the folder of the fakes, Linux only). Then run each test of `p3_triage.rs`, `p4_repair.rs`, `p4_no_loss.rs`, `p6_features.rs` and `p7_exec.rs` 3 times (rule 6). Do this first: it hides the real cause of about 50 more tests. |
| FIX-12: the sandbox `PATH` of the unit tests | the 27 `horch-core` tests of the sandbox group on Linux | `crates/horch-core/src/harness/launch.rs` (`LaunchEnv::for_test`, line 153: give `path` a folder with fake `bwrap` and `socat`, as `claude.rs:1665` does), and the tests `crates/horch-core/tests/baseline_oracles.rs`, `context_launch.rs`, `execution_plan.rs`, `harness_defaults.rs`, `p7_context_256k.rs`, `p7_conformance_job.rs` |
| FIX-11: the profile of the fakes in `horch-core` tests | the 42 tests of class d on macOS and Linux | `crates/horch-core/tests/p7_conformance.rs`, `p7_context_limit.rs`, `p7_operator_boundary.rs`, `p7_wrapper_exec.rs` (the `match dir.file_name()` block: use the profile folder as `cargo_profile` of T-59 does in `crates/horch-e2e/src/harness.rs`, or share 1 helper in `crates/horch-e2e`) |
| FIX-14: Windows `horch-platform` test | 1 compile error | `crates/horch-platform/tests/sandbox.rs` (line 226: gate the test `hiding_sandbox_cannot_connect_to_a_unix_socket_in_a_hidden_folder` with `#[cfg(unix)]`). After it, run the CI on Windows once to see the test targets of the other crates. |
| FIX-17: the 2 `ExitCode` clippy errors | `release-lint` | `crates/horch/src/cmd/compact.rs:276` (`print(outcome);` returns an `ExitCode`), `crates/horch/src/cmd/messaging.rs:906` (`write_completed(...)?;` returns an `ExitCode`). Run `cargo clippy --workspace --bins --release --locked -- -D warnings` after it: more errors can be behind the next crate. |
| FIX-15: status words in the triage SQL | `no_status_literal_in_ledger_sql` | `crates/horch-core/src/taskledger/triage/mod.rs:201` (the literals `'landed'` and `'abandoned'`: build them from the enums of `taskledger/model.rs`) |
| FIX-18: the unhealthy profile `antigravity` in e2e tests | 11 tests on macOS (`lifecycle.rs` 2, `p4_release.rs` 1, `p4_verifier.rs` 1, `p6_intake.rs` 1, `p6_judge_pair.rs` 6) and the same cause in tests of Linux | `crates/horch-e2e/src/harness.rs` (the boot conformance of a test fleet: pass `conformance.json` for the fake `agy`, or route the tests to a healthy profile), and the 5 test files. Check the `AGY_FAILS` of `crates/horch-e2e/tests/p7_conformance_binary.rs:338` first. |
| FIX-19: the codex home of the `messaging.rs` tests | 9 tests on both OSes | `crates/horch-e2e/tests/messaging.rs` (make `home/.codex/config.toml` before the worker starts, or find the commit that made codex trust a precondition), `crates/horch-e2e/src/harness.rs` |
| FIX-20: fleet process refusal in `p5_route_ledger_task.rs` | 2 tests on both OSes | `crates/horch/tests/p5_route_ledger_task.rs` (lines 371 and 386: the test shell must not be a fleet process, so clear the attempt env of the test) |
| FIX-21: the 4 small red tests of the core | `fitness_no_context_touches_foreign_tables` (`crates/horch-core/src/fleetd/ideas/application/answers.rs:380` and `:660` read the table `events` of the context `events`: use the reader of the events context), `harness_codex_rules_snapshot` (`crates/horch-core/tests/harness.rs:25`), `watch_once_reports_in_text_and_json` (`crates/horch/tests/w3_ledger_cli_pins.rs:434`), `fleetd_pass.rs:511` and `:670` (`launching`) | the 4 files named in the column Fixes. Run after T-172 (it holds `pass.rs`). |
| FIX-22: the Linux sandbox test and the reply timeout | `sandbox::tests::local_network_allows_loopback_and_refuses_an_off_host_address`, `large_reply_to_non_reading_client_times_out_and_does_not_block_the_server`, `gate_at_its_limit_is_abandoned_and_its_group_killed`, `p6_release_run_with_an_open_question_...` (`p6_release.rs:219`), `operator_request_from_a_worker_sandbox_is_refused_for_each_operator_kind` (`p5_operator_boundary.rs:802`) | `crates/horch-platform/src/sandbox.rs:374`, `crates/horch-core/tests/channel.rs:252`, `crates/horch-core/tests/gate_jobs.rs:964`, `crates/horch-e2e/tests/p6_release.rs:219`, `crates/horch-e2e/tests/p5_operator_boundary.rs:802`. Run after FIX-13: the last 2 can pass then. |
| FIX-16: 2 e2e tests of `p4_*` | `pl4_sleep_past_a_branch_deadline_fires_nothing_on_wake`, `slow_worktree_add_of_60_s_keeps_the_heartbeat_and_the_tree_i...`, `run_without_gates_holds_dispatch_and_loses_no_task` | `crates/horch-e2e/tests/p4_release.rs`, `crates/horch-e2e/tests/p4_clock_and_heartbeat.rs` (replace `horch ledger run new` and `horch ledger watch` by the push flow; pass `--run <slug>` when 2 runs are active). Run after FIX-18 for `run_without_gates...`. |

After FIX-11 to FIX-22, T-89, T-232 to T-234 and T-94: run the CI 1 time more. Do not run it before. A run costs about 2 h.
