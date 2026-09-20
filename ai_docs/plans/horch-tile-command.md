# Brief: `horch tile` reorganizes the live panes into the canonical fleet grid

Repo: /Users/mascott/projects/multi-herdr. Start from `main` (`git pull --ff-only`), create branch `horch-tile`.
Design source: `ai_docs/plans/tiling-manager-spec.md` sections 2 (rules), 4 (model), 5.3 (balance), 5.4 (report), 5.5 (repair), 6 (herdr surface). Survey of today's code: `ai_docs/reports/layout-survey.md`. Read both before writing code.

## What the operator wants
A command to run AFTER workers exist that rearranges every pane in the workspace into this shape:

```
tab 1                              tab 2..N (overflow, no orchestrator)
+-------+--------+--------+        +--------+--------+--------+
|       |   W    |   W    |        |   W    |   W    |   W    |
|   O   +--------+--------+        +--------+--------+--------+
|       |   W    |   W    |        |   W    |   W    |   W    |
+-------+--------+--------+        +--------+--------+--------+
```
O = orchestrator, always leftmost and full height on tab 1. Tab 1 holds up to 4 workers (2 rows x 2 columns). Each overflow tab holds up to 6 (2 rows x 3 columns). Fill order: column 1 top, column 1 bottom, column 2 top, column 2 bottom, column 3 top, column 3 bottom, then the next tab. Equal column widths and equal row heights on every tab; on tab 1 the orchestrator is one of three equal columns (share 1/3 with two worker columns, 1/2 with one, whole tab with none).

Command surface:
- `horch tile` — apply: move panes, close emptied tabs, balance, then print the report.
- `horch tile --plan` — print the moves it would make and the resulting grid, change nothing.
- `horch tile --workspace <id>` as the other layout commands accept.
- `horch layout` — must now report every tab (today it reports one). Same renderer per tab, one block per tab, in tab order.
Scope is the reorganize command, the multi-tab report, AND automatic tiling after every spawn and every worker close (see the ADDED section at the end, which supersedes anything here that says otherwise).

## Algorithm (decided; implement this, do not redesign)
Two phases, so it works regardless of how scrambled the current shape is and never depends on same-tab moves:

1. Gather. `herdr tab list --workspace <ws>`; for each tab `herdr pane list`/`pane layout` to get every pane with tab id and rect. Identify the orchestrator: the mailbox role `orchestrator` pane, else the leftmost full-height pane of the tab that has the most full-height panes... no: else the leftmost full-height pane of the FIRST tab, exactly as `layout.rs` does today. Every other pane is a worker. Order workers deterministically: by current tab index, then x, then y (reading order), so a re-run is stable and roughly preserves who sits near whom.
2. Park. Create one scratch tab (`herdr tab create --workspace <ws> --label "horch-tile-scratch" --no-focus`) and move every worker into it with `herdr pane move <pane> --tab <scratch> --split right` (any shape is fine there). When a source overflow tab becomes empty, close it (`herdr tab close`), never tab 1 or the orchestrator's tab. After this, tab 1 holds the orchestrator alone. Skip the park phase for a pane already in its target slot only if you can prove it; otherwise always park — simplicity beats cleverness here.
3. Place. Walk the ordered workers and for each compute its slot (tab index, column, row) from the capacities (4 on tab 1, 6 per overflow tab) and execute exactly the split the spec's placement rule gives:
   - first worker of tab 1: `pane move <w> --tab <tab1> --split right --target-pane <orchestrator>`
   - column c bottom: `--split down --target-pane <top pane of column c>`
   - column c+1 top: `--split right --target-pane <top pane of column c>`
   - first worker of a new overflow tab: `pane move <w> --new-tab` (or `tab create` then move; whichever the installed herdr supports; created without focus).
   Use `--ratio` on the move when the installed herdr accepts it (spec 5.3 gives the values); balance afterwards regardless.
4. Close the scratch tab when it is empty. Restore focus to the pane that had it before (`pane layout` reports `focused_pane_id`), or to the orchestrator.
5. Balance every tab: reuse the resize simulation in `balance.rs` but drive the targets from the known grid you just built (you know exactly which panes are columns and rows; do not re-infer from edges). Tab 1: orchestrator share per the rule above. Overflow tabs: N equal columns, rows 1/2. Note the existing `worker_rows` treats a lone full-height pane as the orchestrator, which is wrong on an overflow tab with an odd worker count; do not reuse that inference on overflow tabs.
6. Print the report (every tab).

`--plan` runs step 1, computes steps 2-5 as a list of herdr commands with a rendered "after" grid, prints them, and exits 0 without calling any mutating herdr command.

## herdr surface: verify before coding (gate)
Run `herdr --version` and `herdr tab --help`, `herdr tab create --help`, `herdr tab close --help`, `herdr pane move --help`, `herdr pane layout --help`. Compare with `herdr-docs/cli-reference.md` lines 144-175 and `socket-api.md` ~230. Record in `ai_docs/reports/horch-tile-herdr-surface.md`: the exact flags available, the JSON shapes returned (run each read-only command once against the current workspace and paste trimmed output), and whether `pane move` keeps the running process (test in step "smoke" below before relying on it). If `pane move` does not exist or kills the process, STOP and report to the orchestrator with `horch tell` before writing the mutating code; the plan-only mode still has value.

