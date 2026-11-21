# Long-lived processes record the binary they run

A detached horch process (the telemetry collector, a task-ledger watchdog)
keeps running the binary it started with, with old prices and old readers.
After `horch install` replaces that file, the process runs old code
(#216). Each such process records its `ExeIdentity` (resolved path, size,
`mtime_ns`) when it starts, and its starter compares that with the binary
that should run now (`crates/horch-core/src/exeid.rs`).

## Consequences

- `horch install` restarts each live collector or watchdog that is stale
  on the binary just installed. A failure only warns and names
  `horch telemetry ensure`.
- `main`'s `Command::watched` excludes `install` (and `worker`) from the
  post-command `taskledger_watch::ensure`. After `install` restarted the
  watchdog on the new binary, `ensure` would move it back onto the old
  binary that ran `install` (#216).
