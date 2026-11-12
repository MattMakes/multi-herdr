# Understand - fleetd: the horch fleet control plane

| | |
|---|---|
| Project | `fleetd` |
| Mode / depth | auto / standard |
| Lifecycle | brownfield (horch runs today; fleetd is a set of changes to it) |
| Produced | 2026-11-12 by ddd-understand |
| Inputs | docs/specs/fleetd/design.md, docs/specs/fleetd/amendments.md (A19 to A22), the revision context of round 2, docs/architecture.md, docs/specs/fleetd/preservation-04-behavior.md, docs/specs/fleetd/preservation-05-contracts-and-values.md, docs/specs/fleetd/preservation-06-wiring-and-platform.md, docs/specs/fleetd/research-01-industry-practice.md, the ticket and the DDD context file of the run |

## In plain words

We wrote down why fleetd exists, who it serves, what counts as success and how big it may get.

**Decided:** fleetd is a change to horch, not a new product. It ships inside the 1 horch binary, for 1 operator and 1 team. Success is 4 numbers: idle tasks caught in under 10 minutes, 0 lost work, 0 tasks outside an approved plan, 0 attempts burned by usage limits. All 178 old OS-specific code sites move behind the platform layer in 1 phase, Phase P, before Phase 1 (operator decision D16).

**Assumed:** The design gives no number for using the subscription fully, so we used a proxy: no usage window resets with usage left while work is ready.

**Riskiest:** If the proxy for full usage is wrong, later steps may build pacing toward the wrong target.

---

## 1. System

**One-liner.** A supervised daemon inside horch that takes rough ideas from the operator, runs AI coding agents on them, and keeps every task, worktree and branch accounted for until the work reaches the primary branch.

**Problem.** The operator runs a fleet of AI coding agents with horch. Tasks get forgotten, workers do not find work or sit idle on it, finished work is never reported, the watchdog misses updates, stacked ideas merge in 1 orchestrator conversation, and dead workers leave orphaned worktrees. Each failure comes from a job that horch still leaves to an LLM.

**Why now.** Daily stalls and lost worktrees cost subscription usage and operator attention. horch already has the parts (recorded PIDs, a supervisor per worker, worktrees per task, finish steps), and a design review on 2026-11-11 and 2026-11-12 approved the design.

**Success in one sentence.** No task stalls silently: an idle task is retried or escalated in under 10 minutes (G1), with 0 lost work (G2).

## 2. Business Model Canvas

| Block | Content |
|---|---|
| Customer segments | The operator: a solo developer who runs a fleet of AI coding agents on 1 machine. Friends who install horch and get new releases through horch self-update |
| Value propositions | No task stalls silently: every task ends verified done or escalated with a reason. No paid-for work is lost: every worktree and branch is known and rescued before removal. Rough ideas in, approved plans out: 1 idea at a time, nothing built before approval. The subscription usage is used fully across agent CLIs and providers. Parallel features reach the primary branch through 1 pull request each, with conflicts found early |
| Channels | The horch CLI (horch idea add, horch ideas, horch status, horch idea approve); herdr panes, sidebar and notifications; the needs-you folder; GitHub releases and horch self-update |
| Customer relationships | Self-service from the terminal; an interview with the operator in the pane of each sub-orchestrator; quick approval of written assumptions |
| Revenue streams | None: horch is the operator's own tool; the return is operator throughput (assumed) |
| Key resources | The ledger (1 SQLite file, 1 writer); the files of the fleet workspace; git repositories, worktrees and rescue refs; the worker profiles in teammates/; subscription usage of each provider pool; the host machine |
| Key activities | Dispatch ideas through a fixed state machine; plan 1 idea at a time with operator approval; bind tasks to workers and supervise attempts with leases; verify each task on disk; lease, reconcile and rescue worktrees; land task branches and integrate features; pace dispatch to each usage window; keep each agent CLI adapter conformant |
| Key partners | herdr; agent CLI vendors (Anthropic, OpenAI, Google, OpenCode, pi, Prime); OS service managers (launchd, systemd, Windows services and Task Scheduler); GitHub |
| Cost structure | Subscription usage of each provider pool; machine resources (disk for worktrees, CPU and memory for agents); operator attention for interviews, approvals and escalations |

