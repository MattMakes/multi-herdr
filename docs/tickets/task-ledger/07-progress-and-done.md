# 07: Worker progress and done through the ledger

**What to build:** A worker reports through the ledger, and the
orchestrator is woken when that changes what it should do.

- `horch ledger progress "<text>"` and `horch ledger note "<text>"` write an
  event and move `last_progress_at` (source `agent`), and reset
  `stall_rounds`.
- `horch ledger done "<summary>" [--unlanded "<reason>"]` refuses while the
  task has a worktree that is not `cleaned`, unless `--unlanded`. It writes
  the summaries, `done` on the task and the worker, and tells the
  orchestrator `[<role>] DONE: ...` through the existing channel and mail.
- After a task goes `done`, horch checks whether tasks became claimable. If
  so, it wakes the orchestrator with 1 line naming them and the
  `plan-workers` numbers.
- When `ledger status` turns complete for the run, horch wakes the
  orchestrator with 1 line: all tasks are done; verify and report to the
  operator.
- `horch note` and `horch done` keep working; when the worker has a
  `workers` row they also write the ledger.

**Blocked by:** 04 (Workers record their PIDs), 06 (Land through the ledger)

**Status:** ready-for-agent

**Persona:** primary `backend-developer`, fallback `opus`

**Parallel:** yes

**Spec:** `docs/specs/task-ledger.md` sections 5.3 (the agent signal), 5.6, 9

- [ ] `done` with an `open` worktree exits non-zero and says to run
      `horch ledger land` or pass `--unlanded`.
- [ ] `done --unlanded` sets the worktree `unlanded` and the orchestrator
      line contains `UNLANDED:`.
- [ ] Finishing task A that blocks task B sends exactly 1 wake line naming
      B.
- [ ] Finishing the last task sends exactly 1 "all tasks are done" line.
- [ ] A `done` retried after a crash between the write and the message does
      not write a second `done` event.
