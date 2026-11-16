# Aggregate Design Canvas - Feature

Context `integration` · aggregate id `feature` · pattern domain-model · lives in `crates/horch-core/src/fleetd/integration/domain/feature.rs`
Discover candidate: `feature` (this step adds run-trial-merges, open-resolver-task and merge-pull-request) · canvas format: https://github.com/ddd-crew/aggregate-design-canvas

## 1. Name
**Feature** - 1 sub-orchestrator, 1 feature branch and 1 pull request, from its first assumptions to merged or abandoned (D11, D12).

## 2. Description
The Feature guards how its feature branch moves: only by fast-forward to a gated candidate with a compare-and-set on the tip that fleetd last saw, only with a rescue ref of the old tip before a rebase, and it is ready only with a current full gate pass. It records its gate runs, its trial merges, its push state and its pull request state, so staleness and the state of the pull request are decided inside 1 boundary. Its state table has 4 states: open, ready, merged and abandoned (A28, A34).
Alternative rejected: gate runs as their own aggregate, because "ready" needs the current pass and the current tips in 1 decision.

## 3. State transitions
```
open → ready → merged
open → merged          (changes found in the primary branch by content; ideas in flight move)
ready → open           (a tip moves, or a new idea with --to; the pull request becomes a draft)
open | ready → abandoned   (operator only)
```
| From | Command | To | Rejected when |
|---|---|---|---|
| (none) | `open-feature` | open | the idea already has a feature |
| open | `run-trial-merges` | open | - (queues trial_merge jobs; records TrialMerge facts) |
| open | `stack-feature` | open | the feature is already stacked |
| open | `rebase-feature-branch` | open | no rescue ref of the old tip (I10); a bundle of the feature is in gating or passed (A34) |
| open | `run-full-gate` | open | tasks of the idea are not all landed |
| open | `mark-feature-ready` | ready | no full gate pass on the current tips; an idea of the feature is not integrating, cancelled or parked |
| ready | `reopen-feature` | open | no tip moved and no new idea with --to |
| open, ready | `fetch-remote` | same | remote = none |
| open, ready | `push-feature-branch` | same | remote = none; the fleet clock is before push_not_before |
| ready | `open-pull-request` | ready | a pull request exists; remote = none |
| ready | `merge-pull-request` | ready | the pr setting does not let fleetd merge |
| open, ready | `check-merged` | merged | the changes are not in the primary branch by content (I8) |
| merged | `move-ideas-to-new-feature` | merged | no idea of the feature is in flight |
| open, ready | `abandon-feature` | abandoned | the caller is not the operator |

## 4. Enforced invariants
| # | Invariant (same wording as `code.json` and the test name) | Whose job is it - why transactional |
|---|---|---|
| I8 | I8: a feature is merged only when its changes are in the primary branch by content, also after a squash merge | fleetd's, at check-merged |
| I9 | I9: a feature branch moves only by fast-forward to a candidate that passed its gate; each ref move is a compare-and-set on seen_tip or seen_remote_tip, and a failed compare-and-set moves nothing and records the new tip | fleetd's, at every move |
| I10 | I10: before each rebase or forced update of a branch, a rescue ref of the old tip exists | fleetd's, before the rebase |
| F4 | A feature is ready only with a full gate pass on its current tip over the fetched tip of the primary branch, and each idea of the feature integrating, cancelled or parked | fleetd's, at mark-feature-ready |
| F5 | A gate pass goes stale when either judged tip moves (04 F5) | fleetd's, on every tip move |
| F6 | A feature is stacked on at most 1 older feature | fleetd's, at stack-feature |
| F7 | A feature has 1 of 4 states, open, ready, merged and abandoned; merged and abandoned are terminal, and only the operator sets abandoned (A28, A34) | the one-door transition table |
| F8 | fleetd does not queue a rebase of a feature branch while 1 of its bundles is in gating or in passed (A34) | fleetd's, at rebase-feature-branch |
| F9 | A merge with ideas in flight moves those ideas to a new feature (A28, A34) | fleetd's, in the pass of check-merged |
| F10 | With remote = none, fleetd does not fetch, push or open a pull request; the pull request state follows the feature state (A28) | fleetd's, at each push, fetch and pull request command |
| F11 | A failed push sets push_state failed and push_not_before; at 3 failures in a row, 1 item goes on the needs-you list (A33) | fleetd's, when the push job ends |

