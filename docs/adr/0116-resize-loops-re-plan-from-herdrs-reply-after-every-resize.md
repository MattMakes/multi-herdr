# Resize loops re-plan from herdr's reply after every resize

Every loop that moves column dividers sends 1 `herdr pane resize` at a time
and re-plans against the post-resize layout herdr hands back, instead of
predicting the next position. This holds for `balance_tab` and `equalize` in
`crates/horch-core/src/workspace/arrange.rs` and for `tile::balance_op` in
`crates/horch-core/src/workspace/tile.rs`, which emits only the next single
op. Re-reading real geometry needs no prediction, so accumulated rounding
cannot walk the grid off target. `Herdr::pane_resize`
(`crates/horch-core/src/workspace/herdr.rs`) moves the divider on the
`direction` side of a pane by `amount`, and `balance::ResizeOp`
(`crates/horch-core/src/workspace/balance.rs`) is 1 such call.

## Consequences

- `amount` is a delta expressed as a fraction of the split container that
  owns the divider. That is what herdr's `--amount` means.
- A row is a right-leaning chain: divider `k` belongs to a split spanning
  `[pos[k-1], x1]`, and `--amount` is a fraction of THAT span. A divider is
  moved by asking the pane on the side it should grow into to grow.
  `tile::balance_op` reuses this model from `balance::plan`, which instead
  simulates herdr's proportional rescale to emit a whole sequence against
  predicted geometry.
- `tile::balance_op` takes its targets from the grid, not from pane edges:
  column boundaries are `x0 + round(k * w / columns)` whatever each row
  holds, so a row with a spanning bottom pane still lines up with the row
  above it. It returns `None` once every divider is within `TOLERANCE`.
- The reply carries `changed` (`model::Resize::changed`). It is false when
  herdr declines the move, which in practice means the divider is against
  the minimum pane width. It is the loop breaker for every resize sequence,
  so callers discover the minimum pane width instead of hard-coding one.
- A loop also stops when a divider is asked to move from the same place
  twice, which means the previous op did not land.
- herdr stores split ratios with 4 decimal places. `Herdr::pane_move` and
  `Herdr::pane_resize` format `--ratio` and `--amount` with `{:.4}`, so the
  request stays in step with what herdr can honour.
