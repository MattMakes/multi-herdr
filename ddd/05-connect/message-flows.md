# Message flows — fleetd: the horch fleet control plane

## In plain words

We drew how the 6 parts of fleetd pass work to each other, in 9 step-by-step flows and 51 named messages.

**Decided:** All 6 parts run in 1 program, so they talk by in-memory messages that a 5-second loop re-checks. Only the operator's commands, the agents' files, herdr, git, GitHub and process starts leave the program. worker-profiles starts every agent process.

**Assumed:** We assumed a lost in-memory message is harmless because the loop re-reads the database every 5 seconds and redoes missing work.

**Riskiest:** If a part reacts twice to the same message and is not built to ignore the repeat, it can launch a worker twice or open 2 features for 1 idea.

---

_Step 5 of the DDD chain (connect) · produced 2026-11-16T21:49:38Z · mode auto · depth standard · inputs: ddd/03-decompose/decompose.json, ddd/04-strategize/strategize.json, ddd/02-discover/discover.json, ddd/manifest.json, ddd/01-understand/understand.json, ddd/glossary.md, docs/specs/fleetd/design.md_

Rendered from `connect.json` by `ddd connect render`. The JSON is the chain (contract §7): edit it and re-render rather than editing tables here.

**Scale target:** 1–1 deployables, 1 team(s) — fleetd is a subcommand of the single horch binary. The operator and the fleet of AI agents are 1 team.. Mechanisms below assume the distances recorded per relationship; `ddd-organise` confirms or flips them.

## How to read the diagrams

| Arrow | Meaning |
|---|---|
| `->>` solid, filled head | synchronous command or query — the sender waits for the answer |
| `-)` solid, open head | asynchronous message — an event published (or a command queued); sender continues |
| `-->>` dotted | response to a query or command |
| `opt …` | steps that only happen under that condition |

Labels read `message-id (kind via mechanism)`. `via in-process` means both parties live in the same deployable (in-memory event or direct call); `message-bus` means an outbox-published event; `http`/`grpc` a network call.

## Flows

### F1 — Capture an idea and open its feature (scenario S1: Happy path: 1 rough idea becomes 1 merged feature)

S1 part 1, up to the approval request. The operator adds an idea, fleetd starts a sub-orchestrator pane for the feature, the sub-orchestrator writes assumptions.md, integration opens the feature branch and ideas asks the operator to approve. The direct ticket path (horch idea add --direct) skips the interview and opens the feature at once.

Policies realised: start-lead-on-capture, open-feature-on-assumptions, request-approval-on-feature, open-feature-on-direct

```mermaid
sequenceDiagram
  autonumber
  actor operator as Operator
  participant ideas as Ideas
  participant worker_profiles as Worker profiles and adapters
  actor sub_orchestrator as Sub-orchestrator
  participant integration as Landing and integration
  operator->>ideas: add-idea (command via other)
  Note over operator,ideas: horch CLI request of class operator over the fleetd command channel, with the operator token#59; a request file with the same request_id when fleetd is down (A24, A35)
  ideas-->>operator: add-idea (response via other)
  ideas->>worker_profiles: start-agent-process (command via in-process)
  Note over ideas,worker_profiles: sub-orchestrator CLI in a pane that ideas gets from the herdr adapter#59; worker-profiles builds the env
  sub_orchestrator-)ideas: write-assumptions (command via file)
  Note over sub_orchestrator,ideas: assumptions.md after the interview
  ideas-)integration: assumptions-written (event via in-process)
  integration-)ideas: feature-opened (event via in-process)
  ideas-)operator: approval-requested (event via file)
  Note over ideas,operator: needs-you/ file
  opt direct ticket path
    operator->>ideas: add-direct-idea (command via other)
    ideas-)integration: direct-idea-accepted (event via in-process)
  end
```

| # | From | To | Message | Kind | Sync | Via |
|---|---|---|---|---|---|---|
| 1 | operator | ideas | `add-idea` | command | yes | other |
| 2 | ideas | operator | `add-idea` | response | yes | other |
| 3 | ideas | worker-profiles | `start-agent-process` | command | yes | in-process |
| 4 | sub-orchestrator | ideas | `write-assumptions` | command | no | file |
| 5 | ideas | integration | `assumptions-written` | event | no | in-process |
| 6 | integration | ideas | `feature-opened` | event | no | in-process |
| 7 | ideas | operator | `approval-requested` | event | no | file |
| 8 | operator | ideas | `add-direct-idea` | command | yes | other |
| 9 | ideas | integration | `direct-idea-accepted` | event | no | in-process |

### F2 — Approve and commit the plan (scenario S1: Happy path: 1 rough idea becomes 1 merged feature)

S1 part 2, from approval to a committed plan. The operator approves, fleetd writes approval.json, the sub-orchestrator writes the spec and plan.yaml, plan validation writes plan.verdict.json and commits the plan. F8 continues with the lease and the launch.

Policies realised: promote-on-plan-committed

```mermaid
sequenceDiagram
  autonumber
  actor operator as Operator
  participant ideas as Ideas
  actor sub_orchestrator as Sub-orchestrator
  participant task_execution as Task execution
  operator->>ideas: approve-assumptions (command via other)
  Note over operator,ideas: horch CLI request of class operator over the fleetd command channel, with the operator token#59; a request file with the same request_id when fleetd is down (A24, A35)
  ideas-)sub_orchestrator: assumptions-approved (event via file)
  Note over ideas,sub_orchestrator: approval.json with the hash you saw
  sub_orchestrator-)ideas: write-spec (command via file)
  Note over sub_orchestrator,ideas: spec.md and requirements.yaml
  sub_orchestrator-)ideas: submit-plan (command via file)
  Note over sub_orchestrator,ideas: plan.yaml
  ideas-)sub_orchestrator: plan-verdict-written (event via file)
  Note over ideas,sub_orchestrator: plan.verdict.json
  ideas-)task_execution: plan-committed (event via in-process)
```

| # | From | To | Message | Kind | Sync | Via |
|---|---|---|---|---|---|---|
| 1 | operator | ideas | `approve-assumptions` | command | yes | other |
| 2 | ideas | sub-orchestrator | `assumptions-approved` | event | no | file |
| 3 | sub-orchestrator | ideas | `write-spec` | command | no | file |
| 4 | sub-orchestrator | ideas | `submit-plan` | command | no | file |
| 5 | ideas | sub-orchestrator | `plan-verdict-written` | event | no | file |
| 6 | ideas | task-execution | `plan-committed` | event | no | in-process |

### F3 — Verify, land and roll up (scenario S1: Happy path: 1 rough idea becomes 1 merged feature)

S1 part 4, from the worker's result to the idea roll-up. The worker writes result.json, the wrapper writes exit.json, the verifier writes verdict.json and moves the task to done, integration lands the task branch on the feature branch, worktree-custody removes the worktree and ideas rolls the idea up. F9 continues with the pull request.

Policies realised: ready-branch-on-done, after-landing, full-gate-on-landed

```mermaid
sequenceDiagram
  autonumber
  actor worker as Worker
  participant task_execution as Task execution
  participant worker_profiles as Worker profiles and adapters
  actor sub_orchestrator as Sub-orchestrator
  participant integration as Landing and integration
  participant worktree_custody as Worktree custody
  participant ideas as Ideas
  worker-)task_execution: write-result (command via file)
  Note over worker,task_execution: outbox/result.json#59; status completed, blocked or failed
  worker_profiles-)task_execution: attempt-exited (event via file)
  Note over worker_profiles,task_execution: exit.json, written by the fleet-run wrapper
  task_execution-)sub_orchestrator: task-verdict-written (event via file)
  Note over task_execution,sub_orchestrator: verdict.json
  task_execution-)integration: task-done (event via in-process)
  Note over task_execution,integration: only the verifier emits it
  integration-)worktree_custody: task-branch-landed (event via in-process)
  Note over integration,worktree_custody: after the bundle passes the fast gate job and the feature ref moves by compare-and-set from the base tip (A27)
  integration-)ideas: task-branch-landed (event via in-process)
  ideas-)integration: idea-tasks-landed (event via in-process)
```

| # | From | To | Message | Kind | Sync | Via |
|---|---|---|---|---|---|---|
| 1 | worker | task-execution | `write-result` | command | no | file |
| 2 | worker-profiles | task-execution | `attempt-exited` | event | no | file |
| 3 | task-execution | sub-orchestrator | `task-verdict-written` | event | no | file |
| 4 | task-execution | integration | `task-done` | event | no | in-process |
| 5 | integration | worktree-custody | `task-branch-landed` | event | no | in-process |
| 6 | integration | ideas | `task-branch-landed` | event | no | in-process |
| 7 | ideas | integration | `idea-tasks-landed` | event | no | in-process |

### F4 — Usage limit moves the task to its fallback persona (scenario S2: A usage limit moves the task to its fallback persona with no burned attempt)

S2. A usage limit is not a stall and burns no attempt. task-execution asks worker-profiles for the fallback persona's environment and relaunches on the same worktree. The rest is the normal verify path of F3.

Policies realised: fallback-on-usage-limit, launch-on-fallback