### 2b. Value proposition → capability → beats

| Value proposition | Rests on | Beats |
|---|---|---|
| No task stalls silently: every task ends verified done or escalated with a reason | cap-task-supervision, cap-task-verification, cap-operator-attention | alt-horch-orchestrator-conversation |
| No paid-for work is lost: every worktree and branch is known and rescued before removal | cap-worktree-custody, cap-fleet-record | alt-horch-orchestrator-conversation, alt-hand-run-agents |
| Rough ideas in, approved plans out: 1 idea at a time, nothing built before approval | cap-idea-intake, cap-idea-planning | alt-horch-orchestrator-conversation |
| The subscription usage is used fully across agent CLIs and providers | cap-usage-pacing, cap-agent-cli-adaption | alt-hand-run-agents |
| Parallel features reach the primary branch through 1 pull request each, with conflicts found early | cap-feature-integration | alt-horch-orchestrator-conversation, alt-hosted-merge-queue |

## 3. Goals

| Id | Goal | Metric | Target | Horizon |
|---|---|---|---|---|
| G1 | No task stalls silently: every task ends verified done or escalated to the operator with a reason | Minutes from a worker going idle on a held task until fleetd retries or escalates it; count of tasks left assigned with an expired lease | Under 10 min (about 7 min by design); 0 tasks left assigned with an expired lease | Release of migration Phase 2 |
| G2 | No work that a worker paid for is lost | Count of worktrees or branches removed without being landed, merged or saved on a verified rescue ref | 0 | Release of migration Phase 3 |
| G3 | Each idea is built only from its own approved, validated plan and reaches the primary branch through its feature | Count of tasks outside an approved, validated plan, plus count of processes that planned 2 ideas at the same time | 0 | Release of migration Phase 6 |
| G4 | The fleet uses the subscription usage fully and gets as much work done as possible | Pool windows that reset with usage left while a task was ready; attempts burned by a usage limit or a provider outage | 0 attempts burned by a usage limit; target for unused usage per window to confirm (Q1) | Release of migration Phase 2 |

## 4. Impact map

- **G1 - No task stalls silently**
  - **operator** (person, primary) - today finds stalls by watching panes
    - **I1** Stops checking panes to find workers that stalled, sit idle or never reported
      - **D1** (must) A task whose worker stops making progress loses its lease within minutes and is retried or escalated
      - **D2** (should) 1 status view of every idea, task, lease and worker with its last signal
      - **D3** (should) Each escalation lands in needs-you, in horch ideas and as a herdr notification
  - **worker** (system, secondary) - today can end its turn without reporting
    - **I2** Ends every attempt with a result file, or exits so the attempt is retried from its committed work
      - **D4** (must) A stop gate that refuses to end a turn without a result file
      - **D5** (must) The task file and the start note are written before the worker launches
- **G2 - No work that a worker paid for is lost**
  - **operator** (person, primary)
    - **I3** Stops recovering or redoing work from orphaned worktrees and lost branches
      - **D6** (must) A rescue ref is written and read back before any worktree is removed
      - **D7** (should) horch worktrees and horch worktree restore
  - **worker** (system, secondary)
    - **I4** Works only in the worktree fleetd leased to it and never creates or removes a worktree
      - **D8** (must) Each worktree is leased in the ledger before it exists, and only fleetd creates or removes it
  - **friend** (person, secondary) - gets every phase at once through self-update
    - **I5** Gets each new phase through horch self-update, and the running daemon restarts on the new binary with no lost task
      - **D9** (must) horch self-update restarts the running daemon on the new binary
