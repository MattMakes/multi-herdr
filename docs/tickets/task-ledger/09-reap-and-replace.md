# 09: Reap a dead worker and hand its task to a new one

**What to build:** `horch ledger reap <worker>` ends a dead worker and
requeues its task so a new worker of the same persona continues it.

1. It runs the stop of ticket 05 on the worker.
2. The worker row becomes `incomplete` with `end_reason` and `ended_at`.
3. The task goes back to `queued` with `current_worker_id` cleared and its
   priority lowered by 1000, so it is claimed first.
4. Its worktree stays `open` with its branch and uncommitted files.
5. It writes `reaped` and `requeued` events and wakes the orchestrator with
   1 line naming the persona to spawn.

`horch spawn` takes `--replaces <worker>` and sets `previous_worker_id`.
The new worker's `claim` takes that task, moves the worktree to itself, and
prints every previous attempt's `summary`, `transcript_path` (a path only)
and worktree, with this instruction: use what you can of the previous
work, and read the previous transcript in chunks to learn what went wrong
before you continue.

**Blocked by:** 03 (Queue tasks and claim them), 05 (Stop 1 worker by its recorded PIDs), 06 (Land through the ledger)

**Status:** ready-for-agent

**Persona:** primary `opus`, fallback `backend-developer`

**Parallel:** yes

**Spec:** `docs/specs/task-ledger.md` section 5.4

- [ ] After a reap the dead worker's row is never reused; the replacement
      has its own worker id, PIDs and session.
- [ ] The replacement's claim prints the transcript paths of every earlier
      attempt, oldest first.
- [ ] The worktree row's `worker_id` moves to the replacement and its
      uncommitted file survives.
- [ ] Reaping a worker that is already `done` refuses and changes nothing.
- [ ] `tasks.attempts` counts every attempt.
