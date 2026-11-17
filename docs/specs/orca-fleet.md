# Design: a horch fleet in Orca (capability map and adapter)

Status: withdrawn as a plan. The operator chose to build the fleet natively in the Orca fork.
That design is `docs/horch-native-design.md` in `MattMakes/orca`, on branch
`claude/practical-ride-mcsllc`. The capability map of section 3 here still holds. Sections
1 and 4 to 9, on where the engine lives, do not. Nothing in this document is built.

This spec lives in multi-herdr for 2 reasons: tickets 01 to 11 of section 8 are multi-herdr
work, and Orca's repository keeps planning docs out of git (`docs/**` in its `.gitignore`).
A path that starts with `src/`, `db/` or `skill-guides/` is in `MattMakes/orca`. Every other
path is in this repository.

Sources read: `MattMakes/multi-herdr` at `e517075` (v0.5.1), `MattMakes/orca` at `225baa24`,
and [multi-herdr issue 349](https://github.com/MattMakes/multi-herdr/issues/349) with its design
doc "Herdr Agent Fleet: System Design". Section 10 says what was checked and how.

## 1. The answer first

**Do not port the fleet into Orca. Run the same fleet engine in Orca.**

The question was what to build into Orca so that it has multi-herdr's self-orchestration,
teammate flows and configured agents. The map in section 3 shows that the gap is almost all
_engine_: the ledger, the roster, the finish pipeline, routing and custody. multi-herdr already
owns that engine as about 136,000 lines of Rust in `horch-core` and `horch`. Issue 349 is
about to turn it into a deterministic daemon (`horch fleetd`) that treats the pane runtime as
a replaceable part.

So the design is one engine and two pane runtimes:

| Part                             | Where it lives                             | Size                                  |
| -------------------------------- | ------------------------------------------ | ------------------------------------- |
| The fleet engine                 | multi-herdr, as today, after 349           | Exists                                |
| Four amendments to 349           | multi-herdr                                | Small; they shape work 349 does       |
| An Orca pane-runtime adapter     | multi-herdr                                | Small; about a dozen `orca` CLI calls |
| A Fleet plugin (panel, commands) | Its own repo, loaded by Orca's plugin host | Small; no Orca source changes         |
| Patches to Orca core             | The Orca fork                              | **None required**                     |

Three facts decide it:

1. **Orca is a fast upstream, and the fork carries no patches.** The 50 commits its checkout
   holds are all upstream work, with pull request numbers up to 43403. Orca's orchestration
   database is at schema version 46, with a migration file per step
   (`src/main/runtime/orchestration/db/contract-constants.ts`). A fork that adds version 47
   collides with upstream's version 47. Every core patch is a rebase cost forever.
2. **A port means two implementations of one subtle state machine.** The ledger is 25,700
   lines, the harness adapters 19,500, the roster 5,900, routing 4,700, telemetry 6,500. A
   TypeScript copy would drift from the Rust original on the first change after 349.
3. **Orca already exposes every operation the engine needs from a pane runtime.** After 349
   the engine needs only: open a terminal that runs a command in a folder, close it, send 1
   constant line, read the screen, create and remove a worktree. Orca's CLI has all of them
   (section 6).

Choose the native port (section 9) only if Orca must run fleets on a machine that has no
`horch`: for example, to offer the feature upstream, or to ship the fork to people who will
not install horch.

## 2. What "multi-herdr level" means

The target is multi-herdr _after_ 349, not multi-herdr today. 349 removes several things that
would otherwise look like features to port:

| Today in multi-herdr                                | After 349                                                    |
| --------------------------------------------------- | ------------------------------------------------------------ |
| 1 long orchestrator conversation holds every idea   | A deterministic idea dispatcher; 1 sub-orchestrator per idea |
| Workers pull: `horch ledger claim`                  | fleetd pushes: `task.yaml` and `START.md` before launch      |
| A claim never expires                               | A lease that only earned activity renews                     |
| Done is the worker running `horch ledger done`      | Done is `result.json` plus passing acceptance checks         |
| A lazy watchdog, 600 s checks, typed mail as remedy | A supervised 5 s reconcile loop that acts in code            |
| Workers create worktrees                            | fleetd leases a worktree before it exists; rescue refs       |
| Many ledger writers                                 | fleetd is the only writer                                    |
| Typed mail, mail ids, a courier, a channel relay    | Files, and 1 constant doorbell line                          |

The three things the request names map to these parts of the engine:

- **Self-orchestration:** the idea dispatcher, the task ledger, push dispatch, leases, the
  reconcile loop.
- **Teammate flows:** spec-writer to ticket-writer to workers; the finish pipeline (check,
  comment-sweep, code-sweep with reviewer rounds, complexity-sweep, recheck, land, pr, cleanup);
  the creative council; the worktree steward.
- **Configured agents:** the roster. 1 markdown file per teammate, 44 frontmatter fields, about
  110 teammates, about 230 bundled skills, 6 harness adapters.

## 3. The capability map

"Where it is built" uses these words:

- **Have**: Orca does it today. The fleet uses it as it is.
- **Engine**: the horch engine does it, unchanged, once the adapter exists.
- **Adapter**: new Orca-specific code in multi-herdr (section 6).
- **Plugin**: the Fleet plugin (section 7).
- **Drop**: do not bring it to Orca.

### 3.1 Control plane

| Capability                          | multi-herdr (after 349)                                     | Orca today                                                                                                                                 | Where it is built |
| ----------------------------------- | ----------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------ | ----------------- |
| 1 owner of orchestration state      | `fleetd` is the only ledger writer                          | The runtime owns `orchestration.db`; the CLI is an RPC client. `orca serve service install` runs it under launchd or systemd               | Engine            |
| A loop that acts without a model    | 5 s level-triggered reconcile, 7 invariants                 | None. The coordinator is a model that follows a skill. The old in-process loop is retired (`coordinator-start` "performs no effects")      | Engine            |
| A durable record                    | Runs, attempts, finish steps and events are kept            | Settled Runs are pruned after 24 hours (`db/maintenance/settled-run-retention.ts`)                                                         | Engine            |
| Task graph                          | Blockers, waves, parallel or exclusive, owned files         | `deps` as a JSON list; a task turns `ready` when its deps complete. Nothing was found that handles a failed dep or caps concurrent workers | Engine            |
| Dispatch                            | Push: the task file exists before the process starts        | Push: `worker-start` binds the Task and types a preamble                                                                                   | Engine            |
| Leases, stall ladder, bounded retry | Lease 120 s, stall at 5 min, 3 attempts with backoff        | A 60 s readiness timeout and a breaker after 3 failed starts. No stall timer. "Silence … never proves exit"                                | Engine            |
| Stop, reap, replace                 | Exact PIDs; the retry gets the old worktree and transcript  | `worker-stop` (guarded; can answer `stop_unknown`), `--retry-of`                                                                           | Engine, Adapter   |
| Done is a verified fact             | `result.json`, a stop hook, acceptance checks run by fleetd | `worker_done` mail is a self-report. A stop without it becomes an `escalation` to the coordinator model                                    | Engine            |

### 3.2 Intake and flows

| Capability                     | multi-herdr (after 349)                                                        | Orca today                                                                       | Where it is built |
| ------------------------------ | ------------------------------------------------------------------------------ | -------------------------------------------------------------------------------- | ----------------- |
| Idea to approved plan          | `horch idea add`; a state machine per idea; an interview; approval by hash     | None. `task-create` is by hand; `decompose()` is a stub                          | Engine            |
| Ticket contract                | Persona, Kind, Review, Check, Own, Do not touch; `add-map --dry-run` validates | A Task has a title, a spec, a status, deps and a result                          | Engine            |
| Operator approval              | Only a command run outside any fleet process records it                        | Decision gates that the coordinator resolves; no timeout is enforced             | Engine, Plugin    |
| Finish pipeline                | 8 steps; verification profiles `full`, `light`, `minimal`                      | None. `merge_ready` is ignored. Nothing sets the declared `merging` phase        | Engine            |
| Code review rounds             | Reviewers spawned by code; findings `F<n>`; answers; waivers                   | None in orchestration. The UI can send diff notes and failing checks to an agent | Engine            |
| Run branch, land, 1 PR per run | Rebase, check, fast-forward under a land lock, push; run gate; run close       | No local land. Hosted merge through `gh`, AI commit and PR text, PR watch        | Engine, Adapter   |
| Councils and domain teams      | Creative council; Unreal, Swift, Godot, Blender, design and video teams        | None                                                                             | Engine            |

### 3.3 Configured agents

| Capability                   | multi-herdr (after 349)                                                                  | Orca today                                                                                                               | Where it is built |
| ---------------------------- | ---------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------ | ----------------- |
| Named teammates              | 1 file each; adding a file adds a teammate; an operator overlay folder                   | None. Settings hold 1 unnamed default per agent kind. Orca "profiles" are app-data profiles                              | Engine            |
| What a teammate fixes        | Model, effort, permission mode, tools, MCP servers, plugins, sandbox, env, system prompt | `--agent`, `--model`, `--effort`. The launch request has no field for a system prompt, tools, MCP servers, skills or env | Engine            |
| Harness adapters             | 6 CLIs, with capability answers per CLI                                                  | 45 agent kinds can launch; about 23 have status hooks                                                                    | Engine            |
| Starting clean               | Plugins off by name, exactly 0 MCP servers, no subagent tool, a private `CODEX_HOME`     | None                                                                                                                     | Engine            |
| Skills per launch            | Phase catalogs; a skill bundle per launch; expected skills named in the briefing         | `orca skills install` is global. Nothing is chosen per launch. Plugin `skills` and `mcpServers` are reserved, not open   | Engine            |
| Validation                   | `horch teammates --check` fails a bad roster                                             | Not applicable                                                                                                           | Engine            |
| Reserved tiers and fallbacks | Seats in code; `fallbacks:` per teammate                                                 | None                                                                                                                     | Engine            |

### 3.4 Worktrees, resources and surface

| Capability                 | multi-herdr (after 349)                                                         | Orca today                                                                                                                                                          | Where it is built |
| -------------------------- | ------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------- |
| Worktree custody           | Lease before create; reconcile against `git worktree list`; rescue refs; triage | Creates and removes worktrees with lineage and setup hooks. Cleanup rules are off by default and refuse dirty or unpushed trees. No rescue. Archive is switched off | Engine, Adapter   |
| Questions and answers      | Files; workers ask their sub-orchestrator                                       | A mailbox with delivery holds, question threads and cross-server mail                                                                                               | Engine            |
| "Needs you"                | 1 folder, 1 list, 1 notification                                                | Notifications for agent needs input, turn finished, terminal bell and plugins; mobile push                                                                          | Adapter, Plugin   |
| Usage-aware routing        | Pools, probes, substitute a fallback or refuse                                  | Reads usage windows for 11 providers. Nothing gates or reroutes a launch                                                                                            | Engine            |
| Context policy             | A watch threshold, a handoff, a compact, native window control                  | Context use is tracked for chat sessions only, not for terminal agents                                                                                              | Engine            |
| Cost and telemetry         | Exact cost per session, teammate and tier; skill use                            | Token and cost analytics from transcripts; per-automation usage. Nothing per task                                                                                   | Engine            |
| CLI updates, doctor, smoke | A 7-day release cooldown; `doctor`; `smoke`                                     | Suppresses update prompts; probes versions                                                                                                                          | Engine, Adapter   |
| Session records and resume | Every session, its summary and its facts are kept                               | Provider session ids, session search, resume                                                                                                                        | Engine            |
| Seeing the fleet           | Panes, `horch status`, a telemetry screen                                       | A sidebar tree with lineage and agent status, a board with columns, diff review, a mobile app. No Run or Task view. The crew panel is switched off in code          | Adapter, Plugin   |
| Remote and mobile          | Claude Remote Control on the orchestrator                                       | Paired servers, SSH hosts, a mobile app. They let you watch a fleet from anywhere; the fleet itself runs on 1 machine (6.7)                                         | Have              |
| Pane grid                  | `horch tile`, `horch balance`                                                   | Tabs and workspaces lay themselves out                                                                                                                              | Drop              |
| herdr install and docs     | `herdr-install`, `herdr-docs-sync`                                              | Not applicable                                                                                                                                                      | Drop              |

### 3.5 What the map says

- Orca is **ahead** of today's horch on the control-plane base: 1 state owner, push dispatch,
  status from hooks, mail that waits for a turn to end, terminals that outlive the app. Those are
  the same directions 349 takes. This is why Orca is a good host for the engine.
- Orca has **none** of the fleet layer: no roster, no ticket contract, no finish pipeline, no
  land, no routing, no custody. Every row of that layer is "Engine".
- The only rows that need Orca-specific work are the ones that touch a pane, a worktree or the
  screen. That is the adapter and the plugin.

## 4. Decision: where the engine lives

| Option                                                     | Orca core patches         | Second implementation | Works without horch | Verdict    |
| ---------------------------------------------------------- | ------------------------- | --------------------- | ------------------- | ---------- |
| N: port the fleet to TypeScript in Orca core               | Many, in the fastest code | Yes                   | Yes                 | Rejected   |
| P: build the fleet as an Orca plugin worker                | None                      | Yes                   | Yes                 | Rejected   |
| **E: horch engine, Orca adapter, Fleet plugin for the UI** | **None**                  | **No**                | **No**              | **Chosen** |

Why P is rejected: a `trusted-worker` plugin is a real Node process and could hold the logic,
but the host may stop an idle worker, a hook gets 2 s, and it would still be a second
implementation. A plugin is the right home for the _view_, not for the ledger.

What E costs: the fleet runs only on a host that has both `horch` and Orca. Orca's native
orchestration (`orca orchestration …`) stays what it is: light, ad hoc delegation from a chat.
The fleet is the planned, verified path. Section 6.6 keeps the two apart.

The engine does not launch workers with `orca orchestration worker-start`. That command builds
Orca's own command line, which has no place for a teammate's tools, MCP servers, skills or
system prompt, and it types a preamble that tells the worker to report by `worker_done` mail.
Both conflict with the roster and with "done is a file on disk". The engine builds the command
and Orca runs it in a terminal.

This extends 349's non-goal "herdr stays the place where you watch panes": herdr stays
supported, and Orca becomes a second place.

## 5. Architecture

```text
 operator
   |  desktop, browser or phone
   v
+----------------------------------------------------------------------+
| Orca (unchanged)                                                     |
|                                                                      |
|  sidebar: home workspace                Fleet plugin (section 7)     |
|    +- T-0007-01  [in-progress]            panel: needs you, ideas,   |
|    +- T-0007-02  [in-review]              tasks, worktrees, pools    |
|    +- T-0007-03  [completed]              commands, status bar item  |
|  terminals: idea-0007, 1 per worker             |                    |
|  hook server and OSC 7501: agent status         | reads               |
+-------^-------------------------^---------------|--------------------+
        | orca CLI                | status line   v
        | (section 6)             | from the wrapper   horch status --json
+-------|-------------------------|------------------------------------+
| horch fleetd (multi-herdr, after 349)                                |
|                                                                      |
|  ledger (only writer) - idea dispatcher - leases - verifier          |
|  roster - harness adapters - routing - finish steps - rescue         |
|                                                                      |
|  Mux trait --------------+--> herdr adapter (exists)                 |
|  WorktreeProvider trait -+--> Orca adapter  (new)                    |
+----------------------------------------------------------------------+
        |
        v  the wrapper runs each agent CLI inside an Orca terminal
   claude, codex, opencode, pi, prime-agent, agy
```

### 5.1 One idea, end to end, in Orca

1. You run `horch idea add "<idea>"`, or use the form in the Fleet panel.
2. fleetd opens a terminal `idea-0007` in the home workspace. The sub-orchestrator reads the
   code and interviews you there. Orca's mobile app can reach the same session (V4).
3. You approve the written assumptions, by command or by the button in the panel.
4. The sub-orchestrator writes the spec and the tickets. fleetd validates the plan and commits
   it to the ledger in 1 transaction.
5. For each task that can start, fleetd writes the worktree lease, creates the worktree through
   Orca as a child workspace, writes `task.yaml` and `START.md`, and opens a terminal there that
   runs the wrapper. The card shows `in-progress`.
6. The worker works. Orca's sidebar shows working, blocked or done from the agent's hooks.
   fleetd renews the lease only on real activity.
7. The worker writes `result.json`. fleetd runs the finish steps. A comment-remover or a
   reviewer opens as another terminal in the same task workspace. The card moves to
   `in-review`, then `completed`, and the workspace is removed after the branch landed.
8. When every task is done, the run branch gets its pull request. fleetd links it on the home
   workspace, so Orca shows its checks and comments. You merge.

### 5.2 Rules

Rules that hold the design together:

1. **The ledger is the only truth.** Orca's sidebar, board and plugin panel are views. Nothing
   reads them back.
2. **The engine reaches Orca only through 2 traits**, `Mux` and `WorktreeProvider`. No other
   module names Orca or herdr.
3. **The adapter calls only public `orca` CLI commands with `--json`.** It reads every id, path
   and branch back from the result. It never assumes a name.
4. **A failed view update never fails a task.** It is logged and retried on the next pass.

## 6. The Orca adapter

All of this is Rust in multi-herdr. The Orca executable comes from `HORCH_ORCA_BIN`, default
`orca`. `HORCH_MUX=orca` or `horch fleet --mux orca` selects it; inside an Orca terminal
(`ORCA_TERMINAL_HANDLE` is set) it is the default.

### 6.1 `Mux`: panes

| Operation      | What the engine needs                    | Orca command                                                                       |
| -------------- | ---------------------------------------- | ---------------------------------------------------------------------------------- |
| `reachable`    | Is the runtime up?                       | `orca status --json`                                                               |
| `open_pane`    | Run 1 command in a folder; get a pane id | `orca terminal create --worktree path:<dir> --title <role> --command <cmd> --json` |
| `close_pane`   | End the pane and what runs in it         | `orca terminal close --terminal <handle> --tab --json`                             |
| `send_line`    | Type 1 line and press Enter              | `orca terminal send --terminal <handle> --text <line> --enter --json`              |
| `read_screen`  | An excerpt for an escalation             | `orca terminal read --terminal <handle> --screen --json`                           |
| `list_panes`   | Find panes no live attempt owns          | `orca terminal list --worktree <selector> --json`                                  |
| `agent_status` | The weakest liveness input               | `orca agent list --worktree <selector> --json`                                     |
| `layout`       | Tile and balance                         | Not offered. The trait reports the capability as absent                            |

- `orca terminal create` types `--command` into the host's shell. horch's pane command already
  starts with `exec /usr/bin/env -u ANTHROPIC_API_KEY …` and carries its own variables, so the
  adapter needs no env flag. The API-key rule of multi-herdr holds unchanged.
- An agent that horch starts in an Orca terminal still reports to Orca: Orca's managed hooks
  read `ORCA_PANE_KEY` from the terminal's environment, and OSC 7501 covers "an agent run by
  hand" (`src/main/agent-hooks/README.md`). So the sidebar shows working, blocked and done with
  no extra code.
- After 349's file protocol lands, the only caller of `send_line` passes the constant doorbell.
  Until then `horch tell` uses the same operation.
- A pane's identity is its workspace plus its title. The terminal handle is a cache: when a
  handle is unknown after an Orca restart, the adapter finds the pane again with `list_panes`.
- Every pane of a task opens in the task's workspace: the worker, its comment-remover and its
  reviewers. Idea panes and shared-checkout tasks open in the home workspace.
- On Windows the adapter passes `--shell` so that it knows which shell reads the command.
- The adapter does not call Orca on every 5 s pass. It calls on a state change, and polls
  `agent_status` every 30 s. Process state and the wrapper heartbeat stay the strong signals,
  as 349 says.

### 6.2 `WorktreeProvider`: worktrees

349 makes fleetd the only creator of worktrees. With Orca, fleetd creates them _through_ Orca,
so each task is a real Orca workspace with its own card, diff and terminal.

| Operation | Order of events                                                                                                                                                                                                                                    |
| --------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `create`  | 1. Write the `reserved` row. 2. `orca worktree create --repo <selector> --name <task> --branch <fleet branch> --base-branch <ref> --parent-worktree <home> --setup skip --json`. 3. Read back the id, path and branch. 4. Write the `created` row. |
| `remove`  | Only after fleetd proved the branch landed or the rescue ref holds the work: `orca worktree rm --worktree <selector> --json`. Never `--force`.                                                                                                     |
| `list`    | `git worktree list --porcelain` stays the truth for invariant I6. `orca worktree list --json` adds the Orca ids.                                                                                                                                   |

- `--branch` names the branch without Orca's configured prefix, so the fleet's branch names
  hold. A taken name gets a numeric suffix, which is why step 3 reads the branch back.
- `--setup skip` is the default. It matches horch today, where a worktree is a plain checkout
  and the check command provisions itself. `inherit` is an option per run once the live check
  (section 8) proves how the CLI reports that setup finished.
- A crash between steps 1 and 4 is safe: 349's reconciler adopts a directory with no row and
  drops a `reserved` row with no directory.
- The path is data from the provider. Today 12 files of `horch-core` name `.worktrees/`; with
  a provider none of them may build a path from it.
- The herdr side keeps a git-direct provider with the same 3 operations.

### 6.3 Presentation sync

A one-way, idempotent push from ledger facts to Orca metadata, run inside the reconcile pass.

| Ledger fact               | Orca call                                                                                                             |
| ------------------------- | --------------------------------------------------------------------------------------------------------------------- |
| Task leased, tree created | `orca worktree set --display-name "<T-id> <title>" --comment "<persona>, attempt <n>" --workspace-status in-progress` |
| Verifying or in review    | `--workspace-status in-review`                                                                                        |
| Landed                    | `--workspace-status completed`, then `remove`                                                                         |
| Escalated to the operator | `--unread` and a comment that starts `NEEDS YOU:`                                                                     |
| Run PR opened             | `orca worktree set --worktree <home> --pr <number>`                                                                   |

This is the deliberate source of delight: with no new UI, the fleet shows up as a living tree
in Orca's sidebar and as cards that move across Orca's board. The 4 status ids are Orca's
defaults (`todo`, `in-progress`, `in-review`, `completed`). A board without them only loses the
column move.

### 6.4 Status from the wrapper

349's wrapper reports agent state to herdr with `pane report-agent`. In Orca the wrapper writes
an OSC 7501 program-status line instead, so a pane that is verifying or landing (no agent
process) still shows what it does. Hooks win over OSC 7501 while an agent runs, by Orca's own
precedence rule. For "needs you", the wrapper rings the terminal bell. Orca notifies on a bell
when the operator has that setting on.

### 6.5 Doctor, smoke and tests

- `horch doctor` with the Orca mux checks: `orca status`, the CLI version, that the project is
  a local Orca project, and a throwaway terminal create, send, read and close. It warns when
  Orca's workspace cleanup rules are on for the project.
- The same checks run at fleet start. Orca updates itself often, so a CLI flag can change
  under the adapter. A failed check marks the mux unhealthy and stops dispatch, as 349 does for
  an agent CLI that fails conformance.
- `horch smoke fleet` runs the same no-token story against Orca.
- `horch-e2e` gets a fake `orca` binary beside its fake harness binaries, so the hermetic suite
  covers the adapter with no Orca running.
- `scripts/live/orca.sh` is the live check against a real Orca.

### 6.6 Keeping the fleet and Orca's own orchestration apart

- `orca skills install` puts Orca's `orchestration` and `orca-cli` skills in the agents' skill
  folders. A fleet worker that loads one could start Orca workers that the ledger never sees.
  Every Claude teammate base adds both names to `disabled_skills`. For Codex, the worker
  briefing carries the rule, and the live check proves that its sandbox and execpolicy refuse
  `orca orchestration` (V8).
- A vanished worktree is an escalation in 349. With Orca the reconciler first asks whether the
  tree had landed; a landed tree that Orca's cleanup removed is recorded as removed.
- Fleet panes use the machine's own CLI logins. Orca's managed accounts apply only to agents
  that Orca launches.

### 6.7 Failure modes

| What happens                                 | What the fleet does                                                                                                                                      |
| -------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Orca's runtime is down                       | Dispatch pauses. Live attempts go on: Orca's terminal daemon outlives the app, and leases renew on process and heartbeat. `horch status` names the cause |
| Orca restarts and a handle is stale          | The adapter finds the pane by workspace and title                                                                                                        |
| You close a worker's tab                     | The process ends, the wrapper sees it, and the attempt fails and retries by the profile's policy                                                         |
| You type into a worker's terminal            | Allowed, as in herdr. It counts as activity                                                                                                              |
| Orca's cleanup removes a landed worktree     | Recorded as removed                                                                                                                                      |
| Orca's cleanup removes an unlanded worktree  | Orca refuses a dirty or unpushed tree. If one still vanishes, fleetd escalates, as 349 says                                                              |
| An Orca update changes a CLI flag            | The start check fails closed and names the command                                                                                                       |
| The project is on an SSH, WSL or paired host | Refused. fleetd and the worktrees must be on 1 machine. Run horch and `orca serve` on that machine and pair to it                                        |

## 7. The Fleet plugin

A separate repository with an `orca-plugin.json`, loaded by Orca's plugin host. It changes no
Orca source and installs like any plugin.

- **Execution profile:** `trusted-worker`. The worker is a Node process that runs
  `horch status --json` and pushes the result to the panel.
- **Capabilities:** `workspace:read`, `workspace:write` (to open a task's workspace),
  `agent:read`, `notifications:show`, `settings:own` (the path of `horch`).
- **Panel** (components TSX, no build step), in this order: Needs you, Ideas with their state,
  Tasks with lease age and attempt, Worktrees with state and size, Usage pools.
- **Forms and buttons in the panel:** add an idea, approve or revise an idea, open a task's
  workspace.
- **Palette commands:** Fleet: Open panel, Fleet: Add idea. Both open the panel; the form is
  in the panel.
- **Status bar item:** `Fleet: 3 running, 1 needs you`.
- **Refresh:** on `agent.status.changed` and `workspace.changed` events, and on a timer only
  while the panel is open.

Rules:

1. The plugin never writes the ledger. Every action is a `horch` command, so fleetd stays the
   only writer.
2. Approval needs a click. 349 already refuses `horch idea approve` from inside a fleet attempt;
   the plugin worker is not one, and an agent has no way to press the button.
3. With no `horch` on the path, the panel says so and offers nothing else. It never shows empty
   state as "no work".

## 8. Build map

Two releases. Each is complete on its own, and the first is useful if the second never ships.

**Release 1, "a fleet in Orca":** `horch fleet --mux orca` runs a whole fleet with Orca as the
pane runtime. You watch and steer it in Orca's sidebar, board, terminals and mobile app, and
use `horch status` in a terminal for the board.

**Release 2, "the Fleet panel":** the plugin of section 7.

Tickets, in the shape of a ticket map (`docs/specs/ticket-format.md`):

| #   | Title                                                                      | Repo         | After          | Wave |
| --- | -------------------------------------------------------------------------- | ------------ | -------------- | ---- |
| 01  | Amend 349: the `Mux` trait, and a scan rule that keeps herdr behind it     | multi-herdr  | in 349 phase 1 | 1    |
| 02  | Amend 349: the `WorktreeProvider` trait with a git-direct provider         | multi-herdr  | in 349 phase 3 | 1    |
| 03  | Amend 349: neutral pane and scope ids in place of `HERDR_PANE_ID`          | multi-herdr  | in 349 phase 1 | 1    |
| 04  | A fake `orca` binary in `horch-e2e`, and a conformance suite per mux       | multi-herdr  | 01             | 2    |
| 05  | Orca `Mux` adapter: the pane operations of 6.1                             | multi-herdr  | 01, 03, 04     | 3    |
| 06  | Orca `WorktreeProvider`: create, remove, list                              | multi-herdr  | 02, 04         | 3    |
| 07  | Presentation sync                                                          | multi-herdr  | 05, 06         | 4    |
| 08  | Wrapper status as OSC 7501, and the bell for "needs you"                   | multi-herdr  | 05             | 4    |
| 09  | `doctor`, `smoke` and `agent-list` for Orca; `HORCH_MUX`, `HORCH_ORCA_BIN` | multi-herdr  | 05             | 4    |
| 10  | Switch Orca's 2 skills off in every Claude teammate base                   | multi-herdr  | none           | 1    |
| 11  | `scripts/live/orca.sh`: the live check on a real Orca; README section      | multi-herdr  | 06, 07, 08, 09 | 5    |
| 12  | Fleet plugin: panel, status bar item, notifications                        | its own repo | 11             | 6    |
| 13  | Fleet plugin: the idea form, approve and revise, palette commands          | its own repo | 12             | 7    |

Critical path: 01 and 02 inside 349 phases 1 and 3, then 04, 05 and 06, 07, 11.

"After" names 349's own migration phases where a ticket is part of one. Release 1 needs 349
phases 1 and 3. It gains push dispatch and the result gate when 349 phases 4 and 5 land, with
no adapter change.

### The 4 amendments to 349

These are the reason to decide now. Each one shapes code that 349 writes anyway, and each is
cheap before that code exists and costly after.

1. **`Mux` trait.** fleetd reaches the pane runtime only through it. Today 36 files of
   `horch-core` outside `workspace/` name herdr. 349's doc already says "the herdr adapter";
   this makes it a rule that a scan test enforces.
2. **`WorktreeProvider` trait.** 349 phase 3 makes fleetd the only caller of
   `git worktree add`. Put that call behind a trait from the start, and take the path from the
   provider.
3. **Neutral identity.** A pane id and a scope id that do not carry herdr's name, with the
   per-runtime variable (`HERDR_PANE_ID`, `ORCA_TERMINAL_HANDLE`) read in 1 place.
4. **Status out as an operation.** The wrapper calls `report_status`; herdr maps it to
   `pane report-agent`, Orca to OSC 7501.

## 9. The native port, sized

For the record, this is what "build it into Orca" means if option N is ever chosen. Sizes are
the matching horch modules, as a guide to scale.

| To build in Orca                                                               | horch reference            | Orca seam it must touch                                                      |
| ------------------------------------------------------------------------------ | -------------------------- | ---------------------------------------------------------------------------- |
| A roster: parser, 44 fields, validation, overlay                               | `roster`, 5,900 lines      | New module; a `--persona` flag on `worker-start` and `agent launch`          |
| Launch extras per CLI: tools, MCP, system prompt, skills bundle, sandbox, home | `harness`, 19,500 lines    | `AgentLaunchIntent`, the command builders, or the `agent.launch.before` hook |
| A fleet ledger: tickets, attempts, leases, steps, worktrees, events            | `taskledger`, 25,700 lines | A separate SQLite file. Never the `orchestration.db` version chain           |
| A controller loop: dispatch, leases, stall ladder, invariants                  | 349's fleetd               | A new service in the runtime and in `orcad`                                  |
| Finish steps, land under a lock, 1 PR per run, run gate                        | `taskledger/finish`, `vcs` | `src/main/git`, `src/main/source-control`                                    |
| Idea dispatcher and sub-orchestrators                                          | 349 phase 6                | New module, new CLI group                                                    |
| Routing on usage, with fallbacks                                               | `routing`, 4,700 lines     | `src/main/rate-limits` already reads 11 providers                            |
| Context policy for terminal agents                                             | `compaction`, 2,300 lines  | The status-line script reads `rate_limits` only today                        |
| Cost per task and per persona                                                  | `telemetry`, `usage`       | `src/main/claude-usage`, `codex-usage`                                       |
| Rescue refs and triage                                                         | `taskledger/triage`        | `src/main/workspaces/cleanup`                                                |
| A Run and Task view                                                            | None                       | Renderer and mobile; the crew panel is off for performance                   |

One thing to watch under option E: Orca reserves plugin contribution points named `skills`,
`mcpServers` and `agentHarnesses`, and they must be empty today
(`src/shared/plugins/contributions/plugin-contribution-points.ts`). When
upstream opens them, the Fleet plugin can offer the roster's teammates to Orca's own launchers,
still with no fork patch.

## 10. How this design was checked

**Read first-hand in this session**

- multi-herdr: `README.md`, `GLOSSARY.md`, `docs/fleet-workflow.md`, `docs/command-flow.md`,
  `docs/phase-skills.md`, `docs/specs/ticket-format.md`, the task-ledger spec (sections 1 to 6,
  9, 10), `teammates/_template.md`, both base briefings, the `WorkspaceClient` trait, module
  line counts, and the count of files that name herdr.
- Issue 349 and all of its linked design doc that covers architecture, ledger, profiles, ideas,
  worktrees, supervision, migration and open questions.
- Orca: the orchestration skill guide and its 5 references; the CLI specs for `orchestration`,
  `terminal`, `worktree create`, `worktree set`, `worktree rm`, `status` and `serve`; the
  plugin host API, hooks and capabilities; `spawn-agent-launch-plugin-hook.ts`; the hook
  README; the session-restore page; the schema version; the 24-hour Run retention; the
  crew-panel switch; the 50 commits of the shallow checkout.

**Taken from 3 read-only code surveys of Orca, spot-checked above**

- The orchestration data model and its gaps: no stall timer, no failed-dep propagation, no
  concurrency cap, no merge step.
- Agent launch: about 23 hook installers, no named presets, no per-launch MCP or skills.
- Worktree cleanup rules, automations, session history and the lack of a local land flow.

**Not verified: no Orca was running here, and 349 is not built**

| #   | To confirm with `scripts/live/orca.sh`                                            | If it fails                                                |
| --- | --------------------------------------------------------------------------------- | ---------------------------------------------------------- |
| V1  | The JSON field names of `terminal create` and `worktree create` results           | Adjust the parser; no design change                        |
| V2  | The `orca` CLI reaches the runtime from a launchd or systemd unit                 | fleetd passes the runtime address it found at start        |
| V3  | Orca does not put a workspace with a running fleet pane to sleep                  | The adapter marks fleet workspaces active, or doctor warns |
| V4  | A bell reaches the phone as a push, and the phone can type in that terminal       | The plugin sends the notification; answer on the desktop   |
| V5  | OSC 7501 from the wrapper shows while no agent runs, and hooks win while one does | Drop 6.4; the sidebar comment carries the state            |
| V6  | How `worktree create --setup inherit` reports that setup finished                 | `skip` stays the only mode                                 |
| V7  | Whether fleetd can reuse the first terminal that `orca worktree create` opens     | Cosmetic: 1 spare tab per task                             |
| V8  | A Codex fleet pane cannot run `orca orchestration`                                | Add an execpolicy rule that forbids it                     |
| V9  | Terminal handles survive an Orca restart                                          | Already covered: lookup by workspace and title             |

"None required" for Orca core patches is a claim from reading the CLI specs, not from a run.
V1 to V9 are where it could prove wrong, and each has a fallback that stays outside Orca core.

## 11. Decisions for the operator

| #   | Decision                                     | Recommended                                                         |
| --- | -------------------------------------------- | ------------------------------------------------------------------- |
| D1  | Where the engine lives                       | Option E: 1 engine, an Orca adapter, a plugin for the view          |
| D2  | When to build the adapter                    | On 349's seams (tickets 01 to 03), not on today's `WorkspaceClient` |
| D3  | What a task is in Orca                       | 1 child workspace per task worktree                                 |
| D4  | Where idea panes open                        | As terminals in the workspace the fleet was started from            |
| D5  | Where the plugin lives                       | Its own repository, not the Orca fork                               |
| D6  | Whether the fork ever carries fleet patches  | No, unless section 1's condition for the native port holds          |
