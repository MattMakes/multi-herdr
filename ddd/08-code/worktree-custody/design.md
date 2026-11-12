# Design - Worktree custody (`worktree-custody`)

| | |
|---|---|
| Classification | supporting (cost-reducer) · sourcing build · pattern **active-record** |
| Deployable | `horch` (modular-monolith) · team `core-team` · data store own (the ledger, single writer fleetd) |
| Module path | `crates/horch-core/src/fleetd/worktree_custody` |
| Structure | records + use cases (active record) |
| Purpose | Worktree custody owns every worktree from the moment it is reserved to the moment it is removed, so that no work a worker paid for is lost (G2). It decides when a worktree may be created, reused, rescued or removed, and it reconciles the ledger, the work directory and git on every pass. It serves task-execution, which gets a worktree for each leased task. It does not land or delete branches (integration) and does not decide task states (task-execution). - canvas: `ddd/07-define/worktree-custody/bounded-context-canvas.md` |
| Domain roles | execution-context |

## 1. Module layout
```
crates/horch-core/src/fleetd/worktree_custody/
  mod.rs               public contract of the context (driving ports and published event types only)
  domain/
    worktree.rs        Worktree record and its state table
    rescue.rs          rescue plan: private index, commit-tree, update-ref, read back
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
No aggregates by design (pattern = active-record, strategize N2). The records and their rules:
- A worktree row is written in the same transaction that leases the task, before the directory exists. Only fleetd creates or removes worktrees (FR-11). A row left in reserved by a crash is created or dropped on the next pass.
- No worktree is removed until its commits, its uncommitted files and its untracked files are saved on a rescue ref under refs/horch/rescue/, and that ref is read back and shown to hold them (RR-9). A rescue that fails verification leaves the tree in place and escalates the task.
- Every pass compares the ledger with the work directory, and git worktree list every 60 s. A worktree on disk with no row, or with no live owner attempt, is adopted or rescued within 1 pass and never deleted (RR-10, I6).
- Drop and cleanup refuse a dirty tree, a branch with a git stash entry, unpublished unique commits, and a HEAD that moved during the checks (04 W6).
- Archive and trash refs only move forward, each ref write is read back before a folder is removed, and trash refs expire after 30 days (04 W8). Triage commits modified and untracked files before ARCHIVE or DELETE (04 W9).
- A worktree survives attempts: a retry, a fallback restart or a resume reuses the same worktree (understand N1). The worktree goes to landing when its task branch is verified and to landed when the branch landed; only a landed or rescued worktree is removed (decompose N6).
- An abandoned task branch is dropped only after worktree-rescued is recorded (connect N4). No git call runs inside a ledger transaction.

## 3. Application services (use cases)
| Use case | Command (`discover` id) | Trigger and rule | Loads | Decides / enforces |
|---|---|---|---|---|
| `CreateWorktree` | `create-worktree` | git worktree add on the task branch. Only fleetd creates worktrees. | store rows | see §2 |
| `ReconcileWorktrees` | `reconcile-worktrees` | Every pass reads rows and /fleet/work; every 60 s also git worktree list --porcelain. | store rows | see §2 |
| `RescueWorktree` | `rescue-worktree` | Private index, commit-tree, update-ref, then verify. Reads the ref back and compares every file before any removal (RR-9). | store rows | see §2 |
| `RemoveWorktree` | `remove-worktree` | Only after landed or a verified rescue. | store rows | see §2 |

Transaction: 1 aggregate (or 1 store call set) per use case through the one-door transition function; the event row is written in the same transaction; process starts, git calls and check runs happen after the commit (connect N4).

## 4. Ports
### Driving
| Port | Operations | Called by |
|---|---|---|
| `WorktreeCustodyCommands` | create, reconcile, rescue, remove; called by the reconcile pass and horch worktree restore | ReconcilePassDriver and CommandChannelHandler |
| `WorktreeCustodyQueries` | read model WorktreeRows for horch worktrees |  |
| `WorktreeCustodyEventHandlers` | policies on task-leased, attempt-failed, task-branch-landed | InProcessReconcileConsumer |
| `RescueRefs` | in-process call from integration: write and read back a rescue ref of a branch tip before a rebase or forced update (I10, define N6) | InProcessWorktreeCustody |

### Driven
| Port | Operations | Why it exists |
|---|---|---|
| `WorktreeCustodyStore` | worktrees rows; the reserved row is written in the leasing transaction | persistence, collaborator or infrastructure need named in the operations |
| `EventPublisher` | writes the event row in the same ledger transaction | persistence, collaborator or infrastructure need named in the operations |
| `WorktreeGitGateway` | the only caller of git worktree add, list --porcelain and remove, and the only writer of refs under refs/horch/rescue/ (define N5); private-index rescue commit, update-ref, read back; stash list; never inside a ledger transaction | persistence, collaborator or infrastructure need named in the operations |
| `WorkDirScanner` | lists the work directory and reads dirty and untracked state | persistence, collaborator or infrastructure need named in the operations |
| `Clock` | the 60 s git worktree list cadence and the 30 day trash ref expiry | persistence, collaborator or infrastructure need named in the operations |

## 5. Adapters
| Port | Implementation | Technology | Notes |
|---|---|---|---|
| `WorktreeCustodyCommands` | `ReconcilePassDriver and CommandChannelHandler` | production | - |
| `WorktreeCustodyEventHandlers` | `InProcessReconcileConsumer` | production | - |
| `WorktreeCustodyStore` | `SqliteWorktreeStore` | production | extends the worktrees table |
| `WorktreeCustodyStore` | `InMemoryWorktreeStore` | in-memory fake | - |
| `EventPublisher` | `LedgerEventLog` | production | - |
| `EventPublisher` | `RecordingEventPublisher` | in-memory fake | - |
| `WorktreeGitGateway` | `CliWorktreeGit` | production | taskledger::worktree_ops, triage refs |
| `WorktreeGitGateway` | `TempRepoWorktreeGit` | in-memory fake | - |
| `WorkDirScanner` | `FsWorkDirScanner` | production | - |
| `WorkDirScanner` | `FakeWorkDir` | in-memory fake | - |
| `Clock` | `PlatformAwakeClock` | production | - |
| `Clock` | `FakeClock` | in-memory fake | - |
| `RescueRefs` | `InProcessWorktreeCustody` | production | - |

## 6. Domain events
### Published
| Event (`discover` id) | Produced by | Consumers | Contract payload (`connect.json`) | Delivery | Contract test |
|---|---|---|---|---|---|
| `worktree-created` | use case that emits it (§3) | task-execution | worktree_id, task_id, path, branch | at-least-once | `contract: worktree-created` |
| `rescue-failed` | use case that emits it (§3) | task-execution | worktree_id, task_id | at-least-once | `contract: rescue-failed` |

### Consumed
| Event | From | Policy (`discover` id) | Handled by | Handler → command | Delivery | Idempotency |
|---|---|---|---|---|---|---|
| `task-leased` | task-execution | policy (see canvas) | policy | reconcile pass handler | at-least-once | the reconcile pass is level-triggered, so a reaction can run again; create-worktree is a no-op when the worktree exists for the task - test `idempotent: task-leased` |
| `attempt-failed` | task-execution | policy (see canvas) | policy | reconcile pass handler | at-least-once | the reconcile pass is level-triggered, so a reaction can run again; the worktree state moves once per attemptId - test `idempotent: attempt-failed` |
| `task-branch-landed` | integration | policy (see canvas) | policy | reconcile pass handler | at-least-once | the reconcile pass is level-triggered, so a reaction can run again; remove-worktree and roll-up-idea are no-ops when already done - test `idempotent: task-branch-landed` |

## 7. Read models
| Read model | Source events | Readers | Storage | Rebuild |
|---|---|---|---|---|
| `WorktreeRows` | worktree-created, worktree-orphaned, worktree-rescued, rescue-failed, worktree-removed | operator and agents through horch CLI read-only commands | ledger tables and files of this context | derived again on each pass from the ledger |

## 8. Persistence
Extends the existing worktrees table with state, owner_attempt, base_sha, head_sha, dirty, rescue_ref, disk_bytes and seen_at (Phase 3). The reserved row is written in the same transaction that leases the task. git worktree add, remove and the rescue commit run after the commit, never inside it.

## 9. Tests
| Kind | Name | Verifies |
|---|---|---|
| invariant | `i6_worktree_on_disk_without_row_is_adopted_not_deleted` | I6: every worktree on disk has a row, every row not removed has its directory, and none is removed unless landed or on a verified rescue ref |
| invariant | `rr9_rescue_ref_is_read_back_before_removal` | RR-9: no worktree is removed before its work is on a verified rescue ref |
| fault injection | `fm_killed_worker_dirty_untracked_files_on_verified_rescue_ref` | RR-9: no worktree is removed before its work is on a verified rescue ref |
| invariant | `failed_rescue_verification_keeps_the_tree` | A failed rescue leaves the tree and escalates (RR-9) |
| preservation | `pres_04_w6_drop_refuses_dirty_stash_unique_or_moved_head` | Drop and cleanup refuse dirty tree, stash entry, unique commits, moved HEAD (04 W6) |
| preservation | `pres_04_w8_refs_move_forward_and_are_read_back` | Archive and trash refs only move forward and are read back; trash expires after 30 days (04 W8) |
| preservation | `pres_04_w9_triage_commits_untracked_before_delete` | Triage commits modified and untracked files before ARCHIVE or DELETE (04 W9) |
| invariant | `reserved_row_left_by_crash_is_created_or_dropped_next_pass` | A worktree row is written before the directory exists (FR-11) |
| invariant | `retry_reuses_the_same_worktree` | A retry, a fallback restart or a resume reuses the same worktree |
| contract | `published worktree-created carries exactly worktree_id, task_id, path, branch (connect.json payload) and nothing consumers did not ask for` | contract: worktree-created |
| contract | `published rescue-failed carries exactly worktree_id, task_id (connect.json payload) and nothing consumers did not ask for` | contract: rescue-failed |
| idempotency | `handling task-leased twice (same message id) has the same effect as once` | idempotent: task-leased |
| idempotency | `handling attempt-failed twice (same message id) has the same effect as once` | idempotent: attempt-failed |
| idempotency | `handling task-branch-landed twice (same message id) has the same effect as once` | idempotent: task-branch-landed |
| invariant | `rescue_ref_for_integration_reads_back_before_return` | A rescue ref asked for by integration is written and read back before the call returns (I10, define N6) |
| fitness | `fitness_domain_imports_nothing_outward` | §1 |

## 10. Integration & anti-corruption
Wraps taskledger::worktree_ops and the triage ref code (04 W6, W8, W9) behind WorktreeGitGateway. It alone calls git worktree add, remove and list and writes refs under refs/horch/rescue/ (define N5); integration asks it for a rescue ref through RescueRefs before a rebase (A13). Reacts to task-leased, attempt-failed and task-branch-landed. Publishes worktree-created and rescue-failed to task-execution.

## 11. Decisions, assumptions, open points
- Decisions D1 (module root) and D4 (only the verifier sets done) apply to every context; D2 applies to task-execution; D3 (wrapper owner) is option B, worker-profiles (orchestrator, D15).
- Assumptions: A1, A2, A5, A7, A9, A13 · Open question: Q3 in code.json where it names this context.
