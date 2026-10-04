# G7 data-dir: every horch process in a fleet reads one skill store

Single-branch work: follow `ai_docs/plans/finish/01-single-branch.md`.

## GOAL

The orchestrator pane, each worker wrapper and each worker's agent (and the
`horch` commands that agent runs) resolve the same data root (skill store)
as the `horch fleet` or `horch spawn` that created them, even when herdr does
not pass the spawner's environment to a split pane.

## CONTEXT

- G3 (`ai_docs/reports/finish/worker-env.md`, "Not done"): `8873d0d` carries
  `data_root` in the brief, and `horch worker` uses it. The orchestrator pane
  and the worker's agent do not get it. A pane that `horch spawn` splits does
  not inherit the spawner's env, and herdr does not apply workspace `--env`
  to split panes (L2, LA-5).
- Proposed fix: a `HORCH_DATA_DIR` variable read in
  `crates/horch-core/src/runtime/paths.rs` (above `XDG_DATA_HOME`, the same
  way the other `HORCH_*` overrides are read; arc_05: only the runtime layer
  reads env). The pane command (`workspace/paneshell.rs`, built by G3 with
  `/usr/bin/env -u ...`) sets `HORCH_DATA_DIR=<data_root>` for fleet and
  spawn panes, so every child process inherits it. Check the Windows form of
  the pane command too.
- Decide whether `HORCH_DATA_DIR` makes the brief's `data_root` redundant.
  Keep one mechanism if it does, and say which in the report.

## FILES

own: `crates/horch-core/src/runtime/paths.rs`, `runtime/context.rs`
(the field only), `crates/horch-core/src/workspace/paneshell.rs`,
`crates/horch/src/cmd/recipes.rs` (pane command only),
`crates/horch/src/cmd/spawn.rs`, `crates/horch-core/src/messaging/brief.rs`,
`crates/horch-core/src/execution/lifecycle.rs`, their tests, the e2e test
G3 added for LA-5 (extend it to the agent and the orchestrator),
`docs/` where `XDG_DATA_HOME` is documented,
`ai_docs/reports/finish/data-dir.md`.

do not touch: `skills/`, `harness/launch.rs`, `skills/activation.rs`,
`execution/plan.rs` (G4 and GW11 work there), `competition/`, `dataset/`.

## CHECKS

Unit tests for the precedence, the e2e (`cargo build --workspace --bins`
first), `arch_scan`, clippy -D warnings, rustfmt on your files. COMMITTED.

## REPORT

`horch tell orchestrator "NOTE: COMMITTED <sha>..."`, then `horch done`.
