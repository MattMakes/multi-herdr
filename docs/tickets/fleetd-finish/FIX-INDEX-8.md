# Fix tickets index: CI baseline report (CI0) and red-baseline.txt

Sources covered:
- CI baseline report: `docs/specs/fleetd/gates/ci-baseline-2026-11.md` on `fleet/fleetd-finish` (CI run `44680906908`, commit `085427fba539a050c92c489c07e3d43615342f55`)
- Pre-T-880 red test list: `ai_docs/plans/fleetd-build/red-baseline.txt` (220 rows, commit `adb0bafc`, 2026-11-18)
- Existing FIX-1 ticket: [100-ci-fix1-e2e-harness-profile.md](100-ci-fix1-e2e-harness-profile.md) (no new ticket written for FIX-1)

## Fix tickets

| File | Fix unit | Count of tests | Persona | Own paths | Blocked by |
|---|---|---|---|---|---|
| [101-ci-fix2-linux-socat.md](101-ci-fix2-linux-socat.md) | FIX-2 | 27 | primary `opus`, fallback `codex-sol` | `.github/workflows/ci.yml` |  |
| [102-ci-fix3-rustdoc.md](102-ci-fix3-rustdoc.md) | FIX-3 | 33 rustdoc errors (in 19 `horch-core` files + 1 `horch-e2e` file) | primary `opus`, fallback `codex-sol` | `crates/horch-core/src/fleetd/fleet_runtime/application/jobs.rs`, `crates/horch-core/src/fleetd/ideas/application/park_cancel.rs`, `crates/horch-core/src/fleetd/integration/adapters/exec_push.rs`, `crates/horch-core/src/fleetd/integration/application/commands.rs`, `crates/horch-core/src/fleetd/integration/application/new_features.rs`, `crates/horch-core/src/fleetd/task_execution/application/lease.rs`, `crates/horch-core/src/fleetd/task_execution/application/outbox_ingest.rs`, `crates/horch-core/src/fleetd/task_execution/application/quota.rs`, `crates/horch-core/src/fleetd/task_execution/application/verifier.rs`, `crates/horch-core/src/fleetd/worker_profiles/adapters/launcher.rs`, `crates/horch-core/src/fleetd/worker_profiles/application/drift_idea.rs`, `crates/horch-core/src/fleetd/worktree_custody/adapters/git.rs`, `crates/horch-core/src/fleetd/worktree_custody/application/create.rs`, `crates/horch-core/src/harness/codex.rs`, `crates/horch-core/src/roster/validation.rs`, `crates/horch-core/src/taskledger/finish/mod.rs`, `crates/horch-core/src/taskledger/import.rs`, `crates/horch-core/src/taskledger/schema.rs`, `crates/horch-core/src/taskledger/tasks.rs`, `crates/horch-core/src/taskledger/wakes.rs`, `crates/horch-core/src/taskledger/workers.rs`, `crates/horch-e2e/src/probe.rs` | T-4 (`04-c1-land-t898.md`), T-8 (`08-c5-land-t561.md`), T-10 (`10-c7-land-t895.md`), T-11 (`11-c8-land-t897.md`), T-14 (`14-c11-t900-review-done.md`), 35 (`35-fix-r1-f1-complexity-sweep-sandbox.md`), 51 (`51-fix-r5-f3-judge-verification-plan-no-command-check.md`) |
| [103-ci-fix4-clippy-chunks-exact.md](103-ci-fix4-clippy-chunks-exact.md) | FIX-4 | 1 clippy error | primary `opus`, fallback `codex-sol` | `crates/horch-core/src/fleetd/worktree_custody/adapters/git.rs` | T-4 (`04-c1-land-t898.md`), T-11 (`11-c8-land-t897.md`), 102 |
| [104-ci-fix5-windows-horch-core-tests.md](104-ci-fix5-windows-horch-core-tests.md) | FIX-5 | 16 Windows compile errors (in 4 `horch-core` test modules) | primary `qa-engineer`, fallback `opus` | `crates/horch-core/src/harness/codex.rs`, `crates/horch-core/src/lib.rs`, `crates/horch-core/src/taskledger/finish/land.rs`, `crates/horch-core/src/taskledger/finish/pr.rs`, `crates/horch-core/src/taskledger/outbox.rs` | 102 |
| [105-ci-fix6-windows-horch-marketplace-tests.md](105-ci-fix6-windows-horch-marketplace-tests.md) | FIX-6 | 2 Windows compile errors (1 test) | primary `qa-engineer`, fallback `opus` | `crates/horch-marketplace/tests/marketplace.rs` |  |
| [106-ci-fix8-horch-core-stale-tests.md](106-ci-fix8-horch-core-stale-tests.md) | FIX-8 | 7 | primary `qa-engineer`, fallback `opus` | `crates/horch-core/src/taskledger/stop.rs`, `crates/horch-core/tests/context_prime.rs`, `crates/horch-core/tests/stack_edges.rs`, `crates/horch-core/tests/worktree_disk.rs` | T-13 (`13-c10-land-t893.md`) |
| [107-ci-fix8-horch-bin-unit-stale-tests.md](107-ci-fix8-horch-bin-unit-stale-tests.md) | FIX-8 | 4 | primary `qa-engineer`, fallback `opus` | `crates/horch/src/cmd/recipes.rs`, `crates/horch/src/cmd/rescuecmd.rs`, `crates/horch/src/cmd/sessionscmd.rs` | T-10 (`10-c7-land-t895.md`) |
| [108-ci-fix8-horch-cli-context-ledger-add-map.md](108-ci-fix8-horch-cli-context-ledger-add-map.md) | FIX-8 | 8 | primary `qa-engineer`, fallback `opus` | `crates/horch/tests/context_cli.rs`, `crates/horch/tests/ledger_cli.rs`, `crates/horch/tests/trf_11_add_map_cli.rs` |  |
| [109-ci-fix8-horch-cli-p3-p4.md](109-ci-fix8-horch-cli-p3-p4.md) | FIX-8 | 19 (18 class `f` + 1 class `f?`) | primary `qa-engineer`, fallback `opus` | `crates/horch/tests/p3_worktrees_cli.rs`, `crates/horch/tests/p4_question_file.rs`, `crates/horch/tests/p4_result_cli.rs` |  |
| [110-ci-fix8-horch-cli-p5-routes.md](110-ci-fix8-horch-cli-p5-routes.md) | FIX-8 | 14 (6 class `f` + 8 class `f?`) | primary `qa-engineer`, fallback `opus` | `crates/horch/tests/p5_route_ledger_run.rs`, `crates/horch/tests/p5_route_ledger_task.rs`, `crates/horch/tests/p5_route_operator_commands.rs` |  |
| [111-ci-fix8-horch-cli-p6-w3-pins.md](111-ci-fix8-horch-cli-p6-w3-pins.md) | FIX-8 | 13 (12 class `f` + 1 class `f?`) | primary `qa-engineer`, fallback `opus` | `crates/horch/tests/p6_retired_run_commands.rs`, `crates/horch/tests/p6_retired_worker_commands.rs`, `crates/horch/tests/w3_ledger_cli_pins.rs` |  |
| [112-ci-fix8-e2e-done-fakes-fleet18.md](112-ci-fix8-e2e-done-fakes-fleet18.md) | FIX-8 | 15 (11 `red-baseline` + 3 class `f` + 1 class `f?`) | primary `qa-engineer`, fallback `opus` | `crates/horch-e2e/tests/done_budget.rs`, `crates/horch-e2e/tests/fakes.rs`, `crates/horch-e2e/tests/fleet18_brief_worktree.rs`, `crates/horch-e2e/tests/fleet18_messaging.rs` | 100 (`100-ci-fix1-e2e-harness-profile.md`), T-11 (`11-c8-land-t897.md`) |
| [113-ci-fix8-e2e-i308-skills-watchdog-worktree.md](113-ci-fix8-e2e-i308-skills-watchdog-worktree.md) | FIX-8 | 18 (18 `red-baseline`, 1 also class `f?`) | primary `qa-engineer`, fallback `opus` | `crates/horch-e2e/tests/i308_fleet_steward.rs`, `crates/horch-e2e/tests/i308_triage.rs`, `crates/horch-e2e/tests/skill_intake_spawn.rs`, `crates/horch-e2e/tests/watchdog_liveness.rs`, `crates/horch-e2e/tests/worktree.rs` | 100 (`100-ci-fix1-e2e-harness-profile.md`) |
| [114-ci-fix8-e2e-p0-p1-release.md](114-ci-fix8-e2e-p0-p1-release.md) | FIX-8 | 14 | primary `qa-engineer`, fallback `opus` | `crates/horch-e2e/tests/p0_dispatch.rs`, `crates/horch-e2e/tests/p0_landing.rs`, `crates/horch-e2e/tests/p0_liveness.rs`, `crates/horch-e2e/tests/p0_rescue.rs`, `crates/horch-e2e/tests/p0_supervision.rs`, `crates/horch-e2e/tests/p1_release.rs` | 100 (`100-ci-fix1-e2e-harness-profile.md`) |
| [115-ci-fix8-e2e-p2-p3.md](115-ci-fix8-e2e-p2-p3.md) | FIX-8 | 34 | primary `qa-engineer`, fallback `opus` | `crates/horch-e2e/tests/p2_leases.rs`, `crates/horch-e2e/tests/p2_release.rs`, `crates/horch-e2e/tests/p3_refusals.rs`, `crates/horch-e2e/tests/p3_release.rs`, `crates/horch-e2e/tests/p3_unlanded.rs`, `crates/horch-e2e/tests/p3_worktrees.rs` | 100 (`100-ci-fix1-e2e-harness-profile.md`), T-11 (`11-c8-land-t897.md`) |
| [116-ci-fix8-e2e-taskledger-part1.md](116-ci-fix8-e2e-taskledger-part1.md) | FIX-8 | 16 | primary `qa-engineer`, fallback `opus` | `crates/horch-e2e/tests/taskledger.rs` | 100 (`100-ci-fix1-e2e-harness-profile.md`), T-13 (`13-c10-land-t893.md`), T-14 (`14-c11-t900-review-done.md`) |
| [117-ci-fix8-e2e-taskledger-part2.md](117-ci-fix8-e2e-taskledger-part2.md) | FIX-8 | 24 | primary `qa-engineer`, fallback `opus` | `crates/horch-e2e/tests/taskledger.rs` | 116 |
| [118-ci-fix8-e2e-taskledger-part3.md](118-ci-fix8-e2e-taskledger-part3.md) | FIX-8 | 12 | primary `qa-engineer`, fallback `opus` | `crates/horch-e2e/tests/taskledger.rs` | 117 |
| [119-ci-fix8-e2e-taskledger-part4.md](119-ci-fix8-e2e-taskledger-part4.md) | FIX-8 | 11 | primary `qa-engineer`, fallback `opus` | `crates/horch-e2e/tests/taskledger.rs` | 118 |
| [120-ci-fix9-expected-failures.md](120-ci-fix9-expected-failures.md) | FIX-9 | 7 | primary `qa-engineer`, fallback `opus` | `crates/horch-e2e/expected-failures/fleet18_observability.txt`, `crates/horch-e2e/expected-failures/taskledger.txt`, `crates/horch-e2e/tests/expected_failures.rs` | 100 (`100-ci-fix1-e2e-harness-profile.md`), T-13 (`13-c10-land-t893.md`), T-14 (`14-c11-t900-review-done.md`), T-15 (`15-c12-t882-observability.md`), 119 |
| [121-ci-fix10-timing-gate-jobs.md](121-ci-fix10-timing-gate-jobs.md) | FIX-10 | 1 | primary `qa-engineer`, fallback `opus` | `crates/horch-core/tests/gate_jobs.rs` |  |
| [122-ci-fix10-timing-execution-store.md](122-ci-fix10-timing-execution-store.md) | FIX-10 | 1 | primary `qa-engineer`, fallback `opus` | `crates/horch-core/src/execution/store.rs`, `crates/horch-core/tests/execution_store.rs` | T-5 (`05-c2-t899-wrapper-operator.md`), 70 (`70-fix-r4-f1-queued-request-refusal-visibility.md`), 74 (`74-fix-r4-tests-prove-rules.md`) |
| [123-ci-fix10-timing-fleetd-channel.md](123-ci-fix10-timing-fleetd-channel.md) | FIX-10 | 1 | primary `qa-engineer`, fallback `opus` | `crates/horch-core/src/fleetd/fleet_runtime/adapters/channel_server.rs`, `crates/horch-core/tests/fleetd_channel.rs` | T-4 (`04-c1-land-t898.md`), 36 (`36-fix-r1-f2-windows-channel-bounded-write.md`), 71 (`71-fix-r4-f2-channel-server-stop-without-socket-path.md`) |
| [124-ci-fix10-timing-fleetd-pass.md](124-ci-fix10-timing-fleetd-pass.md) | FIX-10 | 1 | primary `qa-engineer`, fallback `opus` | `crates/horch-core/tests/fleetd_pass.rs` |  |
| [125-ci-fix10-horch-core-lib-tests.md](125-ci-fix10-horch-core-lib-tests.md) | FIX-10 | 3 | primary `qa-engineer`, fallback `opus` | `crates/horch-core/src/harness/codex.rs`, `crates/horch-core/src/harness/launch.rs`, `crates/horch-core/src/taskledger/schema.rs` | 60 (`60-fix-r3-f2-antigravity-exec-stop-hook.md`), 102, 104 |
| [126-ci-fix10-horch-core-check-sandbox.md](126-ci-fix10-horch-core-check-sandbox.md) | FIX-10 | 4 | primary `qa-engineer`, fallback `opus` | `crates/horch-core/tests/check_sandbox.rs` | 35 (`35-fix-r1-f1-complexity-sweep-sandbox.md`) |
| [127-ci-fix10-horch-core-custody-p3-merges-status.md](127-ci-fix10-horch-core-custody-p3-merges-status.md) | FIX-10 | 10 | primary `qa-engineer`, fallback `opus` | `crates/horch-core/tests/fleetd_handlers_custody.rs`, `crates/horch-core/tests/fleetd_pass_p3.rs`, `crates/horch-core/tests/trial_merges.rs`, `crates/horch-core/tests/w3_finish_steps.rs`, `crates/horch-core/tests/w3_taskledger_core_status.rs` |  |
| [128-ci-fix10-horch-core-roster-harness-template.md](128-ci-fix10-horch-core-roster-harness-template.md) | FIX-10 | 8 | primary `qa-engineer`, fallback `opus` | `crates/horch-core/tests/baseline_oracles.rs`, `crates/horch-core/tests/fr5_push_only.rs`, `crates/horch-core/tests/harness.rs`, `crates/horch-core/tests/intake_roster.rs`, `crates/horch-core/tests/p5_profile_validation.rs`, `crates/horch-core/tests/p7_wrapper_exec.rs`, `crates/horch-core/tests/trf_02_ticket_template.rs` |  |
| [129-ci-fix10-horch-bin-unit-unexplained.md](129-ci-fix10-horch-bin-unit-unexplained.md) | FIX-10 | 4 (3 class `f?` + 1 `red-baseline`) | primary `qa-engineer`, fallback `opus` | `crates/horch/src/cmd/context.rs`, `crates/horch/src/cmd/messaging.rs`, `crates/horch/src/cmd/taskledger.rs`, `crates/horch/src/main.rs` | T-11 (`11-c8-land-t897.md`) |
| [130-ci-fix10-horch-cli-writers-legacy-ledger.md](130-ci-fix10-horch-cli-writers-legacy-ledger.md) | FIX-10 | 3 | primary `qa-engineer`, fallback `opus` | `crates/horch/tests/p5_cli_writers.rs`, `crates/horch/tests/r_cli_cmds_ledger.rs` | T-11 (`11-c8-land-t897.md`) |
| [131-ci-fix10-e2e-p4-safety.md](131-ci-fix10-e2e-p4-safety.md) | FIX-10 | 1 | primary `qa-engineer`, fallback `opus` | `crates/horch-e2e/tests/p4_safety.rs` | 100 (`100-ci-fix1-e2e-harness-profile.md`), T-17 (`17-d2-sr3-external-message.md`) |
| [132-ci-fix10-e2e-p7-docs-sync-release.md](132-ci-fix10-e2e-p7-docs-sync-release.md) | FIX-10 | 2 | primary `qa-engineer`, fallback `opus` | `crates/horch-e2e/tests/p7_docs_sync.rs`, `crates/horch-e2e/tests/p7_release.rs` | 100 (`100-ci-fix1-e2e-harness-profile.md`) |
| [133-ci-fix10-e2e-skills-exposure-fleet-start-ideas.md](133-ci-fix10-e2e-skills-exposure-fleet-start-ideas.md) | FIX-10 | 4 (`red-baseline`) | primary `qa-engineer`, fallback `opus` | `crates/horch-e2e/tests/fleet_start.rs`, `crates/horch-e2e/tests/p0_ideas.rs`, `crates/horch-e2e/tests/skills_exposure.rs` | 100 (`100-ci-fix1-e2e-harness-profile.md`) |

