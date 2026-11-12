# Design - Task execution (`task-execution`)

| | |
|---|---|
| Classification | core (cost-reducer) · sourcing build · pattern **domain-model** |
| Deployable | `horch` (modular-monolith) · team `core-team` · data store own (the ledger, single writer fleetd) |
| Module path | `crates/horch-core/src/fleetd/task_execution` |
| Structure | hexagonal (domain / application / adapters) |
| Purpose | Task execution decides which worker runs each ready task, holds every attempt to an earned lease, and decides from facts on disk when a task is done, so that no task stalls silently (G1) and the subscription usage is used fully (G4). It serves the sub-orchestrator and the operator: they see each task verified done or escalated with a reason. It does not plan tasks (ideas), keep worktrees (worktree-custody), land branches (integration), or build launch environments and start processes (worker-profiles). - canvas: `ddd/07-define/task-execution/bounded-context-canvas.md` |
| Domain roles | execution-context |

## 1. Module layout
```
crates/horch-core/src/fleetd/task_execution/
  mod.rs               public contract of the context (driving ports and published event types only)
  domain/
    task.rs            Task root, Attempt entity, Lease, state machine
    verification.rs    Verification root, JudgeFinding, VerifiedVerdict (private constructor)
    dispatch.rs        DispatchPlanner domain service
    stall.rs           StallLadder domain service
    values.rs          TaskId, AttemptId, AwakeInstant, StallRung, PathClaim, Backoff
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
| `task` | `Task` | 7 | 12 commands → 15 events | `aggregate-canvas-task.md` |
| `verification` | `Verification` | 5 | 5 commands → 7 events | `aggregate-canvas-verification.md` |

Value objects: AttemptBudget, AttemptId, AttemptOutcome, AwakeInstant, Backoff, CheckOutcome, FindingRank, FixRound, JudgedTree, Lease, PathClaim, ProfileRef, ResultFile, ReviewOutcome, StallRung, TaskId, TaskKind, TaskState, VerificationId, VerifiedVerdict · Entities: Attempt, JudgeFinding · Domain services: DispatchPlanner, StallLadder
- `DispatchPlanner`: Picks ready tasks for workers across Task instances: I4, path overlap (04 L6), exclusive tasks, health guards and pool pacing. Pure: takes snapshots and a Clock reading.
- `StallLadder`: Maps a liveness snapshot (process alive, heartbeat age, activity age, agent state) and an awake-time reading to the next rung: doorbell, doorbell, end.

## 3. Application services (use cases)
| Use case | Command (`discover` id) | Trigger and rule | Loads | Decides / enforces |
|---|---|---|---|---|
| `PromoteTask` | `promote-task` | pending to ready when every dependency is done, or waiting_quota to ready when the pool resets. | `Task` by id | see §2 |
| `DispatchTask` | `dispatch-task` | No cap. Only the health guards (free disk, load) and the pacing of the pool hold dispatch. | `Task` by id | see §2 |
| `LaunchAttempt` | `launch-attempt` | Starts the wrapper and the CLI in its own process group. A resume after an answer also launches. Asks LaunchEnvironmentGateway, then AgentProcessGateway, after the ledger transaction commits (connect N3, N4). | `Task` by id | see §2 |
| `RenewLease` | `renew-lease` | Every reconcile pass (5 s). Lease time counts only while the machine is awake. | `Task` by id | see §2 |
| `ApplyStallLadder` | `apply-stall-ladder` | Every pass: doorbell, doorbell after 2 min, then end. stall_after default 5 min; start timeout 90 s. | `Task` by id | see §2 |
| `MoveToFallback` | `move-to-fallback` | Fallback persona on another provider, else waiting_quota with the lease paused. | `Task` by id | see §2 |
| `WriteResult` | `write-result` | horch result writes outbox/result.json. Records the result on the Attempt. It never moves the task to done (define N1). | `Task` by id | see §2 |
| `RunCheck` | `run-check` | The verifier, with today's check timeout (20 min, max 24 h). | `Verification` by id | see §2 |
| `RunReview` | `run-review` | Today's review scripts, unchanged. Rounds: light 1, full 3. | `Verification` by id | see §2 |
| `WriteJudgeVerdict` | `write-judge-verdict` | The worker of a judge task writes its verdict file. | `Verification` by id | see §2 |
| `OpenCriticalFix` | `open-critical-fix` | At most 2 fix rounds. | `Verification` by id | see §2 |
| `SettleHighFinding` | `settle-high-finding` | Fix or dismiss with a written reason. | `Verification` by id | see §2 |
| `EndAttempt` | `end-attempt` | Stops the process group through the platform layer (Linux: SIGINT, 20 s, SIGKILL). | `Task` by id | see §2 |
| `ScheduleRetry` | `schedule-retry` | Reads max_attempts and backoff from the profile. | `Task` by id | see §2 |
| `EscalateTask` | `escalate-task` | Publishes task-escalated with its reason. ideas puts the task on the 1 needs-you list (A14). | `Task` by id | see §2 |
| `RequeueTask` | `requeue-task` | Resets attempt_floor. | `Task` by id | see §2 |
| `MarkTaskDone` | `mark-task-done` | The verifier writes verdict.json. Takes a VerifiedVerdict; only the verification module can construct one (D4). | `Task` by id | see §2 |

Transaction: 1 aggregate (or 1 store call set) per use case through the one-door transition function; the event row is written in the same transaction; process starts, git calls and check runs happen after the commit (connect N4).

## 4. Ports
### Driving
| Port | Operations | Called by |
|---|---|---|
| `TaskExecutionCommands` | one operation per use case; called by the reconcile pass, the file ingester and the command channel | ReconcilePassDriver (fleet-runtime) and CommandChannelHandler |
| `TaskExecutionQueries` | read models StartFile and JudgeFindings; used by horch CLI read-only commands |  |
| `TaskExecutionEventHandlers` | policies on plan-committed, plan-refused, task-replanned, question-answered, own-assumption-recorded, worktree-created, rescue-failed, task-branch-conflicted, resolver-task-opened; at-least-once, idempotent on the connect key, agent-status-changed | InProcessReconcileConsumer |
| `AttemptFileIngest` | result.json, exit.json, heartbeat.json, progress and usage-limit files of an attempt outbox (write-result, attempt-exited, usage-limit-reported); 1 ingestion per (path, sha256) | OutboxFileIngester (atomic files, ingested table) |

### Driven
| Port | Operations | Why it exists |
|---|---|---|
| `TaskRepository` | load and save Task with its Attempts and Lease through the one-door transition function of fleet-runtime | persistence, collaborator or infrastructure need named in the operations |
| `VerificationRepository` | load and save Verification by task id | persistence, collaborator or infrastructure need named in the operations |
| `EventPublisher` | writes the event row in the same ledger transaction as the state change; consumers react in the reconcile pass | persistence, collaborator or infrastructure need named in the operations |
| `LaunchEnvironmentGateway` | asks worker-profiles for the launch environment of a profile (sync, in-process) | persistence, collaborator or infrastructure need named in the operations |
| `AgentProcessGateway` | sends start-agent-process to worker-profiles; never inside a ledger transaction (connect N3, N4) | persistence, collaborator or infrastructure need named in the operations |
| `CheckRunner` | runs the acceptance command and the review scripts in the worktree in the worker sandbox, with the V-10 timeout | persistence, collaborator or infrastructure need named in the operations |
| `LivenessProbe` | process alive with start time through the platform interface, heartbeat age from heartbeat-written, last activity, and the herdr agent status from agent-status-changed | persistence, collaborator or infrastructure need named in the operations |
| `QuotaGateway` | pool usage window and reset time from the CLI clients (06 S13) | persistence, collaborator or infrastructure need named in the operations |
| `Clock` | awake time for leases and stall rungs (PL-4) | persistence, collaborator or infrastructure need named in the operations |
| `WorkerFiles` | writes task.yaml (assign-task) and inbox/START.md (start-work) before the launch, and verdict.json (task-verdict-written); atomic, never rewritten | persistence, collaborator or infrastructure need named in the operations |

## 5. Adapters
| Port | Implementation | Technology | Notes |
|---|---|---|---|
| `TaskExecutionCommands` | `ReconcilePassDriver (fleet-runtime) and CommandChannelHandler` | production | - |
| `TaskExecutionEventHandlers` | `InProcessReconcileConsumer` | production | - |
| `AttemptFileIngest` | `OutboxFileIngester` | production | atomic files, ingested table |
| `TaskRepository` | `SqliteTaskRepository` | production | - |
| `TaskRepository` | `InMemoryTaskRepository` | in-memory fake | - |
| `VerificationRepository` | `SqliteVerificationRepository` | production | - |
| `VerificationRepository` | `InMemoryVerificationRepository` | in-memory fake | - |
| `EventPublisher` | `LedgerEventLog` | production | - |
| `EventPublisher` | `RecordingEventPublisher` | in-memory fake | - |
| `LaunchEnvironmentGateway` | `InProcessWorkerProfiles` | production | - |
| `LaunchEnvironmentGateway` | `FakeLaunchEnvironment` | in-memory fake | - |
| `AgentProcessGateway` | `InProcessWorkerProfiles` | production | - |
| `AgentProcessGateway` | `FakeAgentProcesses` | in-memory fake | - |
| `CheckRunner` | `SandboxedCheckRunner` | production | wraps taskledger::finish review scripts |
| `CheckRunner` | `ScriptedCheckRunner` | in-memory fake | - |
| `LivenessProbe` | `PlatformLivenessProbe` | production | crates/horch-platform, heartbeat.json, agent-status-changed |
| `LivenessProbe` | `FakeLivenessProbe` | in-memory fake | - |
| `QuotaGateway` | `CliQuotaProbe` | production | routing::quota |
| `QuotaGateway` | `FakeQuota` | in-memory fake | - |
| `Clock` | `PlatformAwakeClock` | production | - |
| `Clock` | `FakeClock` | in-memory fake | - |
| `WorkerFiles` | `FsWorkerFiles` | production | ProtectedFiles of fleet-runtime |
| `WorkerFiles` | `InMemoryWorkerFiles` | in-memory fake | - |

## 6. Domain events
### Published
| Event (`discover` id) | Produced by | Consumers | Contract payload (`connect.json`) | Delivery | Contract test |
|---|---|---|---|---|---|
| `task-escalated` | use case that emits it (§3) | ideas, integration | task_id, reason | at-least-once | `contract: task-escalated` |
| `replan-requested` | use case that emits it (§3) | ideas | task_id, attempts | at-least-once | `contract: replan-requested` |
| `task-leased` | use case that emits it (§3) | worktree-custody | task_id, attempt_id, worktree_id, branch | at-least-once | `contract: task-leased` |
| `attempt-failed` | use case that emits it (§3) | worktree-custody | attempt_id, task_id, outcome | at-least-once | `contract: attempt-failed` |
| `task-done` | use case that emits it (§3) | integration | task_id, branch, verdict | at-least-once | `contract: task-done` |
| `assign-task` | use case that emits it (§3) | worker | task, idea, key, attempt, profile, worktree, branch, read_first[], goal, covers[], acceptance[], lease{expires, renew_every_s}, report_to | at-least-once | `contract: assign-task` |
| `start-work` | use case that emits it (§3) | worker | task_yaml, rescue_commit?, progress_notes? | at-least-once | `contract: start-work` |
| `task-verdict-written` | use case that emits it (§3) | sub-orchestrator | task, attempt, decision, reasons[] | at-least-once | `contract: task-verdict-written` |

### Consumed
| Event | From | Policy (`discover` id) | Handled by | Handler → command | Delivery | Idempotency |
|---|---|---|---|---|---|---|
| `attempt-exited` | agent-cli | policy (see canvas) | policy | reconcile pass handler | at-least-once | once per attemptId; exit.json is immutable - test `idempotent: attempt-exited` |
| `usage-limit-reported` | provider-pool | policy (see canvas) | policy | reconcile pass handler | at-least-once | once per (attemptId, pool); a repeat for a moved attempt is ignored - test `idempotent: usage-limit-reported` |
| `plan-committed` | ideas | policy (see canvas) | policy | reconcile pass handler | at-least-once | the reconcile pass is level-triggered, so a reaction can run again; promote-task is a no-op for a task past planned - test `idempotent: plan-committed` |
| `plan-refused` | ideas | policy (see canvas) | policy | reconcile pass handler | at-least-once | the reconcile pass is level-triggered, so a reaction can run again; escalate-task is a no-op for an escalated task - test `idempotent: plan-refused` |
| `task-replanned` | ideas | policy (see canvas) | policy | reconcile pass handler | at-least-once | the reconcile pass is level-triggered, so a reaction can run again; promote once per (taskId, replan round) - test `idempotent: task-replanned` |
| `question-answered` | ideas | policy (see canvas) | policy | reconcile pass handler | at-least-once | the reconcile pass is level-triggered, so a reaction can run again; 1 launch per (taskId, questionId) - test `idempotent: question-answered` |
| `own-assumption-recorded` | ideas | policy (see canvas) | policy | reconcile pass handler | at-least-once | the reconcile pass is level-triggered, so a reaction can run again; 1 launch per (taskId, questionId) - test `idempotent: own-assumption-recorded` |
| `worktree-created` | worktree-custody | policy (see canvas) | policy | reconcile pass handler | at-least-once | the reconcile pass is level-triggered, so a reaction can run again; launch once per (taskId, attemptId) - test `idempotent: worktree-created` |
| `rescue-failed` | worktree-custody | policy (see canvas) | policy | reconcile pass handler | at-least-once | the reconcile pass is level-triggered, so a reaction can run again; escalate-task is a no-op for an escalated task - test `idempotent: rescue-failed` |
| `task-branch-conflicted` | integration | policy (see canvas) | policy | reconcile pass handler | at-least-once | the reconcile pass is level-triggered, so a reaction can run again; 1 new attempt per (taskId, conflicting feature tip) - test `idempotent: task-branch-conflicted` |
| `resolver-task-opened` | integration | policy (see canvas) | policy | reconcile pass handler | at-least-once | the reconcile pass is level-triggered, so a reaction can run again; 1 resolver task per (featureId, conflict) - test `idempotent: resolver-task-opened` |
| `heartbeat-written` | worker-profiles | policy (see canvas) | policy | reconcile pass handler | at-least-once | the file is rewritten every 10 s; the highest seq wins and a lower or equal seq is ignored - test `idempotent: heartbeat-written` |
| `agent-status-changed` | herdr | policy (see canvas) | policy | reconcile pass handler | at-least-once | edge events can be lost by design; each pass re-reads session.snapshot, so a repeat or a loss changes nothing - test `idempotent: agent-status-changed` |

## 7. Read models
| Read model | Source events | Readers | Storage | Rebuild |
|---|---|---|---|---|
| `StartFile` | task-leased, worktree-created | operator and agents through horch CLI read-only commands | ledger tables and files of this context | derived again on each pass from the ledger |
| `JudgeFindings` | judge-verdict-written, critical-fix-opened, high-finding-settled | operator and agents through horch CLI read-only commands | ledger tables and files of this context | derived again on each pass from the ledger |

## 8. Persistence
Tables tasks, task_deps, task_covers, attempts (rows with a task_id), leases. SqliteTaskRepository loads 1 task with its attempts and its lease row and saves them through the one-door transition function of fleet-runtime in 1 transaction (04 S7). SqliteVerificationRepository keeps verification state and judge findings in a verifications table keyed by task_id (new in schema v8). No process start, check run or git call runs inside a transaction (connect N4): the use case commits, then calls AgentProcessGateway or CheckRunner, then records the outcome in a second transaction.

## 9. Tests
| Kind | Name | Verifies |
|---|---|---|
| invariant | `i1_leased_task_without_unexpired_lease_is_stalled_in_the_same_pass` | I1: a leased or running task has exactly 1 unexpired lease and 1 live attempt |
| invariant | `i2_task_with_an_undone_dependency_is_not_promoted` | I2: no task is ready while a dependency is not done |
| invariant | `i4_dispatch_refuses_a_worker_with_a_live_attempt` | I4: no worker runs more than 1 attempt at a time |
| invariant | `i5_orphan_process_or_pane_is_stopped` | I5: every live pane and process under the fleet belongs to a live attempt |
| fault injection | `fm_result_file_alone_leaves_task_verifying` | Only a VerifiedVerdict from the verification module moves a task to done (RR-3, define N1) |
| invariant | `verified_verdict_has_no_constructor_outside_the_verification_module (compile-fail test)` | Only a VerifiedVerdict from the verification module moves a task to done (RR-3, define N1) |
| preservation | `pres_04_s7_reap_requeues_in_1_transaction_before_the_signal` | A reap ends the attempt and requeues the task in 1 transaction before any signal (04 S7) |
| fault injection | `fm_fleetd_killed_mid_reap_requeues_exactly_once` | A reap ends the attempt and requeues the task in 1 transaction before any signal (04 S7) |
| preservation | `pres_04_s8_stop_signals_only_the_recorded_pid_of_this_host` | Stop signals only the recorded PID with its start time and refuses another host (04 S8) |
| preservation | `pres_04_l8_operator_requeue_resets_the_attempt_count` | An operator requeue resets the attempt count through attempt_floor (04 L8) |
| preservation | `pres_04_l6_overlapping_paths_are_not_dispatched_together` | No 2 running tasks of 1 feature own overlapping paths; an exclusive task runs alone (04 L6) |
| preservation | `pres_06_s13_quota_probe_holds_no_credential` | Quota probes read limits through each CLI client; horch holds no credential (06 S13) |
| fault injection | `fm5_usage_limit_burns_no_attempt_and_moves_to_fallback` | A usage limit burns no attempt (RR-11) |
| invariant | `retry_backoff_30s_2m_8m_then_replan_then_escalate` | A task has at most max_attempts attempts, then 1 replan, then escalation (RR-6) |
| fault injection | `fm3_stopped_worker_loses_lease_and_task_is_retried` | A stalled worker loses its lease (RR-1, RR-2) |
| invariant | `sleep_does_not_expire_a_lease` | Lease and stall time count only while the machine is awake (PL-4) |
| invariant | `check_runs_in_the_worker_sandbox_not_with_fleetd_rights` | A verdict is verified only when result.json validates and the check passes in the worker sandbox |
| invariant | `code_kind_needs_review_pass_with_at_most_2_requeues` | A code kind is verified only after the review scripts pass (V-14 limits) |
| invariant | `judgment_kind_with_open_critical_is_not_verified` | A judgment kind is verified only with no open CRITICAL judge finding |
| invariant | `third_critical_round_escalates_to_the_operator` | At most 2 critical fix rounds, then the task escalates |
| invariant | `second_judge_new_medium_or_low_finding_is_dropped` | A second judge raises no new MEDIUM or LOW finding |
| transition | `transition_verifying_to_done_only_with_verified_verdict` | transition: verifying -> done |
| transition | `transition_running_to_waiting_quota_pauses_the_lease` | transition: running -> waiting_quota |
| transition | `transition_escalated_to_ready_only_by_operator_requeue` | transition: escalated -> ready |
| use case | `given_ready_task_and_healthy_profile_when_dispatch_then_task_leased_and_worktree_row_reserved` | use case: dispatch-task |
| use case | `given_running_attempt_when_result_file_ingested_then_result_written_and_task_not_done` | use case: write-result |
| idempotency | `handling attempt-exited twice (same attempt_id; exit.json is immutable) has the same effect as once` | idempotent: attempt-exited |
| idempotency | `handling usage-limit-reported twice (same attempt_id, pool) has the same effect as once; a repeat for a moved attempt is ignored` | idempotent: usage-limit-reported |
| contract | `published task-escalated carries exactly task_id, reason (connect.json payload) and nothing consumers did not ask for` | contract: task-escalated |
| contract | `published replan-requested carries exactly task_id, attempts (connect.json payload) and nothing consumers did not ask for` | contract: replan-requested |
| contract | `published task-leased carries exactly task_id, attempt_id, worktree_id, branch (connect.json payload) and nothing consumers did not ask for` | contract: task-leased |
| contract | `published attempt-failed carries exactly attempt_id, task_id, outcome (connect.json payload) and nothing consumers did not ask for` | contract: attempt-failed |
| contract | `published task-done carries exactly task_id, branch, verdict (connect.json payload) and nothing consumers did not ask for` | contract: task-done |
| idempotency | `handling plan-committed twice (same message id) has the same effect as once` | idempotent: plan-committed |
| idempotency | `handling plan-refused twice (same message id) has the same effect as once` | idempotent: plan-refused |
| idempotency | `handling task-replanned twice (same message id) has the same effect as once` | idempotent: task-replanned |
| idempotency | `handling question-answered twice (same message id) has the same effect as once` | idempotent: question-answered |
| idempotency | `handling own-assumption-recorded twice (same message id) has the same effect as once` | idempotent: own-assumption-recorded |
| idempotency | `handling worktree-created twice (same message id) has the same effect as once` | idempotent: worktree-created |
| idempotency | `handling rescue-failed twice (same message id) has the same effect as once` | idempotent: rescue-failed |
| idempotency | `handling task-branch-conflicted twice (same message id) has the same effect as once` | idempotent: task-branch-conflicted |
| idempotency | `handling resolver-task-opened twice (same message id) has the same effect as once` | idempotent: resolver-task-opened |
| invariant | `review_requeue_keeps_its_own_counter` | An operator requeue sets the attempt count to 0; a review requeue keeps its own counter (orchestrator, D15) |
| invariant | `result_json_with_another_status_is_refused` | result.json has a status from the closed list completed, blocked, failed |
| invariant | `usage_limit_without_reset_at_waits_for_pool_probe_ok` | A usage limit with no reset_at waits until the pool probe reports ok |
| contract | `published assign-task carries exactly task, idea, key, attempt, profile, worktree, branch, read_first[], goal, covers[], acceptance[], lease{expires, renew_every_s}, report_to (connect.json payload) and nothing consumers did not ask for` | contract: assign-task |
| contract | `published start-work carries exactly task_yaml, rescue_commit?, progress_notes? (connect.json payload) and nothing consumers did not ask for` | contract: start-work |
| contract | `published task-verdict-written carries exactly task, attempt, decision, reasons[] (connect.json payload) and nothing consumers did not ask for` | contract: task-verdict-written |
| idempotency | `handling heartbeat-written twice (same seq; a lower or equal seq is ignored) has the same effect as once` | idempotent: heartbeat-written |
| idempotency | `handling agent-status-changed twice (same pane, agent_status; each pass re-reads session.snapshot) has the same effect as once` | idempotent: agent-status-changed |
| fitness | `fitness_domain_imports_nothing_outward` | §1 |

## 10. Integration & anti-corruption
Conforms to ideas (plan-committed, task-replanned) and to worktree-custody (worktree-created) through their published events. Talks to worker-profiles only through LaunchEnvironmentGateway and AgentProcessGateway (connect N3, D3). Writes task.yaml, START.md and verdict.json through WorkerFiles. Publishes task-escalated; ideas owns the needs-you list (A14). The herdr agent status arrives as agent-status-changed, and heartbeat.json comes from the fleet-run wrapper of worker-profiles. Reads the existing review scripts of taskledger::finish through CheckRunner unchanged (D6, V-14). Agent CLIs reach this context only as files in the attempt outbox.

## 11. Decisions, assumptions, open points
- Decisions D1 (module root) and D4 (only the verifier sets done) apply to every context; D2 applies to task-execution; D3 (wrapper owner) is option B, worker-profiles (orchestrator, D15).
- Assumptions: A1, A2, A5, A7, A9, A3, A4, A10, A14 · Open question: Q3 in code.json where it names this context.
