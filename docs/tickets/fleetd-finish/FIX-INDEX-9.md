# Fix Index 9 — Local Baseline Red Tests Without a Ticket

Source baseline: `git -C /Users/matthewscott/repo/multi-herdr show fleet/fleetd-finish:docs/specs/fleetd/gates/baseline-2026-11.md` (`## Failing tests`, 361 failing tests across 65 test binaries/files).

## Step 1: Coverage Table of the 361 Red Tests of the Baseline

A ticket covers a test when it names the test, or when it states that it repairs all red tests of the test file.

> **Note on 2 crate labels in `baseline-2026-11.md`:**
> - `baseline-2026-11.md` lists `tests/messaging.rs` (11 red tests) under `### horch-core (41 failing)`, because both `horch-core` and `horch-e2e` have a `tests/messaging.rs` target. All 11 failing tests live in `crates/horch-e2e/tests/messaging.rs`.
> - `baseline-2026-11.md` lists `tests/channel.rs` (4 red tests) under `### horch-platform (4 failing)`, because both `horch-platform` and `horch-e2e` have a `tests/channel.rs` target. All 4 failing tests live in `crates/horch-e2e/tests/channel.rs`.

| Baseline section | Test file | Red tests | Covering ticket(s) (before `140`–`146`) | Not covered (before `140`–`146`) | Action |
|---|---|---:|---|---:|---|
| `horch-e2e` | `crates/horch-e2e/tests/brief.rs` | 1 | `none` | 1 | New ticket `145` |
| `horch-e2e` | `crates/horch-e2e/tests/done_budget.rs` | 5 | `112` (names 2) | 3 | Add 3 tests to ticket `112` |
| `horch-e2e` | `crates/horch-e2e/tests/e2e.rs` | 12 | `none` | 12 | New ticket `141` |
| `horch-e2e` | `crates/horch-e2e/tests/fakes.rs` | 4 | `112` (names all 4) | 0 | Covered |
| `horch-e2e` | `crates/horch-e2e/tests/fleet18_brief_worktree.rs` | 9 | `112` (names 6) | 3 | Add 3 tests to ticket `112` |
| `horch-e2e` | `crates/horch-e2e/tests/fleet18_delivery_probe.rs` | 3 | `none` | 3 | New ticket `143` |
| `horch-e2e` | `crates/horch-e2e/tests/fleet18_messaging.rs` | 10 | `112` (names 3; also `11`) | 7 | Add 7 tests to ticket `112` |
| `horch-e2e` | `crates/horch-e2e/tests/fleet18_observability.rs` | 11 | `15`, `120` (names 2) | 9 | Add 9 tests to ticket `15` (or `120`) |
| `horch-e2e` | `crates/horch-e2e/tests/fleet_start.rs` | 1 | `133` (owns file, names another test) | 1 | Add 1 test to ticket `133` |
| `horch-e2e` | `crates/horch-e2e/tests/i308_fleet_steward.rs` | 8 | `113` (names 7) | 1 | Add 1 test to ticket `113` |
| `horch-e2e` | `crates/horch-e2e/tests/i308_triage.rs` | 4 | `113` (names 3) | 1 | Add 1 test to ticket `113` |
| `horch-e2e` | `crates/horch-e2e/tests/lifecycle.rs` | 14 | `none` | 14 | New ticket `140` |
| `horch-e2e` | `crates/horch-e2e/tests/p0_liveness.rs` | 4 | `114` (names all 4) | 0 | Covered |
| `horch-e2e` | `crates/horch-e2e/tests/p0_rescue.rs` | 3 | `114` (names all 3) | 0 | Covered |
| `horch-e2e` | `crates/horch-e2e/tests/p0_supervision.rs` | 2 | `114` (names all 2) | 0 | Covered |
| `horch-e2e` | `crates/horch-e2e/tests/p1_jobs.rs` | 2 | `91` (names all 2) | 0 | Covered |
| `horch-e2e` | `crates/horch-e2e/tests/p1_pass.rs` | 4 | `37`, `41`, `42` (name all 4) | 0 | Covered |
| `horch-e2e` | `crates/horch-e2e/tests/p1_release.rs` | 2 | `114` (names all 2) | 0 | Covered |
| `horch-e2e` | `crates/horch-e2e/tests/p2_leases.rs` | 7 | `115`, `11` (names all 7) | 0 | Covered |
| `horch-e2e` | `crates/horch-e2e/tests/p2_release.rs` | 1 | `115` (names 1) | 0 | Covered |
| `horch-e2e` | `crates/horch-e2e/tests/p3_refusals.rs` | 10 | `115` (names all 10) | 0 | Covered |
| `horch-e2e` | `crates/horch-e2e/tests/p3_release.rs` | 1 | `115` (names 1) | 0 | Covered |
| `horch-e2e` | `crates/horch-e2e/tests/p3_unlanded.rs` | 8 | `115` (names all 8) | 0 | Covered |
| `horch-e2e` | `crates/horch-e2e/tests/p3_worktrees.rs` | 7 | `115` (names all 7) | 0 | Covered |
| `horch-e2e` | `crates/horch-e2e/tests/p4_safety.rs` | 1 | `131`, `17` (names 1) | 0 | Covered |
| `horch-e2e` | `crates/horch-e2e/tests/p4_verifier.rs` | 4 | `61` (owns file, names new test only) | 4 | Add 4 tests to ticket `61` |
| `horch-e2e` | `crates/horch-e2e/tests/p5_operator_boundary.rs` | 1 | `04`, `05`, `74` (own file, do not name test) | 1 | Add 1 test to ticket `74` |
| `horch-e2e` | `crates/horch-e2e/tests/p5_read_only_worker.rs` | 3 | `none` | 3 | New ticket `144` |
| `horch-e2e` | `crates/horch-e2e/tests/p5_release.rs` | 3 | `none` | 3 | New ticket `144` |
| `horch-e2e` | `crates/horch-e2e/tests/p7_docs_sync.rs` | 3 | `132` (owns file, names another test) | 3 | Add 3 tests to ticket `132` |
| `horch-e2e` | `crates/horch-e2e/tests/relay_first_delivery.rs` | 1 | `none` | 1 | New ticket `143` |
| `horch-e2e` | `crates/horch-e2e/tests/restored_pane.rs` | 1 | `none` | 1 | New ticket `145` |
| `horch-e2e` | `crates/horch-e2e/tests/routing_provenance.rs` | 1 | `none` | 1 | New ticket `145` |
| `horch-e2e` | `crates/horch-e2e/tests/scenario.rs` | 2 | `none` | 2 | New ticket `145` |
| `horch-e2e` | `crates/horch-e2e/tests/skill_intake_spawn.rs` | 5 | `113` (names all 5) | 0 | Covered |
| `horch-e2e` | `crates/horch-e2e/tests/skills_exposure.rs` | 17 | `133` (names 2) | 15 | Add 15 tests to ticket `133` |
| `horch-e2e` | `crates/horch-e2e/tests/startup_error.rs` | 3 | `none` | 3 | New ticket `145` |
| `horch-e2e` | `crates/horch-e2e/tests/taskledger.rs` | 61 | `116`, `117`, `118`, `119` (name 59; also `13`, `14`) | 2 | Add 2 tests to ticket `116` |
| `horch-e2e` | `crates/horch-e2e/tests/tell_grace.rs` | 1 | `none` | 1 | New ticket `145` |
| `horch-e2e` | `crates/horch-e2e/tests/watchdog_liveness.rs` | 6 | `113` (names 1) | 5 | Add 5 tests to ticket `113` |
| `horch-e2e` | `crates/horch-e2e/tests/worktree.rs` | 3 | `113` (names 1) | 2 | Add 2 tests to ticket `113` |
| `horch` | `crates/horch/tests/context_cli.rs` | 3 | `108` (names all 3) | 0 | Covered |
| `horch` | `crates/horch/tests/ledger_cli.rs` | 2 | `108` (names all 2) | 0 | Covered |
| `horch` | `crates/horch/tests/p3_worktrees_cli.rs` | 9 | `109` (names all 9) | 0 | Covered |
| `horch` | `crates/horch/tests/p4_question_file.rs` | 4 | `109` (names all 4) | 0 | Covered |
| `horch` | `crates/horch/tests/p4_result_cli.rs` | 6 | `109` (names all 6) | 0 | Covered |
| `horch` | `crates/horch/tests/p5_cli_writers.rs` | 1 | `130` (names 1) | 0 | Covered |
| `horch` | `crates/horch/tests/p5_route_ledger_run.rs` | 4 | `110` (names all 4) | 0 | Covered |
| `horch` | `crates/horch/tests/p5_route_ledger_task.rs` | 4 | `110` (names all 4) | 0 | Covered |
| `horch` | `crates/horch/tests/p5_route_operator_commands.rs` | 6 | `110` (names all 6) | 0 | Covered |
| `horch` | `crates/horch/tests/p6_retired_run_commands.rs` | 1 | `111`, `47` (names 1) | 0 | Covered |
| `horch` | `crates/horch/tests/p6_retired_worker_commands.rs` | 1 | `111`, `47` (names 1) | 0 | Covered |
| `horch` | `crates/horch/tests/r_cli_cmds_ledger.rs` | 2 | `130` (names all 2) | 0 | Covered |
| `horch` | `crates/horch/tests/trf_11_add_map_cli.rs` | 3 | `108` (names all 3) | 0 | Covered |
| `horch` | `crates/horch/tests/w3_ledger_cli_pins.rs` | 10 | `111` (names all 10) | 0 | Covered |
| `horch-core` | `crates/horch-core/tests/baseline_oracles.rs` | 2 | `101` (`arc_09_argv_matches_baseline`), `128` (`oracle_launch_matches`) | 0 | Covered (see note under ticket `128` below) |
| `horch-core` | `crates/horch-core/tests/check_sandbox.rs` | 4 | `126` (names all 4) | 0 | Covered |
| `horch-core` | `crates/horch-core/tests/context_prime.rs` | 1 | `106` (names 1) | 0 | Covered |
| `horch-core` | `crates/horch-core/tests/fleetd_handlers_custody.rs` | 1 | `127` (names 1) | 0 | Covered |
| `horch-core` | `crates/horch-core/tests/fleetd_pass_p3.rs` | 3 | `127` (names all 3) | 0 | Covered |
| `horch-core` | `crates/horch-core/tests/fr5_push_only.rs` | 1 | `128` (names 1) | 0 | Covered |
| `horch-core` | `crates/horch-core/tests/harness.rs` | 1 | `128` (names 1) | 0 | Covered |
| `horch-core` | `crates/horch-core/tests/intake_roster.rs` | 2 | `128` (names all 2) | 0 | Covered |
| `horch-core` (actually `horch-e2e`) | `crates/horch-e2e/tests/messaging.rs` | 11 | `none` | 11 | New ticket `142` |
| `horch-core` | `crates/horch-core/tests/p5_profile_validation.rs` | 1 | `128` (names 1) | 0 | Covered |
| `horch-core` | `crates/horch-core/tests/p7_conformance.rs` | 2 | `none` | 2 | New ticket `146` |
| `horch-core` | `crates/horch-core/tests/p7_wrapper_exec.rs` | 1 | `128` (names 1) | 0 | Covered |
| `horch-core` | `crates/horch-core/tests/stack_edges.rs` | 1 | `106` (names 1) | 0 | Covered |
| `horch-core` | `crates/horch-core/tests/trf_02_ticket_template.rs` | 1 | `128` (names 1) | 0 | Covered |
| `horch-core` | `crates/horch-core/tests/trial_merges.rs` | 3 | `127` (names all 3) | 0 | Covered |
| `horch-core` | `crates/horch-core/tests/w3_finish_steps.rs` | 1 | `127` (names 1) | 0 | Covered |
| `horch-core` | `crates/horch-core/tests/w3_taskledger_core_status.rs` | 2 | `127` (names all 2) | 0 | Covered |
| `horch-core` | `crates/horch-core/tests/worktree_disk.rs` | 3 | `106` (names all 3) | 0 | Covered |
| `horch (unit)` | `crates/horch/src/{context.rs,messaging.rs,recipes.rs,rescuecmd.rs,sessionscmd.rs,taskledger.rs}` (`unittests src/main.rs`) | 7 | `107` (names 4), `129` (names 3) | 0 | Covered |
| `horch-core (unit)` | `crates/horch-core/src/{fleetd/worker_profiles/adapters/codex.rs,taskledger/schema.rs,taskledger/stop.rs}` (`unittests src/lib.rs`) | 4 | `106` (names 2 in `stop.rs`), `125` (names 2 in `codex.rs` and `schema.rs`) | 0 | Covered |
| `horch-platform` (actually `horch-e2e`) | `crates/horch-e2e/tests/channel.rs` | 4 | `none` | 4 | New ticket `143` |
| **Total** | **65 test binaries/files** | **361** | **242 covered before `140`–`146`** | **119 not covered before `140`–`146`** | **62 in new tickets `140`–`146`; 57 to add to existing tickets** |

