# Aggregate Design Canvas - Bundle

Context `integration` · aggregate id `bundle` · pattern domain-model · lives in `crates/horch-core/src/fleetd/integration/domain/bundle.rs`
Discover candidate: none. strategize N5 names the bundle state machine as an aggregate of integration, and the design gives it the tables `bundles` and `bundle_members` (A27, A34). It takes build-bundle, run-fast-gate and bisect-bundle from the `task-branch` candidate (A17) · canvas format: https://github.com/ddd-crew/aggregate-design-canvas

## 1. Name
**Bundle** - the ready task branches of 1 feature, built on the feature branch tip in queue order and judged by 1 run of the fast gate (A27).

## 2. Description
The Bundle decides when a group of verified task branches lands on its feature branch. fleetd writes the bundle and its members before the build job, so a crash never forgets a member. The bundle moves the feature ref only by compare-and-set from its base tip, and it owns its candidate ref `refs/horch/bundle/<id>` until the ref is deleted.
Alternative rejected: the bundle as a value object of TaskBranch, because a bundle spans several task branches, has its own state machine and its own row, and must survive a crash between its build and its land.

## 3. State transitions
```
building → gating → passed → landed
building → dropped    (member conflict, build failure, moved feature tip)
gating → failed       (fast gate failed; bisect into child bundles)
gating → dropped      (moved feature tip)
passed → dropped      (compare-and-set of the feature ref failed)
```
| From | Command | To | Rejected when |
|---|---|---|---|
| none | `build-bundle` | building | a member is in another bundle that is not ended (I12) |
| building | `run-fast-gate` | gating | the bundle_build job did not pass |
| gating | (fast_gate job passed) | passed | - |
| gating | `bisect-bundle` | failed | the fast_gate job did not fail |
| passed | (feature ref move observed) | landed | the compare-and-set did not move the ref |
| building, gating, passed | (feature tip moved, member conflict, failed compare-and-set) | dropped | - |

## 4. Enforced invariants
| # | Invariant (same wording as `code.json` and the test name) | Whose job is it - why transactional |
|---|---|---|
| U1 | fleetd writes the bundles row and its bundle_members rows in 1 transaction before the bundle_build job (A27) | fleetd's, in part 2 of the pass |
| U2 | A bundle moves the feature ref only by compare-and-set from its base tip; a failed compare-and-set drops the bundle and its members go back to ready (I9, A34) | fleetd's, in part 3 of the pass (git update-ref) |
| U3 | A task branch is a member of at most 1 bundle that is not ended (I12) | fleetd's, at build-bundle |
| U4 | fleetd deletes the candidate ref refs/horch/bundle/<id> of a landed, failed or dropped bundle when no child bundle of it is building, gating or passed, and reads the ref store again to verify the deletion (A34) | fleetd's, every pass; a failed deletion is tried again |
| U5 | The fast gate runs once on a bundle, as a fast_gate job keyed by the candidate tip and the base tip (A23, A33) | the job runner's: a passed result is used again |

## 5. Corrective policies
| Situation found late | Then | Within | Owner |
|---|---|---|---|
| A bundle is dropped | its members go back to ready, and the next bundle is built from the ready members (rebundle-on-dropped) | 1 pass | integration |
| A bundle fails the fast gate | bisect into child bundles; the failing branch goes to gate_failed | 1 pass per level | integration |
| A candidate ref deletion fails | try again on the next pass; horch status shows the count of bundle refs | 1 pass | integration |
| The build or gate job passes its time limit plus 10 min | the job ends abandoned and runs again | 1 pass | fleet-runtime job runner |

## 6. Handled commands → created events
| Command (`discover` id) | Preconditions | Event(s) (`discover` id) | Payload (from `events[].data`) |
|---|---|---|---|
| `build-bundle` | ready task branches of 1 feature; no live bundle of the feature in gating or passed | `bundle-built`, `bundle-dropped` | feature_id, branches, queue order, base tip |
| `run-fast-gate` | the bundle_build job passed | `fast-gate-passed`, `fast-gate-failed` | feature_id, candidate tip, base tip |
| `bisect-bundle` | the fast gate failed | `task-branch-gate-failed` (TaskBranch), child bundles | branch, task_id, gate_output |

## 7. Throughput
| Metric | Average | Maximum | Basis |
|---|---|---|---|
| Commands per instance | 3 | 10 | 1 build, 1 gate, a bisect of a few levels |
| Concurrent clients per instance | 1 | 1 | fleetd is the only writer |

Conflict risk: low → no optimistic retry (A5).

## 8. Size
| Metric | Estimate | Basis |
|---|---|---|
| Events per instance | 2 to 6 | as above |
| Lifetime of an instance | minutes to 1 hour | the build job and the gate job |

Size: small → no snapshots needed.

## 9. Model sketch
- Root entity `Bundle`: BundleId, FeatureId, BundleState, base tip, CandidateRef, parent BundleId, version.
- Entities inside: `BundleMember` (task branch name, queue order, member tip).
- Value objects: BundleId, BundleState, CandidateRef, Tip, JudgedTree, GateResult.
- References to other aggregates by id: FeatureId, TaskBranch names, the job ids of its bundle_build and fast_gate jobs.
- Internal domain events: the events in §6 → written as event rows by `LedgerEventLog`.
- A bundle and its member task branches change in the 1 transaction of part 2 of a pass (A24, A17).

## 10. Notes & open questions
- fleetd does not queue a rebase of the feature branch while 1 of its bundles is in gating or in passed (A34). The Feature aggregate checks it.
- The domain service BundleBisector splits a failed bundle into child bundles.
