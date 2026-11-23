Tip: 895e384850230f5b2a5a6eaa824b0ba9b4f4d1f3

# Pre-gate of Phase 5

This is the pre-gate of Phase 5 (task T-324, ticket `docs/tickets/fleetd-finish/211-pre-gate-p5.md`).
The real gate (`docs/specs/fleetd/gates/fleetd-p5.md`, task E3) runs after the merge of `origin/main`.
The gate line is in `docs/specs/fleetd/design.md`, Migration plan, item 7, and in
`docs/tickets/fleetd-p5/49-gate.md`.

Host: macOS aarch64, 2026-11-20, 1-minute load 11 to 14. Each command ran through
`scripts/dev-sandbox.sh`, 1 `cargo test` command at a time, with `--no-fail-fast`.
The 1-minute load stayed below 32, so no run used `--test-threads=4`.

Commands:

1. `cargo build --workspace --bins --locked`: passed.
2. `cargo test -p horch-core --locked --no-fail-fast --test nfr --test arch_scan --test architecture_doc --test fleetd_single_writer --test fr7_typed_lines --test fleetd_lock --test fleetd_channel --test peer_process`: 1 failed (arch_scan).
3. `cargo test -p horch-core --locked --no-fail-fast --test fleetd_channel_client --test fleetd_polite_stops --test p5_claude_sandbox`: passed. These suites hold the tests of the REQUIRED map items and of SR-8.
4. `cargo test -p horch-e2e --locked --no-fail-fast --test p5_operator_boundary --test p5_untrusted_files --test p5_requests --test p5_read_only_worker --test p5_push_stop --test p5_release`: passed.
5. `grep -n "| 5 |" crates/horch-e2e/expected-failures/*.txt`: no line. Phase 5 has no expected-failures entry.

## Rows

| Row | Test binary | Test name | Result | First failure line |
| --- | --- | --- | --- | --- |
| FR-7 | horch-core `fr7_typed_lines` | `only_the_fixed_doorbell_line_is_typed`; `the_list_holds_each_typed_line_once_and_each_is_1_line` | passed (see Q1) | - |
| RR-5 | horch-core `arch_scan` | `fitness_only_fleet_runtime_opens_the_ledger_writable` | passed | - |
| RR-5 | horch-core `fleetd_lock` | `second_fleetd_exits_at_once_while_the_lock_is_held` | passed | - |
| RR-5 | horch-core `fleetd_single_writer` | `cli_command_opens_ledger_read_only_and_sends_a_request` | passed | - |
| RR-8 | horch-e2e `p5_requests` | `request_sent_2_times_with_1_request_id_has_1_effect`; `fleetd_killed_after_the_commit_only_archives_the_file`; `fleetd_killed_before_the_commit_applies_the_request_file_once_after_restart`; `known_request_id_with_another_payload_is_refused` | passed | - |
| SR-5 | horch-e2e `p5_untrusted_files` | `request_from_another_user_on_the_channel_is_refused`; `request_file_with_mode_0644_is_refused_with_a_reason`; `request_file_of_another_owner_is_refused_with_a_reason` | passed | - |
| SR-6 | horch-e2e `p5_untrusted_files` | `symlinked_result_is_rejected_and_fleetd_stays_healthy`; `progress_over_its_limit_is_rejected_and_fleetd_stays_healthy`; `result_with_the_attempt_id_of_another_attempt_is_rejected` | passed | - |
| SR-8 | horch-e2e `p5_operator_boundary` | `operator_request_from_a_worker_sandbox_is_refused_for_each_operator_kind`; `operator_request_from_a_process_of_a_live_attempt_is_refused`; `operator_request_from_a_test_in_a_gate_is_refused_for_each_act`; `wrapper_runs_a_full_attempt_with_a_token_that_it_cannot_read` | passed (see Q2) | - |
| SR-8 | horch-core `p5_claude_sandbox` | `claude_rules_under_the_platform_sandbox_refuse_the_3_acts` | passed | - |
| FM-15 | horch-e2e `p5_requests` | `request_sent_2_times_with_1_request_id_has_1_effect`; `lost_reply_is_found_by_horch_request_and_not_applied_again` | passed | - |
| FM-17 | horch-e2e `p5_read_only_worker` | `fm17_codex_worker_writes_its_result_in_the_task_folder`; `fm17_codex_worker_needs_no_ledger_allow_rule` | passed | - |
| FM-23 | horch-e2e `p5_untrusted_files` | `half_written_files_are_never_read`; `rejected_path_is_not_read_again` | passed | - |
| FM-24 | horch-core `fleetd_lock` | `second_fleetd_exits_at_once_while_the_lock_is_held`; `second_fleetd_exits_on_a_generation_mismatch` | passed | - |
| REQUIRED 1 (Q1 of the map): Windows pipe, 1 instance | horch-platform (Windows only) | `windows::channel::client_waits_on_pipe_busy_and_connects_after_the_first_client_ends` | CI only, not run on this Mac | - |
| REQUIRED 2 (Q8 of the map): unix socket client check | horch-core `fleetd_channel_client` | `client_refuses_a_server_that_is_not_the_fleetd_of_the_ledger` | passed | - |
| REQUIRED 2: deny of writes in `run/` | horch-core `p5_claude_sandbox` | `claude_worker_settings_deny_writing_the_run_folder_and_the_ledger`; `claude_worker_settings_never_allow_the_channel_socket` | passed | - |
| REQUIRED 3: server-side operator check, `HORCH_ROLE` not trusted | horch-e2e `p5_operator_boundary` | `operator_only_command_with_horch_role_set_or_removed_is_refused_for_each_command`; `forged_orchestrator_seat_is_refused`; `operator_only_rescue_drop_from_a_worker_is_refused` | passed | - |
| REQUIRED 4 (Q12 of the map): polite stop is a row | horch-core `fleetd_polite_stops` | `polite_stop_is_a_row_before_the_signal`; `old_stop_file_becomes_a_row_once` | passed | - |
| REQUIRED 5: read-only worker | horch-e2e `p5_read_only_worker` | the 3 tests of the binary | passed | - |
| Push stop | horch-e2e `p5_push_stop` | `a_stop_of_a_push_worker_ends_its_attempt_and_its_agent_only`; `each_stop_that_fleetd_sends_is_recorded_and_a_second_stop_sends_nothing`; `a_stalled_push_task_stays_stalled_while_its_old_agent_lives` | passed | - |
| Release test of Phase 5 | horch-e2e `p5_release` | `update_stops_no_run`; `old_push_worker_runs_progress_and_done_after_the_update_with_1_effect_each`; `old_claim_worker_runs_progress_and_done_after_the_update_with_1_effect_each` | passed | - |
| Arch test (guard) | horch-core `arch_scan` | `fitness_no_context_touches_foreign_tables` | failed, 3 of 3 runs | `crates/horch-core/tests/arch_scan.rs:1880:5: a context names a table of another context:` |
| Arch test (guard) | horch-core `arch_scan` | the 35 other tests | passed | - |
| Guard | horch-core `nfr` | 11 tests (`nfr_02_live_store_rss` and `nfr_02_tick_budget` are `#[ignore]`) | passed | - |
| Guard | horch-core `architecture_doc` | 3 tests | passed | - |
| Guard | horch-core `fleetd_channel` | 9 tests (`channel_client_child` runs only as a child) | passed | - |
| Guard | horch-core `peer_process` | 11 tests | passed | - |
| Expected failures of Phase 5 | `crates/horch-e2e/expected-failures/*.txt` | `grep -n "\| 5 \|"` | no line | - |

