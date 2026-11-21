# The telemetry collector reads a cold corpus in batches

The telemetry collector reads new events in batches and holds at most 1
batch in memory (NFR-02, L7). Until W18 a cold tick held every new event of
the corpus: 1.5 million events and 3.4 GB peak RSS on the 1 GB corpus. Each
batch is appended before its cursor is saved. A crash after a batch's
append and before its cursor save resumes with no event lost and none
stored twice. `crates/horch-core/tests/nfr.rs` checks the bound.

## Consequences

- The peak RSS check reads `ru_maxrss` from `getrusage`, which reports
  bytes on macOS and KiB on Linux. The conversion divides by a per-OS
  factor.
- The ignored `project_rollups_timing` benchmark in
  `tests/telemetry_collect.rs` measures the screen's project filter on real
  data: `Collector::project_rollups` for the project with the most events,
  against the old path (every event file read, then `store::rollup`). It
  does 3 runs of each and prints the median. It reads the directory in
  `HORCH_PERF_STATE`, which is a COPY of a state directory, never the live
  one: `HORCH_PERF_STATE=<copy> cargo test --release -p horch-core --test
  telemetry_collect -- --ignored --nocapture project_rollups_timing`.
