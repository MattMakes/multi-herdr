# How the fleet builds this repo

Much of this repository was built by a horch fleet working on itself. This
page tells you how an orchestrator runs a batch of work ("a run"), so you can
read its files, join it as a worker, or run one yourself.

## Roles

| role | who | commands |
|---|---|---|
| orchestrator | 1 pane, started by `horch fleet` | `horch sessions`, `horch spawn`, `horch assign`, `horch tell`, `horch route`, `horch inbox` |
| worker | 1 pane per unit of work, started by `horch spawn` | `horch note`, `horch tell orchestrator`, `horch done` |

The worker briefing is `teammates/_base/fleet-worker.md`; the orchestrator
briefing is `teammates/_base/fleet-orchestrator.md`; the orchestrator's
playbook is the bundled skill `skills/orchestrate/SKILL.md`. A worker never
starts a subagent. A worker that needs more hands sends `QUESTION:` to the
orchestrator, and the orchestrator spawns another pane.

## Plan files

`ai_docs/` is local scratch. It is not in git (no commit holds it), so a
fresh clone has no `ai_docs/`. A run keeps its plans there, in `ai_docs/plans/<run>/`:

| file | what it holds |
|---|---|
| `00-conventions.md` (or `00-common.md`) | the rules every unit follows: sources, the branch mode, shared files, the gate, hard rules |
| `<unit>.md` (for example `d09-agent-list.md`) | 1 unit: `GOAL`, `CONTEXT`, `FILES` (`own` and `do not touch`), `STEPS`, and often an `EXECUTION` section the orchestrator adds |
| `<unit>-impl.md` | optional: a detailed implementation plan a planner wrote for an executor |
| `STATUS.md` | the table of units, workers and states (`running`, `MERGED`, `queued`), the merge order and open notes |

