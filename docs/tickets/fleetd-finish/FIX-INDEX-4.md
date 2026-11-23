# Fix tickets index: end review R4 (Phase 5)

Review covered:
- R4: `docs/specs/fleetd/reviews/end-review-p5.md` (on `fleet/fleetd-finish`)

## Finding R4-F5 — Covered by T-5 (T-899 / ticket `05-c2-t899-wrapper-operator.md`)

- **R4-F5 (Important — SR-8, FM-32)**:
  - Location: `crates/horch-core/src/execution/store.rs:1029`; `crates/horch-core/src/fleetd/fleet_runtime/domain/authorize.rs:90`.
  - Summary: The worker wrapper writes session state by sending the operator-class `session_records` request, which makes the wrapper read the operator token. Channel authorization rejects a live attempt peer, but a fallback request file is authorized by token and file ownership alone.
  - **Why no new ticket is written**: Per the extra rule of `fix-tickets-brief-4.md`, task T-5 (`05-c2-t899-wrapper-operator.md`, original ticket `ai_docs/plans/fleetd-build/tickets/61-wrapper-facts-go-by-attempt-files.md`) already covers R4-F5 in full:
    - Requirement 1 of `05-c2-t899-wrapper-operator.md` removes operator-class requests sent from the worker wrapper path (specifically `ExecutionStore` calls in `crates/horch-core/src/execution/store.rs:1034` that call `channel_client::send(..., Kind::SessionRecords, ...)` from a worker wrapper) and routes wrapper session and lifecycle facts through the attempt files (`task_files.rs` / `attempt_files.rs`) ingested by fleetd.
    - Requirement 2 of `05-c2-t899-wrapper-operator.md` updates `crates/horch-core/src/execution/service.rs:278` so legacy pane launch gives `horch worker` no path to read `<fleet root>/run/operator.token`.
    - Requirement 3 of `05-c2-t899-wrapper-operator.md` updates `crates/horch-core/src/fleetd/fleet_runtime/adapters/channel_client.rs:58` to refuse to read `operator.token` or send an `operator` request when the caller is inside a worker wrapper or fleet attempt.
    - Requirement 4 of `05-c2-t899-wrapper-operator.md` adds the unit and e2e tests proving that the wrapper never reads `<fleet root>/run/operator.token` and sends no operator-class request.

## Fix tickets

| File | Finding ID | Rank | Persona | Own paths | Blocked by |
|---|---|---|---|---|---|
| [70-fix-r4-f1-queued-request-refusal-visibility.md](70-fix-r4-f1-queued-request-refusal-visibility.md) | R4-F1 | Critical | primary `opus`, fallback `codex-sol` | `crates/horch-core/src/execution/store.rs`, `crates/horch-core/src/fleetd/fleet_runtime/adapters/request_spool.rs`, `crates/horch-core/tests/execution_store.rs`, `crates/horch-core/tests/fleetd_request_files.rs` | T-5 (`05-c2-t899-wrapper-operator.md`) |
| [71-fix-r4-f2-channel-server-stop-without-socket-path.md](71-fix-r4-f2-channel-server-stop-without-socket-path.md) | R4-F2 | Important | primary `opus`, fallback `codex-sol` | `crates/horch-core/src/fleetd/fleet_runtime/adapters/channel_server.rs`, `crates/horch-core/tests/fleetd_channel.rs` | 36 (`36-fix-r1-f2-windows-channel-bounded-write.md`) |
| [72-fix-r4-f3-windows-protected-files-acl.md](72-fix-r4-f3-windows-protected-files-acl.md) | R4-F3 | Important | primary `opus-hardening`, fallback `opus` | `crates/horch-platform/src/fs.rs`, `crates/horch-platform/src/unix/fs.rs`, `crates/horch-platform/src/windows/fs.rs`, `crates/horch-core/src/fleetd/fleet_runtime/adapters/protected_files.rs`, `crates/horch-core/src/fleetd/fleet_runtime/adapters/request_spool.rs`, `crates/horch-core/src/fleetd/fleet_runtime/adapters/channel_client.rs`, `crates/horch-core/tests/protected_files.rs`, `crates/horch-core/tests/fleetd_request_files.rs` | T-5 (`05-c2-t899-wrapper-operator.md`), 70 |
| [73-fix-r4-f4-dispatch-base-tip-outside-tx.md](73-fix-r4-f4-dispatch-base-tip-outside-tx.md) | R4-F4 | Important | primary `opus-hardening`, fallback `opus` | `crates/horch-core/src/fleetd/task_execution/application/dispatch.rs`, `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs`, `crates/horch-core/tests/push_dispatch.rs` | the landing chain (T-15), 53 (`53-fix-r5-f6-direct-judge-output-ticket-and-kind.md`) |
| [74-fix-r4-tests-prove-rules.md](74-fix-r4-tests-prove-rules.md) | R4-TESTS | Important | primary `qa-engineer`, fallback `opus` | `crates/horch-core/src/execution/store.rs`, `crates/horch-core/tests/p7_operator_boundary.rs`, `crates/horch-e2e/src/harness.rs`, `crates/horch-e2e/tests/p5_operator_boundary.rs` | T-4 (`04-c1-land-t898.md`), T-5 (`05-c2-t899-wrapper-operator.md`), 70 |

