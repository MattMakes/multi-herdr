# End review: phases 2 and 3

## Summary

- Critical: 0.
- Important: 1.
- Minor: 0.
- The verified finding is an all-items rollback after triage has already performed per-item git effects.

## Findings

### R2-F1 — Important — FM-11, RR-7, RR-16, RR-17 — verified

`triage_apply` performs every `ARCHIVE`, `DELETE`, `LAND`, or `PR` effect independently and appends one fact per item (`crates/horch-core/src/taskledger/triage/apply.rs:1482`). Ingestion then checks and records those facts in order (`crates/horch-core/src/taskledger/triage/apply.rs:1673`). If a later fact is refused, `record_facts` rolls back every earlier ledger write and changes the whole job to `failed` (`crates/horch-core/src/fleetd/fleet_runtime/application/jobs.rs:881`).

Concrete case: item 1 writes a verified rescue ref and removes its folder. Item 2 returns a malformed ref, commit, text, or worktree id. Item 1's effect has occurred, but item 2 makes fleetd roll back item 1's `rescued`/`removed` rows. The result is archived and the job ends failed, so the ledger does not record item 1's completed effect. This violates FM-11's rule that an effect is represented by an intended row and then observed idempotently. It also defeats the T-887 preservation fix when another item poisons the same result.

Smallest fix: settle each decision under its own savepoint. A refused fact must fail only its decision and must not roll back valid facts for other decisions. Add a mixed-result test with at least 2 items: one completed rescue/removal and one refused fact. Assert that the completed item's rescue ref and removed state remain recorded.

## Watchlist lines covered

| Line | Ticket | Result |
| ---: | --- | --- |
| 19 | T-469 | Checked. Worktree drop still uses client-read dirty, stash, branch, and uniqueness facts for the immediate refusal (`crates/horch-core/src/fleetd/fleet_runtime/application/handlers_custody.rs:228`), but removal or rescue runs as a custody job and rechecks the destructive effect (`crates/horch-core/src/fleetd/fleet_runtime/application/handlers_custody.rs:313`). Triage `LAND` and `PR` now run in `triage_apply` jobs (`crates/horch-core/src/fleetd/fleet_runtime/application/handlers_custody.rs:573`). No additional phase 2/3 fault verified. |
| 28 | T-376 | Checked, no fault. A done task is a lease holder only while its branch needs repair (`crates/horch-core/src/fleetd/task_execution/application/lease.rs:193`). Expiry ends the repair attempt and worker without moving the done task (`crates/horch-core/src/fleetd/task_execution/application/lease.rs:201`). I1 reports 2 live attempts and does not guess which one to stop (`crates/horch-core/src/fleetd/task_execution/application/lease.rs:342`). |
| 35 | T-888/T-887 | Checked. All 7 named `p3_triage` tests retain their preservation or refusal assertion. They cover committing untracked work, unique detached-head refusal, trash-ref deletion, forward ref movement/readback, failed readback retention, non-overwriting rescue refs, and a replacement folder at a removed path (`crates/horch-e2e/tests/p3_triage.rs:201`, `crates/horch-e2e/tests/p3_triage.rs:243`, `crates/horch-e2e/tests/p3_triage.rs:264`, `crates/horch-e2e/tests/p3_triage.rs:294`, `crates/horch-e2e/tests/p3_triage.rs:335`, `crates/horch-e2e/tests/p3_triage.rs:366`, `crates/horch-e2e/tests/p3_triage.rs:485`). R2-F1 identifies their missing multi-item boundary. |
| 36 | T-881 | Product gap remains, not a phase 2/3 implementation fault. `g52_resume_uses_own_session` is still an expected failure because push dispatch can claim the ready task before manual resume (`crates/horch-e2e/expected-failures/fleet18_observability.txt:3`). See Operator decisions. |
| 37 | T-888/T-887 | R2-F1. Within one item, `run_item` preserves a rescue ref and removal after any later failure (`crates/horch-core/src/taskledger/triage/apply.rs:1530`). Across items, a refused later fact rolls back the preserved earlier fact. |
| 41 | T-881 | Checked, no current fault. A killed agent that reached live state ends through rung 5 as `no_result`, and its worker is `incomplete`, not `launch_failed` (`crates/horch-e2e/tests/p5_push_attempt_process.rs:352`). `launch_failed` is now reserved for a process that did not start (`crates/horch-core/src/fleetd/task_execution/application/launch.rs:358`). |
| 50 | T-396/T-397 | Checked, no current fault. Each repair bind has its own savepoint; a failed bind opens one `repair_start` item and the loop continues (`crates/horch-core/src/fleetd/task_execution/application/repair.rs:282`). `repair_that_cannot_start_fails_only_its_branch` proves a second branch starts after the first has no worktree (`crates/horch-core/tests/repair_attempts.rs:834`). |
| 51 | T-397 | Checked. A missing worktree reaches the explicit `repair_start` path (`crates/horch-core/src/fleetd/task_execution/application/repair.rs:622`). The item retries every pass and offers branch abandonment (`crates/horch-core/src/fleetd/task_execution/application/repair.rs:364`). No silent stall exists, but the design needs the decision below about recovery versus abandonment. |
| 63 | T-397 | Checked, no code fault reproduced. Every orphaned row with a known HEAD is considered for rescue (`crates/horch-core/src/fleetd/worktree_custody/application/reconcile.rs:652`). An active run suppresses duplicates; an abandoned run retries; a failed run deliberately remains visible and does not retry the same HEAD (`crates/horch-core/src/fleetd/worktree_custody/application/reconcile.rs:696`). |
| 72 | T-397 | Not verified in phase 2/3 code. The reported fetch process hang is in the integration job executor, outside this review's owned implementation. Its impact is relevant: a held git-writer queue can delay the rescue and repair paths reviewed here. Treat it as an unresolved release dependency, not as evidence against the phase 3 rescue queue. |

