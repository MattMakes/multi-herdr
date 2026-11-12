# DDD coverage summary - fleetd: the horch fleet control plane, round 2

Workspace: `ddd/`. Mode: auto. Depth: standard. Source of the design: `docs/specs/fleetd/design.md`, with amendments A19 to A22.

Read this file first. Then open `ddd/review.html` in any browser. It needs no server.

## Decided in round 2

Each decision of round 2 has a `chosen` value now. D16 is a decision of the operator. D15 is a decision of the orchestrator, and the operator can reverse it.

Marked [DIFFERS]: the choice is not the recommendation of the step. These come first.

1. [DIFFERS] understand D2: Do the 178 existing OS-specific sites in production code move behind the new platform layer? Option C: all 178 sites move in 1 dedicated phase, Phase P, between Phase 0 and Phase 1 (A19). The step recommended B. Operator, D16.
2. [DIFFERS] discover D2: Where do daemon supervision and adapter conformance sit on the timeline? Option B: their own operating phase outside the idea flow. The step recommended A. Orchestrator, D15.
3. [DIFFERS] strategize D1: Is the ideas subdomain core or supporting? Option C: split. Plan validation is a domain-model module, and intake and questions stay active-record. The step recommended A. Orchestrator, D15.
4. [DIFFERS] code D3: Which context owns the fleet-run wrapper logic? Option B: worker-profiles. The code step recommended C (fleet-runtime). Orchestrator, D15.
5. discover D1: Where are the pivotal events that cut the timeline into phases? Option A: 4 pivotal events and 5 phases. Orchestrator, D15.
6. decompose D1: Is the verifier a context of its own, or a module of task-execution? Option A: a module of task-execution. Orchestrator, D15.
7. decompose D2: Which context owns worker questions? Option A: ideas. Orchestrator, D15.
8. decompose D3: Are worker profiles and adapters a context of their own? Option A: its own context worker-profiles. Orchestrator, D15.
9. decompose D4: Is worktree custody a context of its own? Option A: its own context worktree-custody. Orchestrator, D15.
10. strategize D2: Domain model with an events table, or event-sourced? Option A: domain model with an events table. Orchestrator, D15.
11. connect D2: Which context owns the fleet-run wrapper? Option B: worker-profiles. Orchestrator, D15.
12. organise D2: Where do docs sync and adapter conformance run? Option A: a `horch` subcommand that fleetd schedules. Orchestrator, D15.
13. define D1: Which business model fits the 2 core contexts? Option A: cost-reducer. Orchestrator, D15.
14. define D2: Which domain role fits the ideas context? Option A: specification-model plus approver. Orchestrator, D15.
15. contracts D1: Which field-name convention do the payloads use? Option B: the snake_case names of the design, in each file. Orchestrator, D15.

The ticket names 3 decisions that differ from their recommendation. The JSON shows 4: understand D2 also differs, because the step recommended B and the operator chose C.

## Steps

All 9 steps are `done`. No step is pending, stale or draft.

| Step | Status | Open Q | Blocking | Open decisions | Main artifact | Counts |
|---|---|---|---|---|---|---|
| 01 understand | done | 7 | 0 | 0 | `ddd/01-understand/understand.md` | 4 goals, 12 capabilities |
| 02 discover | done | 1 | 0 | 0 | `ddd/02-discover/event-storm.md` | 87 events, 4 pivotal, 15 hotspots, 4 scenarios, 6 phases |
| 03 decompose | done | 0 | 0 | 0 | `ddd/03-decompose/subdomains.md` | 6 subdomains, 6 contexts, 13 relationships |
| 04 strategize | done | 0 | 0 | 0 | `ddd/04-strategize/core-domain-chart.md` | core: task-execution, integration. All 6 are build. |
| 05 connect | done | 0 | 0 | 0 | `ddd/05-connect/message-flows.md` | 9 flows, 51 messages, 9 coupling concerns, 0 sagas |
| 06 organise | done | 0 | 0 | 0 | `ddd/06-organise/team-topology.md` | 1 team, 1 deployable, target 1-1, within target. Style: modular monolith. |
| 07 define | done | 0 | 0 | 0 | `ddd/07-define/<context>/bounded-context-canvas.md` x6, `ddd/07-define/system-context.md` | 6 canvases, 12 quality attributes |
| 08 code | done | 1 | 0 | 0 | `ddd/08-code/implementation-plan.md` | 5 aggregates: Plan (ideas), Task, Verification (task-execution), Feature, TaskBranch (integration). Language: Rust. |
| 09 contracts | done | 0 | 0 | 0 | `ddd/09-contracts/contracts.md` | 30 contracts: 22 published, 8 inherited from external systems. 24 in-process messages need no wire contract. |
| glossary | | | | | `ddd/glossary.md` | 20 shared terms, 2 with context-specific meanings (Feature, Lease), 8 coined in code |

