# How the commands fit together

Two binaries share one kernel. `horch` runs a fleet: one orchestrator that
spawns workers. `multi-herdr-dataset` runs a competition round: N candidates
and a judge. Both create executions through the same spawn service and write
them to the same execution ledger.

A worker or a candidate runs one of 6 agent CLIs: `claude`, `codex`,
`opencode`, `pi`, `prime` (binary `prime-agent`) or `antigravity` (binary
`agy`). `horch agent-list` shows which of them this machine has.

## 1. The big picture

```mermaid
flowchart TB
    subgraph setup["Setup (once)"]
        I["just install"] --> HI["horch install<br/>(horch and multi-herdr-dataset on PATH)"]
        I --> HF["writes ~/.local/bin/herdr-fleet"]
        HI --> DOC["horch doctor<br/>(herdr reachable?)"]
        DOC --> SM["horch smoke messaging / fleet / tile<br/>(self-checks)"]
        HI --> AL["horch agent-list<br/>(which agent CLIs are installed;<br/>no herdr, no model call)"]
        MK["horch skills install / horch marketplace<br/>(install a skill once, pinned)"]
    end

    subgraph fleet["horch: the fleet"]
        HF2["herdr-fleet [flavor]"] --> FL["horch fleet"]
        FL --> ORCH["orchestrator pane<br/>(Claude or Codex)"]
        FL --> TEL["telemetry space<br/>(horch telemetry collector)"]
        ORCH -->|"horch spawn &lt;teammate&gt;"| SVC
        SVC --> WK["worker pane<br/>(horch worker → claude, codex, opencode,<br/>pi, prime or antigravity)"]
    end

    subgraph dataset["multi-herdr-dataset: a competition round"]
        RUN["multi-herdr-dataset run &lt;task&gt;"] --> COORD["coordinator<br/>(tick loop)"]
        COORD -->|"spawns each candidate"| SVC
        SVC --> CAND["candidate panes<br/>(in their own worktrees)"]
        COORD -->|"detached job"| JUDGE["judge-job<br/>(claude -p, read-only)"]
    end

    SVC{{"spawn service<br/>plan → record → brief → pane → launch"}}
    SVC --> LEDGER[("execution ledger<br/>state dir/&lt;project&gt;.json")]
    JUDGE --> LEDGER
    COORD --> EVENTS[("dataset events<br/>state dir/multi-herdr/&lt;project&gt;/events")]
    MK -.->|"skills exposed at launch"| SVC

    LEDGER --> SESS["horch sessions<br/>(hides candidates/judge unless --all)"]
    LEDGER --> COST["horch cost / horch usage<br/>(counts workers, candidates, judge)"]
    EVENTS --> DATA["status · export · readiness<br/>outcome · rebuild · resume"]
```

`<project>` is a slug of the project path. The state dir is
`$HORCH_STATE_DIR`, else `${XDG_STATE_HOME:-~/.local/state}/horch`.

## 2. The fleet: orchestrator and workers

```mermaid
sequenceDiagram
    actor You
    participant O as Orchestrator pane
    participant H as horch (CLI)
    participant L as Ledger
    participant W as Worker pane

    You->>H: herdr-fleet (horch fleet)
    H->>L: insert orchestrator record
    H->>O: open pane, start Claude/Codex

    O->>H: horch sessions
    H-->>O: past workers, summaries, gotchas
    O->>H: horch route <teammate> (optional dry run)
    H-->>O: spawn, substitute a fallback, or refuse (exit 3)
    O->>H: horch spawn <teammate> "Read and follow plan.md"
    H->>H: plan: roster + quota/routing + skills (no I/O)
    H->>L: insert record (Planned)
    H->>W: write brief, split pane, run `horch worker`
    H->>L: Starting, pane id recorded
    W->>L: Running; session id recorded (minted or discovered)
    W->>W: agent works

    W->>H: horch note "progress"
    H->>L: append note
    W->>H: horch tell orchestrator "QUESTION: ..."
    H->>O: text arrives in the orchestrator pane
    O->>H: horch tell <role> "ANSWER: ..."  (or horch assign <role> "task")
    H->>W: text arrives in the worker pane

    W->>H: horch done "summary"
    H->>L: Done + summary
    H->>O: DONE message
    H->>W: start a detached re-tile, close the pane
```

