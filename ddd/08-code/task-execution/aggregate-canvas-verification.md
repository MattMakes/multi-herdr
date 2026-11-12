# Aggregate Design Canvas - Verification

Context `task-execution` · aggregate id `verification` · pattern domain-model · lives in `crates/horch-core/src/fleetd/task_execution/domain/verification.rs`
Discover candidate: `verification` (handles run-check, run-review, open-critical-fix; this step adds write-judge-verdict and settle-high-finding) · canvas format: https://github.com/ddd-crew/aggregate-design-canvas

## 1. Name
**Verification** - the verifier's judgment of 1 task, from the attempt exit with a result to a verified or failed verdict. One per task; a new attempt resets it.

## 2. Description
The Verification runs the task's check in the worker's sandbox, then the review scripts for a code kind, or waits on the paired judge for a judgment kind. It is the only code that can create a VerifiedVerdict, the value that moves a task to done (define N1, decision D4). It is apart from Task because a check runs up to 20 min outside any transaction, and the separate module is the seam that proves no other path sets done.
Alternative rejected: verification state inside Task (option D2-C).

## 3. State transitions
```
pending → checking → reviewing → verified          (code kind)
checking → awaiting_judge → verified               (judgment kind)
checking → verified                                (judge kind)
awaiting_judge → critical_fix → awaiting_judge     (at most 2 rounds)
critical_fix → escalated
checking → check_failed ; reviewing → review_failed
```
| From | Command | To | Rejected when |
|---|---|---|---|
| pending | `run-check` | checking | result.json is missing or invalid |
| checking | `run-check` (result) | reviewing, awaiting_judge, verified or check_failed | the judged tree is not the current tree |
| reviewing | `run-review` | verified or review_failed | the review exceeds the V-14 limits (2 requeues; rounds 1 light, 3 full) |
| awaiting_judge | `write-judge-verdict` | verified or critical_fix | the verdict file has no rank on a finding |
| awaiting_judge | `open-critical-fix` | critical_fix | 2 rounds are already used (then escalated) |
| awaiting_judge | `settle-high-finding` | awaiting_judge | the finding is not HIGH, or has no written reason |

## 4. Enforced invariants
| # | Invariant (same wording as `code.json` and the test name) | Whose job is it - why transactional |
|---|---|---|
| V1 | A verdict is verified only when result.json validates and the check passes in the worker sandbox | the verifier's, when it records the check result |
| V2 | A code kind is verified only after the review scripts pass (V-14 limits) | the verifier's, when it records the review |
| V3 | A judgment kind is verified only with no open CRITICAL judge finding | the verifier's, when it records the judge verdict |
| V4 | At most 2 critical fix rounds, then the task escalates | the verifier's, at open-critical-fix |
| V5 | A second judge raises no new MEDIUM or LOW finding | the verifier's, when it ingests the second verdict |

## 5. Corrective policies
| Situation found late | Then | Within | Owner |
|---|---|---|---|
| New commits after a passed check | the verification restarts; the old pass is stale | 1 pass | task-execution (judged tree) |
| A HIGH finding is open | the sub-orchestrator fixes or dismisses it with a reason | no limit; it never blocks alone | policy lead-settles-high |

## 6. Handled commands → created events
| Command (`discover` id) | Preconditions | Event(s) (`discover` id) | Payload (from `events[].data`) |
|---|---|---|---|
| `run-check` | result written; attempt exited | `check-passed`, `check-failed` | taskId, judgedTree, outcome |
| `run-review` | check passed; code kind | `review-passed`, `review-failed` | taskId, findings |
| `write-judge-verdict` | awaiting_judge | `judge-verdict-written` | taskId, findings with ranks |
| `open-critical-fix` | a CRITICAL finding; round under 3 | `critical-fix-opened` | taskId, round |
| `settle-high-finding` | a HIGH finding | `high-finding-settled` | taskId, findingId, reason |

## 7. Throughput
| Metric | Average | Maximum | Basis |
|---|---|---|---|
| Commands per instance per attempt | 3 | 8 | check, review or judge, up to 2 fix rounds |
| Concurrent clients per instance | 1 | 1 | fleetd is the only writer |

Conflict risk: low → no optimistic retry (A5).

## 8. Size
| Metric | Estimate | Basis |
|---|---|---|
| Judge findings per instance | 0 to 20 | 1 judge report per round |
| Lifetime of an instance | 1 attempt | reset by the next attempt |

Size: small → no snapshots needed.

## 9. Model sketch
- Root entity `Verification`: VerificationId (= TaskId), state, JudgedTree, CheckOutcome, ReviewOutcome, FixRound, version.
- Entities inside: `JudgeFinding` (addressed by finding id to settle a HIGH item).
- Value objects: FindingRank, JudgedTree, VerifiedVerdict (constructor private to this module).
- References to other aggregates by id: TaskId, the judge TaskId.
- Internal domain events: the events in §6 → written as event rows by `LedgerEventLog`.

## 10. Notes & open questions
- The review scripts of taskledger::finish run unchanged behind CheckRunner (design D6).
- No open question names this aggregate.
