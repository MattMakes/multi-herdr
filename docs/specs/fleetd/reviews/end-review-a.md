Reviewed tip: `08c6d4ffd9341a5de41c1b10af251a492406c045`.

## Summary

0 Critical, 3 Important, and 2 Minor findings.
The Important findings are the missing RR-12 session-resume arm, the macOS check-sandbox LAN escape, and the accepted dead `spawn_adopt` request.
The Minor findings are two comment sweeps that removed the reason for otherwise surprising code.
All findings are verified by static code review at the tip above; I ran no test.

## Findings

### R8-F1 — Important — RR-12: an answered question never resumes the worker session

**Verified.** `docs/specs/fleetd/design.md:165` requires fleetd to resume the worker with the answer or start a fresh worker from the question, answer, and committed work. The implementation only builds the latter: `crates/horch-core/src/fleetd/task_execution/application/dispatch.rs:637` collects answered questions for the next `START.md`, and `crates/horch-core/src/fleetd/task_execution/domain/task_files.rs:124` renders them. `crates/horch-e2e/tests/p6_worker_question.rs:347` proves that fresh-attempt arm.

No production path sets `workers.resumed_from_session`; its only references are the schema/model reads and insert field in `crates/horch-core/src/taskledger/workers.rs:47`, `:101`, `:187`, `:214`, and `:230`. Thus design line 1015's “fleetd resumes the worker's session with the answer” is not met. A concrete case is a worker that asks one clarification after building substantial conversational context: the answer starts A2 with text and committed files, but not the model session of A1.

Smallest fix: when an answered question has a prior attempt session id, launch the next attempt with that session and record `resumed_from_session`; retain the current fresh-attempt path as the fallback. Add one binary test for same-session resume and one for fresh fallback. T-15 correctly retired manual `horch spawn --resume`, but deleting `g52_resume_uses_own_session` left this still-required RR-12 arm without code or a test.

### R8-F2 — Important — SR-3/SR-4: a macOS task check can connect through the host LAN address

**Verified.** The check contract says “loopback and Unix sockets only” in `crates/horch-core/src/fleetd/worker_profiles/adapters/check_sandbox.rs:45`. On macOS, `crates/horch-platform/src/mac/sandbox.rs:73` denies remote IP and then allows `localhost:*`; Seatbelt also treats the host's own LAN address as local. Commit `2ca577e1` changed `sr3_check_that_connects_off_host_is_refused` from a listener on that address to unreachable `192.0.2.1`, and its new comment explicitly records that the LAN connection is allowed (`crates/horch-e2e/tests/p4_verifier.rs:554`).

A concrete check can connect to a service bound on the machine's LAN address even though the verdict text promises that only loopback is reachable. This permits communication with host services outside the check's declared boundary and means the current test can pass while the documented rule is false.

Smallest fix: make the macOS check sandbox deny every non-loopback interface, or mark macOS check sandboxing unsupported until the platform can enforce that boundary. Restore a live listener test on the host LAN address in addition to the TEST-NET address.

### R8-F3 — Important — SR-8 command-channel contract: fleetd still accepts dead `spawn_adopt`

**Verified.** The design's request catalog at `docs/specs/fleetd/design.md:342` does not include `spawn_adopt` and says fleetd refuses unknown request kinds. Nevertheless, `crates/horch-core/src/fleetd/fleet_runtime/domain/requests.rs:53`, `:95`, and `:136` keep it in the accepted operator catalog, and `crates/horch-core/src/fleetd/fleet_runtime/application/handlers_worker.rs:57` registers its state-changing handler. `crates/horch/tests/p6_retired_worker_commands.rs:108` also classifies it as a retained internal record even though it has no client after T-895.

A holder of the operator token can therefore submit a legacy adoption payload and mutate a spawning worker through an endpoint that no supported flow uses. Authentication limits the caller, but the dead endpoint still enlarges the state-machine and request attack surface beyond the design.

Smallest fix: move `spawn_adopt` to `RETIRED`, remove its payload validator and handler, and update the retired-request catalog tests. The watchlist already assigns this cleanup to T-877.

### R8-F4 — Minor — comment sweep T-96 removed the explanation of a first-writer race

**Verified.** Commit `f1e2b0b3` removed the only comment on `has_session_records_table` at `crates/horch-core/src/execution/store.rs:732`. The removed text explains why a reader can observe a newly created ledger before migration commits the `session_records` table. The function otherwise looks like defensive schema probing with no reason to exist.

Smallest fix: restore the two-line race explanation above `has_session_records_table`.

### R8-F5 — Minor — comment sweep T-77 left a module-wide lint suppression unexplained

**Verified.** Commit `54ebe0e8` removed “Not each test file that uses `support` starts a test fleetd” immediately above `#[allow(dead_code)]` at `crates/horch/tests/support/mod.rs:4`. The remaining module-wide suppression can hide unrelated dead helpers and no longer states why it is needed.