```mermaid
sequenceDiagram
  autonumber
  actor provider_pool as Provider pool
  participant task_execution as Task execution
  participant worker_profiles as Worker profiles and adapters
  actor worker as Worker
  participant integration as Landing and integration
  provider_pool-)task_execution: usage-limit-reported (event via other)
  Note over provider_pool,task_execution: reported through the wrapper and the quota probe
  task_execution->>worker_profiles: launch-environment (query via in-process)
  Note over task_execution,worker_profiles: fallback persona on another provider
  worker_profiles-->>task_execution: launch-environment (response via in-process)
  task_execution->>worker_profiles: start-agent-process (command via in-process)
  Note over task_execution,worker_profiles: same worktree, no attempt burned
  worker-)task_execution: write-result (command via file)
  worker_profiles-)task_execution: attempt-exited (event via file)
  task_execution-)integration: task-done (event via in-process)
```

| # | From | To | Message | Kind | Sync | Via |
|---|---|---|---|---|---|---|
| 1 | provider-pool | task-execution | `usage-limit-reported` | event | no | other |
| 2 | task-execution | worker-profiles | `launch-environment` | query | yes | in-process |
| 3 | worker-profiles | task-execution | `launch-environment` | response | yes | in-process |
| 4 | task-execution | worker-profiles | `start-agent-process` | command | yes | in-process |
| 5 | worker | task-execution | `write-result` | command | no | file |
| 6 | worker-profiles | task-execution | `attempt-exited` | event | no | file |
| 7 | task-execution | integration | `task-done` | event | no | in-process |

### F5 — Stalled attempt, worker question and resumed work (scenario S3: A worker stalls, its tree is kept, and the retry continues the same work)

S3. The clock ends a stalled attempt, worktree-custody keeps the tree, a retry starts in the same worktree, the worker asks a question, the sub-orchestrator records its own assumption or answers from approved items through the command channel, and task-execution relaunches the attempt.

Policies realised: resume-on-own-assumption, resume-on-answer

```mermaid
sequenceDiagram
  autonumber
  participant clock as Clock (external)
  participant task_execution as Task execution
  participant worktree_custody as Worktree custody
  participant worker_profiles as Worker profiles and adapters
  actor worker as Worker
  participant ideas as Ideas
  actor sub_orchestrator as Sub-orchestrator
  clock-)task_execution: apply-stall-ladder (command via in-process)
  task_execution-)worktree_custody: attempt-failed (event via in-process)
  Note over task_execution,worktree_custody: the worktree is kept for the retry
  task_execution->>worker_profiles: start-agent-process (command via in-process)
  Note over task_execution,worker_profiles: retry after backoff with a handoff file
  worker-)ideas: ask-question (command via file)
  Note over worker,ideas: outbox/questions/#59; the worker commits and exits
  ideas-)sub_orchestrator: worker-question-asked (event via file)
  Note over ideas,sub_orchestrator: inbox file plus the constant doorbell line
  sub_orchestrator-)ideas: answer-question (command via file)
  Note over sub_orchestrator,ideas: horch idea note writes a file in the idea folder#59; a worker-class request never uses the command channel (A35)
  opt no approved item covers it
    ideas-)task_execution: own-assumption-recorded (event via in-process)
  end
  opt an approved item covers it
    ideas-)task_execution: question-answered (event via in-process)
  end
  task_execution->>worker_profiles: start-agent-process (command via in-process)
```

| # | From | To | Message | Kind | Sync | Via |
|---|---|---|---|---|---|---|
| 1 | clock | task-execution | `apply-stall-ladder` | command | no | in-process |
| 2 | task-execution | worktree-custody | `attempt-failed` | event | no | in-process |
| 3 | task-execution | worker-profiles | `start-agent-process` | command | yes | in-process |
| 4 | worker | ideas | `ask-question` | command | no | file |
| 5 | ideas | sub-orchestrator | `worker-question-asked` | event | no | file |
| 6 | sub-orchestrator | ideas | `answer-question` | command | no | file |
| 7 | ideas | task-execution | `own-assumption-recorded` | event | no | in-process |
| 8 | ideas | task-execution | `question-answered` | event | no | in-process |
| 9 | task-execution | worker-profiles | `start-agent-process` | command | yes | in-process |

### F6 — Replan, refusal and escalation to the operator (no discover scenario — derived flow)

No discover scenario covers this path. It realises the cross-context policies of a task that runs out of attempts: the sub-orchestrator replans once, a refused replan or a failed rescue escalates the task, and the operator can abandon the task branch.

Policies realised: lead-replans-on-request, promote-on-replan, escalate-on-plan-refused, escalate-on-rescue-failed, operator-decides-escalation

```mermaid
sequenceDiagram
  autonumber
  participant task_execution as Task execution
  participant ideas as Ideas
  actor sub_orchestrator as Sub-orchestrator
  participant worktree_custody as Worktree custody
  participant integration as Landing and integration
  actor operator as Operator
  task_execution-)ideas: replan-requested (event via in-process)
  Note over task_execution,ideas: after the last attempt
  sub_orchestrator-)ideas: replan-task (command via file)
  opt the replan validates
    ideas-)task_execution: task-replanned (event via in-process)
  end
  opt the replan is refused
    ideas-)task_execution: plan-refused (event via in-process)
  end
  opt a rescue commit fails
    worktree_custody-)task_execution: rescue-failed (event via in-process)
  end
  task_execution-)ideas: task-escalated (event via in-process)
  Note over task_execution,ideas: ideas puts the task on the 1 needs-you list
  task_execution-)integration: task-escalated (event via in-process)
  operator->>integration: abandon-task-branch (command via other)
  Note over operator,integration: horch CLI request of class operator over the fleetd command channel, with the operator token#59; a request file with the same request_id when fleetd is down (A24, A35)
  integration-->>operator: abandon-task-branch (response via other)
```

| # | From | To | Message | Kind | Sync | Via |
|---|---|---|---|---|---|---|
| 1 | task-execution | ideas | `replan-requested` | event | no | in-process |
| 2 | sub-orchestrator | ideas | `replan-task` | command | no | file |
| 3 | ideas | task-execution | `task-replanned` | event | no | in-process |
| 4 | ideas | task-execution | `plan-refused` | event | no | in-process |
| 5 | worktree-custody | task-execution | `rescue-failed` | event | no | in-process |
| 6 | task-execution | ideas | `task-escalated` | event | no | in-process |
| 7 | task-execution | integration | `task-escalated` | event | no | in-process |
| 8 | operator | integration | `abandon-task-branch` | command | yes | other |
| 9 | integration | operator | `abandon-task-branch` | response | yes | other |

### F7 — Conflicting features are stacked with a resolver task (scenario S4: 2 in-flight features conflict and are stacked with 1 resolver task)

S4. The primary branch moves, integration runs trial merges with git, stacks the younger feature on the older one and opens a resolver task, which task-execution runs like any task. A task branch that conflicts in a bundle goes back to task-execution for a new attempt.

Policies realised: on-primary-moved, relaunch-on-conflict

```mermaid
sequenceDiagram
  autonumber
  participant git as git (external)
  participant integration as Landing and integration
  participant task_execution as Task execution
  participant ideas as Ideas
  actor operator as Operator
  git-)integration: primary-branch-moved (event via other)
  Note over git,integration: the periodic fetch job (fetch_period_s, 60 s) records the remote tip in remote_tips#59; the next pass reads it (A23)
  integration->>git: trial-merge-result (query via other)
  Note over integration,git: git merge-tree as a trial_merge job keyed by the 2 tips, outside the pass#59; the next pass reads its result (A23)
  git-->>integration: trial-merge-result (response via other)
  integration-)task_execution: resolver-task-opened (event via in-process)
  Note over integration,task_execution: a normal task of a code kind
  task_execution-)integration: task-done (event via in-process)
  opt a task branch conflicts in a bundle
    integration-)task_execution: task-branch-conflicted (event via in-process)
  end
  integration-)ideas: task-branch-landed (event via in-process)
  ideas-)integration: idea-tasks-landed (event via in-process)
  integration-)operator: pull-request-opened (event via other)
```

| # | From | To | Message | Kind | Sync | Via |
|---|---|---|---|---|---|---|
| 1 | git | integration | `primary-branch-moved` | event | no | other |
| 2 | integration | git | `trial-merge-result` | query | yes | other |
| 3 | git | integration | `trial-merge-result` | response | yes | other |
| 4 | integration | task-execution | `resolver-task-opened` | event | no | in-process |
| 5 | task-execution | integration | `task-done` | event | no | in-process |
| 6 | integration | task-execution | `task-branch-conflicted` | event | no | in-process |
| 7 | integration | ideas | `task-branch-landed` | event | no | in-process |
| 8 | ideas | integration | `idea-tasks-landed` | event | no | in-process |
| 9 | integration | operator | `pull-request-opened` | event | no | other |

### F8 — Lease, write the task files and launch the first attempt (scenario S1: Happy path: 1 rough idea becomes 1 merged feature)

S1 part 3, from the committed plan (F2) to a running attempt. task-execution leases a task in the same ledger transaction as the worktree lease row, worktree-custody creates the worktree after the commit, task-execution writes task.yaml and START.md, and worker-profiles starts the fleet-run wrapper. The wrapper heartbeat and the herdr agent status then renew the lease. F3 continues at the result.

Policies realised: create-worktree-on-lease, launch-on-worktree, start-process-on-launch