- **G3 - Each idea is built only from its own approved, validated plan**
  - **operator** (person, primary)
    - **I6** Drops rough ideas into a queue at any time instead of typing them into 1 orchestrator conversation
      - **D10** (must) horch idea add, with the --direct fast lane and --to for an open feature
      - **D11** (could) The horch ideas board
    - **I7** Approves each idea's written assumptions before any spec or task exists
      - **D12** (must) horch idea approve, which only works outside any fleet process and records the hash of the assumptions seen
    - **I9** Merges 1 pull request per feature instead of reconciling task branches by hand
      - **D15** (must) Task branches land on their feature branch in bundles behind the fast gate, and each feature opens 1 pull request
  - **sub-orchestrator** (system, secondary) - plans for 1 feature
    - **I8** Plans 1 idea at a time from its idea folder and decides worker questions itself unless they change product direction, scope or a trade-off the operator owns
      - **D13** (must) A fresh sub-orchestrator process for each phase, with the idea folder as its only memory
      - **D14** (should) Worker questions with an owner, a state and a time limit
- **G4 - The fleet uses the subscription usage fully**
  - **worker** (system, secondary)
    - **I10** Moves to its fallback persona or waits for the pool to reset when it hits a usage limit, instead of being reaped
      - **D16** (must) A required fallback persona for every worker persona, and the waiting_quota state
  - **operator** (person, primary)
    - **I11** Stops sizing parallel work by hand and leaves pacing to fleetd
      - **D17** (should) Dispatch paced to each pool's usage window and paused only by the disk and load health guards
  - **provider-pool** (organisation, off-stage) - its usage limit or outage can block a worker

| Id | Actor | Kind | Role | Description | Goals |
|---|---|---|---|---|---|
| operator | Operator | person | primary | Drops ideas, answers interviews, approves assumptions, receives escalations, merges each feature's pull request | G1, G2, G3, G4 |
| sub-orchestrator | Sub-orchestrator | system | secondary | Interactive Claude Code, 1 per feature, a fresh process per phase, plans 1 idea at a time | G3 |
| worker | Worker | system | secondary | Any agent CLI with a named profile, 1 attempt of 1 task in a leased worktree; also runs judge, resolver and triage tasks | G1, G2, G4 |
| friend | Friend | person | secondary | Installs horch and gets each release through horch self-update | G2 |
| provider-pool | Provider pool | organisation | off-stage | A model provider's usage window; a limit or outage can block a worker | G4 |

## 5. Capabilities

| Id | Capability | What it means here | Evolution |
|---|---|---|---|
| cap-idea-intake | Idea intake and dispatch | Capture rough ideas with no AI, assign each to 1 feature, move each idea through the fixed idea state machine | custom |
| cap-idea-planning | Idea clarification and planning | Research, interview, written assumptions, operator approval, spec, requirements and a plan that passes validation | custom |
| cap-task-supervision | Task supervision | Push dispatch, earned leases, liveness from outside the model, the stall ladder, retries and escalation | custom |
| cap-task-verification | Task verification | Done from facts on disk: acceptance checks in the worker's sandbox, today's review scripts for code kinds, a paired judge for judgment kinds | custom |
| cap-worktree-custody | Worktree custody and rescue | Lease each worktree before it exists, reconcile it with git, save its work on a verified rescue ref before removal | custom |
| cap-feature-integration | Feature landing and integration | Gated bundles onto the feature branch, trial merges between features, stacking and rebasing, 1 pull request per feature | custom |
| cap-usage-pacing | Usage pacing and fallback | Pace dispatch to each pool's usage window, fallback persona or waiting_quota on a limit, pause only on health guards | custom |
| cap-agent-cli-adaption | Agent CLI adaption and conformance | 1 profile into exact flags, env and files per CLI; logins and env scrubs; docs sync and conformance checks | custom |
| cap-fleet-record | Fleet record keeping | 1 event log of every state change with its cause, 1 writer, legal transitions, forward-only schema changes | custom |
| cap-operator-attention | Operator attention and escalation | Everything that needs the operator in 1 place, reminders in working hours, park what waits too long | custom |
| cap-host-services | Host platform services | Service restart, hang detection, process facts, group stop, file watching, sandboxing, sleep and wake, machine probe on 3 OSes; from Phase P also process identity, signals, file locks, permissions, paths, links, shell quoting, local sockets and pipes, child detach | commodity |
| cap-pane-hosting | Terminal pane hosting | Terminals, agent names, screen-based status and the sidebar (herdr) | product |

