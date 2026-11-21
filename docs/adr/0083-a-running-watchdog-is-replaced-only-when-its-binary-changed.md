# A running watchdog is replaced only when its binary changed

Each task ledger has 1 watchdog, and horch commands do not start a second
one or replace it needlessly. The watchdog's lock records the identity of
the horch binary that runs it. `LockHolder::binary_replaced` in
`taskledger::watchdog` treats the watchdog that holds a run's lock as
replaceable only when the file it started from changed since (a
self-update rename or a rebuild replaced it), or when it is older than the
binary record (#216). Only then does it move onto the new file. A holder
that runs another binary at another path, unchanged, is left alone: 2
horch binaries on 1 ledger do not replace each other's watchdog on every
command.

## Consequences

- `watchdog::holder` judges the lock's age against `EXE_RECORD_GRACE_S`
  (10 s) by the wall clock, not the ledger clock: `HORCH_NOW` moves ledger
  stamps but not a lock.
- A watchdog that was just started and has not taken its lock yet counts
  as running. `watchdog::record_start` records on the run the watchdog
  process a starter just started (its PID and start time), with no
  checkin, before it takes its lock. Until it takes the lock and checks
  in, the next command sees it alive in `needs_restart` and does not start
  a second one. On a loaded machine a second one could take over when the
  first was killed, and the restart that followed never came.