## Shared file and sequencing notes

- **`crates/horch-e2e/tests/taskledger.rs` sequence (tickets 116 -> 117 -> 118 -> 119 -> 120)**:
  - Tickets [116-ci-fix8-e2e-taskledger-part1.md](116-ci-fix8-e2e-taskledger-part1.md), [117-ci-fix8-e2e-taskledger-part2.md](117-ci-fix8-e2e-taskledger-part2.md), [118-ci-fix8-e2e-taskledger-part3.md](118-ci-fix8-e2e-taskledger-part3.md), and [119-ci-fix8-e2e-taskledger-part4.md](119-ci-fix8-e2e-taskledger-part4.md) share `crates/horch-e2e/tests/taskledger.rs` and must run in sequence: 116 is `Blocked by` 100, T-13 (`13-c10-land-t893.md`), and T-14 (`14-c11-t900-review-done.md`); 117 is `Blocked by: 116`; 118 is `Blocked by: 117`; 119 is `Blocked by: 118`; and 120 (FIX-9) is `Blocked by: 100, T-13, T-14, T-15, 119`.
- **FIX-9 (`120-ci-fix9-expected-failures.md`) and `MAP.md` tickets 06 to 15**:
  - `crates/horch-e2e/expected-failures/taskledger.txt` is in `Own` of tickets 13 (`13-c10-land-t893.md`, T-13) and 14 (`14-c11-t900-review-done.md`, T-14) of `MAP.md`.
  - `crates/horch-e2e/expected-failures/fleet18_observability.txt` is in `Own` of ticket 15 (`15-c12-t882-observability.md`, T-15) of `MAP.md`.
  - Therefore ticket 120 is marked `Blocked by: 100, T-13, T-14, T-15, 119`.
