# How the fleet builds this repo

Much of this repository was built by a horch fleet working on itself. This
page tells you how fleetd turns your ideas into landed work, so you can read
its files, follow a worker, or run a fleet yourself. The first steps are in
the section "Start" of the [README](../README.md#start).

## Roles

| role | who | commands |
|---|---|---|
| you | the operator | `horch fleet`, `horch idea add`, `horch idea show`, `horch idea approve`, `horch idea revise`, `horch ideas`, `horch ledger status`, `horch feature abandon` |
| fleetd | 1 daemon for each ledger, under the service manager of the OS | the only ledger writer: dispatch, the verifier, landing, the jobs |
| sub-orchestrator | 1 pane for each feature (`idea-lead`), started by fleetd | `horch idea note`; it writes the files of its idea folder |
| worker | 1 pane for each attempt, started by fleetd | `horch ledger progress`, `horch ask`, `horch result`, `horch ledger handoff` |

The worker briefing is `teammates/_base/fleet-worker.md`; the
sub-orchestrator briefing is `teammates/_base/fleet-idea-lead.md`. A worker
never starts a subagent. A worker that needs more hands asks with
`horch ask`, and the sub-orchestrator of its feature replans.
`horch inbox` lists the reachable roles, then prints and acknowledges pending
messages, and ends with `N messages acknowledged`. `--roles` lists roles only.
`--peek` prints messages and leaves them pending. Inside a fleet, only the
registered orchestrator pane acknowledges the orchestrator's mail: another pane
gets the messages as with `--peek`, and 1 stderr line that says so. A worker's
`inbox` prints and acknowledges only the mail to its own role.

The role list includes agent state and unanswered questions. `horch peek
<role> --lines 20` reads recent pane output without typing. A relay write does
not acknowledge a message; only `inbox` does. The courier keeps retrying pending
mail and pauses with an operator notification when Claude needs `/login`.

Every mail line shows its mail id: `[<role> #<id>] <KEYWORD>: <text>`. A
redelivery repeats the id. The orchestrator answers 1 question with
`horch tell <role> --answers <id> "<text>"`, and the worker sees
`[orchestrator re #<id>] <text>`. A tell without `--answers` closes no
question.

Push is the only dispatch model: fleetd binds each task to its attempt
before the pane opens, and writes `<task folder>/inbox/START.md`. A ticket
that you wrote goes in with `horch idea add --direct <ticket file>`. The 1
exception is a `spawn_only` controller, such as `skill-importer`, that sends
a brief to a role with `horch_access: report`: the brief is its task.

## From an idea to landed work

Every task belongs to an idea (`docs/specs/fleetd/design.md`, Ideas). fleetd
moves each idea through a fixed state machine:

| step | who | command or file |
|---|---|---|
| capture an idea | you | `horch idea add "<idea>"`, `horch idea add -f <file>`, `--to <feature>` to join an open feature |
| add a ticket that you wrote | you | `horch idea add --direct <ticket file>` (no interview, 1 work task) |
| interview, assumptions | sub-orchestrator | `interview.md`, `assumptions.md` in the idea folder |
| approve or revise | you | `horch idea show <id>`, then `horch idea approve <id>` or `horch idea revise <id> "<text>"` |
| spec and tickets | sub-orchestrator | `spec.md`, `requirements.yaml`, tickets in format C1 (`docs/specs/ticket-format.md`), `plan.yaml` |
| check a plan by hand | anyone | `horch ledger add-map --dry-run <file>`: the plan validator on a `MAP.md` or a `plan.yaml` (writes nothing) |
| validate the plan, make the tasks | fleetd | `plan.verdict.json` |
| dispatch a ready task | fleetd | the worktree, `inbox/START.md`, a worker in exec mode |
| record milestones | worker | `horch ledger progress "<text>"` |
| ask a question | worker | `horch ask "<question>"`; the sub-orchestrator answers with `horch idea note` |
| report the result | worker | `horch result --status completed --summary "<paragraph>"` |
| hand the task to a fresh attempt | worker | `horch ledger handoff <path>` |
| the finish steps (check, code-sweep, complexity-sweep, recheck) | fleetd | jobs of the verifier |
| land the task branch on the feature branch | fleetd | the landing state machine, behind the fast gate |
| the full gate and the pull request | fleetd | the feature branch is pushed for 1 pull request |
| read the board | anyone | `horch ideas`, `horch ledger status` (each hold on dispatch with its cause) |
| merge | you | merge the pull request; fleetd sees the merge |
| drop a feature | you | `horch feature abandon <feature>` |

A task with no check does not land, unless its ticket says
`**Check:** none`. The orchestrator commands of the old flow
(`horch ledger run new`, `add-map` without `--dry-run`, `add`, `task`,
`plan-workers`, `run gate`, `run close`, `unblock`, `reap`, `spawn`,
`worker new`, `worker pid`, and `horch spawn`) and the worker commands
`horch ledger claim` and `horch ledger finish` are retired. Each one prints
the command that replaces it. The table "The old flow and the new" in
[command-flow.md](command-flow.md#2c-the-old-flow-and-the-new) lists each
step of the old flow and what you do now.

The verifier starts the code-sweep reviewers itself. A ticket's
`**Review:**` sets the rounds (none 0, light 1, full at most 3). Rounds 2
and 3 review only the change since the last reviewed tree, with
`codex-reviewer-light` and `architect-reviewer-light`. Findings keep their
`F<n>` id in every round. The author answers in the answers file
(`F<n>: <reason>`), and the next round's reviewers write `- F<n> accepted`
or `- F<n> rejected: <why>` under `## Answers`.

fleetd records each attempt at its launch, and stops an orphan worker by
its recorded PID. The next attempt of the task gets the transcript path and
the worktree of the attempt before it. You keep 4 powers:
`horch ledger stop <worker> "<reason>"` stops 1 worker,
`horch task cancel <task>` cancels 1 task and stops its worker,
`horch ledger fallback` gives 1 task to another persona, and
`horch idea add --direct <ticket file>` adds 1 task by hand.
`horch idea cancel <idea> "<reason>"` cancels a whole idea.
`horch task requeue <task>` queues a blocked task again. `horch ledger unfinished` lists past work that never landed,
including the history imported from the old JSON ledgers
(`horch ledger import`). Session records live in the `session_records`
table of the task ledger; `import` copies an old JSON session ledger there
once.

An outstanding review, unanswered question or `progress "waiting: ..."` note
gives a worker a waiting reason and deadline. These waits suspend stall counting
while valid. The counter starts fresh after the wait ends; a gone agent is
still reaped immediately. Status lists the reason and age.

Use `horch ledger worktree new <name> --from <ref>` for a different base, and
`horch ledger worktree scratch <name>` for work that will not land. Drop a clean
tree with `horch ledger worktree drop <name> --reason "<why>"`; use
`--keep-branch` to retain unique commits. No command forces removal.
A worktree that no step takes to an outcome is decided in a worktree triage
(see "Worktree triage").

## Worktree triage

A worktree is *parked* when no step takes it to an outcome: its worker did
not land it (`horch ledger done --unlanded`), its task was cancelled, its run
was abandoned, its cleanup failed, or git knows a folder under `.worktrees/`
that the ledger does not. A parked worktree is often the only copy of work
that cost real time, and each one holds a full checkout and its build output.
The operator decides what happens to each one. Code collects the facts and
applies the decisions; the `worktree-steward` teammate adds judgement.

| step | who | command |
|---|---|---|
| list the parked worktrees with their facts | steward | `horch worktree triage` (`--json` for machines) |
| show the facts, a relevance, a suggestion and a reply table | steward | (in its pane) |
| fill `Decision` (and `Direction` for `FINISH` and `PR`) | operator | (in the steward's pane) |
| run the decisions | steward | `horch worktree triage apply <file>` (`-` reads stdin) |
| bring an archived or trashed folder back | anyone | `horch worktree restore <name>` |
| delete trash refs older than 30 days | anyone | `horch ledger gc` |

`horch fleet` runs the triage at start. When it finds 1 or more parked
worktrees, it spawns `worktree-steward` in a pane beside the first pane.
With none, no steward starts.

Each row of `horch worktree triage` has an ID (`W1`, `W2`, ... in name order)
and: the task, findings and origin; the branch, path and base; the created,
last commit and newest file times; the disk use and its ignored share (build
output); ahead and behind the base; the diffstat and its 3 largest paths; the
modified, staged and untracked counts, with the 3 largest untracked files;
`superseded` (every commit has a patch-equivalent commit in the base, from
`git cherry`); the conflict count of a merge into the base (`git merge-tree`);
the last finish step; and why it is parked.

The reply is a markdown table with the columns `ID`, `Worktree`, `Suggested`,
`Decision` and `Direction`. Code parses it, never a model. `apply` refuses the
whole reply, and runs nothing, when a line has an unknown ID, a `Worktree`
that no longer matches its ID, an empty or unknown decision, a `FINISH` or
`PR` with no `Direction`, a `PR` with no `gh` or no git remote, or an ID that
appears twice. Each error names its line.

| decision | effect |
|---|---|
| `LAND` | Commits the uncommitted files, then rebases the branch onto the branch that the main checkout has checked out. On the default branch (`main`, `master` or `origin/HEAD`) that branch does not move: the work goes to a new branch `land/<name>`. On a feature branch, that branch fast-forwards under the land lock. Then the folder is removed. A conflict changes nothing and reports `conflicts: choose FINISH`. |
| `PR` | Pushes the branch, opens a pull request with the first line of `Direction` as its title and `Direction` as its body, then archives the folder. Offered only when `gh` and a git remote exist. |
| `FINISH` | Writes `Direction` to `ai_docs/triage/<name>.md` and queues a task with it as the brief (`--primary <persona>`, default `opus`). The task inherits the worktree, and the orchestrator is woken. Needs an active run. |
| `ARCHIVE` | Commits the uncommitted files, writes `refs/horch/archive/<name>` at the tip, removes the folder and keeps the branch. `horch worktree restore <name>` makes the folder again. |
| `KEEP` | Changes nothing. The row comes back at the next triage. |
| `DELETE` | Commits the uncommitted files, writes `refs/horch/trash/<name>/<UTC time>` at the tip, removes the folder, then deletes the branch once the trash ref is read back at the tip. `horch ledger gc` deletes the trash ref after 30 days; until then `horch worktree restore <name>` brings it back. |

A superseded row suggests `DELETE`. A row that does not merge clean suggests
`FINISH` or `ARCHIVE`, never `LAND`. Ignored files are never committed: an
`ARCHIVE` or `DELETE` drops build output and keeps everything else.

The rules: no git call forces (`git worktree remove` without `--force`,
never `git branch -D`). Before a folder is removed, `unsafe_to_remove`, or an
archive or trash ref that holds the tip, proves that no commit and no
uncommitted file is lost. A failed decision deletes nothing: the row becomes
`cleanup_failed` with the reason, and has no decision. `LAND` never moves the
default branch. Only `gc` deletes a ref that may hold the only copy of a
commit, and only a trash ref older than 30 days.

Each decision is recorded on the worktree row (`decision`, `decided_at`, and
`archive_ref` and `bytes_freed` where they apply) and as a ledger event. An
open `import_findings` row of the worktree closes: `requeued` for `FINISH`,
`dismissed` for the others. `ARCHIVE` and `PR` leave the row `archived`;
`DELETE` leaves it `trashed`.

A parked worktree with no decision stays on disk, and
`horch ledger status` prints a
`Parked worktrees: <count> (<disk>, measured <age> ago)` line. The disk is the
size `horch worktree triage` stored at its last run, and `<age>` is the age
of the oldest measurement; `status` walks no folder. Rows that triage has not
measured add `; <n> not measured`. With no measurement the line says
`(disk not measured; run horch worktree triage)`.

## Tickets and plan files

The tickets are the contract. They live in git, in `docs/tickets/<slug>/`,
beside the spec `docs/specs/<slug>.md`. Each ticket starts with the field
block of `docs/specs/ticket-format.md`. fleetd stores a snapshot of each
ticket of a validated plan, and `START.md` holds it. A worker edits only the `**Own:**` paths of
its ticket. A worker that needs another file asks first.

`ai_docs/` is local scratch. It is not in git (no commit holds it), so a
fresh clone has no `ai_docs/`. A run keeps its notes there, in `ai_docs/plans/<run>/`:

| file | what it holds |
|---|---|
| `00-conventions.md` (or `00-common.md`) | the rules every unit follows: sources, the branch mode, shared files, the gate, hard rules |
| `decisions.md` | the operator's decisions, their reasons and the rejected options |
| `brief.md` | the operator's idea in their words, the input of the spec-writer task |
| `<unit>-impl.md` | optional: a detailed implementation plan a planner wrote for an executor |

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

fleetd gives each task its own worktree and branch, and lands it.

- fleetd makes the worktree before the attempt starts. `START.md` names the
  worktree and the branch. The branch starts from the feature branch.
- A worker never runs `git worktree add`, `horch worktree new` or the
  `horch ledger worktree` commands. It works only in the worktree that
  `START.md` names.
- Worktrees live under `.worktrees/` in the main checkout. Each builds its
  own `target/` (2 to 8 GB). Never share one `CARGO_TARGET_DIR` between
  worktrees.
- A worker commits exact files in its worktree, then runs `horch result`.
  The verifier runs the finish steps. The landing state machine lands the
  task branch on the feature branch, and fleetd removes the worktree when no
  work can be lost. A branch that does not land keeps a rescue ref.
- Nobody runs `git checkout` or `git switch` in the shared main checkout.

### Shared-checkout mode (retired with `horch ledger add --shared`)

`horch ledger add` is retired, so no new task is queued in this mode. The
rules below apply to a shared task of an older run. A task queued with
`--shared` works in the main checkout, not a worktree.
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
- A worker edits only the `**Own:**` paths of its ticket.
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
the branch and reports it to the orchestrator. In a ledger run it parks the
task with its worktree; its dependents wait, and `horch ledger requeue`
resumes it on the same worktree.

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

Every `horch tell`, `horch ledger progress` and `horch ledger done` message uses Simplified
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
