# Implementation plan - fleetd: the horch fleet control plane

Produced by `ddd-code` on 2026-11-12 from `ddd/07-define/define.json`, `ddd/04-strategize/strategize.json`, `ddd/05-connect/connect.json`, `ddd/06-organise/organise.json`, `ddd/02-discover/discover.json`. Language rust · framework none · architecture hexagonal · deployables horch · scaffold generated: no

## In plain words

This plan says in which order to build fleetd, in 9 slices. The order is the 9 phases of the design's migration plan (Phase 0, Phase P, Phases 1 to 7), so each slice ships only when its gate tests pass.

**Decided:** All 6 parts of the design become modules under `crates/horch-core/src/fleetd/`. The platform layer is the leaf crate `crates/horch-platform` (root `crates/horch-platform/src/`), which every context calls. worker-profiles owns the fleet-run wrapper (D3 option B). Each slice names its failing tests first, then the modules.

**Assumed:** The existing modules (taskledger, vcs, runtime, roster, harness) stay where they are and are wrapped by adapters, until the phase that replaces their behaviour removes the old path.

**Riskiest:** If a slice leaves the old path in place, the old watchdog and fleetd can both act on 1 task. Each slice therefore ends with the removal of the old path it replaces. Slice P moves 178 OS-specific sites at once; its only safety net is that each test gives the same result before and after the move.

---

## Order and why
1. The design's migration plan fixes the order (assumption A8). Phases 1 to 4 stop most daily stalls and stop lost worktrees, so they come first. Slice P comes right after Slice 0 (operator decision D16): it builds the platform crate that every later slice calls. This puts fleet-runtime (the single writer) before the core contexts, because every core rule runs inside its reconcile pass.
2. `task-execution` is the first core context, in slices 1, 2 and 4. Its pivotal event `task-done` arrives in slice 4, when the verifier replaces the worker's own done.
3. `integration` and `ideas` come last (slice 6), because a plan must land through the single writer to be all-or-nothing (design: Phase 6 depends on Phase 5).
4. Read models are the existing horch CLI views, rebuilt from the ledger in the slice that changes their tables.

Each slice is a vertical slice: the tests in "Tests (write first)" are written and seen to fail before the modules, as define N3 requires for the quality attributes of priority high. Each slice is done when its tests pass and the fitness tests still pass.

## Slice 0 - Walking skeleton and Phase 0 red tests
- Goal: the module folders exist, the fitness tests run in CI on macOS, Linux and Windows, every failure of Phase 0 has a red test, and each red test is on the list of expected failures with the phase that makes it green (A22). CI runs the test suite on Windows from this slice; today the Windows job only builds.
- Implements: design Migration plan Phase 0; decision D1; `design.md` §1 of every context.
- Files/modules: `crates/horch-core/src/fleetd/{ideas,task_execution,worktree_custody,integration,worker_profiles,fleet_runtime}/mod.rs` with empty `domain/`, `application/`, `adapters/`; the list of expected failures (A15) and the test runner rule that reads it; the Windows test job in `.github/workflows/ci.yml`; `crates/horch-core/tests/arch_scan.rs`; scenarios in `crates/horch-e2e`; a row per new module in `docs/architecture.md` (the test `every_core_module_has_a_row_in_the_architecture_doc` requires it).
- Tests (write first): `fitness_domain_imports_nothing_outward`, `fitness_only_fleet_runtime_opens_the_ledger_writable`, `fitness_no_context_touches_foreign_tables` (organise N4), `expected_failures_list_gates_the_run`; red e2e scenarios for FM-1 to FM-8 and FM-25 and the new Phase 0 scenarios (several ready branches, 2 conflicting features, a usage-limited worker, a machine sleep, a daemon restart on a new binary, a hung fleetd on each OS).
- Done when: `cargo test -p horch-core --test arch_scan` is green with no business logic, each Phase 0 scenario exists and fails for the documented reason, and a run passes on macOS, Linux and Windows when each test off the list passes and each test on the list fails.