## File overlaps and `pass.rs` notes

- **`pass.rs` dependency (ticket 73 / R4-F4)**: Finding R4-F4 (`crates/horch-core/src/fleetd/task_execution/application/dispatch.rs:315` and `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs:1404`) requires reading the base branch tips through the sandboxed `CliGitGateway` in Part 1 of the pass (`Pass::read_facts`) before `Pass::decide` opens the single-writer transaction (`TransactionBehavior::Immediate`), and passing them into `Pass::dispatch_rows`. Because `Own` includes `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs`, ticket 73 is marked `Blocked by: the landing chain (T-15)`. None of tickets 70, 71, 72, or 74 touches `pass.rs`.
- **Shared files within this ticket set and with existing tickets**:
  - `crates/horch-core/src/fleetd/fleet_runtime/adapters/request_spool.rs` and `crates/horch-core/tests/fleetd_request_files.rs` are in `Own` of both ticket 70 (R4-F1) and ticket 72 (R4-F3). Ticket 72 is marked `Blocked by: 70` so they do not run in parallel.
  - `crates/horch-core/src/execution/store.rs` is in `Own` of ticket 05 (`05-c2-t899-wrapper-operator.md`, T-5), ticket 70 (R4-F1), and ticket 74 (R4-TESTS). Ticket 70 is marked `Blocked by: T-5`, and ticket 74 is marked `Blocked by: T-4, T-5, 70`.
  - `crates/horch-core/src/fleetd/fleet_runtime/adapters/channel_client.rs` is in `Own` of ticket 05 (`05-c2-t899-wrapper-operator.md`, T-5) and ticket 72 (R4-F3). Ticket 72 is marked `Blocked by: T-5, 70`.
  - `crates/horch-core/src/fleetd/fleet_runtime/adapters/channel_server.rs` and `crates/horch-core/tests/fleetd_channel.rs` are in `Own` of ticket 36 (`36-fix-r1-f2-windows-channel-bounded-write.md` from `FIX-INDEX-1.md`, plus T-4 for `fleetd_channel.rs`) and ticket 71 (R4-F2). Ticket 36 Fixes Windows reply write timeouts and active connection cancellation in `channel_server.rs`; ticket 71 fixes `ChannelServer::end` hanging in `accept` when `<fleet root>/run` or `fleetd.sock` is deleted before `stop()`. Ticket 71 is marked `Blocked by: 36`.
  - `crates/horch-core/src/fleetd/task_execution/application/dispatch.rs` is in `Own` of ticket 53 (`53-fix-r5-f6-direct-judge-output-ticket-and-kind.md` from `FIX-INDEX-2.md`) and ticket 73 (R4-F4). Ticket 73 is marked `Blocked by: the landing chain (T-15), 53`.

## Tests that do not prove their rule

The 7 points from `## Tests that do not prove their rule` in `docs/specs/fleetd/reviews/end-review-p5.md` are covered as follows:

