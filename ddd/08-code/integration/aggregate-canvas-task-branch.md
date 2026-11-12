# Aggregate Design Canvas - TaskBranch

Context `integration` · aggregate id `task-branch` · pattern domain-model · lives in `crates/horch-core/src/fleetd/integration/domain/task_branch.rs`
Discover candidate: `task-branch` (this step adds open-triage-task) · canvas format: https://github.com/ddd-crew/aggregate-design-canvas

## 1. Name
**TaskBranch** - the branch of 1 task, a ledger row before it exists in git, from building to landed or abandoned (D10).

## 2. Description
The TaskBranch decides when a verified task's work may land on its feature branch, under the land lock and behind the fast gate, and when the branch may be deleted. Every fleet branch in git has a row; an unknown one is adopted, never deleted.
Alternative rejected: the landing state on the Task aggregate, because landing is a git fact of integration and the task is done before it lands.

## 3. State transitions
```
building → verifying → ready → in_bundle → landed
in_bundle → conflicted → verifying
in_bundle → gate_failed → verifying
any → abandoned          (operator, after worktree-rescued)
```
| From | Command | To | Rejected when |
|---|---|---|---|
| verifying | `mark-branch-ready` | ready | the task is not done |
| ready | `build-bundle` | in_bundle or conflicted | - |
| in_bundle | `run-fast-gate` | in_bundle | - (records the gate result on the bundle) |
| in_bundle | `fast-forward-feature-branch` | landed | the task is no longer landing or held by its worker (04 F9); the gate did not pass |
| in_bundle | `bisect-bundle` | gate_failed | the branch is not the failing one |
| gate_failed | `open-triage-task` | gate_failed | a triage task exists |
| any | `abandon-task-branch` | abandoned | the caller is not the operator; worktree-rescued is not recorded |

## 4. Enforced invariants
| # | Invariant (same wording as `code.json` and the test name) | Whose job is it - why transactional |
|---|---|---|
| I8 | I8: a task branch is landed only when its changes are in the feature branch by content | fleetd's, at the fast-forward |
| I11 | I11: every fleet branch in git has a branches row; one without is adopted as an orphan row, never deleted | fleetd's, every pass |
| I12 | I12: a branch is deleted only when landed, merged or rescued, and no git stash entry names it | fleetd's, before a delete |
| B4 | A task branch is ready only when its task is done (verified) | fleetd's, at mark-branch-ready |
| B5 | Under the land lock, just before the fast-forward, the task is still landing and held by its worker (04 F9) | fleetd's, under the lock |
| B6 | A red land after a rebase resets the branch to its tip from before the land (04 F7) | fleetd's, on a red land |
| B7 | An escalated task branch is abandoned only by the operator and only after worktree-rescued (connect N4) | the operator's |

## 5. Corrective policies
| Situation found late | Then | Within | Owner |
|---|---|---|---|
| The bundle fails the fast gate | bisect; the failing branch goes to gate_failed with a triage task | 1 pass | integration |
| The branch conflicts in the bundle | the task goes back to the same worker | 1 pass | policy relaunch-on-conflict in task-execution |
| A row passes due_at | escalate the row | 1 pass | integration |

## 6. Handled commands → created events
| Command (`discover` id) | Preconditions | Event(s) (`discover` id) | Payload (from `events[].data`) |
|---|---|---|---|
| `mark-branch-ready` | task-done | `task-branch-verified` | branch, taskId |
| `build-bundle` | ready branches of 1 feature | `bundle-built`, `task-branch-conflicted` | featureId, branches, queue order |
| `run-fast-gate` | a bundle | `fast-gate-passed`, `fast-gate-failed` | featureId, tip |
| `fast-forward-feature-branch` | gate passed; land lock held | `task-branch-landed` | branch, taskId, featureId, tip |
| `bisect-bundle` | gate failed | `task-branch-gate-failed` | branch, taskId |
| `open-triage-task` | gate_failed | `triage-task-opened` | featureId, taskId |
| `abandon-task-branch` | operator; worktree-rescued | `task-branch-abandoned` | branch, taskId, rescueRef |

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
- Root entity `TaskBranch`: BranchName, TaskId, FeatureId, LandingState, BranchOwner, DueAt, QueueSeq, RescueRef, orphan flag, version.
- Entities inside: none.
- Value objects: Tip, Bundle, RescueRef, DueAt.
- References to other aggregates by id: TaskId, FeatureId, WorktreeId.
- Internal domain events: the events in §6 → written as event rows by `LedgerEventLog`.

## 10. Notes & open questions
- 04 L5 and 04 L9 (unlanded work kept and listed) are owned here, as decompose N3 says.
- Q5: a task branch that no longer applies after a feature rebase goes to conflicted at build-bundle.