- **FIX-3 (`102-ci-fix3-rustdoc.md`), FIX-4 (`103-ci-fix4-clippy-chunks-exact.md`), FIX-5 (`104-ci-fix5-windows-horch-core-tests.md`), and FIX-10 (`125-ci-fix10-horch-core-lib-tests.md`)**:
  - Ticket 102 is marked `Blocked by: T-4` (plus T-8, T-10, T-11, T-14, 35, 51 for the specific files under `crates/horch-core/src/fleetd/` and `crates/horch-core/src/taskledger/`).
  - Ticket 103 shares `crates/horch-core/src/fleetd/worktree_custody/adapters/git.rs` with T-4, T-11, and ticket 102, so it is marked `Blocked by: T-4, T-11, 102`.
  - Ticket 104 shares `crates/horch-core/src/harness/codex.rs` with ticket 102, so it is marked `Blocked by: 102`.
  - Ticket 125 shares `crates/horch-core/src/harness/codex.rs` and `crates/horch-core/src/taskledger/schema.rs` with tickets 102 and 104 (and `crates/horch-core/src/harness/launch.rs` with ticket 60), so it is marked `Blocked by: 60, 102, 104`.
- **No 2 tickets that can run at the same time share an `Own` path**: every shared file across tickets 101 to 133 and existing tickets in `MAP.md` and `FIX-INDEX-1.md` through `FIX-INDEX-7.md` has an explicit `Blocked by` chain in the table above.

