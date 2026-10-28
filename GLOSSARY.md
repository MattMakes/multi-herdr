# horch fleet

A horch fleet is 1 orchestrator and the workers it spawns in herdr panes,
coordinated through the task ledger, to turn vetted ideas into work landed
on the main branch.

## People and sessions

**Operator**:
The human who talks with the orchestrator, vets ideas and answers its
questions.
_Avoid_: user, invoker

**Orchestrator**:
The 1 agent session per run that turns vetted ideas into queued tasks and
spawns workers; it does not do the work itself.
_Avoid_: lead, manager

**Worker**:
1 agent process life in 1 pane, with 1 supervisor PID, 1 agent PID and 1
session, that claims at most 1 task and exits when it is finished.
_Avoid_: agent (alone), seat, slot

**Persona**:
The teammate a worker runs as (model, effort, briefing), named on a task as
the primary or fallback that may claim it.
_Avoid_: tier, worker type, role

**Role**:
A worker's address in a workspace, such as `sonnet-feature-3`, used by
`horch tell`.
_Avoid_: name, persona

## Work

**Idea**:
Something the operator wants built, discussed with the orchestrator until
it is vetted; a vetted idea becomes a spec and tickets.

**Ticket**:
The file (and optionally the tracker issue) that describes 1 unit of work
for a worker to read.
_Avoid_: plan file, issue (alone)

**Task**:
The ledger's record of 1 ticket: who may claim it, what blocks it, its
state and its attempts.
_Avoid_: job, unit, assignment

**Run**:
1 orchestrator session's body of work: its tasks, its run branch and its 1
pull request.
_Avoid_: session, batch

**Run branch**:
The integration branch the orchestrator creates from the main branch for a
run; every task of the run lands on it.
_Avoid_: integration worktree, feature branch

**Attempt**:
1 worker's try at 1 task; a task retried after a dead worker has a new
attempt with a new worker.

**Claim**:
A worker taking the next task whose persona matches its own.
_Avoid_: assign, pick up

**Finish step**:
1 of the ordered, evidence-checked steps every attempt passes before it
can be done: check, code-sweep, complexity-sweep, recheck, land, pr,
cleanup.
_Avoid_: gate (alone), post-step

**Land**:
Putting a task's commits on the run branch by rebase and fast-forward, and
pushing it.
_Avoid_: merge

**Finding**:
A fact the import found about past work that did not finish as intended,
such as a worktree that never landed.

## Liveness

**Checkin**:
The supervisor's heartbeat: proof the worker's process is alive, not that
it makes progress.
_Avoid_: heartbeat (in user-facing text), ping

**Progress**:
Evidence that the agent is advancing its task: its own ledger call, its
transcript growing, herdr's state changing, or its working timer running.

**Proxy checkin**:
Progress the watchdog records for an agent that made no ledger call but is
visibly working.

**Watchdog**:
The 1 horch process per run that checks every working worker on a fixed
interval.

**Reap**:
Stopping a dead worker by its recorded PIDs and pane, and requeueing its
task for a new attempt.
_Avoid_: kill, clean up