## Step 2: New Fix Tickets (`140` to `146`)

No two new tickets share an `Own` path, and none of their `Own` paths overlap with any existing ticket (`01` to `133`).

| Ticket file | Count of tests | Persona | Own paths | Blocked by |
|---|---:|---|---|---|
| `140-fix-e2e-lifecycle.md` | 14 | primary `qa-engineer`, fallback `opus` | `crates/horch-e2e/tests/lifecycle.rs` | `100-ci-fix1-e2e-harness-profile.md`, `05-c2-t899-wrapper-operator.md` |
| `141-fix-e2e-telemetry-and-balance.md` | 12 | primary `qa-engineer`, fallback `opus` | `crates/horch-e2e/tests/e2e.rs` | `100-ci-fix1-e2e-harness-profile.md` |
| `142-fix-e2e-messaging.md` | 11 | primary `qa-engineer`, fallback `opus` | `crates/horch-e2e/tests/messaging.rs` | `100-ci-fix1-e2e-harness-profile.md` |
| `143-fix-e2e-channel-and-delivery-probes.md` | 8 | primary `qa-engineer`, fallback `opus` | `crates/horch-e2e/tests/channel.rs, crates/horch-e2e/tests/fleet18_delivery_probe.rs, crates/horch-e2e/tests/relay_first_delivery.rs` | `100-ci-fix1-e2e-harness-profile.md` |
| `144-fix-e2e-p5-read-only-worker-and-release.md` | 6 | primary `qa-engineer`, fallback `opus` | `crates/horch-e2e/tests/p5_read_only_worker.rs, crates/horch-e2e/tests/p5_release.rs` | `100-ci-fix1-e2e-harness-profile.md`, `43-harness-teardown-under-load.md` |
| `145-fix-e2e-brief-restored-routing-scenario-startup-tell-grace.md` | 9 | primary `qa-engineer`, fallback `opus` | `crates/horch-e2e/tests/brief.rs, crates/horch-e2e/tests/restored_pane.rs, crates/horch-e2e/tests/routing_provenance.rs, crates/horch-e2e/tests/scenario.rs, crates/horch-e2e/tests/startup_error.rs, crates/horch-e2e/tests/tell_grace.rs` | `100-ci-fix1-e2e-harness-profile.md` |
| `146-fix-core-p7-conformance.md` | 2 | primary `qa-engineer`, fallback `opus` | `crates/horch-core/tests/p7_conformance.rs, crates/horch-e2e/src/exec.rs` | `100-ci-fix1-e2e-harness-profile.md` |
| **Total** | **62** | | | |

