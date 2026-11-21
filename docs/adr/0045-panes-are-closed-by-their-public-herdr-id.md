# Panes are closed by their public herdr id

herdr gives a pane its internal id in `HERDR_PANE_ID`, while the spawner and
the ledgers record the public pane id. `PaneWorker` and `done` in
`crates/horch-core/src/execution/lifecycle.rs` upgrade the internal id with
`pane get` to the public id before they use it.

## Consequences

- `PaneWorker::close_pane` closes the pane by its public id, the one the
  ledgers record.
