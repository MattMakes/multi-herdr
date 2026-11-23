# One collector runs behind a directory lock

`crates/horch-core/src/telemetry/lock.rs` holds the collector singleton
(section 12.2, SPC-01). `telemetry/collector.lock/` is created with
`create_dir`, which is atomic, the same idiom as `SessionLedger::lock`. The
holder writes `collector.json` next to it. A lock whose pid is dead is
broken, and the breaker says so. The lock is also removed on every normal
exit. Liveness is "the pid exists and has the recorded start time"
(`crate::procid::alive`), so a reused pid does not keep a stale lock alive.

## Consequences

- A `collector.json` without a start time (written before it existed), or a
  platform without start times (Windows), falls back to "the pid exists".
- A lock directory without `collector.json` is live while it is young
  (under 5 s old): a holder sits between `create_dir` and writing its info
  for a moment. Only an old, empty lock is stale.
- A recorded pid that names no single process is a dead holder. Cast to a
  `pid_t` it can turn into -1, and `kill(-1)` signals every process.
- The holder records its binary (`crate::exeid::ExeIdentity`: path, size
  and modification time). A collector keeps running the binary it started
  with, so after `just install` replaces that file it runs old code: old
  prices, old readers. `runs_other_binary` finds it, and `stop` ends it, by
  pid and start time only, so `horch telemetry ensure` and `horch install`
  can start a current one.
- `crates/horch-core/src/telemetry/start.rs` holds the start handshake
  (section 12.1, SPC-03). `horch telemetry ensure` does not wait for the
  collector it starts: under load the new process can take longer than any
  fixed wait to take the lock. It records the start first
  (`start::record`), so a later `ensure` can tell a slow start from a
  failed one: it writes `telemetry/start.json` (a `Request`) and returns.
  `telemetry::start` runs `exe telemetry` in the telemetry workspace,
  never focused. An old collector workspace with 1 pane is reused. The
  process it runs gets the request id in `START_ENV`
  (`HORCH_TELEMETRY_START`) and writes `telemetry/starting.json` (a
  `Marker` with its pid) as its first step. The process that then takes
  the lock closes every other old collector workspace
  (`close_after_claim`) and removes both files. A process that finds the
  lock held by another collector ends.
- A later call reads the 2 files while no collector holds the lock
  (`pending`): a marker whose pid is dead is a failed start, and so is a
  request with no marker after `UNSEEN_AFTER`. Either is reported, and
  `ensure` starts the collector again. A wrong guess costs nothing: the
  lock lets only 1 collector run.
- `UNSEEN_AFTER` is 300 s. It decides only when to retry, never whether
  `ensure` succeeds.
- `start::unseen_too_long` ages the request by the file's modification
  time, not by `HORCH_NOW`: a pinned clock must not hold a start forever.
- Nothing in the handshake holds telemetry. The collector reads ledgers and
  transcripts from where they are, so a late start loses no event
  (section 14).
