# Aggregate Design Canvas - TaskBranch

Context `integration` · aggregate id `task-branch` · pattern domain-model · lives in `crates/horch-core/src/fleetd/integration/domain/task_branch.rs`
Discover candidate: `task-branch` (this step adds open-triage-task; build-bundle, run-fast-gate and bisect-bundle move to the Bundle aggregate, A17) · canvas format: https://github.com/ddd-crew/aggregate-design-canvas

## 1. Name
**TaskBranch** - the branch of 1 task, a ledger row before it exists in git, from building to landed or abandoned (D10).

## 2. Description
The TaskBranch decides when a verified task's work may land on its feature branch, under the land lock and behind the fast gate, and when the branch may be deleted. Every fleet branch in git has a row; an unknown one is adopted, never deleted. fleetd owns `conflicted` and `gate_failed`: it starts a resolver attempt or a fix attempt of the task in its worktree, and counts each failure in `repair_attempts` of the branch while the task stays `done` (A27, A34).
Alternative rejected: the landing state on the Task aggregate, because landing is a git fact of integration and the task is done before it lands.

## 3. State transitions
```
building → verifying → ready → in_bundle → landed
verifying → building                 (the check fails, a new attempt)
in_bundle → ready                    (bundle dropped, or bisect finds it clean)
in_bundle → conflicted → ready       (resolver attempt passes its check)
in_bundle → gate_failed → verifying  (fix attempt passes the check)
conflicted | gate_failed → same      (failed repair attempt: repair_attempts + 1)
building | verifying → abandoned     (task cancelled, after worktree-rescued)
conflicted | gate_failed → abandoned (horch branch abandon, after worktree-rescued)
any not landed → abandoned           (horch feature abandon, rescue ref first)
```
| From | Command | To | Rejected when |
|---|---|---|---|
| verifying | `mark-branch-ready` | ready | the task is not done; a judged task whose judge task is not done |
| ready | `build-bundle` (Bundle) | in_bundle | the branch is a member of another bundle that is not ended (I12) |
| in_bundle | `fast-forward-feature-branch` | landed | the task is no longer landing or held by its worker (04 F9); the compare-and-set of the feature ref failed |
| in_bundle | `bisect-bundle` (Bundle) | gate_failed or ready | - |
| conflicted, gate_failed | `escalate-task-branch` | same state, on the needs-you list | repair_attempts is under max_attempts |
| gate_failed | `open-triage-task` | gate_failed | a triage task exists |
| any not landed | `abandon-task-branch` | abandoned | worktree-rescued is not recorded |

## 4. Enforced invariants
| # | Invariant (same wording as `code.json` and the test name) | Whose job is it - why transactional |
|---|---|---|
| I8 | I8: a task branch is landed only when its changes are in the feature branch by content | fleetd's, at the fast-forward |
| I11 | I11: every fleet branch in git has a branches row; one without is adopted as an orphan row, never deleted | fleetd's, every pass |
| I12 | I12: a branch is deleted only when landed, merged or rescued, and no git stash entry names it; a task branch is a member of at most 1 bundle that is not ended | fleetd's, before a delete and at build-bundle |
| B4 | A task branch is ready only when its task is done (verified) | fleetd's, at mark-branch-ready |
| B5 | Under the land lock, just before the fast-forward, the task is still landing and held by its worker (04 F9) | fleetd's, under the lock |
| B6 | A red land after a rebase resets the branch to its tip from before the land (04 F7) | fleetd's, on a red land |
| B7 | An escalated task branch is abandoned only by the operator and only after worktree-rescued (connect N4) | the operator's |
| B8 | conflicted and gate_failed are owned by fleetd; a failed resolver or fix attempt adds 1 to repair_attempts, and at max_attempts the branch goes on the needs-you list (A27, A34) | fleetd's, in the pass that sees attempt-failed |
| B9 | A branch of a cancelled task is abandoned only after its rescue ref is written (A34, connect F10) | fleetd's, on task-cancelled |

## 5. Corrective policies
| Situation found late | Then | Within | Owner |
|---|---|---|---|
| The bundle fails the fast gate | bisect; the failing branch goes to gate_failed, and a fix attempt of its task starts with the gate output (task-branch-gate-failed) | 1 pass | integration, then task-execution |
| The branch conflicts when the candidate is built | conflicted; a resolver attempt starts in its worktree with the conflict in zdiff3 form | 1 pass | integration, then task-execution |
| A resolver or fix attempt fails | repair_attempts + 1, the next one after the backoff; at max_attempts escalate-task-branch | 1 pass | integration |
| The task is cancelled | rescue-worktree, then abandoned after worktree-rescued | 1 to 2 passes | integration with worktree-custody |
| A row passes due_at | escalate the row | 1 pass | integration |

## 6. Handled commands → created events
| Command (`discover` id) | Preconditions | Event(s) (`discover` id) | Payload (from `events[].data`) |
|---|---|---|---|
| `mark-branch-ready` | task-done | `task-branch-verified` | branch, task_id |
| `fast-forward-feature-branch` | bundle passed; land lock held | `task-branch-landed` | branch, task_id, feature_id, tip |
| `open-triage-task` | a full gate failure | `triage-task-opened` | feature_id, task_id |
| `escalate-task-branch` | repair_attempts at max_attempts | `task-branch-repair-escalated` | branch, task_id, repair_attempts |
| `abandon-task-branch` | worktree-rescued | `task-branch-abandoned` | branch, task_id, rescue_ref |

## 7. Throughput
| Metric | Average | Maximum | Basis |
|---|---|---|---|
| Commands per instance | 5 | 15 | 1 land, up to a few conflicts and gate failures |
| Concurrent clients per instance | 1 | 1 | fleetd is the only writer |

Conflict risk: low → no optimistic retry (A5).

## 8. Size
| Metric | Estimate | Basis |
|---|---|---|
| Events per instance | 5 to 15 | as above |
| Lifetime of an instance | hours to days | until landed or abandoned |

Size: small → no snapshots needed.

## 9. Model sketch
- Root entity `TaskBranch`: BranchName, TaskId, FeatureId, LandingState, BranchOwner, DueAt, QueueSeq, RescueRef, RepairAttempts, BundleId of its live bundle, orphan flag, version.
- Entities inside: none.
- Value objects: Tip, BundleId, RescueRef, DueAt, RepairAttempts.
- References to other aggregates by id: TaskId, FeatureId, WorktreeId, BundleId.
- Internal domain events: the events in §6 → written as event rows by `LedgerEventLog`.

## 10. Notes & open questions
- 04 L5 and 04 L9 (unlanded work kept and listed) are owned here, as decompose N3 says.
- Q5 answered (orchestrator, D15): task branches rebase at landing; a task branch that no longer applies after a feature rebase goes to conflicted at build-bundle.
- A resolver attempt and a fix attempt are attempts of the task branch, run by task-execution with the lease of the task. Their failure never moves the task (A34).