```mermaid
sequenceDiagram
  autonumber
  participant task_execution as Task execution
  participant worktree_custody as Worktree custody
  actor worker as Worker
  participant worker_profiles as Worker profiles and adapters
  participant herdr as herdr (external)
  task_execution-)worktree_custody: task-leased (event via in-process)
  Note over task_execution,worktree_custody: the worktree row commits in reserved in the same ledger transaction#59; git worktree add runs after the commit as a worktree_add job (A32)
  worktree_custody-)task_execution: worktree-created (event via in-process)
  task_execution-)worker: assign-task (command via file)
  Note over task_execution,worker: task.yaml, written before the launch
  task_execution-)worker: start-work (command via file)
  Note over task_execution,worker: inbox/START.md, written before the launch
  task_execution->>worker_profiles: start-agent-process (command via in-process)
  Note over task_execution,worker_profiles: the attempt row commits in launching first#59; the fleet-run wrapper starts in its own process group through the 1 launcher (SR-7)#59; START.md path is the argument#59; ANTHROPIC_API_KEY is never in the env
  worker_profiles-)task_execution: heartbeat-written (event via file)
  Note over worker_profiles,task_execution: heartbeat.json every 10 s
  herdr-)task_execution: agent-status-changed (event via other)
  Note over herdr,task_execution: through the herdr adapter#59; session.snapshot is re-read every pass
```

| # | From | To | Message | Kind | Sync | Via |
|---|---|---|---|---|---|---|
| 1 | task-execution | worktree-custody | `task-leased` | event | no | in-process |
| 2 | worktree-custody | task-execution | `worktree-created` | event | no | in-process |
| 3 | task-execution | worker | `assign-task` | command | no | file |
| 4 | task-execution | worker | `start-work` | command | no | file |
| 5 | task-execution | worker-profiles | `start-agent-process` | command | yes | in-process |
| 6 | worker-profiles | task-execution | `heartbeat-written` | event | no | file |
| 7 | herdr | task-execution | `agent-status-changed` | event | no | other |

### F9 — Open the pull request and close the feature (scenario S1: Happy path: 1 rough idea becomes 1 merged feature)

S1 part 5, from the full gate (F3) to the merged feature. The feature is ready, integration pushes the feature branch as a push job, opens the pull request on GitHub, tells the operator, GitHub reports the merge, and the clock confirms the merge in the primary branch by content before ideas closes the idea.

Policies realised: close-on-feature-merged, pr-on-ready

```mermaid
sequenceDiagram
  autonumber
  participant integration as Landing and integration
  participant git as git (external)
  participant github as GitHub (external)
  actor operator as Operator
  participant clock as Clock (external)
  participant ideas as Ideas
  integration-)git: push-feature-branch (command via other)
  Note over integration,git: a push job after the full gate passes and the feature is ready#59; a push that rewrites uses --force-with-lease on seen_remote_tip#59; a failed run sets push_state failed and the next run waits for push_not_before (A28, A33)
  integration->>github: open-pull-request (command via other)
  Note over integration,github: after the push job passed#59; pr setting gh opens it, manual records the compare URL#59; remote = none opens no pull request
  github-->>integration: open-pull-request (response via other)
  integration-)operator: pull-request-opened (event via other)
  opt pr setting gh
    github-)integration: pull-request-merged (event via other)
    Note over github,integration: read by the fetch job (pr = gh)
  end
  clock-)integration: check-merged (command via in-process)
  Note over clock,integration: reads git: in the primary branch by content
  integration-)ideas: feature-merged (event via in-process)
```

| # | From | To | Message | Kind | Sync | Via |
|---|---|---|---|---|---|---|
| 1 | integration | git | `push-feature-branch` | command | no | other |
| 2 | integration | github | `open-pull-request` | command | yes | other |
| 3 | github | integration | `open-pull-request` | response | yes | other |
| 4 | integration | operator | `pull-request-opened` | event | no | other |
| 5 | github | integration | `pull-request-merged` | event | no | other |
| 6 | clock | integration | `check-merged` | command | no | in-process |
| 7 | integration | ideas | `feature-merged` | event | no | in-process |

### F10 — Cancel an idea, its tasks and their task branches (scenario S5: An idea is cancelled: its tasks are cancelled, and each task branch is rescued and abandoned)

S5. The operator cancels an idea or 1 task. task-execution stops the process group of each live attempt, then cancels each task that is not terminal. integration asks worktree-custody for a rescue ref of each task branch and abandons the branch only after it exists (A34).

Policies realised: cancel-tasks-on-idea-cancelled, abandon-branch-on-task-cancelled

```mermaid
sequenceDiagram
  autonumber
  actor operator as Operator
  participant ideas as Ideas
  participant task_execution as Task execution
  participant integration as Landing and integration
  participant worktree_custody as Worktree custody
  operator->>ideas: cancel-idea (command via other)
  Note over operator,ideas: horch CLI request of class operator over the fleetd command channel, with the operator token#59; a request file with the same request_id when fleetd is down (A24, A35)
  ideas-->>operator: cancel-idea (response via other)
  ideas-)task_execution: idea-cancelled (event via in-process)
  opt the operator cancels 1 task
    operator->>task_execution: cancel-task (command via other)
    task_execution-->>operator: cancel-task (response via other)
  end
  task_execution-)integration: task-cancelled (event via in-process)
  Note over task_execution,integration: after the process group of a live attempt is stopped through the platform layer
  integration-)worktree_custody: rescue-worktree (command via in-process)
  Note over integration,worktree_custody: a rescue job, outside the pass
  worktree_custody-)integration: worktree-rescued (event via in-process)
  Note over worktree_custody,integration: integration then moves the task branch to abandoned
```

| # | From | To | Message | Kind | Sync | Via |
|---|---|---|---|---|---|---|
| 1 | operator | ideas | `cancel-idea` | command | yes | other |
| 2 | ideas | operator | `cancel-idea` | response | yes | other |
| 3 | ideas | task-execution | `idea-cancelled` | event | no | in-process |
| 4 | operator | task-execution | `cancel-task` | command | yes | other |
| 5 | task-execution | operator | `cancel-task` | response | yes | other |
| 6 | task-execution | integration | `task-cancelled` | event | no | in-process |
| 7 | integration | worktree-custody | `rescue-worktree` | command | no | in-process |
| 8 | worktree-custody | integration | `worktree-rescued` | event | no | in-process |

### F11 — A task branch repair reaches its limit and the operator decides (no discover scenario — derived flow)

No discover scenario covers this path. A task branch fails the gate, fleetd starts fix attempts of the done task, and each failure counts in repair_attempts of the branch, not in the attempts of the task. At max_attempts the branch goes on the needs-you list, and the operator requeues the task or abandons the branch (A27, A34). A conflicted branch with its resolver attempts follows the same path.

Policies realised: triage-on-gate-failed, escalate-branch-on-repair-limit, operator-decides-branch

```mermaid
sequenceDiagram
  autonumber
  participant integration as Landing and integration
  participant task_execution as Task execution
  participant worker_profiles as Worker profiles and adapters
  participant ideas as Ideas
  actor operator as Operator
  participant worktree_custody as Worktree custody
  integration-)task_execution: task-branch-gate-failed (event via in-process)
  Note over integration,task_execution: the bisect found the failing branch
  worker_profiles-)task_execution: attempt-exited (event via file)
  Note over worker_profiles,task_execution: exit.json of the fix attempt
  task_execution-)integration: attempt-failed (event via in-process)
  Note over task_execution,integration: the task stays done#59; repair_attempts of the branch goes up by 1
  opt repair_attempts reaches max_attempts
    integration-)ideas: task-branch-repair-escalated (event via in-process)
    Note over integration,ideas: ideas puts 1 item on the needs-you list
    integration-)task_execution: task-branch-repair-escalated (event via in-process)
  end
  opt the operator requeues: repair_attempts goes to 0 and 1 more attempt starts
    operator->>task_execution: requeue-task (command via other)
    Note over operator,task_execution: horch CLI request of class operator over the fleetd command channel, with the operator token#59; a request file with the same request_id when fleetd is down (A24, A35)
  end
  opt the operator abandons the branch
    operator->>integration: abandon-task-branch (command via other)
    Note over operator,integration: horch CLI request of class operator over the fleetd command channel, with the operator token#59; a request file with the same request_id when fleetd is down (A24, A35)
    integration-)worktree_custody: rescue-worktree (command via in-process)
    Note over integration,worktree_custody: a rescue job, outside the pass
    worktree_custody-)integration: worktree-rescued (event via in-process)
    Note over worktree_custody,integration: integration then moves the branch to abandoned and answers the request
  end
```

| # | From | To | Message | Kind | Sync | Via |
|---|---|---|---|---|---|---|
| 1 | integration | task-execution | `task-branch-gate-failed` | event | no | in-process |
| 2 | worker-profiles | task-execution | `attempt-exited` | event | no | file |
| 3 | task-execution | integration | `attempt-failed` | event | no | in-process |
| 4 | integration | ideas | `task-branch-repair-escalated` | event | no | in-process |
| 5 | integration | task-execution | `task-branch-repair-escalated` | event | no | in-process |
| 6 | operator | task-execution | `requeue-task` | command | yes | other |
| 7 | operator | integration | `abandon-task-branch` | command | yes | other |
| 8 | integration | worktree-custody | `rescue-worktree` | command | no | in-process |
| 9 | worktree-custody | integration | `worktree-rescued` | event | no | in-process |

### F12 — A closed pull request and an abandoned feature (no discover scenario — derived flow)