Totals: 9 open questions, 0 blocking, 0 open decisions.

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
- `ddd/08-code/code.json`, `ddd/08-code/implementation-plan.md`, 1 `design.md` per context, `ddd/08-code/ideas/aggregate-canvas-plan.md`, `ddd/08-code/task-execution/aggregate-canvas-task.md`, `ddd/08-code/task-execution/aggregate-canvas-verification.md`, `ddd/08-code/integration/aggregate-canvas-feature.md`, `ddd/08-code/integration/aggregate-canvas-task-branch.md`, `ddd/08-code/decisions/` (6 option specs)
- `ddd/09-contracts/contracts.json`, `ddd/09-contracts/contracts.md`, `ddd/09-contracts/schemas/` (37 files), `ddd/09-contracts/examples/` (74 files)

## Bounded contexts

6 contexts in 1 deployable (`horch`):

- task-execution - core, domain-model
- integration - core, domain-model
- ideas - supporting, domain-model for plan validation, active-record for intake and questions (strategize D1)
- worktree-custody - supporting, active-record
- worker-profiles - supporting, active-record. Owns the fleet-run wrapper (connect D2, code D3).
- fleet-runtime - supporting, active-record

The platform layer is the leaf crate `crates/horch-platform`. Each context calls it. It is not a context (A19).

Deployables: 1 of a target of 1-1. Why not more: the design requires 1 binary (D9, FM-34), the ledger has 1 writer process, and task-execution, worker-profiles and fleet-runtime are joined by synchronous calls. An `arch_scan` rule enforces the context boundaries.

## Review page and diagrams

- `ddd/review.html` - the review page, 4520 KB. It lists 84 items that need a person, and 0 that stop the work.
- `ddd/diagrams/` - 17 SVG and 17 PNG: context map, core domain chart, event storm, C4 context, teams and deployables, flows F1 to F9, aggregates for ideas, task-execution and integration. `ddd/diagrams/decisions/` holds 5 option diagrams (SVG and PNG).
- `ddd/diagrams/final.html` - the Blueprint final render, 851163 bytes, 0 errors. Title: "fleetd: the horch fleet control plane Diagram". It shows 15 nodes (the 6 contexts, the horch store and 8 external actors and systems) in 1 boundary, "horch binary", with 55 connections.
- `ddd/diagrams/final-F1.html` to `final-F9.html` - 1 sequence render for each flow, 0 errors each. The specs are `ddd/diagrams/final.architecture.json` and `ddd/diagrams/final-F1.sequence.json` to `final-F9.sequence.json`.

## Validation

`ddd validate ddd`: RESULT OK. 0 errors, 4 warnings.

- `[organise] team 'core-team' has high cognitive load - consider splitting or a platform team`
- `[contracts] entries[attempt-exited] is an in-process call (organise co-locates its parties in one deployable) - a wire contract invents a seam the build does not have; cover it with a code-level test instead`
- `[contracts] entries[task-escalated] is an in-process call` (same text)
- `[contracts] entries[heartbeat-written] is an in-process call` (same text)

`ddd contracts check ddd`: RESULT OK. 0 errors, 3 warnings. The 3 warnings are the same 3 in-process contracts: `attempt-exited`, `heartbeat-written` and `task-escalated`. 30 entries cover 27 cross-party messages. 24 messages stay inside the deployable `horch` and get no wire contract.

## Blocking open questions

None. 0 of the 9 open questions are blocking.

