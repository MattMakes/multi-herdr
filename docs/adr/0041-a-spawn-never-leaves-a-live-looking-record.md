# A spawn never leaves a live-looking record

`ExecutionService::spawn` in `crates/horch-core/src/execution/service.rs`
applies an `ExecutionPlan` in a fixed order: insert the record (`Planned`),
write the brief, split a pane, run the worker in it, mark the record
`Starting` with its pane, then tile, best effort. The record is never live
before its pane runs the worker, and a step that fails leaves no
live-looking record.

## Consequences

- A failed split records `LaunchFailed{Split}` and frees the role. A failed
  run also closes the pane.
- `HORCH_FAULT` points exercise the order: `abort-after-execution-insert`,
  `abort-after-brief` and `abort-after-pane-split` end the process there, as
  in a crash. `fail-pane-split` and `fail-pane-run` make the step fail.
- `ExecutionService::insert` allocates the role and writes the `Planned`
  record under the ledger lock, so 2 concurrent spawns cannot take 1 role.
- Once the worker runs in its pane, a failed `mark_starting` write is only
  logged and does not fail the spawn. The worker sets `Running` itself.
- `SpawnError` implements `std::error::Error` without `source`, because its
  `Display` already carries the cause and `{e:#}` would print it twice.
