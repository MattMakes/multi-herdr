# G3 worker-env: no key in any pane process; one skill store for spawner and worker; roster warnings everywhere

Single-branch work: follow `ai_docs/plans/finish/01-single-branch.md`.
Project rule (CLAUDE.md): never read, print, set or pass the value of
`ANTHROPIC_API_KEY`. Removing it from an environment is the point here.
Never set it, even to a sentinel, in a test that runs a real `claude`.

## GOAL

1. No process in a horch pane holds `ANTHROPIC_API_KEY` (or any
   `FORBIDDEN_ENV` name): the pane command itself starts with `env -u NAME`
   for each name, for worker panes, the orchestrator pane and dataset
   candidate panes.
2. A worker always reads the skill store its spawner read: the brief (or the
   pane command) carries `data_root`, as it already carries `teammates_dir`.
3. Every roster load prints the F7 warnings once: `execution/lifecycle.rs:321`
   and `crates/horch/src/dataset/judge_job.rs:125` call `Roster::load_layered`
   and print nothing.

## CONTEXT

- (1) Finding F5 of `ai_docs/reports/finish/acceptance-dataset.md`: the herdr
  server carries the key from the operator's shell; every pane shell inherits
  it; the `horch worker` wrapper removes it before the agent starts, so no
  agent sees it, but the wrapper process does. `FORBIDDEN_ENV` is in
  `crates/horch-core/src/harness/launch.rs:119`. Pane command lines are
  built by `crates/horch-core/src/workspace/paneshell.rs` (find every
  caller: `cmd/recipes.rs` `pane_command_for`, spawn, the dataset candidate
  launch). If the candidate pane command is built inside
  `competition/coordinator.rs`, do not edit it: tell the orchestrator the
  exact change and continue; opus-61 owns that file. Prefer one change in
  paneshell so every caller gets it.
  Windows: horch builds pane commands for Windows too (paneshell has
  PowerShell/cmd forms?) — check, and do the right thing per shell.
- (2) L2 gotcha in `ai_docs/reports/finish/acceptance-fleet.md` (LA-5):
  a pane that `horch spawn` splits does not inherit the spawner's env, and
  herdr does not apply workspace `--env` to split panes, so with
  `XDG_DATA_HOME` set the worker re-resolved skills in the default store and
  failed "unknown bundled skill 'whats-next'". `teammates_dir` already
  travels in the brief (`messaging/brief.rs`); do the same for `data_root`
  and make the worker use it.
- (3) `ai_docs/reports/finish/roster-resilience.md`, "Outside scope".
  `crates/horch/src/dataset/run.rs` is G1's: do not touch it.

## FILES

own: `crates/horch-core/src/workspace/paneshell.rs`,
`crates/horch/src/cmd/recipes.rs` (pane command only),
`crates/horch/src/cmd/spawn.rs`, `crates/horch-core/src/messaging/brief.rs`,
`crates/horch-core/src/execution/lifecycle.rs`,
`crates/horch-core/src/execution/service.rs`,
`crates/horch/src/dataset/judge_job.rs`, the e2e tests for these
(`crates/horch-e2e/tests/`; the fakes already flag the key:
`fake-claude.rs`), `ai_docs/reports/finish/worker-env.md`.

do not touch: `competition/coordinator.rs`, `competition/observe.rs`,
`crates/horch/src/dataset/` other files, `execution/plan.rs`,
`skills/activation.rs`, `harness/claude_plugins.rs`, `messaging/mailbox.rs`,
`crates/horch/src/cmd/messaging.rs`.

## STEPS

1. Pane `env -u` + unit test of the command line + an e2e check that the
   pane process (the fake shell) does not see the key. COMMITTED.
2. `data_root` in the brief; worker uses it; e2e with a non-default store.
   COMMITTED.
3. Roster warnings at the 2 call sites; tests. COMMITTED.
4. Targeted checks per step (the tests you changed, `cargo build --workspace
   --bins` before any e2e, clippy -D warnings on the crates you touched,
   rustfmt on your files).

## DONE WHEN

The 3 goals have tests; the report lists them with commits.

## REPORT

`horch tell orchestrator "NOTE: COMMITTED <sha>..."` per step, then
`horch done` with the summary.