## Rows of the CI report or of `red-baseline.txt` that have no new ticket here, with the reason

### 1. Covered by FIX-1 (`100-ci-fix1-e2e-harness-profile.md`) or FIX-7 (orchestrator CI run)

- **FIX-1 (2 group rows in `ci-baseline-2026-11.md`: ~528 tests on macOS / ~523 on Linux that build `horch` and the fakes, and 38 tests that build only the fakes, failing with `--profile out` at `crates/horch-e2e/src/harness.rs:963` and `:1012-1016`)**:
  - **Reason**: Already covered by [100-ci-fix1-e2e-harness-profile.md](100-ci-fix1-e2e-harness-profile.md) (task T-59). The brief instructs: "Write no ticket for FIX-1."
- **FIX-7 (Windows next errors after FIX-5 and FIX-6)**:
  - **Reason**: Done by the orchestrator on the next CI run (`workflow_dispatch`). The brief instructs: "write no ticket for FIX-7".

### 2. Skipped per the brief because an existing ticket already owns the file

1. **`crates/horch-e2e/tests/p1_pass.rs` (4 rows in `red-baseline.txt`: rows 105-108)** — Covered by [37-fix-r1-f3-pass-loop-event-wake.md](37-fix-r1-f3-pass-loop-event-wake.md) (T-39, `FIX-INDEX-1.md`), and already migrated to `RunSeed` on `fleet/fleetd-finish`:
   - `scans_of_10000_folders_keep_the_heartbeat_under_30_s_and_go_on_at_their_cursors_after_a_restart`
   - `fm11_fleetd_aborted_after_the_commit_does_each_effect_once_on_restart`
   - `rr4_tasks_complete_with_all_file_watch_events_dropped`
   - `rr7_fleetd_killed_mid_run_ends_as_a_clean_run`
