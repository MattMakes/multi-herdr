# DDD coverage summary - fleetd: the horch fleet control plane

Workspace: `ddd/`. Mode: auto. Depth: standard. Source of the design: `docs/specs/fleetd/design.md`.

Read this file first. Then open `ddd/review.html` in any browser. It needs no server.

## Steps

All 9 steps are `done`. No step is pending, stale or draft.

| Step | Status | Open Q | Blocking | Open decisions | Main artifact | Counts |
|---|---|---|---|---|---|---|
| 01 understand | done | 8 | 0 | 1 | `ddd/01-understand/understand.md` | 4 goals, 12 capabilities |
| 02 discover | done | 5 | 0 | 2 | `ddd/02-discover/event-storm.md` | 82 events, 4 pivotal, 15 hotspots, 4 scenarios |
| 03 decompose | done | 3 | 0 | 4 | `ddd/03-decompose/subdomains.md` | 6 subdomains, 6 contexts, 13 relationships |
| 04 strategize | done | 2 | 0 | 2 | `ddd/04-strategize/core-domain-chart.md` | core: task-execution, integration. All 6 are build. |
| 05 connect | done | 5 | 0 | 1 | `ddd/05-connect/message-flows.md` | 7 flows, 42 messages, 9 coupling concerns, 0 sagas |
| 06 organise | done | 2 | 0 | 1 | `ddd/06-organise/team-topology.md` | 1 team, 1 deployable, target 1-1, within target. Style: modular monolith. |
| 07 define | done | 5 | 0 | 2 | `ddd/07-define/<context>/bounded-context-canvas.md` x6, `ddd/07-define/system-context.md` | 6 canvases, 12 quality attributes |
| 08 code | done | 6 | 0 | 1 | `ddd/08-code/implementation-plan.md` | 4 aggregates: Task, Verification (task-execution), Feature, TaskBranch (integration). Language: Rust. |
| 09 contracts | done | 6 | 0 | 1 | `ddd/09-contracts/contracts.md` | 21 contracts: 15 published, 6 inherited from external systems. 21 in-process messages need no wire contract. |
| glossary | | | | | `ddd/glossary.md` | 20 shared terms, 2 with context-specific meanings (Feature, Lease), 6 coined in code |

Totals: 42 open questions, 0 blocking, 15 open decisions.

## Artifacts

- `ddd/manifest.json`
- `ddd/glossary.md`
- `ddd/01-understand/understand.json`, `ddd/01-understand/understand.md`
- `ddd/02-discover/discover.json`, `ddd/02-discover/event-storm.md`
- `ddd/03-decompose/decompose.json`, `ddd/03-decompose/subdomains.md`
- `ddd/04-strategize/strategize.json`, `ddd/04-strategize/core-domain-chart.md`
- `ddd/05-connect/connect.json`, `ddd/05-connect/message-flows.md`, `ddd/05-connect/decisions/` (3 option specs)
- `ddd/06-organise/organise.json`, `ddd/06-organise/team-topology.md`, `ddd/06-organise/decisions/` (4 option specs)
- `ddd/07-define/define.json`, `ddd/07-define/system-context.md`, 1 `bounded-context-canvas.md` in each of `ideas/`, `task-execution/`, `worktree-custody/`, `integration/`, `worker-profiles/`, `fleet-runtime/`
- `ddd/08-code/code.json`, `ddd/08-code/implementation-plan.md`, 1 `design.md` per context, `ddd/08-code/task-execution/aggregate-canvas-task.md`, `ddd/08-code/task-execution/aggregate-canvas-verification.md`, `ddd/08-code/integration/aggregate-canvas-feature.md`, `ddd/08-code/integration/aggregate-canvas-task-branch.md`, `ddd/08-code/decisions/` (6 option specs)
- `ddd/09-contracts/contracts.json`, `ddd/09-contracts/contracts.md`, `ddd/09-contracts/schemas/` (26 files), `ddd/09-contracts/examples/` (48 files)

## Bounded contexts

6 contexts in 1 deployable (`horch`):

- task-execution - core, domain-model
- integration - core, domain-model
- ideas - supporting, active-record
- worktree-custody - supporting, active-record
- worker-profiles - supporting, active-record
- fleet-runtime - supporting, active-record

