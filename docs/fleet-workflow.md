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
`horch inbox` lists the reachable roles, then prints and acknowledges pending
messages, and ends with `N messages acknowledged`. `--roles` lists roles only.
`--peek` prints messages and leaves them pending.

The role list includes agent state and unanswered questions. `horch peek
<role> --lines 20` reads recent pane output without typing. A relay write does
not acknowledge a message; only `inbox` does. The courier keeps retrying pending
mail and pauses with an operator notification when Claude needs `/login`.

The operator starts the orchestrator with no task. Its briefing ends with
"Acknowledge and await further instruction.", so it acknowledges the briefing
and waits for the operator's first message. A worker always gets its task at
spawn.

## Task ledger runs

A run can be driven by the task ledger (`docs/specs/task-ledger.md`). Then
the orchestrator only queues work, and workers claim it:

| step | who | command |
|---|---|---|
| create the run and its run branch | orchestrator | `horch ledger run new <slug>` |
| queue the tickets | orchestrator | `horch ledger add-map <MAP.md>` or `horch ledger add --file <ticket> --primary <persona>` |
| spawn exactly the workers needed | orchestrator | `horch ledger plan-workers`, then `horch spawn <persona>` with no task |
| take a task | worker | `horch ledger claim` |
| record milestones | worker | `horch ledger progress "<text>"` |
| finish: check, code-sweep, complexity-sweep, recheck, land, pr, cleanup | worker | `horch ledger finish` (again after each fix) |
| close the task | worker | `horch ledger done "<summary>"` |
| read the board | anyone | `horch ledger status` (exit 0 = all done) |

`horch worker` records the supervisor's and the agent's PIDs and checks in
every 30 s. A watchdog checks every working worker every 10 minutes and
reaps a worker whose process is gone or that shows no progress for 3
checks; its task is queued again first, and the replacement
(`horch spawn <persona> --replaces <worker>`) gets the dead worker's
transcript path and worktree. `horch ledger stop` is the only way to stop a
worker. `horch ledger unfinished` lists past work that never landed,
including the history imported from the old JSON ledgers
(`horch ledger import`).

An outstanding review, unanswered question or `progress "waiting: ..."` note
gives a worker a waiting reason and deadline. These waits suspend stall counting
while valid. The counter starts fresh after the wait ends; a gone agent is
still reaped immediately. Status lists the reason and age.

Use `horch ledger worktree new <name> --from <ref>` for a different base, and
`horch ledger worktree scratch <name>` for work that will not land. Drop a clean
tree with `horch ledger worktree drop <name> --reason "<why>"`; use
`--keep-branch` to retain unique commits. No command forces removal.

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

### Ledger worktree mode (current)

In a task ledger run, each task gets its own worktree and branch, and
`horch ledger finish` lands it.

- The claim tells the worker when to make the worktree. The worker runs
  `horch ledger worktree new <name>` and works in the path it prints. The
  branch starts from the run branch, or from `--from <ref>`. The ledger
  records the row, so `finish` can check, land and clean it up.
- A worker never runs `git worktree add` or the plain `horch worktree new`
  in a ledger run. A worktree that the ledger does not record is never
  checked or landed.
- `horch ledger worktree scratch <name>` makes recorded throwaway work that
  never lands. `horch ledger worktree drop <name> --reason "<why>"` drops a
  clean worktree that will not land.
- Worktrees live under `.worktrees/` in the main checkout. Each builds its
  own `target/` (2 to 8 GB). Never share one `CARGO_TARGET_DIR` between
  worktrees.
- A worker commits exact files in its worktree, then runs
  `horch ledger finish`. Its `land` step rebases the branch on the run
  branch, runs the check again, and fast-forwards the run branch. Its
  `pr` step writes the task's section in the run's 1 pull request. Its
  `cleanup` step removes the worktree.
- Nobody runs `git checkout` or `git switch` in the shared main checkout.

### Shared-checkout mode (`horch ledger add --shared`)

A task queued with `--shared` works in the main checkout, not a worktree.
It is for repositories where worktrees are impractical (large game assets,
Godot imports). The ledger enforces these limits (#166):

- At most 1 shared task runs at a time. A 2nd shared task waits with
  "exclusive: shared checkout".
- The main checkout must be on the run branch. Otherwise `finish` refuses
  and prints the `git switch <run branch>` line for the orchestrator.
- `check` needs a clean checkout, and `land` needs at least 1 commit since
  the claim.

In this mode:

- Scratch repos for experiments go in `.worktrees/_scratch/`, which is
  git-ignored and inside the trusted project folder. Never use `/tmp`.
- A worker edits only the files in the `FILES` section of its plan.
- The tree must compile at every commit. Never leave a half-edited `.rs`
  file while you wait.
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
- Warning: never run tests or the gate under `git rebase -x` or a git hook.

### Plain worktree mode (no ledger run)

A fleet that does not use a ledger run gives each unit its own branch and
worktree with the plain `horch worktree` commands.

- The orchestrator owns 1 integration worktree on the run's branch. Nobody
  else edits, commits or builds there.
- Each unit gets its own branch in its own worktree, created from the
  integration branch:

  ```bash
  horch worktree new <unit>
  cd <the path that horch prints>
  ```

- The orchestrator removes a worktree as soon as its unit merges.

When a unit is complete, the worker runs
`horch worktree land --check "<gate command>"`. The command rebases the
branch, runs the gate, fast-forwards the integration branch, and removes the
worktree. On a conflict, the worker rebases, runs the check, and runs the land
command again. `horch done` refuses while the worker has an open claim.
`horch done --unlanded "<reason>" "<summary>"` is the last resort. It keeps
the branch and reports it to the orchestrator.

Shared files (for example `skills/README.md`, `skills/copied.json`,
`SKIP_NEW_TEAMMATES`) get lines from several units. Each unit adds its lines
in alphabetical order and keeps both sides at a rebase conflict.

## Reports

Each unit writes a report to `ai_docs/reports/<run>/<unit>.md`. The
report is local scratch and the worker does not commit it; the `horch done`
summary carries the result. A report
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