2. **`crates/horch-e2e/tests/fleet18_observability.rs` (6 rows in `red-baseline.txt`: rows 70-75)** — Covered by [15-c12-t882-observability.md](15-c12-t882-observability.md) (T-15, `MAP.md`), and `g52_concurrent_workers_get_their_own_session_and_transcript` + `g54_doctor_missing_mcp_and_hanging_docker_are_warnings` were already fixed in `7532a60d`:
   - `g50_live_foreign_runs_and_legacy_orchestrators_warn_but_do_not_block`
   - `g53_ledger_handoff_compacts_resumes_and_records_progress_once`
   - `g51_sessions_points_to_ledger_status_only_for_the_callers_active_run`
   - `g54_doctor_missing_mcp_and_hanging_docker_are_warnings`
   - `g52_concurrent_workers_get_their_own_session_and_transcript`
   - `ledger_stop_stops_a_spawned_worker_by_the_ingested_pid`
3. **`crates/horch-e2e/tests/p3_triage.rs` (10 rows in `red-baseline.txt`: rows 130-139)** — Covered by [92-fix-weak-tests-r2-multi-item-triage-e2e.md](92-fix-weak-tests-r2-multi-item-triage-e2e.md) (T-55, `FIX-INDEX-7.md`):
   - `a_folder_at_the_path_of_a_removed_row_is_a_new_orphaned_row_and_nothing_is_removed`
   - `rr17_triage_refuses_a_unique_detached_head_and_the_tree_stays`
   - `rr16_failed_read_back_keeps_the_folder`
   - `rr17_triage_deletes_a_branch_only_when_the_trash_ref_holds_its_tip`
   - `pres_04_w9_triage_commits_untracked_before_delete`
   - `pres_04_w8_refs_move_forward_and_are_read_back`
   - `restore_runs_no_hook_of_the_project`
   - `trash_ref_older_than_30_days_is_collected_and_a_rescue_ref_is_not`
   - `restore_makes_its_tree_through_a_worktree_add_job`
   - `rr16_second_rescue_through_triage_does_not_overwrite_the_first_ref`
