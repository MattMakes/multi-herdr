# a7a-workspace report (A7 part a)

Branch `ard/a7a-workspace`. Requirements ARC-19 and ARC-27.

## What changed

- `git mv` of `herdr.rs`, `layout.rs`, `tile.rs`, `balance.rs`, `paneshell.rs` into `crates/horch-core/src/workspace/`. Only `use` paths changed in the moved files.
- The old paths are 3-line shims: `pub use crate::workspace::<m>::*;`. A12 deletes them.
- `workspace/model.rs` holds the wire model types: `Rect`, `Pane`, `LayoutPane`, `Layout`, `Tab`, `Workspace`, `Resize`, `Move`, `Focus`, `NewTab`, `NewWorkspace`, `Direction`, `FocusDir`. `workspace/herdr.rs` re-exports them with `pub use crate::workspace::model::*;`. The private envelope structs and the parsing stay in `workspace/herdr.rs`.
- `layout.rs`, `tile.rs` and `balance.rs` import from `crate::workspace::model`.
- `workspace/client.rs`: trait `WorkspaceClient` (11 methods), implemented for `Herdr` by delegation, and `pub type HerdrClient = Herdr;`. No caller uses the trait yet.
- `workspace/testing.rs`: `FakeWorkspace` with `new`, `fail_next(method, message)`, `set_screen(pane, text)`, `calls() -> Vec<FakeCall>`, `pane_ids()`, `workspace_ids()`. Split allocates `p1`, `p2`, ... The root pane of `workspace_create` also takes the next id. Workspace ids are `w1`, `w2`, ...
- Fixtures: `crates/horch-core/tests/fixtures/herdr/*.json` (7 files).

## Tests added: 7

- `workspace::herdr::tests::arc_19_herdr_parsing_contract`
- `workspace::tests::arc_27_tile_balance_pure`
- `workspace::tests::arc_27_scan_ignores_comment_lines_and_catches_code`
- 4 tests of `FakeWorkspace` in `workspace/testing.rs`

## Decisions

- `WorkspaceClient::workspace_create` has the real signature `(label, cwd: Option<&str>, focus: bool)`. The plan listed `(label, focus)`. The plan said to use the real signature.
- `arc_27_tile_balance_pure` skips lines whose trimmed text starts with `//`. The orchestrator approved this. `workspace/tile.rs:683` has a doc comment that names `Herdr`. That line is unchanged from the base.
- `arc_19_herdr_parsing_contract` lives in `workspace/herdr.rs` tests because the envelope structs are private.

## Gate status

- `just gate` does not exist in the base. I ran the conventions §4 commands.
- The base has 47 failing tests (the same 47 on this branch). The gate rule until the baseline fix merges: the failing set must equal the base set. fmt, build, `teammates --check` and `check-deps` pass.

## For A7 part b

- `tilecmd.rs` still imports through the shims (`horch_core::herdr`, `tile`, `balance`, `layout`).
- `Herdr::send_line`, `wait_output`, `integration_status`, `pane_focus*`, `pane_move*`, `pane_resize` and `tab_*` are not in the trait. Add them when part b needs them.
- `Herdr` calls `crate::agent::herdr_bin()`. A2 changes `agent.rs`.