## Slice P - the platform layer: Phase P, the move of the existing OS-specific code
- Goal: the leaf crate `crates/horch-platform` exists with 1 interface and 1 module per OS, and all 178 production OS-specific sites call it, with no change of behavior (D16, A19).
- Implements: design Migration plan Phase P; PL-2 and PL-6; assumption A7; `preservation-06-wiring-and-platform.md` section 6. Every context calls the crate directly; no context owns it (decompose N7).
- Files/modules: `crates/horch-platform/src/{lib.rs,mac.rs,linux.rs,windows.rs}` (the interface, NotSupported, and the concerns of the existing code: process identity and liveness, signals and stop, process groups, file locks, file permissions, paths and home directories, links, shell and quoting, local sockets and pipes, detach, the machine probe, the sandbox host, and the small rest); each of the 178 sites in the workspace, also the 45 socket sites of 06 S11 that Slice 5 deletes later; the PL-2 rule in `crates/horch-core/tests/arch_scan.rs`. The `cfg` sites in test code stay.
- Tests (write first): `fitness_workspace_production_code_has_no_os_cfg_outside_horch_platform`, `pl6_each_test_has_the_same_result_before_and_after_the_move`, `platform_not_supported_is_explicit_on_each_module`. A test that fails on Windows for a gap of today is fixed, or it is skipped on Windows by name with the row of section 6 that states the gap.
- Done when: design gate PL-2 and PL-6 is green on macOS, Linux and Windows. A red test of Phase 0 is still on the list of expected failures and does not block the gate.

## Slice 1 - fleet-runtime and task-execution: Phase 1, the supervised level-triggered pass
- Implements: fleet-runtime `design.md` §3 (`check-fleetd`, `restart-fleetd`, `self-update`); the Task aggregate reap path (`aggregate-canvas-task.md`, invariant T4 and I1, I4, I5); decision D2.
- Files/modules: `fleetd/fleet_runtime/domain/{ledger.rs,pass.rs,heartbeat.rs}`, `fleetd/fleet_runtime/adapters/sqlite.rs` (wraps `taskledger::store`), `crates/horch-platform/src/{mac,linux,windows}.rs` (the Phase 1 concerns: service install and restart, hang detection, sleep and wake, file watching, the channel transport), `fleetd/task_execution/domain/task.rs`, `application/commands.rs` (`EndAttempt`, `ScheduleRetry`, `EscalateTask`), `crates/horch/src/cmd/fleetd.rs` (composition root). Remove the lazy watchdog `ensure` start.
- Tests (write first): `fm12_hung_fleetd_restarted_on_each_os`, `pres_06_s14_hung_fleetd_restarted_on_macos`, `pres_06_s14_hung_fleetd_restarted_on_linux`, `pres_06_s14_hung_fleetd_restarted_on_windows`, `fm_fleetd_killed_mid_reap_requeues_exactly_once`, `pres_04_s7_reap_requeues_in_1_transaction_before_the_signal`, `pres_04_s8_stop_signals_only_the_recorded_pid_of_this_host`, `pres_04_s10_changed_binary_restarts_between_passes`, `i1_leased_task_without_unexpired_lease_is_stalled_in_the_same_pass`, `i2_task_with_an_undone_dependency_is_not_promoted`, `i4_dispatch_refuses_a_worker_with_a_live_attempt`, `i5_orphan_process_or_pane_is_stopped`, `illegal_transition_is_refused_and_writes_nothing`, `pres_06_s12_unknown_probe_neither_pauses_nor_passes`.
- Done when: design gate FM-4, FM-7, FM-12, FM-13 is green on macOS, Linux and Windows.

## Slice 2 - task-execution: Phase 2, leases, capped waits and waiting_quota
- Implements: `renew-lease`, `apply-stall-ladder`, `move-to-fallback`, `promote-task`; Task invariants T5, T7; StallLadder; schema v8 (`leases`, `verifications`, `profile_health`).
- Files/modules: `fleetd/task_execution/domain/{task.rs,stall.rs,values.rs}`, `application/commands.rs`, `adapters/gateways.rs` (`CliQuotaProbe`, `PlatformLivenessProbe`, `PlatformAwakeClock`); `fleetd/worker_profiles/domain/profile.rs` (required fallback).
- Tests (write first): `fm3_stopped_worker_loses_lease_and_task_is_retried`, `fm5_usage_limit_burns_no_attempt_and_moves_to_fallback`, `sleep_does_not_expire_a_lease`, `retry_backoff_30s_2m_8m_then_replan_then_escalate`, `profile_without_fallback_is_refused`, `pres_06_s13_quota_probe_holds_no_credential`, `transition_running_to_waiting_quota_pauses_the_lease`, `newer_schema_is_refused_and_backup_precedes_migration`.
- Done when: design gate FM-3, FM-5 is green; a usage-limited worker is not reaped and burns no attempt; the idle-claim scenario is caught in under 10 min. Remove "3 checks of 600 s".