No discover scenario covers this path. The fetch job reads a pull request that is closed and not merged, and 1 item goes on the needs-you list. The operator runs horch feature abandon. integration writes a rescue ref of each branch that is not landed, abandons it, and ideas cancels each idea of the feature that is not merged, which cancels its tasks (A28, A34).

Policies realised: operator-decides-closed-pr, cancel-on-feature-abandoned, cancel-tasks-on-idea-cancelled

```mermaid
sequenceDiagram
  autonumber
  participant github as GitHub (external)
  participant integration as Landing and integration
  actor operator as Operator
  participant worktree_custody as Worktree custody
  participant ideas as Ideas
  participant task_execution as Task execution
  github-)integration: pull-request-closed (event via other)
  Note over github,integration: read by the fetch job (pr = gh)#59; integration raises 1 needs-you item
  operator->>integration: abandon-feature (command via other)
  Note over operator,integration: horch CLI request of class operator over the fleetd command channel, with the operator token#59; a request file with the same request_id when fleetd is down (A24, A35)
  integration-->>operator: abandon-feature (response via other)
  integration-)worktree_custody: rescue-worktree (command via in-process)
  Note over integration,worktree_custody: 1 rescue job for each branch of the feature that is not landed
  worktree_custody-)integration: worktree-rescued (event via in-process)
  Note over worktree_custody,integration: integration moves each such branch to abandoned
  integration-)ideas: feature-abandoned (event via in-process)
  ideas-)task_execution: idea-cancelled (event via in-process)
  Note over ideas,task_execution: for each idea of the feature that is not merged
  task_execution-)integration: task-cancelled (event via in-process)
  Note over task_execution,integration: the branch already has its rescue ref and is abandoned
```

| # | From | To | Message | Kind | Sync | Via |
|---|---|---|---|---|---|---|
| 1 | github | integration | `pull-request-closed` | event | no | other |
| 2 | operator | integration | `abandon-feature` | command | yes | other |
| 3 | integration | operator | `abandon-feature` | response | yes | other |
| 4 | integration | worktree-custody | `rescue-worktree` | command | no | in-process |
| 5 | worktree-custody | integration | `worktree-rescued` | event | no | in-process |
| 6 | integration | ideas | `feature-abandoned` | event | no | in-process |
| 7 | ideas | task-execution | `idea-cancelled` | event | no | in-process |
| 8 | task-execution | integration | `task-cancelled` | event | no | in-process |

### F13 — A job is abandoned and the operator requeues it (no discover scenario — derived flow)

No discover scenario covers this path. Long work is a job outside the pass (A23, A33). Part 1 of each pass reads the ended jobs. A job stopped at its time limit, or whose process is gone, ends abandoned, and the context that owns its kind waits for the next run. At abandoned_run_limit no new run starts, ideas puts 1 item on the needs-you list, and the operator runs horch job requeue.

Policies realised: operator-requeues-job

```mermaid
sequenceDiagram
  autonumber
  participant clock as Clock (external)
  participant fleet_runtime as Fleet runtime
  participant integration as Landing and integration
  participant ideas as Ideas
  actor operator as Operator
  clock-)fleet_runtime: observe-jobs (command via in-process)
  Note over clock,fleet_runtime: part 1 of the pass reads the facts, with no transaction
  fleet_runtime-)integration: job-abandoned (event via in-process)
  Note over fleet_runtime,integration: for example a full_gate job past its time limit#59; the next run waits for the job backoff
  opt abandoned_runs reaches abandoned_run_limit (3)
    fleet_runtime-)ideas: job-abandoned (event via in-process)
    Note over fleet_runtime,ideas: ideas puts 1 item on the needs-you list
  end
  operator->>fleet_runtime: requeue-job (command via other)
  Note over operator,fleet_runtime: horch CLI request of class operator over the fleetd command channel, with the operator token#59; a request file with the same request_id when fleetd is down (A24, A35)
  fleet_runtime-->>operator: requeue-job (response via other)
  fleet_runtime-)integration: job-requeued (event via in-process)
  Note over fleet_runtime,integration: the next run of the job starts
```

| # | From | To | Message | Kind | Sync | Via |
|---|---|---|---|---|---|---|
| 1 | clock | fleet-runtime | `observe-jobs` | command | no | in-process |
| 2 | fleet-runtime | integration | `job-abandoned` | event | no | in-process |
| 3 | fleet-runtime | ideas | `job-abandoned` | event | no | in-process |
| 4 | operator | fleet-runtime | `requeue-job` | command | yes | other |
| 5 | fleet-runtime | operator | `requeue-job` | response | yes | other |
| 6 | fleet-runtime | integration | `job-requeued` | event | no | in-process |

### F14 — Adapter drift becomes an idea, and the profile is healthy again (no discover scenario — derived flow)

No discover scenario covers this path. fleetd schedules the docs_sync job, then the conformance job. A failed conformance check marks each profile of the adapter unhealthy, so dispatch holds, and ideas queues an idea to update the adapter. When the checks pass again, worker-profiles marks the profiles healthy (design: Docs sync and adapter conformance).

Policies realised: block-profile-on-conformance, idea-on-adapter-drift, unblock-profile-on-conformance

```mermaid
sequenceDiagram
  autonumber
  participant clock as Clock (external)
  participant worker_profiles as Worker profiles and adapters
  participant task_execution as Task execution
  participant ideas as Ideas
  clock-)worker_profiles: run-conformance-tests (command via in-process)
  Note over clock,worker_profiles: a conformance job after the docs_sync job
  opt a conformance check fails
    worker_profiles-)task_execution: profile-marked-unhealthy (event via in-process)
    worker_profiles-)ideas: adapter-conformance-failed (event via in-process)
    Note over worker_profiles,ideas: the idea follows the normal idea flow (F1)
  end
  opt each conformance check passes again
    worker_profiles-)task_execution: profile-marked-healthy (event via in-process)
  end
```

| # | From | To | Message | Kind | Sync | Via |
|---|---|---|---|---|---|---|
| 1 | clock | worker-profiles | `run-conformance-tests` | command | no | in-process |
| 2 | worker-profiles | task-execution | `profile-marked-unhealthy` | event | no | in-process |
| 3 | worker-profiles | ideas | `adapter-conformance-failed` | event | no | in-process |
| 4 | worker-profiles | task-execution | `profile-marked-healthy` | event | no | in-process |

## Message catalogue

