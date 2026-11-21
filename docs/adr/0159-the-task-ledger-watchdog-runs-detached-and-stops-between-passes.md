# The task-ledger watchdog runs detached and stops between passes

The watchdog `horch ledger watch --run <id>`
(`crates/horch/src/cmd/taskledger_watch.rs`) is started by
`taskledger_watch::start_detached` in its own session, so closing the herdr
pane that started it does not end it. Its output goes to
`<ledger dir>/watchdog-<run>.log`.

## Consequences

- SIGTERM sets an atomic flag, and `exit_between_passes` ends the loop
  only between passes, never inside one. A pass can be in a reap, which
  requeues the task before it stops the agent (#216 restart, review of
  T-3). So a SIGTERM during a reap ends the watchdog only after the reap.
  The agent that ignores TERM still gets KILL after the stop grace, and
  its pane is closed. A watchdog that died at once would leave both alive,
  with the task already requeued.
