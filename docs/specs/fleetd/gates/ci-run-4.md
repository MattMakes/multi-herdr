# CI run 4 of fleetd, 2026-11-21

- Run: 44861461538, https://github.com/MattMakes/multi-herdr/actions/runs/44861461538
- Trigger: `workflow_dispatch` on `fleet/fleetd-finish`, started 2026-11-21 03:30Z. The orchestrator cancelled the run at 06:31Z (T-310 task note, `gh run cancel`), because `test-windows` had run for 3 h with no output.
- Head commit of the run: `2c6ef0b41acecb742357047f1087310d9eca768f` (T-302 comment sweep). The tip of `origin/fleet/fleetd-finish` at the start, and the local branch, were the same commit. The check "headSha equals the tip" holds.
- Toolchain: rustc 1.100.0 (0ef1bc386 2026-11-09), the same as CI3.
- Compared with: CI3, `docs/specs/fleetd/gates/ci-run-3.md` (run 44784709135, head `001e73c2`). About 340 commits landed between the 2 heads.
- Later tip: when I wrote this report, `origin/fleet/fleetd-finish` was `4e806d0b`, 40 commits after the head of the run. Each failure below says whether one of them fixed it.
- Source of the counts: the sums of the lines `test result:` of each job log, and the blocks `---- <test> stdout ----` of the failed tests. I ran no test locally (ticket rule). Logs read: `test (macos-15)`, `test (ubuntu-latest)`, `docs`, `release-lint`, `test-windows` (the log of the cancelled job).
- Run result: cancelled (the workflow state), with 3 failed jobs and 1 cancelled job. 2 of 6 jobs passed (CI3: 2 of 6).

| Job | OS | Result | Time |
|---|---|---|---|
| `test (macos-15)` | macOS | failure: 5186 passed, 23 failed, 30 ignored | 93 min 12 s |
| `test (ubuntu-latest)` | Linux | failure: 5157 passed, 52 failed, 30 ignored | 74 min 5 s |
| `docs` | Linux | **failure** (new): 1 rustdoc error | 23 s |
| `release-lint` | Linux | **success** | 1 min 47 s |
| `build-windows` | Windows | success | 2 min 2 s |
| `test-windows` | Windows | **cancelled after 3 h 0 min 42 s** (the job has `continue-on-error: true`): 2666 passed, 813 failed, 7 ignored in 280 test binaries, then a hang in `fleet18_messaging.rs` | cancelled 06:31:08Z |

## Classes

- (a) a test that is on `crates/horch-e2e/expected-failures/`
- (b) a compile error on that OS
- (c) a test that is unix-only by its code but runs on Windows
- (d) a real fault of the product (or of the test harness) on that OS
- (e) a fault of the CI setup
- (f) the test is stale: it calls a command that fleetd retired, or it lacks a setup that the product needs now (the class of CI0 to CI3)

A cause that the log does not name in the failure line is marked "probable".

## Jobs of `ci.yml`

| Job | OS |
|---|---|
| `test` (matrix) | `ubuntu-latest`, `macos-15` |
| `docs` | `ubuntu-latest` |
| `release-lint` | `ubuntu-latest` |
| `build-windows` | `windows-latest` |
| `test-windows` | `windows-latest`, `continue-on-error: true` |

The file has no `timeout-minutes` and no per-test timeout. A hung test runs until the 6 h limit of GitHub. I did not edit `ci.yml` (ticket rule). See the class (e) row in the section `## Windows`.

## macOS

Job `test (macos-15)`: failure. 5186 tests passed, 23 failed, 30 ignored. In CI3: 5002 passed, 188 failed.

| Test | Class | First error line | file:line of the cause |
|---|---|---|---|
| `g50_legacy_orchestrator_and_host_load_are_advisory` | d? | ``assertion `left == right` failed: horch: NOTE: the health guard free_disk does not hold dispatch: the probe has no reading`` | `crates/horch-e2e/tests/fleet18_observability.rs:806` |
| `a_fleet_with_an_abandoned_worktree_spawns_the_worktree_steward` | a | on the list with another reason: `horch ledger cancel T-1 not needed now: status: exit status: 1` | `crates/horch-e2e/tests/i308_fleet_steward.rs:41` |
| `triage_lists_rows_from_a_ledger_it_cannot_write` | f | `horch ledger cancel T-1 not needed now: status: exit status: 1` | `crates/horch-e2e/tests/i308_fleet_steward.rs:41` |
| `a_reaped_worker_that_ends_on_the_polite_stop_has_its_pane_closed` | d | ``assertion `left == right` failed`` left `["request_file_rejected:/tmp/horch-e2e-p0-polite-pa-.../fleet/run/requests/horch-probe-1595.json"]`, right `[]` | `crates/horch-e2e/tests/p0_liveness.rs:594` (the fake agent leaves a probe request file; T-268 fixed the same cause in `p1_orphans` only) |
| `fm3_idle_claim_is_caught_in_under_10_min` | f | `horch: 2 active runs in .../work/alpha: F-0002, eps. Name one with --run <slug>` | `crates/horch-e2e/tests/p0_liveness.rs:63` |
| `rr10_hand_made_worktree_is_adopted_within_1_list_period` | d? | ``assertion `left == right` failed: 70`` | `crates/horch-e2e/tests/p3_worktrees.rs:545` |
| `restore_of_a_task_makes_branch_rescue_t_seq_n_and_its_tree_with_each_rescued_file`, `restore_of_a_task_whose_tree_is_removed_uses_its_rescue_ref`, `restore_refuses_when_the_branch_or_the_folder_exists_and_changes_nothing`, `restore_runs_no_hook_of_the_project` (4) | d? | `timed out waiting for fleetd to rescue the tree of T1` | `crates/horch/tests/p3_worktrees_cli.rs:140` |
| `pl4_sleep_past_a_branch_deadline_fires_nothing_on_wake` | f | `horch ledger watch: status: exit status: 1` | `crates/horch-e2e/tests/p4_clock_and_heartbeat.rs:246` |
| `run_without_gates_holds_dispatch_and_loses_no_task` | f | `horch: 2 active runs in .../work/alpha: F-0002, zeta. Name one with --run <slug>` | `crates/horch-e2e/tests/p4_release.rs:1116` |
| `operator_skills_e2e_changed_since_spawn_fails_the_launch`, `operator_skills_e2e_deleted_since_spawn_fails_the_launch`, `plugin_skills_e2e_changed_since_spawn_fails_the_launch` (3) | f? | `the agent ran: stdout: stderr: horch: the skills changed since <attempt> was spawned: 'test-mo...` | `crates/horch-e2e/tests/skills_exposure.rs:664`, `:678`, `:777` |
| `skl_06_e2e_exposure_antigravity` | d? | ``assertion failed: agent_calls(&h, "antigravity").is_empty()`` | `crates/horch-e2e/tests/skills_exposure.rs:257` |
| `a_critical_finding_fails_until_a_fix_is_reviewed_again` | d? | `timed out waiting for the reviews of a round` | `crates/horch-e2e/tests/taskledger.rs:293` (the wait helper) |
| `a_push_review_task_ends_done_by_its_findings_file_and_its_task_lands` | d? | `timed out waiting for T-1 done` | `crates/horch-e2e/tests/taskledger.rs:293` |
| `a_live_worker_that_does_not_claim_is_reported_once` | f | `horch: 2 active runs in .../work/alpha: F-0002, a. Name one with --run <slug>` | `crates/horch-e2e/tests/taskledger.rs:2831` |
| `a_parked_unlanded_task_names_its_dependents_and_a_stuck_run_is_reported_once` | f? | `EOF while parsing a value at line 1 column 0: status: exit status: 1` | `crates/horch-e2e/tests/taskledger.rs:259` |
| `a_task_whose_attempts_all_fail_is_failed_and_its_replan_requeues_it` | d? | the assert has no message (the block is empty) | `crates/horch-e2e/tests/taskledger.rs:2926` |
| `the_watchdog_ends_pre_claim_rows_whose_process_is_gone` | f | `horch: 3 active runs in .../work/alpha: F-0003, F-0002, a. Name one with --run <slug>` | `crates/horch-e2e/tests/taskledger.rs:2794` |
| `the_whole_flow_on_the_fake_herdr` | a | `timed out waiting for A's worktree`, then the panic of the list check `crates/horch-e2e/src/expected.rs:108` | `crates/horch-e2e/tests/taskledger.rs:293` |

