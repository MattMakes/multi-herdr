# D09 agent-list: `horch agent-list`

Unit slug: `agent-list`. Branch: `ds/agent-list`.

## GOAL

`horch agent-list [--json] [--no-probe]` lists every agent harness horch can
drive, and for each: whether its binary is found (path), its version (a
`--version` probe with a short timeout, no model turn; skipped with
`--no-probe`), the effort levels it accepts, its key capabilities (resume,
headless, skill exposure), the models the roster uses with it and which
teammates use each model (with their default effort), and its usage pool
state from the cached quota view (no refresh). A summary column says
`available` when the binary is found and the pool is not exhausted.

## CONTEXT

- Read first: `00-conventions.md`. Then: `crates/horch-core/src/harness/mod.rs`
  (`HarnessKind`, `HarnessKind::binary(&HarnessBins)`),
  `harness/capabilities.rs` (`Capabilities.effort` and the rest),
  `crates/horch-core/src/routing/quota_probe.rs` (`harness_version`, used by
  the dataset preflight), `crates/horch/src/dataset/preflight.rs` (how it
  probes versions), `crates/horch/src/cmd/` (`teammatescmd.rs`,
  `quotacmd` or the quota command, for output style and `--json` habits),
  `crates/horch/src/main.rs` (clap subcommands).
- Data-driven: iterate every `HarnessKind` (skip `None`). Do not hard-code
  the list or its length in code or tests: D08 adds a new kind
  (`Antigravity`) in parallel, and it must appear without an edit here. If
  `HarnessKind` has no public "all kinds" list, add one in `harness/mod.rs`
  (1 const or fn; tell D08 through the orchestrator by naming it in your
  report).
- Pure core: put the inventory in `crates/horch-core/src/harness/inventory.rs`
  as a pure function of (roster, bins resolution facts, version facts, quota
  view). The CLI gathers the facts. Test the pure function with a table.
- Output style: an aligned table like `horch teammates`; `--json` emits a
  stable array.

## FILES

own:
- `crates/horch-core/src/harness/inventory.rs` (new), `harness/mod.rs`
  (1 `mod` line and the all-kinds list if missing)
- `crates/horch/src/cmd/agentlist.rs` (new), `crates/horch/src/cmd/mod.rs`
  (1 line), `crates/horch/src/main.rs` (the subcommand only)
- `crates/horch/tests/agent_list.rs` (new)
- `README.md` (1 line in the command list)
- `ai_docs/reports/design-skills/agent-list.md`

do not touch: every other file.

## STEPS

0. Create the worktree (conventions §3).
1. Core inventory + table tests (`agent_list_inventory_*`).
2. CLI command with `--json` and `--no-probe`; the probe has a timeout of
   about 5 s per harness and runs the harnesses in parallel threads.
3. CLI tests (`crates/horch/tests/agent_list.rs`) with fake bins in a temp
   dir (`HORCH_*_BIN` overrides) and a fixture roster: found vs missing
   binary, versions, efforts, models per harness, `--json` shape,
   `--no-probe` runs no binary.
4. Gate. Commits `Harness: Add the agent inventory`, `CLI: Add horch agent-list`.
   Write and commit the report with a sample output. Follow conventions §7.

## DONE WHEN

- `cargo run --bin horch -- agent-list` prints a row per harness on this
  machine; the gate is green.

## REPORT

- Sample output (text and JSON), the all-kinds list name, gotchas.
