Reviewed tip: `3de43412982f2b9df45fe628106da517f4e30d38`.

## Summary

0 Critical, 1 Important, and 6 Minor findings.
The Important finding is the macOS task-check sandbox's access to services on the host LAN address.
The Minor findings are comment sweeps that removed a preservation, durability, security, or concurrency reason.
All findings are verified by static review. The ticket forbids `cargo test`, so no test was run.

## Findings

### R8-F1 — Important — SR-3/SR-4: a macOS task check can connect through the host LAN address

**Verified.** `crates/horch-core/src/fleetd/worker_profiles/adapters/check_sandbox.rs:23` promises no off-host network and says only loopback and Unix sockets are available. On macOS, `crates/horch-platform/src/mac/sandbox.rs:60` denies remote IP and then allows `localhost:*`; Seatbelt treats the host's own LAN address as local. The test `sr3_check_that_connects_off_host_is_refused` uses unreachable TEST-NET address `192.0.2.1` at `crates/horch-e2e/tests/p4_verifier.rs:518`, and at `:628` it explicitly expects a host loopback listener to be reachable on macOS.

A concrete check can connect to a service bound on the host's LAN address while its verdict says the check has only loopback access. This exposes host services outside the acceptance command's stated boundary, and the current test passes without testing that boundary.

Smallest fix: make the macOS sandbox deny each non-loopback interface, or return `NotSupported` until it can. Restore a live listener test on the host LAN address in addition to the TEST-NET test.

### R8-F2 — Minor — RR-13: comment sweep T-254 removed the moved-HEAD preservation invariant

**Verified.** Commit `0069a171` removed the explanation above `refuse_moved_head`, now at `crates/horch-core/src/fleetd/fleet_runtime/application/handlers_custody.rs:251`. The removed text states the non-obvious cross-job rule: fleetd compares the checked HEAD before queueing, the rescue job reads HEAD again, and removal occurs only when that rescued commit contains the later HEAD. That is why a commit after an accepted drop is not lost.

Smallest fix: restore the RR-13 explanation above `refuse_moved_head`.

### R8-F3 — Minor — RR-14/SR-6: comment sweep T-288 removed the unlanded-result contract

**Verified.** Commit `1bd038bd` removed the contract above `park_unlanded`, now at `crates/horch-core/src/fleetd/task_execution/application/verifier.rs:330`. The removed text tied the state transitions, same-tree requeue, blocked dependents, no-check behavior, and printable treatment of worker-controlled reason and summary to RR-14 and SR-6. The function remains correct, but the reason for bypassing the verifier and preserving the tree is no longer documented.

Smallest fix: restore the RR-14/SR-6 contract above `park_unlanded`.

### R8-F4 — Minor — D266-F3: comment sweep T-419 removed the crash-window reason from the pass

**Verified.** Commit `8967b416` removed the explanation above `Pass::record_locks_left`, now at `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs:634`. It stated that the lock journal closes the crash window between killing git and committing the needs-you item, so the next pass can recover the path.

Smallest fix: restore the D266-F3 crash-window explanation above `record_locks_left`.

### R8-F5 — Minor — D266-F3: comment sweep T-419 removed the journal-before-error ordering invariant

**Verified.** The same commit removed the explanation above `locks_left`, now at `crates/horch-core/src/fleetd/worktree_custody/adapters/git.rs:589`. The removed text states that lock paths enter the durable journal before the timed git error returns. That ordering is the durability guarantee reviewed in D266-F3.

Smallest fix: restore the journal-before-error invariant above `locks_left`.

### R8-F6 — Minor — D266-F3: comment sweep T-419 removed why the lock journal has its folder lock

**Verified.** Commit `8967b416` removed the explanation above `change_journal`, now at `crates/horch-core/src/fleetd/worktree_custody/adapters/git.rs:629`. The removed text says a job process and fleetd can update the journal concurrently; without it, the directory lock looks redundant and is vulnerable to later removal.

Smallest fix: restore the concurrent-writer reason above `change_journal`.

### R8-F7 — Minor — RR-5: comment sweep T-400 removed why role allocation needs a workspace lock

**Verified.** Commit `bfe32de9` removed the explanation above `ExecutionService::insert`, now at `crates/horch-core/src/execution/service.rs:144`. The removed text says the record store is optimistic and the role counter is a plain file, so two concurrent spawns can select one role unless allocation and the `Planned` record are protected by the same workspace lock.

Smallest fix: restore the race explanation above `ExecutionService::insert`.

## Watchlist lines covered

