# A worker that fails or exits early always leaves word

`run_worker` in `crates/horch-core/src/execution/lifecycle.rs` loads the
brief, builds the context, registers the mailbox, sets `Running`, launches
the agent through the harness flow, waits and records its exit. It returns
the agent's exit code (1 when a signal ended it), which `horch worker`
exits with. The order is Spec A §8 (architecture design §4.5). The worker
never calls `done`: the agent runs `horch done`, which closes this pane.
Every failure on this path reaches the spawner or the orchestrator, because
otherwise nothing would: the pane closes with the worker and the
orchestrator waits.

## Consequences

- A ledger write that fails is logged and the worker goes on: the agent
  must start even when its bookkeeping cannot be written. That includes
  `set_running`: the agent still starts, and its exit ends the record.
- A failure after the brief loads and before the agent launches
  (`enter_context`, `register`) ends the record at once as
  `Failed(AgentExited{None})`, the same state a launch failure gets, then
  returns the original error. The record's history first gets the error
  text as event `EVENT_STARTUP_FAILED` (`"startup-failed"`), which
  `horch sessions` shows. Without it the record stays `Starting` with a dead
  worker until a deadline. The recording is best effort and its own error is
  logged. Then the worker closes its own pane, as `done` does. A close that
  fails is logged and the startup error still returns.
- A failure of `load_brief` records nothing in the ledger, because the
  record is unknown.
- Every startup failure also leaves its error text for `horch spawn`
  (`WorkerSteps::leave_startup_error`) before the pane closes. A pane line
  starts with `exec`, so the pane, with its stderr, closes when the worker
  ends, also after a `load_brief` failure, and the spawner's error is the
  only one the operator sees. `fail_startup` prints the error and leaves it
  for the spawner first, because the close ends this process.
- The startup error file (#192) lives under the data root, which the pane
  command sets (`HORCH_DATA_DIR`), so worker and spawner agree on it before
  the brief is read (`startup_error_path`). The data root is shared by every
  workspace of the user, and roles repeat across workspaces, so the file is
  named by `StartupKey` (the record id, or the public pane id when no brief
  loaded), not by role. `startup_error_path` returns `None` when the key
  could name a path.
- `take_startup_error` reads and deletes the file. A file written before the
  spawn time belongs to an earlier worker: it is deleted and not returned.
  The spawn time is compared at whole seconds, the coarsest mtime a file
  system keeps, so a file written in the same second counts even when its
  mtime reads earlier.
- An agent that exits before `horch done` has reported nothing, and the
  orchestrator would wait for it forever. `PaneWorker::agent_exited` gives
  the record an `EVENT_EXITED` (`"exited"`) entry, and the worker process,
  still alive in the pane, tells the orchestrator. This holds only after an
  agent ran: a startup failure is `horch spawn`'s to report. A failed tell
  is printed.
