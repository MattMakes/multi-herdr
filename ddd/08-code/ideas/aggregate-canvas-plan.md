# Aggregate Design Canvas - Plan

Context `ideas` · aggregate id `plan` · pattern domain-model (plan validation module only, strategize D1 option C) · lives in `crates/horch-core/src/fleetd/ideas/domain/plan.rs`
Discover candidate: `plan` (handles submit-plan, validate-plan, replan-task) · canvas format: https://github.com/ddd-crew/aggregate-design-canvas

## 1. Name
**Plan** - the plan.yaml of 1 idea, from its submission by the sub-orchestrator to a committed set of tasks. One per idea; a revised plan gets a new content hash.

## 2. Description
The Plan holds the rules that decide whether a task may exist: coverage of every requirement, a task graph with no cycle, a paired judge for each judgment-kind task, the model guide and the declared gates of the project (FR-3, FR-4). It commits all tasks in 1 transaction or none. It is the only aggregate of ideas: idea intake and questions stay active-record records in IdeasStore (A12), so their commands are transaction scripts.
Alternative rejected: plan validation as an active-record check inside intake (strategize D1 option A), because the plan rules leak into callers.

## 3. State transitions
```
none → submitted → committed
submitted → refused → submitted        (a revised plan, new content hash)
committed → committed                  (replan-task, once per task)
```
| From | Command | To | Rejected when |
|---|---|---|---|
| none | `submit-plan` | submitted | the idea is not approved (I7) |
| submitted | `validate-plan` | committed or refused | a check fails: then refused, with every reason |
| refused | `submit-plan` | submitted | the content hash is the same as the refused plan |
| committed | `replan-task` | committed | the task was replanned once already, or the changed plan fails a check |

## 4. Enforced invariants
| # | Invariant (same wording as `code.json` and the test name) | Whose job is it - why transactional |
|---|---|---|
| P1 | I3: every requirement of an idea in in_progress or later is covered by at least 1 task that is not cancelled | the validator's, at commit and at each replan |
| P2 | A plan is committed all-or-nothing after every validator check passes (FR-3, FR-4) | the validator's, in the commit transaction |
| P3 | A judgment-kind task without a paired judge refuses the plan | the validator's, at commit |
| P4 | The task dependencies of a plan form a graph with no cycle (FR-3) | the validator's, at commit and at each replan |
| P5 | A plan of a project with no declared fast gate is refused | the validator's, at commit |
| P6 | A task is replanned at most once | the plan's, at replan-task |

## 5. Corrective policies
| Situation found late | Then | Within | Owner |
|---|---|---|---|
| A requirement loses its last task by a cancel | the idea is reported uncovered and the sub-orchestrator replans | 1 pass | ideas (I3 check of check_invariants) |
| A task asks for a replan a second time | the task escalates to the operator | 1 pass | task-execution, then the needs-you list of ideas |

## 6. Handled commands → created events
| Command (`discover` id) | Preconditions | Event(s) (`discover` id) | Payload (from connect) |
|---|---|---|---|
| `submit-plan` | assumptions approved; spec written | `plan-submitted` | idea_id, plan.yaml |
| `validate-plan` | submitted | `plan-committed`, `plan-refused` | idea_id, task_ids[] / idea_id, task_id?, reasons[] |
| `replan-task` | committed; replan-requested for the task | `task-replanned` | task_id, plan_change |

## 7. Throughput
| Metric | Average | Maximum | Basis |
|---|---|---|---|
| Commands per instance | 2 | 6 | 1 submit and 1 validate, plus refusals and replans |
| Concurrent clients per instance | 1 | 1 | fleetd is the only writer |

Conflict risk: low → no optimistic retry (A5).

## 8. Size
| Metric | Estimate | Basis |
|---|---|---|
| Planned tasks per instance | 1 to 40 | 1 idea, 1 feature |
| Lifetime of an instance | 1 idea | closed when the idea is merged or cancelled |

Size: small → no snapshots needed.

## 9. Model sketch
- Root entity `Plan`: PlanId (= idea id), PlanContentHash, state, version.
- Entities inside: `PlannedTask` (addressed by task id for a replan).
- Value objects: TaskDag, RequirementCoverage, JudgePairing, DeclaredGates.
- References to other aggregates by id: the idea id, each task id of task-execution.
- Domain service: `PlanValidator`, pure over plan.yaml and the requirements.
- Internal domain events: the events in §6 → written as event rows by `LedgerEventLog`; plan-verdict-written goes to plan.verdict.json through IdeaFolderWriter.

## 10. Notes & open questions
- The fast gate of this repository is still an open question of understand (Q8). P5 refuses a plan until a fast gate is declared.
- No open question of this step names this aggregate.
