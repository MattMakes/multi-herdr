# horch reads the time from 1 pinnable clock

Every read of the current time in horch goes through `clock::now` in
`crates/horch-core/src/clock.rs`. A test can pin the time with
`HORCH_NOW=<RFC 3339>` and get the same answer from the ledger, the
telemetry collector, the quota states and the spawn gate. Calling
`Utc::now` directly breaks that. A pinned clock is announced once on
stderr, so it cannot go unnoticed in a real run.

## Consequences

- `clock.rs` does not read `HORCH_NOW` itself. The binary's bootstrap
  reads it into `RuntimeContext::settings` and passes it to
  `clock::install` once.
- `clock::stamp` writes the ledger's timestamp format, RFC 3339 UTC with
  whole seconds and `Z`: `2026-09-28T17:40:02Z`. Lexicographic order then
  equals chronological order, and ledger queries and comparisons on these
  strings rely on that.
- Heartbeats are the exception. `heartbeat::beat` stamps the heartbeat
  with the real wall clock (`Utc::now`), never a pinned `HORCH_NOW`,
  because the reader compares it with real file modification times.
- The liveness checks in `compaction::jobfile` also use `Utc::now`:
  heartbeats and spawn times are real wall-clock times, and comparing them
  with a pinned test clock would misjudge liveness.