4. **`crates/horch-e2e/tests/p1_jobs.rs` (2 rows in `red-baseline.txt`: rows 103-104)** — Covered by [91-fix-weak-tests-r1-fm12-long-job-and-channel.md](91-fix-weak-tests-r1-fm12-long-job-and-channel.md) (T-54, `FIX-INDEX-7.md`), which explicitly migrates `checked_task` in `p1_jobs.rs` to `RunSeed`:
   - `job_past_its_limit_runs_again_after_its_backoff_and_after_3_abandoned_runs_goes_to_you`
   - `job_of_5_min_runs_while_fleetd_stays_healthy_and_is_not_restarted`

### 3. Rows of `red-baseline.txt` already deleted or migrated between `adb0bafc` (2026-11-18) and `fleet/fleetd-finish` (16 rows)

1. **`horch | trf_11_add_map_cli.rs | trf_12_claim_prints_the_snapshot_after_the_ticket_changed_or_went` (`red-baseline.txt` row 32)**:
   - **Reason**: Removed in commit `947c2f2f` (`T-880: CLI tests of routed commands run with a test fleetd`) because Phase 4/5 retired `horch ledger claim` under `FR-5`.
2. **`horch | trf_37_ledger_handoff_cli.rs | trf_37_handoff_requeues_the_task_and_the_next_claim_reads_the_handoff_first` (`red-baseline.txt` row 36)**:
   - **Reason**: File `crates/horch/tests/trf_37_ledger_handoff_cli.rs` was removed in commit `947c2f2f` (T-880) because Phase 4/5 replaced claim-based handoff with push-attempt `outbox/handoff.json`.