## Tests to add to a ticket that exists

In accordance with Step 2 ("For a test not covered inside a file that has a ticket: do not write a second ticket for the file. List the test in the index section `## Tests to add to a ticket that exists` with the ticket number"), the following 57 tests from `baseline-2026-11.md` live in files already owned by existing tickets (`FIX-INDEX-8.md` tickets `112`–`133` were generated from the 232-test `red-baseline.txt` snapshot rather than the 361-test `baseline-2026-11.md` snapshot):

### Ticket `15` (`15-c12-t882-observability.md`) or Ticket `120` (`120-ci-fix8-e2e-fleet18-observability.md`) — `crates/horch-e2e/tests/fleet18_observability.rs` (9 tests)
- `g49_spawn_doctor_and_inbox_warn_once_and_name_installed_binary` — `spawn failed: horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`
- `g50_legacy_orchestrator_and_host_load_are_advisory` — `spawn failed: horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`
- `g51_pane_gone_context_filters_and_completed_summaries` — `spawn failed: horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`
- `g51_sessions_filter_workspace_live_role_and_indent_summary` — `spawn failed: horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`
- `g52_concurrent_workers_get_their_own_session_and_transcript` — `spawn failed: horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`
- `g52_done_discovery_uses_the_same_launch_token` — `spawn failed: horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`
- `g53_below_threshold_handoff_does_not_schedule` — `spawn failed: horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`
- `g53_first_handoff_without_prior_warning_does_not_schedule` — `spawn failed: horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`
- `g53_progress_warns_once_and_missing_handoff_is_refused` — `spawn failed: horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`

