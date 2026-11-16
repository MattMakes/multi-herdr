# Design - Landing and integration (`integration`)

| | |
|---|---|
| Classification | core (cost-reducer) · sourcing build · pattern **domain-model** |
| Deployable | `horch` (modular-monolith) · team `core-team` · data store own (the ledger, single writer fleetd) |
| Module path | `crates/horch-core/src/fleetd/integration` |
| Structure | hexagonal (domain / application / adapters) |
| Purpose | Integration decides when verified task branches land on their feature branch and when a feature is ready for its pull request, so that each idea reaches the primary branch through its own feature (G3) and no landing overwrites other work (G2). It serves the operator, who merges 1 tested pull request per feature, and the sub-orchestrator, which gets conflicts as resolver tasks. It finds conflicts between features early by trial merges and keeps each feature branch rebased. It does not verify tasks (task-execution) and does not remove worktrees or write rescue refs (worktree-custody). - canvas: `ddd/07-define/integration/bounded-context-canvas.md` |
| Domain roles | execution-context |

## 1. Module layout
```
crates/horch-core/src/fleetd/integration/
  mod.rs               public contract of the context (driving ports and published event types only)
  domain/
    feature.rs         Feature root, GateRun, TrialMerge, stack link
    task_branch.rs     TaskBranch root, landing state machine
    bundle.rs          Bundle root, BundleMember, bundle state machine, candidate ref
    bisector.rs        BundleBisector domain service
    stack.rs           StackPlanner domain service
    values.rs          BranchName, Tip, SeenTip, RescueRef, JudgedTree, GateResult, PushState, PrState
  application/
    commands.rs        1 use case per owned command
    handlers.rs        policies on consumed events
    queries.rs         read models
    ports.rs           driven port traits
  adapters/
    sqlite.rs          ledger-backed store or repositories
    memory.rs          in-memory fakes for tests
    gateways.rs        adapters to other contexts and adopted systems
```
Dependency rule: `domain/` imports nothing outside itself and std; `application/` imports `domain/` only; `adapters/` may import both and existing horch-core modules. No production file of the workspace outside `crates/horch-platform` holds `cfg(target_os)`, `cfg(unix)`, `cfg(windows)` or `cfg(target_family)` (PL-2, from Phase P). Enforced by `crates/horch-core/tests/arch_scan.rs` (tests: `fitness_domain_imports_nothing_outward`, `fitness_workspace_production_code_has_no_os_cfg_outside_horch_platform`, `fitness_only_fleet_runtime_opens_the_ledger_writable`, `fitness_no_context_touches_foreign_tables`).
Composition root: `crates/horch/src/cmd/fleetd.rs` (daemon and `horch fleetd check`) wires every adapter to its port; short-lived CLI commands wire the read-only queries and the command channel client.

## 2. Domain model
| Aggregate | Root | Invariants | Commands → events | Canvas |
|---|---|---|---|---|
| `feature` | `Feature` | 11 | 15 commands → 19 events | `aggregate-canvas-feature.md` |
| `task-branch` | `TaskBranch` | 9 | 5 commands → 7 events | `aggregate-canvas-task-branch.md` |
| `bundle` | `Bundle` | 5 | 3 commands → 4 events | `aggregate-canvas-bundle.md` |

Value objects: BranchName, BranchOwner, BundleId, BundleState, CandidateRef, ConflictSet, DueAt, FeatureId, FeatureState, GateKind, GateResult, JudgedTree, LandingState, PrSetting, PrState, PushState, QueueSeq, RemoteSetting, RepairAttempts, RescueRef, SeenTip, StackLink, TaskId, Tip · Entities: BundleMember, GateRun, TrialMerge · Domain services: BundleBisector, StackPlanner
- `BundleBisector`: Finds the failing task branch of a red bundle from gate results; pure over GateRun facts.
- `StackPlanner`: Orders in-flight features from trial-merge facts: younger on older, bottom of a stack first for rebase.

