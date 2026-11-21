# The task ledger path is independent of the state root

`crates/horch-core/src/runtime/paths.rs` keeps the task ledger path
independent of the state root on purpose. The state root is writable from
a Codex pane (#181). Only `HORCH_LEDGER_DB` moves the ledger.

## Consequences

- A command whose state root is not the default one but that has no
  `HORCH_LEDGER_DB` gets a warning from `ledger_isolation_warning` (#199):
  the state moved, but the task ledger did not.
- The ledger records the machine's name beside every PID, because a PID
  names a process only on its own host. `host_name` in
  `crates/horch-core/src/runtime/machine.rs` takes `$HORCH_HOSTNAME`
  (tests), else the OS host name, else `COMPUTERNAME` or `HOSTNAME`, else
  `localhost`.