## Slice 3 - worktree-custody: Phase 3, worktree leases and rescue
- Implements: worktree-custody `design.md` §3 (`create-worktree`, `reconcile-worktrees`, `rescue-worktree`, `remove-worktree`); invariant I6; RR-9.
- Files/modules: `fleetd/worktree_custody/domain/{worktree.rs,rescue.rs}`, `application/`, `adapters/sqlite.rs` (extends the `worktrees` table), `adapters/gateways.rs` (wraps `taskledger::worktree_ops` and the triage refs); `horch worktrees` and `horch worktree restore` in `crates/horch/src/cmd/worktree.rs`. Run once in report-only mode first.
- Tests (write first): `fm_killed_worker_dirty_untracked_files_on_verified_rescue_ref`, `rr9_rescue_ref_is_read_back_before_removal`, `i6_worktree_on_disk_without_row_is_adopted_not_deleted`, `pres_04_w6_drop_refuses_dirty_stash_unique_or_moved_head`, `pres_04_w8_refs_move_forward_and_are_read_back`, `pres_04_w9_triage_commits_untracked_before_delete`, `failed_rescue_verification_keeps_the_tree`, `contract: worktree-created`, `idempotent: task-leased`.
- Done when: design gate FM-25 to FM-28 is green; a worker killed with dirty and untracked files loses nothing.

## Slice 4 - task-execution: Phase 4, push dispatch and the verifier (pivotal event `task-done`)
- Implements: `dispatch-task`, `launch-attempt`, `write-result`, `run-check`, `run-review`, `mark-task-done`; `aggregate-canvas-verification.md` (V1, V2); decision D4; flow F2 and F3 of connect; worker-profiles `LaunchEnvironmentQuery`, `AgentProcesses` and the fleet-run wrapper (D3 option B); the worker files task.yaml, START.md and verdict.json.
- Files/modules: `fleetd/task_execution/domain/{verification.rs,dispatch.rs}`, `application/{commands.rs,handlers.rs}`, `adapters/gateways.rs` (`SandboxedCheckRunner` wraps the `taskledger::finish` review scripts unchanged); `fleetd/worker_profiles/domain/launch_env.rs`; `fleetd/worker_profiles/application/commands.rs` (`StartAgentProcess`) and the fleet-run wrapper with its composition root `crates/horch/src/cmd/fleet_run.rs`; Stop hooks for Claude Code, Codex and Antigravity; `horch result`.
- Tests (write first): `fm_result_file_alone_leaves_task_verifying`, `verified_verdict_has_no_constructor_outside_the_verification_module (compile-fail test)`, `fm_anthropic_api_key_reaches_no_child_process`, `pres_05_e10_roster_rejects_a_teammate_file_that_sets_it`, `check_runs_in_the_worker_sandbox_not_with_fleetd_rights`, `code_kind_needs_review_pass_with_at_most_2_requeues`, `pres_04_l6_overlapping_paths_are_not_dispatched_together`, `start_agent_process_inside_transaction_panics_in_debug`, `contract: task-done`, `contract: task-leased`, `contract: assign-task`, `contract: start-work`, `contract: task-verdict-written`, `contract: heartbeat-written`, `idempotent: heartbeat-written`, `result_json_with_another_status_is_refused`, `start_agent_process_for_a_live_attempt_is_a_no_op`, `wrapper_scrubs_env_and_builds_config_home_before_exec`, the worker-profiles preservation tests `pres_06_s3` to `pres_06_s15`.
- Done when: design gate FM-2, FM-6, FM-9 is green; `CLAIM_TASK` is gone; the task reaches `task-done` only through the verifier.

## Slice 5 - fleet-runtime: Phase 5, one ledger writer
- Implements: the command channel and request files; outbox ingestion (`AttemptFileIngest`, `IdeaFolderIngest`); 04 M5; 06 S11.
- Files/modules: `fleetd/fleet_runtime/domain/channel.rs`, `adapters/` (`PlatformChannelServer`, `RequestFileSpool`, `FsProtectedFiles`), `fleetd/task_execution/adapters/` (`OutboxFileIngester`); the arch_scan rule that every other process opens the ledger read-only; remove Codex's `broad_ledger_rule` sandbox escape.
- Tests (write first): `cli_command_opens_ledger_read_only_and_sends_a_request`, `pres_04_m5_foreign_owned_outbox_is_refused`, `pres_06_s11_worker_report_is_a_file_on_every_os`, `only_the_fixed_doorbell_line_is_typed`, `pres_06_s2_warns_on_herdr_resume_agents_on_restore`, `request_with_a_seen_idempotency_key_returns_the_first_outcome`, `refused_request_returns_its_reason`, `idempotent: agent-status-changed`, `idempotent:` tests of every file-borne message.
- Done when: design gate FM-17, FM-24 is green and the arch test passes. The Windows channel transport is a live-test item of the design.