Side commands any time: `horch inbox` (live roles), `horch layout` /
`horch tile` / `horch balance` (the pane grid), `horch quota` (the usage
pools: claude, codex, opencode-zen, google, local), `horch agent-list` (the
agent CLIs, their versions and pool state), `horch spawn --resume <id>`
(continue an old worker's session).

An `antigravity` worker draws on the `google` pool. That pool has no probe, so
`horch quota` shows it as `unknown`. `horch cost` cannot price an
`antigravity` session: `agy` writes no local usage file.

## 3. A dataset round

```mermaid
flowchart TD
    R["multi-herdr-dataset run &lt;task&gt;<br/>--candidates N --budget-usd X"] --> PF{"preflight PRE-01..PRE-13<br/>git and --promote-to · disk · RAM · CPU/GPU · limits<br/>harness --version · provider pools · parallelism<br/>budget · judge · storage · herdr"}
    PF -->|"a check fails"| X4["exit 4<br/>no worktree, no model call"]
    PF -->|"pass, but no candidate fits the usage limits"| X3["exit 3"]
    PF -->|"pass"| PLAN["plan the round<br/>(which teammates, labels A, B, C…)"]
    PLAN --> WT["N git worktrees from one base commit"]
    WT --> WS["dataset workspace in herdr<br/>(root pane runs `watch`)"]
    WS --> SP["spawn candidates in waves<br/>(same spawn service as horch spawn)"]
    SP --> OBS{"each tick: observe"}
    OBS -->|"horch done"| FZ
    OBS -->|"crash / pane gone / deadline"| FZ
    OBS -->|"spend reaches the budget limit<br/>(hard ceiling minus judge reserve)"| CANCEL["cancel the running candidates<br/>(data kept)"] --> FZ
    OBS -->|"spend + committed reaches the limit / low disk"| STOP["no new launches<br/>(running candidates go on)"] --> OBS
    FZ["freeze (commit) + validate<br/>(gates from .multi-herdr/dataset.yaml)"] --> EL{"any eligible?<br/>(completed and every gate passed)"}
    EL -->|"no"| REJ["REJECTED"]
    EL -->|"yes"| JB["blind judge bundle<br/>(no model names, no cost)"]
    JB --> JJ["judge-job (detached)<br/>claude -p, read-only tools"]
    JJ --> PARSE{"strict parse +<br/>winner policy"}
    PARSE -->|"failed twice / tie / abstain / low confidence"| NI["NEEDS_INTERVENTION · exit 5<br/>(worktrees kept)"]
    PARSE -->|"reject all / winner not eligible"| REJ
    PARSE -->|"winner"| DEC["DECIDED"]
    DEC -->|"no --promote-to (default)"| CL["cleanup worktrees<br/>(branches kept)"]
    DEC -->|"--promote-to B"| PROM{"revalidate on B,<br/>publish by compare-and-swap"}
    PROM -->|"ok, receipt written"| CL
    PROM -->|"conflict / dirty checkout / ref moved"| NI
    PROM -->|"stale or gates fail"| REJ
    REJ --> CL
    CL --> DONE["COMPLETE"]
```

The last line of `run` names the outcome, and the exit code follows it:

| outcome | exit |
|---|---|
| DECIDED or PROMOTED (the round then reaches COMPLETE) | 0 |
| REJECTED because the budget stopped the candidates | 3 |
| NEEDS_INTERVENTION | 5 |
| REJECTED for any other reason | 6 |

Source: `outcome_line` in `crates/horch/src/dataset/run.rs`; the codes are in
`crates/horch/src/dataset/mod.rs` (`exit`).

Every arrow above is recorded as an event. After a crash or `kill -9`:

```mermaid
flowchart LR
    K["coordinator killed"] --> RS["multi-herdr-dataset resume &lt;exp&gt;"]
    RS --> RD["read events, adopt existing<br/>executions and the judge job"]
    RD --> CONT["continue the round<br/>(no duplicate candidates,<br/>judgments, or promotions)"]
```

## 4. After a round (operator commands)

`promote`, `rollback` and `cleanup` are hidden from `--help`, but they work.

```mermaid
flowchart LR
    NI["NEEDS_INTERVENTION<br/>(with a winner) or COMPLETE"] -->|"promote &lt;round&gt; --to B"| P["promotion engine"]
    P --> PR["PROMOTED + receipt"]
    PR -->|"rollback &lt;round&gt;"| RB["target ref moved back<br/>(only if nobody moved it since)"]
    NI -->|"cleanup &lt;round&gt; --force"| C["worktrees removed<br/>(branches kept unless --prune-branches)"]
    DONE["decided rounds"] -->|"outcome &lt;round&gt; --kind verified/regression/revert"| OUT["post-merge score"]
    DONE -->|"export"| EX["JSONL dataset"]
    EX -->|"readiness"| RE["enough data per arm?"]
```

## Together or apart

| Command | Needs a herdr server | Needs a fleet | Spends model money |
|---|---|---|---|
| `horch fleet` / `herdr-fleet` | yes | no (it creates one) | yes, the orchestrator |
| `horch spawn`, `tell`, `assign`, `inbox`, `tile` | yes | run inside a fleet pane | `spawn` does |
| `horch done` | yes | run by a worker | no |
| `horch note` | no (it writes the ledger only) | run by a worker | no |
| `horch sessions`, `cost`, `usage`, `quota`, `route` | no | no | no (`quota --refresh` probes) |
| `horch agent-list` | no | no | no (it runs each agent CLI's `--version` unless `--no-probe`) |
| `multi-herdr-dataset run` / `resume` | yes | no (own workspace) | yes, candidates and judge |
| `multi-herdr-dataset status`, `export`, `readiness`, `outcome`, `rebuild` | no | no | no |
| `multi-herdr-dataset promote`, `rollback`, `cleanup` | no | no | no (git, plus the gates on a promote) |

The two systems run independently. A dataset round does not need a fleet,
and it never messages the fleet's orchestrator. What they share is the spawn
service and the execution ledger, so `horch cost` counts candidates and the
judge. `horch sessions` hides candidates and the judge unless you pass `--all`.
