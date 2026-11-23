# Snapshot costs match the store to the last bit

The collector in `crates/horch-core/src/telemetry/collect.rs` builds its
rollups so that their costs equal those of `store::rollup` to the last
bit. `RollupFold` folds the rollups of `WINDOWS` over the table's events and
`RollupFold::add`s each event in store order, as `store::rollup` does.
`build_indexed` builds `build_snapshot` from the store's `Table` the same
way: each sum adds its events in store order. Floating-point sums depend on
their order, so changing the order breaks this equality.

## Consequences

- `collect::file_hash` takes the `content_hash` of the compact snapshot
  file from its bytes, because a serde_json parse would not give the same
  floats back to the last bit. It returns `None` when the file is absent or
  is not compact JSON that starts with `schema` and `generated_at` (the
  field order of `Snapshot`).
- The project filter of the screen: the rollups of 1 project from the
  collector's table equal, per window, those from the event files. A
  collector opened on the files folds the costs it read, to the last bit.
  The collector that stored the events keeps their costs exact. serde_json
  reads some of them back 1 ulp off, so those agree to 1e-9, and the tokens
  and counts agree exactly.
