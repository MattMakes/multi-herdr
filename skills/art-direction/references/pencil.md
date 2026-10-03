# Pencil canvas (optional)

Load this file only when a Pencil design tool is available in your session and the brief asks for mockups on a `.pen` canvas. Everything in the main workflow works without it. Tool names below are the Pencil tool names; your harness may show them with a server prefix.

## Session start

1. `get_editor_state()`: see which document is open and what is selected. If it fails, Pencil is not running; report that and continue without mockups.
2. `open_document("<path>.pen")`: open the target file. A path that does not exist creates a new document.
3. `get_guidelines(topic=...)`: `"landing-page"` for marketing pages, `"web-app"` for app screens, add `"table"` for data tables.
4. `set_variables({...})`: load the contract tokens (color roles, fonts, spacing). Variables do not persist between sessions or between files; set them on every `open_document`.
5. `get_variables()`: confirm every token is stored.
6. `snapshot_layout()`: confirm the canvas size.

## Before you edit an existing file

Read it first: `batch_get(patterns=["*"])` for the node tree and `snapshot_layout()` for computed sizes. Never describe a `.pen` file from memory. Note any screen whose size does not match the target and say so before you build on it.

## Building

- Canvas width: 1440 for marketing pages; check the same design at 390 wide before handoff. App screens use the project's target resolution. Mobile screens use 390 x 844.
- `batch_design` takes at most 25 operations per call. Split larger builds. Operation forms: `I` insert, `C` copy, `U` update, `R` replace, `M` move, `D` delete, `G` generate an image.
- After each structural step, run `snapshot_layout()`. A node with width or height 0 is usually a flex container with no explicit size, or a `fill_container` child inside a parent with no width. Give it an explicit size and check again before you continue.
- After each major step, run `get_screenshot(nodeId=...)` and compare it with the storyboard shot.
- Place a new screen with `find_empty_space_on_canvas(direction="right", ...)`. Keep screens left to right with about 120 px between them; stack states of one screen vertically with about 80 px between them.
- Name screens `Screen <n> - <feature> (<state>)`. Add iterations as new screens with a `[v2]` suffix in the same file; do not create a new file for an iteration.
- `get_style_guide_tags()` and `get_style_guide(tags=[...])` give inspiration only. The contract wins every conflict.

## Token drift on the canvas

1. `search_all_unique_properties(parentIds=["<root>"], propertyNames=["bg","color","fontFamily","fontSize","gap"])`: list every value in use.
2. Compare the list with the contract. Every value outside the contract is drift.
3. `replace_all_matching_properties(parentIds=[...], matchProperty="bg", matchValue="<old>", newValue="<new>")`: fix drift one property at a time (bg, color, borderColor, fill, fontFamily). Take care with spacing: one number can serve several purposes.
4. `get_screenshot` to confirm.

## Handoff

- Before a large change, add a note on the canvas with the date, the list of screens, the active tokens and what will change.
- `export_nodes(nodeIds=[...], format="png")` exports frames for review or for the builder. Link the exports from the contract.