Smallest fix: restore that reason, or narrow the allowance to the specific conditional helper that needs it.

## Watchlist lines covered

| Line | Point | Result |
|---:|---|---|
| 48 | T-601 after T-892 | **Checked, no fault:** `crates/horch-e2e/tests/p4_verifier.rs:359` now starts fleetd, lets push dispatch launch the fake agent through the production wrapper, writes `result.json`, runs the check job, and reaches `done`. The fake model remains a test double, but the wrapper is no longer a stand-in. Verified. |
| 61 | T-561 judge verification after T-892 | **Checked, no fault:** `crates/horch-core/src/fleetd/task_execution/domain/verification.rs:102` gives a no-check judge `not_applicable` so its structural verdict decides it; `crates/horch-core/tests/idea_direct.rs:629` covers the direct judge output. Verified. |
| 67 | T-892 early exit visibility | **Checked, no fault:** `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs:2244` ends a proven exit as `asked`, `quota`, or `no_result`; `crates/horch-e2e/tests/p5_push_attempt_process.rs:338` covers exit without result. Verified. |
| 70 | T-898 FR-10 on three OSes and real CLIs | **Checked, no code fault:** `crates/horch-core/src/fleetd/fleet_runtime/adapters/peer_process.rs:1` uses OS process identity, ancestry, group, and session facts, and `crates/horch-core/tests/p7_operator_boundary.rs` probes the sandbox. The required real Claude/Codex run is not verified; see Operator decisions. |
| 71 | T-895/T-603 agy adapter-only proof | **Checked, no code fault:** the fake exec tests prove adapter behavior only. Real agy conformance remains intentionally unsupported/unhealthy at this tip and is not verified. |
| 73 | T-895 dead `spawn_adopt` | **Finding R8-F3.** Verified. |
| 74 | T-898 orphan and process-group reuse | **Checked, no fault:** `crates/horch-core/src/fleetd/fleet_runtime/adapters/peer_process.rs:276` matches recorded process groups only while their leader identity/start time still agrees; the double-fork orphan deliberately falls back to the sandbox boundary. Real CLI evidence is not verified. |
| 75 | T-898 token-only request file | **Checked, no fault against the current design:** `crates/horch-core/src/fleetd/fleet_runtime/adapters/token_file.rs:79` requires an owner-only 0600 token under an owner-only 0700 run folder, and `request_spool.rs:296` requires the request folder to remain owner-only. Token possession is the designed file credential; the worker sandbox is the separation boundary. Verified statically. |
| 76 | T-895 live attempt with no process and no pane | **Checked, no fault:** `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs:3073` does not infer death without proof, but `:2373` still renews or expires every live attempt lease and `:2426` ends an expired one `stalled`. Thus `exit.json` ends it immediately, and otherwise the lease bounds it. Verified. |
| 77 | T-900 push review contract | **Checked, no fault:** `crates/horch-e2e/tests/taskledger.rs:1450` proves review tasks write findings, end `verified`, create no review worktree or check job, and let the parent task land. Verified. |

## Tests that do not prove their rule

| Rule or change | Test/evidence | Missing proof |
|---|---|---|
| RR-12 | `answer_by_tell_answers_reaches_the_next_attempt` | Proves only the fresh-attempt arm. It does not prove resuming the answered worker's model session. Verified gap; R8-F1. |
| SR-3/SR-4 | `sr3_check_that_connects_off_host_is_refused` | Proves refusal of TEST-NET `192.0.2.1`; it deliberately stopped proving refusal of the host LAN address. Verified gap; R8-F2. |
| FR-10/SR-8 | fake conformance and unit probes | They prove the wrapper and sandbox contracts with fakes, not that currently installed real Claude and Codex preserve the orphan/operator boundary. Not verified. |
| T-601/T-892 | `push_dispatched_task_launches_writes_task_files_and_reaches_done_through_verifier` | Proves the production push/wrapper/check path with a fake CLI, not behavior of a real model CLI. Verified for the wrapper path only. |
| T-232 reviewer fallback | `usage_limited_worker_restarts_on_the_fallback_and_burns_no_attempt` and `exec_usage_limit_restarts_on_the_fallback_and_burns_no_attempt` | Prove RR-11 after a quota signal. They do not prove that an unhealthy reviewer profile changes to its task-specific fallback. The current design explicitly holds unhealthy profiles with no fallback at `domain/dispatch.rs:16`; this needs the operator decision below, not a code finding against the current Q15 ruling. |
| T-249 unproven live attempt | `watch_once_reports_in_text_and_json` | Proves the observer reports `dead` without mutating the task. It does not itself advance the fleet clock to prove lease expiry; the bounded exit is established by the generic lease path at `pass.rs:2373`. Verified by code, not this test. |

