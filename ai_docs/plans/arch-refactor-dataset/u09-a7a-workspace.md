# U09 a7a-workspace: move the herdr client and the tiler into workspace/

Unit slug: `a7a-workspace`. Branch: `ard/a7a-workspace`. Phase: A7 (part a).
Requirements: ARC-19, ARC-27.

## GOAL

`crates/horch-core/src/workspace/` holds the herdr client, its public model
types, a `WorkspaceClient` trait with a real and a fake implementation, and
the pure tiler modules (layout, tile, balance, paneshell). The old module
paths are re-export shims. Behavior and every existing test are unchanged.

## CONTEXT

- Read first: `ai_docs/plans/arch-refactor-dataset/01-fleet-conventions.md`.
- Read in `00-master-plan.md`: §1 (layout: `workspace/{client,herdr,model,layout,tile,balance,arrange,paneshell}.rs`),
  §2 "workspace", §3 "A7", §4 ARC-19 and ARC-27 rows, §4 Spec A §17 item 12.
- This unit is part (a) of A7. Part (b) comes later and does: `tilecmd.rs`
  orchestration → `workspace/arrange.rs`, the `messaging/` modules,
  `send_line`/`wait_for_tail` → `messaging/delivery`, and `lifecycle::done`.
  Do NOT do part (b). Do not edit `crates/horch/src/cmd/tilecmd.rs`,
  `mailbox.rs`, `message.rs`.
- Today: `crates/horch-core/src/herdr.rs` (931 lines) has the wire structs
  (`Rect`, `Pane`, `LayoutPane`, `Layout`, `Tab`, `Workspace`, `Resize`,
  `Move`, `Focus`, `NewTab`, `NewWorkspace`), the enums `Direction` and
  `FocusDir`, and `pub struct Herdr` with methods `pane_get`, `pane_list`,
  `pane_split`, `pane_run`, `pane_send_text`, `pane_send_keys`,
  `pane_close`, `pane_exists`, `pane_read`, `pane_layout`, `focused_pane`,
  `tab_*`, `workspace_list`, `pane_focus*`, `pane_move*`, `pane_resize`,
  `workspace_create`, `workspace_close`, `wait_output`,
  `integration_status`, `agent_prompt`, `send_line`.
  15 files import `herdr::`.
- Parallel units edit other files at the same time: U05 (A1) changes
  `no_tile`/`resume` parameters in `crates/horch/src/cmd/*.rs`; later the A2
  unit changes `agent.rs`. Keep your edits inside the files you own so that
  rebases stay small.

## FILES

own:
- `crates/horch-core/src/workspace/mod.rs` (new)
- `crates/horch-core/src/workspace/client.rs` (new)
- `crates/horch-core/src/workspace/herdr.rs` (new; the moved `herdr.rs`)
- `crates/horch-core/src/workspace/model.rs` (new)
- `crates/horch-core/src/workspace/testing.rs` (new)
- `crates/horch-core/src/workspace/layout.rs`, `tile.rs`, `balance.rs`, `paneshell.rs` (new; moved)
- `crates/horch-core/src/herdr.rs`, `layout.rs`, `tile.rs`, `balance.rs`, `paneshell.rs` (become shims)
- `crates/horch-core/src/lib.rs` (add `pub mod workspace;`)
- `crates/horch-core/tests/fixtures/herdr/**` (new, wire JSON samples)
- `ai_docs/reports/arch-refactor-dataset/a7a-workspace.md`

do not touch: every other file. Callers keep compiling through the shims.

## STEPS

1. Create the worktree (conventions §2).
2. Move files with `git mv` so history follows:
   `herdr.rs` → `workspace/herdr.rs`, and the same for `layout.rs`,
   `tile.rs`, `balance.rs`, `paneshell.rs`. Then create a new file at each
   old path that only re-exports, for example
   `//! Moved to [`crate::workspace::tile`]; this shim goes away in A12.`
   followed by `pub use crate::workspace::tile::*;`. Re-export private-use
   items too if callers use them (check with `cargo build`).
   Change the moved files only in their `use` paths. Check:
   `git diff -M --stat` shows renames with high similarity, and
   `cargo test --workspace` passes.