### Ticket `61` (`61-fix-r3-tests-push-dispatch-verifier-and-planned-kinds.md`) — `crates/horch-e2e/tests/p4_verifier.rs` (4 tests)
- `fm9_code_task_with_a_failing_check_stays_not_done` — `timed out after 20s waiting for a verifier rejection of T-1`
- `fm9_docs_task_without_its_section_stays_not_done` — `timed out after 20s waiting for a verifier rejection of T-1`
- `passing_code_task_reaches_done_only_through_the_verifier` — `timed out after 20s waiting for T-1 to pass the verifier`
- `sr4_check_that_writes_outside_its_paths_is_refused` — `timed out after 20s waiting for a verifier rejection of T-1`

### Ticket `74` (`74-fix-r4-tests-operator-boundary-and-verify.md`) — `crates/horch-e2e/tests/p5_operator_boundary.rs` (1 test)
- `operator_request_from_a_test_in_a_gate_is_refused_for_each_act` — `timed out after 10s waiting for the task to run`

### Ticket `112` (`112-ci-fix8-e2e-done-fakes-fleet18.md`) — `done_budget.rs` (3), `fleet18_brief_worktree.rs` (3), `fleet18_messaging.rs` (7) (13 tests total)
- `crates/horch-e2e/tests/done_budget.rs`:
  - `done_finishes_in_time_against_a_stuck_orchestrator` — `called 'Result::unwrap()' on an 'Err' value: reading the operator token`
  - `done_retried_after_a_kill_during_the_push_records_one_done` — `called 'Result::unwrap()' on an 'Err' value: reading the operator token`
  - `done_returns_in_time_when_herdr_hangs_on_pane_get` — `called 'Result::unwrap()' on an 'Err' value: reading the operator token`