## 6. Constraints

| Id | Kind | Constraint |
|---|---|---|
| C1 | technical | fleetd is a subcommand of the single horch binary: 1 deployable unit, so the CLI and the daemon always have the same version |
| C2 | organisational | Absolute operator rule: ANTHROPIC_API_KEY is never used and is removed from every child, pane line, detached child, updater and probe; every claude run uses the subscription login. agy children lose the Google API keys |
| C3 | technical | macOS, Linux and Windows are first-class. Production code of the workspace has no OS cfg outside the platform layer; 1 platform interface, 1 module per OS, explicit not-supported values |
| C4 | technical | Exactly 1 process writes the ledger (SQLite WAL, ~/.horch/ledger.sqlite3). Forward-only migration at daemon start, backup first (3 kept), newer schema refused, no migration from a development build |
| C5 | organisational | Only the operator approves an idea's assumptions, with a command run outside any fleet process |
| C6 | technical | fleetd keeps the behaviors of the Preservation analysis (04 W6, F9, F7, F5, L5, L9, W8, W9, F11, S7, L6, L8, S8, S10, M5 and the rest) |
| C7 | technical | fleetd keeps the contract surface: exit codes 0 to 3, the mail contract, UNLANDED and UNREGISTERED reports, horch self-update and release assets, ticket format C1, project facts |
| C8 | technical | Fixed values stay: land lock 1 h with 5 tries, check timeout 20 min (max 24 h), git push 10 min and other network calls 3 min, code-sweep at most 2 requeues, review rounds 1 and 3 |
| C9 | timeline | 9 migration phases (Phase 0, Phase P, then 1 to 7) in order; each released to every user at once when its gate test passes; Phase 0 keeps a list of expected failures |
| C10 | contractual | Agent CLI vendors set the terms: usage windows and limits per pool, Codex user hooks need trust, a CLI release is used only after the update cooldown of more than 1 week |
| C11 | technical | The only bytes typed into a terminal are 1 fixed doorbell line; all hand-off is through atomically written files |
| C12 | technical | Each agent works inside a context limit of 256k tokens |
| C13 | budget | No spend budget and no cap on features, sub-orchestrators or workers; only disk and load health guards pause new dispatch |
| C14 | technical | herdr holds no state fleetd depends on; after a herdr restart fleetd rebuilds from the ledger and files |

## 7. Non-goals

- Running the fleet across several machines: 1 host and 1 disk in version 1
- Replacing herdr's interface: herdr stays the place where the operator watches panes
- Agents merging to protected branches or deploying: integration stops at a pull request for the operator to merge
- A syntax-aware merge driver in version 1
- 1 pull request for several features
- A spend budget or a cap on parallel work
- An AI that decides what happens next to an idea: the dispatcher is plain code

## 8. Existing systems

| System | Role | Will |
|---|---|---|
| horch task ledger (schema v7) and its finish steps | Tasks, claims, worker rows, worktrees table, check, sweeps and land | integrate |
| horch ledger watch (the lazily started watchdog) | Edge-triggered stall checks that mail the orchestrator | replace |
| The long-lived orchestrator conversation | Receives ideas, creates tasks, reads worker mail and watchdog wakes | replace |
| horch worker supervisor (run_worker) | Per-worker supervisor thread; becomes the fleet-run wrapper | integrate |
| teammates roster | Worker profiles as teammate frontmatter | integrate |
| herdr-docs-sync | Mirrors herdr docs; widened to every agent CLI with conformance | replace |
| herdr | Pane runtime, agent status, notifications | integrate |
| Agent CLIs: Claude Code, Codex CLI, Antigravity, OpenCode, pi, Prime | Run the agents of the fleet | integrate |
| git | Worktrees, branches, refs, merge-tree trial merges | integrate |
| GitHub and gh | Pull requests and release assets | integrate |
| OS service managers: launchd, systemd, Windows service and Task Scheduler | Keep fleetd and herdr running | integrate |
| Provider quota probes | Read usage limits through each CLI's own client | integrate |

