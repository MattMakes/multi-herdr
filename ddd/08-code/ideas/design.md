# Design - Ideas (`ideas`)

| | |
|---|---|
| Classification | supporting (cost-reducer) · sourcing build · pattern **active-record** |
| Deployable | `horch` (modular-monolith) · team `core-team` · data store own (the ledger, single writer fleetd) |
| Module path | `crates/horch-core/src/fleetd/ideas` |
| Structure | records + use cases (active record) |
| Purpose | Ideas takes each rough idea of the operator from capture to 1 committed, validated plan, and closes the idea when its feature is merged (G3). It decides when an idea may move to its next state, holds the operator approval gate, and answers worker questions against the approved assumptions, so that no AI plans 2 ideas at once and no task exists outside an approved plan (FR-2, FR-10, FR-12). It does not run tasks (task-execution), land branches or open the feature (integration), or start processes (fleet-runtime). - canvas: `ddd/07-define/ideas/bounded-context-canvas.md` |
| Domain roles | specification-model, approver |

## 1. Module layout
```
crates/horch-core/src/fleetd/ideas/
  mod.rs               public contract of the context (driving ports and published event types only)
  domain/
    idea.rs            Idea record and its state table
    question.rs        Question record
    plan_validator.rs  PlanValidator domain service (pure)
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
Dependency rule: `domain/` imports nothing outside itself and std; `application/` imports `domain/` only; `adapters/` may import both and existing horch-core modules. No file under `crates/horch-core/src/fleetd/` holds `cfg(target_os)`, `cfg(unix)` or `cfg(windows)`. Enforced by `crates/horch-core/tests/arch_scan.rs` (tests: `fitness_domain_imports_nothing_outward`, `fitness_fleetd_tree_has_no_os_cfg`, `fitness_only_fleet_runtime_opens_the_ledger_writable`).
Composition root: `crates/horch/src/cmd/fleetd.rs` (daemon and `horch fleetd check`) wires every adapter to its port; short-lived CLI commands wire the read-only queries and the command channel client.

## 2. Domain model
No aggregates by design (pattern = active-record, strategize N2). The records and their rules:
- Only code advances an idea. When an idea is captured, the dispatcher starts its sub-orchestrator on the next pass and the idea goes to clarifying; a --direct idea goes to ticketing with no research and no interview (FR-1, FR-2, FR-12).
- An idea passes awaiting_approval only when approval.json, written by fleetd, holds the sha256 of the current assumptions.md, and the approve command ran outside every fleet attempt. An approval written by any other process is ignored (FR-10, I7, C5).
- The assumptions validator refuses assumptions.md while any item is marked [open]. When the file validates, fleetd requests the operator approval and posts a herdr notification.
- While an idea waits on the operator, its lease is paused. After 30 min idle the sub-orchestrator process stops; after 7 days with no answer the idea goes to parked with a note in needs-you/.
- When the assumptions are approved, or revised and approved again, the dispatcher starts a fresh sub-orchestrator process for the next phase. No phase carries a context window into the next (understand N11 keeps the decision rule of the sub-orchestrator).
- A plan is committed all-or-nothing, and only after it passes schema, DAG, coverage, profile and citation checks; a plan for a project with no declared fast gate and full gate is refused (FR-3, FR-4). A judgment-kind task without a paired judge task in another session refuses the plan (understand N12).
- Every requirement of an idea in in_progress or later is covered by at least 1 task that is not cancelled (I3).
- A worker question is a ledger row with an owner, the sub-orchestrator of the feature. On each pass fleetd starts the owner if it is not running, until an answer is recorded; past its time limit the question escalates to needs-you/ (RR-12, discover Q2 for the limit).
- The sub-orchestrator answers a question in 1 of 4 ways: cite the approved line, send a revise to the operator, replan the task, or record its own reversible assumption. Only product direction, scope or a trade-off the operator must own goes to the operator (understand N11, design decision D13).
- When task-execution asks for a replan, the sub-orchestrator replans the task once; the replanned task goes back to task-execution as task-replanned.
- When every task of the idea is landed on its feature branch, the idea goes to integrating. When the feature is merged and every worktree of the idea is removed, the idea goes to merged (discover N4).
- Every feature that is not merged and not abandoned has exactly 1 live sub-orchestrator or waits on the operator (I7, I13). The dispatcher starts a missing one and stops an extra one on each pass, through worker-profiles launch-environment and fleet-runtime start-agent-process (connect N3).

Domain services: `PlanValidator`: Schema, DAG, coverage, profiles, model guide, paired judges, declared gates and citations to approved assumptions; pure over plan.yaml and the requirements.

## 3. Application services (use cases)
| Use case | Command (`discover` id) | Trigger and rule | Loads | Decides / enforces |
|---|---|---|---|---|
| `AddIdea` | `add-idea` | horch idea add "..." or -f notes.md, optional --to <feature>. | store rows | see §2 |
| `StartSubOrchestrator` | `start-sub-orchestrator` | fleetd starts 1 fresh process per phase in the pane of the feature, and restarts a dead or missing one on the next pass. Asks LaunchEnvironmentGateway, then AgentProcessGateway, after commit (connect N3). | store rows | see §2 |
| `AskOperator` | `ask-operator` | 1 question at a time: what done looks like, scope edges, constraints, affected areas, priority. | store rows | see §2 |
| `AnswerInterviewQuestion` | `answer-interview-question` | The operator answers in the pane. horch idea note records it. | store rows | see §2 |
| `AmendIdea` | `amend-idea` | horch idea amend <id> "...". | store rows | see §2 |
| `WriteAssumptions` | `write-assumptions` | Writes assumptions.md: [confirmed] and [assumed] items, none [open]. | store rows | see §2 |
| `RequestApproval` | `request-approval` | Moves the idea to awaiting_approval and notifies the operator. Reminds once an hour in working hours. | store rows | see §2 |
| `ParkIdea` | `park-idea` | Issued by policy after 7 days of waiting or an escalation. The operator also runs horch idea park. | store rows | see §2 |
| `CancelIdea` | `cancel-idea` | horch idea cancel. | store rows | see §2 |
| `ReviseAssumptions` | `revise-assumptions` | horch idea revise <id> "...". | store rows | see §2 |
| `ApproveAssumptions` | `approve-assumptions` | horch idea approve <id>. Refused inside any fleet attempt; profiles deny it. fleetd writes approval.json with the sha256 the operator saw; any other approval file is ignored (I7). | store rows | see §2 |
| `AddDirectIdea` | `add-direct-idea` | horch idea add --direct <ticket file>, optional --to. Refused inside a fleet attempt. | store rows | see §2 |
| `WriteSpec` | `write-spec` | Writes spec.md and requirements.yaml after approval. | store rows | see §2 |
| `SubmitPlan` | `submit-plan` | Writes tickets and plan.yaml. | store rows | see §2 |
| `ValidatePlan` | `validate-plan` | The plan validator: paired judges, declared gates, model guide, overlapping paths. Commits tasks in 1 transaction. | store rows | see §2 |
| `AskQuestion` | `ask-question` | Writes 1 question file, commits work in progress, exits. | store rows | see §2 |
| `AnswerQuestion` | `answer-question` | fleetd starts the owner on each pass until an answer is recorded. 4 cases (design, Ideas, How a worker's question is resolved). | store rows | see §2 |
| `EscalateQuestion` | `escalate-question` | Fires when the time limit of the question row passes. | store rows | see §2 |
| `ReplanTask` | `replan-task` | Once per task. | store rows | see §2 |
| `RollUpIdea` | `roll-up-idea` | in_progress to integrating. | store rows | see §2 |
| `CloseIdea` | `close-idea` | Needs every worktree of the idea removed. | store rows | see §2 |

Transaction: 1 aggregate (or 1 store call set) per use case through the one-door transition function; the event row is written in the same transaction; process starts, git calls and check runs happen after the commit (connect N4).

## 4. Ports
### Driving
| Port | Operations | Called by |
|---|---|---|
| `IdeasCommands` | one operation per use case; called by the command channel (horch idea ...), the reconcile pass and the idea-folder ingester | CommandChannelHandler and ReconcilePassDriver |
| `IdeasQueries` | read models IdeaBoard, InterviewRecord, InFlightDiffs, ApprovedAssumptionsAndSpec |  |
| `IdeasEventHandlers` | policies on feature-opened, replan-requested, task-branch-landed, feature-merged | InProcessReconcileConsumer |
| `IdeaFolderIngest` | assumptions.md, spec.md, requirements.yaml, plan.yaml, question and answer files; 1 ingestion per (path, sha256) | IdeaFolderIngester |

### Driven
| Port | Operations | Why it exists |
|---|---|---|
| `IdeasStore` | ideas, requirements, questions rows and sub-orchestrator attempt rows (task_id null) | persistence, collaborator or infrastructure need named in the operations |
| `EventPublisher` | writes the event row in the same ledger transaction | persistence, collaborator or infrastructure need named in the operations |
| `LaunchEnvironmentGateway` | asks worker-profiles for the idea-lead launch environment | persistence, collaborator or infrastructure need named in the operations |
| `AgentProcessGateway` | starts or stops the sub-orchestrator pane through fleet-runtime | persistence, collaborator or infrastructure need named in the operations |
| `NeedsYouInbox` | approval requests, parked ideas and escalated questions; herdr notification | persistence, collaborator or infrastructure need named in the operations |
| `AttemptContext` | tells whether the caller runs inside a fleet attempt, so approve and --direct refuse there | persistence, collaborator or infrastructure need named in the operations |
| `Clock` | 30 min idle stop, 7 day park, question time limit, hourly reminder | persistence, collaborator or infrastructure need named in the operations |

## 5. Adapters
| Port | Implementation | Technology | Notes |
|---|---|---|---|
| `IdeasCommands` | `CommandChannelHandler` | production | and ReconcilePassDriver |
| `IdeasEventHandlers` | `InProcessReconcileConsumer` | production | - |
| `IdeaFolderIngest` | `IdeaFolderIngester` | production | - |
| `IdeasStore` | `SqliteIdeasStore` | production | - |
| `IdeasStore` | `InMemoryIdeasStore` | in-memory fake | - |
| `EventPublisher` | `LedgerEventLog` | production | - |
| `EventPublisher` | `RecordingEventPublisher` | in-memory fake | - |
| `LaunchEnvironmentGateway` | `InProcessWorkerProfiles` | production | - |
| `LaunchEnvironmentGateway` | `FakeLaunchEnvironment` | in-memory fake | - |
| `AgentProcessGateway` | `InProcessFleetRuntime` | production | - |
| `AgentProcessGateway` | `FakeAgentProcesses` | in-memory fake | - |
| `NeedsYouInbox` | `NeedsYouFolder` | production | - |
| `NeedsYouInbox` | `InMemoryNeedsYou` | in-memory fake | - |
| `AttemptContext` | `EnvAttemptContext` | production | - |
| `AttemptContext` | `FakeAttemptContext` | in-memory fake | - |
| `Clock` | `PlatformAwakeClock` | production | - |
| `Clock` | `FakeClock` | in-memory fake | - |

## 6. Domain events
### Published
| Event (`discover` id) | Produced by | Consumers | Contract payload (`connect.json`) | Delivery | Contract test |
|---|---|---|---|---|---|
| `approval-requested` | use case that emits it (§3) | operator | ideaId, assumptionsHash | at-least-once | `contract: approval-requested` |
| `worker-question-asked` | use case that emits it (§3) | sub-orchestrator | taskId, questionId, text | at-least-once | `contract: worker-question-asked` |
| `assumptions-written` | use case that emits it (§3) | integration | ideaId, featureId? | at-least-once | `contract: assumptions-written` |
| `direct-idea-accepted` | use case that emits it (§3) | integration | ideaId, ticketFile, featureId? | at-least-once | `contract: direct-idea-accepted` |
| `plan-committed` | use case that emits it (§3) | task-execution | ideaId, taskIds[] | at-least-once | `contract: plan-committed` |
| `plan-refused` | use case that emits it (§3) | task-execution | ideaId, taskId?, reasons[] | at-least-once | `contract: plan-refused` |
| `task-replanned` | use case that emits it (§3) | task-execution | taskId, planChange | at-least-once | `contract: task-replanned` |
| `question-answered` | use case that emits it (§3) | task-execution | questionId, taskId | at-least-once | `contract: question-answered` |
| `own-assumption-recorded` | use case that emits it (§3) | task-execution | questionId, taskId | at-least-once | `contract: own-assumption-recorded` |
| `idea-tasks-landed` | use case that emits it (§3) | integration | ideaId, featureId | at-least-once | `contract: idea-tasks-landed` |

### Consumed
| Event | From | Policy (`discover` id) | Handled by | Handler → command | Delivery | Idempotency |
|---|---|---|---|---|---|---|
| `feature-opened` | integration | policy (see canvas) | policy | reconcile pass handler | at-least-once | the reconcile pass is level-triggered, so a reaction can run again; ideas requests approval once per (ideaId, assumptionsHash) - test `idempotent: feature-opened` |
| `replan-requested` | task-execution | policy (see canvas) | policy | reconcile pass handler | at-least-once | the reconcile pass is level-triggered, so a reaction can run again; 1 replan per task - test `idempotent: replan-requested` |
| `task-branch-landed` | integration | policy (see canvas) | policy | reconcile pass handler | at-least-once | the reconcile pass is level-triggered, so a reaction can run again; remove-worktree and roll-up-idea are no-ops when already done - test `idempotent: task-branch-landed` |
| `feature-merged` | integration | policy (see canvas) | policy | reconcile pass handler | at-least-once | the reconcile pass is level-triggered, so a reaction can run again; close-idea is a no-op for a merged idea - test `idempotent: feature-merged` |

## 7. Read models
| Read model | Source events | Readers | Storage | Rebuild |
|---|---|---|---|---|
| `IdeaBoard` | idea-captured, assumptions-written, approval-requested, assumptions-approved, idea-parked, idea-cancelled, plan-committed, idea-tasks-landed, idea-merged | operator and agents through horch CLI read-only commands | ledger tables and files of this context | derived again on each pass from the ledger |
| `InterviewRecord` | interview-question-asked, interview-answer-recorded, idea-amended | operator and agents through horch CLI read-only commands | ledger tables and files of this context | derived again on each pass from the ledger |
| `InFlightDiffs` | plan-committed, idea-tasks-landed | operator and agents through horch CLI read-only commands | ledger tables and files of this context | derived again on each pass from the ledger |
| `ApprovedAssumptionsAndSpec` | assumptions-approved, assumptions-revised, spec-written, own-assumption-recorded | operator and agents through horch CLI read-only commands | ledger tables and files of this context | derived again on each pass from the ledger |

## 8. Persistence
Tables ideas, requirements, questions and the sub-orchestrator rows of attempts (task_id null, assumption A6). Idea-folder files are ingested once per (path, sha256) through the ingested table. approval.json is written only by fleetd, after the approve request arrives on the command channel from a caller outside every fleet attempt (I7).

## 9. Tests
| Kind | Name | Verifies |
|---|---|---|
| invariant | `i3_uncovered_requirement_is_reported_and_plan_refused` | I3: every requirement of an idea in in_progress or later is covered by at least 1 task that is not cancelled |
| invariant | `i7_changed_assumptions_after_approval_hold_the_idea` | I7: no idea is past awaiting_approval unless approved_sha256 matches the current assumptions.md |
| fault injection | `fm32_forged_approval_is_ignored` | I7: no idea is past awaiting_approval unless approved_sha256 matches the current assumptions.md |
| invariant | `i7_missing_sub_orchestrator_is_started_extra_is_stopped` | I7: every idea from clarifying to integrating has exactly 1 live sub-orchestrator attempt or waits on the operator |
| invariant | `i13_open_feature_without_sub_orchestrator_gets_one` | I13: a sub-orchestrator exists for each feature that is not merged and not abandoned |
| invariant | `approve_refused_inside_a_fleet_attempt` | horch idea approve and --direct refuse inside a fleet attempt (FR-10) |
| invariant | `invalid_plan_commits_no_task` | A plan is committed all-or-nothing after every validator check passes (FR-3, FR-4) |
| invariant | `plan_without_paired_judge_is_refused` | A judgment-kind task without a paired judge refuses the plan |
| fault injection | `fm29_idea_waiting_7_days_is_parked` | An idea waiting 7 days goes to parked with a needs-you note (FM-29) |
| fault injection | `fm35_unanswered_question_escalates` | A worker question is re-driven until answered and escalates past its limit (RR-12, FM-35) |
| use case | `given_ticket_file_when_add_direct_then_ticketing_with_1_task_and_its_judge` | use case: add-direct-idea |
| contract | `published approval-requested carries exactly ideaId, assumptionsHash (connect.json payload) and nothing consumers did not ask for` | contract: approval-requested |
| contract | `published worker-question-asked carries exactly taskId, questionId, text (connect.json payload) and nothing consumers did not ask for` | contract: worker-question-asked |
| contract | `published assumptions-written carries exactly ideaId, featureId? (connect.json payload) and nothing consumers did not ask for` | contract: assumptions-written |
| contract | `published direct-idea-accepted carries exactly ideaId, ticketFile, featureId? (connect.json payload) and nothing consumers did not ask for` | contract: direct-idea-accepted |
| contract | `published plan-committed carries exactly ideaId, taskIds[] (connect.json payload) and nothing consumers did not ask for` | contract: plan-committed |
| contract | `published plan-refused carries exactly ideaId, taskId?, reasons[] (connect.json payload) and nothing consumers did not ask for` | contract: plan-refused |
| contract | `published task-replanned carries exactly taskId, planChange (connect.json payload) and nothing consumers did not ask for` | contract: task-replanned |
| contract | `published question-answered carries exactly questionId, taskId (connect.json payload) and nothing consumers did not ask for` | contract: question-answered |
| contract | `published own-assumption-recorded carries exactly questionId, taskId (connect.json payload) and nothing consumers did not ask for` | contract: own-assumption-recorded |
| contract | `published idea-tasks-landed carries exactly ideaId, featureId (connect.json payload) and nothing consumers did not ask for` | contract: idea-tasks-landed |
| idempotency | `handling feature-opened twice (same message id) has the same effect as once` | idempotent: feature-opened |
| idempotency | `handling replan-requested twice (same message id) has the same effect as once` | idempotent: replan-requested |
| idempotency | `handling task-branch-landed twice (same message id) has the same effect as once` | idempotent: task-branch-landed |
| idempotency | `handling feature-merged twice (same message id) has the same effect as once` | idempotent: feature-merged |
| fitness | `fitness_domain_imports_nothing_outward` | §1 |

## 10. Integration & anti-corruption
Talks to worker-profiles and fleet-runtime only through LaunchEnvironmentGateway and AgentProcessGateway to start the sub-orchestrator pane (connect N3). Sub-orchestrators reach this context only as files in the idea folder; the operator reaches it through horch idea commands on the command channel.

## 11. Decisions, assumptions, open points
- Decisions D1 (module root) and D4 (only the verifier sets done) apply to every context; D2 applies to task-execution; D3 (wrapper owner) is open.
- Assumptions: A1, A2, A5, A7, A9, A6 · Open questions: Q1 to Q6 in code.json where they name this context.