- `crates/horch-e2e/tests/fleet18_brief_worktree.rs`:
  - `g56_facts_reach_the_launch_context_and_are_project_scoped` — `spawn failed: horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`
  - `g56_later_spawn_receives_recorded_fact` — `spawn failed: horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`
  - `g57_compacting_orchestrator_types_its_command_card` — `spawn failed: horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`
- `crates/horch-e2e/tests/fleet18_messaging.rs`:
  - `a_pass_with_an_unknown_orchestrator_ends_within_the_push_budget` — `spawn failed: horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`
  - `g60_codex_queue_confirmation_stays_pending_until_inbox` — `spawn failed: horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`
  - `g60_plain_done_and_tell_without_registration_arrive_after_late_registration` — `spawn failed: horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`
  - `g70_long_multiline_questions_and_blocked_survive_transport_ack_and_reply` — `spawn failed: horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`
  - `g70_pinned_cli_question_reply_resets_stalls` — `spawn failed: horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`
  - `g70_pinned_cli_sixty_minute_review_wait_and_normal_reap_after_completion` — `spawn failed: horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`
  - `g70_pinned_cli_wait_expires_and_dead_agent_is_immediate` — `spawn failed: horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`

### Ticket `113` (`113-ci-fix8-e2e-i308-skills-watchdog-worktree.md`) — `i308_fleet_steward.rs` (1), `i308_triage.rs` (1), `watchdog_liveness.rs` (5), `worktree.rs` (2) (9 tests total)
- `crates/horch-e2e/tests/i308_fleet_steward.rs`:
  - `a_fleet_with_no_parked_worktree_spawns_no_steward` — `called 'Result::unwrap()' on an 'Err' value: reading the operator token`
