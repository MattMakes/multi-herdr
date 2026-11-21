# A worker reports its startup error through a file

A worker pane line starts with `exec`, so a real pane closes when
`horch worker` exits and takes the worker's stderr with it. A worker that
dies before it registers therefore writes its startup error to a file
under `~/.local/share/horch/startup-errors` (W13 C4). `horch spawn`, which
waits for the worker to register, reads that file, fails with its text and
deletes it.

## Consequences

- The e2e test `crates/horch-e2e/tests/startup_error.rs` runs fake-herdr in
  the `exec` scenario, so the pane really runs `horch worker`. For a
  missing brief, a herdr wrapper gives the pane another
  `HORCH_WORKSPACE_ID`, so the worker looks for its brief in a mailbox that
  does not have it. For a late worker, a wrapper holds the pane line until
  the spawn has returned.
