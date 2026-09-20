# Bug: `horch tile` takes over the pane the operator is viewing

GOAL: After `horch tile` runs, by hand or automatically after `horch spawn`
and after a worker closes, the operator sees the same tab and the same
focused pane as before, whenever both still exist. `horch smoke tile` and
`horch smoke fleet` prove it.

## CONTEXT

The operator reports: "the horch tile command takes over the currently
viewed pane". Treat this as a bug. Reproduce it first, then fix the cause.

What the code does today (read these before you reproduce):

- `crates/horch/src/cmd/tilecmd.rs:430-443`. After `apply(herdr, &plan)`
  the command calls `herdr.tab_focus(active)` to put the viewed TAB back, or
  falls back to the orchestrator's tab. It never restores the focused PANE.
- `crates/horch-core/src/herdr.rs:358-361` `focused_pane()` reads
  `focused_pane_id` from `pane layout`. `herdr.rs:102-105` records
  `active_tab_id`. So both values are available before the tiler moves
  anything.
- `crates/horch-core/src/herdr.rs:416-419`: `herdr tab focus <tab>` is the
  only focus call in use. `herdr pane focus` moves to a NEIGHBOUR with
  `--direction left|right|up|down` and an optional `--pane <ID>` origin. It
  cannot focus a pane by id. See `herdr pane focus --help` (herdr 0.8.2).
- `herdr-docs/cli-reference.md:415` lists `herdr plugin pane focus <pane_id>`.
  Nobody has tested whether that command works from a plain shell. Test it
  in Step 1; if it focuses a pane by id, use it and skip the neighbour walk.
- Every split and move already passes `--no-focus` (`herdr.rs:311, 459,
  471, 513`). Read `crates/horch/src/cmd/spawn.rs` to see what value of
  `focus` the spawn path passes to the split at `herdr.rs:505`.
- The auto-tile hook runs after every `horch spawn` and after every worker
  pane closes. `crates/horch/src/cmd/smoke.rs:411-575` has `smoke tile` and
  the spawn-tiles check. Read how `smoke tile` builds its scratch workspace;
  reuse that so you never tile the live fleet workspace.