- `crates/horch-e2e/tests/i308_triage.rs`:
  - `finish_queues_a_task_that_inherits_the_worktree` — `horch [new, --goal, g, --finish, check,land] failed: horch: 'horch ledger run new' is retired. Add work with 'horch idea add "<text>"'; fleetd plans and starts the run.`
- `crates/horch-e2e/tests/watchdog_liveness.rs`:
  - `a_busy_pi_worker_with_no_timer_is_kept_until_stall_after_then_stopped_at_rung_3` — `spawn: horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`
  - `a_detached_fleetd_does_not_inherit_the_starting_panes_workspace` — `called 'Result::unwrap()' on an 'Err' value: reading the operator token`
  - `a_fleetd_started_from_another_workspace_wakes_the_runs_workspace` — `called 'Result::unwrap()' on an 'Err' value: reading the operator token`
  - `a_sigterm_to_fleetd_during_a_reap_leaves_the_reap_to_the_next_fleetd` — `spawn: horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`
  - `a_wake_from_another_workspace_goes_to_the_runs_workspace` — `called 'Result::unwrap()' on an 'Err' value: reading the operator token`
- `crates/horch-e2e/tests/worktree.rs`:
  - `arc_31_e2e_done_refuses_then_unlanded_reports` — `spawn failed: horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`
  - `arc_31_e2e_list_and_done_survive_a_deleted_worktree_folder` — `spawn failed: horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`

### Ticket `116` (`116-ci-fix8-e2e-taskledger-a.md`) — `crates/horch-e2e/tests/taskledger.rs` (2 tests)
- `legacy_plumbing_lives_under_ledger_legacy` — `horch [ledger, legacy, init, --goal, legacy check] failed: error: unrecognized subcommand 'legacy'`
- `status_on_a_fresh_ledger_creates_it_and_says_no_runs` — `status failed: horch: reading the operator token at /Users/matthewscott/.horch/operator.token: No such file or directory (os error 2)`

### Ticket `132` (`132-ci-fix10-e2e-p7-docs-sync-release.md`) — `crates/horch-e2e/tests/p7_docs_sync.rs` (3 tests)
- `new_fake_cli_version_fails_a_check_marks_its_profiles_unhealthy_and_adds_1_idea` — `timed out after 20s waiting for an unhealthy row of claude and an idea` (Note: shares root cause with ticket `146` in `crates/horch-e2e/src/exec.rs:221`; block `132` on `146` or run `146` first.)
- `passed_conformance_marks_the_profiles_healthy_and_dispatch_resumes` — `timed out after 20s waiting for an unhealthy row of claude and an idea`
- `unhealthy_profile_gets_no_dispatch_and_status_names_the_hold` — `timed out after 20s waiting for an unhealthy row of claude and an idea`

### Ticket `133` (`133-ci-fix10-e2e-skills-exposure-fleet-start-ideas.md`) — `fleet_start.rs` (1), `skills_exposure.rs` (15) (16 tests total)
- `crates/horch-e2e/tests/fleet_start.rs`:
  - `a_scratch_fleet_warns_about_the_shared_ledger_and_does_not_create_it` — `timed out after 10s waiting for a live fleetd at /private/var/folders/lr/fc3461w92f91125m79233x540000gn/T/horch-e2e-p6-scratch-fleet-n2zX09/home/.horch/fleetd.sock`
