# The telemetry collector reads only inputs whose stamp changed

A collector tick in `crates/horch-core/src/telemetry/collect.rs` must stay
cheap (NFR-02). Parsing every ledger took most of a tick, and so did opening
every transcript of every record. So the collector keeps a change stamp per
ledger, per input and per directory, and reads only what changed since the
last tick. `Collector::refresh_ledgers` reads the ledgers whose length,
inode or modification time changed. `Collector::poll` polls the inputs of a
record only when they changed since its last poll, reading at most `limit`
observations, and returns `None` when nothing changed.

## Consequences

- The inode is part of a ledger's stamp. It catches a rewrite by rename to
  the same length within 1 modification-time tick. The W16 audit found the
  cache keyed a ledger by length and modification time only, and such a
  rewrite was missed.
- A ledger that does not parse keeps its last good copy and is read again
  next tick. A deleted ledger is dropped.
- `Collector::poll` takes the stamps before the poll, so a write during it
  shows next tick. The stamps are kept only when the poll read to the end
  (`Polled::more` is false). Otherwise new data would be marked as seen.
- `Listing::walk` stamps each directory before reading it, so a file added
  during the walk shows next tick.
- OpenCode keeps its sessions in SQLite, which writes to the database or to
  its write-ahead log, so the collector stamps both.
- `collect::wal_stamp` stamps an empty `-wal` as no log. The first
  `sqlite3 -readonly` read of a WAL database creates an empty `-wal` on
  Linux (sqlite 3.40), so the stamp a tick took before that read differed
  from the next tick's although nothing was written, and a quiet tick saved
  the cursors again. A write fills the log. A checkpoint that empties it
  writes the database.
- `poll_lines_until` keeps its line buffer in a `thread_local` from call to
  call (NFR-02 peak RSS). A transcript line can be megabytes long. A new
  buffer per call grew by doubling each time, and macOS kept every freed
  block of more than 1 MB resident in its cache.
- `Store::open` sizes the event table and the key index once, from the
  stored line count plus 1/8 headroom for the events of the next hours
  (NFR-02 peak RSS). Grown by doubling, they left each old block in the
  allocator's cache of freed large blocks, still resident.
