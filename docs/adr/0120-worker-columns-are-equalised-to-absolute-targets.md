# Worker columns are equalised to absolute targets

`horch balance` (`crates/horch/src/cmd/balancecmd.rs`) makes every worker
column the same width. This is not decoration. The planning is pure and
lives in `crates/horch-core/src/workspace/balance.rs`
(`horch_core::workspace::balance`). The herdr calls live in
`horch_core::workspace::arrange`. `horch layout` relies on even
columns: `layout::analyze_with` (`crates/horch-core/src/workspace/layout.rs`)
decides whether the grid is
ragged by unioning the exact integer edges of every half-height pane and
asking whether any pane strictly contains an edge the other row produced. It
never reads the split tree. Identical column boundaries in both rows are the
precondition that analysis is built on. Fresh herdr splits meet it but give
a halving cascade: 3 columns come out 1/2, 1/4, 1/4. Once a single divider
drifts, the rows stop agreeing on integers, `analyze` invents phantom
columns and reports `ragged` for a grid whose rows both hold N panes. Acting
on that advice adds a third pane to a column and drives the grid to
`offgrid`, where the advisor has no suggestion at all. Equalising keeps both
rows on the same integers, so it repairs the advisor.

## Consequences

- A herdr row is a right-leaning binary chain, not a flat N-way container:
  divider `k` is owned by a split spanning `[pos[k-1], x1]`, and moving it
  rescales everything to its right proportionally. `balance::plan`
  reproduces both facts, so a planned op sequence matches what herdr does.
- The target is absolute: column `k` of an `n`-column row belongs at
  `x0 + round(k * w / n)`, derived only from the row's current extent and
  pane count. Concurrent passes aim at the same schedule, so a nudge from 1
  pass is never undone by the other, at worst redundant. `plan` is
  idempotent (`plan_is_idempotent`) and interleaved passes converge on the
  next pass instead of oscillating (`interleaved_passes_converge`), so
  balancing needs no lock.
- `rows_match` refuses to equalise unless the 2 rows hold the same number of
  panes (at least 2) over the same extent. Unequal row lengths mean the grid
  is genuinely ragged: a column is half built or half drained. Equalising 1
  row alone would move it off the other row's integers and manufacture the
  phantom raggedness this module prevents. That case is left to the
  advisor's ragged handling. `balancecmd::balance_text` tells "level
  already" apart from "not a shape worth touching".
- `worker_rows` groups panes into rows by `y`, never by `x`, so it stays
  correct exactly when the columns have drifted, which is the case that needs
  repair. It returns `None` when the tab is not a settled 2-row grid.
- Geometry checks accept a 1-cell difference. `balance::TOLERANCE` (1) leaves
  a column boundary within 1 cell of its target alone, and `tile::TOLERANCE`
  is also 1, and `smoke::off` treats coordinates within 1 cell as equal
  when it checks the tiled grid. This absorbs herdr border rows and herdr's
  rounding of split ratios to 4 decimal places, which moves pane
  rectangles by up to 1 cell. In `analyze_with` a column is "full" when it
  spans the tab height and "half" when it is 1 of 2 stacked rows, within the
  same 1-cell tolerance.
- `layout::Cell::depth` counts the panes stacked over a column slice instead
  of comparing heights, so a column split with an uneven ratio still reads as
  2 rows. A depth of 3 or more means the tab is not a 2-row grid.
- `smoke::ticker` proves a pane's process survived a move by a count it
  writes to a file, not by screen text. A pane squeezed to a few columns by
  a deliberately bad split shows nothing readable, and `pane read` would
  report it as a dead process.
- `horch tile` restores the viewed tab and the pane that tab hands the
  keyboard to. Its park phase moves the focused pane out with `--no-focus`,
  and herdr then hands focus to a pane that stayed. Before the restore,
  tiling put the keyboard on the orchestrator pane every time.
