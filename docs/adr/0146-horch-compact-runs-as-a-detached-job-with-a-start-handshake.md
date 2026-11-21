# horch compact runs as a detached job with a start handshake

`horch compact` (`crates/horch/src/cmd/compact.rs`) starts a detached child
that does the compaction, and waits only for a start handshake: the child
must write `job.json` at step `wait-idle` within `START_TIMEOUT` (design
§6.8a). `START_TIMEOUT` is 30 s, not the design's 1.5 s or the later 5 s.
The child boots a process, opens the ledger and runs the checks, and that
grows with host load. Measured on an 18-CPU host: 0.05 to 0.4 s with 16
parallel runs, 1.4 to 2.5 s with 250 busy loops added. A fleet gate run
slowed other tests 15 times. The wait ends as soon as the job file appears,
and a child that dies is seen at once (`try_wait`), so only a live child
that never writes the file waits the full time.

## Consequences

- In `compact::exited` the job file is read again after the child's exit
  is seen. `poll` read `job.json` before it saw the exit, and the child may
  have written it in between. Skipping the re-read would report a
  successful start as failed. After the re-read, the exit code says why a
  child that never started exited.
- `compact::handoff_file` resolves a record's handoff file against the
  record's workdir (`record_workdir`), never against this process's cwd,
  because `horch compact` runs from another directory than the worker
  (review finding 14).
- A Codex pane with `permission_mode: plan` runs read-only and cannot write
  its handoff file (X6). Compaction switches it to write with `horch mode`
  before it sends the compaction request.
