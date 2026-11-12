# Aggregate Design Canvas - Feature

Context `integration` · aggregate id `feature` · pattern domain-model · lives in `crates/horch-core/src/fleetd/integration/domain/feature.rs`
Discover candidate: `feature` (this step adds run-trial-merges, open-resolver-task and merge-pull-request) · canvas format: https://github.com/ddd-crew/aggregate-design-canvas

## 1. Name
**Feature** - 1 sub-orchestrator, 1 feature branch and 1 pull request, from its first assumptions to merged or abandoned (D11, D12).

## 2. Description
The Feature guards how its feature branch moves: only by fast-forward to a gated candidate, only with a rescue ref of the old tip before a rebase, and it is ready only with a current full gate pass. It records its gate runs and trial merges, so staleness is decided inside 1 boundary.
Alternative rejected: gate runs as their own aggregate, because "ready" needs the current pass and the current tips in 1 decision.

## 3. State transitions
```
open → ready → merged
ready → open          (a tip moves; the pass goes stale)
open | ready → abandoned
```
| From | Command | To | Rejected when |
|---|---|---|---|
| (none) | `open-feature` | open | the idea already has a feature |
| open | `run-trial-merges` | open | - (records TrialMerge facts) |
| open | `stack-feature` | open | the feature is already stacked |
| open | `rebase-feature-branch` | open | no rescue ref of the old tip (I10) |
| open | `run-full-gate` | open | tasks of the idea are not all landed |
| open | `mark-feature-ready` | ready | no full gate pass on the current tips |
| ready | `open-pull-request` | ready | a pull request exists |
| ready | `merge-pull-request` | ready | the pr setting does not let fleetd merge |
| ready | `check-merged` | merged | the changes are not in the primary branch by content (I8) |

## 4. Enforced invariants
| # | Invariant (same wording as `code.json` and the test name) | Whose job is it - why transactional |
|---|---|---|
| I8 | I8: a feature is merged only when its changes are in the primary branch by content, also after a squash merge | fleetd's, at check-merged |
| I9 | I9: a feature branch moves only by fast-forward to a candidate that passed its gate | fleetd's, at every move |
| I10 | I10: before each rebase or forced update of a branch, a rescue ref of the old tip exists | fleetd's, before the rebase |
| F4 | A feature is ready only with a full gate pass on its current tip over the current primary branch | fleetd's, at mark-feature-ready |
| F5 | A gate pass goes stale when either judged tip moves (04 F5) | fleetd's, on every tip move |
| F6 | A feature is stacked on at most 1 older feature | fleetd's, at stack-feature |

## 5. Corrective policies
| Situation found late | Then | Within | Owner |
|---|---|---|---|
| A trial merge with another feature conflicts | stack the younger feature; open 1 resolver task | 1 pass after a tip moves | integration |
| A rebase conflicts | open a resolver task | 1 pass | integration |
| The full gate fails | open triage tasks of the feature | 1 pass | integration |

## 6. Handled commands → created events
| Command (`discover` id) | Preconditions | Event(s) (`discover` id) | Payload (from `events[].data`) |
|---|---|---|---|
| `open-feature` | assumptions written or direct idea accepted | `feature-opened` | featureId, branch, primaryBranch, ideaId |
| `run-trial-merges` | a tip moved | `trial-merge-passed`, `trial-merge-conflicted` | featureA, featureB or primary, files |
| `stack-feature` | trial merge conflicted | `feature-stacked` | featureId, stackedOn |
| `open-resolver-task` | conflict | `resolver-task-opened` | featureId, taskId, conflict |
| `rebase-feature-branch` | primary moved; rescue ref written | `feature-branch-rebased`, `feature-rebase-conflicted` | featureId, tip |
| `run-full-gate` | idea tasks landed | `full-gate-passed`, `full-gate-failed` | featureId, tip, baseTip |
| `mark-feature-ready` | current full gate pass | `feature-marked-ready` | featureId |
| `open-pull-request` | ready | `pull-request-opened` | featureId, prUrl |
| `merge-pull-request` | ready; pr setting allows | `pull-request-merged` | featureId |
| `check-merged` | ready | `feature-merged` | featureId |

## 7. Throughput
| Metric | Average | Maximum | Basis |
|---|---|---|---|
| Commands per instance per day | 20 | 200 | every primary move triggers trial merges and a rebase |
| Concurrent clients per instance | 1 | 1 | fleetd is the only writer |

Conflict risk: low → no optimistic retry (A5).

## 8. Size
| Metric | Estimate | Basis |
|---|---|---|
| Gate runs and trial merges per instance | 10 to 200 | 1 per tip move |
| Lifetime of an instance | days to weeks | until the pull request merges |

Size: medium → keep only the latest GateRun per gate kind and the latest TrialMerge per pair inside the aggregate; older rows stay in the ledger for history.

## 9. Model sketch
- Root entity `Feature`: FeatureId, name, BranchName, primary branch, StackLink, FeatureState, PrSetting, pr URL, version.
- Entities inside: `GateRun` (by gate kind), `TrialMerge` (by pair of refs).
- Value objects: Tip, GateKind, GateResult, RescueRef, ConflictSet, JudgedTree.
- References to other aggregates by id: IdeaId, TaskBranch names.
- Internal domain events: the events in §6 → written as event rows by `LedgerEventLog`.

## 10. Notes & open questions
- Q5 answered (orchestrator, D15): a rebase does not wait for running attempts of the feature; their task branches rebase at landing.
- The rescue ref of the old tip comes from worktree-custody through RescueRefGateway; the rebase runs only after the ref reads back (A13, define N6).
- define Q1 answered: connect has open-pull-request and pull-request-merged; the GitHub collaboration sits behind PullRequestGateway and GitObserver.