| Message | Kind | Producer | Consumers | Payload | Delivery | Contract type | Notes |
|---|---|---|---|---|---|---|---|
| `add-idea` | command | operator | ideas | text, feature_id?, request_id, operator_token | sync | ui | idempotency: fleetd applies a request once per request_id and returns the stored result again; a known request_id with a different payload hash is refused (A24); response: state (applied or refused), result (when applied), reason (when refused); also via file (async, at-least-once): request file in /fleet/run/requests/ when fleetd is down; fleetd reads it at start; the file carries the same request_id and the operator token (A24, A35); horch idea add |
| `add-direct-idea` | command | operator | ideas | ticket_file, feature_id?, request_id, operator_token | sync | ui | idempotency: fleetd applies a request once per request_id and returns the stored result again; a known request_id with a different payload hash is refused (A24); response: state (applied or refused), result (when applied), reason (when refused); also via file (async, at-least-once): request file in /fleet/run/requests/ when fleetd is down; fleetd reads it at start; the file carries the same request_id and the operator token (A24, A35); horch idea add --direct |
| `approve-assumptions` | command | operator | ideas | idea_id, assumptions_hash, request_id, operator_token | sync | ui | idempotency: an approval of the same assumptionsHash is a no-op; a stale hash is refused; fleetd applies a request once per request_id and returns the stored result again; a known request_id with a different payload hash is refused (A24); response: state (applied or refused), result (when applied), reason (when refused); also via file (async, at-least-once): request file in /fleet/run/requests/ when fleetd is down; fleetd reads it at start; the file carries the same request_id and the operator token (A24, A35) |
| `abandon-task-branch` | command | operator | integration | branch, task_id, request_id, operator_token | sync | ui | idempotency: an abandoned branch is not abandoned twice; the rescue ref is returned again; fleetd applies a request once per request_id and returns the stored result again; a known request_id with a different payload hash is refused (A24); response: state (applied or refused), result (when applied), reason (when refused); also via file (async, at-least-once): request file in /fleet/run/requests/ when fleetd is down; fleetd reads it at start; the file carries the same request_id and the operator token (A24, A35) |
| `write-assumptions` | command | sub-orchestrator | ideas | idea_id, assumptions.md | at-least-once | ui | idempotency: fleetd ingests a file once per (path, content hash); files are atomic and immutable |
| `write-spec` | command | sub-orchestrator | ideas | idea_id, spec.md, requirements.yaml | at-least-once | ui | idempotency: once per (ideaId, content hash) |
| `submit-plan` | command | sub-orchestrator | ideas | idea_id, plan.yaml | at-least-once | ui | idempotency: once per (ideaId, plan content hash); plan.verdict.json records the result |
| `answer-question` | command | sub-orchestrator | ideas | question_id, answer|assumption, citation? | at-least-once | ui | idempotency: fleetd ingests the file 1 time, by path and hash; the question row moves to answered once per question_id; horch idea note of the sub-orchestrator writes a file in its idea folder. A request of class worker is a file and never uses the command channel or the request folder (design: The command channel, A35). fleetd checks the citation of a covered answer. |
| `replan-task` | command | sub-orchestrator | ideas | task_id, plan_change | at-least-once | ui | idempotency: once per (taskId, replan round); 1 replan per task |
| `write-result` | command | worker | task-execution | task, attempt, status (completed|blocked|failed), summary, commits[], files_changed[], acceptance_self_check[], notes_for_next | at-least-once | ui | idempotency: once per attemptId; result.json is immutable; outbox/result.json, written with horch result; status is 1 of the closed list completed, blocked, failed (orchestrator, D15) |
| `ask-question` | command | worker | ideas | task_id, question_id, text | at-least-once | ui | idempotency: once per questionId (1 file each); outbox/questions/, written with fleet ask |
| `attempt-exited` | event | worker-profiles | task-execution | attempt_id, exit_status, has_result | at-least-once | customer-supplier | idempotency: once per attemptId; exit.json is immutable; exit.json, written by the fleet-run wrapper (worker-profiles, D2) from the exit fact of the CLI process |
| `usage-limit-reported` | event | provider-pool | task-execution | attempt_id, pool, reset_at? | at-least-once | external-api | idempotency: once per (attemptId, pool); a repeat for a moved attempt is ignored; reset_at is optional (orchestrator, D15): when it is absent, fleetd waits until the pool probe reports ok |
| `primary-branch-moved` | event | git | integration | primary_branch, tip | at-least-once | external-api | idempotency: once per (primaryBranch, tip) |
| `trial-merge-result` | query | git | integration | feature_a, feature_b|primary | sync | external-api | response: clean, paths[]; minted: git merge-tree trial merge |
| `check-merged` | command | clock | integration | feature_id | at-least-once | external-api | idempotency: feature-merged is emitted once per featureId |
| `apply-stall-ladder` | command | clock | task-execution | attempt_id | at-least-once | external-api | idempotency: each rung is recorded once per (attemptId, rung) |
| `approval-requested` | event | ideas | operator | idea_id, assumptions_hash | at-least-once | ui | idempotency: 1 needs-you file per (ideaId, assumptionsHash) |
| `worker-question-asked` | event | ideas | sub-orchestrator | task_id, question_id, text | at-least-once | ui | idempotency: 1 inbox file per questionId |
| `task-escalated` | event | task-execution | ideas, integration | task_id, reason | at-least-once | partnership | v1; idempotency: the reconcile pass is level-triggered, so a reaction can run again; 1 needs-you file per (taskId, reason); integration records it once per taskId; ideas puts the task on the 1 needs-you list (decompose N9); the operator sees it there |
| `pull-request-opened` | event | integration | operator | feature_id, pr_url | at-least-once | ui | idempotency: 1 pull request per featureId |
| `assumptions-written` | event | ideas | integration | idea_id, feature_id? | at-least-once | published-language | v1; idempotency: the reconcile pass is level-triggered, so a reaction can run again; integration opens at most 1 feature per idea; open-feature is a no-op when the idea has a feature |
| `feature-opened` | event | integration | ideas | feature_id, branch, primary_branch, idea_id | at-least-once | published-language | v1; idempotency: the reconcile pass is level-triggered, so a reaction can run again; ideas requests approval once per (ideaId, assumptionsHash) |
| `direct-idea-accepted` | event | ideas | integration | idea_id, ticket_file, feature_id? | at-least-once | published-language | v1; idempotency: the reconcile pass is level-triggered, so a reaction can run again; at most 1 feature per idea |
| `plan-committed` | event | ideas | task-execution | idea_id, task_ids[] | at-least-once | customer-supplier | v1; idempotency: the reconcile pass is level-triggered, so a reaction can run again; promote-task is a no-op for a task past planned |
| `plan-refused` | event | ideas | task-execution | idea_id, task_id?, reasons[] | at-least-once | customer-supplier | v1; idempotency: the reconcile pass is level-triggered, so a reaction can run again; escalate-task is a no-op for an escalated task; each refusal carries its reasons |
| `task-replanned` | event | ideas | task-execution | task_id, plan_change | at-least-once | customer-supplier | v1; idempotency: the reconcile pass is level-triggered, so a reaction can run again; promote once per (taskId, replan round) |
| `question-answered` | event | ideas | task-execution | question_id, task_id | at-least-once | customer-supplier | v1; idempotency: the reconcile pass is level-triggered, so a reaction can run again; 1 launch per (taskId, questionId) |
| `own-assumption-recorded` | event | ideas | task-execution | question_id, task_id | at-least-once | customer-supplier | v1; idempotency: the reconcile pass is level-triggered, so a reaction can run again; 1 launch per (taskId, questionId) |
| `idea-tasks-landed` | event | ideas | integration | idea_id, feature_id | at-least-once | published-language | v1; idempotency: the reconcile pass is level-triggered, so a reaction can run again; the full gate runs once per (featureId, feature tip) |
| `replan-requested` | event | task-execution | ideas | task_id, attempts | at-least-once | published-language | v1; idempotency: the reconcile pass is level-triggered, so a reaction can run again; 1 replan per task |
| `task-leased` | event | task-execution | worktree-custody | task_id, attempt_id, worktree_id, branch | at-least-once | customer-supplier | v1; idempotency: the reconcile pass is level-triggered, so a reaction can run again; create-worktree is a no-op when the worktree exists for the task |
| `attempt-failed` | event | task-execution | worktree-custody, integration | attempt_id, task_id, outcome | at-least-once | customer-supplier | v1; idempotency: the reconcile pass is level-triggered, so a reaction can run again; the worktree state moves once per attempt_id; repair_attempts of a branch goes up once per attempt_id; worktree-custody keeps the worktree for the retry. For a resolver or fix attempt of a done task, integration adds 1 to repair_attempts of the branch (A27, A34) |
| `task-done` | event | task-execution | integration | task_id, branch, verdict | at-least-once | partnership | v1; idempotency: the reconcile pass is level-triggered, so a reaction can run again; mark-branch-ready is a no-op for a ready branch |
| `worktree-created` | event | worktree-custody | task-execution | worktree_id, task_id, path, branch | at-least-once | published-language | v1; idempotency: the reconcile pass is level-triggered, so a reaction can run again; launch once per (taskId, attemptId) |
| `rescue-failed` | event | worktree-custody | task-execution | worktree_id, task_id | at-least-once | published-language | v1; idempotency: the reconcile pass is level-triggered, so a reaction can run again; escalate-task is a no-op for an escalated task |
| `task-branch-conflicted` | event | integration | task-execution | branch, task_id | at-least-once | partnership | v1; idempotency: the reconcile pass is level-triggered, so a reaction can run again; 1 new attempt per (taskId, conflicting feature tip) |
| `resolver-task-opened` | event | integration | task-execution | feature_id, task_id, conflict | at-least-once | partnership | v1; idempotency: the reconcile pass is level-triggered, so a reaction can run again; 1 resolver task per (featureId, conflict) |
| `task-branch-landed` | event | integration | worktree-custody, ideas | branch, task_id, feature_id, tip | at-least-once | published-language | v1; idempotency: the reconcile pass is level-triggered, so a reaction can run again; remove-worktree and roll-up-idea are no-ops when already done |
| `feature-merged` | event | integration | ideas | feature_id | at-least-once | published-language | v1; idempotency: the reconcile pass is level-triggered, so a reaction can run again; close-idea is a no-op for a merged idea |
| `launch-environment` | query | worker-profiles | task-execution, ideas | profile, task_id|idea_id, attempt_id? | sync | customer-supplier | v1; response: cli, flags[], env (scrubbed: no ANTHROPIC_API_KEY), config_home_files[], sandbox_profile; resolve a profile to the exact launch of 1 process; task-execution reads it to check a fallback persona and its provider (F4); worker-profiles uses it inside start-agent-process |
| `start-agent-process` | command | task-execution | worker-profiles | profile, task_id|idea_id, attempt_id?, start_file, pane? | sync | customer-supplier | v1; idempotency: 1 process per attempt_id, or per idea_id for a sub-orchestrator; a start for a live recorded process is a no-op; response: pid, pgid, pane; worker-profiles owns it (decompose N8, D2): it resolves the launch environment and starts the fleet-run wrapper (attempt) or the sub-orchestrator CLI (pane) through the platform layer; ideas sends it too (R11) |
| `open-pull-request` | command | integration | github | feature_id, branch, base_branch | sync | external-api | idempotency: 1 pull request per feature_id; integration reads features.pr first and does not open a second one; response: pr; after the full gate passes; pr setting gh opens it with gh, manual pushes the feature branch and records the compare URL; a stacked feature uses the lower feature branch as base (design: Features, landing and integration) |
| `pull-request-merged` | event | github | integration | feature_id, pr, merge_commit | at-least-once | external-api | idempotency: once per feature_id; check-merged confirms the merge in the primary branch by content before feature-merged; the merge fact from GitHub; a manual merge is found by check-merged alone |
| `assign-task` | command | task-execution | worker | task, idea, key, attempt, profile, worktree, branch, read_first[], goal, covers[], acceptance[], lease{expires, renew_every_s}, report_to | at-least-once | ui | v1; idempotency: task.yaml is written by fleetd before the launch, atomic and never rewritten; a change is a new attempt; task.yaml, written by fleetd from the committed plan (design: Workspace layout and file protocol) |
| `start-work` | command | task-execution | worker | task_yaml, rescue_commit?, progress_notes? | at-least-once | ui | v1; idempotency: 1 inbox/START.md per attempt; the doorbell line that points at it is constant (FM-15); inbox/START.md, what the worker reads first; its path is the launch argument |
| `heartbeat-written` | event | worker-profiles | task-execution | task, attempt, pid, pgid, mux_pane, proc_state, agent_state, last_output_at, last_tool_event, tokens, seq, written_at | at-most-once | customer-supplier | v1; idempotency: the file is rewritten every 10 s; the highest seq wins and a lower or equal seq is ignored; heartbeat.json, written by the fleet-run wrapper (worker-profiles, D2), not the model; an outside signal that renews the lease |
| `task-verdict-written` | event | task-execution | sub-orchestrator | task, attempt, decision, reasons[] | at-least-once | ui | v1; idempotency: 1 verdict.json per verified attempt; fleetd is its only writer; verdict.json, the decision of the verifier |
| `plan-verdict-written` | event | ideas | sub-orchestrator | idea, decision (committed|refused), reasons[] | at-least-once | ui | v1; idempotency: 1 plan.verdict.json per (idea, plan content hash); plan.verdict.json, the result of plan validation; a refusal carries its reasons |
| `assumptions-approved` | event | ideas | sub-orchestrator | idea, assumptions_hash | at-least-once | ui | v1; idempotency: fleetd writes approval.json only on your approve, with the hash you saw, and ignores any other approval.json (FM-32, I7); approval.json, written by fleetd when you run horch idea approve |
| `agent-status-changed` | event | herdr | task-execution, ideas | pane, agent_status | at-most-once | external-api | idempotency: edge events can be lost by design; each pass re-reads session.snapshot, so a repeat or a loss changes nothing; also via other (sync, sync): session.snapshot, re-read in every pass (level-triggered); pane.agent_status_changed from the herdr socket events.subscribe, received through the herdr adapter of fleet-runtime (R12, R13): working, blocked on a dialog, or idle |
| `cancel-idea` | command | operator | ideas | idea_id, request_id, operator_token | sync | ui | idempotency: a cancelled idea is not cancelled twice; fleetd applies a request once per request_id and returns the stored result again; a known request_id with a different payload hash is refused (A24); response: state (applied or refused), result (when applied), reason (when refused); also via file (async, at-least-once): request file in /fleet/run/requests/ when fleetd is down; fleetd reads it at start; the file carries the same request_id and the operator token (A24, A35); horch idea cancel (request kind idea_cancel) |
| `cancel-task` | command | operator | task-execution | task_id, request_id, operator_token | sync | ui | idempotency: a cancelled task is not cancelled twice; fleetd applies a request once per request_id and returns the stored result again; a known request_id with a different payload hash is refused (A24); response: state (applied or refused), result (when applied), reason (when refused); also via file (async, at-least-once): request file in /fleet/run/requests/ when fleetd is down; fleetd reads it at start; the file carries the same request_id and the operator token (A24, A35); horch task cancel (request kind task_cancel). A policy also issues it inside fleetd for a replan that replaces the task and for the cancel or abandon of its idea or feature (A34) |
| `requeue-task` | command | operator | task-execution | task_id, request_id, operator_token | sync | ui | idempotency: fleetd applies a request once per request_id and returns the stored result again; a known request_id with a different payload hash is refused (A24); response: state (applied or refused), result (when applied), reason (when refused); also via file (async, at-least-once): request file in /fleet/run/requests/ when fleetd is down; fleetd reads it at start; the file carries the same request_id and the operator token (A24, A35); horch task requeue (request kind task_requeue). It sets the attempt count to 0, and for a task branch at its repair limit it sets repair_attempts to 0 and starts 1 more attempt (A27, D15) |
| `abandon-feature` | command | operator | integration | feature_id, request_id, operator_token | sync | ui | idempotency: an abandoned feature is not abandoned twice; fleetd applies a request once per request_id and returns the stored result again; a known request_id with a different payload hash is refused (A24); response: state (applied or refused), result (when applied), reason (when refused); also via file (async, at-least-once): request file in /fleet/run/requests/ when fleetd is down; fleetd reads it at start; the file carries the same request_id and the operator token (A24, A35); horch feature abandon (request kind feature_abandon). Only the operator sets abandoned (A28) |
| `requeue-job` | command | operator | fleet-runtime | job_id, request_id, operator_token | sync | ui | idempotency: fleetd applies a request once per request_id and returns the stored result again; a known request_id with a different payload hash is refused (A24); response: state (applied or refused), result (when applied), reason (when refused); also via file (async, at-least-once): request file in /fleet/run/requests/ when fleetd is down; fleetd reads it at start; the file carries the same request_id and the operator token (A24, A35); horch job requeue (request kind job_requeue). It sets abandoned_runs of the job key to 0, and the next run starts (A33) |
| `drop-rescue-ref` | command | operator | worktree-custody | ref, request_id, operator_token | sync | ui | idempotency: a deleted ref is not deleted twice; fleetd applies a request once per request_id and returns the stored result again; a known request_id with a different payload hash is refused (A24); response: state (applied or refused), result (when applied), reason (when refused); also via file (async, at-least-once): request file in /fleet/run/requests/ when fleetd is down; fleetd reads it at start; the file carries the same request_id and the operator token (A24, A35); horch rescue drop (request kind rescue_drop). Only the operator deletes a rescue ref of class unique (A27, I10) |
| `idea-cancelled` | event | ideas | task-execution | idea_id, cause | at-least-once | customer-supplier | v1; idempotency: the reconcile pass is level-triggered, so a reaction can run again; cancel-task is a no-op for a task that is terminal; task-execution cancels each task of the idea that is not terminal (cancel-tasks-on-idea-cancelled, A34) |
| `task-cancelled` | event | task-execution | integration | task_id, branch, cause | at-least-once | partnership | v1; idempotency: the reconcile pass is level-triggered, so a reaction can run again; a branch is rescued and abandoned once per branch; fleetd first stops the process group of a live attempt, then moves the task to cancelled. integration then writes a rescue ref of the task branch and abandons it (abandon-branch-on-task-cancelled, A34) |
| `rescue-worktree` | command | integration | worktree-custody | branch, task_id, worktree | at-least-once | customer-supplier | idempotency: 1 rescue ref per (branch, tip); a rescue job is keyed by its subject and tip (A33); integration asks worktree-custody to write a rescue ref before a branch is abandoned or rewritten. The rescue runs as a rescue job outside the pass (A23, CC7) |
| `worktree-rescued` | event | worktree-custody | integration | branch, task_id, rescue_ref, rescue_class | at-least-once | published-language | v1; idempotency: the reconcile pass is level-triggered, so a reaction can run again; integration abandons a branch once per branch; the rescue ref exists, of class unique or superseded (A27). integration drops the branch only after this event (CC7) |
| `task-branch-gate-failed` | event | integration | task-execution | branch, task_id, gate_output | at-least-once | partnership | v1; idempotency: the reconcile pass is level-triggered, so a reaction can run again; 1 fix attempt is started per (branch, repair_attempts); the bisect found the failing branch. fleetd starts a fix attempt of the task in its worktree with the gate output (triage-on-gate-failed, A27) |
| `task-branch-repair-escalated` | event | integration | ideas, task-execution | branch, task_id, repair_attempts | at-least-once | published-language | v1; idempotency: 1 needs-you item per branch, written by key (design: The needs-you list has a life cycle); repair_attempts of the branch reached max_attempts of the task. ideas puts 1 item on the needs-you list. task-execution accepts horch task requeue of the done task, which sets repair_attempts to 0 and starts 1 more attempt of the branch. The task stays done (A27, A34) |
| `feature-abandoned` | event | integration | ideas | feature_id | at-least-once | published-language | v1; idempotency: the reconcile pass is level-triggered, so a reaction can run again; cancel-idea is a no-op for an idea that is merged or cancelled; each branch of the feature that is not landed has a rescue ref and is abandoned first. ideas cancels each idea of the feature that is not merged (cancel-on-feature-abandoned, A28) |
| `pull-request-closed` | event | github | integration | feature_id, pr_state | at-least-once | external-api | idempotency: once per feature_id and pr_state; read by the fetch job (pr = gh): the pull request is closed and not merged. pr_state is closed, and 1 item goes on the needs-you list (A28) |
| `push-feature-branch` | command | integration | git | feature_id, branch, seen_remote_tip | at-least-once | external-api | idempotency: an effect job: a failed run is never used again, and each retry is a new run (A33); a push job of the feature branch, outside the pass. A push that rewrites uses --force-with-lease on seen_remote_tip. A passed push sets push_state ok; a failed push sets push_state failed and push_not_before (A28) |
| `observe-jobs` | command | clock | fleet-runtime | pass | at-most-once | external-api | idempotency: each pass reads the ended jobs again; a job row ends once; part 1 of each pass reads the ended jobs and their results. A running job whose process is gone ends abandoned (A23) |
| `job-abandoned` | event | fleet-runtime | task-execution, integration, worktree-custody, worker-profiles, ideas | job_id, kind, subject, abandoned_runs | at-least-once | open-host-service | v1; idempotency: the reconcile pass is level-triggered, so a reaction can run again; the owner context reads the job row; 1 needs-you item per job key; a job stopped at its time limit, or its process was lost. It has no verdict. The context that owns the job kind waits for the next run after the backoff. At abandoned_run_limit (3) no new run starts, and ideas puts 1 item on the needs-you list (A23, A33) |
| `job-requeued` | event | fleet-runtime | task-execution, integration, worktree-custody, worker-profiles | job_id, kind, subject | at-least-once | open-host-service | v1; idempotency: the reconcile pass is level-triggered, so a reaction can run again; the next run starts once per job key; abandoned_runs of the job key is 0, and the next run starts (A33) |
| `run-conformance-tests` | command | clock | worker-profiles | cli, version | at-least-once | external-api | idempotency: a conformance job keyed by the CLI and its version (A33); fleetd schedules the docs_sync job daily and on each CLI version change, then the conformance job. Neither job has a repository (design: Docs sync and adapter conformance) |
| `adapter-conformance-failed` | event | worker-profiles | ideas | cli, version, failed_checks | at-least-once | customer-supplier | v1; idempotency: the reconcile pass is level-triggered, so a reaction can run again; 1 idea per (cli, version) drift; a conformance check failed, the check Operator boundary included. worker-profiles marks each profile of the adapter unhealthy, and ideas queues an idea to update the adapter (idea-on-adapter-drift) |
| `profile-marked-unhealthy` | event | worker-profiles | task-execution | profile_id, reason | at-least-once | customer-supplier | v1; idempotency: the reconcile pass is level-triggered, so a reaction can run again; dispatch holds once per profile; fleetd dispatches no task to the profile; horch status shows the dispatch hold (A28) |
| `profile-marked-healthy` | event | worker-profiles | task-execution | profile_id | at-least-once | customer-supplier | v1; idempotency: the reconcile pass is level-triggered, so a reaction can run again; dispatch resumes once per profile; the conformance checks of the adapter passed; fleetd dispatches to the profile again (unblock-profile-on-conformance) |