## 5. Corrective policies
| Situation found late | Then | Within | Owner |
|---|---|---|---|
| A trial merge with another feature conflicts | stack the younger feature; open 1 resolver task | 1 pass after the trial_merge job ends | integration |
| A rebase conflicts | open a resolver task | 1 pass after the rebase job ends | integration |
| The full gate fails | open triage tasks of the feature | 1 pass | integration |
| A push fails | push again after the backoff (retry-push-on-failure); at 3 failures, 1 needs-you item | push_not_before | integration |
| A lower feature of a stack is merged | rebase the upper feature on the primary tip (unstack-on-merged) | 1 pass | integration |
| A pull request is closed without a merge | the operator decides; horch feature abandon is the exit (operator-decides-closed-pr) | the operator | operator |

## 6. Handled commands → created events
| Command (`discover` id) | Preconditions | Event(s) (`discover` id) | Payload (from `events[].data`) |
|---|---|---|---|
| `open-feature` | assumptions written or direct idea accepted | `feature-opened` | feature_id, branch, primary_branch, idea_id |
| `run-trial-merges` | a tip moved | `trial-merge-passed`, `trial-merge-conflicted` | feature A, feature B or primary, files |
| `stack-feature` | trial merge conflicted | `feature-stacked` | feature_id, stacked_on |
| `open-resolver-task` | conflict | `resolver-task-opened` | feature_id, task_id, conflict |
| `rebase-feature-branch` | primary moved; rescue ref written; no bundle gating or passed | `feature-branch-rebased`, `feature-rebase-conflicted` | feature_id, tip |
| `run-full-gate` | idea tasks landed | `full-gate-passed`, `full-gate-failed` | feature_id, tip, base tip |
| `mark-feature-ready` | current full gate pass | `feature-marked-ready` | feature_id |
| `reopen-feature` | a tip moved or a new idea with --to | `feature-reopened` | feature_id |
| `fetch-remote` | fetch period; remote set | `remote-tips-fetched`, `primary-branch-moved` | repository, remote tips |
| `push-feature-branch` | remote set; after push_not_before | `feature-branch-pushed`, `push-failed` | feature_id, branch, seen_remote_tip |
| `open-pull-request` | ready; remote set | `pull-request-opened` | feature_id, pr_url |
| `merge-pull-request` | ready; pr setting allows | `pull-request-merged` | feature_id |
| `check-merged` | open or ready | `feature-merged` | feature_id |
| `move-ideas-to-new-feature` | merged with ideas in flight | `ideas-moved-to-new-feature` | feature_id, new feature_id, idea ids |
| `abandon-feature` | operator request | `feature-abandoned` | feature_id |

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
- Root entity `Feature`: FeatureId, name, BranchName, primary branch, StackLink, FeatureState, PrSetting, PrState, pr URL, SeenTip (seen_tip, seen_remote_tip), PushState (push_state, push_not_before, failures in a row), RemoteSetting, version.
- Entities inside: `GateRun` (by gate kind, with its job), `TrialMerge` (by pair of refs, with its job).
- Value objects: Tip, GateKind, GateResult, RescueRef, ConflictSet, JudgedTree, SeenTip, PushState, PrState, RemoteSetting.
- References to other aggregates by id: IdeaId, TaskBranch names, BundleId of a live bundle.
- Internal domain events: the events in §6 → written as event rows by `LedgerEventLog`.

## 10. Notes & open questions
- Q5 answered (orchestrator, D15): a rebase does not wait for running attempts of the feature; their task branches rebase at landing.
- The rescue ref of the old tip comes from worktree-custody through RescueRefGateway; the rebase runs only after the ref reads back (A13, define N6).
- define Q1 answered: connect has open-pull-request, pull-request-merged and pull-request-closed; the GitHub collaboration sits behind PullRequestGateway and GitObserver.
- Trial merges, rebases, the full gate, fetch and push are jobs outside the pass (A23, A33). The remote tips of the periodic fetch are rows of remote_tips.
