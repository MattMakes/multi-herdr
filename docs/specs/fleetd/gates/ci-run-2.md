# CI run 2 of fleetd, 2026-11-20

- Run: 44701297167, https://github.com/MattMakes/multi-herdr/actions/runs/44701297167
- Trigger: `workflow_dispatch` on `fleet/fleetd-finish`, started 2026-11-20 08:17Z.
- Head commit of the run: `7fbdc0d3b049119b0392e7feb26370fa33fa35c4`.
- Tip of `origin/fleet/fleetd-finish` when the report was written: `e2b2d89ee6a86529f277329fd4e5ec3a43c3161c`. The run head is an ancestor of the tip. The tip has 30 or more newer commits, among them T-44, T-40, T-38 and T-4. The check "headSha equals the tip" of step 3 of the ticket does not hold: the run started before those commits landed. This report covers `7fbdc0d3`, not the tip.
- Toolchain: `stable` = rustc 1.100.0 (0ef1bc386 2026-11-09), the same as CI0.
- Compared with: CI0, `docs/specs/fleetd/gates/ci-baseline-2026-11.md` (run 44680906908, head `085427fb`).
- Source of the counts: the sums of the lines `test result:` of each job log, and the blocks `---- <test> stdout ----` of the failed tests. Nothing is run locally.
- Run result: failure. 1 of 6 jobs passed. The run ended at 09:29:34Z.

| Job | OS | Result | Time |
|---|---|---|---|
| `test (macos-15)` | macOS | failure: 4796 passed, 270 failed | 54 min 42 s |
| `test (ubuntu-latest)` | Linux failure: 4691 passed, 375 failed | 72 min 28 s |
| `docs` | Linux | failure | 1 min 7 s |
| `release-lint` | Linux | failure | 1 min 50 s |
| `build-windows` | Windows | success | 2 min 6 s |
| `test-windows` | Windows | failure (the job has `continue-on-error: true`): 0 tests ran | 2 min 16 s |

1 of 6 jobs is green. This is the same count as CI0.

## Classes

- (a) a test that is on `crates/horch-e2e/expected-failures/`
- (b) a compile error on that OS
- (c) a test that is unix-only by its code but runs on Windows
- (d) a real fault of the product (or of the test harness) on that OS
- (e) a fault of the CI setup
- (f) not one of the classes of the ticket: the test is stale. It calls a command that fleetd retired, or it lacks a setup that the product needs now. It fails the same way on each OS. CI0 used the same class.

## Does the harness fix of T-59 work with cargo 1.100?

Yes for `crates/horch-e2e`. No for 4 test files of `horch-core`.

- On macOS 0 tests of `horch-e2e` fail with ``profile `out` is not defined``. In CI0 about 528 tests failed that way. The e2e tests now start the harness, build `horch` and the fakes, and run. 4796 tests passed in this job. In CI0 4411 passed.
- 38 tests still fail with ``error: profile `out` is not defined``. They are in 4 test files of `horch-core`. Each file has its own copy of the code that took the profile from the folder name, which T-59 did not change:
  - `crates/horch-core/tests/p7_conformance.rs:75-81` (20 tests)
  - `crates/horch-core/tests/p7_context_limit.rs:97-103` (6 tests)
  - `crates/horch-core/tests/p7_operator_boundary.rs:90-96` (6 tests)
  - `crates/horch-core/tests/p7_wrapper_exec.rs:75-81` (6 tests)
- These are the "38 tests that build only the fakes" of CI0. They are still red. The fix is in unit FIX-11 below.

## macOS

Job `test (macos-15)`: failure. 4796 tests passed, 270 tests failed. In CI0: 4411 passed, 666 failed.

### Groups

| Group | Count | Class | First error line | file:line |
|---|---|---|---|---|
| Tests of the retired command `horch ledger run new` (the e2e harness and the CLI tests start a run with it) | 116 | f | `horch ledger run new is retired. Add an idea: horch idea add "<idea>"` | `crates/horch-e2e/tests/taskledger.rs:195` (57 tests), `p3_refusals.rs:52`, `p3_unlanded.rs:50`, `p3_worktrees.rs:90`, `p2_leases.rs:107`, `p0_liveness.rs:32`, `p0_rescue.rs:70`, `p0_supervision.rs:34`, `p1_release.rs:39`, `p1_jobs.rs:51`, `p1_pass.rs:58`, `fakes.rs:987`, `fleet18_brief_worktree.rs:83` |
| Tests of the retired command `horch spawn` | 39 | f | `horch spawn is retired. fleetd launches each attempt. To pair with a persona, plan its task with an interactive persona` | `crates/horch-e2e/tests/lifecycle.rs:111` (12 tests), `skills_exposure.rs:66` (10), `skills_exposure.rs:649`, `fleet18_observability.rs:310`, `routing_provenance.rs:54`, and others |
| The operator token file is missing in the test home | 19 | f | `reading the operator token` | `crates/horch-e2e/tests/done_budget.rs:96` (5), `fleet18_observability.rs:674` (4), `:68`, `:792`, `:1044`, `crates/horch/src/cmd/rescuecmd.rs:290`, `:329`, `crates/horch-core/tests/context_prime.rs` and others |
| The run has no idea in flight (FR-12) | 15 | f | `run r1 has no idea in flight of its feature F-0001, and a task needs an idea (FR-12). Add one: horch idea add` | `crates/horch-e2e/tests/fleet18_messaging.rs:725` (5) and `:311` (3), `crates/horch-core/tests/worktree_disk.rs:191` (3), `crates/horch-core/src/taskledger/stop.rs:1043` (2), `crates/horch/src/cmd/sessionscmd.rs:438` |
| Tests that call another retired command | 2 | f | `horch ledger task is retired. Add a ticket that you wrote: horch idea add --direct <ticket file>` | `crates/horch/src/cmd/recipes.rs:1152` |
| The fakes of `horch-core` are built with `--profile out` | 38 | d | ``error: profile `out` is not defined`` after `the build of the fakes failed: cd ".../crates/horch-core/../.." && env -u CARGO_MANIFEST_DIR ... cargo build --locked -p horch-e2e --bin fake-claude ... --profile out` | `crates/horch-core/tests/p7_conformance.rs:86`, `p7_context_limit.rs:108`, `p7_operator_boundary.rs:101`, `p7_wrapper_exec.rs:86` (the cause is at the lines 75-81, 97-103, 90-96 and 75-81 of the same files) |
| Other failures | 41 | see below | see below | see below |

