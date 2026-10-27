# 03: Queue tasks and claim them by persona

**What to build:** The orchestrator queues work and workers take it.
`horch ledger add` queues 1 task from a ticket file with its primary and
fallback persona, `--parallel` or `--exclusive`, `--blocked-by`, `--own`
and `--no-touch` files, the land check and the phase. `horch ledger claim`,
run by a worker, takes the next claimable task whose `claimable_by` equals
the worker's persona, or records `no_task` and prints the 1 line the worker
must send to the orchestrator, with its supervisor PID.

`horch ledger plan-workers` prints, per persona, how many workers to spawn
now. `horch ledger fallback <task> "<reason>"` lets the fallback persona
claim. `horch ledger cancel <task> "<reason>"` drops a task.
`horch ledger status` lists the run's tasks and exits 0 only when every
task is `done` or `cancelled` and every worktree is `cleaned` or
`unlanded` (non-zero otherwise).

A run row exists before tasks can be added: `horch fleet` creates it with
the spec's default thresholds. Until ticket 04 lands, `claim` accepts the
worker's identity from `HORCH_WORKER_ID` and `HORCH_TEAMMATE` and creates
the `workers` row if it is missing; ticket 04 removes that fallback.

This ticket also owns the 1 helper that wakes the orchestrator: it sends
1 line through the existing channel relay and durable mail, as
`horch tell orchestrator` does. Tickets 07, 09 and 10 call it; none of
them writes its own.

**Blocked by:** 02 (The task ledger store)

**Status:** ready-for-agent

**Persona:** primary `backend-developer`, fallback `opus`

**Parallel:** yes

**Spec:** `docs/specs/task-ledger.md` sections 4.2 to 4.5, 5.1, 5.2, 5.6, 6

- [ ] `add` refuses a dependency cycle and a `--blocked-by` that names an
      unknown task.
- [ ] The claimable rule is exactly the one in section 4.3: queued, every
      blocker `done`, the exclusive rule, and no `own` file held by a
      running task. Each clause has its own test.
- [ ] Only `claimable_by` may claim: a fallback-persona worker gets
      `no_task` until `ledger fallback` runs, then gets the task.
- [ ] A race test: 8 threads claim against 3 claimable tasks; exactly 3
      claims succeed, each task has 1 `working` worker, and 5 get `no_task`.
- [ ] `claim` prints the `task_file`, the `own` and `do_not_touch` files,
      and the exact `horch tell orchestrator "[<role>] NOTE: No task for
      <persona>. I exit. PID <pid>."` line on `no_task`.
- [ ] `plan-workers` subtracts live workers of a persona in `spawning`,
      `starting` or `claiming`.
- [ ] Every state change writes an `events` row.
- [ ] `--json` on `status`, `show`, `plan-workers` and `claim`.
- [ ] The wake helper has a test against the fake channel: 1 call gives 1
      delivered line and 1 mail entry.