1. **RR-8 / FM-15 (`session_records_applies_a_row_only_when_it_is_as_read`)**: Covered by finding ticket [70-fix-r4-f1-queued-request-refusal-visibility.md](70-fix-r4-f1-queued-request-refusal-visibility.md) (tests `refused_and_rejected_queued_request_files_open_needs_you_items` and `queued_session_records_is_not_reported_as_committed_and_is_recoverable_on_conflict`).
2. **RR-8 / FM-15 (Channel server start/stop unit tests)**: Covered by finding ticket [71-fix-r4-f2-channel-server-stop-without-socket-path.md](71-fix-r4-f2-channel-server-stop-without-socket-path.md) (test `stop_completes_when_socket_folder_is_removed_before_stop` in `crates/horch-core/tests/fleetd_channel.rs`).
3. **SR-5 (protected-file reader tests)**: Covered by finding ticket [72-fix-r4-f3-windows-protected-files-acl.md](72-fix-r4-f3-windows-protected-files-acl.md) (test `valid_owner_only_file_and_request_spool_succeed_on_host_platform` in `crates/horch-core/tests/protected_files.rs` and `crates/horch-core/tests/fleetd_request_files.rs`).
4. **SR-4 / RR-5 (dispatch binding tests)**: Covered by finding ticket [73-fix-r4-f4-dispatch-base-tip-outside-tx.md](73-fix-r4-f4-dispatch-base-tip-outside-tx.md) (test `bind_uses_pre_read_sandboxed_base_tips_and_runs_no_git_in_write_tx` in `crates/horch-core/tests/push_dispatch.rs`).
5. **SR-8 / FM-32 (request authorization and operator-boundary adapter tests — wrapper never receives or reads the operator token)**: Covered by existing ticket [05-c2-t899-wrapper-operator.md](05-c2-t899-wrapper-operator.md) (task T-5, requirement 4: tests proving the wrapper process cannot read `<fleet root>/run/operator.token`, a wrapper request of class `operator` is refused and recorded, and legacy pane launch gives the wrapper no path to the token).
6. **SR-8 / FM-32 (Codex adapter version and rendering tests)**: Covered by ticket [74-fix-r4-tests-prove-rules.md](74-fix-r4-tests-prove-rules.md) (test `codex_operator_boundary_conformance_refuses_unproven_version_and_probes_sandbox_acts` in `crates/horch-core/tests/p7_operator_boundary.rs`; plus operator decision OD-3 for real-CLI release conformance).
7. **RR-5 (`trf_45_session_db_is_the_task_ledger_unless_the_state_dir_moved` — inherited live `HORCH_LEDGER_DB`)**: Covered by ticket [74-fix-r4-tests-prove-rules.md](74-fix-r4-tests-prove-rules.md) (test `sealed_harness_strips_inherited_live_horch_ledger_db_and_fleet_root` in `crates/horch-e2e/tests/p5_operator_boundary.rs`).

## Minor findings

- **R4 (`end-review-p5.md`)**: None (0 Minor findings).

## Open questions

1. **OD-1 — Unlisted request kinds**:
   Keep `worker_progress`, `worker_done`, `run_adopt`, and `question_answer`, and add them to the Phase 6 MAP because current CLI flows use them. Retire `spawn_adopt`, which has no client. Keep the other 3 spawn record kinds only if the fleetd launch protocol still emits each one; otherwise retire them. The current catalog accepts all 8 as operator kinds. [verified recommendation; client-use inventory not fully verified] (Note: `ai_docs/plans/fleetd-build/tickets/61-wrapper-facts-go-by-attempt-files.md` step 3 also retires `spawn_adopt` and unused wrapper operator kinds as part of T-899 / T-5.)
2. **OD-2 — Windows support**:
   Decide whether Windows fleetd is a release target. The recommendation is to implement ACL-based protected-file reads before claiming Windows support. Otherwise, document that request-file fallback and sandboxed workers are unavailable there. [verified recommendation] (Ticket 72 implements the ACL-based protected-file and request-folder rule on Windows; if the operator chooses to declare Windows fleetd out of scope instead, ticket 72 can be replaced with the doc and `horch install` / `fleetd` refusal update.)
3. **OD-3 — Real CLI boundary gate**:
   Require one real Claude and one real Codex operator-boundary conformance run before release and after supported CLI version changes. Fake CLIs and rendered-policy tests cannot prove the external sandbox implementation. [verified recommendation]
