# horch done records the work before it closes its own pane

`horch done` (`done` in `crates/horch-core/src/execution/lifecycle.rs`) is
the worker-facing self-shutdown. It runs only when the work is truly
complete, not while the worker waits on a question. Its order is fixed:
queue the report to the orchestrator (`DoneSteps::queue_report`), mark the
claims unlanded, mark the record done (`DoneSteps::mark_done`), run
`DoneSteps::after_mark_done`, push the report (`DoneSteps::push_report`,
best effort, within the push budget), record a session id still
undiscovered (`DoneSteps::record_session`), unregister the mailbox entry,
settle the grid (`DoneSteps::settle`), write the fleet run row
(`DoneSteps::record_run`) and close the pane. Every step happens before the
pane close, because closing the pane kills the `done` process tree itself.
The underlying agent session stays on disk and resumable.

## Consequences

- The report is made durable first, or `done` fails as if nothing changed.
  A failed report leaves the record not done, the claims open, the role
  registered and the pane open. The error tells the worker to retry. A
  worker whose report was lost must not leave.
- A push that can take long belongs in `DoneSteps::push_report`, never in
  `queue_report`. The push runs after the record is done, so a push killed
  by the agent's command timeout (#67) still leaves the work done. The
  default `push_report` has nothing to push.
- `DoneSteps::after_mark_done` runs after the record is done and before the
  report push. `horch ledger done` pushes its wake lines there, inside the
  same push budget, so a kill during them also leaves the work done. The
  wakes run at most once. The default does nothing.
- In `report_and_record` the claims are marked unlanded only after the
  report is queued. A claim marked unlanded is no longer open, and a retry
  of a failed report must still find it open to report the same line.
- `queue_done_report` adds the report failure as an `anyhow` context, not
  as a new error, because the CLI's sandbox hint looks for the underlying
  herdr failure in the error chain.
- A retry after a `done` that recorded its summary and then could not close
  the pane only closes the pane (`DoneSteps::recorded`).
- Once the record holds the summary, the dataset coordinator closes the
  pane of a candidate whose record is done, so it can close this pane first,
  at any step of `close`. A pane that is gone counts as closed: the steps
  that need only the workspace still run, nothing fails and `done`
  succeeds. When herdr itself does not answer, the pane may still be open:
  the same steps run, and `done` fails at the end so the caller knows.
- The fleet run row is written after the final session discovery, so a
  Codex run row finds its transcript.
- A departing worker leaves a hole in the grid, and herdr hands its width to
  whichever neighbour happens to be its split sibling. That lumps the grid
  rather than spreading it. `settle` hands the tidy to a detached child
  process, because by the time the hole exists the pane close has killed
  `done`'s own process tree. The child pulls the last worker into the free
  slot, and an overflow tab that lost its last worker closes itself.