| Line | T-number | Result |
|---:|---|---|
| 48 | T-601 after T-892 | **Checked, no fault:** `crates/horch-e2e/tests/p4_verifier.rs:359` uses push dispatch, the production wrapper, `result.json`, and the verifier to reach `done`. The CLI remains fake, but the wrapper is no longer a stand-in. **Verified.** |
| 61 | T-561 after T-892 | **Checked, no fault:** `crates/horch-core/src/fleetd/task_execution/domain/verification.rs:102` makes a no-check judge `not_applicable`; the structural verdict decides it. **Verified.** |
| 67 | T-892 | **Checked, no fault:** an early exit writes `exit.json`; the pass ends the attempt and applies retry policy. `crates/horch-e2e/tests/p5_push_attempt_process.rs:338` covers exit without a result. **Verified.** |
| 70 | T-898 | **Checked, no code fault:** `crates/horch-core/src/fleetd/fleet_runtime/adapters/peer_process.rs` uses OS process facts, and `crates/horch-core/tests/p7_operator_boundary.rs:304` includes the double-fork orphan. Real Claude and Codex execution is **not verified**. |
| 71 | T-895/T-603 | **Checked, no code fault:** `crates/horch-core/tests/p7_wrapper_exec.rs` proves adapter behavior with fake CLIs only. Real Antigravity remains unsupported, so its real behavior is **not verified**. |
| 73 | T-895 | **Checked, no fault:** `spawn_adopt` is absent from `Kind::ALL` at `crates/horch-core/src/fleetd/fleet_runtime/domain/requests.rs:62` and a queued request is refused as retired by `crates/horch/tests/p6_retired_worker_commands.rs:623`. **Verified.** |
| 74 | T-898 | **Checked, no fault:** group stops snapshot members and recheck the recorded leader identity before signaling. The orphan intentionally relies on the sandbox after leaving the process ancestry. Real-CLI evidence is **not verified**. |
| 75 | T-898 G1 | **Checked, no fault against the current design:** `token_file.rs:60` reads an owner-only protected token under the owner-only run folder. Token possession is the request-file credential; sandbox denial is the worker separation. **Verified statically.** |
| 76 | T-895 | **Checked, no fault:** a live attempt with no proved process is not declared dead, but lease expiry still bounds it and ends it stalled. **Verified by code.** |
| 77 | T-900 | **Checked, no fault:** `crates/horch-e2e/tests/taskledger.rs:1432` proves review tasks create no check job or review worktree and allow the parent branch to land. **Verified.** |

## Tests that do not prove their rule

| Rule or change | Test or evidence | What is not proven |
|---|---|---|
| SR-3/SR-4 | `sr3_check_that_connects_off_host_is_refused` | It proves refusal of TEST-NET `192.0.2.1`, while it expects access to a listener on the host LAN interface on macOS. R8-F1. **Verified gap.** |
| FR-10/SR-8 | fake conformance and unit probes | They prove the wrapper and sandbox contracts with fakes, not the behavior of installed Claude and Codex. **Not verified.** |
| RR-12 | `answered_question_resumes_the_session_of_the_worker` and the refused-resume cases in `p6_worker_question.rs` | They now prove both same-session resume and fresh fallback with a fake CLI. A real CLI resume is **not verified**. |
| T-249 | `watch_once_reports_in_text_and_json` | It proves the observer's dead verdict, not lease expiry. Generic lease tests and code establish the bound. **Verified by code, not this test.** |
| T-232 reviewer fallback | quota fallback tests | They prove fallback after a usage limit. They do not prove fallback from an unhealthy review profile; the current Q15 ruling intentionally holds it. **Verified product decision.** |
| T-292 paged/set reads | statement-count and equivalence tests | They prove bounded SQL and fixture equivalence, not every concurrent mutation between pages. The readers use snapshots per set and cursors between sets, matching the prior pass semantics. **Verified by code.** |
| FM-36 | `p4_landing::settled` | It now reads bundles and branches in one SQLite statement, but remains test-harness observation rather than product synchronization. **Verified as adequate test polling, not a product proof.** |

### Deleted and weakened tests audited

