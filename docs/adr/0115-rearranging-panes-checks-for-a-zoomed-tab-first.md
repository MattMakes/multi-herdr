# Rearranging panes checks for a zoomed tab first

herdr refuses to move panes into or out of a zoomed tab, and reports
`changed: false`, `reason: "zoomed_tab"` instead of failing. A tiler that
ignored zoom would walk through its whole plan achieving nothing. So
`model::Layout` (`crates/horch-core/src/workspace/model.rs`) parses `zoomed`,
true while 1 pane fills the tab, and `arrange::run`
(`crates/horch-core/src/workspace/arrange.rs`) looks up the zoomed tab
(`zoomed_tab`) before it moves anything and refuses to tile, telling the
operator to unzoom.

## Consequences

- Any caller that rearranges panes checks `zoomed` first, not only the tiler.
- `model::Layout` also parses `focused_pane_id`, because a tiler has to know
  the focused pane to hand focus back afterwards.