Design text questions (open, the operator decides; `ai_docs/plans/fleetd-finish/plan.md`, "Design text questions"):

- Q1 (FR-7): `docs/specs/fleetd/design.md:143` says 1 fixed doorbell line. `TYPED_LINES` in `crates/horch-core/src/messaging/delivery.rs:33` holds 9 fixed lines. The FR-7 tests prove the closed list of 9 lines, not 1 line.
- Q2 (SR-8): `docs/specs/fleetd/design.md:182` says the sandbox test records refusal rows. An OS sandbox refusal leaves no `requests` row. `p5_operator_boundary` proves the OS refusal, and it proves a server refusal with a row only after a bypass of the sandbox.

CI-only rows:

- The Windows single pipe instance test (REQUIRED 1). Not run here.
- The Windows ACL checks of protected files (SR-5 on Windows, T-63). Not run here.
- The Linux sandbox paths of SR-8 (bwrap). Not run here.

## Red rows

1. `arch_scan` `fitness_no_context_touches_foreign_tables` fails 3 of 3 runs on tip `895e3848`.
   - Failure text: `a context names a table of another context:`, then 14 lines:
     - `fleetd/fleet_runtime/application/pass.rs:3178: table workers is not a table of fleet_runtime`
     - `fleetd/task_execution/adapters/sqlite.rs:205: table runs is not a table of task_execution`
     - `fleetd/task_execution/application/dispatch.rs:498: table ideas`; `:509 task_files`; `:589 worktrees`; `:619 runs`; `:619 projects`; `:644 task_files`
     - `fleetd/task_execution/application/launch.rs:428: table questions`
     - `fleetd/ideas/application/sub_orchestrators.rs:699: table tasks`
     - `fleetd/worktree_custody/application/disk.rs:144: tables jobs, tasks, ideas`
     - `fleetd/worktree_custody/application/reconcile.rs:603: table jobs`
   - File and line: `crates/horch-core/tests/arch_scan.rs:1880`.
   - Cause (verified with `git blame` of each line): new SQL joins that read tables of another context. Commits: `6c453b24`, `ddceb62c`, `cc0d7e23`, `d15dd771`, `74c79ed6` (T-292), `0b81e22e` (T-313), `583b4402` (T-300).
   - Owner: T-327 (queued, the orchestrator named it).

## Rules with no test

- FR-7 as the literal design text (1 fixed doorbell line): no test proves it. The tests prove a closed list of 9 lines (Q1).
- SR-8 as the literal design text (the sandbox test records refusal rows): no test proves a row after an OS refusal. Q2 says that such a row cannot exist.
- REQUIRED 1 and the Windows rules of SR-5: no test runs on this Mac. Only Windows CI proves them.
