# `horch tile` parks every worker in a scratch tab, then places it

`crates/horch-core/src/workspace/tile.rs` places every pane of a fleet
workspace into 1 deterministic 2-row grid, and
`crates/horch-core/src/workspace/arrange.rs` runs the plan against herdr.
The orchestrator is the leftmost full-height pane of tab 1 and never moves.
Tab 1 holds 4 workers in 2 columns beside it. Each overflow tab holds 6
workers in 3 columns and has no orchestrator. Slots fill column 1 top,
column 1 bottom, column 2 top, column 2 bottom, and so on. That order is
`Slot`'s ordering, so placement, reuse and reporting read the same list.
herdr refuses to move a pane into the tab it already occupies
(`changed: false, reason: "same_tab"`, `socket-api.md:231`), so `tile::plan`
has 2 phases: park every worker in a scratch tab, then place them all from
there. The plan needs no same-tab case and does not care how scrambled the
tab was.

## Consequences

- Parking uses the same 2-row grid with equal columns, not a chain of right
  splits. herdr enforces no minimum pane size: 9 chained right splits
  measured 46, 23, 11, 5, 2, 1, 0, 0, 0 cells wide without 1 error. A chain
  would hand a live agent a zero-column terminal on the way past.
- A right split subdivides only its own pane's rect. Splitting column 1's
  half-height top pane to the right opens the new column in the top row only,
  and splitting that new pane DOWN gives 2 quarter-height panes, a shape that
  cannot be recovered. So `tile::grid_op` opens column `c >= 2` twice, against
  column `c-1`'s top pane and then its bottom pane, the order
  `crates/horch/src/cmd/recipes.rs` has always used by hand.
- `tile::SCRATCH_LABEL` (`horch-tile-scratch`) labels the parking tab. It
  exists for at most 1 run. A run that dies leaves it, and the next run
  gathers its panes like any others and places them.
- `arrange::apply` aborts the run when herdr declines a move. Every later op
  targets a pane the earlier ones were to place, so continuing would build an
  unplanned shape. Aborting is safe because no pane is ever closed: the
  workers sit in the scratch tab and the next run finishes the job.
- `tile::Fleet::newcomer` is placed last however its rect sorts. `horch spawn`
  splits the orchestrator's pane by default, so a new worker lands
  full-height right of the orchestrator with the smallest `x` of any worker.
  Sorted by geometry it would take column 1 top and rotate every other worker
  1 slot on every spawn.
- `tile::order_workers` keeps the current reading order, so a re-run is
  stable and neighbours stay neighbours. Column `x` edges are clustered within
  `TOLERANCE` first, so a divider that drifted 1 cell cannot swap a column's
  2 panes.
- `tile::tab_is_canonical` compares structure only and ignores column widths.
  Uneven columns are the balance step's job, and a tab that needs only a
  resize must not be torn down and rebuilt.
- `arrange::TileLock` is a workspace-wide lock, so 2 tilers never interleave
  moves. Tiling moves panes out of their tabs and back, and a second tiler
  reading half way through would see a shape neither planned. A tiler waits
  for the lock instead of skipping: a spawn that skipped tiling would leave
  its new worker wherever the split put it. `horch balance` needs no lock
  because its target is absolute.
- `arrange::find_orchestrator` takes the orchestrator from the mailbox
  registry, else the leftmost full-height pane of the first tab (the guess
  `horch layout` has always made), and otherwise refuses. Tiling a workspace
  whose orchestrator cannot be identified would file it as a worker and move
  it.
- Only tab 1 holds the orchestrator. Overflow tabs are analysed with
  `layout::Orchestrator::Absent`, and `tab_is_canonical` gets `None` there.
  The positional guess would take a lone full-height worker column for the
  orchestrator and silently drop it from the grid.
- `arrange::report` prints the whole workspace's grid, 1 block per tab in
  tab-strip order, and seeks the orchestrator only on the tab that holds it,
  for the same reason.