Total: 116 + 39 + 19 + 15 + 2 + 38 + 41 = 270.

### Other failures (41 tests)

| Test | Class | First error line | file:line |
|---|---|---|---|
| `check_runs_in_the_worker_sandbox_not_with_fleetd_rights`, `check_job_has_no_forbidden_env`, `claude_worker_check_runs_in_the_platform_sandbox`, `codex_worker_check_keeps_codex_sandbox` | f? | ``called `Option::unwrap()` on a `None` value`` | `crates/horch-core/tests/check_sandbox.rs:254` |
| `restore_of_a_task_makes_branch_rescue_t_seq_n_and_its_tree_with_each_rescued_file`, `restore_of_a_task_whose_tree_is_removed_uses_its_rescue_ref`, `restore_refuses_when_the_branch_or_the_folder_exists_and_changes_nothing`, `restore_runs_no_hook_of_the_project` | f? | `timed out waiting for fleetd to rescue the tree of T1` | `crates/horch/tests/p3_worktrees_cli.rs:140` |
| `triage_archive_is_refused_with_a_later_task` | f? | `assertion left == right failed: W1 feat DELETE: queued in the triage_apply job 1` | `crates/horch/tests/p5_cli_writers.rs:403` |
| `the_legacy_resume_and_assign_arms_run`, `the_legacy_subcommands_write_read_and_report_a_session` | f? | `["add", "--record-id", "r1", ...]: Output { status ... }` | `crates/horch/tests/r_cli_cmds_ledger.rs:29` |
| `cmd::context::tests::ctx_06_windows_view_lists_claude_and_codex_teammates` | f? | the table of teammates differs (`codex-sol ... 256,000`) | `crates/horch/src/cmd/context.rs:1267` |
| `cmd::messaging::tests::trf_45_note_and_ledger_progress_are_1_command_and_write_no_json` | f? | ``Err: refused: not handled in this version`` | `crates/horch/src/cmd/messaging.rs:1532` |
| `cmd::taskledger::tests::no_allowed_ledger_subcommand_takes_a_check_command` | f? | `["reopen"]` | `crates/horch/src/cmd/taskledger.rs:2151` |
| `harness::codex::tests::each_launch_gets_only_its_own_rules` | f? | ``assertion failed: read(&orchestrator).contains(r#"pattern = ["horch", "spawn"]"#)`` | `crates/horch-core/src/harness/codex.rs:2761` |
| `taskledger::schema::v7_tests::trf_48_every_old_version_migrates_to_current` | f? | `assertion left == right failed: version 14` | `crates/horch-core/src/taskledger/schema.rs:2279` |
| `no_status_literal_in_ledger_sql` | f | `status words written as literals; build them from the enums in taskledger/model.rs` | `crates/horch-core/tests/w5_status_words_scan.rs:93` |
| `g64_skill_states_each_delegation_and_reporting_rule_within_budget` | f? | `missing verified (with evidence) or not verified` | `crates/horch-e2e/tests/fleet18_brief_worktree.rs:391` |
| `g52_concurrent_workers_get_their_own_session_and_transcript` | f | `timed out waiting for two session transcripts` | `crates/horch-e2e/tests/fleet18_observability.rs:331` |
| `g52_resume_uses_own_session` | a | is on the list of expected failures with the reason `horch spawn refused: no ready task for persona codex-terra`, but it failed with `timed out waiting for two session transcripts` | `crates/horch-e2e/src/expected.rs:108` |
| `g53_ledger_handoff_compacts_resumes_and_records_progress_once` | a | is on the list with the reason `horch ledger run new is retired`, but it failed with `reading the operator token` | `crates/horch-e2e/src/expected.rs:108` |
| `an_exhausted_claude_pool_starts_no_codex_steward`, `a_fleet_with_an_abandoned_worktree_spawns_the_worktree_steward`, `the_no_change_steward_task_finishes_without_a_worktree` | a | is on the list, but it failed with another message (`horch ledger cancel T-1 not needed now: status: exit status: 1`, or `no run 'solo-w1'`) | `crates/horch-e2e/src/expected.rs:108` |
| `triage_delete_accepts_a_clean_detached_head_reachable_from_a_branch`, `triage_delete_refuses_a_detached_unique_commit`, `triage_lists_rows_from_a_ledger_it_cannot_write` | f | `horch ledger cancel T-1 not needed now: status: exit status: 1` | `crates/horch-e2e/tests/i308_fleet_steward.rs:41` |
| `arc_16_e2e_fail_split` | f? | `assertion left == right failed: []` | `crates/horch-e2e/tests/lifecycle.rs:456` |
| `rr4_tasks_complete_with_all_file_watch_events_dropped`, `rr7_fleetd_killed_mid_run_ends_as_a_clean_run` | f? | `the test fleetd exited (Some(ExitStatus(unix_wait_status(0)))):` | `crates/horch-e2e/src/fleetd.rs:105` |
| `held_task_with_a_live_worker_on_phase_1_is_not_reaped_after_the_update_and_has_1_lease` | f | `horch ledger show T-1 --json: status: exit status: 1` | `crates/horch-e2e/tests/p2_release.rs:199` |
| `live_tree_and_dirty_orphan_on_phase_2_are_active_and_listed_and_nothing_is_removed_after_the_update` | f? | `assertion left == right failed: status: exit status: 0` | `crates/horch-e2e/tests/p3_release.rs:401` |
| `pl4_sleep_past_a_branch_deadline_fires_nothing_on_wake` | f | `horch ledger watch: status: exit status: 1` | `crates/horch-e2e/tests/p4_clock_and_heartbeat.rs:246` |
| `run_without_gates_holds_dispatch_and_loses_no_task` | f? | `horch ledger status names the hold` | `crates/horch-e2e/tests/p4_release.rs:1116` |
| `fr5_worker_never_runs_a_task_search` | f? | `This is a conformance check of horch. Do each step, then give 1 JSON object as your last message` | `crates/horch-e2e/tests/p4_safety.rs:235` |
| `fm9_docs_task_without_its_section_stays_not_done` | f? | `assertion left == right failed: {"attempt":"<uuid>/A1","check_hash":...` | `crates/horch-e2e/tests/p4_verifier.rs:492` |
| `operator_request_from_a_test_in_a_gate_is_refused_for_each_act` | f? | `assertion left == right failed: the gate changes no state of the ledger` | `crates/horch-e2e/tests/p5_operator_boundary.rs:1345` |
| `the_hermetic_story`, `the_hermetic_story_with_an_ollama_that_starts_in_7_s` | f? | `assertion left == right failed: left: Some(1), right: Some(3)` (step 7, `route opus: REFUSED, exit 3`) | `crates/horch-e2e/tests/scenario.rs:241` |
| `operator_skills_e2e_exposure_claude`, `operator_skills_e2e_check_warns_on_a_missing_name`, `git_env_cannot_reach_another_repository` | f | `stdout:` (empty stdout, the `horch spawn` refusal is on stderr) | `crates/horch-e2e/tests/skills_exposure.rs:511`, `:592`, `:637` |
| `legacy_plumbing_lives_under_ledger_legacy` | f | `assertion left == right failed: .../ledger.sqlite3` against `.../state/ledger.sqlite3` | `crates/horch-e2e/tests/taskledger.rs:300` |
| `status_on_a_fresh_ledger_creates_it_and_says_no_runs` | f | `status: exit status: 1` | `crates/horch-e2e/tests/taskledger.rs:274` |

The count of rows above is lower than 41 because a row can name up to 4 tests.

## Linux

Job `test (ubuntu-latest)`: failure. 4691 tests passed, 375 tests failed. In CI0: 4384 passed, 693 failed. The job ran from 08:17:06Z to 09:29:34Z, 72 min 28 s. The job did not hang: the live log lagged for 30 min, and 3 tests of `p4_repair.rs` each waited the full 320 s.

The runner is Ubuntu with `bubblewrap` 0.11.1 and `socat` 1.8.1.1. The step `Install bubblewrap` ran and `kernel.apparmor_restrict_unprivileged_userns` is 0. Both programs are installed. The socat install of T-75 is on the run head.

### Groups of `test (ubuntu-latest)`

| Group | Count | Class | First error line | file:line |
|---|---|---|---|---|
| Tests of the retired commands `horch ledger run new`, `horch spawn`, `horch ledger task`, the missing operator token, and a run with no idea in flight | 190 | f | the same first error lines as the first 5 groups of the macOS section | the same files as on macOS |
| The fakes of `horch-core` are built with `--profile out` | 38 | d | ``error: profile `out` is not defined`` | `crates/horch-core/tests/p7_conformance.rs:86`, `p7_context_limit.rs:108`, `p7_operator_boundary.rs:101`, `p7_wrapper_exec.rs:86` (the cause is at the lines 75-81, 97-103, 90-96 and 75-81) |
| The sandbox refuses to launch: `bwrap` and `socat` are installed, but not on the `PATH` of the test | 82 | d | `refusing to launch without a sandbox: bwrap and socat not on PATH; install bubblewrap and socat` (unit tests of `horch-core`), `sandbox is not supported on linux: bwrap is not on PATH; install bubblewrap` (fleetd of the e2e tests) | see the table below |
| Timeouts of fleetd in e2e tests that start a job | 18 | d | `timed out after 120 s waiting for <the state>` (the tests of `taskledger.rs`: `timed out waiting for 1 agent PIDs`) | `crates/horch-e2e/src/harness.rs` (12), `crates/horch-e2e/tests/taskledger.rs` (4), `fleet18_observability.rs` (2) |
| Other failures | 47 | see below | see below | see below |

Total: 190 + 38 + 82 + 18 + 47 = 375.

### The sandbox group (class d, 82 tests)

The class of CI0 for these tests was (e), the fault of the CI setup. It is not (e) now. The install step works. The programs are on the `PATH` of the runner. The tests give them another `PATH`.

| Cause | Tests | file:line of the cause |
|---|---|---|
| The unit tests build `LaunchEnv::for_test()` with `path: None`. The sandbox check `which(path, None, "socat")` then searches no folder, so no host can pass it | the 27 of CI0 (`harness::launch::tests::*` 17, `execution_plan.rs` 2, `context_launch.rs` 2, `baseline_oracles.rs` 2, `harness_defaults.rs`, `p7_context_256k.rs`, `p7_conformance_job.rs`, `claude.rs`) | `crates/horch-core/src/harness/launch.rs:153-159` (`for_test`), `crates/horch-core/src/harness/claude.rs:1637-1658` (`sandbox_host_problem`) |
| The e2e harness seals the environment of fleetd and of each CLI with `PATH` = the folder of the fakes only. fleetd cannot find `/usr/bin/bwrap`, so each job that needs a sandbox is refused (`running git config --list ... no sandbox: sandbox is not supported on linux: bwrap is not on PATH`). The log has 3884 such lines | the e2e tests of the groups of the e2e harness: `crates/horch-e2e/src/harness.rs` (27 waits that time out), `p4_no_loss.rs` (5), `p6_features.rs` (5), `p4_repair.rs` (3), `fakes.rs` (3), `p5_operator_boundary.rs` (2), `p6_idea_flow_binary.rs` (2), `p7_docs_sync.rs` (2), `p7_release.rs` (2), `p0_landing.rs`, `p3_triage.rs`, `p4_rescue_refs.rs`, `p7_conformance_binary.rs` | `crates/horch-e2e/src/harness.rs:375` (`env.insert("PATH", self.bin ...)` in `seal`) |

The 18 timeouts and 12 of the 47 "other" failures (the 11 tests of `p3_triage.rs` and `i308_triage.rs`, with `unknown field no_sandbox, expected decisions`) have the same cause: the job result carries `no_sandbox`, the tests wait for another result. These are probable, not proved: the log of each test does not name `bwrap` in the failure line.

### Other failures of Linux (47 tests)

The tests that fail on macOS too are in the macOS section, with the same cause: `check_sandbox.rs` (4), `cmd::context`, `cmd::messaging`, `cmd::taskledger`, `p5_cli_writers.rs`, `codex.rs:2761`, `schema.rs:2279`, `w5_status_words_scan.rs`, `g64_...`, `arc_16_e2e_fail_split`, `rr4` and `rr7` (`fleetd.rs:105`), `p2_release.rs`, `p3_release.rs`, `p4_clock_and_heartbeat.rs`, `p4_verifier.rs`, `p5_operator_boundary.rs`, `scenario.rs` (2), `skills_exposure.rs` (3), `taskledger.rs` (3), `restore_runs_no_hook_of_the_project`. The tests that fail on Linux only:

| Test | Class | First error line | file:line |
|---|---|---|---|
| `store_dirlock_excludes_concurrent_writers` | f? | ``Err: Any { .. }`` | `crates/horch-core/tests/execution_store.rs:343` |
| `line_over_the_limit_is_refused` | f? | ``Err: Os { code: 104, kind: ConnectionReset ...}`` | `crates/horch-core/tests/fleetd_channel.rs:157` |
| `gate_at_its_limit_is_abandoned_and_its_group_killed` | f? | `the gate group was killed` | `crates/horch-core/tests/gate_jobs.rs:958` |
| `harness::launch::tests::an_agent_launch_drops_the_git_repository_variables` | f? | `assertion failed` | `crates/horch-core/src/harness/launch.rs:3134` |
| `a_folder_at_the_path_of_a_removed_row_is_a_new_orphaned_row_and_nothing_is_removed`, `pres_04_w8_refs_move_forward_and_are_read_back`, `pres_04_w9_triage_commits_untracked_before_delete`, `multi_item_triage_apply_preserves_completed_first_item_when_second_item_fails`, `rr16_second_rescue_through_triage_does_not_overwrite_the_first_ref`, `restore_makes_its_tree_through_a_worktree_add_job`, `rr16_failed_read_back_keeps_the_folder`, `rr17_triage_deletes_a_branch_only_when_the_trash_ref_holds_its_tip`, `trash_ref_older_than_30_days_is_collected_and_a_rescue_ref_is_not`, `rr17_triage_refuses_a_unique_detached_head_and_the_tree_stays` | d | `fleetd refused the facts of the result file: the facts of the result file have the wrong form: unknown field no_sandbox, expected decisions` | `crates/horch-e2e/tests/p3_triage.rs` |
| `archive_restore_and_gc_through_the_cli` | d | the same `no_sandbox` result | `crates/horch-e2e/tests/i308_triage.rs` |
| `a_worker_can_drop_a_clean_worktree_from_an_explicit_other_run` | f? | `horch worktrees --json: status: exit status: 1` | `crates/horch-e2e/tests/i308_fleet_steward.rs` |
| `operator_request_from_a_worker_sandbox_is_refused_for_each_operator_kind` | d | `horch rescue drop reaches fleetd: horch: sandbox is not supported on linux: bwrap is not on PATH` | `crates/horch-e2e/tests/p5_operator_boundary.rs` |
| `real_conformance_gate_script_runs_isolated_and_retains_reports` | d | `the conformance suite gave 2 wrong runs` | `crates/horch-e2e/tests/p7_conformance_binary.rs` |
| `the_import_finds_work_that_never_landed` | f? | `assertion left == right failed` | `crates/horch-e2e/tests/taskledger.rs` |

### What the sandbox group means for the CI

The change of T-75 (socat in the install step) is correct and is not enough. A Linux run is green on this point only after the 2 causes of the table above are fixed. The e2e tests that need a sandbox on Linux have a fleetd with no `bwrap`. They fail on the runner, and they would pass on a developer host only if the host `PATH` is in the sealed environment.


## Windows

Job `build-windows` (`cargo build --workspace --locked`): success.

Job `test-windows` (`cargo test --workspace --locked --no-fail-fast`): failure. **0 tests ran**: the build of the tests did not compile. The counts are 0 passed and 0 failed. Cargo stops at a compile error, even with `--no-fail-fast`.

| Test or step | Class | First error line | file:line |
|---|---|---|---|
| `horch-platform` test `sandbox` | c | ``error[E0433]: cannot find `unix` in `os``` (`std::os::unix::net::UnixListener::bind`) | `crates/horch-platform/tests/sandbox.rs:232` (test `hiding_sandbox_cannot_connect_to_a_unix_socket_in_a_hidden_folder`, from line 226) |
| `horch-core` (lib test), test of `land` | c | ``error[E0433]: cannot find `unix` in `os``` (`use std::os::unix::fs::PermissionsExt`) | `crates/horch-core/src/taskledger/finish/land.rs:1446` |
| same test | c | ``error[E0599]: no method named `set_mode` found for struct `Permissions` `` | `crates/horch-core/src/taskledger/finish/land.rs:1471` |
| `horch-core` (lib test), test of `pr` | c | ``error[E0433]: cannot find `test_support` in the crate root`` (the module has `#[cfg(all(test, unix))]`) | `crates/horch-core/src/taskledger/finish/pr.rs:700` (module at `crates/horch-core/src/lib.rs:73`) |
| `horch-core` (lib test), tests of `codex` | b | ``error[E0425]: cannot find function `prepare_in` in this scope`` (2 uses; the function is unix-only) | `crates/horch-core/src/harness/codex.rs:3312`, `:3417` |
| `horch-core` (lib test), test of the outbox lock | c | ``error[E0433]: cannot find module or crate `libc` in this scope`` (13 uses of `libc`) | `crates/horch-core/src/taskledger/outbox.rs:504-538` |

Total: 16 compile errors in `horch-core` (lib test) and 1 in `horch-platform` (test `sandbox`). 3 warnings do not fail the job: unused import `FORBIDDEN_ENV` at `fleetd/worker_profiles/adapters/launcher.rs:112`, unused import `std::process::Command` at `workspace/herdr.rs:870`, unused variable `round` at `fsx.rs:879`. 2 more warnings are dead code: `HANG_LIMIT_S` at `crates/horch-platform/src/service.rs:36` and `CONTROL` at `crates/agent-docs-sync/tests/changes.rs:244`.

More compile errors can show up in other crates after these are fixed. The crate `horch-platform` stopped the build first ("build failed, waiting for other jobs to finish"), so crates built after it are not checked.

## Job `docs` (`cargo doc --workspace --no-deps --all-features --locked`, `RUSTDOCFLAGS=-D warnings`)

Failure. 33 rustdoc errors, all in `horch-core`, the same 33 as in CI0 (same files, same lines). The first error line of each group is in the column.

| Group | Count | Class | First error line | file:line |
|---|---|---|---|---|
| public doc links to a private item | 16 | d | ``public documentation for `exec_push` links to private item `SAFE_CONFIG` `` | `fleetd/integration/adapters/exec_push.rs:12` (2), `fleetd/integration/application/new_features.rs:130`, `fleetd/task_execution/application/lease.rs:11`, `fleetd/task_execution/application/quota.rs:337`, `fleetd/worktree_custody/adapters/git.rs:509` (2), `:666`, `fleetd/worktree_custody/application/create.rs:234`, `harness/codex.rs:1906`, `roster/validation.rs:1842`, `taskledger/import.rs:1214`, `taskledger/schema.rs:1447`, `:1454`, `taskledger/tasks.rs:61`, `taskledger/workers.rs:782` |
| name is both a function and a module | 9 | d | `` `cancel` is both a function and a module `` | `fleetd/ideas/application/park_cancel.rs:17,70,327`, `fleetd/integration/application/commands.rs:9,141`, `fleetd/task_execution/application/outbox_ingest.rs:11`, `fleetd/task_execution/application/verifier.rs:25,27,466` |
| unclosed HTML tag | 3 | d | ``unclosed HTML tag `n` `` | `fleetd/fleet_runtime/application/jobs.rs:673`, `fleetd/worker_profiles/application/drift_idea.rs:5,6` |
| redundant explicit link target | 3 | d | `redundant explicit link target` | `fleetd/task_execution/application/quota.rs:2,3`, `fleetd/worker_profiles/adapters/launcher.rs:6` |
| unresolved link | 2 | d | ``unresolved link to `create_and_wait` `` | `taskledger/finish/mod.rs:546`, `taskledger/wakes.rs:535` (`held_state`) |

All paths are under `crates/horch-core/src/`. The error at `crates/horch-e2e/src/probe.rs:11` that FIX-INDEX-8 expects does not show: rustdoc stops at `horch-core`, so `horch-e2e` is not documented in this run.

## Job `release-lint` (`cargo clippy --workspace --bins --release --locked -- -D warnings`)

Failure. 1 error, the same as in CI0.

| Step | Class | First error line | file:line |
|---|---|---|---|
| `cargo clippy` | d | ``error: using `chunks_exact` with a constant chunk size`` (`-D clippy::chunks-exact-to-as-chunks`) | `crates/horch-core/src/fleetd/worktree_custody/adapters/git.rs:823` |

## Does `ci.yml` have `timeout-minutes` for the test jobs?

No. `.github/workflows/ci.yml` has 0 lines with `timeout-minutes`. Each job runs to the GitHub limit of 360 min (6 h). A hung test holds the job for 6 h. The job `test (ubuntu-latest)` took 72 min on this run. A `timeout-minutes` of 90 for the test jobs would stop a real hang in 90 min and not in 6 h. This is a note: the ticket forbids an edit of `ci.yml`.

## Difference to CI0

CI0 = run 44680906908 (head `085427fb`). CI1 = this run (head `7fbdc0d3`).

### Per job

| Job | CI0 | CI1 | Change |
|---|---|---|---|
| `test (macos-15)` | 4411 passed, 666 failed | 4796 passed, 270 failed | better: 396 fewer failures, 385 more passes |
| `test (ubuntu-latest)` | 4384 passed, 693 failed | 4691 passed, 375 failed | better: 318 fewer failures, 307 more passes |
| `docs` | failure, 33 errors | failure, 33 errors | still red, no change |
| `release-lint` | failure, 1 error | failure, 1 error | still red, no change |
| `build-windows` | success | success | no change |
| `test-windows` | failure, 18 compile errors, 0 tests | failure, 17 compile errors, 0 tests | still red, 1 error moved |

### Groups of CI0

| Group of CI0 | State in CI1 |
|---|---|
| About 528 macOS tests (523 Linux) that build `horch` and the fakes in the e2e harness with `--profile out` (FIX-1, T-59) | **fixed**. 0 tests of `horch-e2e` fail with the profile error. |
| 38 tests that build only the fakes with `--profile out` | **still red**. They are the 38 tests of 4 files of `horch-core` (`p7_conformance.rs`, `p7_context_limit.rs`, `p7_operator_boundary.rs`, `p7_wrapper_exec.rs`). T-59 changed `crates/horch-e2e/src/harness.rs` only. |
| 27 Linux tests: `bwrap and socat not on PATH` (FIX-2, T-75) | **still red**. The install step now runs and the programs are installed (log lines of the step `Install bubblewrap`). The tests still fail, with the same message, because they give the launch a `PATH` with no `bwrap` and no `socat`. The class changes from (e) to (d). See the section Linux. |
| `docs`: 33 rustdoc errors (FIX-3) | **still red**. The same 33 errors. FIX-3 is not on the run head. |
| `release-lint`: 1 clippy error (FIX-4) | **still red**. The same error. |
| Windows: `horch-marketplace` test `marketplace`, 2 errors (FIX-6, T-79) | **fixed**. The crate compiles on Windows. |
| Windows: `horch-core` lib test, 16 errors (FIX-5) | **still red**. The same 16 errors at the same lines. |
| Windows: next errors after FIX-5 and FIX-6 (FIX-7) | **new**: `crates/horch-platform/tests/sandbox.rs:232` (1 error, class c). More can follow after the `horch-core` errors are fixed. |
| 108 non-build rows of CI0 on macOS (107 plus 1 counted by name) | 75 **fixed**, 33 **still red** (list below). |
| The 7 rows of class (a) of CI0 | 5 still on the list and still fail with another message (list in the macOS section). The 2 others are not red on macOS. |

### The 33 rows of CI0 that are still red on macOS

`a_tree_whose_removal_failed_frees_no_space`, `both_gates_run_again_after_the_rebase`, `check_job_has_no_forbidden_env`, `check_runs_in_the_worker_sandbox_not_with_fleetd_rights`, `claude_worker_check_runs_in_the_platform_sandbox`, `cmd::context::tests::ctx_06_windows_view_lists_claude_and_codex_teammates`, `cmd::messaging::tests::trf_45_note_and_ledger_progress_are_1_command_and_write_no_json`, `cmd::recipes::tests::trf_43_the_steward_task_is_queued_and_its_spawn_has_no_task_text`, `cmd::rescuecmd::tests::drop_deletes_the_ref_and_verifies_it`, `cmd::rescuecmd::tests::worker_caller_is_refused`, `cmd::sessionscmd::tests::trf_44_sessions_shows_a_ledger_worker_with_it`, `cmd::taskledger::tests::no_allowed_ledger_subcommand_takes_a_check_command`, `codex_worker_check_keeps_codex_sandbox`, `ctx_05_prime_launch_records_the_applied_window`, `g52_resume_uses_own_session`, `g53_ledger_handoff_compacts_resumes_and_records_progress_once`, `g64_skill_states_each_delegation_and_reporting_rule_within_budget`, `g70_question_wait_clears_on_orchestrator_answer_and_stalls_restart`, `g70_reviewer_wait_holds_sixty_minutes_then_three_idle_checks_reap`, `g70_waiting_background_deadline_and_dead_process`, `harness::codex::tests::each_launch_gets_only_its_own_rules`, `over_budget_never_removes_an_active_tree_and_holds_dispatch`, `over_budget_removes_landed_and_rescued_trees_of_finished_runs_oldest_first`, `restore_of_a_task_makes_branch_rescue_t_seq_n_and_its_tree_with_each_rescued_file`, `restore_of_a_task_whose_tree_is_removed_uses_its_rescue_ref`, `restore_refuses_when_the_branch_or_the_folder_exists_and_changes_nothing`, `restore_runs_no_hook_of_the_project`, `taskledger::schema::v7_tests::trf_48_every_old_version_migrates_to_current`, `taskledger::stop::tests::trf_36_a_cancel_stops_the_worker_by_its_pids_and_queues_nothing`, `taskledger::stop::tests::trf_36_a_worker_that_cannot_be_stopped_leaves_the_cancel_standing`, `the_legacy_resume_and_assign_arms_run`, `the_legacy_subcommands_write_read_and_report_a_session`, `triage_archive_is_refused_with_a_later_task`.

### New red tests (red in CI1, not red in CI0)

About 190 tests of `horch-e2e` are new in the list of failures. They are not new faults. In CI0 they did not run: each failed at the harness build, and the e2e tests are red on the 1st line that runs a retired command. The names are in the section macOS (groups, with file:line). By file, the macOS counts are:

`taskledger.rs` 61, `lifecycle.rs` 14, `skills_exposure.rs` 17, `p3_refusals.rs` 10, `p3_unlanded.rs` 8, `p3_worktrees.rs` 7, `p2_leases.rs` 7, `fleet18_brief_worktree.rs` 7, `i308_fleet_steward.rs` 6, `fleet18_messaging.rs` 8, `done_budget.rs` 5, `p0_liveness.rs` 4, `fleet18_observability.rs` 13, `fakes.rs` 4, `p0_rescue.rs` 3, `scenario.rs` 2, `p1_release.rs` 2, `p1_pass.rs` 2, `p1_jobs.rs` 2, `p0_supervision.rs` 2, `fleetd.rs` (harness) 2, and 1 each in `tell_grace.rs`, `startup_error.rs` (3), `routing_provenance.rs`, `restored_pane.rs`, `p5_operator_boundary.rs`, `p4_verifier.rs`, `p4_safety.rs`, `p4_release.rs`, `p4_clock_and_heartbeat.rs`, `p3_release.rs`, `p2_release.rs`.

Tests of `horch-core` that are new on macOS: `no_status_literal_in_ledger_sql` (`crates/horch-core/tests/w5_status_words_scan.rs:93`). It was not in the CI0 macOS list.

## Tickets for the failures that are still red

The tickets are `FIX-INDEX-8.md` (101-133) and `FIX-INDEX-9.md` (140-146) in `docs/tickets/fleetd-finish/`.

| Failure | Ticket |
|---|---|
| `docs`, 33 errors | 102 |
| `release-lint`, 1 error | 103 |
| Windows `horch-core` lib test, 16 errors | 104 |
| Windows `horch-marketplace` | 105 (already fixed in CI1) |
| Windows `horch-platform` `sandbox.rs:232` | FIX-14 |
| Linux `bwrap` and `socat` | 101 (see the section Linux) |
| 38 `horch-core` tests that build the fakes with `--profile out` | FIX-11 (ticket 146 owns `p7_conformance.rs`, for another cause: the fake `exit_without_result` at `crates/horch-e2e/src/exec.rs:221`) |
| `no_status_literal_in_ledger_sql` | FIX-15 |
| `taskledger.rs` (61 tests) | 116, 117, 118, 119 (and 120 for the expected-failures list) |
| `lifecycle.rs` | 140 |
| `skills_exposure.rs` | 133 |
| `p3_refusals.rs`, `p3_unlanded.rs`, `p3_worktrees.rs`, `p2_leases.rs`, `p2_release.rs`, `p3_release.rs` | 115 |
| `fleet18_brief_worktree.rs`, `fleet18_messaging.rs`, `done_budget.rs`, `fakes.rs` | 112 |
| `i308_fleet_steward.rs` | 113 |
| `fleet18_observability.rs`, the 5 tests of class (a) | 15, 120 |
| `p0_liveness.rs`, `p0_rescue.rs`, `p0_supervision.rs`, `p1_release.rs` | 114 |
| `p1_pass.rs` (`fleetd.rs:105`, rr4 and rr7), `p1_jobs.rs` | 37, 41, 42, 91 |
| `p4_verifier.rs` | 61 |
| `p5_operator_boundary.rs` | 74 |
| `p4_safety.rs` | 131 |
| `p4_release.rs`, `p4_clock_and_heartbeat.rs` | FIX-16 |
| `scenario.rs`, `startup_error.rs`, `tell_grace.rs`, `restored_pane.rs`, `routing_provenance.rs` | 145 |
| `crates/horch/tests/p3_worktrees_cli.rs` | 109 |
| `crates/horch/tests/p5_cli_writers.rs`, `r_cli_cmds_ledger.rs` | 130 |
| `crates/horch/src/cmd/recipes.rs`, `rescuecmd.rs`, `sessionscmd.rs` | 107 |
| `crates/horch/src/cmd/context.rs`, `messaging.rs`, `taskledger.rs` | 129 |
| `crates/horch-core/tests/check_sandbox.rs` | 126 |
| `crates/horch-core/tests/worktree_disk.rs`, `stack_edges.rs`, `src/taskledger/stop.rs` | 106 |
| `crates/horch-core/src/harness/codex.rs:2761`, `src/taskledger/schema.rs:2279` | 125 |
| Linux: the sandbox `PATH` (`harness.rs:375`, `launch.rs:153`) | FIX-12 and FIX-13 (ticket 101 only edits `ci.yml`, and the install step is not the cause) |
| Linux: the timing tests `store_dirlock_excludes_concurrent_writers`, `line_over_the_limit_is_refused`, `gate_at_its_limit_is_abandoned_and_its_group_killed` | 121, 122, 123 |
| Linux: `p3_triage.rs` (the `no_sandbox` result) | 92 owns the file for another cause (T-55); the cause here is the sandbox `PATH`, so FIX-12 covers it |

## Fix units

Only failures with no ticket. FIX-11 to FIX-16 are new. The numbers FIX-1 to FIX-10 are in CI0.

Each unit is for 1 worker. The order is the order of the value.

| Unit | Fixes | Files it touches |
|---|---|---|
| FIX-12: the sandbox `PATH` of the unit tests | the 27 `horch-core` tests of the sandbox group on Linux (class d) | `crates/horch-core/src/harness/launch.rs` (`LaunchEnv::for_test`, line 153: give `path` a folder with fake `bwrap` and `socat`, as `claude.rs:1665` does for the sandbox tests, or run the tests with the host `PATH`), and the tests `crates/horch-core/tests/execution_plan.rs`, `context_launch.rs`, `baseline_oracles.rs`, `harness_defaults.rs`, `p7_context_256k.rs`, `p7_conformance_job.rs`. Run after tickets 60, 125 and 128: they own `launch.rs` and `baseline_oracles.rs`. Do not read `PATH` from the host in a test that must not use the host. |
| FIX-13: the sandbox `PATH` of the e2e harness | about 55 e2e tests on Linux (the `bwrap` group and the timeouts that wait for a job) | `crates/horch-e2e/src/harness.rs` (`seal`, line 375: add the folder of `bwrap` and `socat` after the folder of the fakes, Linux only). Then run each test of `p3_triage.rs`, `p4_repair.rs`, `p4_no_loss.rs`, `p6_features.rs` 3 times (rule 6). |
| FIX-11: the profile of the fakes in `horch-core` tests | the 38 tests of class d on macOS and Linux | `crates/horch-core/tests/p7_conformance.rs`, `p7_context_limit.rs`, `p7_operator_boundary.rs`, `p7_wrapper_exec.rs` (the `match dir.file_name()` block: use the profile folder as `harness.rs` `cargo_profile` of T-59 does, or share 1 helper in `crates/horch-e2e`). Order: after ticket 146, which owns `p7_conformance.rs`, and after ticket 128, which owns `p7_wrapper_exec.rs`. |
| FIX-14: Windows `horch-platform` test | 1 compile error | `crates/horch-platform/tests/sandbox.rs` (line 226: gate the test `hiding_sandbox_cannot_connect_to_a_unix_socket_in_a_hidden_folder` with `#[cfg(unix)]`). Run it with tickets 104 and 105: the next CI run on Windows needs all 3 to compile. |
| FIX-15: status words in the triage SQL | `no_status_literal_in_ledger_sql` | `crates/horch-core/src/taskledger/triage/mod.rs:201` (the literals `'landed'` and `'abandoned'`: build them from the enums of `taskledger/model.rs`) |
| FIX-16: 2 e2e tests with 2 active runs | `run_without_gates_holds_dispatch_and_loses_no_task` (`horch: 2 active runs ... Name one with --run <slug>`), `pl4_sleep_past_a_branch_deadline_fires_nothing_on_wake` | `crates/horch-e2e/tests/p4_release.rs`, `crates/horch-e2e/tests/p4_clock_and_heartbeat.rs` (pass `--run <slug>`, or close the run that the test does not use) |

Not in the units, on purpose: the 4 timing tests of FIX-10 that are still red on Linux have tickets 121, 122 and 123. The test `an_agent_launch_drops_the_git_repository_variables` (`launch.rs:3134`) fails on Linux only with no error line, and no ticket has it. It goes to ticket 125, which owns `launch.rs`.

After FIX-11 to FIX-16 and tickets 102, 103 and 104: run the CI 1 time more. Do not run it before. A run costs 72 min.