Deployables: 1 of a target of 1-1. Why not more: the design requires 1 binary (D9, FM-34), the ledger has 1 writer process, and 3 contexts are joined by synchronous calls.

## Review page and diagrams

- `ddd/review.html` - the review page. It lists 61 items that need a person, and 15 that stop the work (the 15 open decisions).
- `ddd/diagrams/` - 19 SVG and 19 PNG: context map, core domain chart, event storm, C4 context, teams and deployables, flows F1 to F7, aggregates for task-execution and integration. `ddd/diagrams/decisions/` holds 5 option diagrams (SVG and PNG).
- Blueprint final render: `ddd export blueprint ddd --deliver` wrote `ddd/diagrams/final.architecture.json` and 7 sequence specs `ddd/diagrams/final-F1.sequence.json` to `final-F7.sequence.json`. It rendered `ddd/diagrams/final-F1.html` to `final-F7.html` with 0 errors each.
- **`ddd/diagrams/final.html` does not exist.** The architecture render failed with exit 1 on 2 runs. The error text:

  ```
  blueprint deliver architecture ddd/diagrams/final.architecture.json failed (exit 1); previous ddd/diagrams/final.html left untouched
    [delivery/receipt-invalid] Could not parse the successful artifact-check receipt: Unterminated string in JSON at position 65308 (line 186 column 100)
  ```

  The failure is in the Blueprint renderer receipt, not in the DDD workspace. The architecture spec is in `ddd/diagrams/final.architecture.json`.

## Validation

`ddd validate ddd`: RESULT OK. 0 errors, 2 warnings.

- `[connect] messages[start-agent-process] is a command not present in discover - new message or renamed id?`
- `[organise] team 'core-team' has high cognitive load - consider splitting or a platform team`

`ddd contracts check ddd`: RESULT OK. 0 errors, 0 warnings.

## Blocking open questions

None. 0 of the 42 open questions are blocking.

## Open decisions

Each decision has no `chosen` value. The operator decides each one. The recommendation comes from the step that recorded it.

### understand D2: Do the 178 existing OS-specific sites in production code move behind the new platform layer?

- A: Only new fleetd code uses the platform layer. Existing sites stay.
- B: Move the existing sites that fleetd calls behind the layer as each phase touches them.
- C: Move all 178 sites behind the layer in 1 dedicated phase.
- Recommendation: B.

### discover D1: Where are the pivotal events that cut the fleetd timeline into phases?

- A: 4 pivotal events (assumptions approved, plan committed, task done, idea tasks landed) and 5 phases: clarifying, planning, executing, landing, integrating.
- B: 2 pivotal events (assumptions approved, task done) and 3 phases: shaping, building, integrating.
- C: 5 pivotal events (task done split from verification) and 6 phases.
- Recommendation: A.

### discover D2: Where do daemon supervision and adapter conformance sit on the timeline?

- A: At the end of the executing phase, because they act on running attempts and profiles.
- B: In their own operating phase outside the idea flow.
- C: Off the board, as a missing hotspot only.
- Recommendation: A (recorded as assumption A3).

### decompose D1: Is the verifier a context of its own, or a module of task-execution?

- A: Module of task-execution. Verification is an aggregate beside task and attempt.
- B: Context of its own (verification) that task-execution calls after attempt-exited.
- C: Merge verification into integration, because both run gates.
- Recommendation: A.

### decompose D2: Which context owns worker questions: ideas or task-execution?

- A: ideas owns the question aggregate.
- B: task-execution owns the question aggregate.
- C: A context of its own (operator attention) for questions, approvals and escalations.
- Recommendation: A.

### decompose D3: Are worker profiles and adapters a context of their own, or a part of task-execution?

- A: Own context worker-profiles.
- B: Module of task-execution.
- C: Part of fleet-runtime, as a platform concern.
- Recommendation: A.

### decompose D4: Is worktree custody a context of its own, or a part of integration?

- A: Own context worktree-custody.
- B: Part of integration.
- C: Part of task-execution, beside the attempt.
- Recommendation: A.