## 3. Application services (use cases)
| Use case | Command (`discover` id) | Trigger and rule | Loads | Decides / enforces |
|---|---|---|---|---|
| `OpenFeature` | `open-feature` | Writes the features row, then creates the feature branch. | `Feature` by id | see §2 |
| `MarkBranchReady` | `mark-branch-ready` | verifying to ready. | `TaskBranch` by id | see §2 |
| `BuildBundle` | `build-bundle` | 1 bundle per feature from the ready task branches in queue order: the bundles and bundle_members rows, then 1 bundle_build job (A27). | `Bundle` by id | see §2 |
| `RunFastGate` | `run-fast-gate` | Queues the fast_gate job of the bundle, keyed by the candidate tip and the base tip; a passed result is used again (A33). | `Bundle` by id | see §2 |
| `FastForwardFeatureBranch` | `fast-forward-feature-branch` | Land lock 1 h, 5 tries. A red land resets to the tip before the rebase (04 F7). | `TaskBranch` by id | see §2 |
| `BisectBundle` | `bisect-bundle` | Finds the failing task branch. | `Bundle` by id | see §2 |
| `OpenTriageTask` | `open-triage-task` | A task of the feature for a failure of the full gate. | `TaskBranch` by id | see §2 |
| `AbandonTaskBranch` | `abandon-task-branch` | horch branch abandon, an operator request with request_id and the operator token, or a policy on task-cancelled and feature-abandoned. Sends rescue-worktree and moves the branch to abandoned only after worktree-rescued. | `TaskBranch` by id | see §2 |
| `RunTrialMerges` | `run-trial-merges` | Each in-flight feature against the primary branch and against each other in-flight feature. Queues 1 trial_merge job per pair, keyed by the 2 tips; git merge-tree with no checkout; records TrialMerge with its job (A23). | `Feature` by id | see §2 |
| `StackFeature` | `stack-feature` | Sets stacked_on to the older feature. | `Feature` by id | see §2 |
| `OpenResolverTask` | `open-resolver-task` | At most 100 changed lines or 2 files, else split by file. rerere is on. | `Feature` by id | see §2 |
| `RebaseFeatureBranch` | `rebase-feature-branch` | Asks worktree-custody for a rescue ref of the old tip first (RescueRefGateway, A13), then queues a rebase job. Not while a bundle of the feature is gating or passed (A34). Does not wait for running attempts; their task branches rebase at landing. | `Feature` by id | see §2 |
| `RunFullGate` | `run-full-gate` | Queues the full_gate job of the feature, keyed by the feature tip and the fetched primary tip; once when its tasks are landed, again when a pass goes stale. | `Feature` by id | see §2 |
| `MarkFeatureReady` | `mark-feature-ready` | features.state open to ready. | `Feature` by id | see §2 |
| `OpenPullRequest` | `open-pull-request` | pr setting gh or manual. Not with remote = none. The pull request state follows the feature state (A28). | `Feature` by id | see §2 |
| `MergePullRequest` | `merge-pull-request` | On GitHub, or a plain git merge. The default merger is the operator. Runs only when the project pr setting lets fleetd merge; by default the operator merges and check-merged observes it. | `Feature` by id | see §2 |
| `CheckMerged` | `check-merged` | Every pass reads git, not the GitHub API. | `Feature` by id | see §2 |
| `EscalateTaskBranch` | `escalate-task-branch` | Puts a task branch on the needs-you list when repair_attempts reaches max_attempts of the profile. Publishes task-branch-repair-escalated (A34). | `TaskBranch` by id | see §2 |
| `FetchRemote` | `fetch-remote` | Queues 1 fetch job for each repository at each fetch period (fetch_period_s), also after a failed run. The job records each remote tip in remote_tips. Not with remote = none (A23, A33). | `Feature` by id | see §2 |
| `PushFeatureBranch` | `push-feature-branch` | Queues a push job of the feature branch with --force-with-lease on seen_remote_tip. Each retry is a new run after push_not_before; at 3 failures in a row, 1 needs-you item (A33). | `Feature` by id | see §2 |
| `ReopenFeature` | `reopen-feature` | Moves a ready feature back to open when its tip or the fetched primary tip moves, or a new idea is accepted with --to; its pull request becomes a draft (A28). | `Feature` by id | see §2 |
| `AbandonFeature` | `abandon-feature` | horch feature abandon, an operator request with request_id and the operator token. Writes a rescue ref for each branch that is not landed, moves each such branch to abandoned, and publishes feature-abandoned (A28). | `Feature` by id | see §2 |
| `MoveIdeasToNewFeature` | `move-ideas-to-new-feature` | When a feature is merged with ideas in flight, opens a new feature and moves each such idea to it (A28, A34). | `Feature` by id | see §2 |