- **T-15 (`6c8a4f45`), verified:** `g52_done_discovery_uses_the_same_launch_token` retired pane-spawn discovery. `g52_resume_uses_own_session` was deleted before RR-12 resume existed; current `answered_question_resumes_the_session_of_the_worker` and the refused-resume cases now prove resume plus fresh fallback.
- **T-90 (`9e1efb7d`), verified:** `add_map_queues_a_ticket_map_in_one_step` retired queueing because FR-12 forbids a task without an idea. `p6_retired_run_commands::add_map_is_retired_and_its_dry_run_stays` and `trf_11_add_map_cli` cover the retained dry-run behavior. The commit message accounts for each dropped push-form assert.
- **T-232 (`37aa09a1`), verified:** nine deleted tests cover four worker land/push cases, changed commits after land, two reviewer-fallback cases, new-tree review supersession, and same-tree round reuse. FM-43 is covered by `fm43_transient_push_failure_is_followed_by_a_new_run_of_the_push_job`; one-time landing is covered by `crash_before_and_after_the_commit_of_the_ref_move_lands_once` and I8 tests. Push dispatch prevents author-tree mutation after an attempt ends. The old `Tools.reviewers_down` and manual fallback paths are retired; quota fallback remains covered.
- **T-78 (`78684eea`), verified:** `each_launch_gets_only_its_own_rules` now asserts on retained orchestrator-only `horch sessions` rather than retired `horch spawn` behavior.
- **T-77 (`54ebe0e8`), verified fixed:** commit `c4594687` restored the reason for the module-wide `dead_code` allowance.
- **T-209/T-236, verified:** T-209 did not reproduce the hidden write. T-236 identified the mutating wait judge. `workers::waits_now` is read-only, while the fleetd pass owns wait-ending writes.
- **T-249 (`f0a907f2`), verified:** the observer-only dead verdict does not mutate the task. The pass ends the attempt through `exit.json` or lease expiry.
- **T-246 (`2ca577e1`), verified faulty:** replacing the host-LAN listener with TEST-NET weakened the SR-3 assertion. R8-F1.
- **T-287 (`1f919fea`), verified:** `rr13_drop_with_a_head_that_moved_during_the_checks_is_refused_and_the_tree_stays` proves the pre-acceptance refusal. `rr13_a_commit_after_an_accepted_drop_is_kept_in_the_rescue_ref` proves the post-acceptance race. The code remains correct; R8-F2 is its removed invariant comment.
- **T-288 (`88de7014`), verified:** `rr14_done_unlanded_in_a_fleetd_attempt_parks_the_task` proves the tree stays, dependents remain pending, and requeue uses the same tree. A worker can deliberately park its own task, but gains no verifier pass or landed state; the operator must requeue it. That is RR-14's worker-reported unlanded flow. R8-F3 covers the removed contract comment.
- **T-294/T-286, verified:** retired wakes are asserted absent under FM-16. The cancel and release paths remain ledger-driven.
- **T-309, verified:** Linux jobs use the inner `setsid` in the PID namespace and are stopped through the sandbox leader; agent launches retain host PID facts and `--new-session`. The security reasons removed by its sweep were restored by `d70a4032`.
- **T-320, verified:** `domain/conformance.rs:545` classifies only explicit permission/refusal errors as refusal. ENOENT counts only when the probe and host see the same fleet-root identity. Other conformance checks do not turn arbitrary errors into Pass.
- **T-292, verified:** lease repair is page-budgeted; disk-budget and Own-file readers use set reads; snapshot and equivalence tests compare their prior row-by-row results. No changed selection rule was found.
- **Comment sweeps, verified:** I inspected every run commit whose subject contains `comment sweep`. Restorations cover the earlier T-77, T-89, T-172, T-286, T-300, T-309, T-319, T-327, T-332, T-335, T-338, T-342, T-356, T-365, T-368, T-382, T-383, T-386, and T-389 reason removals. R8-F2 through R8-F7 are the remaining removals that state a rule, invariant, race, or security reason.

### Other orchestrator points

1. **RR-12/design line 1015 — verified fixed.** `launch.rs:446` records `resumed_from_session`; `p6_worker_question.rs:478` proves resume, and its refused-resume cases prove fresh fallback.
2. **T-236 waits — verified.** The fleetd pass ends each wait omitted by `workers::waits_now`; the read CLI performs no write.
3. **T-249 bounded dead attempt — verified by code.** The observer does not end it; `exit.json` or lease expiry does.
4. **T-254 `worker_launch_failed` — verified.** It is an operator-class kind at `requests.rs:140`. Only the wrapper/launch helper has the token path, and server authorization still applies.
5. **T-286/T-294 retired wakes — verified.** The replacement asserts require no old wake under FM-16.
6. **T-287 RR-13 — verified.** Both sides of the acceptance race have tests; see above.
7. **T-288 RR-14 — verified.** The tree and dependent behavior match RR-14; a malicious worker can park but cannot land or mark verified.
8. **T-300 failed resume — verified.** A refused resume records `resume_refused`, burns no retry budget, and starts fresh immediately. This correctly treats native-resume refusal as a platform path, not a task failure.
9. **T-309 terminal protection — verified.** Agent and job paths use separate, intentional process/session models, and every stop path checks recorded identity.
10. **T-320 false Pass — verified fixed.** No equivalent error-only Pass was found.
11. **T-292 readers — verified.** Paging and set reads preserve the prior selections and use snapshot boundaries where a set must agree.

## Operator decisions

1. **macOS LAN boundary.** Recommendation: keep the documented loopback-only task-check contract and make macOS return `NotSupported` until it can enforce it. Do not broaden SR-3 silently. **Verified mismatch; R8-F1.**
2. **Real CLI release evidence.** Recommendation: run the logged-in real Claude and Codex conformance suite before release, including the orphan operator-boundary probe. Fake adapters cannot prove installed CLI behavior. **Not verified.**
3. **Antigravity release scope.** Recommendation: keep Antigravity outside the release claim until its real sandbox and private-home conformance is green. **Not verified by design.**
4. **Unhealthy review fallback.** Recommendation: use a healthy cross-provider fallback for review tasks instead of holding indefinitely. Current behavior follows Q15, so this is a product decision rather than a finding. **Verified behavior.**
5. **Token-only request files.** Recommendation: retain the owner-only token plus owner-only spool as the file credential unless a platform-neutral peer proof is designed. **Verified statically.**
