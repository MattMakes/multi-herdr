# Report-access panes are served outside the sandbox as the brief

A `horch_access: report` pane gets a report socket (`messaging::report`)
for the life of its agent. `start_report_server` in
`crates/horch-core/src/execution/lifecycle.rs` serves it from the worker
process, which runs outside the agent's sandbox. Each request runs as
`horch` with the brief's identity in the environment, never the pane's, so
a sandboxed agent cannot act as another role.

## Consequences

- The guard `start_report_server` returns is bound to a named variable
  (`_report`) and held until the agent exits. Dropping it, for example by
  binding it to `_`, removes the socket at once.