Transaction: 1 aggregate (or 1 store call set) per use case through the one-door transition function; the event row is written in the same transaction; process starts, git calls and check runs happen after the commit (connect N4).

## 4. Ports
### Driving
| Port | Operations | Called by |
|---|---|---|
| `IntegrationCommands` | one operation per use case; called by the reconcile pass and the command channel (abandon-task-branch, abandon-feature: operator requests with request_id and the operator token) | ReconcilePassDriver (fleet-runtime) and CommandChannelHandler |
| `IntegrationQueries` | read models WorktreeList (branch side), PullRequestView, TrialMergeResult |  |
| `IntegrationEventHandlers` | policies on task-done, task-escalated, task-cancelled, attempt-failed (repair_attempts), worktree-rescued, assumptions-written, direct-idea-accepted, idea-tasks-landed, pull-request-closed, and job-abandoned and job-requeued of its own job kinds | InProcessReconcileConsumer |
| `GitObserver` | primary-branch-moved, remote-tips-fetched and check-merged facts read from git and from remote_tips each pass (clock initiated), and pull-request-merged and pull-request-closed from GitHub | GitPollObserver |

### Driven
| Port | Operations | Why it exists |
|---|---|---|
| `FeatureRepository` | load and save Feature with GateRuns and TrialMerges | persistence, collaborator or infrastructure need named in the operations |
| `TaskBranchRepository` | load and save TaskBranch by name | persistence, collaborator or infrastructure need named in the operations |
| `EventPublisher` | writes the event row in the same ledger transaction | persistence, collaborator or infrastructure need named in the operations |
| `GitGateway` | the only caller of rebase, merge-tree, fast-forward, push, fetch and branch delete (define N5); content containment, stash list; a git command that reads or writes a working tree or uses the network runs as a job through the JobQueue of fleet-runtime; only git update-ref on a local ref, a compare-and-set, runs in part 3 of a pass (A23, A32) | persistence, collaborator or infrastructure need named in the operations |
| `GateRunner` | queues the declared fast gate and full gate of the project as fast_gate and full_gate jobs on a candidate tip, and reads their results in a later pass (A23) | persistence, collaborator or infrastructure need named in the operations |
| `PullRequestGateway` | open-pull-request: opens a pull request with pr gh, or pushes and builds the compare URL with pr manual; marks it a draft when the feature goes back to open; nothing with remote = none (A28) | persistence, collaborator or infrastructure need named in the operations |
| `LandLock` | the land lock (1 h, 5 tries) | persistence, collaborator or infrastructure need named in the operations |
| `Clock` | due_at of branch rows and gate staleness | persistence, collaborator or infrastructure need named in the operations |
| `RescueRefGateway` | asks worktree-custody to write and read back a rescue ref of the old tip before each rebase or forced update (I10, A13) | persistence, collaborator or infrastructure need named in the operations |
| `BundleRepository` | load and save Bundle with its BundleMembers (bundles, bundle_members) and the candidate ref of each bundle | persistence, collaborator or infrastructure need named in the operations |

