# 13: End-to-end run on the fake herdr

**What to build:** 1 e2e scenario that proves the whole flow on the
existing fake herdr and fake agents:

1. The orchestrator adds 3 tasks: A and B parallel, C blocked by A.
2. `plan-workers` says 2; 2 workers spawn and claim A and B.
3. The worker on B stops making progress (its fake agent hangs with a
   frozen timer and no transcript writes).
4. The worker on A lands its worktree; the folder is gone; the task is
   done; the orchestrator is woken for C; a third worker claims C.
5. After 3 checks on an injected clock, the watchdog reaps B's worker by
   its PIDs; the other workers keep running; a replacement claims B and
   receives the old transcript path.
6. A spare worker finds no task, sends the no-task line with its PID and
   exits.
7. All tasks end done, every worktree is `cleaned`, and `ledger status`
   exits 0. `stops` holds only rows for exact PIDs and panes.

**Blocked by:** 12 (Switch the briefings)

**Status:** ready-for-agent

**Persona:** primary `qa-engineer`, fallback `opus`

**Parallel:** no (runs alone: it exercises every area at once)

**Spec:** `docs/specs/task-ledger.md` all sections

- [ ] The scenario passes 10 times in a row.
- [ ] No real model call and no `ANTHROPIC_API_KEY` anywhere in it.
- [ ] It runs inside the existing e2e harness, which reaps every process it
      started.