## Integration decisions (one per context-map relationship)

| Relationship | Upstream → downstream | Pattern | Mechanism | Assumed distance | Rationale |
|---|---|---|---|---|---|
| R1 | ideas → integration | published-language | in-process-call | same-deployable | Both contexts are code inside the 1 fleetd process (design: Architecture and roles; scale target 1 deployable). In-memory domain events of a versioned type, consumed in the level-triggered reconcile pass (design: Supervision, liveness and reconciliation). |
| R2 | ideas → task-execution | customer-supplier | in-process-call | same-deployable | Both contexts are code inside the 1 fleetd process (design: Architecture and roles; scale target 1 deployable). Events from ideas; task-execution reacts. No caller needs an answer now. |
| R3 | task-execution → ideas | published-language | in-process-call | same-deployable | Both contexts are code inside the 1 fleetd process (design: Architecture and roles; scale target 1 deployable). replan-requested as an in-memory event; the sub-orchestrator does the replan through files. |
| R4 | task-execution → worktree-custody | customer-supplier | in-process-call | same-deployable | Both contexts are code inside the 1 fleetd process (design: Architecture and roles; scale target 1 deployable). task-leased and the worktree lease row commit in 1 ledger transaction; git worktree add runs after the commit (decompose R4, CC5). |
| R5 | worktree-custody → task-execution | published-language | in-process-call | same-deployable | Both contexts are code inside the 1 fleetd process (design: Architecture and roles; scale target 1 deployable). worktree-created and rescue-failed as in-memory events read in the next pass. |
| R6 | task-execution → integration | partnership | in-process-call | same-deployable | Both contexts are code inside the 1 fleetd process (design: Architecture and roles; scale target 1 deployable). task-done and task-escalated as in-memory events; integration hosts mark-branch-ready. R6 and R7 are a partnership (orchestrator, D15): they change together. |
| R7 | integration → task-execution | partnership | in-process-call | same-deployable | Both contexts are code inside the 1 fleetd process (design: Architecture and roles; scale target 1 deployable). task-branch-conflicted and resolver-task-opened as in-memory events; task-execution launches the attempt. R6 and R7 are a partnership (orchestrator, D15). |
| R8 | integration → worktree-custody | published-language | in-process-call | same-deployable | Both contexts are code inside the 1 fleetd process (design: Architecture and roles; scale target 1 deployable). task-branch-landed as an in-memory event; worktree-custody refuses a removal that loses work (04 W6). |
| R9 | integration → ideas | published-language | in-process-call | same-deployable | Both contexts are code inside the 1 fleetd process (design: Architecture and roles; scale target 1 deployable). task-branch-landed and feature-merged as in-memory events; ideas never reads git. |
| R10 | worker-profiles → task-execution | customer-supplier | in-process-call | same-deployable | Both contexts are code inside the 1 fleetd process (design: Architecture and roles; scale target 1 deployable). task-execution sends start-agent-process to worker-profiles, which owns the environment scrub and the fleet-run wrapper (decompose N8, D2). task-execution asks launch-environment synchronously only to check a fallback persona (F4). heartbeat.json comes back as a file. |
| R11 | worker-profiles → ideas | customer-supplier | in-process-call | same-deployable | Both contexts are code inside the 1 fleetd process (design: Architecture and roles; scale target 1 deployable). ideas sends start-agent-process to worker-profiles for a sub-orchestrator pane (F1), with the same environment scrub. |
| R12 | fleet-runtime → task-execution | open-host-service | in-process-call | same-deployable | Both contexts are code inside the 1 fleetd process (design: Architecture and roles; scale target 1 deployable). Calls through the fleet-runtime facade: the single ledger writer (decompose N5) and the herdr adapter, which gives panes and delivers agent-status-changed. Process starts go through worker-profiles, not fleet-runtime. No flow draws a step to fleet-runtime: the ledger writer sits under every step (A3), and the flows draw the herdr adapter as herdr itself. Round 3: fleet-runtime also runs the task_check job of task-execution and delivers job-abandoned and job-requeued (F13). |
| R13 | fleet-runtime → ideas | open-host-service | in-process-call | same-deployable | Both contexts are code inside the 1 fleetd process (design: Architecture and roles; scale target 1 deployable). The same facade for ideas: the command channel delivers horch idea commands and the answers of the sub-orchestrator, and the herdr adapter gives sub-orchestrator panes. No flow draws a step to fleet-runtime: the ledger writer sits under every step (A3), and the flows draw the herdr adapter as herdr itself. |
| R14 | fleet-runtime → integration | open-host-service | in-process-call | same-deployable | Both contexts are code inside the 1 fleetd process (design: Architecture and roles; scale target 1 deployable). integration calls the fleet-runtime facade: the single ledger writer and the job runner. integration queues the jobs bundle_build, fast_gate, full_gate, trial_merge, rebase, fetch and push in the transaction of part 2 of a pass, and reads their results in part 1 of a later pass. A pass never waits for a job (A23, A33). F13 draws job-abandoned and job-requeued. |
| R15 | fleet-runtime → worktree-custody | open-host-service | in-process-call | same-deployable | Both contexts are code inside the 1 fleetd process (design: Architecture and roles; scale target 1 deployable). worktree-custody calls the fleet-runtime facade: the single ledger writer and the job runner. worktree-custody queues the jobs worktree_add, worktree_remove, worktree_list and rescue in the transaction of part 2 of a pass, and reads their results in part 1 of a later pass. A pass never waits for a job (A23, A33). F13 draws job-abandoned and job-requeued with integration as the example owner; the same 2 events reach this context for its own job kinds, so no flow draws a separate step. |
| R16 | fleet-runtime → worker-profiles | open-host-service | in-process-call | same-deployable | Both contexts are code inside the 1 fleetd process (design: Architecture and roles; scale target 1 deployable). worker-profiles calls the fleet-runtime facade: the single ledger writer and the job runner. worker-profiles queues the jobs docs_sync and conformance in the transaction of part 2 of a pass, and reads their results in part 1 of a later pass. A pass never waits for a job (A23, A33). F13 draws job-abandoned and job-requeued with integration as the example owner; the same 2 events reach this context for its own job kinds, so no flow draws a separate step. |