## 5. Adapters
| Port | Implementation | Technology | Notes |
|---|---|---|---|
| `IntegrationCommands` | `ReconcilePassDriver (fleet-runtime) and CommandChannelHandler` | production | - |
| `IntegrationEventHandlers` | `InProcessReconcileConsumer` | production | - |
| `GitObserver` | `GitPollObserver` | production | - |
| `FeatureRepository` | `SqliteFeatureRepository` | production | - |
| `FeatureRepository` | `InMemoryFeatureRepository` | in-memory fake | - |
| `TaskBranchRepository` | `SqliteTaskBranchRepository` | production | - |
| `TaskBranchRepository` | `InMemoryTaskBranchRepository` | in-memory fake | - |
| `EventPublisher` | `LedgerEventLog` | production | - |
| `EventPublisher` | `RecordingEventPublisher` | in-memory fake | - |
| `GitGateway` | `CliGitGateway` | production | vcs::git, vcs::network, vcs::landing |
| `GitGateway` | `TempRepoGitGateway` | in-memory fake | tests on a real temp repository |
| `GateRunner` | `ProjectGateRunner` | production | - |
| `GateRunner` | `ScriptedGateRunner` | in-memory fake | - |
| `PullRequestGateway` | `GhCliPullRequests` | production | - |
| `PullRequestGateway` | `ManualCompareUrl` | production | - |
| `PullRequestGateway` | `FakePullRequests` | in-memory fake | - |
| `LandLock` | `LedgerLandLock` | production | - |
| `LandLock` | `InMemoryLandLock` | in-memory fake | - |
| `Clock` | `LedgerFleetClock` | production | - |
| `Clock` | `FakeClock` | in-memory fake | - |
| `RescueRefGateway` | `InProcessWorktreeCustody` | production | - |
| `RescueRefGateway` | `FakeRescueRefs` | in-memory fake | - |
| `BundleRepository` | `SqliteBundleRepository` | production | - |
| `BundleRepository` | `InMemoryBundleRepository` | in-memory fake | - |

## 6. Domain events
### Published
| Event (`discover` id) | Produced by | Consumers | Contract payload (`connect.json`) | Delivery | Contract test |
|---|---|---|---|---|---|
| `pull-request-opened` | use case that emits it (§3) | operator | feature_id, pr_url | at-least-once | `contract: pull-request-opened` |
| `feature-opened` | use case that emits it (§3) | ideas | feature_id, branch, primary_branch, idea_id | at-least-once | `contract: feature-opened` |
| `task-branch-conflicted` | use case that emits it (§3) | task-execution | branch, task_id | at-least-once | `contract: task-branch-conflicted` |
| `resolver-task-opened` | use case that emits it (§3) | task-execution | feature_id, task_id, conflict | at-least-once | `contract: resolver-task-opened` |
| `task-branch-landed` | use case that emits it (§3) | worktree-custody, ideas | branch, task_id, feature_id, tip | at-least-once | `contract: task-branch-landed` |
| `feature-merged` | use case that emits it (§3) | ideas | feature_id | at-least-once | `contract: feature-merged` |
| `open-pull-request` | use case that emits it (§3) | github | feature_id, branch, base_branch | sync | `contract: open-pull-request` |
| `rescue-worktree` | use case that emits it (§3) | worktree-custody | branch, task_id, worktree | at-least-once | - |
| `task-branch-gate-failed` | use case that emits it (§3) | task-execution | branch, task_id, gate_output | at-least-once | `contract: task-branch-gate-failed` |
| `task-branch-repair-escalated` | use case that emits it (§3) | ideas, task-execution | branch, task_id, repair_attempts | at-least-once | `contract: task-branch-repair-escalated` |
| `feature-abandoned` | use case that emits it (§3) | ideas | feature_id | at-least-once | `contract: feature-abandoned` |
| `push-feature-branch` | use case that emits it (§3) | git | feature_id, branch, seen_remote_tip | at-least-once | `contract: push-feature-branch` |

