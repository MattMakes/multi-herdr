# Design - Ideas (`ideas`)

| | |
|---|---|
| Classification | supporting (cost-reducer) · sourcing build · pattern **domain-model** (plan validation module only; intake and questions active-record, A12) |
| Deployable | `horch` (modular-monolith) · team `core-team` · data store own (the ledger, single writer fleetd) |
| Module path | `crates/horch-core/src/fleetd/ideas` |
| Structure | hexagonal for the Plan aggregate; records + use cases (active record) for intake and questions |
| Purpose | Ideas takes each rough idea of the operator from capture to 1 committed, validated plan, and closes the idea when its feature is merged (G3). It decides when an idea may move to its next state, holds the operator approval gate, and answers worker questions against the approved assumptions, so that no AI plans 2 ideas at once and no task exists outside an approved plan (FR-2, FR-10, FR-12). It does not run tasks (task-execution), land branches or open the feature (integration), or start processes (worker-profiles). It owns the 1 needs-you list, where everything that waits for the operator is shown. - canvas: `ddd/07-define/ideas/bounded-context-canvas.md` |
| Domain roles | specification-model, approver |

## 1. Module layout
```
crates/horch-core/src/fleetd/ideas/
  mod.rs               public contract of the context (driving ports and published event types only)
  domain/
    idea.rs            Idea record and its state table
    question.rs        Question record
    plan.rs            Plan aggregate: the plan validation module
    plan_validator.rs  PlanValidator domain service (pure)
    needs_you.rs       NeedsYou item: stable key (kind and row), state open, resolved or superseded
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
| `plan` | `Plan` | 7 | 3 commands → 4 events | `aggregate-canvas-plan.md` |

Value objects: DeclaredGates, JudgePairing, PlanContentHash, PlanId, RequirementCoverage, TaskDag · Entities: PlannedTask · Domain services: PlanValidator

## 3. Application services (use cases)
| Use case | Command (`discover` id) | Trigger and rule | Loads | Decides / enforces |
|---|---|---|---|---|
| `AddIdea` | `add-idea` | horch idea add "..." or -f notes.md, optional --to <feature>. A policy also issues it for an adapter drift that conformance finds. An idea of a feature waits in captured while another idea of its feature is in clarifying, specifying or ticketing (A28). | store rows | see §2 |
| `StartSubOrchestrator` | `start-sub-orchestrator` | fleetd starts 1 fresh process per phase in the pane of the feature, and restarts a dead or missing one on the next pass. Asks LaunchEnvironmentGateway, then AgentProcessGateway, after commit (connect N3). | store rows | see §2 |
| `AskOperator` | `ask-operator` | 1 question at a time: what done looks like, scope edges, constraints, affected areas, priority. | store rows | see §2 |
| `AnswerInterviewQuestion` | `answer-interview-question` | The operator answers in the pane. horch idea note records it. | store rows | see §2 |
| `AmendIdea` | `amend-idea` | horch idea amend <id> "...". | store rows | see §2 |
| `WriteAssumptions` | `write-assumptions` | Writes assumptions.md: [confirmed] and [assumed] items, none [open]. | store rows | see §2 |
| `RequestApproval` | `request-approval` | Moves the idea to awaiting_approval and notifies the operator. Reminds once an hour in working hours. | store rows | see §2 |
| `ParkIdea` | `park-idea` | Issued by policy after 7 days of waiting or an escalation. The operator also runs horch idea park. | store rows | see §2 |
| `CancelIdea` | `cancel-idea` | horch idea cancel, an operator request with request_id and the operator token. A policy also issues it when the feature of the idea is abandoned. Publishes idea-cancelled, and task-execution cancels each task of the idea that is not terminal (A34). | store rows | see §2 |
| `ReviseAssumptions` | `revise-assumptions` | horch idea revise <id> "...". | store rows | see §2 |
| `ApproveAssumptions` | `approve-assumptions` | horch idea approve <id>. Refused inside any fleet attempt; profiles deny it. fleetd writes approval.json with the sha256 the operator saw; any other approval file is ignored (I7). | store rows | see §2 |
| `AddDirectIdea` | `add-direct-idea` | horch idea add --direct <ticket file>, optional --to. Refused inside a fleet attempt. | store rows | see §2 |
| `WriteSpec` | `write-spec` | Writes spec.md and requirements.yaml after approval. | store rows | see §2 |
| `SubmitPlan` | `submit-plan` | Writes tickets and plan.yaml. | `Plan` by id | see §2 |
| `ValidatePlan` | `validate-plan` | The plan validator: paired judges, declared gates, model guide, overlapping paths. Commits tasks in 1 transaction. | `Plan` by id | see §2 |
| `AskQuestion` | `ask-question` | Writes 1 question file, commits work in progress, exits. | store rows | see §2 |
| `AnswerQuestion` | `answer-question` | fleetd starts the owner on each pass until an answer is recorded. 4 cases (design, Ideas, How a worker's question is resolved). The sub-orchestrator answers with horch idea note, a file of class worker in its idea folder; it never uses the command channel (A35). | store rows | see §2 |
| `EscalateQuestion` | `escalate-question` | Fires when the time limit of the question row passes. | store rows | see §2 |
| `ReplanTask` | `replan-task` | Once per task. | `Plan` by id | see §2 |
| `RollUpIdea` | `roll-up-idea` | in_progress to integrating. | store rows | see §2 |
| `CloseIdea` | `close-idea` | Needs every worktree of the idea removed. | store rows | see §2 |

Transaction: 1 aggregate (or 1 store call set) per use case through the one-door transition function; the event row is written in the same transaction; process starts, git calls and check runs happen after the commit (connect N4).

## 4. Ports
### Driving
| Port | Operations | Called by |
|---|---|---|
| `IdeasCommands` | one operation per use case; called by the command channel (operator requests of horch idea ..., each with request_id and the operator token), the reconcile pass and the idea folder ingester | CommandChannelHandler and ReconcilePassDriver |
| `IdeasQueries` | read models IdeaBoard, InterviewRecord, InFlightDiffs, ApprovedAssumptionsAndSpec |  |
| `IdeasEventHandlers` | policies on feature-opened, replan-requested, task-branch-landed, feature-merged, task-escalated (onto the needs-you list) and agent-status-changed (sub-orchestrator pane state); also feature-abandoned (cancel its ideas), task-branch-repair-escalated, job-abandoned and adapter-conformance-failed (onto the needs-you list, or a new idea for a drift) | InProcessReconcileConsumer |
| `IdeaFolderIngest` | assumptions.md, spec.md, requirements.yaml, plan.yaml, question files and the answer notes of horch idea note; 1 ingestion per (path, sha256); a file of class worker is read only as a regular file, not through a link, under its size limit (SR-6) | IdeaFolderIngester |

### Driven
| Port | Operations | Why it exists |
|---|---|---|
| `IdeasStore` | active-record rows of idea intake and questions: ideas, requirements, questions and sub-orchestrator attempt rows (task_id null) | persistence, collaborator or infrastructure need named in the operations |
| `EventPublisher` | writes the event row in the same ledger transaction | persistence, collaborator or infrastructure need named in the operations |
| `LaunchEnvironmentGateway` | asks worker-profiles for the idea-lead launch environment | persistence, collaborator or infrastructure need named in the operations |
| `AgentProcessGateway` | sends start-agent-process to worker-profiles to start or stop the sub-orchestrator pane; never inside a ledger transaction | persistence, collaborator or infrastructure need named in the operations |
| `NeedsYouInbox` | the 1 needs-you list (OR-3): approval requests, parked ideas, escalated questions, escalated tasks, escalated task branches, abandoned jobs, illegal transitions; each item has a stable key (its kind and its row) and a state open, resolved or superseded, and a producer writes by key (A28); herdr notification | persistence, collaborator or infrastructure need named in the operations |
| `AttemptContext` | tells whether the caller runs inside a fleet attempt, so approve and --direct refuse there | persistence, collaborator or infrastructure need named in the operations |
| `Clock` | deadlines on the fleet clock: the 30 min idle stop and the question time limit; calendar time: the 7 day park and the hourly reminder (PL-4, A25) | persistence, collaborator or infrastructure need named in the operations |
| `PlanRepository` | load and save Plan with its PlannedTasks by idea id and content hash | persistence, collaborator or infrastructure need named in the operations |
| `IdeaFolderWriter` | writes plan.verdict.json (plan-verdict-written) and approval.json (assumptions-approved) into the idea folder, atomic; fleetd is their only writer | persistence, collaborator or infrastructure need named in the operations |

## 5. Adapters
| Port | Implementation | Technology | Notes |
|---|---|---|---|
| `IdeasCommands` | `CommandChannelHandler and ReconcilePassDriver` | production | - |
| `IdeasEventHandlers` | `InProcessReconcileConsumer` | production | - |
| `IdeaFolderIngest` | `IdeaFolderIngester` | production | - |
| `IdeasStore` | `SqliteIdeasStore` | production | - |
| `IdeasStore` | `InMemoryIdeasStore` | in-memory fake | - |
| `EventPublisher` | `LedgerEventLog` | production | - |
| `EventPublisher` | `RecordingEventPublisher` | in-memory fake | - |
| `LaunchEnvironmentGateway` | `InProcessWorkerProfiles` | production | - |
| `LaunchEnvironmentGateway` | `FakeLaunchEnvironment` | in-memory fake | - |
| `AgentProcessGateway` | `InProcessWorkerProfiles` | production | - |
| `AgentProcessGateway` | `FakeAgentProcesses` | in-memory fake | - |
| `NeedsYouInbox` | `NeedsYouFolder` | production | - |
| `NeedsYouInbox` | `InMemoryNeedsYou` | in-memory fake | - |
| `AttemptContext` | `EnvAttemptContext` | production | - |
| `AttemptContext` | `FakeAttemptContext` | in-memory fake | - |
| `Clock` | `LedgerFleetClock` | production | - |
| `Clock` | `FakeClock` | in-memory fake | - |
| `PlanRepository` | `SqlitePlanRepository` | production | - |
| `PlanRepository` | `InMemoryPlanRepository` | in-memory fake | - |
| `IdeaFolderWriter` | `FsIdeaFolderWriter` | production | ProtectedFiles of fleet-runtime |
| `IdeaFolderWriter` | `InMemoryIdeaFolder` | in-memory fake | - |

## 6. Domain events
### Published
| Event (`discover` id) | Produced by | Consumers | Contract payload (`connect.json`) | Delivery | Contract test |
|---|---|---|---|---|---|
| `approval-requested` | use case that emits it (§3) | operator | idea_id, assumptions_hash | at-least-once | `contract: approval-requested` |
| `worker-question-asked` | use case that emits it (§3) | sub-orchestrator | task_id, question_id, text | at-least-once | `contract: worker-question-asked` |
| `assumptions-written` | use case that emits it (§3) | integration | idea_id, feature_id? | at-least-once | `contract: assumptions-written` |
| `direct-idea-accepted` | use case that emits it (§3) | integration | idea_id, ticket_file, feature_id? | at-least-once | `contract: direct-idea-accepted` |
| `plan-committed` | use case that emits it (§3) | task-execution | idea_id, task_ids[] | at-least-once | `contract: plan-committed` |
| `plan-refused` | use case that emits it (§3) | task-execution | idea_id, task_id?, reasons[] | at-least-once | `contract: plan-refused` |
| `task-replanned` | use case that emits it (§3) | task-execution | task_id, plan_change | at-least-once | `contract: task-replanned` |
| `question-answered` | use case that emits it (§3) | task-execution | question_id, task_id | at-least-once | `contract: question-answered` |
| `own-assumption-recorded` | use case that emits it (§3) | task-execution | question_id, task_id | at-least-once | `contract: own-assumption-recorded` |
| `idea-tasks-landed` | use case that emits it (§3) | integration | idea_id, feature_id | at-least-once | `contract: idea-tasks-landed` |
| `plan-verdict-written` | use case that emits it (§3) | sub-orchestrator | idea, decision (committed\|refused), reasons[] | at-least-once | `contract: plan-verdict-written` |
| `assumptions-approved` | use case that emits it (§3) | sub-orchestrator | idea, assumptions_hash | at-least-once | `contract: assumptions-approved` |
| `idea-cancelled` | use case that emits it (§3) | task-execution | idea_id, cause | at-least-once | `contract: idea-cancelled` |

### Consumed
| Event | From | Policy (`discover` id) | Handled by | Handler → command | Delivery | Idempotency |
|---|---|---|---|---|---|---|
| `task-escalated` | task-execution | policy (see canvas) | policy | reconcile pass handler | at-least-once | the reconcile pass is level-triggered, so a reaction can run again; 1 needs-you file per (taskId, reason); integration records it once per taskId - test `idempotent: task-escalated` |
| `feature-opened` | integration | policy (see canvas) | policy | reconcile pass handler | at-least-once | the reconcile pass is level-triggered, so a reaction can run again; ideas requests approval once per (ideaId, assumptionsHash) - test `idempotent: feature-opened` |
| `replan-requested` | task-execution | policy (see canvas) | policy | reconcile pass handler | at-least-once | the reconcile pass is level-triggered, so a reaction can run again; 1 replan per task - test `idempotent: replan-requested` |
| `task-branch-landed` | integration | policy (see canvas) | policy | reconcile pass handler | at-least-once | the reconcile pass is level-triggered, so a reaction can run again; remove-worktree and roll-up-idea are no-ops when already done - test `idempotent: task-branch-landed` |
| `feature-merged` | integration | policy (see canvas) | policy | reconcile pass handler | at-least-once | the reconcile pass is level-triggered, so a reaction can run again; close-idea is a no-op for a merged idea - test `idempotent: feature-merged` |
| `agent-status-changed` | herdr | policy (see canvas) | policy | reconcile pass handler | at-most-once | edge events can be lost by design; each pass re-reads session.snapshot, so a repeat or a loss changes nothing - test `idempotent: agent-status-changed` |
| `task-branch-repair-escalated` | integration | policy (see canvas) | policy | reconcile pass handler | at-least-once | 1 needs-you item per branch, written by key (design: The needs-you list has a life cycle) - test `idempotent: task-branch-repair-escalated` |
| `feature-abandoned` | integration | policy (see canvas) | policy | reconcile pass handler | at-least-once | the reconcile pass is level-triggered, so a reaction can run again; cancel-idea is a no-op for an idea that is merged or cancelled - test `idempotent: feature-abandoned` |
| `job-abandoned` | fleet-runtime | policy (see canvas) | policy | reconcile pass handler | at-least-once | the reconcile pass is level-triggered, so a reaction can run again; the owner context reads the job row; 1 needs-you item per job key - test `idempotent: job-abandoned` |
| `adapter-conformance-failed` | worker-profiles | policy (see canvas) | policy | reconcile pass handler | at-least-once | the reconcile pass is level-triggered, so a reaction can run again; 1 idea per (cli, version) drift - test `idempotent: adapter-conformance-failed` |

## 7. Read models
| Read model | Source events | Readers | Storage | Rebuild |
|---|---|---|---|---|
| `IdeaBoard` | idea-captured, assumptions-written, approval-requested, assumptions-approved, idea-parked, idea-cancelled, plan-committed, idea-tasks-landed, idea-merged | operator and agents through horch CLI read-only commands | ledger tables and files of this context | derived again on each pass from the ledger |
| `InterviewRecord` | interview-question-asked, interview-answer-recorded, idea-amended | operator and agents through horch CLI read-only commands | ledger tables and files of this context | derived again on each pass from the ledger |
| `InFlightDiffs` | plan-committed, idea-tasks-landed | operator and agents through horch CLI read-only commands | ledger tables and files of this context | derived again on each pass from the ledger |
| `ApprovedAssumptionsAndSpec` | assumptions-approved, assumptions-revised, spec-written, own-assumption-recorded | operator and agents through horch CLI read-only commands | ledger tables and files of this context | derived again on each pass from the ledger |
| `NeedsYouList` | approval-requested, idea-parked, question-escalated, task-escalated, task-requeued, task-branch-repair-escalated, job-abandoned | operator and agents through horch CLI read-only commands | ledger tables and files of this context | derived again on each pass from the ledger |

## 8. Persistence
Tables ideas, requirements, questions and the sub-orchestrator rows of attempts (task_id null, assumption A6). Idea-folder files are ingested once per (path, sha256) through the ingested table. approval.json is written only by fleetd, after the approve request arrives on the command channel with the operator token from a caller outside every fleet attempt (I7, SR-8). The needs_you table holds 1 row per key (kind and row) with its state open, resolved or superseded; a producer writes by key (A28).

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
| contract | `published approval-requested carries exactly idea_id, assumptions_hash (connect.json payload) and nothing consumers did not ask for` | contract: approval-requested |
| contract | `published worker-question-asked carries exactly task_id, question_id, text (connect.json payload) and nothing consumers did not ask for` | contract: worker-question-asked |
| contract | `published assumptions-written carries exactly idea_id, feature_id? (connect.json payload) and nothing consumers did not ask for` | contract: assumptions-written |
| contract | `published direct-idea-accepted carries exactly idea_id, ticket_file, feature_id? (connect.json payload) and nothing consumers did not ask for` | contract: direct-idea-accepted |
| contract | `published plan-committed carries exactly idea_id, task_ids[] (connect.json payload) and nothing consumers did not ask for` | contract: plan-committed |
| contract | `published plan-refused carries exactly idea_id, task_id?, reasons[] (connect.json payload) and nothing consumers did not ask for` | contract: plan-refused |
| contract | `published task-replanned carries exactly task_id, plan_change (connect.json payload) and nothing consumers did not ask for` | contract: task-replanned |
| contract | `published question-answered carries exactly question_id, task_id (connect.json payload) and nothing consumers did not ask for` | contract: question-answered |
| contract | `published own-assumption-recorded carries exactly question_id, task_id (connect.json payload) and nothing consumers did not ask for` | contract: own-assumption-recorded |
| contract | `published idea-tasks-landed carries exactly idea_id, feature_id (connect.json payload) and nothing consumers did not ask for` | contract: idea-tasks-landed |
| idempotency | `handling feature-opened twice (same message id) has the same effect as once` | idempotent: feature-opened |
| idempotency | `handling replan-requested twice (same message id) has the same effect as once` | idempotent: replan-requested |
| idempotency | `handling task-branch-landed twice (same message id) has the same effect as once` | idempotent: task-branch-landed |
| idempotency | `handling feature-merged twice (same message id) has the same effect as once` | idempotent: feature-merged |
| invariant | `plan_with_a_dependency_cycle_is_refused` | The task dependencies of a plan form a graph with no cycle (FR-3) |
| invariant | `plan_without_declared_fast_gate_is_refused` | A plan of a project with no declared fast gate is refused |
| invariant | `second_replan_of_a_task_is_refused` | A task is replanned at most once |
| transition | `transition_refused_to_submitted_needs_a_new_content_hash` | transition: refused -> submitted |
| invariant | `question_escalates_after_15_min_and_operator_wait_has_no_limit` | A worker question with no answer from a running sub-orchestrator escalates to the operator after 15 min; the wait for the operator has no limit (orchestrator, D15) |
| invariant | `sub_orchestrator_answer_arrives_as_an_idea_note_file` | The sub-orchestrator answers a worker question with horch idea note, a file of class worker; it never uses the command channel, and fleetd stays the only ledger writer (A35, connect N9) |
| invariant | `escalated_task_appears_on_the_1_needs_you_list` | There is 1 needs-you list and ideas owns it (OR-3, decompose N9) |
| contract | `published plan-verdict-written carries exactly idea, decision (committed\|refused), reasons[] (connect.json payload) and nothing consumers did not ask for` | contract: plan-verdict-written |
| contract | `published assumptions-approved carries exactly idea, assumptions_hash (connect.json payload) and nothing consumers did not ask for` | contract: assumptions-approved |
| idempotency | `handling task-escalated twice (same task_id, reason) has the same effect as once` | idempotent: task-escalated |
| idempotency | `handling agent-status-changed twice (same pane, agent_status; each pass re-reads session.snapshot) has the same effect as once` | idempotent: agent-status-changed |
| contract | `published idea-cancelled carries exactly idea_id, cause (connect.json payload) and nothing consumers did not ask for` | contract: idea-cancelled |
| idempotency | `handling task-branch-repair-escalated twice (same branch; 1 needs-you item per branch by key) has the same effect as once` | idempotent: task-branch-repair-escalated |
| idempotency | `handling feature-abandoned twice (same feature_id; cancel-idea is a no-op for a merged or cancelled idea) has the same effect as once` | idempotent: feature-abandoned |
| idempotency | `handling job-abandoned twice (same job key; 1 needs-you item per job key) has the same effect as once` | idempotent: job-abandoned |
| idempotency | `handling adapter-conformance-failed twice (same cli, version; 1 idea per drift) has the same effect as once` | idempotent: adapter-conformance-failed |
| invariant | `replan_that_leaves_a_judgment_task_without_judge_is_refused` | A plan change that would leave a judgment task with no judge task is refused (A34) |
| invariant | `needs_you_item_written_twice_by_key_is_1_item_and_resolves_with_its_row` | The needs-you list has a life cycle: 1 item per key, open, resolved or superseded (A28) |
| invariant | `second_idea_of_a_feature_waits_in_captured_until_the_first_leaves_ticketing` | The ideas of 1 feature are planned in order: a captured idea waits while another idea of its feature is in clarifying, specifying or ticketing (A28) |
| invariant | `i7_two_ideas_of_1_feature_share_1_live_sub_orchestrator` | I7: each feature that has an idea from clarifying to integrating has exactly 1 live sub-orchestrator attempt, or waits for the operator (A28) |
| invariant | `abandoned_feature_cancels_each_idea_that_is_not_merged` | Whenever a feature is abandoned, each idea of the feature that is not merged goes to cancelled (A28) |
| fitness | `fitness_domain_imports_nothing_outward` | §1 |

## 10. Integration & anti-corruption
Talks to worker-profiles only through LaunchEnvironmentGateway and AgentProcessGateway to start the sub-orchestrator pane (connect N3, D3). A sub-orchestrator answers a worker question with horch idea note, a file of class worker in its idea folder; it never uses the command channel, and fleetd stays the only ledger writer (A35, A18). 1 live sub-orchestrator runs for each feature, and the ideas of 1 feature are planned in order (I7, I13, A28). A cancelled idea sends idea-cancelled to task-execution; an abandoned feature cancels each idea that is not merged. An adapter drift (adapter-conformance-failed) becomes a new idea. task-branch-repair-escalated and job-abandoned put 1 item on the needs-you list. The herdr agent status arrives as agent-status-changed. Sub-orchestrators reach this context only as files in the idea folder; the operator reaches it through horch idea commands on the command channel, each with request_id and the operator token.

## 11. Decisions, assumptions, open points
- Decisions D1 (module root) and D4 (only the verifier sets done) apply to every context; D2 applies to task-execution; D3 (wrapper owner) is option B, worker-profiles (orchestrator, D15).
- Assumptions: A1, A2, A5, A7, A9, A6, A12, A14, A18 · Open questions: none (Q3 is answered, see deprecated[] in code.json).