3. **`horch | w3_ledger_cli_pins.rs` — 4 tests removed in commit `947c2f2f` (T-880) (`red-baseline.txt` rows 40, 41, 44, 49)**:
   - `claim_prints_the_line_to_send_the_ticket_and_the_project_facts`
   - `a_worker_claims_makes_a_worktree_and_finishes`
   - `spawn_checks_the_run_the_persona_and_the_replaced_worker`
   - `a_push_attempt_claims_nothing_and_finish_and_done_refuse`
   - **Reason**: Removed in commit `947c2f2f` (T-880) when `horch ledger claim` and `horch spawn` were retired under `FR-5`.
4. **`horch-e2e | fleet18_brief_worktree.rs | g56_facts_reach_claim_and_launch_context_and_are_project_scoped` (`red-baseline.txt` row 65)**:
   - **Reason**: Renamed and migrated in commit `7c8e0086` (`T-886`) to `g56_facts_reach_the_launch_context_and_are_project_scoped` (without `horch ledger run new` or `horch ledger claim`), which passes on `fleet/fleetd-finish`.
5. **`horch-e2e | fleet18_messaging.rs` — 4 tests removed in commit `947c2f2f` (T-880) (`red-baseline.txt` rows 66-69)**:
   - `g60_ledger_done_and_wake_queue_without_registration_then_courier_delivers`
   - `g62_ledger_wake_uses_run_workspace_and_records_login_queue_cause`
   - `g70_finish_cli_creates_reviewer_wait_and_background_progress_cli_sets_deadline`
   - `ledger_done_finds_the_orchestrator_in_a_copied_old_registry`
   - **Reason**: Removed in commit `947c2f2f` (T-880) because Phase 5 replaced claim-attempt `horch ledger done` / wake CLI calls with attempt outbox files.