## Coupling concerns

- **CC1** (high) — task-execution and integration depend on each other: task-done, task-escalated, task-cancelled and attempt-failed go one way, task-branch-conflicted, task-branch-gate-failed and resolver-task-opened go the other. Both are core, so a change in one state machine can force a change in the other. — contexts: task-execution, integration — smell: bidirectional; flows: F3, F6, F7, F10, F11; mitigation: Keep both directions as versioned in-memory events only, never direct calls. The orchestrator (D15) declared R6 and R7 a partnership: the 2 contexts plan their changes together (answers Q4).
- **CC2** (medium) — ideas and task-execution depend on each other: plan and answer events go to task-execution, replan-requested comes back to ideas. — contexts: ideas, task-execution — smell: bidirectional; flows: F2, F5, F6; mitigation: Accepted: decompose split the 2 directions into R2 and R3 on purpose. Both directions stay events.
- **CC3** (medium) — ideas and integration depend on each other: assumptions-written and idea-tasks-landed go to integration, feature-opened, task-branch-landed and feature-merged come back. — contexts: ideas, integration — smell: bidirectional; flows: F1, F3, F7; mitigation: Accepted: R1 and R9 are both published languages; ideas never reads git and integration never reads idea files.
- **CC4** (medium) — task-execution and worktree-custody depend on each other (R4 and R5): the worktree lifecycle follows the task lease and the attempt end, and launch waits for the worktree. — contexts: task-execution, worktree-custody — smell: bidirectional; flows: F2, F5, F6; mitigation: Accepted as the lifecycle link of decompose N6; keep it as events in both directions.
- **CC5** (medium) — task-leased is an event, but task-execution cannot launch until the worktree exists, and a failed git worktree add must come back to task-execution. The event hides a command that needs a result. — contexts: task-execution, worktree-custody — smell: event-should-be-command; flows: F8; mitigation: task-execution acts as the process manager of dispatch: lease and worktree lease row in 1 transaction, then create-worktree, then launch on worktree-created, with a timeout back to ready.; saga / process-manager candidate — design it at deep depth
- **CC6** (high) — Trial merges, git worktree add and remove, and process starts are external calls. If one runs inside the ledger write transaction, a slow git or a hung process start holds the only writer. — contexts: integration, worktree-custody, worker-profiles — smell: external-in-transaction; flows: F7, F8, F9, F13; mitigation: The design settles it (A23, A24, A32): a pass has 3 parts. Part 1 reads the facts with no transaction, part 2 writes each effect as a row in an intended state in 1 transaction, and part 3 runs the effects after the commit. Each git call is a job outside the pass, and each process start goes through the 1 launcher after the commit.
- **CC7** (high) — abandon-task-branch drops a task branch in integration, but the rescue ref that keeps the work is made in worktree-custody (04 W6). Both must happen, in order, or work is lost. So integration sends rescue-worktree to worktree-custody and waits for worktree-rescued, while task-branch-landed goes the same way: a command and an event in both directions on R8. — contexts: integration, worktree-custody — smell: distributed-transaction; flows: F6, F10, F11, F12; mitigation: integration asks worktree-custody to rescue first (rescue-worktree) and drops the branch only on worktree-rescued; integration owns this process. The design states the order for a cancel, horch branch abandon and horch feature abandon (A27, A28, A34). The bidirectional link on R8 is accepted: git has 2 callers by concern (design: Module owners), so integration must ask worktree-custody for each rescue ref.; saga / process-manager candidate — design it at deep depth
- **CC8** (medium) — plan-refused is an ideas fact, but escalate-task runs in task-execution, which owns neither the plan nor the reasons. — contexts: ideas, task-execution — smell: wrong-policy-host; flows: F6; mitigation: Keep the policy in task-execution only for a replan of an existing task; a refused first plan stays in ideas. ideas owns the 1 needs-you list (orchestrator, D15), so task-escalated goes back to ideas (F6).
- **CC9** (low) — task-branch-landed has 2 consumers that need different data: worktree-custody needs the task and worktree, ideas needs the feature and idea roll-up. — contexts: integration, worktree-custody, ideas — smell: wide-fan-out; flows: F3, F7; mitigation: Keep 1 thin event (branch, task_id, feature_id, tip); each consumer reads its own rows.
- **CC10** (medium) — A cancel and an abandon cross 3 contexts in a chain: ideas cancels an idea, task-execution cancels its tasks, integration rescues and abandons each branch through worktree-custody. horch feature abandon starts the same chain from integration. A step that is lost leaves a task, a branch or a worktree that is not terminal. — contexts: ideas, task-execution, integration, worktree-custody — smell: distributed-transaction; flows: F10, F12; mitigation: Each step is a local transaction on its own rows, and the level-triggered pass re-derives the next step from the rows: an idea in cancelled with a task that is not terminal, a task in cancelled with a branch that is not abandoned. Each task state that is not terminal has an exit to cancelled (A34).; saga / process-manager candidate — design it at deep depth
- **CC11** (low) — job-abandoned and job-requeued fan out from fleet-runtime to the 4 contexts that own job kinds and to ideas for the needs-you item. — contexts: fleet-runtime, task-execution, integration, worktree-custody, worker-profiles, ideas — smell: wide-fan-out; flows: F13; mitigation: Keep 1 thin event (job_id, kind, subject, abandoned_runs). Each owner context reacts only to its own job kinds and reads its own rows; ideas writes the needs-you item by key.