3. `workspace/model.rs`: move the public types that callers use (`Pane`,
   `Layout`, `LayoutPane`, `Tab`, `Workspace`, `Rect`, `Direction`,
   `FocusDir`, and the reply structs that callers read) out of
   `workspace/herdr.rs` into `model.rs`, and re-export them from
   `workspace/herdr.rs` so `herdr::Pane` still resolves. The JSON wire
   parsing stays in `workspace/herdr.rs` (serde derives may stay on the
   model types if they are the wire shape; do not restructure parsing).
4. `workspace/client.rs`: the trait.
   ```rust
   pub trait WorkspaceClient {
       fn pane_get(&self, pane: &str) -> Result<Pane>;
       fn pane_list(&self, workspace: &str) -> Result<Vec<Pane>>;
       fn pane_split(&self, from: &str, direction: Direction) -> Result<String>;
       fn pane_run(&self, pane: &str, command: &str) -> Result<()>;
       fn pane_close(&self, pane: &str) -> Result<()>;
       fn agent_prompt(&self, pane: &str, text: &str) -> Result<()>;
       fn pane_send_text(&self, pane: &str, text: &str) -> Result<()>;
       fn pane_send_keys(&self, pane: &str, keys: &str) -> Result<()>;
       fn pane_read(&self, pane: &str, source: &str) -> Result<String>;
       fn workspace_create(&self, label: &str, focus: bool) -> Result<NewWorkspace>;
       fn workspace_close(&self, workspace: &str) -> Result<()>;
   }
   ```
   Match the argument types of the existing `Herdr` methods exactly (read
   `workspace_create`'s real signature and use it). Implement the trait for
   `Herdr` by delegating to the inherent methods. Add
   `pub type HerdrClient = Herdr;`. Do not change any caller to use the
   trait in this unit.
5. `workspace/testing.rs`: `pub struct FakeWorkspace` implementing
   `WorkspaceClient` in memory: a `RefCell` state of workspaces and panes, a
   call log (`Vec<FakeCall>` with method name and args), injected failures
   (`fail_next(method: &'static str, message)`), split allocates ids
   `p1`, `p2`, ...; close removes; `pane_read` returns text set by
   `set_screen(pane, text)`. Unit tests for the fake: split adds, close
   removes, an injected failure fires once.
6. Tests:
   - `arc_19_herdr_parsing_contract` (in `workspace/herdr.rs` tests or a new
     `crates/horch-core/tests/workspace.rs`): for each wire shape that
     `Herdr` parses (pane get, pane list, layout, tab list, workspace list,
     split reply, workspace create reply), a JSON fixture under
     `crates/horch-core/tests/fixtures/herdr/` parses into the model types
     with the expected field values. Take the JSON shapes from the existing
     herdr tests and from `crates/horch-e2e/src/bin/fake-herdr.rs`.
   - `arc_27_tile_balance_pure`: read the source of `workspace/layout.rs`,
     `workspace/tile.rs`, `workspace/balance.rs`; assert none contains
     `std::process`, `Command::new`, `Herdr`, `WorkspaceClient`,
     `crate::workspace::herdr`, `crate::herdr` or `std::env`. If one does
     today, stop and send `QUESTION:` with the line.
   - All existing tests in the moved modules keep their names and bodies.
7. Gate after each step. Commits: `A7: Move herdr client and tiler into workspace/`,
   `A7: Split workspace model types`, `A7: Add WorkspaceClient and FakeWorkspace`,
   `A7: Add ARC-19 and ARC-27 tests`.
8. Write and commit the report (the trait, the fake, the shims, gotchas for
   A7 part b). Follow conventions §6 to finish.

## CONSTRAINTS

- The tiler is not rewritten. `horch tile --plan` output must not change.
- No new crate.

## DONE WHEN

- `cargo test --workspace` passes with the same test count as the base plus
  your new tests.
- `arc_19_herdr_parsing_contract` and `arc_27_tile_balance_pure` pass.
- The old module paths are shims of 1 to 5 lines each.
- The full gate is green (except failures that exist on the base; name them).

## REPORT

- `horch note` after each commit.
- `horch done` summary: files moved, trait signature, fake API, gotchas.