### 8a. Alternatives today

| Id | Alternative | Kind | Used by | Does well | Does badly | Competes with |
|---|---|---|---|---|---|---|
| alt-horch-orchestrator-conversation | Today's horch with 1 long-lived orchestrator conversation | in-house-tool | operator, worker, sub-orchestrator | Already runs; recorded PIDs, supervisor per worker, worktrees per task, finish steps, review scripts | Tasks exist only if the LLM creates them; idle claims; done is a model action; lazy watchdog; ideas blur; orphaned worktrees | cap-idea-intake, cap-idea-planning, cap-task-supervision, cap-task-verification, cap-worktree-custody, cap-feature-integration, cap-operator-attention |
| alt-hand-run-agents | Agent CLIs run by hand in terminal panes | manual-process | operator | No setup; full steering; each CLI's own login | Does not scale; usage windows go unused; no record of worktrees, leases or results | cap-usage-pacing, cap-agent-cli-adaption, cap-worktree-custody |
| alt-hosted-merge-queue | Hosted merge queues (GitHub merge queue, bors, Mergify) | competitor | operator | Proven batching, bisection, testing on the latest base | Work on pull requests on hosted CI, not local agent task branches; no early conflict detection between features | cap-feature-integration |

## 9. Scale target

- Deployable units: 1-1 · Teams: 1 · Users: 1 operator per host, plus friends on their own hosts
- Notes: fleetd is a subcommand of the single horch binary. The operator and the fleet of AI agents are 1 team.

## 10. Assumptions and open questions

| Id | Assumption | Confidence |
|---|---|---|
| A1 | Lifecycle is brownfield: the design is a set of changes to horch, not a rewrite | high |
| A2 | The operator is the single primary actor; friends are a secondary segment with no approval role | medium |
| A3 | horch earns no revenue; its return is operator throughput | medium |
| A4 | G4 uses a proxy target: no pool window resets with usage left while a task is ready; a usage limit burns 0 attempts | low |
| A5 | Judge, resolver and triage agents are workers running a task kind, not separate actors | medium |
| A6 | Control-plane capabilities are custom; pane hosting is product (herdr); host platform services are commodity | medium |
| A7 | Alternatives are inferred from the design and research: today's horch, agents by hand, hosted merge queues | medium |
| A8 | /fleet paths are a neutral layout mapped to ai_docs/plans/<run>/, the state root and ~/.horch/ledger.sqlite3 | high |
| A9 | When the design and another source disagree, the design wins | high |
| A10 | Mode, depth, 1 deployable and 1 team are fixed by the run | high |
| A11 | All 178 OS-specific production sites move behind the platform layer in Phase P, between Phase 0 and Phase 1, with no change of behavior; test cfg sites stay (D16, A19) | high |

| Id | Question | Blocking | Owner |
|---|---|---|---|
| Q1 | What number counts as full use of the subscription usage for G4? | no | operator |
| Q3 | Which Windows transport carries the fleetd command channel? | no | live test |
| Q4 | How are Codex hooks pre-trusted for unattended workers? | no | live test |
| Q5 | Does a private HOME fully isolate agy and keep its login? | no | live test |
| Q6 | herdr report-agent precedence, custom key command fields, and preview 0.9.4 against 0.9.5 | no | live test |
| Q7 | Resolver task size limit, and accuracy of a structural merge tool on this repository | no | live test |
| Q8 | Which command is the fast gate of this repository (no unit-only command today)? Proposed answer (orchestrator, D15): the workspace tests without the crate horch-e2e; a run must confirm it | no | operator |

