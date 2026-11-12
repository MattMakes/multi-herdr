# Team topology - fleetd: the horch fleet control plane

_Step 6 of 9 (organise) · mode auto · depth standard · produced 2026-11-12_
_Inputs: ddd/manifest.json, ddd/03-decompose/decompose.json, ddd/05-connect/connect.json, ddd/04-strategize/strategize.json, ddd/01-understand/understand.json, ddd/glossary.md, docs/specs/fleetd/design.md, docs/architecture.md_

## In plain words

We decided who owns each part of fleetd and how many separate programs ship.

**Decided:** 1 team (the operator and the AI agents) owns all 6 parts. All 6 parts ship in 1 program, the horch binary, as separate modules. The OS code sits in 1 library inside it (crates/horch-platform), and docs sync is a horch command inside it too.

**Assumed:** Each part keeps its own database tables and folders inside that 1 program, and no part edits another part's tables.

**Riskiest:** 1 team owns 6 parts, 2 of them core. An arch_scan rule checks the module walls; if the rule is weak, the parts tangle and a later split costs much more.

---

## Summary

- 1 team, 1 deployable, topology style `modular-monolith`; target 1-1, within target. The design fixes it: fleetd is a subcommand of the single horch binary (design: Amendments D9, FM-34).
- Core contexts `task-execution` and `integration` are owned by `core-team`, with the 4 supporting contexts. No context is bought or generic. herdr, the agent CLIs, git, GitHub and the OS service managers are external systems behind adapters, not deployables (A6).
- First extraction candidate: none. No input gives a split reason (see Scale check).

## Teams

| Team | Type | Size | Owns contexts | Cognitive load | Notes (minimal team API) |
|---|---|---|---|---|---|
| `core-team` Operator and agent fleet | stream-aligned | not fixed (1 operator plus a changing number of agents, A1) | `ideas`, `task-execution`, `worktree-custody`, `integration`, `worker-profiles`, `fleet-runtime` | high | Runs the horch binary on the operator's machine. Reached through the horch CLI, herdr panes and the needs-you folder. |

### Cognitive-load assessment

- **Intrinsic load:** 2 core contexts (`task-execution` and `integration`, model complexity 8 each in strategize) and 4 supporting contexts (complexity 4 to 5).
- **Extraneous load:** 1 deployable, but 3 host OSes (understand C3), 6 agent CLIs, herdr, git, GitHub and 3 service managers.
- **Rubric:** deployables per person is fine (1 unit). Contexts per team is high (6, the threshold is 5). Qualitative drivers are high (2 core contexts).
- **Verdict:** high (A7).
- **Mitigation:** no second team exists, so a context cannot move. Keep each context a module with its own tables and folders, enforce the boundaries with an arch_scan rule (A11, orchestrator D15), give an agent 1 context per task where possible, and add the contexts one at a time in the 9 migration phases: Phase 0, Phase P, Phases 1 to 7 (understand C9, N19).

## Interaction modes

One team - no cross-team interactions.

## Deployment topology

```mermaid
flowchart LR
  subgraph teams["Teams"]
    t_core_team["Operator and agent fleet (stream-aligned)"]
  end
  subgraph contexts["Bounded contexts"]
    c_ideas["Ideas (supporting)"]
    c_task_execution["Task execution (core)"]
    c_worktree_custody["Worktree custody (supporting)"]
    c_integration["Landing and integration (core)"]
    c_worker_profiles["Worker profiles and adapters (supporting)"]
    c_fleet_runtime["Fleet runtime (supporting)"]
  end
  subgraph deployables["Deployable units"]
    d_horch[["horch binary (modular-monolith)"]]
  end
  t_core_team --> c_ideas
  t_core_team --> c_task_execution
  t_core_team --> c_worktree_custody
  t_core_team --> c_integration
  t_core_team --> c_worker_profiles
  t_core_team --> c_fleet_runtime
  c_ideas --> d_horch
  c_task_execution --> d_horch
  c_worktree_custody --> d_horch
  c_integration --> d_horch
  c_worker_profiles --> d_horch
  c_fleet_runtime --> d_horch
```

### Deployables

#### horch binary (`horch`, modular-monolith)

- **Contexts:** `ideas`, `task-execution`, `worktree-custody`, `integration`, `worker-profiles`, `fleet-runtime` (1 module each inside horch-core).
- **Team:** `core-team`
- **Data store:** own - 1 SQLite ledger (`~/.horch/ledger.sqlite3`, WAL) plus the workspace files. Each context owns its tables and folders (see below).
- **Independent deploy:** yes, through horch self-update and the GitHub release assets (understand C7).
- **Process roles (N1):** the same binary runs as the fleetd daemon (the only ledger writer), as short-lived CLI commands (read-only ledger, requests on the command channel), as the fleet-run wrapper (1 per attempt, owned by `worker-profiles`, A3) and as `horch fleetd check`. Docs sync and adapter conformance are a horch subcommand that fleetd schedules (D2, A10).
- **Why these contexts live together:** fleetd is a subcommand of the single horch binary, so the CLI, the daemon and the wrapper always have the same version (design: Amendments D9, FM-34; understand C1). All 13 relationships are in-process (connect N1). task-execution calls worker-profiles and fleet-runtime synchronously at every launch (connect F2, F4). 1 team owns every context.
- **Platform layer (A9, N5):** the leaf library crate `crates/horch-platform` (design A19, D16), linked into this binary. It is not a context and not a deployable. Each context calls it directly. Phase P creates it and moves the 178 production OS-specific sites behind it.
- **Boundary rule (A11, N4):** an `arch_scan` rule fails a context that reads or writes the ledger tables or workspace folders of another context.