PRIOR WORK, from the ledger (worker opus-2, PR #4, merged as `5a40de7`):
- `ai_docs/reports/horch-tile-herdr-surface.md` is the verified herdr
  surface: exact flags, JSON shapes, and the finding that `pane move` keeps
  pane ids and live processes. Trust it. Section 1 line 41 states there is
  no focus-by-id; re-check that against `herdr plugin pane focus`.
- `ai_docs/plans/horch-tile-command.md` is the original spec.
- Tiling tests: 299 pass at merge time. `horch smoke tile` prints
  `PASS: a bad shape became the canonical grid over 2 tabs, all 7 panes still running.`

## FILES

Own:
- `crates/horch-core/src/tile.rs`
- `crates/horch-core/src/herdr.rs`
- `crates/horch/src/cmd/tilecmd.rs`
- `crates/horch/src/cmd/spawn.rs`
- `crates/horch/src/cmd/smoke.rs`
- `ai_docs/reports/horch-tile-focus.md` (new)

Do NOT touch (another worker owns them right now):
- anything under `teammates/`, `skills/`, `docs/`
- `README.md`
- `crates/horch-core/src/skills.rs`, `launch.rs`, `teammates.rs`, `prompts.rs`
- `crates/horch-core/tests/`
- `.herdr-orchestrator/`

WARNING: never run `horch tile` (apply) against the live fleet workspace
you are running in. It holds the orchestrator and another worker. Use a
scratch workspace, as `smoke tile` does. `horch tile --plan` is read-only
and safe.

## STEPS

Run `horch note "..."` after each step, with the step number.

### Step 1. Reproduce and write the report

1. Read the files named in CONTEXT.
2. Build: `cargo build --bin horch`.
3. Create a scratch workspace the way `smoke tile` does (read
   `smoke.rs:417-533` and reuse its helper, or run the same herdr commands
   by hand). Put 1 orchestrator-like pane and 3 worker-like panes in it,
   deliberately not in canonical shape.
4. Focus a NON-orchestrator pane on tab 1 with `herdr pane focus --direction ...`.
   Record `herdr pane layout` JSON: `active_tab_id` and `focused_pane_id`.
5. Run `target/debug/horch tile` against the scratch workspace. Record the
   same two values after.
6. Repeat with the viewed tab set to a second tab (create one with
   `herdr tab create --focus`), then run tile again. Record before and after.
7. Repeat for `horch spawn`: from the scratch workspace, spawn a
   `smoke` teammate (`horch spawn smoke "..."`, token-free) while a
   non-orchestrator pane is focused. Record before and after.
8. Test `herdr plugin pane focus <pane_id>` on the scratch workspace. Record
   whether it focuses that pane by id, and its exact output.

Write `ai_docs/reports/horch-tile-focus.md` with: a table of the 4
before/after observations (active tab, focused pane), the exact command
lines, the verdict on `herdr plugin pane focus`, and the cause in 3
sentences naming the code line.

Check: the report exists. If none of the 4 cases changes focus, stop and
send `horch tell orchestrator "[<role>] QUESTION: I cannot reproduce. <what
you tried>"` and wait.

### Step 2. Define the wanted behavior in code comments and a pure function

The rule, in this priority:
1. The viewed tab stays the viewed tab if it still exists after tiling.
   Otherwise the viewed tab becomes the tab that now holds the previously
   focused pane. Otherwise the orchestrator's tab.
2. The focused pane stays focused if it is on the viewed tab after tiling.
   Otherwise focus the orchestrator pane when the viewed tab is tab 1, and
   leave herdr's choice on other tabs.

Add to `tile.rs`:
- `pub struct FocusState { pub tab: String, pub pane: String }`.
- `pub fn focus_target(before: &FocusState, after: &Layout-or-snapshot,
  orchestrator: &str) -> FocusTarget` as a PURE function over the snapshot
  types that `tilecmd.rs` already builds (`TabShape`, `TabRef`, the
  snapshot struct). No herdr calls inside.
- If `herdr plugin pane focus <id>` did NOT work in Step 1, also add
  `pub fn step_toward(from: &Rect, to: &Rect) -> Option<Direction>`: the
  neighbour direction whose axis has the larger center distance; `None`
  when the rects overlap on both axes. Use the rect type `pane layout`
  already parses in `herdr.rs`.

Unit tests in `tile.rs` for `focus_target` (tab kept; tab gone, pane moved;
both gone) and for `step_toward` (left, right, up, down, same).

Check: `cargo test -p horch-core tile` passes.

### Step 3. Implement the restore

In `herdr.rs`:
- If Step 1 showed `herdr plugin pane focus <id>` works: add
  `pub fn pane_focus_by_id(&self, pane: &str) -> Result<()>` wrapping it.
- Otherwise add `pub fn pane_focus_walk(&self, tab: &str, target: &str)
  -> Result<bool>`: loop at most `pane_count` times; read `pane layout` for
  the tab; if `focused_pane_id == target` return `Ok(true)`; compute
  `step_toward(focused.rect, target.rect)`; call
  `herdr pane focus --direction <d> --pane <focused>`; if `None` or the
  focused pane does not change, return `Ok(false)`.

In `tilecmd.rs`:
- Capture `FocusState` before `apply`. Use the values the command already
  reads (`active_tab_id`, `focused_pane`). If either is missing, skip
  restore and keep today's behavior.
- After `apply` and after the column balance, call `tile::focus_target`,
  then `tab_focus` on the chosen tab, then the pane focus call. Log one
  line in the existing output style: `focus: kept <tab> <pane>` or
  `focus: moved to <tab> <pane> (<reason>)`.
- Keep the existing fallback to the orchestrator tab when the state is
  unreadable.

In `spawn.rs`:
- Read what `focus` value the split gets. If the split focuses the new
  pane, pass `false`. The operator's view must not move on spawn. If
  changing it breaks how the worker pane receives its first task (the
  briefing is typed into the pane; check `cmd/worker.rs` and the send-text
  path), record why in the report and leave it; then the auto-tile restore
  in `tilecmd.rs` must still bring focus back.

Check: rebuild; repeat Step 1 cases 4 to 7 by hand on a fresh scratch
workspace; all 4 keep the viewed tab and focused pane. Record the
after-fix table in the report.

### Step 4. Prove it in the smoke checks

In `smoke.rs`:
- `smoke tile`: before applying, focus a non-orchestrator pane on tab 1 and
  record `FocusState`. After tiling, assert the viewed tab and focused pane
  are unchanged when that pane stayed on tab 1. Print
  `PASS: focus stayed on <tab> <pane>.` as an extra line. Keep the existing
  PASS line and the 7-panes assertion.
- The spawn-tiles check (`smoke.rs:551-575`): focus a worker pane before the
  spawn; after the spawn and its auto-tile, assert focus is unchanged.
  Print `PASS: horch spawn left focus on <pane>.`

Check: `horch smoke tile` and `horch smoke fleet` both PASS with the new
lines. Paste both outputs into the report.

### Step 5. Verify, commit, PR

```
cargo build --release --bin horch
cargo test --workspace
cargo fmt --check -p horch-core -- crates/horch-core/src/tile.rs crates/horch-core/src/herdr.rs
cargo fmt --check -p horch -- crates/horch/src/cmd/tilecmd.rs crates/horch/src/cmd/spawn.rs crates/horch/src/cmd/smoke.rs
target/release/horch smoke tile
target/release/horch smoke fleet
target/release/horch tile --plan
```

`cargo fmt --check` on other files has pre-existing diffs; check only the
files you touched. `horch tile --plan` on the live workspace is read-only.

Branch: `horch-tile-keep-focus` from `main`. First commit line:
`horch tile: keep the operator's tab and focused pane`. Push. Open a PR
against `main` with `gh pr create`. PR body: the before and after tables,
the `herdr plugin pane focus` verdict, the cause, the smoke outputs, the
test count. Add one line: "README not updated; the orchestrator owns that
file in a parallel task."

## CONSTRAINTS

- Do not tile the live fleet workspace. Scratch workspaces only.
- Do not change the tiling algorithm, the slot order, or the column
  balance. This task only adds focus capture and restore.
- Keep every herdr call behind a method on `Herdr` in `herdr.rs`, as the
  existing code does. No raw `herdr` invocations in `tilecmd.rs`.
- No subagents, no Agent tool, no background tasks. If the task needs more
  workers, send `QUESTION:` to the orchestrator with the split you propose
  and wait.
- Do not edit files outside FILES. If a fix needs one, send `BLOCKED:` and
  wait.

## DONE WHEN

- `ai_docs/reports/horch-tile-focus.md` has the before-fix table, the cause,
  the `plugin pane focus` verdict, the after-fix table, and both smoke
  outputs.
- `cargo test --workspace` passes with the new `tile.rs` unit tests.
- `horch smoke tile` and `horch smoke fleet` print the new focus PASS lines.
- PR open against `main`.

## REPORT

- `horch note "..."` after each step.
- `horch tell orchestrator "[<role>] BLOCKED: ..."` if stuck, then wait.
- `horch done "..."`: PR URL, branch, commit, cause in one sentence, whether
  `plugin pane focus` worked, files changed, test count, anything noticed
  outside scope and left alone.
