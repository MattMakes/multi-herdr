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

A run lives in `ai_docs/plans/<run>/` (example: `ai_docs/plans/design-skills/`):

| file | what it holds |
|---|---|
| `00-conventions.md` | the rules every unit follows: sources, worktrees, shared files, the gate, hard rules, the merge protocol |
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

## Worktrees

- The orchestrator owns 1 integration worktree on the run's branch (for the
  design-skills run: `design-skills`). Nobody else edits, commits or builds
  there.
- Each unit gets its own branch `ds/<unit>` in its own worktree, created from
  the integration branch:

  ```bash
  git -C <main checkout> worktree add -b ds/<unit> <main checkout>/.worktrees/<unit> design-skills
  cd <main checkout>/.worktrees/<unit>
  ```

- Unit worktrees live under `.worktrees/` in the main checkout. The
  directory is in `.gitignore`. Each worktree builds its own `target/`
  (2 to 8 GB), so the orchestrator removes a worktree as soon as its unit
  merges. `00-conventions.md` of the run gives the exact path (for the
  design-skills run: section 8; units started before it finish in their old
  directory).

- Each worktree uses its own `CARGO_TARGET_DIR` (the default `target/` in
  the worktree). Sharing one corrupts builds and fakes.
- Nobody runs `git checkout` or `git switch` in the shared main checkout.
  Workers share it.
- A Codex worker's sandbox cannot write `.git` of a worktree, so units that
  commit go to Claude teammates.

## The merge protocol

When a unit is complete, the worker:

1. rebases on the integration branch: `git -C <worktree> rebase design-skills`;
2. runs the gate again (`HORCH_REQUIRE_GIT=1 HORCH_REQUIRE_SQLITE=1 just gate`);
3. sends exactly 1 line:
   `horch tell orchestrator "[<role>] NOTE: READY-TO-MERGE ds/<unit> <short-sha>. Gate green. See <report path>."`;
4. waits. On `REBASE` it repeats 1 to 4. On `MERGED` it runs
   `horch done "<summary>"`.

The orchestrator merges, runs the gate on the integration branch, and updates
`STATUS.md`. Its merge and gate helpers live in `/tmp` (`STATUS.md` names
`/tmp/igate-ds.sh` and `/tmp/dsmerge.sh`). They are not in the repository,
and this page does not describe them.

Shared files (for example `skills/README.md`, `skills/provenance.json`,
`SKIP_NEW_TEAMMATES`) get lines from several units. Each unit adds its lines
in alphabetical order and keeps both sides at a rebase conflict.

## Reports

Each unit writes `ai_docs/reports/<run>/<unit>.md` and commits it. A report
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

| directory | holds |
|---|---|
| `ai_docs/designs/` | designs with requirement tables (the IDs the coverage check reads) |
| `ai_docs/plans/` | plans: 1 directory per run, plus older single-file plans |
| `ai_docs/reports/` | reports: 1 directory per run, plus older single-file reports |
| `ai_docs/gates/` | gate inputs, for example `architecture-refactor/CURRENT_PHASE` |
| `ai_docs/checkpoints/` | execution-state snapshots for a resumed run |
| `ai_docs/reflections/` | what went well and badly after a run |

## Never use an API key

Every `claude` run uses the operator's claude.ai login. Never read, set,
export or pass `ANTHROPIC_API_KEY`. If you run `claude` yourself, remove it:
`env -u ANTHROPIC_API_KEY claude ...`. horch itself removes it from every
child process (`FORBIDDEN_ENV` in `crates/horch-core/src/harness/launch.rs`).