## Open questions

- understand Q1 (operator): "What number counts as full use of the subscription usage for G4: a share of each pool window consumed by its reset, or a count of ready tasks left waiting while a pool has usage?"
- understand Q3 (live test): Which Windows transport carries the fleetd command channel?
- understand Q4 (live test): How are Codex hooks pre-trusted for unattended workers?
- understand Q5 (live test): Does a private HOME fully isolate agy config, skills and MCP, and keep its login?
- understand Q6 (live test): Does herdr pane report-agent state override the screen-based status, which fields do herdr custom key commands take, and do the herdr facts hold for preview 0.9.4 against 0.9.5?
- understand Q7 (live test): Is 100 changed lines or 2 files the right size limit for a resolver task, and how accurate is a structural merge tool on the recorded conflicts?
- understand Q8 (operator): Which command is the fast gate of this repository? Proposed answer (orchestrator, D15): the workspace tests without the crate `horch-e2e`. A run must confirm it.
- discover Q3 (operator): Does an operator requeue also restore the single replan (H12)? Answered part (orchestrator, D15): an operator requeue sets the attempt count to 0, and a requeue by a review keeps its own counter.
- code Q3 (operator): Which context owns docs sync? The code step places `run-conformance-tests` in worker-profiles and plans docs sync in the Phase 7 slice with no owning context.

## Open decisions

None. Each decision of each step has a `chosen` value.

## Review order

Each line quotes the step's own `plain_words.riskiest`.

1. `ddd/glossary.md` - do the words match how you talk about the fleet? Discover says: "The words 'ready' and 'landed' each name 2 or 3 states. The design now makes each message name the entity with the state. If a later step drops the entity, a status can say work is safe when it has not reached the primary branch."
2. `ddd/03-decompose/subdomains.md` - are the boundaries right? Each later step depends on them. "If worker profiles is not its own part, the rule that strips ANTHROPIC_API_KEY from every launch has no single owner, and 1 missed launch path bills the API key."
3. `ddd/04-strategize/core-domain-chart.md` - is the core really the core? "If the simple parts of the ideas part (intake and questions) grow rules, the flat design there will leak bugs."
4. `ddd/06-organise/team-topology.md` - deployable count and module walls. "1 team owns 6 parts, 2 of them core. An arch_scan rule checks the module walls; if the rule is weak, the parts tangle and a later split costs much more."
5. `ddd/07-define/` canvases - business decisions and verification metrics. "The rescue ref that must exist before a branch rebase (I10) belongs to worktree-custody, but the rebase belongs to integration, and no message joins them yet (Q6). A wrong answer can lose the old tip of a branch."
6. `ddd/08-code/implementation-plan.md` - invariants and aggregate sizes. "Wrapping old modules means 2 code paths live side by side until each phase removes the old one. If a phase forgets to remove its old path, the old watchdog and the new fleetd can both act on 1 task. Phase P moves 178 OS-specific sites at once; its only safety net is that every test gives the same result before and after the move."

## Known faults of the tools

- `ddd/diagrams/final.html` is rendered with a patched copy of the plugin in `/tmp`. The copy changes 1 line of `skills/blueprint/scripts/check-render-output.mjs`: `process.exit(ok ? 0 : 1)` becomes `process.exitCode = ok ? 0 : 1`. The cause and the proof are in `docs/specs/fleetd/blueprint-render-failure.md`. The installed plugin is not changed. The installed plugin still fails this render on macOS.
- The renderers write the em dash into some Markdown and HTML files. Those files are not edited by hand.
- In `ddd/diagrams/c4-context.png`, the row of context labels under the system box runs past the right edge of the picture. The "Worker profiles and adapters" label is cut off, and the "Worktree custody" label is not visible.

## Next

All 9 steps are done. No decision is open. Answer the 4 operator questions (understand Q1, understand Q8, discover Q3, code Q3) when they block a phase. The 5 live-test questions are answered by runs.

Then implement: hand `ddd/08-code/implementation-plan.md` to `software:create-plan`, with `ddd/09-contracts/contracts.md` as the message spec. Then use TDD for each slice.

To change a design decision, run ddd from that step. Each later step then runs again.