## Data-store ownership

All rows go through the single ledger writer in `fleet-runtime` (decompose N5). The owning context decides its rows (N2).

| Context | Store / schema | Owned by deployable | How others get at it |
|---|---|---|---|
| `ideas` | ledger tables `ideas`, `requirements`, `questions` (A5); idea folders | `horch` | events `plan-committed`, `idea-tasks-landed`, `question-answered` |
| `task-execution` | ledger tables `tasks`, `task_deps`, `task_covers`, `attempts`, `leases`; task and attempt folders | `horch` | events `task-leased`, `task-done`, `task-escalated`, `replan-requested` |
| `worktree-custody` | ledger table `worktrees` | `horch` | events `worktree-created`, `rescue-failed` |
| `integration` | ledger tables `features`, `branches`, `gate_runs`, `trial_merges` | `horch` | events `feature-opened`, `task-branch-landed`, `feature-merged`, `resolver-task-opened` |
| `worker-profiles` | teammate profile files, no ledger table | `horch` | query `launch-environment` through the module contract |
| `fleet-runtime` | ledger tables `events`, `ingested`; the command channel | `horch` | command `start-agent-process`; the single ledger writer |

## Decisions

- **D1 (topology, chosen A, made by step):** 1 horch binary. Option B (a separate fleetd daemon binary) and option C (a separate wrapper binary) let the CLI and the daemon differ in version, the failure FM-34 prevents. The design settles it (D9).
- **D2 (topology, chosen A, made by strategist):** where docs sync runs. Decided by the orchestrator (D15). The operator can reverse it. A: a horch subcommand that fleetd schedules, inside the 1 deployable. This is the recommendation of the step. The strongest argument against A: the design says to generalise `crates/herdr-docs-sync`, which is its own binary today (option B). Recorded in A10.

## Scale check

- **Target:** 1-1 deployables (manifest notes: "fleetd is a subcommand of the single horch binary. The operator and the fleet of AI agents are 1 team."). **Actual:** 1. **Within target:** yes.
- **Why not fewer:** 1 is the minimum.
- **Why not more:** `ideas`, `integration` and `worktree-custody` have async-only inbound, so they are the cheapest to extract. No input gives a split reason: no independent scaling, runtime, regulation, stack or second team. The design requires 1 binary (D9, FM-34). The ledger has 1 writer process (understand C4). `task-execution`, `worker-profiles` and `fleet-runtime` are joined by synchronous calls (connect F2, F4), so a split there adds a process hop to every launch.

## Upstream notes for this step

- understand N6, decompose N1: confirmed. All 6 contexts are modules in 1 deployable (A2, D1).
- strategize N3: confirmed. 1 team owns both cores (A1).
- understand N13, N18, decompose N7: confirmed. The platform layer is a leaf crate inside the 1 deployable, not a context (A9, N5).
- connect N1: confirmed. The process boundaries it lists are between process roles of the 1 binary and external systems, not between deployables (N1).

## Assumptions

- **A1** (high) 1 stream-aligned team, core-team, owns all 6 contexts. The size is not fixed because the agent count has no cap (understand C13).
- **A2** (high) 1 deployable, the horch binary, holds all 6 contexts (design D9, FM-34; understand C1, N6; decompose N1).
- **A3** (high) The fleet-run wrapper and `horch fleetd check` are process roles of the horch binary. `worker-profiles` owns the wrapper (orchestrator, D15; connect D2, N3).
- **A4** (medium) 1 SQLite ledger plus the workspace files; each context owns its tables (table above).
- **A5** (high) The `questions` table belongs to `ideas` (decompose D2 option A, orchestrator D15). `ideas` also owns the 1 needs-you list (decompose N9).
- **A6** (high) herdr, the agent CLIs, git, GitHub and the OS service managers are external systems behind adapters, not deployables (strategize N1).
- **A7** (medium) Cognitive load of core-team is high; mitigation as above.
- **A8** (medium) Each context owns its workspace folders: idea folders to `ideas`, task and attempt folders to `task-execution`, profile files to `worker-profiles` (understand N17).
- **A9** (high) The platform layer is the leaf crate `crates/horch-platform` inside the 1 deployable, not a context and not a deployable (design A19, D16).
- **A10** (medium) Docs sync and adapter conformance run as a horch subcommand that fleetd schedules (D2 option A). Its owning context stays open (decompose Q2).
- **A11** (high) An `arch_scan` rule enforces the context boundaries (orchestrator, D15).

## Open questions

None open. Q1 and Q2 are answered (orchestrator, D15) and listed as deprecated in the JSON:

- **Q1** (answered) Docs sync is a horch subcommand that fleetd schedules inside the 1 deployable (D2, A10).
- **Q2** (answered) An `arch_scan` rule enforces the context boundaries (A11, N4).

## Notes for downstream

- **N3** (decision; define, code) Docs sync is a horch subcommand in the 1 deployable; its owning context stays open (decompose Q2).
- **N4** (boundary; define, code) An `arch_scan` rule enforces the context boundaries.
- **N5** (boundary; code) `crates/horch-platform` is a leaf library crate linked into the 1 deployable; deployable_count stays 1.