### Deleted and weakened tests audited

- **T-15 (`6c8a4f45`), verified:** `g52_done_discovery_uses_the_same_launch_token` retired pane-spawn discovery; `g52_concurrent_workers_get_their_own_session_and_transcript` keeps launch-token isolation and fleetd reads attempt `session.json`. `g52_resume_uses_own_session` retired manual `horch spawn --resume`, but its commit message correctly admits that no test proves RR-12 answer resume; that gap is R8-F1.
- **T-90 (`9e1efb7d`), verified:** `add_map_queues_a_ticket_map_in_one_step` retired the queueing arm because FR-12 forbids a task without an idea. `p6_retired_run_commands::add_map_is_retired_and_its_dry_run_stays` and `trf_11_add_map_cli` cover the remaining dry-run behavior. The commit message maps each other dropped assertion to push verifier, worktree disk, feature PR, review-fix, shared-task, or no-wake behavior; those replacement paths exist.
- **T-232 (`37aa09a1`), verified:** the nine deleted tests are the four worker-land/push tests, `a_commit_after_the_land_is_checked_and_reviewed_again`, two reviewer-fallback tests, `a_new_tree_supersedes_the_open_reviews_of_the_old_round`, and `a_replacement_with_the_same_tree_reuses_the_finished_round`. FM-43 is covered by `p4_remote::fm43_transient_push_failure_is_followed_by_a_new_run_of_the_push_job`; one-time landing is covered by `fleetd_pass_p4b::crash_before_and_after_the_commit_of_the_ref_move_lands_once` and the I8 tests; changed-tip review is covered by the review input key and round-two fix test. Push dispatch prevents a worker from changing the ended author's tree during open reviews, and a passed sweep ends the task. The two old fallback mechanisms (`Tools.reviewers_down` and manual `ledger fallback`) are retired; quota fallback remains covered, while unhealthy-profile behavior is an explicit Q15 hold.
- **T-78 (`78684eea`), verified:** `each_launch_gets_only_its_own_rules` replaced the retired `horch spawn` distinction with `horch sessions`, which remains orchestrator-only at `crates/horch-core/src/harness/codex.rs:2743`.
- **T-77 (`54ebe0e8`), verified:** the code did not change, but the explanatory comment removal is R8-F5.
- **T-209/T-236, verified:** T-209 added read-only diagnostics without reproducing the hidden write. T-236 found it: status called the mutating wait judge. `workers::waits_now` at `workers.rs:999` now filters ended waits without writes, while the fleetd pass calls `workers::waiting` at `pass.rs:2388` and commits `wait_ended` or the moved cap.
- **T-249 (`f0a907f2`), verified:** the changed expectation is correct for the observer-only `watch` command; the pass, not `watch`, owns the bounded lease exit.
- **T-246 (`2ca577e1`), verified and faulty:** replacing the host LAN listener with TEST-NET weakened the SR-3 assertion; R8-F2.
- **Comment sweeps, verified:** I inspected every commit whose subject contains `comment sweep`. The T-89 security comments were restored by `9642a990`. Security-boundary and release-gate explanations removed in other sweeps still exist at their owning production or script sites. The two reason removals not preserved elsewhere are R8-F4 and R8-F5.
- **T-287 and T-288, not verified at this early tip:** commits `fc994679` and `88de7014` are not ancestors of `08c6d4ff`. The later review must audit the renamed RR-13 test and the RR-14 `unlanded` result key.

## Operator decisions

1. **Unhealthy review profiles.** The current Q15 design visibly holds a task whose primary profile is unhealthy and does not use its task-specific fallback (`domain/dispatch.rs:16`). Recommendation: change the ruling for review tasks so a healthy cross-provider fallback runs immediately; otherwise one unavailable reviewer can hold the author's finish pipeline indefinitely. Verified behavior; product choice required.
2. **Real CLI release evidence.** The code and fake probes cannot prove the double-fork orphan boundary for installed Claude and Codex. Recommendation: before release, the operator runs `scripts/live/p7-real-conformance.sh` for both CLIs in a logged-in shell and records the `Operator boundary` evidence. Not verified in this review.
3. **macOS LAN meaning.** The implementation currently treats the machine's LAN address as acceptable local access while its own diagnostic promises loopback only. Recommendation: keep the stricter loopback-only contract and fix or disable the macOS sandbox, rather than broaden SR-3 silently. Verified mismatch; R8-F2.
4. **Token-only request files.** The current design treats the owner-only token plus owner-only spool as the complete file credential. Recommendation: keep this contract unless a platform-neutral, non-worker-forgeable peer proof is specified; do not add an ad hoc payload proof. Verified statically.
5. **Antigravity release scope.** Current tests prove its adapter with a fake while real agy lacks the required sandbox/private-home guarantees. Recommendation: keep agy excluded from the release claim until its real conformance report is green. Not verified.
