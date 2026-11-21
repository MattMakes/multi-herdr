# herdr focus moves only by neighbour steps

herdr 0.8.2 has no focus-by-id for an ordinary pane. `herdr pane focus`
moves only to a neighbour in a direction, and `herdr plugin pane focus`
answers `plugin_pane_not_found`. `herdr tab focus <tab>` is the only way to
focus a specific pane's tab. So `Herdr::tab_focus` and
`Herdr::pane_focus_walk` (`crates/horch-core/src/workspace/herdr.rs`) put a
named pane back in focus by focusing its tab and then stepping 1 neighbour
at a time, steered by `tile::step_toward`
(`crates/horch-core/src/workspace/tile.rs`).

## Consequences

- `model::FocusDir` is deliberately separate from `Direction`. `Direction` is
  split geometry, where `left` and `up` are not words herdr accepts, and its
  `FromStr` rejects them on purpose.
- `herdr pane focus --direction <d> --pane <origin>` answers exit 0 with
  `changed: false, reason: "no_neighbor"` when nothing sits on that side.
  `Herdr::pane_focus` reports running out of room as data, not an error, and
  a walk uses it as a loop breaker.
- `herdr pane layout <pane>` reports the whole tab that holds that pane. The
  walk probes with the target pane itself to learn its tab, and returns false
  when it is not the expected tab.
- `pane_focus_walk` returns false instead of an error when focus cannot be
  placed. The caller is restoring a view, and a view that cannot be restored
  must never fail the operation (such as `horch tile`) that moved the panes.
- Like `tab focus`, a walk pulls the whole workspace into view.
- The step budget is the pane count. Each step closes the larger of the 2
  axis gaps, so focus reaches any pane of a 2-row grid in at most 2 moves.
  The budget only stops a walk that oscillates because herdr picked a
  different neighbour than the geometry suggested.
- `tile::step_toward` picks the direction from the axis on which the 2 rects
  are separated, and uses centre distance only to break the tie for a
  diagonal neighbour separated on both axes. Centre distance alone fails on
  the ragged bottom row a tab ends with whenever it is not full: the bottom
  pane spans 2 columns, so its centre sits left of the top-right pane above it
  although it is directly below. A centre-only rule sent the walk left, right,
  left again and never arrived (measured in a live run). Rects that overlap on
  both axes are the same pane and give `None`.