Total: 1 + 1 + 1 + 1 + 1 + 1 + 4 + 1 + 1 + 3 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 = 23.

The "2 active runs" tests (class f, 4 on macOS) fail because the project of the test has a second active run, named like a feature (`F-0002`). `horch ledger watch` and `horch ledger status` then need `--run <slug>`. The log does not say which command makes the second run. T-251 (CI FIX-16) and T-252 (CI FIX-23) are done and these tests are red at the head of the run.

## Linux

Job `test (ubuntu-latest)`: failure. 5157 tests passed, 52 failed, 30 ignored. In CI3: 4821 passed, 368 failed. The log has 1211 lines `bwrap is not on PATH`, all of them in the 10 tests of the group "pass helper with an empty PATH" below.

### Groups

| Group | Count | Class | First error line | file:line of the cause |
|---|---|---|---|---|
| The pass helper of the test sets `PATH` to an empty folder, so fleetd finds no `bwrap` | 10 | d (test harness) | `no sandbox: sandbox is not supported on linux: bwrap is not on PATH; install bubblewrap; fleetd runs git in a project only in the sandbox, so the job ran no git` (the jobs list of the failure text, 121 lines for each test) | `crates/horch-e2e/tests/p4_no_loss.rs:377` (`cmd.env("PATH", &no_clis)`), `crates/horch-e2e/tests/p6_features.rs:507` (the same line). T-240 (CI FIX-13) put `bwrap` and `socat` on the sealed `PATH` of the harness (`crates/horch-e2e/src/harness.rs:173-198`), and these 2 helpers replace that `PATH`. |
| Tests that fail on macOS too | 15 | see the next paragraph | | |
| Tests that fail on Linux only | 27 | see the table below | | |

The 10 tests of the first group: `i8_squash_merge_with_a_conflict_resolution_is_seen_as_landed`, `i9_gate_result_on_a_stale_tip_moves_no_ref`, `i10_crash_before_and_after_the_rescue_of_a_forced_update_loses_no_tip`, `i11_orphan_fleet_branch_is_adopted_and_not_deleted`, `i12_stash_entry_blocks_the_deletion_of_its_branch` (`p4_no_loss.rs`, each: `not after 30 passes: <what>`), `fm39_cancelled_task_does_not_block_its_idea`, `fm41_merge_with_an_idea_in_flight_moves_it_to_a_new_feature`, `fm42_lower_feature_abandoned_rebases_the_upper`, `fm42_lower_feature_merged_rebases_the_upper`, `several_features_run_at_the_same_time` (`p6_features.rs`).

Total: 10 + 15 + 27 = 52.

### Tests that fail on macOS too (15), with the same cause

`g50_legacy_orchestrator_and_host_load_are_advisory` (Linux message: `timed out waiting for the launch of the worker`, `fleet18_observability.rs:385`), `a_fleet_with_an_abandoned_worktree_spawns_the_worktree_steward` (class a, `i308_fleet_steward.rs:41`), `triage_lists_rows_from_a_ledger_it_cannot_write` (`i308_fleet_steward.rs:41`), `rr10_hand_made_worktree_is_adopted_within_1_list_period` (`p3_worktrees.rs:545`), `pl4_sleep_past_a_branch_deadline_fires_nothing_on_wake` (`p4_clock_and_heartbeat.rs:246`), `run_without_gates_holds_dispatch_and_loses_no_task` (`p4_release.rs:1116`), `fm3_idle_claim_is_caught_in_under_10_min` (`p0_liveness.rs:63`), 4 tests of `skills_exposure.rs` (`:664`, `:678`, `:777`, `:257`), and 4 tests of `taskledger.rs` (`a_live_worker_that_does_not_claim_is_reported_once`, `a_parked_unlanded_task_names_its_dependents_and_a_stuck_run_is_reported_once`, `a_task_whose_attempts_all_fail_is_failed_and_its_replan_requeues_it`, `the_watchdog_ends_pre_claim_rows_whose_process_is_gone`). The table of the section `## macOS` gives the first error line of each. The 4 `skills_exposure.rs` tests and `rr10_...` fail with the same line on both OSes.