## Tests that do not prove their rule

| Rule | Test | Missing proof |
| --- | --- | --- |
| FM-11, RR-16, RR-17 | The 7 named tests in `crates/horch-e2e/tests/p3_triage.rs` | Each proves its decision in isolation. None submits a multi-item `triage_apply` result where a later refused fact must not erase an earlier completed effect. |
| FM-38 | `repair_that_cannot_start_fails_only_its_branch` in `crates/horch-core/tests/repair_attempts.rs:840` | It proves isolation and visibility when a worktree row is marked cleaned. It does not prove whether normal triage or import can create that state before a conflicted or gate-failed branch is settled. |
| FM-13 | `g52_resume_uses_own_session` in `crates/horch-e2e/tests/fleet18_observability.rs:500` | The test is an expected failure. It therefore documents that manual session resume loses the push-dispatch race; it does not prove an operator can resume the old session. |
| FM-12, FM-25 | Phase 3 rescue queue tests | They prove queue state transitions, not that an unrelated fetch process releases the shared git-writer queue within the pass or job deadline. Watchlist line 72 remains external evidence of that missing integration proof. |

## Operator decisions

1. **Manual resume under push dispatch:** retire `horch spawn --resume` for fleet-managed worker tasks, or add a ledger reservation that suppresses dispatch while resume binds. Recommendation: retire the manual worker resume path. FM-13 already specifies fresh attempts in the same worktree, and a reservation would add a second dispatch protocol.
2. **Repair with no worktree row:** decide whether this is an impossible invariant breach or a supported recovery state. Recommendation: define it as an invariant breach, keep the visible `repair_start` item, and add tests that triage and import cannot clean the last tree of a conflicted or gate-failed branch. Automatic reconstruction risks choosing a tree after custody has intentionally removed it.
3. **Fetch process hang from watchlist line 72:** assign the integration/job end review to reproduce or close it before release. Recommendation: require a bounded-process test that proves timeout, result ingestion, and git-writer release when a project has no origin.
4. **R2-F1 settlement boundary:** accept per-decision failure isolation as the rule. This matches the existing per-item effect loop and the design rule that one bad row does not stop a pass.