6. **`horch-e2e | i308_triage.rs | finish_queues_a_task_that_inherits_the_worktree_and_wakes_the_orchestrator` (`red-baseline.txt` row 85)**:
   - **Reason**: Removed in commit `947c2f2f` (T-880).
7. **`horch-e2e | p0_landing.rs | two_conflicting_features_keep_their_tips_and_reach_a_decision` (`red-baseline.txt` row 93)**:
   - **Reason**: Already fixed and made green in commit `28be192d` (`fleetd-p6 55: the p0_landing test of 2 conflicting features is green`).
8. **`horch-e2e | taskledger.rs` — 4 claim tests removed in commit `7c8e0086` (`T-886`) (`red-baseline.txt` rows 163, 164, 168, 198)**:
   - `a_claim_names_an_ignored_ticket_by_an_absolute_path_that_opens_from_the_worktree`
   - `a_claim_starts_no_watchdog`
   - `a_claim_names_a_tracked_ticket_by_its_relative_path`
   - `eight_parallel_claim_processes_take_exactly_three_tasks`
   - **Reason**: Removed in commit `7c8e0086` (`T-886: horch-e2e and horch CLI tests bind their task with the bind helper (FR-5), not horch ledger claim`).

## Open questions

1. **Additional rustdoc error in `crates/horch-e2e/src/probe.rs:11` after T-898 (T-4)**: In addition to the 19 `crates/horch-core/src/` files from `ci-baseline-2026-11.md`, `crates/horch-e2e/src/probe.rs:11` (added by T-898) has 1 `rustdoc::private_intra_doc_links` error (`[\`operator_boundary\`]`). Ticket [102-ci-fix3-rustdoc.md](102-ci-fix3-rustdoc.md) includes `crates/horch-e2e/src/probe.rs` in `**Own:**` and is blocked by T-4 so `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features --locked` passes across the whole workspace.
2. **Class `f?` rows in the CI report that are actually class `f` retired-command or e2e harness-build failures**:
   - The 4 rows in `crates/horch/tests/p5_route_ledger_run.rs` and 4 rows in `crates/horch/tests/p5_route_ledger_task.rs` fail with `assert_eq failed: ["ledger", "run", "new", "r1"]` (`horch ledger run new is retired`) and are grouped into ticket [110-ci-fix8-horch-cli-p5-routes.md](110-ci-fix8-horch-cli-p5-routes.md).
   - `question_of_a_push_attempt_writes_1_question_file` in `crates/horch/tests/p4_question_file.rs:186` fails at the same `w.ok(&["ledger", "run", "new", "r1"])` line as the other 3 tests in `p4_question_file.rs` and is grouped into ticket [109-ci-fix8-horch-cli-p3-p4.md](109-ci-fix8-horch-cli-p3-p4.md).
   - The 4 `horch-e2e/src/harness.rs:963` rows in the CI report (`arc_31_e2e_a_ledger_worktree_is_not_unregistered`, `fr5_worker_never_runs_a_task_search`, `next_attempt_of_its_task_runs_in_exec_mode`, and `operator_boundary_passes_for_each_adapter`) failed on CI because `harness.rs:963` built the fakes with `--profile out` (FIX-1); `arc_31_e2e_a_ledger_worktree_is_not_unregistered` also calls `horch ledger run new` and is covered in ticket [113-ci-fix8-e2e-i308-skills-watchdog-worktree.md](113-ci-fix8-e2e-i308-skills-watchdog-worktree.md), while the other 3 are verified 3 times in tickets [131-ci-fix10-e2e-p4-safety.md](131-ci-fix10-e2e-p4-safety.md) and [132-ci-fix10-e2e-p7-docs-sync-release.md](132-ci-fix10-e2e-p7-docs-sync-release.md).