### Tests that fail on Linux only

| Test | Class | First error line | file:line of the cause | State after the run |
|---|---|---|---|---|
| `sandbox::tests::local_network_allows_loopback_and_refuses_an_off_host_address` | d (test) | ``assertion `left == right` failed left: "refused ConnectionRefused Some(111)"`` | `crates/horch-platform/src/sandbox.rs:374` | **fixed after this run by `b29ac09b`** (T-309: the test connects to a loopback listener inside the sandbox) |
| `large_reply_to_non_reading_client_times_out_and_does_not_block_the_server` | d (test) | ``called `Result::unwrap_err()` on an `Ok` value: ()`` | `crates/horch-platform/tests/channel.rs:252` | **fixed after this run by `b29ac09b`** (`LARGE_REPLY` is 1 MiB) |
| `gate_at_its_limit_is_abandoned_and_its_group_killed` | d | `the gate group was killed` | `crates/horch-core/tests/gate_jobs.rs:964` | **fixed after this run by `b29ac09b`** (new PID namespace; a KILL of the group ends the sandbox) |
| `a_check_past_its_timeout_is_killed_with_its_children` | d | `timed out waiting for the process in sleep.pid to be gone` | `crates/horch-e2e/tests/taskledger.rs:293` | probably fixed after this run by `b29ac09b` (it gives a PID namespace to check jobs, `sandboxed_local`); not proved |
| `sandbox_write_outside_is_refused` | d (test) | ``assertion `left == right` failed: CheckResult { check: SandboxBoundary, verdict: Fail, evidence: "the outside write: allowed" }`` | `crates/horch-core/tests/p7_conformance.rs:349` | **fixed after this run by `0061d502`** (T-320: the machine is in `/var/tmp` on Linux) |
| `operator_boundary_fails_when_the_rules_allow_the_channel`, `operator_boundary_fails_when_the_rules_allow_the_token_read`, `codex_operator_boundary_conformance_refuses_unproven_version_and_probes_sandbox_acts` | d | `CheckResult { check: OperatorBoundary, verdict: Pass, ... "refused: read t..."` (the probe counted ENOENT as a refusal) | `crates/horch-core/tests/p7_operator_boundary.rs:376`, `:385`, `:518` | **fixed after this run by `c6dede2f`** (T-320) |
| `given_3_ready_branches_when_bundle_then_fast_gate_runs_once_and_none_is_forgotten` | d | `timed out after 120 s waiting for 3 running task_check jobs` | `crates/horch-e2e/tests/p0_landing.rs` (wait at `harness.rs:1288`) | probably fixed after this run by `d8a08e35` (T-254, CI FIX-25 names this test); not proved on Linux |
| `real_conformance_gate_script_runs_isolated_and_retains_reports` | d? | `status: exit status: 1 stdout: fleet root: /tmp/p7rc.bGtXwo/fleet` | `crates/horch-e2e/tests/p7_conformance_binary.rs:465` | not fixed. The fixture is under `/tmp`, and the Linux sandbox mounts a new tmpfs on `/tmp` (the cause that `0061d502` and `c6dede2f` give for their tests): probable cause |
| `operator_request_from_a_worker_sandbox_is_refused_for_each_operator_kind` | d? | ``assertion `left == right` failed: no attack changes the state of the ledger`` | `crates/horch-e2e/tests/p5_operator_boundary.rs:802` | not fixed. The commit message of `b29ac09b` says the test is green on `2c6ef0b4` on Linux, and this run is red on the same commit: that is a flake unless the tmpfs cause (row above) applies. Rule 6 of the run applies |
| `sr3_check_that_connects_off_host_is_refused` | d (test) | `the check did not reach loopback, so its refusal proves nothing` | `crates/horch-e2e/tests/p4_verifier.rs:629` | not fixed. On Linux `Network::Local` is a new network namespace with its own loopback (`b29ac09b` message), so a listener of the test on the host loopback refuses: probable cause |
| `verifier_check_sends_no_message_outside_the_fleet` | d (test) | `stderr: curl: (7) Failed to connect to 127.0.0.1 port 43619 after 1 ms` | `crates/horch-core/tests/sr3_no_agent_push.rs:461` | not fixed. The same cause as the row above (probable) |
| `a_codex_workers_check_cannot_write_outside_its_worktree` | d? | ``assertion `left == right` failed: codex-sol: JobRow { id: 11, kind: TaskCheck, ...`` | `crates/horch-e2e/tests/taskledger.rs:1942` | not fixed, cause not proved |
| `phase4::ask_scenario_sends_one_question`, `phase4::bound_scenario_writes_entry_json_at_entry`, `phase4::noresult_scenario_writes_nothing`, `phase4::result_scenario_writes_result_json` (4) | d? | `timed out after 120 s waiting for the worktree of the bound task` | `crates/horch-e2e/src/harness.rs:1288` (test `crates/horch-e2e/tests/fakes.rs`) | not fixed, cause not proved. The block does not name `bwrap` |
| `fm2_agent_that_never_claims_has_its_task_bound_at_spawn` | d? | `the task is not bound to worker opus-1: status: exit status: 1` | `crates/horch-e2e/tests/p0_dispatch.rs:260` | not fixed, cause not proved |
| `fr2_each_sub_orchestrator_attempt_has_1_idea` | d? | ``assertion `left == right` failed: the launch names 0 ideas`` | `crates/horch-e2e/tests/p6_sub_orchestrators.rs:353` | not fixed (it is red in CI3 on macOS too; green on macOS now) |
| `sleep_does_not_expire_a_lease` | f | `horch ledger watch: status: exit status: 1` (the same cause as `fm3_...`: probably 2 active runs; the line is the same as `fm3_...`) | `crates/horch-e2e/tests/p0_liveness.rs:63` | not fixed |
| `an_exhausted_claude_pool_starts_no_codex_steward`, `the_no_change_steward_task_finishes_without_a_worktree` | a | on the list with another reason: `horch ledger cancel T-1 not needed now: status: exit status: 1` | `crates/horch-e2e/tests/i308_fleet_steward.rs:41` | not fixed. T-94 (queued) covers the list |
| `triage_delete_accepts_a_clean_detached_head_reachable_from_a_branch`, `triage_delete_refuses_a_detached_unique_commit` | f | `horch ledger cancel T-1 not needed now: status: exit status: 1` | `crates/horch-e2e/tests/i308_fleet_steward.rs:41` | not fixed |
| `g54_sandbox_warning_matches_launch_on_unsupported_host` | d? | `warning: delivery test failed: herdr did not answer inside the push budget` | `crates/horch-e2e/tests/fleet18_observability.rs:1106` | not fixed, cause not proved |
| `cmd::taskledger_watch::tests::watch_once_runs_the_phase4_pass` | d? | ``called `Result::unwrap()` on an `Err` value: QueryReturnedNoRows``, after `starting job 1: the launch failed: No such file or directory (os error 2)` | `crates/horch/src/cmd/taskledger_watch.rs:527` | not fixed, cause not proved |

The rows of this table hold 27 tests: 22 in the first 17 rows, plus `sleep_does_not_expire_a_lease`, the 2 class (a) tests and the 2 `triage_delete_*` tests.

## Windows

Job `build-windows` (`cargo build --workspace --locked`): success.

Job `test-windows` (`cargo test --workspace --locked --no-fail-fast`): **cancelled after 3 h 0 min 42 s, `hang, cancelled after 3 h`**. The compile error of CI3 (`horch-platform` test `sandbox`) is fixed, so the tests run on Windows now for the first time: 2666 passed, 813 failed, 7 ignored in 280 test binaries (the same workspace has 384 binaries on macOS and Linux, so about 104 binaries did not run). The log has 814 lines `... FAILED`; 2 of them are in the binary that hangs.

### The hang (class d, "hang, cancelled after 3 h"; class e for the missing timeout)

- Step in progress at the cancel: `Run cargo test --workspace --locked --no-fail-fast`, started 2026-11-21 03:30:46Z. The steps `Post Run Swatinem/rust-cache` and `Post Run actions/checkout` were pending.
- Binary: `crates/horch-e2e/tests/fleet18_messaging.rs` (20 tests), started 03:51:25Z. The last log line of a test is 03:56:30Z. No line follows until the cancel at 06:31:04Z (2 h 34 min).
- Tests that started and did not end (the lines `has been running for over 60 seconds`):

| Test | Line at | Class |
|---|---|---|
| `g60_missing_registration_still_queues_worker_mail` | 03:52:25Z | d, hang, cancelled after 3 h |
| `g60_optional_question_projection_error_does_not_abort_courier_startup` | 03:52:25Z | d, hang, cancelled after 3 h |
| `g62_courier_notifies_once_and_resumes_after_login` | 03:52:32Z | d, hang, cancelled after 3 h |
| `g62_login_screen_is_detected_before_typing` | 03:56:30Z | d, hang, cancelled after 3 h |

- In the same binary: 2 tests failed (`g60_plain_done_and_tell_without_registration_arrive_after_late_registration` after 7 s, `a_pass_with_an_unknown_orchestrator_ends_within_the_push_budget` after 3 min 5 s), 6 passed, and 8 tests printed no result. The courier tests wait for a herdr pane and a typed message; the cause of the hang is not proved ("probable": the herdr fake or the courier loop has no Windows path; the harness has no wait limit that ends a test).
- Class (e): `ci.yml` has no `timeout-minutes` for `test-windows`, and the test harness has no limit for a test that hangs. A hang gives a run of 6 h. I did not edit `ci.yml`.
- Tests after `fleet18_messaging.rs` in the order of cargo (about 104 binaries) have no result in this run.

### Groups of the 814 failed lines (counts by the full text of each failure block)

| Group | Count | Class | First error line | file:line of the cause |
|---|---|---|---|---|
| The compiled-in roster base is rejected: a CRLF checkout | 369 | d | ``called `Result::unwrap()` on an `Err` value: compiled-in base 'channel' Caused by: file does not start with a '---' frontmatter fence`` | `crates/horch-core/src/roster/parser.rs:31` (`strip_prefix("---\n")`) and `:33` (`find("\n---\n")`). The repository has no `.gitattributes`, and `actions/checkout` on Windows checks out with CRLF (probable; `crates/agent-docs-sync/tests/changes.rs` shows `"# Guide\r\n"`). First user: `crates/horch/src/cmd/context.rs:972`. Files: 141 in the lib test of `horch-core`, 59 in the bin test of `horch`, `context_roster.rs` 27, `execution_plan.rs` 17, `golden_prompts.rs` 16, `p7_profile_complete.rs` 16 |
| A verbatim path `\\?\C:\...` goes to `git` | 163 | d | `git ["worktree", "add", "-q", "--detach", "\\\\?\\C:\\Users\\runneradmin\\...\\gate-F-0001", ...]: fatal: could not create leading directories of '//?/C:/.../.git': Invalid argument`; for the tests of the launch: `worktree \\?\C:\...\r1-T1-after is not made yet: the worktree_add job 1 is running` | The test folders come from `canonicalize()` (`crates/horch-core/tests/bisect.rs:96`, `crates/horch-core/src/fleetd/task_execution/application/launch.rs:485`), which gives a verbatim path on Windows. Whether the product code passes such a path to `git` is not checked. Files: `w3_finish_steps.rs` 38, lib test 34, `bundles.rs` 14, `i308_triage_apply.rs` 12, `worktree_jobs.rs` 8, `worktree_create.rs` 7 |
| `git` runs only in a sandbox, and Windows has none | 83 | d | `no sandbox: sandbox is not supported on windows: Windows has no Seatbelt and no bubblewrap` (`running git config --list --show-origin --show-scope -z in ...`) | `crates/horch-core/src/fleetd/integration/application/commands.rs:407` (test `git_ref_tips_reads_a_project_below_home`); the sandbox is `crates/horch-platform/src/windows/sandbox.rs`. Files: `integration_git.rs` 13, `idea_direct.rs` 12, `ref_move.rs` 9, `branch_abandon.rs` 6, `merged_with_ideas.rs` 6, `new_features.rs` 6 |
| `/tmp` is hard-coded | 37 | c | ``called `Result::unwrap()` on an `Err` value: Custom { kind: NotFound, error: PathError { path: "D:/tmp\\rl7q3ozg", ... "The system cannot find the path specified." } }`` | `crates/horch/tests/r_cli_cmds_ledger.rs:19` (`tempdir_in("/tmp")`); the same use in `p7_conformance.rs` 20, `context_prime.rs` 8, `p5_claude_sandbox.rs` 4, `p7_claude_exec.rs` 3 |
| `node` gets a `C:\` path as a module | 8 | c | `node failed: node:internal/modules/esm/load:203 throw new ERR_UNSUPPORTED_ESM_URL_SCHEME` | `crates/horch-core/tests/pi_compact.rs` (8 tests) |
| Other | 154 | d? | see the next table | |

Total of the groups: 369 + 163 + 83 + 37 + 8 + 154 = 814.

The 154 other failures, by file (first error line of the larger groups):

| Tests | Class | First error line | file:line |
|---|---|---|---|
| `worktree_jobs.rs` 13, `push_job.rs` 8, `fetch_job.rs` 5, `telemetry_collect.rs` 7 | d? | ``assertion `left == right` failed left: Running`` (a job does not end), and `HOME is set` (`worktree_jobs.rs:65`) | `crates/horch-core/tests/fetch_job.rs`, `push_job.rs`, `worktree_jobs.rs:65` |
| `fleetd_channel.rs` 6 | d | ``called `Result::unwrap()` on an `Err` value: Os { code: 233, ... }`` (the pipe is not connected) | `crates/horch-core/tests/fleetd_channel.rs:157` |
| `fleet18_brief_worktree.rs` 6, `fakes.rs` 4 | d? | `timed out after 120 s waiting for the worktree of the task` | `crates/horch-e2e/src/harness.rs:1288` |
| `w3_ledger_cli_pins.rs` 5, `skills_cli.rs` 3, `baseline_cli.rs` 2, `idea_intake.rs` 3, `idea_waiting.rs` 2 | c / d? | `the task ledger .\.horch\ledger.sqlite3 does not ...`, `git ["init", "--quiet", "--bare"] failed: fatal: unknow...`, `sessions/sessions-bash-era.txt differs from the A0 oracle`, `Rejected { file: "idea.md", reason: "owner si...` | `crates/horch/tests/w3_ledger_cli_pins.rs:360`, `:481`, `:205`; `skills_cli.rs:162`, `:447`; `baseline_cli.rs:35`; `idea_intake.rs:125` |
| `godot_wave.rs` 5 | c | ``assertion `left == right` failed left: (0, "skipped: no Godot\r\n")`` | `crates/horch-core/tests/godot_wave.rs` (CRLF in the expected text) |
| the rest (about 70 tests in 25 files, 34 of them in the lib test of `horch-core`) | d? | one line for each | not listed one by one: the log of the job is the source |

## Job `docs`

Failure (new in CI4: success in CI3). 1 error:

```
error: public documentation for `ingest` links to private item `verifier::park_unlanded`
  --> crates/horch-core/src/fleetd/task_execution/application/ingest.rs:16:24
16 | //!   with its tree ([`verifier::park_unlanded`], RR-14).
   = note: `-D rustdoc::private-intra-doc-links` implied by `-D warnings`
```

Cause: commit `88de7014` "fleetd-finish 167: done --unlanded in a fleetd attempt parks the task (RR-14) (T-288)", 2026-11-20. It added the link in `ingest.rs` to `pub(crate) fn park_unlanded` (`crates/horch-core/src/fleetd/task_execution/application/verifier.rs:397`). Class d (a doc link of the product). Not fixed at the later tip `4e806d0b`: the line is still there.

## Job `release-lint` (`cargo clippy --workspace --bins --release --locked -- -D warnings`)

Success. The 2 `ExitCode` errors of CI3 are fixed (T-244, CI FIX-17).

## Difference to run 3

CI3 = run 44784709135 (head `001e73c2`). CI4 = this run (head `2c6ef0b4`).

### Per job

| Job | CI3 | CI4 | Change |
|---|---|---|---|
| `test (macos-15)` | 5002 passed, 188 failed | 5186 passed, 23 failed | much better: 165 fewer failures |
| `test (ubuntu-latest)` | 4821 passed, 368 failed | 5157 passed, 52 failed | much better: 316 fewer failures |
| `docs` | success | failure, 1 error | **new red** (T-288, `88de7014`) |
| `release-lint` | failure, 2 errors | success | **fixed** (T-244) |
| `build-windows` | success | success | no change |
| `test-windows` | failure, 1 compile error, 0 tests | cancelled after 3 h; 2666 passed, 813 failed; hang | the compile error is **fixed**; the tests now run, and 813 fail; **new**: the hang |

### Groups of CI3

| Group of CI3 | State in CI4 |
|---|---|
| macOS and Linux: tests of the retired command `horch ledger run new` (90 macOS, 89 Linux) | **fixed** as a group: the log has 0 lines `run new is retired`. 7 tests of `taskledger.rs`, `rr10_...` of `p3_worktrees.rs` and `pl4_...` are still red on macOS with another cause (`2 active runs`, a timeout, no message). |
| 42 tests of `horch-core` that build the fakes with `--profile out` | **fixed** (T-242). 0 lines ``profile `out` ``. |
| Linux: 134 tests, the sandbox with no `bwrap` on the `PATH` | **fixed** for the unit tests (T-241) and for the e2e harness (T-240). **10 tests** of `p4_no_loss.rs` and `p6_features.rs` still fail with the same text, because their pass helper sets `PATH` to an empty folder (new cause, see the section `## Linux`). |
| macOS: 11 tests with the unhealthy profile `antigravity` | **fixed** (T-246). |
| macOS: 9 tests of `messaging.rs` (the codex home) | **fixed** (T-247). |
| `p5_route_ledger_task.rs` (2), `fitness_no_context_touches_foreign_tables`, `harness_codex_rules_snapshot`, `fleetd_pass.rs` (2, `launching`), `watch_once_reports_in_text_and_json` | **fixed** (T-248, T-249). |
| `no_status_literal_in_ledger_sql` | **fixed** (T-245). |
| `given_3_ready_branches_...` (macOS), `fr10_b_...`, `a_worker_that_registers_after_the_wait_still_works`, `fr2_...` (macOS) | **fixed** on macOS (T-254, T-160, T-163). `given_3_...` and `fr2_...` are **still red on Linux**. |
| Lifecycle matrix (`lifecycle.rs`, 5 tests) | **fixed**. |
| `p2_release.rs`, `p3_release.rs` | **fixed** |
| `p0_liveness.rs` (2 of CI3: `fm3_...` and `fm5_...`) | `fm5_...` fixed, `fm3_...` **still red** (macOS, Linux), `sleep_does_not_expire_a_lease` red on Linux, `a_reaped_worker_that_ends_on_the_polite_stop_has_its_pane_closed` red on macOS (see the new red tests). |
| `p3_worktrees_cli.rs` (4 restore tests) | **still red** on macOS, green on Linux. Covered by T-290. |
| `skills_exposure.rs` (3 macOS, 4 Linux) | **still red**, now 4 on macOS (`operator_skills_e2e_deleted_since_spawn_fails_the_launch` is the new one) and 4 on Linux. |
| `i308_fleet_steward.rs` (class a, `triage_*`) | still red: 2 on macOS (3 in CI3), 6 on Linux. |
| `taskledger.rs`: `legacy_plumbing_lives_under_ledger_legacy`, `status_on_a_fresh_ledger_creates_it_and_says_no_runs`, `the_import_finds_work_that_never_landed` | **fixed** (T-232, T-233, T-234 are done). |
| `gate_at_its_limit_is_abandoned_and_its_group_killed` (Linux) | **still red**, fixed after this run by `b29ac09b`. |
| `channel.rs:252`, `sandbox.rs:374` (Linux) | **still red**, fixed after this run by `b29ac09b`. |
| `p6_release_run_with_an_open_question_gets_1_idea_1_feature_and_1_owner` (Linux) | **fixed** (T-240 per the commit message of `b29ac09b`). |
| `p3_triage.rs` (10), `i308_triage.rs` (the `no_sandbox` result) (Linux) | **fixed**. |
| Linux `p4_repair.rs` hang (3 tests) | **fixed**. The Linux job took 74 min (CI3: 118 min). |
| `release-lint`: the 2 `ExitCode` errors | **fixed** (T-244). |
| Windows: `horch-platform` test `sandbox` compile error | **fixed** (T-261). |

By test name: macOS 173 fixed, 15 still red, 8 not named in `ci-run-3.md`. Linux 345 fixed, 23 still red, 29 not named in `ci-run-3.md`. A group of CI3 that was named by count only (`taskledger.rs` 58 `run new` tests) has no test names in `ci-run-3.md`, so "not named" can mean "was red, in the unnamed group".

### New red tests (red now, not named in `ci-run-3.md`)

| Test | OS | Cause | Commit or task |
|---|---|---|---|
| `docs` job, `ingest.rs:16` | Linux | a doc link to a private fn | T-288 (`88de7014`) |
| `a_reaped_worker_that_ends_on_the_polite_stop_has_its_pane_closed` | macOS | a probe request file stays (`request_file_rejected`) | not found (T-268 changed the fake agent for `p1_orphans`) |
| `rr10_hand_made_worktree_is_adopted_within_1_list_period` | macOS, Linux | ``assertion left == right failed: 70`` (a time check) | not found (T-275 and T-276 changed the time limits of fleetd teardown) |
| `operator_skills_e2e_deleted_since_spawn_fails_the_launch` | macOS, Linux | `the skills changed since <attempt> was spawned` | the same cause as its 3 sister tests |
| `taskledger.rs`: `a_critical_finding_fails_until_a_fix_is_reviewed_again`, `a_live_worker_that_does_not_claim_is_reported_once`, `a_parked_unlanded_task_names_its_dependents_and_a_stuck_run_is_reported_once`, `a_task_whose_attempts_all_fail_is_failed_and_its_replan_requeues_it`, `the_watchdog_ends_pre_claim_rows_whose_process_is_gone` | macOS, Linux | `2 active runs`, a timeout, or no message | probably in the unnamed group of 58 `run new` tests of CI3; the migration by T-232 to T-234 changed the cause |
| `taskledger.rs`: `a_check_past_its_timeout_is_killed_with_its_children`, `a_codex_workers_check_cannot_write_outside_its_worktree` | Linux | see the section `## Linux` | T-267 (check sandbox on Linux) or T-17 probable |
| `fakes.rs` `phase4::*` (4), `g54_...`, `sleep_does_not_expire_a_lease` | Linux | timeouts, 2 active runs | not found |
| the 10 tests of `p4_no_loss.rs` and `p6_features.rs` | Linux | `PATH` of the pass helper has no `bwrap` | T-240 (it put `bwrap` on the sealed `PATH`; the 2 helpers replace it) |
| `p7_conformance.rs` `sandbox_write_outside_is_refused`, `p7_operator_boundary.rs` (2) | Linux | `/tmp` is a tmpfs in the sandbox | T-125 and T-65 (tests landed before this run); fixed after by `0061d502`, `c6dede2f` |
| `sr3_no_agent_push.rs` `verifier_check_sends_no_message_outside_the_fleet`, `watch_once_runs_the_phase4_pass` | Linux | loopback in the sandbox; the launch of `horch job run` | T-17 or T-301 probable |

### Tasks that cover each failure that is still red

State of the run board at 2026-11-21 06:32Z (`horch ledger status --run fleetd-finish`): 330 tasks, 276 done, 2 running (T-310, T-330), 1 landing (T-286), 13 queued, 38 cancelled.

| Failure | Task that covers it |
|---|---|
| `docs`: `ingest.rs:16` | no ticket |
| The 10 tests of `p4_no_loss.rs` and `p6_features.rs` (Linux) | no ticket |
| The 4 restore tests of `p3_worktrees_cli.rs` (macOS) | T-290 (queued, blocked by T-286, which is landing). T-253 was cancelled. |
| `rr10_...` of `p3_worktrees.rs`; the worktree tests that wait for a rescue | T-286 owns `disk.rs` and `reconcile.rs` (landing). Its title (the states `landing` and `landed`) does not name `rr10_...`: no ticket for this test. |
| `a_fleet_with_an_abandoned_worktree_spawns_the_worktree_steward`, `an_exhausted_claude_pool_starts_no_codex_steward`, `the_no_change_steward_task_finishes_without_a_worktree` (class a), `the_whole_flow_on_the_fake_herdr` (class a) | T-94 (queued, "verify the 7 expected-failures rows"; T-13 was cancelled, so its dependency is stale). The 3 `triage_*` tests: no ticket. |
| `arch_scan` foreign tables (not red in this run; red on the later tip) | T-327 (queued, blocked on `disk.rs` held by T-286) |
| Linux gate group kill, `local_network...`, `channel.rs` | done after this run (T-309, `b29ac09b`) |
| Linux `p7_conformance.rs`, `p7_operator_boundary.rs` | done after this run (T-320) |
| Linux `p7_conformance_binary.rs:465`, `p5_operator_boundary.rs:802` | no ticket. T-319 (queued) is "each stop of a job ends its sandbox"; it names jobs, not these tests. |
| All other rows of the tables above | no ticket |

### `no ticket` rows of CI3 and their state now

| Row of CI3 | State now |
|---|---|
| FIX-11 (42 `--profile out` tests) | **fixed** (T-242) |
| FIX-12 (sandbox `PATH` of the unit tests) | **fixed** (T-241) |
| FIX-13 (sandbox `PATH` of the e2e harness) | **partly fixed** (T-240): 10 tests still fail (new unit FIX-26) |
| FIX-14 (Windows `sandbox.rs`) | **fixed** (T-261); the tests now run and 813 fail |
| FIX-15 (status words in the triage SQL) | **fixed** (T-245) |
| FIX-16 (`p4_release.rs`, `p4_clock_and_heartbeat.rs`) | **still red** (T-251 is done): `2 active runs` and `watch` exit 1 |
| FIX-17 (2 `ExitCode` errors) | **fixed** (T-244) |
| FIX-18 (profile `antigravity`) | **fixed** (T-246) |
| FIX-19 (messaging.rs codex home) | **fixed** (T-247) |
| FIX-20 (`p5_route_ledger_task.rs`) | **fixed** (T-248) |
| FIX-21 (4 small core tests) | **fixed** (T-249) |
| FIX-22 (5 Linux tests) | **partly fixed**: 3 fixed after this run by `b29ac09b`; `p6_release...` fixed; `p5_operator_boundary.rs:802` still red |
| Linux `p4_repair` hang (3 tests, 320 s each) | **fixed** |
| `fm3_...`, `pl4_...`, `skills_exposure.rs`, `i308_fleet_steward.rs` | **still red** |

## Fix units

Only failures with no ticket. Each unit is for 1 worker. The command of each unit is the CI line that failed on macOS-15 at head `2c6ef0b4`, run through `scripts/dev-sandbox.sh` (rule 2 of the run). I did not run any of them: this task reads the CI only (added rule of 2026-11-21). A unit says "no macOS repro" when the failure is Linux-only or Windows-only.

| Unit | Fixes | Files it touches | Command that reproduces 1 failure |
|---|---|---|---|
| FIX-26: the doc link of `ingest.rs` | the `docs` job | `crates/horch-core/src/fleetd/task_execution/application/ingest.rs:16` (name the item in code font, or link a pub item) | `RUSTDOCFLAGS='-D warnings' scripts/dev-sandbox.sh cargo doc --workspace --no-deps --all-features --locked` (fails on macOS the same way; it is the line of the `docs` job) |
| FIX-27: the `PATH` of the pass helpers | 10 Linux tests of `p4_no_loss.rs` and `p6_features.rs` | `crates/horch-e2e/tests/p4_no_loss.rs:377`, `crates/horch-e2e/tests/p6_features.rs:507` (keep the empty `no-clis` folder first and append the host tool folder of the harness, `harness.rs` `host_bin`, Linux only) | no macOS repro (the 10 tests pass on macOS). On Linux: `cargo test -p horch-e2e --test p4_no_loss i8_squash_merge_with_a_conflict_resolution_is_seen_as_landed`. Run each of the 10 tests 3 times (rule 6). |
| FIX-28: the tests that need `--run <slug>` | `fm3_...`, `sleep_does_not_expire_a_lease`, `pl4_...`, `run_without_gates_holds_dispatch_and_loses_no_task`, `a_live_worker_that_does_not_claim_is_reported_once`, `the_watchdog_ends_pre_claim_rows_whose_process_is_gone`, `a_parked_unlanded_task_names_its_dependents_and_a_stuck_run_is_reported_once` (macOS and Linux) | `crates/horch-e2e/tests/p0_liveness.rs:63`, `p4_clock_and_heartbeat.rs:246`, `p4_release.rs:1116`, `taskledger.rs:259`, `:2794`, `:2831`. First find why a second run `F-0002` is active (the log does not say). | `scripts/dev-sandbox.sh cargo test -p horch-e2e --test p0_liveness fm3_idle_claim_is_caught_in_under_10_min` |
| FIX-29: the skills tests | `operator_skills_e2e_changed_since_spawn_fails_the_launch`, `operator_skills_e2e_deleted_since_spawn_fails_the_launch`, `plugin_skills_e2e_changed_since_spawn_fails_the_launch`, `skl_06_e2e_exposure_antigravity` (macOS and Linux) | `crates/horch-e2e/tests/skills_exposure.rs:664`, `:678`, `:777`, `:257` | `scripts/dev-sandbox.sh cargo test -p horch-e2e --test skills_exposure operator_skills_e2e_changed_since_spawn_fails_the_launch` |
| FIX-30: the 3 steward `triage_*` tests, `g50_...`, `rr10_...` | `triage_lists_rows_from_a_ledger_it_cannot_write`, `triage_delete_accepts_a_clean_detached_head_reachable_from_a_branch`, `triage_delete_refuses_a_detached_unique_commit` (the first on macOS and Linux), `g50_legacy_orchestrator_and_host_load_are_advisory`, `rr10_hand_made_worktree_is_adopted_within_1_list_period` | `crates/horch-e2e/tests/i308_fleet_steward.rs:41` (the setup calls `horch ledger cancel T-1`, which fails), `crates/horch-e2e/tests/fleet18_observability.rs:806` (macOS) and `:385` (Linux), `crates/horch-e2e/tests/p3_worktrees.rs:545` | `scripts/dev-sandbox.sh cargo test -p horch-e2e --test i308_fleet_steward triage_lists_rows_from_a_ledger_it_cannot_write` |
| FIX-31: the probe file of the polite-stop test | `a_reaped_worker_that_ends_on_the_polite_stop_has_its_pane_closed` | `crates/horch-e2e/tests/p0_liveness.rs:594` and the fake agent that writes `horch-probe-<pid>.json` (see T-268 for the same fix in `p1_orphans`) | `scripts/dev-sandbox.sh cargo test -p horch-e2e --test p0_liveness a_reaped_worker_that_ends_on_the_polite_stop_has_its_pane_closed` |
| FIX-32: the 5 other `taskledger.rs` tests | `a_critical_finding_fails_until_a_fix_is_reviewed_again`, `a_push_review_task_ends_done_by_its_findings_file_and_its_task_lands`, `a_task_whose_attempts_all_fail_is_failed_and_its_replan_requeues_it` (macOS and Linux), `a_check_past_its_timeout_is_killed_with_its_children` (Linux; probably fixed by `b29ac09b`; run it first), `a_codex_workers_check_cannot_write_outside_its_worktree` (Linux) | `crates/horch-e2e/tests/taskledger.rs:293`, `:2926`, `:1942` | `scripts/dev-sandbox.sh cargo test -p horch-e2e --test taskledger a_critical_finding_fails_until_a_fix_is_reviewed_again` |
| FIX-33: Linux tests with the loopback and `/tmp` of the sandbox | `sr3_check_that_connects_off_host_is_refused`, `verifier_check_sends_no_message_outside_the_fleet`, `real_conformance_gate_script_runs_isolated_and_retains_reports`, `operator_request_from_a_worker_sandbox_is_refused_for_each_operator_kind` | `crates/horch-e2e/tests/p4_verifier.rs:629`, `crates/horch-core/tests/sr3_no_agent_push.rs:461`, `crates/horch-e2e/tests/p7_conformance_binary.rs:465`, `crates/horch-e2e/tests/p5_operator_boundary.rs:802` (put the fixture in `/var/tmp`, as `0061d502` did; start the listener inside the sandbox, as `b29ac09b` did) | no macOS repro (Linux-only). On Linux: `cargo test -p horch-core --test sr3_no_agent_push verifier_check_sends_no_message_outside_the_fleet` |
| FIX-34: Linux worktree waits and launch timeouts | `phase4::ask_scenario_sends_one_question` and its 3 sisters (`fakes.rs`), `fm2_agent_that_never_claims_has_its_task_bound_at_spawn`, `fr2_each_sub_orchestrator_attempt_has_1_idea`, `g54_...`, `watch_once_runs_the_phase4_pass` (cause not proved: read the harness diagnostics first) | `crates/horch-e2e/tests/fakes.rs`, `p0_dispatch.rs:260`, `p6_sub_orchestrators.rs:353`, `fleet18_observability.rs:1106`, `crates/horch/src/cmd/taskledger_watch.rs:527` | no macOS repro (they pass on macOS, except `fr2_...`, which passes on macOS in this run). On Linux: `cargo test -p horch-e2e --test fakes phase4::bound_scenario_writes_entry_json_at_entry` |
| FIX-35: Windows, CRLF of the roster base | 369 tests | `crates/horch-core/src/roster/parser.rs:31` and `:33` (accept `\r\n`), or add `.gitattributes` with `* text=auto eol=lf` for the compiled-in files | no macOS repro of the failure. A test that proves the fix on macOS: `split_frontmatter("---\r\nname: a\r\n---\r\nbody\r\n")` in `crates/horch-core/src/roster/parser.rs` (new unit test); run `scripts/dev-sandbox.sh cargo test -p horch-core --lib roster::parser` |
| FIX-36: Windows, verbatim path to `git` | 163 tests | `crates/horch-core/tests/bisect.rs:96`, `crates/horch-core/src/fleetd/task_execution/application/launch.rs:485` and the other `canonicalize()` calls of the tests (use a path with no `\\?\` prefix); check the product code that passes a path to `git worktree add` | no macOS repro (Windows-only) |
| FIX-37: Windows, no sandbox for `git` | 83 tests | `crates/horch-platform/src/windows/sandbox.rs`; the tests of `integration_git.rs`, `idea_direct.rs`, `ref_move.rs`, `branch_abandon.rs`, `merged_with_ideas.rs`, `new_features.rs` (an explicit decision is needed: a sandbox for Windows, or these tests stay `#[cfg(unix)]`) | no macOS repro (Windows-only) |
| FIX-38: Windows, `/tmp`, `node` and the CRLF of expected text | 37 + 8 + 6 tests | `crates/horch/tests/r_cli_cmds_ledger.rs:19`, `crates/horch-core/tests/p7_conformance.rs`, `context_prime.rs`, `p5_claude_sandbox.rs`, `p7_claude_exec.rs` (`tempdir_in("/tmp")`), `crates/horch-core/tests/pi_compact.rs`, `crates/horch-core/tests/godot_wave.rs` | no macOS repro (Windows-only) |
| FIX-39: Windows, the hang of `fleet18_messaging.rs` and the missing time limit | 4 tests that never end, and the job that runs up to 6 h | `crates/horch-e2e/tests/fleet18_messaging.rs` (`g60_missing_registration_still_queues_worker_mail`, `g60_optional_question_projection_error_does_not_abort_courier_startup`, `g62_courier_notifies_once_and_resumes_after_login`, `g62_login_screen_is_detected_before_typing`), the wait helpers in `crates/horch-e2e/src/harness.rs`, and `.github/workflows/ci.yml` (`timeout-minutes` for `test-windows`; this task could not edit it) | no macOS repro (Windows-only; the 4 tests pass on macOS and Linux) |
| FIX-40: Windows, the other 154 failures | jobs that stay `Running`, the pipe error 233 in `fleetd_channel.rs:157`, the waits of `fleet18_brief_worktree.rs` and `fakes.rs`, the pins of `w3_ledger_cli_pins.rs` | the files of the table "The 154 other failures". Do this after FIX-35 to FIX-37, because they hide the real cause of about 50 of these tests. | no macOS repro (Windows-only) |

Order: FIX-26 and FIX-27 first (small, each makes a job or 10 tests green). FIX-35 to FIX-37 before FIX-40. After FIX-26 to FIX-34, T-286, T-290 and T-94: run the CI 1 time more, and run the Windows job with a limit that ends a hang.