## Files
- `crates/horch-core/src/herdr.rs`: add `tab_list`, `tab_create`, `tab_close`, `pane_move`, and `focused_pane` (from `pane_layout`), keeping the existing `Command::new("herdr")` style and `Envelope<T>` parsing. `WorkspaceCreateResult` may now keep `tab_id`.
- `crates/horch-core/src/tile.rs` NEW, pure: `Slot`, `capacity(tab_index)`, `slots_for(n_workers)`, `plan(orchestrator, workers, existing_tabs) -> Vec<Op>` where `Op` is an enum of the herdr calls (ParkTabCreate, Move{pane, target, split, ratio}, NewTab{pane}, CloseTab, Focus, Resize...). Unit tests over synthetic inputs: 0, 1, 4, 5, 10, 11, 16 workers give the expected slot list and op sequence; ordering is stable; no op ever targets tab 1's orchestrator with `down`; overflow tabs never exceed 3 columns.
- `crates/horch-core/src/layout.rs`: make `analyze`/`render` usable per tab and add a multi-tab wrapper; the orchestrator is only sought on its own tab.
- `crates/horch/src/cmd/tilecmd.rs` NEW: the CLI driver; `crates/horch/src/main.rs`: `Tile { plan: bool, workspace: Option<String> }`; `crates/horch/src/cmd/layoutcmd.rs`: iterate tabs.
- README.md "Layout" section: two sentences on `horch tile`. `teammates/_base/fleet-orchestrator.md`: add one line under the spawning paragraph: "Run `horch tile` after spawning a batch of workers to lay the grid out; run `horch layout` to see it." Pin it in `crates/horch-core/tests/golden_prompts.rs` the way the previous sanctioned changes are pinned.

## Smoke test (no tokens)
Extend `horch smoke` with `horch smoke tile`: scratch workspace, one fake orchestrator pane and 7 fake `smoke` panes (use the same token-free `smoke` teammate the fleet smoke uses, or plain `herdr pane run` shells that `sleep`), deliberately created in a bad shape (e.g. all split down in one column). Run the tile planner and apply; assert via `pane layout`: tab 1 has the orchestrator leftmost full height plus 4 panes in 2x2, tab 2 has 3 panes as 2 top + 1 bottom in 2 columns... check: 3 workers on an overflow tab = column 1 top, column 1 bottom, column 2 top. Assert every original pane id still exists (processes survived), then tear the workspace down. Run `just herdr-fleet-smoke` too to confirm nothing else regressed.

## Checks before PR
`cargo build` 0 new warnings; `rustfmt --check` on touched files only (agent.rs and 5 files in crates/horch have pre-existing diffs, leave them); `cargo test` workspace-wide; `horch teammates --check`; `horch smoke tile` and `horch smoke fleet` pass; then `horch tile --plan` against THIS live workspace (read-only) and paste its output in the PR body. Do NOT run `horch tile` (apply) on this live workspace; the orchestrator will do that after review.

## Commit and PR
Small commits per file group (herdr wrapper; tile planner + tests; CLI + layout; smoke; docs + prompt). Branch `horch-tile`, PR against main titled "Add horch tile to reorganize the fleet grid across tabs", body: what it does, the algorithm in five lines, the herdr surface findings, the smoke result, the live `--plan` output. Report with `horch done` in Simplified Technical English: PR URL, files, test counts, and anything you could not verify.

## Out of scope
Automatic placement in `horch spawn`. Any change to `horch balance`'s public behaviour beyond multi-tab. 3-row layouts. Windows-specific paths.

## ADDED 2026-09-20: run the tiler automatically, no tokens involved

The operator does not want to spend model tokens to get tiling. So horch itself runs the tiler at the two moments the grid changes, deterministically, with no prompt to any agent:

1. **After every `horch spawn`.** In `crates/horch/src/cmd/spawn.rs`, after `pane_run` succeeds (where `equalize_quietly` runs today), call the tile apply path for the workspace instead. Errors are non-fatal: print one line to stderr and keep the spawn result. Opt-out: `--no-tile` flag on spawn, and env `HORCH_TILE=0` (read by the same code path) for an operator who wants the old behaviour. The spawn still prints the new pane id on stdout, unchanged.
   Efficiency: if the new pane is already in the slot the placement rule gives (the common case when the orchestrator did nothing clever), the park-and-place phases are a lot of herdr calls for nothing. Add a fast path: compute the plan; if the plan is "no moves, balance only", skip parking and only balance. Otherwise run the full two-phase apply. This keeps spawn snappy without a second algorithm.
2. **After a worker closes its own pane.** The detached post-close child in `crates/horch/src/cmd/balancecmd.rs` (spawned from `messaging.rs` on `horch done`) currently runs balance; make it run the tile apply path with the same fast path. That fills the hole a departed worker leaves and closes an emptied overflow tab.
3. **Orchestrator prompt line** (replaces the line in the Files section above): "`horch spawn` lays the grid out for you after each spawn, and again when a worker closes. You never pass `--from-pane` or `--direction`. Run `horch layout` to see the grid, and `horch tile` only if it looks wrong." Update `teammates/_base/fleet-orchestrator.md` accordingly and pin it in the golden test. Also remove the "Spawned panes split YOUR pane by default; add --from-pane ... --direction ..." paragraph and the `horch layout ... next split` advice from that base prompt, since the orchestrator no longer needs to choose. Keep `--from-pane`/`--direction` working on the CLI as a manual escape hatch; when they are given, still tile afterwards unless `--no-tile`.
4. **Smoke:** `horch smoke tile` also covers: spawn 5 fake panes in a row with plain `horch spawn smoke` (no placement flags) and assert the grid is tab 1 2x2 plus tab 2 with one pane, then close one tab-1 pane via its `horch done` path and assert the tab-2 pane moved down into the hole and tab 2 is gone.
5. The `horch tile` command stays as the manual re-run.

Re-read this brief from the top before continuing; the Files, Smoke, Checks, and PR sections apply to the added scope too.