- `crates/horch-e2e/tests/skills_exposure.rs`:
  - `git_env_cannot_reach_another_repository` — `horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`
  - `operator_skills_e2e_changed_since_spawn_fails_the_launch` — `horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`
  - `operator_skills_e2e_deleted_since_spawn_fails_the_launch` — `horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`
  - `operator_skills_e2e_exposure_codex` — `horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`
  - `operator_skills_e2e_ledger_record_lists_them` — `horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`
  - `plugin_skills_e2e_changed_since_spawn_fails_the_launch` — `horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`
  - `plugin_skills_e2e_ledger_record_lists_them` — `horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`
  - `skl_06_e2e_exposure_antigravity` — `horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`
  - `skl_06_e2e_exposure_claude` — `horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`
  - `skl_06_e2e_exposure_codex` — `horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`
  - `skl_06_e2e_exposure_opencode` — `horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`
  - `skl_06_e2e_exposure_pi` — `horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`
  - `skl_06_e2e_exposure_prime` — `horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`
  - `skl_06_e2e_marketplace_offline` — `horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`
  - `worker_reads_the_skill_store_its_spawner_read` — `horch: horch spawn is retired. fleetd launches each attempt; add work with 'horch idea add' or 'horch run new'.`

### Additional notes on covered tests in `106` and `128`
- **Ticket `106` (`crates/horch-core/src/taskledger/stop.rs`):** `baseline-2026-11.md` lists `taskledger::stop::tests::trf_36_a_cancel_stops_the_worker_by_its_pids_and_queues_nothing` and `taskledger::stop::tests::trf_36_a_worker_that_cannot_be_stopped_leaves_the_cancel_standing`, which commit `6260fc5c` (`T-893`) renamed to `trf_36_a_stop_ends_the_worker_by_its_pids_and_queues_nothing` and `trf_36_a_worker_that_cannot_be_stopped_returns_false`. Ticket `106` names the post-`T-893` test identifiers.
- **Ticket `128` (`crates/horch-core/tests/baseline_oracles.rs`):** `arc_09_argv_matches_baseline` is named in ticket `101-ci-fix2-linux-socat.md` (which only edits `.github/workflows/ci.yml`), but it also failed locally on macOS in `baseline-2026-11.md` with `oracle mismatch for claude/happy`. Ticket `128` already owns `crates/horch-core/tests/baseline_oracles.rs` and runs `cargo test -p horch-core --test baseline_oracles --locked`, so `128` will also run `arc_09_argv_matches_baseline`.

## Open questions

1. **Tests of `horch spawn` (retired in Phase 6 under `FR-5`, ticket `47`) and orchestrator pane launch in `horch fleet` (retired in Phase 6 under `FM-16`):**
   - Across `140` (`lifecycle.rs`), `141` (`e2e.rs`), `142` (`messaging.rs`), `143` (`fleet18_delivery_probe.rs`, `relay_first_delivery.rs`), and `145` (`brief.rs`, `restored_pane.rs`, `routing_provenance.rs`, `scenario.rs`, `startup_error.rs`, `tell_grace.rs`), several tests directly test the retired `horch spawn` CLI or the retired `horch fleet` orchestrator pane launch / delivery probe.
   - Each ticket instructs the worker to migrate tests whose subject is a surviving feature (where `horch spawn` was only fixture setup) to `h.serve_fleetd()` / push-attempt setup, and to send `QUESTION:` to the orchestrator citing `FR-5` or `FM-16` rather than deleting any test on their own decision. If you prefer a blanket orchestrator rule up front (for example, delete tests that solely test retired `horch spawn` or the retired orchestrator pane of `horch fleet`), workers can apply that rule directly.
2. **Dependency between `146-fix-core-p7-conformance.md` and `132-ci-fix10-e2e-p7-docs-sync-release.md`:**
   - The 3 failing tests of `crates/horch-e2e/tests/p7_docs_sync.rs` in `baseline-2026-11.md` (`new_fake_cli_version_fails_a_check_marks_its_profiles_unhealthy_and_adds_1_idea`, `passed_conformance_marks_the_profiles_healthy_and_dispatch_resumes`, `unhealthy_profile_gets_no_dispatch_and_status_names_the_hold`) install a fake `claude` wrapper with `,exit_without_result` and fail because `crates/horch-e2e/src/exec.rs:221` ignores `exit_without_result` when `check` is true. Ticket `146` fixes `crates/horch-e2e/src/exec.rs:221`, so running `146` before `132` will resolve the underlying fake-exec cause for `132` as well.
