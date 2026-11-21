# A cancel or stop settles the ledger before any process is signalled

`tasks::cancel` is the 1 cancel path (#136). In the caller's transaction the
task lets go of its worker, the worker row ends `stopped`, and an open
worktree becomes `abandoned`. Its branch and folder stay, so a requeue can
reopen it. The worker's process is not signalled there: after the commit,
the caller stops it with `stop::stop_cancelled` (TRF-36). This builds on the
stop rule of ADR 0002.

## Consequences

- `stop::stop_worker` stops the agent, then its supervisor, each by the PID
  and start time in its row, then the pane by its id. Before any signal it
  sets the row to `opts.final_status` (`stopped` or `incomplete`) and puts a
  task the worker held back in the queue. The supervisor then sees a worker
  that is no longer working and records only the exit code instead of
  reporting a crash.
- `finish::mark_done` never turns a task cancelled meanwhile into done
  (#136): its `UPDATE` is guarded by the status. A parked (unlanded) task
  lets go of its worker and is not marked finished.
- `land_stages` checks that the task is still this worker's (`still_mine`)
  under the land lock, just before the fast-forward. A cancel that arrived
  during the rebase, the check or the lock wait lands nothing.
