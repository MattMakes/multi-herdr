# Aggregate Design Canvas - Task

Context `task-execution` · aggregate id `task` · pattern domain-model · lives in `crates/horch-core/src/fleetd/task_execution/domain/task.rs`
Discover candidates: `task` and `attempt` (merged by decision D2; `attempt` is in `deprecated[]`) · canvas format: https://github.com/ddd-crew/aggregate-design-canvas

## 1. Name
**Task** - 1 unit of planned work of 1 idea, from the plan commit to done, escalated or cancelled. It holds its attempts, because an attempt has no life outside its task.

## 2. Description
The Task decides when it may run, which attempt holds it, when that attempt loses its lease, and whether a failure retries, replans or escalates. Attempt and Lease are inside the boundary because the reap must end the attempt and requeue the task in 1 transaction before any signal (04 S7), and I1 ties the task state to exactly 1 unexpired lease.
Alternatives rejected: Attempt as its own aggregate (option D2-A), because the reap would cross 2 aggregates and a crash between the writes loses the requeue (#216). Verification inside Task (option D2-C), because a 20 min check would hold the task and the only-the-verifier-sets-done rule would have no module seam.

## 3. State transitions
```
pending → ready → leased → running → verifying → done
running → stalled → failed → ready            (retry while attempts remain)
failed → escalated → ready                    (operator requeue, attempt_floor reset)
running → waiting_quota → ready               (pool reset)
running → leased                              (fallback persona on another provider)
running → blocked_on_user → leased            (answer recorded)
any non-terminal → cancelled
```
| From | Command | To | Rejected when |
|---|---|---|---|
| pending | `promote-task` | ready | a dependency is not done (I2) |
| ready | `dispatch-task` | leased | the worker already runs an attempt (I4); paths overlap a running task of the feature (04 L6); the profile is unhealthy; a health guard holds dispatch |
| leased | `launch-attempt` | running | the worktree is not created yet |
| running | `renew-lease` | running | no earned signal: process dead, heartbeat older than 30 s, or no activity within stall_after |
| running | `renew-lease` (expired) | stalled | - |
| running | `apply-stall-ladder` | stalled | the next rung is a doorbell, not the end |
| running | `write-result` | running | never changes the state; the attempt exit moves it to verifying |
| running | `move-to-fallback` | leased or waiting_quota | the report is for an attempt that already moved |
| stalled, verifying | `end-attempt` | failed | the attempt already ended |
| failed | `schedule-retry` | ready | attempts minus attempt_floor reached max_attempts (then replan-requested) |
| failed | `escalate-task` | escalated | - |
| verifying | `mark-task-done` | done | no VerifiedVerdict is given (define N1) |
| escalated | `requeue-task` | ready | the caller is not the operator |

## 4. Enforced invariants
| # | Invariant (same wording as `code.json` and the test name) | Whose job is it - why transactional |
|---|---|---|
| I1 | I1: a leased or running task has exactly 1 unexpired lease and 1 live attempt | fleetd, in the same pass that sees the expiry; the task state and its lease change together |
| I2 | I2: no task is ready while a dependency is not done | checked at promote; dependencies are other tasks, so the reconcile pass re-checks it every pass |
| T3 | Only a VerifiedVerdict from the verification module moves a task to done (RR-3, define N1) | the verifier's; a type guard plus the one-door table (D4) |
| T4 | A reap ends the attempt and requeues the task in 1 transaction before any signal (04 S7) | fleetd's, at the moment of the reap; a crash must not split the 2 writes |
| T5 | A usage limit burns no attempt (RR-11) | fleetd's, at move-to-fallback; the attempt counter and the state change together |
| T6 | An operator requeue resets the attempt count through attempt_floor (04 L8) | the operator's, at requeue |
| T7 | A task has at most max_attempts attempts, then 1 replan, then escalation (RR-6) | fleetd's, at schedule-retry |

## 5. Corrective policies
| Situation found late | Then | Within | Owner |
|---|---|---|---|
| I4 breach: a worker holds 2 attempts | end the younger attempt, requeue its task | 1 pass (5 s) | check_invariants in task-execution |
| I5 breach: a process or pane with no live attempt | stop it through the platform layer | 1 pass | check_invariants in task-execution |
| The worktree rescue failed | escalate-task; nothing is removed | 1 pass | policy escalate-on-rescue-failed |
| The landing conflicted | launch-attempt for the same worker | 1 pass | policy relaunch-on-conflict |

## 6. Handled commands → created events
| Command (`discover` id) | Preconditions | Event(s) (`discover` id) | Payload (from `events[].data`) |
|---|---|---|---|
| `promote-task` | pending or waiting_quota; dependencies done | `task-promoted` | taskId |
| `dispatch-task` | ready; I4; no path overlap | `task-leased` | taskId, attemptId, worktreeId, branch |
| `launch-attempt` | leased; worktree created | `attempt-launched` | taskId, attemptId |
| `renew-lease` | running | `lease-renewed`, `lease-expired` | attemptId, basis |
| `apply-stall-ladder` | running; idle past stall_after | `attempt-stalled` | attemptId, rung |
| `move-to-fallback` | usage limit reported for the live attempt | `fallback-assigned`, `quota-wait-started` | taskId, profile, resetAt |
| `write-result` | the attempt is live; result.json validates | `result-written` | taskId, attemptId, status |
| `end-attempt` | the attempt is live | `attempt-failed` | attemptId, taskId, outcome |
| `schedule-retry` | failed | `retry-scheduled`, `replan-requested` | taskId, attempts |
| `mark-task-done` | verifying; a VerifiedVerdict | `task-done` | taskId, branch, verdict |
| `escalate-task` | failed after the replan, or a failed rescue, or a refused plan | `task-escalated` | taskId, reason |
| `requeue-task` | escalated; operator | `task-requeued` | taskId |

## 7. Throughput
| Metric | Average | Maximum | Basis |
|---|---|---|---|
| Commands per instance per hour | 720 (renew-lease every 5 s pass while running) | 800 | design: 5 s pass |
| Concurrent clients per instance | 1 | 1 | fleetd is the only writer (RR-5) |

Conflict risk: low → no optimistic retry; a version column and the from_state check of the one-door transition make a stale write fail loudly (A5).

## 8. Size
| Metric | Estimate | Basis |
|---|---|---|
| Attempts per instance | 1 to 4 | max_attempts default 3, plus fallback restarts that burn no attempt |
| Lifetime of an instance | minutes to days | an exec attempt runs until result; a parked task waits on an answer |

Size: small → no snapshots needed. Attempts keep ids, PIDs and outcomes only, never transcripts.

## 9. Model sketch
- Root entity `Task`: TaskId, TaskState, TaskKind, ProfileRef, AttemptBudget (attempts, max_attempts, attempt_floor), dependencies by TaskId, PathClaim, version.
- Entities inside: `Attempt` (addressed by AttemptId inside the task for leases, stall rungs and outcomes).
- Value objects: Lease (expires_at, renewed_at, basis, paused), AwakeInstant, StallRung, Backoff, AttemptOutcome, ResultFile, VerifiedVerdict.
- References to other aggregates by id: IdeaId, WorktreeId, BranchName, the worker id.
- Internal domain events: the events in §6 → written as event rows by `LedgerEventLog`.

## 10. Notes & open questions
- Q1: the owner of the fleet-run wrapper decides where the heartbeat and exit files are produced; the Task reads them only as facts.
- Q4: herdr agent status reaches the Task as a LivenessProbe reading, not as a message.
- Q5: a feature rebase does not wait for running attempts in this design.