The orchestrator spawns a worker with the task "Read and follow `<plan
path>` exactly." The plan file is the contract. A worker edits only the files
under `own`. A worker that needs another file asks first.

## Phases and skills

A spawn can name a phase: `research`, `plan`, `implementation` or
`validation`. The phase selects a catalog of bundled skills (for example
`execute`, `tdd`, `debug`, `check`, `handoff` for implementation). A
teammate's own `skills:` attach by name, whatever the phase. See
[phase-skills.md](phase-skills.md).

A planner (for example `staff-engineer`) works in the plan phase and writes a
plan, not the deliverable. The orchestrator then hands the plan to an
executor in the implementation phase.

## Branch modes

### Single-branch mode (current)

All workers share 1 checkout and 1 branch. For the design-skills run, the
checkout is the main checkout and the branch is `design-skills`.

- Nobody creates a worktree for a unit. Scratch repos for experiments go in
  `.worktrees/_scratch/`, which is git-ignored and inside the trusted project
  folder. Never use `/tmp`.
- A worker edits only the files in the `FILES` section of its plan.
- The tree must compile at every commit, because other workers build it.
  Never leave a half-edited `.rs` file while you wait.
- A worker commits only its own paths:

  ```bash
  git commit -m "<Area>: <what changed>" -- <path> <path>
  ```

  A pathspec commit does not pick up the staged or unstaged changes of another
  worker. Never run `git add -A`, `git add .`, `git commit -a`, `git stash`,
  `git reset` or `git rebase`. Never run `git checkout -- <path>` on a file
  you do not own.
- After each commit, the worker runs `git show --stat HEAD`. Every file in it
  must be the worker's own.
- If git reports that `index.lock` exists, wait 2 s and retry. Never delete it.
- Nobody pushes. The orchestrator pushes after the gate passes.
- While it works, a worker runs only the tests it touches. The orchestrator
  runs the full gate once on the tip, in `.worktrees/_gate`, and sends a red
  gate back to the worker whose commit broke it. See
  [testing-and-gates.md](testing-and-gates.md).
- Warning: never run tests or the gate under `git rebase -x` or a git hook.
- A Codex worker's sandbox cannot write `.git`, so units that commit go to
  Claude teammates.

The worker reports with `[<role>] NOTE: COMMITTED <short-sha>. Targeted checks
green.` and then runs `horch done`.

### Worktree mode (older alternative)

The first runs gave each unit its own branch and worktree. Use it only when
the operator asks for it.

- The orchestrator owns 1 integration worktree on the run's branch. Nobody
  else edits, commits or builds there.
- Each unit gets its own branch `ds/<unit>` in its own worktree, created from
  the integration branch:

  ```bash
  git -C <main checkout> worktree add -b ds/<unit> <main checkout>/.worktrees/<unit> design-skills
  cd <main checkout>/.worktrees/<unit>
  ```

- Unit worktrees live under `.worktrees/` in the main checkout. Each builds
  its own `target/` (2 to 8 GB), so the orchestrator removes a worktree as soon
  as its unit merges. Never share one `CARGO_TARGET_DIR` between worktrees.
- Nobody runs `git checkout` or `git switch` in the shared main checkout.

When a unit is complete, the worker:

1. rebases on the integration branch: `git -C <worktree> rebase design-skills`;
2. runs the gate again (`HORCH_REQUIRE_GIT=1 HORCH_REQUIRE_SQLITE=1 just gate`);
3. sends exactly 1 line:
   `horch tell orchestrator "[<role>] NOTE: READY-TO-MERGE ds/<unit> <short-sha>. Gate green. See <report path>."`;
4. waits. On `REBASE` it repeats 1 to 4. On `MERGED` it runs
   `horch done "<summary>"`.

The orchestrator merges, runs the gate on the integration branch, and updates
`STATUS.md`. Its merge helpers are operator-local scripts, not in the
repository.

Shared files (for example `skills/README.md`, `skills/copied.json`,
`SKIP_NEW_TEAMMATES`) get lines from several units. Each unit adds its lines
in alphabetical order and keeps both sides at a rebase conflict.

## Reports

Each unit writes a report to `ai_docs/reports/<run>/<unit>.md`. In
single-branch mode the report is local scratch and the worker does not commit
it; the `horch done` summary carries the result. In worktree mode the unit
commits it. A report
holds: what changed, decisions and why, what was dropped, every oracle or
golden change with a diff summary, gotchas, what was verified only against a
fake, and follow-ups outside the unit's scope. Reports are the best place to
learn why the code is the way it is.

## Messages

Every `horch tell`, `horch note` and `horch done` message uses Simplified
Technical English: short sentences, active voice, 1 fact per sentence. A
report starts with the role tag and 1 keyword:

```
[opus-35] DONE: The report is at ai_docs/reports/x.md. I changed 2 files. Tests pass: 14 of 14.
[opus-35] QUESTION: I need 1 decision before I edit src/parse.rs. ...
```

The keywords are `ready`, `DONE:`, `BLOCKED:`, `NOTE:` and `QUESTION:`. The
rule lives in the 2 briefings in `teammates/_base/`.

## The ai_docs map

`ai_docs/` is local scratch and is not in git. Its directories:

| directory | holds |
|---|---|
| `ai_docs/plans/<run>/` | plans: 1 directory per run |
| `ai_docs/reports/<run>/` | reports: 1 directory per run |
| `ai_docs/checkpoints/<run>/` | execution-state snapshots for a resumed run |
| `ai_docs/reflections/<run>/` | what went well and badly after a run |

The functional specs are not in `ai_docs/`. They are in `docs/specs/*.md`.
Their requirement tables hold the IDs that the coverage check reads.

## Never use an API key

Every `claude` run uses the operator's claude.ai login. Never read, set,
export or pass `ANTHROPIC_API_KEY`. If you run `claude` yourself, remove it:
`env -u ANTHROPIC_API_KEY claude ...`. horch itself removes it from every
child process (`FORBIDDEN_ENV` in `crates/horch-core/src/harness/launch.rs`).