## Deprecated (removed on a re-run; ids are never reused)

- `Q1` (open_questions) — Answered (orchestrator, D15): worker-profiles owns the fleet-run wrapper. Recorded in D2 (option B) and A7. — since 2026-11-12
- `Q2` (open_questions) — Answered by discover N10: discover adopted start-agent-process (command) and launch-environment and trial-merge-result (read models, modelled here as queries). Recorded in A6. — since 2026-11-12
- `Q3` (open_questions) — Answered (orchestrator, D15): git has 2 callers, split by concern. worktree-custody calls git for worktrees and rescue refs, integration for branches and merges. Recorded in A8. — since 2026-11-12
- `Q4` (open_questions) — Answered (orchestrator, D15): task-execution and integration are a partnership. Recorded in CC1, R6, R7 and N6. decompose still names R6 customer-supplier and R7 published-language. — since 2026-11-12
- `Q5` (open_questions) — Answered (orchestrator, D15): the sub-orchestrator answers a worker question through the command channel; fleetd stays the only ledger writer. Recorded in answer-question and F5. — since 2026-11-12
- `Q6` (open_questions) — Answered (discover N13): discover adopts the 6 ids with the types of connect. assign-task and start-work are commands. heartbeat-written, task-verdict-written, plan-verdict-written and agent-status-changed are events. — since 2026-11-12

## Assumptions

- **A1** (high) All 6 contexts run in the 1 fleetd process, so every context-to-context message is in-process. Only the command channel, the file protocol, the herdr adapter, the platform layer, git and GitHub cross a process boundary (design: Architecture and roles).
- **A2** (high) Cross-context events are in-memory events consumed in the level-triggered 5 s reconcile pass, which re-derives work from ledger rows, so a lost event is caught on the next pass. A pass has 3 parts (facts, 1 transaction, effects) and a work budget with cursors, and it never waits for a job (A23, A24, A32). Delivery is at-least-once and every consumer is idempotent on a natural key (design: Supervision, liveness and reconciliation; decision D1).
- **A3** (high) Every state change in every flow commits through the single ledger writer in fleet-runtime (decompose N5). The flows do not draw that call on each step.
- **A4** (high) Operator commands travel as horch CLI requests of class operator over the fleetd command channel (Unix socket or named pipe, through the platform layer), drawn as via other. When fleetd is down the request goes to a request file in /fleet/run/requests/ that fleetd reads at start. Each request carries a request_id, its idempotency key, and the operator token; fleetd writes a requests row with the stored result or the refusal reason (design: The command channel, A24, A35). Requests of class worker (progress, result, ask, idea note) are files and never use the channel.
- **A5** (high) Agent hand-offs (assumptions.md, plan.yaml, result.json, questions, exit.json) are files: atomic, immutable and read in the pass, so they are at-least-once and idempotent on path and content hash (design: Workspace layout and file protocol).
- **A6** (high) 3 message ids were minted here: launch-environment (query), start-agent-process (command) and trial-merge-result (query). Discover adopted all 3 (discover N10): start-agent-process is a discover command, and the 2 queries reuse discover read-model ids.
- **A7** (high) The fleet-run wrapper belongs to worker-profiles (D2 option B, orchestrator D15). It writes heartbeat.json and exit.json. attempt-exited (exit.json) comes from worker-profiles, the writer of the file; the file records the exit fact of the CLI process.
- **A8** (high) git is called by 2 contexts, split by concern: worktree-custody for worktrees and rescue refs, integration for branches, merges, bundles, trial merges, fetch and push (orchestrator, D15; design: Module owners). Each git call is a job outside the pass (A23).
- **A9** (medium) worker-profiles owns start-agent-process (decompose N8). task-execution and ideas send it; worker-profiles resolves the launch environment, runs the fleet-run wrapper and starts it in its own process group through the platform layer. When the profile needs a pane, the caller gets the pane from the herdr adapter of fleet-runtime (R12, R13) and passes it in the request.
- **A10** (high) Each file of the design workspace layout that crosses the process boundary is 1 message: task.yaml (assign-task), inbox/START.md (start-work), heartbeat.json (heartbeat-written), verdict.json (task-verdict-written), plan.verdict.json (plan-verdict-written) and approval.json (assumptions-approved). The path names its 1 writer (design: Workspace layout and file protocol, rule 3). 6 of these ids are minted here, because discover has no id for them: assign-task, start-work, heartbeat-written, task-verdict-written, plan-verdict-written and agent-status-changed (Q6).
- **A11** (high) Payload field names are the snake_case names of the design (orchestrator, D15). A file message uses the field names of the design example of that file; an in-memory event uses <entity>_id.
- **A12** (high) The model has 14 flows, 7 more than the 4 to 7 of depth standard. F2 and F3 were split at a hand-off (F8, F9) to keep each flow at 9 messages or fewer. Round 3 adds F10 to F14, 1 flow for each new group of cross-context policies of discover (cancel, branch repair, feature abandon, jobs, adapter conformance).
- **A13** (high) Round 3 adds 22 message ids, and each one is a discover command or event. 6 of them are operator requests of the design list of request kinds: cancel-idea, cancel-task, requeue-task, abandon-feature, requeue-job and drop-rescue-ref. No id is minted here in round 3.
- **A14** (medium) pull-request-closed has no cross-context event for its needs-you item, so F12 draws it as a note: integration raises the item, and ideas owns the needs-you list (design: The needs-you list has a life cycle).

## Open questions

None.

---
Next step: `/ddd-organise` groups contexts into teams and deployables using the coupling concerns and sync chains above.
