# Aggregate Design Canvas - Task

Context `task-execution` · aggregate id `task` · pattern domain-model · lives in `crates/horch-core/src/fleetd/task_execution/domain/task.rs`
Discover candidates: `task` and `attempt` (merged by decision D2; `attempt` is in `deprecated[]`). This step adds assign-task and start-work, the 2 file writes of the launch · canvas format: https://github.com/ddd-crew/aggregate-design-canvas

## 1. Name
**Task** - 1 unit of planned work of 1 idea, from the plan commit to done, escalated or cancelled. It holds its attempts, because an attempt has no life outside its task.

## 2. Description
The Task decides when it may run, which attempt holds it, when that attempt loses its lease, and whether a failure retries, replans or escalates. Attempt and Lease are inside the boundary because the reap must end the attempt and requeue the task in 1 transaction before any signal (04 S7), and I1 ties the task state to exactly 1 unexpired lease. Its state table has 12 states, each with 1 owner and its exits, and each state that is not terminal has an exit to cancelled (A26, A34). A resolver attempt or a fix attempt of its task branch is also an attempt of the Task and holds its lease, but its end never moves a done task (A34).
Alternatives rejected: Attempt as its own aggregate (option D2-A), because the reap would cross 2 aggregates and a crash between the writes loses the requeue (#216). Verification inside Task (option D2-C), because a 20 min check would hold the task and the only-the-verifier-sets-done rule would have no module seam.

## 3. State transitions
```
pending → ready → leased → running → verifying → done
leased → ready | failed                       (start timeout or failed worktree_add job, rung 6)
running → stalled → ready | failed            (rung 6; not_before while attempts remain)
stalled → verifying                           (a valid result in the same pass)
verifying → ready                             (outcome rejected, rung 6; or critical_open, no attempt burned)
running → blocked_on_user → ready             (outcome asked; answer recorded)
running → waiting_quota → ready               (outcome quota; pool reset)
running → ready                               (fallback persona on another provider)
failed → ready                                (1 replan, attempt count 0)
failed → escalated → ready                    (operator requeue, attempt_floor reset)
done → done                                   (operator requeue: 1 more resolver or fix attempt)
any non-terminal → cancelled                  (process group stopped first)
```
| From | Command | To | Rejected when |
|---|---|---|---|
| pending | `promote-task` | ready | a dependency is not met: its task is not done, or its task branch is not landed (I2) |
| ready | `dispatch-task` | leased | the worker already runs an attempt (I4); paths overlap a running task of the feature (04 L6); a hold on dispatch applies; the fleet clock is before not_before |
| leased | `assign-task`, `start-work` | leased | the worktree_add job has not passed |
| leased | `launch-attempt` | leased, then running at the first heartbeat | the worktree_add job has not passed |
| running | `renew-lease` | running | no earned signal: process dead, heartbeat older than 30 s, or no activity within stall_after |
| running | `renew-lease` (expired) | stalled | - |
| running | `apply-stall-ladder` | stalled | the next rung is a doorbell, not the end |
| running | `write-result` | running | never changes the state; the attempt exit moves it to verifying |
| running | `move-to-fallback` | ready or waiting_quota | the report is for an attempt that already moved |
| leased, stalled, verifying | `end-attempt` | ready or failed (rung 6) | the attempt already ended |
| stalled, verifying | `schedule-retry` | ready with not_before | attempts minus attempt_floor reached max_attempts (then failed and replan-requested) |
| failed | `escalate-task` | escalated | the task has its 1 replan left and is inside replan_limit |
| verifying | `mark-task-done` | done | no VerifiedVerdict is given (define N1); a judged task whose judge task is not done |
| escalated | `requeue-task` | ready | - (an operator request with the operator token) |
| done | `requeue-task` | done | its task branch is not conflicted or gate_failed |
| any non-terminal | `cancel-task` | cancelled | the task is terminal |

## 4. Enforced invariants
| # | Invariant (same wording as `code.json` and the test name) | Whose job is it - why transactional |
|---|---|---|
| I1 | I1: each live attempt of a task has exactly 1 unexpired lease; each leased or running task has exactly 1 attempt with a live process or pane; a resolver or fix attempt of a task branch holds the lease of its task while it is live | fleetd, in the same pass that sees the expiry; the task state and its lease change together |
| I2 | I2: no task is ready while a dependency is not met; an after: dependency is met when its task is done and its task branch is landed, a judges: dependency when the judged task is in verifying and its structural check passed | checked at promote; dependencies are other tasks and branches, so the reconcile pass re-checks it every pass |
| T3 | Only a VerifiedVerdict from the verification module moves a task to done (RR-3, define N1) | the verifier's; a type guard plus the one-door table (D4) |
| T4 | A reap ends the attempt and requeues the task in 1 transaction before any signal (04 S7) | fleetd's, at the moment of the reap; a crash must not split the 2 writes |
| T5 | The outcomes asked, quota and critical_open burn no attempt (RR-11, A26) | fleetd's, when the attempt ends; the attempt counter and the state change together |
| T6 | An operator requeue resets the attempt count through attempt_floor (04 L8) | the operator's, at requeue |
| T7 | A task has at most max_attempts attempts, then 1 replan, then escalation (RR-6) | fleetd's, at schedule-retry |
| T8 | Each task state that is not terminal has an exit to cancelled; fleetd first stops the process group of a live attempt, and the cancel of a judged task also cancels its judge task (A34) | fleetd's, at cancel-task |
| T9 | A resolver or fix attempt of a done task never moves the task; its failure or its lease expiry adds 1 to repair_attempts of the branch (A34) | fleetd's, when the attempt ends; integration counts it |
| T10 | Each operational timer of a task is a deadline on the fleet clock: the lease, each stall rung, the start timeout, the retry backoff and not_before (PL-4, A25) | fleetd's, every pass |

## 5. Corrective policies
| Situation found late | Then | Within | Owner |
|---|---|---|---|
| I4 breach: a worker holds 2 attempts | end the younger attempt, requeue its task | 1 pass (5 s) | check_invariants in task-execution |
| I5 breach: a process or pane with no live attempt | stop it through the platform layer | 1 pass | check_invariants in task-execution |
| The worktree rescue failed | escalate-task; nothing is removed | 1 pass | policy escalate-on-rescue-failed |
| The landing conflicted or the fast gate failed | a resolver attempt or a fix attempt of the task in its worktree; the task stays done | 1 pass | policies on task-branch-conflicted and task-branch-gate-failed |
| The idea is cancelled or the feature abandoned | cancel-task for each task that is not terminal | 1 pass | policy cancel-tasks-on-idea-cancelled |

## 6. Handled commands → created events
| Command (`discover` id) | Preconditions | Event(s) (`discover` id) | Payload (from `events[].data`) |
|---|---|---|---|
| `promote-task` | pending or waiting_quota; dependencies met | `task-promoted` | task_id |
| `dispatch-task` | ready; I4; no path overlap; no hold | `task-leased` | task_id, attempt_id, worktree_id, branch |
| `assign-task` | leased; worktree_add job passed | (task.yaml written, `attempt-launched` follows) | the task.yaml fields |
| `start-work` | leased; task.yaml written | (inbox/START.md written, `attempt-launched` follows) | task_yaml, rescue_commit?, progress_notes? |
| `launch-attempt` | leased; worktree created | `attempt-launched` | task_id, attempt_id |
| `renew-lease` | running | `lease-renewed`, `lease-expired` | attempt_id, basis |
| `apply-stall-ladder` | running; idle past stall_after | `attempt-stalled` | attempt_id, rung |
| `move-to-fallback` | usage limit reported for the live attempt | `fallback-assigned`, `quota-wait-started` | task_id, profile, reset_at? |
| `write-result` | the attempt is live; result.json validates | `result-written` | task_id, attempt_id, status |
| `end-attempt` | the attempt is live | `attempt-failed` | attempt_id, task_id, outcome |
| `schedule-retry` | a non-verified outcome | `retry-scheduled`, `replan-requested` | task_id, attempts |
| `mark-task-done` | verifying; a VerifiedVerdict | `task-done` | task_id, branch, verdict |
| `escalate-task` | failed after the replan, or a failed rescue, or a refused plan | `task-escalated` | task_id, reason |
| `requeue-task` | escalated or done; operator request | `task-requeued` | task_id |
| `cancel-task` | not terminal | `task-cancelled` | task_id, branch, cause |

## 7. Throughput
| Metric | Average | Maximum | Basis |
|---|---|---|---|
| Commands per instance per hour | 720 (renew-lease every 5 s pass while running) | 800 | design: 5 s pass |
| Concurrent clients per instance | 1 | 1 | fleetd is the only writer from Phase 5 (RR-5, A36) |

Conflict risk: low → no optimistic retry; a version column and the from_state check of the one-door transition make a stale write fail loudly (A5).

## 8. Size
| Metric | Estimate | Basis |
|---|---|---|
| Attempts per instance | 1 to 6 | max_attempts default 3, plus fallback restarts that burn no attempt, plus resolver or fix attempts of the task branch |
| Lifetime of an instance | minutes to days | an exec attempt runs until result; a parked task waits on an answer |

Size: small → no snapshots needed. Attempts keep ids, PIDs and outcomes only, never transcripts.

## 9. Model sketch
- Root entity `Task`: TaskId, TaskState, TaskKind, ProfileRef, AttemptBudget (attempts, max_attempts, attempt_floor), replans, fix_rounds, not_before, dependencies by TaskId, PathClaim, version.
- Entities inside: `Attempt` (addressed by AttemptId inside the task for leases, stall rungs and outcomes; a resolver or fix attempt carries a RepairAttemptRef to its task branch).
- Value objects: Lease (expires_at on the fleet clock, renewed_at, basis, paused), FleetDeadline, StallRung, Backoff, AttemptOutcome, ResultFile, VerifiedVerdict, RepairAttemptRef.
- References to other aggregates by id: IdeaId, WorktreeId, BranchName, the worker id, the judge TaskId.
- Internal domain events: the events in §6 → written as event rows by `LedgerEventLog`.

## 10. Notes & open questions
- Q1 answered (orchestrator, D15): worker-profiles owns the fleet-run wrapper, which writes heartbeat.json and exit.json; the Task reads them only as facts.
- Q4 answered (orchestrator, D15): herdr agent status is the message agent-status-changed; the Task reads it through the LivenessProbe reading.
- Q5 answered (orchestrator, D15): a feature rebase does not wait for running attempts; task branches rebase at landing.
- The lease and the stall rungs use the fleet clock, not awake time (A25). Only the hang check and max_minutes of the wrapper use awake time.