### Consumed
| Event | From | Policy (`discover` id) | Handled by | Handler → command | Delivery | Idempotency |
|---|---|---|---|---|---|---|
| `primary-branch-moved` | git | policy (see canvas) | policy | reconcile pass handler | at-least-once | once per (primaryBranch, tip) - test `idempotent: primary-branch-moved` |
| `task-escalated` | task-execution | policy (see canvas) | policy | reconcile pass handler | at-least-once | the reconcile pass is level-triggered, so a reaction can run again; 1 needs-you file per (taskId, reason); integration records it once per taskId - test `idempotent: task-escalated` |
| `assumptions-written` | ideas | policy (see canvas) | policy | reconcile pass handler | at-least-once | the reconcile pass is level-triggered, so a reaction can run again; integration opens at most 1 feature per idea; open-feature is a no-op when the idea has a feature - test `idempotent: assumptions-written` |
| `direct-idea-accepted` | ideas | policy (see canvas) | policy | reconcile pass handler | at-least-once | the reconcile pass is level-triggered, so a reaction can run again; at most 1 feature per idea - test `idempotent: direct-idea-accepted` |
| `idea-tasks-landed` | ideas | policy (see canvas) | policy | reconcile pass handler | at-least-once | the reconcile pass is level-triggered, so a reaction can run again; the full gate runs once per (featureId, feature tip) - test `idempotent: idea-tasks-landed` |
| `attempt-failed` | task-execution | policy (see canvas) | policy | reconcile pass handler | at-least-once | the reconcile pass is level-triggered, so a reaction can run again; the worktree state moves once per attempt_id; repair_attempts of a branch goes up once per attempt_id - test `idempotent: attempt-failed` |
| `task-done` | task-execution | policy (see canvas) | policy | reconcile pass handler | at-least-once | the reconcile pass is level-triggered, so a reaction can run again; mark-branch-ready is a no-op for a ready branch - test `idempotent: task-done` |
| `pull-request-merged` | github | policy (see canvas) | policy | reconcile pass handler | at-least-once | once per feature_id; check-merged confirms the merge in the primary branch by content before feature-merged - test `idempotent: pull-request-merged` |
| `task-cancelled` | task-execution | policy (see canvas) | policy | reconcile pass handler | at-least-once | the reconcile pass is level-triggered, so a reaction can run again; a branch is rescued and abandoned once per branch - test `idempotent: task-cancelled` |
| `worktree-rescued` | worktree-custody | policy (see canvas) | policy | reconcile pass handler | at-least-once | the reconcile pass is level-triggered, so a reaction can run again; integration abandons a branch once per branch - test `idempotent: worktree-rescued` |
| `pull-request-closed` | github | policy (see canvas) | policy | reconcile pass handler | at-least-once | once per feature_id and pr_state - test `idempotent: pull-request-closed` |
| `job-abandoned` | fleet-runtime | policy (see canvas) | policy | reconcile pass handler | at-least-once | the reconcile pass is level-triggered, so a reaction can run again; the owner context reads the job row; 1 needs-you item per job key - test `idempotent: job-abandoned` |
| `job-requeued` | fleet-runtime | policy (see canvas) | policy | reconcile pass handler | at-least-once | the reconcile pass is level-triggered, so a reaction can run again; the next run starts once per job key - test `idempotent: job-requeued` |

## 7. Read models
| Read model | Source events | Readers | Storage | Rebuild |
|---|---|---|---|---|
| `WorktreeList` | task-branch-verified, task-branch-landed, task-branch-abandoned | operator and agents through horch CLI read-only commands | ledger tables and files of this context | derived again on each pass from the ledger |
| `PullRequestView` | pull-request-opened, feature-merged, feature-reopened, feature-abandoned, feature-branch-pushed, push-failed | operator and agents through horch CLI read-only commands | ledger tables and files of this context | derived again on each pass from the ledger |
| `TrialMergeResult` | trial-merge-passed, trial-merge-conflicted, feature-stacked | operator and agents through horch CLI read-only commands | ledger tables and files of this context | derived again on each pass from the ledger |

## 8. Persistence
Tables features, branches, bundles, bundle_members, gate_runs, trial_merges and remote_tips (Phase 4, A30). SqliteFeatureRepository, SqliteTaskBranchRepository and SqliteBundleRepository save through the one-door transition function. branches holds repair_attempts; features holds seen_tip, seen_remote_tip, push_state, push_not_before and pr_state. gate_runs and trial_merges rows point at their job. Every git command that reads or writes a working tree or uses the network (rebase, merge-tree, push, fetch, the bundle build, both gates) runs as a job outside the pass; only git update-ref on a local ref, a compare-and-set, runs in part 3 of a pass (A23, A32). Each rescue ref request to worktree-custody runs outside the ledger transaction. Network calls keep the bounds of vcs::network as the time limit of their job (04 F11).

