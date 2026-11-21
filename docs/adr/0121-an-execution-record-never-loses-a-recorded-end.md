# An execution record never loses a recorded end

The writers of `ExecutionStore` (`crates/horch-core/src/execution/store.rs`)
keep any end another writer already recorded, and a record is ended only on
evidence that its work is gone. The worker, the spawner and the coordinator
write the same session record concurrently, so every state change only
moves a record that no later writer has passed. A record that has ended is
never moved back to live or given a different end. A writer that takes a
record back, or ends it on a guess, loses a finished result or kills live
work.

## Consequences

- The worker runs as soon as `pane run` answers, before `mark_starting`
  writes, and the spawner records the pane after that. On a loaded machine
  the worker can already have written `Running`, `Done` or `Failed`.
  `mark_starting` therefore moves only a record no worker has touched yet
  (`Planned`, or `LaunchFailed` by a spawn's recovery) to `Starting`, and
  never takes a record back from those ends. The pane is recorded either
  way. Before D18 a `Done` became `Starting` again, and the dataset
  coordinator recorded the finished candidate as `pane_vanished`.
- The other writers are guarded the same way (D18). `mark_running` (the
  worker) keeps an end the coordinator wrote while the worker started: the
  coordinator can time out or cancel a candidate while its worker is still
  starting, and the closed pane then kills the worker. `LaunchFailed` from
  a spawn's recovery still becomes `Running`, because the worker does run.
- `end_live` (a timeout, cancel or launch failure) never overwrites a
  record that already ended, because the worker can run `horch done`
  between a caller's read and this write. It returns the newest matching
  record's state after the write, which the caller records.
- `mark_pane_vanished` ends the still-working rows of a record as
  `PaneVanished` only after re-checking under the ledger lock, because the
  worker may run `horch done` at that moment.
- A `Planned` record with no pane is never live. Once `ABANDONED_AFTER`
  (5 minutes) has passed, the next spawn closes it as `LaunchFailed` and
  frees its role. A younger one is left alone: its spawner may still be on
  its way to a pane.
- `end_if_pane_closed` ends a `working` record only when herdr answers and
  the pane is absent (G5). `horch spawn --resume` of a `working` record
  whose pane is gone ends the record first, so it resumes. An open pane, or
  a herdr that does not answer, leaves the record `working`, and the resume
  refuses. An unreachable herdr never ends live work.
- `plan::check_resumable` refuses a record whose legacy status is still
  `working`. A `Planned` record is not live, but its spawner may still be on
  its way to a pane. Since `end_if_pane_closed` has already ended a record
  whose pane is gone, a remaining working record has an open pane, a pane
  in an unknown state, or no pane named.
- In `execution::lifecycle::done`, a failed report to the orchestrator
  stops the shutdown (W2). The ledger is not marked done, the role stays
  registered, the pane stays open, and the error tells the worker what to
  do next. The worker's result is not lost.
- A killed pane can leave the ledger's `.json.lock` dir behind. The ledger
  breaks a stale lock after about 15 s of spinning rather than hang
  forever.
