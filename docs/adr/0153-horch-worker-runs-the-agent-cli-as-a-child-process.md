# horch worker runs the agent CLI as a child process

`horch spawn` writes a brief (the resolved teammate, model, task, session
and record ids) into the workspace mailbox and runs `horch worker` in the
fresh pane (`crates/horch/src/cmd/worker.rs`).
`execution::lifecycle::run_worker` registers the role, renders that
teammate's briefing, launches the agent CLI and records how it ended. The
agent runs as a child and its exit status is propagated. The old bash
worker `exec`ed the agent, and Windows has no `exec`.

## Consequences

- The pane line itself starts with `exec`, so a worker that fails at
  startup closes its pane and takes its stderr with it. The worker writes
  its startup error to a file keyed by record (or by pane when it could not
  read its brief), and `spawn::wait_for_start` polls that file alongside
  the registration.