## 9. Tests
| Kind | Name | Verifies |
|---|---|---|
| invariant | `i8_task_branch_landed_by_content` | I8: a task branch is landed only when its changes are in the feature branch by content |
| fault injection | `fm_squash_merged_feature_is_merged_by_content` | I8: a feature is merged only when its changes are in the primary branch by content, also after a squash merge |
| invariant | `i9_feature_branch_moves_only_by_fast_forward_to_a_gated_candidate` | I9: a feature branch moves only by fast-forward to a candidate that passed its gate; each ref move is a compare-and-set on seen_tip or seen_remote_tip, and a failed compare-and-set moves nothing and records the new tip |
| invariant | `i10_rebase_without_rescue_ref_is_refused` | I10: before each rebase or forced update of a branch, a rescue ref of the old tip exists |
| invariant | `i11_unknown_fleet_branch_is_adopted_as_orphan_and_reported` | I11: every fleet branch in git has a branches row; one without is adopted as an orphan row, never deleted |
| invariant | `i12_branch_with_stash_entry_is_not_deleted` | I12: a branch is deleted only when landed, merged or rescued, and no git stash entry names it; a task branch is a member of at most 1 bundle that is not ended |
| preservation | `pres_04_f7_red_land_resets_to_tip_before_the_land` | A red land after a rebase resets the branch to its tip from before the land (04 F7) |
| preservation | `pres_04_f9_cancelled_task_lands_nothing` | Under the land lock, just before the fast-forward, the task is still landing and held by its worker (04 F9) |
| preservation | `pres_04_f5_new_commits_reopen_the_land_and_later_steps` | A gate pass goes stale when either judged tip moves (04 F5) |
| preservation | `pres_04_f11_stalled_push_ends_at_10_min_and_frees_the_land_lock` | Every git network call has a wall-clock bound: push 10 min, others 3 min, as the time limit of its job (04 F11, A23) |
| preservation | `pres_04_l5_unlanded_task_keeps_worktree_and_blocks_dependents` | A task parked by done --unlanded keeps its worktree; dependents stay blocked (04 L5) |
| preservation | `pres_04_l9_unlanded_work_is_listed_and_decided_once` | Unfinished, reopen and dismiss keep a record of unlanded work; a finding is decided once (04 L9) |
| fault injection | `fm_failed_fast_gate_after_rebase_keeps_feature_tip_and_rescue_ref` | A fast gate failure after a rebase keeps the feature tip and a rescue ref (I9, I10, I12) |
| invariant | `feature_ready_refused_with_stale_full_gate_pass` | A feature is ready only with a full gate pass on its current tip over the fetched tip of the primary branch, and each idea of the feature integrating, cancelled or parked |
| invariant | `branch_ready_refused_while_task_not_done` | A task branch is ready only when its task is done (verified) |
| invariant | `abandon_waits_for_worktree_rescued` | An escalated task branch is abandoned only by the operator and only after worktree-rescued (connect N4) |
| invariant | `conflicting_features_stack_younger_on_older_once` | A feature is stacked on at most 1 older feature |
| use case | `given_2_features_touching_1_file_when_trial_merge_then_younger_stacked_and_1_resolver_task` | use case: run-trial-merges |
| use case | `given_3_ready_branches_when_bundle_then_fast_gate_runs_once_and_none_is_forgotten` | use case: build-bundle |
| idempotency | `handling primary-branch-moved twice (same primary_branch, tip) has the same effect as once` | idempotent: primary-branch-moved |
| contract | `published pull-request-opened carries exactly feature_id, pr_url (connect.json payload) and nothing consumers did not ask for` | contract: pull-request-opened |
| contract | `published feature-opened carries exactly feature_id, branch, primary_branch, idea_id (connect.json payload) and nothing consumers did not ask for` | contract: feature-opened |
| contract | `published task-branch-conflicted carries exactly branch, task_id (connect.json payload) and nothing consumers did not ask for` | contract: task-branch-conflicted |
| contract | `published resolver-task-opened carries exactly feature_id, task_id, conflict (connect.json payload) and nothing consumers did not ask for` | contract: resolver-task-opened |
| contract | `published task-branch-landed carries exactly branch, task_id, feature_id, tip (connect.json payload) and nothing consumers did not ask for` | contract: task-branch-landed |
| contract | `published feature-merged carries exactly feature_id (connect.json payload) and nothing consumers did not ask for` | contract: feature-merged |
| idempotency | `handling task-escalated twice (same message id) has the same effect as once` | idempotent: task-escalated |
| idempotency | `handling assumptions-written twice (same message id) has the same effect as once` | idempotent: assumptions-written |
| idempotency | `handling direct-idea-accepted twice (same message id) has the same effect as once` | idempotent: direct-idea-accepted |
| idempotency | `handling idea-tasks-landed twice (same message id) has the same effect as once` | idempotent: idea-tasks-landed |
| idempotency | `handling task-done twice (same message id) has the same effect as once` | idempotent: task-done |
| invariant | `no_rebase_runs_before_the_rescue_ref_reads_back` | No rebase runs before the rescue ref of the old tip reads back (I10, define N6) |
| invariant | `feature_rebase_during_running_attempt_then_landing_rebases_or_conflicts` | Task branches rebase at landing; a feature rebase does not wait for running attempts (define N7) |
| fitness | `fitness_git_calls_split_by_concern` | Only worktree-custody calls git worktree and writes refs/horch/rescue/; only integration calls rebase, merge-tree, fast-forward, push and branch delete (define N5) |
| contract | `published open-pull-request carries exactly feature_id, branch, base_branch (connect.json payload) and nothing consumers did not ask for` | contract: open-pull-request |
| idempotency | `handling pull-request-merged twice (same feature_id) has the same effect as once` | idempotent: pull-request-merged |
| contract | `published task-branch-gate-failed carries exactly branch, task_id, gate_output (connect.json payload) and nothing consumers did not ask for` | contract: task-branch-gate-failed |
| contract | `published task-branch-repair-escalated carries exactly branch, task_id, repair_attempts (connect.json payload) and nothing consumers did not ask for` | contract: task-branch-repair-escalated |
| contract | `published feature-abandoned carries exactly feature_id (connect.json payload) and nothing consumers did not ask for` | contract: feature-abandoned |
| contract | `published push-feature-branch carries exactly feature_id, branch, seen_remote_tip (connect.json payload) and nothing consumers did not ask for` | contract: push-feature-branch |
| idempotency | `handling attempt-failed twice (same attempt_id; repair_attempts goes up once per attempt_id) has the same effect as once` | idempotent: attempt-failed |
| idempotency | `handling task-cancelled twice (same task_id; a branch is rescued and abandoned once) has the same effect as once` | idempotent: task-cancelled |
| idempotency | `handling worktree-rescued twice (same branch; abandoned once) has the same effect as once` | idempotent: worktree-rescued |
| idempotency | `handling pull-request-closed twice (same feature_id, pr_state) has the same effect as once` | idempotent: pull-request-closed |
| idempotency | `handling job-abandoned twice (same job key) has the same effect as once` | idempotent: job-abandoned |
| idempotency | `handling job-requeued twice (same job key) has the same effect as once` | idempotent: job-requeued |
| invariant | `bundle_with_moved_feature_tip_is_dropped_and_members_go_back_to_ready` | A bundle moves the feature ref only by compare-and-set from its base tip; a failed compare-and-set drops the bundle and its members go back to ready (I9, A34) |
| invariant | `candidate_ref_is_deleted_and_read_back_after_the_last_child_bundle_ends` | fleetd deletes the candidate ref refs/horch/bundle/<id> of a landed, failed or dropped bundle when no child bundle of it is building, gating or passed, and reads the ref store again to verify the deletion (A34) |
| invariant | `task_branch_is_in_at_most_1_live_bundle` | A task branch is a member of at most 1 bundle that is not ended (I12) |
| invariant | `no_feature_rebase_while_a_bundle_is_gating_or_passed` | fleetd does not queue a rebase of a feature branch while 1 of its bundles is in gating or in passed (A34) |
| invariant | `branch_at_max_repair_attempts_goes_on_the_needs_you_list` | conflicted and gate_failed are owned by fleetd; a failed resolver or fix attempt adds 1 to repair_attempts, and at max_attempts the branch goes on the needs-you list (A27, A34) |
| invariant | `cancelled_task_branch_is_abandoned_after_worktree_rescued` | A branch of a cancelled task is abandoned only after its rescue ref is written (A34, connect F10) |
| invariant | `open_feature_merged_by_content_moves_ideas_in_flight_to_a_new_feature` | A merge with ideas in flight moves those ideas to a new feature (A28, A34) |
| invariant | `feature_abandon_writes_a_rescue_ref_for_each_unlanded_branch_first` | A feature has 1 of 4 states, open, ready, merged and abandoned; merged and abandoned are terminal, and only the operator sets abandoned (A28, A34) |
| invariant | `ready_feature_with_moved_tip_goes_back_to_open_and_its_pr_becomes_a_draft` | A feature has 1 of 4 states, open, ready, merged and abandoned; merged and abandoned are terminal, and only the operator sets abandoned (A28, A34) |
| invariant | `failed_push_runs_again_after_its_backoff_and_3_failures_add_1_needs_you_item` | A failed push sets push_state failed and push_not_before; at 3 failures in a row, 1 item goes on the needs-you list (A33) |
| invariant | `fetch_job_records_the_remote_tip_and_a_failed_fetch_runs_at_the_next_period` | A periodic fetch job records each remote tip in remote_tips (A23) |
| invariant | `remote_none_fetches_pushes_and_opens_nothing` | With remote = none, fleetd does not fetch, push or open a pull request; the pull request state follows the feature state (A28) |
| invariant | `gates_of_5_min_run_as_jobs_and_the_pass_does_not_wait` | Push, fetch, rebase, trial merge, bundle build and both gates are jobs outside the pass (A23) |
| invariant | `bundle_rows_commit_before_the_bundle_build_job` | fleetd writes the bundles row and its bundle_members rows in 1 transaction before the bundle_build job (A27) |
| invariant | `fast_gate_runs_once_per_bundle_as_a_job_keyed_by_both_tips` | The fast gate runs once on a bundle, as a fast_gate job keyed by the candidate tip and the base tip (A23, A33) |
| fitness | `fitness_domain_imports_nothing_outward` | §1 |

