# The telemetry collector holds its lock for its whole life

The telemetry collector is the process that holds the collector lock
(`lock::CollectorLock`) from start to exit. In
`crates/horch/src/cmd/telemetry/screen.rs` the `Source::Collector` variant
owns the lock in a never-read `_lock` field for as long as it lives. Its
`Drop` releases the lock, so the field must stay named (not `_`) and owned
by the source.

## Consequences

- `screen::run` runs the worker and input threads, and `ui` on its own
  thread. `ui` returns when the operator quits, or on `Msg::Shutdown`,
  `Msg::WorkerGone` or `Msg::InputGone`. The worker is then stopped and
  joined, so the lock is released before the command exits and another
  process can take over as collector.
- A herdr pane does not inherit the environment of the horch process that
  opens it. `telemetry::collector_command` passes settings such as the
  state directory as explicit arguments (`--state-dir`).
- A snapshot's `generated_at` on disk is the time of its last content
  change, not of its last tick: the worker keeps the old stamp when only
  the time changed. So `view::snapshot_time` is not "now", and the live
  screen draws at `clock::now()` instead.
- `screen::TerminalGuard` restores the terminal on every exit path, a
  panic included. It first ends any open synchronized update
  (`BeginSynchronizedUpdate`). Otherwise the terminal keeps buffering and
  never shows the rest of the output.
