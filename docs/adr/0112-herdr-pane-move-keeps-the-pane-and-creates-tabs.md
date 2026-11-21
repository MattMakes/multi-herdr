# herdr `pane move` keeps the pane and is how horch creates tabs

`Herdr::pane_move` (`crates/horch-core/src/workspace/herdr.rs`) runs
`herdr pane move <pane> --tab <tab> --split <d> [--target-pane <target>]
[--ratio <r>] --no-focus`. Verified against herdr 0.8.2, a move keeps the
pane id and the running process, so tiling rearranges live agents without
restarting them. horch creates a tab by moving an existing pane into it
(`Herdr::pane_move_new_tab`, tile `Op::NewTab`), not with `tab create`, which
would add a fresh shell pane that then has to be disposed of.

## Consequences

- `--ratio` is the share the TARGET pane keeps. The moved pane gets
  `1 - ratio`.
- A move into the tab the pane already occupies is refused with
  `changed: false, reason: "same_tab"`. Repositioning a pane inside its own
  tab means moving it out and back.
- `herdr pane move <pane> --new-tab --label <label> --no-focus` creates a tab
  holding only the moved pane and returns its id as `created_tab`. This form
  takes neither `--split` nor `--ratio`, and the label flag is `--label`, not
  `--tab-label`.
- herdr closes a tab that loses its last pane. In the reply (`model::Move`),
  `source_layout` is absent when the move emptied and so closed the source
  tab, and `created_tab` is present only for `--new-tab`. Both fields are
  optional, as the trimmed real 0.8.2 reply in the tests shows.