## 10. Integration & anti-corruption
integration alone calls rebase, merge-tree, fast-forward, push and branch delete (define N5). Rescue refs come from worktree-custody through RescueRefGateway, and no rebase runs before the ref reads back (A13). Task branches rebase at landing; a feature rebase does not wait for running attempts. git is an adopted system behind GitGateway: merge-tree facts become TrialMerge entities, and content containment decides landed and merged (I8). GitHub sits behind PullRequestGateway with the pr setting gh or manual; merged is read from git, never from the GitHub API alone. Conforms to task-execution through task-done and task-escalated; task-execution and integration are a partnership for task-cancelled, attempt-failed, task-branch-gate-failed and task-branch-repair-escalated (define N9). A cancelled task sends its branch through rescue-worktree to worktree-custody, and the branch is abandoned after worktree-rescued. horch feature abandon and horch branch abandon arrive as operator requests with request_id and the operator token. pull-request-closed from GitHub leaves the decision to the operator. With remote = none, no fetch, push or pull request happens. Its jobs (bundle_build, fast_gate, full_gate, trial_merge, rebase, fetch, push) run through the JobQueue of fleet-runtime (A16).

## 11. Decisions, assumptions, open points
- Decisions D1 (module root) and D4 (only the verifier sets done) apply to every context; D2 applies to task-execution; D3 (wrapper owner) is option B, worker-profiles (orchestrator, D15).
- Assumptions: A1, A2, A5, A7, A9, A13, A16, A17 · Open questions: none (Q3 is answered, see deprecated[] in code.json).
