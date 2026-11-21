# Tiling restores the operator's view by following the focused pane

`arrange::restore_focus` (`crates/horch-core/src/workspace/arrange.rs`) puts
the operator's view back after `horch tile`, using the rule in
`tile::focus_target` (`crates/horch-core/src/workspace/tile.rs`). The park
phase moves every worker out of its tab with `--no-focus`, so herdr hands
that tab's focus to a pane that stayed, on tab 1 always the orchestrator.
Nothing gives it back, because every later move also passes `--no-focus`.
Restoring the tab alone, or the tab by id, is not enough: an overflow tab
loses its last pane to the park phase, herdr closes it and the tiler
rebuilds it under a new id. The view follows the pane.

## Consequences

- The tab rule, in priority: the viewed tab stays viewed if it still exists,
  else the tab that now holds the previously focused pane, else the
  orchestrator's tab. The middle case is common: matching the old tab id
  alone failed for every overflow tab in a live run.
- The pane rule: the focused pane stays focused if it is on the viewed tab,
  else the orchestrator pane when the viewed tab is the orchestrator's, and
  herdr's own choice on any other tab.
- `focus_target` is pure. It works on the snapshot the command already built
  and calls no herdr command.
- Restoring is best effort throughout. A view that cannot be restored must
  never fail a tiling that has already moved the panes.
- `arrange::run` reads the focused pane and tab from the snapshot before
  anything moves, which costs no herdr call. The tab is kept separately for
  a herdr too old to report which pane holds the focus.
- In that fallback, the restored tab is the one the operator was viewing if
  it survived, else the orchestrator's. It comes from the snapshot taken
  before the rebuild. The snapshot taken after only names the tab herdr
  happened to land on, which is what is being corrected.
- Focus is restored once, after the balance, so the view settles on the
  finished grid. The fast path moved nothing and skips it, because a focus
  call herdr does not need pulls the workspace into view for no reason.
