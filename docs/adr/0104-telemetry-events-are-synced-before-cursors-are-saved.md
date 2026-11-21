# Telemetry events are synced before cursors are saved

The event store in `crates/horch-core/src/telemetry/store.rs` is
`events-YYYY-MM-DD.jsonl`, append-only (section 8.2). Its crash safety comes
from order, not transactions: events are appended and synced first, and
only then is the cursor file `cursors.json` replaced. A crash between the 2
re-reads some input on restart, and the dedupe index drops every event
whose key `(agent, session_id, event_id)` is already stored.

## Consequences

- One collector tick in `crates/horch-core/src/telemetry/collect.rs`
  (section 8.3, snapshot in section 9) runs in this order: read every
  ledger under the state root, keep the records updated inside the
  retention window, poll each record's reader, append the events, sync,
  then save the cursors, fold limit signals into the quota readings and
  probe when due, and write `snapshot.json` (temp file + rename).
- A ledger that does not parse keeps its last good copy, because a write
  may be in flight.
- Events are stored per batch of `BATCH` (10,000) events, so a first read of
  a large history holds only 1 batch in memory.
- `Collector::store_batch` appends and syncs the events, then saves the
  cursors when `save` is `Some(sync)`. A batch inside a read leaves its
  cursors unsynced (`store::save_cursors`). The tick's last save syncs.
- `store::save_cursors` with `sync: false` leaves the new file in the page
  cache. That is safe after the events are synced: a power loss can only
  keep the old cursors, or a torn file that loads as none, and both re-read
  input the store already holds, whose repeats it drops. It returns the
  bytes written.
