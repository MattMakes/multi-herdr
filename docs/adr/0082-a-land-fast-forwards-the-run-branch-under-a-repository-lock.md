# A task-ledger land fast-forwards the run branch under a land lock

The land step (`taskledger::finish::land`) rebases a task branch on the run
branch, runs the check, and fast-forwards the run branch, under 1 land lock
per repository. `land::rebase_and_check` runs the first rebase and check
outside the lock, and goes on under it only when the run branch did not move
meanwhile (#160). When it moved, the next try takes the lock first, so a land
runs the check at most twice. `land::land` keeps the lock guard that
`land_stages` returns until the land is pushed and recorded, so no other land
of the repository comes between the fast-forward and the push.

## Consequences

- A land lock is stale only after its holder's check timeout (capped at
  `MAX_CHECK_TIMEOUT`) plus `LOCK_STALE_MARGIN` (10 min), the time for the
  rebase, the fast-forward and the push around the check (`lock_stale_age`,
  D7). A check can run under the lock, so a shorter age breaks the lock of a
  live holder.
- When the check runs under the lock, `rebase_and_check` refreshes the lock
  first. Its age starts again, so a waiter does not take it for stale.
- `land::require_clean` judges committed work only: a step judges commits,
  never a half-saved tree. In the shared checkout only tracked files count.
  Its untracked files are the operator's or other workers', not the task's.
- `land::rebase` rebases on the target, and a conflict aborts the rebase.
  When the target is an ancestor of the branch head already, nothing is
  rebased: a rebase would flatten the merge commits of the branch, and a
  fast-forward lands them as they are.
- A land that stops after its rebase puts the branch back at its head before
  the land (`land::undo_rebase`, L2). The tree that the steps before land
  judged stays, so the next finish starts no new review round. A task that is
  no longer this worker's keeps its branch as it is: another worker can use
  the worktree now.
- The check runs in its own process group, so a terminal's signal does not
  reach it. `land::signals` makes SIGINT, SIGTERM and SIGHUP to horch kill
  the check's group too. Without this the check ran on with no deadline.
  `CheckGroup` also kills the group on drop, so no check outlives its finish.
- `finish::create_worktree` names a task branch `<run-branch>-T<seq>-<slug>`,
  beside the run branch, not under it: git cannot hold both `fleet/x` and
  `fleet/x/T1-y`.