### strategize D1: Is the ideas subdomain core or supporting?

- A: Supporting, active-record. A flat idea state machine in code. The planning judgment stays in the sub-orchestrator model.
- B: Core, domain-model. The idea and plan are aggregates with invariants.
- C: Split. Plan validation is a supporting module with domain-model rules. Idea intake and questions stay active-record.
- Recommendation: A.

### strategize D2: Do the core subdomains use domain-model with an audit event table, or event-sourced-domain-model?

- A: domain-model. State rows are the truth. transition() writes the row and 1 event in 1 transaction.
- B: event-sourced-domain-model. The events table is the truth and the other tables are rebuilt from it.
- C: Mixed. Event-sourced for task and attempt only, domain-model for integration.
- Recommendation: A.

### connect D2: Which context owns the fleet-run wrapper, the per-attempt program that writes heartbeat.json and exit.json?

- A: task-execution.
- B: worker-profiles.
- C: fleet-runtime.
- Recommendation: B. Note: code D3 asks the same question and recommends C. Decide both together.

### organise D2: Where does docs sync and adapter conformance run: inside the 1 deployable or as its own job binary?

- A: A horch subcommand inside the 1 deployable that fleetd schedules daily and on a CLI version change.
- B: A separate agent-docs-sync job binary, widened from crates/herdr-docs-sync.
- C: A development tool in the repository only. The conformance results ship inside each horch release.
- Recommendation: A.

### define D1: Which business model fits the 2 core contexts, task-execution and integration?

- A: cost-reducer.
- B: revenue-generator.
- C: engagement-creator.
- Recommendation: A.

### define D2: Which domain role fits the ideas context?

- A: specification-model plus approver.
- B: execution-context.
- C: approver alone.
- Recommendation: A.

### code D3: Which context owns the fleet-run wrapper logic (connect D2)?

- A: task-execution.
- B: worker-profiles.
- C: fleet-runtime. It asks worker-profiles for the CLI event map.
- Recommendation: C. This conflicts with the connect D2 recommendation (B).

### contracts D1: Which field-name convention do the file and command-channel payloads use when the design gives no example of the file?

- A: Keep the connect payload names in camelCase for each file without a design example.
- B: Use the design convention everywhere: snake_case and short id names.
- C: Use camelCase everywhere, and rename the design example fields too.
- Recommendation: B.

## Review order

Each line quotes the step's own `plain_words.riskiest`.

1. `ddd/glossary.md` - do the words match how you talk about the fleet? Discover says: "The words 'ready', 'landed' and 'done' each mean 2 or 3 different states. If later steps merge them, a status can say work is safe when it has not reached the primary branch."
2. `ddd/03-decompose/subdomains.md` - are the boundaries right? Each later step depends on them. "If worker profiles is not its own part, the rule that strips ANTHROPIC_API_KEY from every launch has no single owner, and 1 missed launch path bills the API key."
3. `ddd/04-strategize/core-domain-chart.md` - is the core really the core? "If plan checks in the ideas part grow into many rules, the simple design there will leak bugs, and a task could start without an approved plan."
4. `ddd/06-organise/team-topology.md` - deployable count and module walls. "1 team owns 6 parts, 2 of them core. If nothing checks the module walls, the parts tangle and a later split costs much more. Where docs sync runs is still open (D2)."
5. `ddd/07-define/` canvases - business decisions and verification metrics. "The riskiest part is the GitHub and herdr links: no earlier step gave them messages, so we added 2 GitHub rows by hand and asked the connect step to confirm."
6. `ddd/08-code/implementation-plan.md` - invariants and aggregate sizes. "If a phase forgets to remove its old path, the old watchdog and the new fleetd can both act on 1 task. The owner of the per-attempt wrapper program is still open and must be chosen before Phase 4."

## Next

All 9 steps are done. Decide the 15 open decisions first. Start with connect D2 and code D3, because the code step says the wrapper owner must be chosen before Phase 4.

Then implement: hand `ddd/08-code/implementation-plan.md` to `software:create-plan`, with `ddd/09-contracts/contracts.md` as the message spec. Then use TDD for each slice.

To change a design decision, run ddd from that step. Each later step then runs again.