Answered: Q2 (the 178 OS-specific sites) is answered by the operator (D16), option C of D2, and is listed under `deprecated`.

Decisions: D1 (chosen A, brownfield change to horch, cites design "Keep, change, add" and amendment D9). D2 (chosen C by the operator, D16: all 178 sites move in 1 dedicated phase, Phase P, between Phase 0 and Phase 1; the step recommended B, moving sites as each phase touches them).

### 10a. Notes for downstream steps

| Id | For | Kind | Note |
|---|---|---|---|
| N1 | discover, decompose, define | language | 'Task', 'attempt' and 'lease' are 3 things: planned unit; 1 run of 1 worker; time-limited hold renewed only on earned activity |
| N2 | discover, decompose, define | language | 'Feature' = 1 sub-orchestrator, 1 feature branch, 1 pull request; task branch lands on feature branch; feature branch reaches the primary branch (not always main) |
| N3 | discover, decompose | language | 'Orchestrator' today is 1 LLM conversation; in fleetd it is replaced by the dispatcher (code) plus 1 sub-orchestrator per feature |
| N4 | discover, decompose, define | language | 'Done' is set only by the verifier; 'landed' and 'merged' are later facts read from git |
| N5 | decompose, connect, define, code | boundary | fleetd is the only ledger writer; everything else writes files or sends requests on the command channel |
| N6 | decompose, organise, code | decision | 1 deployable unit and 1 team; contexts become modules inside horch-core |
| N7 | discover | process | Idea state order, with the --direct shortcut and the parked and cancelled side states |
| N8 | discover | process | Task branch landing order, with conflicted, gate_failed and abandoned |
| N9 | discover | process | Worktree order reserved, created, active, landing or orphaned; rescue before removal |
| N10 | discover, decompose | process | A usage limit is not a stall; fallback or waiting_quota; lease time counts only while awake |
| N11 | discover, define | process | Worker questions go to the sub-orchestrator; only product direction, scope or trade-offs go to the operator |
| N12 | discover, define | language | Judgment kinds, judge kind, resolver task and code kinds |
| N13 | decompose, organise, code | boundary | The platform layer is the leaf crate crates/horch-platform, not a context: no OS cfg outside it in production code; 1 module per OS; explicit not-supported; arch_scan enforces it |
| N14 | strategize | decision | Pane hosting: herdr wins, do not build. Host platform services: the OS wins, only wrap |
| N15 | discover, define, code | risk | Every preservation row must be owned by a context and kept |
| N16 | define, code | risk | ANTHROPIC_API_KEY never reaches a child process |
| N17 | define, code | other | /fleet paths are neutral and map to horch paths; fleet helper commands are horch subcommands |
| N18 | discover, decompose, organise, code | decision | Phase P (D16, A19): create crates/horch-platform, move all 178 production OS sites with no change of behavior; gate PL-2 and PL-6 |
| N19 | discover, code | process | 9 phases: 0, P, 1 to 7; Phase 0 keeps a list of expected failures; CI runs the tests on Windows from Phase 0 |
| N20 | discover, connect, define | language | A state name is always written with its entity kind; a task is done, its task branch is landed (A20) |

## 11. Run log

- Read: the 5 run sources, research-01, the ticket and the DDD context file.
- Resolved: auto mode and standard depth from the run; scale 1 deployable and 1 team from the run; lifecycle brownfield because horch runs today and the design changes it.
- Recorded: 10 assumptions, 8 open questions (0 blocking), 2 decisions (1 open), 17 notes for downstream.
- Revision round 2 (2026-11-12): recorded D2 = C (operator, D16); answered Q2; added A11 and N18 to N20; changed C3, C9, cap-host-services, N13 and Q8 for amendments A19 to A22. Now: 11 assumptions, 7 open questions (0 blocking), 2 decisions (0 open), 20 notes.
