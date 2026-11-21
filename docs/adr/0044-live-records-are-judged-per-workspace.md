# Live records are judged per workspace

Several fleets can run in 1 project, so the session ledger in
`crates/horch-core/src/execution/records.rs` decides which live records
belong to whom by workspace, not by role or record kind alone.

## Consequences

- `SessionLedger::live_for_role` returns the live (starting or running)
  record of a role with the newest `updated_at`. With `Some(ws)`, only
  records whose `workspace_id` is `ws` or absent count (review finding 13):
  2 fleets of 1 project can both have a `sonnet-1`. It is the 1 selection
  rule of `SessionLedger::assign_in`, `horch compact` and the note check.
- An orchestrator never runs `horch done`. The next `horch fleet` retires
  it instead: `SessionLedger::supersede_orchestrators` marks live
  orchestrator records done, as `superseded`, unless `keep` says their
  workspace is still open, and returns how many it closed. A planned record
  never launched, so it is left alone.
