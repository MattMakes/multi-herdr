# G8 sweep: one data-root mechanism, the remaining pane commands, a flaky test

Single-branch work: follow `ai_docs/plans/finish/01-single-branch.md`.

## GOAL

1. Every pane horch starts sets `HORCH_DATA_DIR` (G7 did the orchestrator
   pane, `cmd/recipes.rs`): also the worker pane (`execution/service.rs`),
   the dataset candidate and judge panes, the telemetry pane and the smoke
   panes. Find every caller of `PaneShell::command_line` /
   `command_line_with_env`; each one sets it.
2. Then the brief's `data_root` field (G3, `8873d0d`) is redundant: remove
   it and its reader, as `ai_docs/reports/finish/data-dir.md` describes, and
   keep the G3/G7 e2e tests passing (they prove the worker and its agent
   read the spawner's store).
3. `docs/command-flow.md` names the data root and `HORCH_DATA_DIR` where it
   describes pane commands (G7 could not edit it then).
4. `routing::quota_probe` test `a_crashing_harness_is_broken_until_it_recovers`
   failed once under a loaded parallel run (G7 report). Find the timing
   assumption and make it hold under load (no sleeps that race; use the
   injected clock or a wait-for). Run it 20 times in a loop under load
   (`cargo test` of another crate in parallel) and record the result.

## FILES

own: `crates/horch-core/src/execution/service.rs`,
`crates/horch-core/src/messaging/brief.rs`, `execution/lifecycle.rs`,
`crates/horch-core/src/workspace/paneshell.rs`, the dataset and telemetry
pane command builders (find them; `crates/horch/src/dataset/`,
`crates/horch/src/cmd/telemetry*`, smoke), `crates/horch-core/src/routing/quota_probe.rs`
(the test), `docs/command-flow.md`, the e2e tests for these,
`ai_docs/reports/finish/sweep.md`.
do not touch: `skills/`, `teammates/`, `scripts/godot/`.

## CHECKS

The tests you touch, `cargo build --workspace --bins` then the e2e
lifecycle, skills_exposure and dataset tests, `arch_scan`, clippy -D
warnings, rustfmt on your files. COMMITTED per goal.

## REPORT

`horch tell orchestrator "NOTE: COMMITTED <sha>..."`, then `horch done`.