## Slice 6 - ideas and integration: Phase 6, the dispatcher, features and landing
- Implements: ideas `design.md` §3 (all 21 use cases, the Plan aggregate of `aggregate-canvas-plan.md` with PlanValidator, I3, I7, I13, the 1 needs-you list); integration `aggregate-canvas-feature.md` and `aggregate-canvas-task-branch.md` (I8 to I12, 04 F5, F7, F9, F11, L5, L9); schema v9 (`features`, `branches`, `gate_runs`, `trial_merges`, idea states); the `idea-lead` persona; `horch idea add`, `--direct`, `--to`, `ideas`, `show`, `note`, `approve`, `revise`, `park`.
- Files/modules: `fleetd/ideas/**`, `fleetd/integration/**` (`CliGitGateway` wraps `vcs::git`, `vcs::network`, `vcs::landing`), `crates/horch/src/cmd/idea.rs`. Retire the long-lived orchestrator conversation and `MAP.md`.
- Tests (write first): `fm32_forged_approval_is_ignored`, `i7_changed_assumptions_after_approval_hold_the_idea`, `fm_squash_merged_feature_is_merged_by_content`, `fm_failed_fast_gate_after_rebase_keeps_feature_tip_and_rescue_ref`, `pres_04_f11_stalled_push_ends_at_10_min_and_frees_the_land_lock`, `pres_04_f9_cancelled_task_lands_nothing`, `pres_04_f7_red_land_resets_to_tip_before_the_land`, `pres_04_f5_new_commits_reopen_the_land_and_later_steps`, `pres_04_l5_unlanded_task_keeps_worktree_and_blocks_dependents`, `pres_04_l9_unlanded_work_is_listed_and_decided_once`, `i3_uncovered_requirement_is_reported_and_plan_refused`, `plan_with_a_dependency_cycle_is_refused`, `plan_without_declared_fast_gate_is_refused`, `question_escalates_after_15_min_and_operator_wait_has_no_limit`, `escalated_task_appears_on_the_1_needs_you_list`, `i9_` to `i13_` tests, `no_rebase_runs_before_the_rescue_ref_reads_back`, `rescue_ref_for_integration_reads_back_before_return`, `feature_rebase_during_running_attempt_then_landing_rebases_or_conflicts`, `fitness_git_calls_split_by_concern`, `idempotent: pull-request-merged`, `given_3_ready_branches_when_bundle_then_fast_gate_runs_once_and_none_is_forgotten`, every `contract:` test of ideas and integration.
- Done when: design gate FM-1, FM-8, FM-10, FM-16, FM-29 to FM-32 is green.

## Slice 7 - worker-profiles: Phase 7, conformance, then exec mode
- Implements: `run-conformance-tests`, `mark-profile-unhealthy`; exec mode as the default; 256k context for every seat.
- Files/modules: `fleetd/worker_profiles/application/commands.rs`, `adapters/` (`CliConformanceRunner`); widen `crates/herdr-docs-sync` into the docs sync subcommand that fleetd schedules (organise D2 option A; owning context open, Q3).
- Tests (write first): `unhealthy_profile_is_skipped_by_dispatch`, `launch_environment_sets_256k_context_limit`, `pres_06_s7_cli_release_younger_than_cooldown_is_held`.
- Done when: design gate FM-18 to FM-20 is green.

## Not in this plan
- The owning context of docs sync and adapter conformance (Q3, decompose Q2).
- The fast gate command of this repository (understand Q8); the Plan aggregate refuses a plan until it is declared.

## Hand-off
1. Run `software:create-plan` on this file to expand each slice into tasks with exact paths, code and commands.
2. Implement slice by slice with `software:tdd`: invariant tests first, watch them fail, then the minimum code.
3. If implementation changes an aggregate boundary or a message contract, update the canvas / `code.json` and re-run `ddd validate ddd --step code`; upstream changes → `ddd` marks steps stale.
