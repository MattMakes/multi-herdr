# Architecture: what lives where

This page tells you what the two programs do and which file to open for each
concern. For how the commands connect at run time, see
[command-flow.md](command-flow.md).

## Two programs over one core

| binary | entry file | what it does |
|---|---|---|
| `horch` | `crates/horch/src/main.rs` | Runs a fleet: 1 orchestrator pane that spawns worker panes, a session ledger, messaging, tiling, cost and telemetry |
| `multi-herdr-dataset` | `crates/horch/src/bin/multi-herdr-dataset.rs` | Runs a competition round: N candidates on 1 task, each in its own git worktree, then a judge; records every step as an event |

Both binaries are thin. They parse flags, build a `RuntimeContext`, call
`horch-core`, and print the result. Both create executions through the same
spawn service and write them to the same execution ledger.

The workspace has 6 crates:

| crate | holds |
|---|---|
| `crates/horch-core` | The domain: roster, harnesses, routing, executions, messaging, workspace, telemetry, the dataset mode |
| `crates/horch` | The `horch` CLI and the `multi-herdr-dataset` binary |
| `crates/horch-marketplace` | Skill sources, installs and the lockfile |
| `crates/horch-e2e` | Fake harness binaries and the hermetic end-to-end tests |
| `crates/herdr-install` | The Herdr CLI installer |
| `crates/herdr-docs-sync` | The mirror of the herdr docs into `herdr-docs/` |

## The rules the tests enforce

`crates/horch-core/tests/arch_scan.rs` scans the source. A junior can break
these rules by accident:

| test | rule | what to do instead |
|---|---|---|
| `arc_05_no_ambient_env_in_core` | No `std::env` read in `horch-core` outside `runtime/` | Read the value in `runtime/` into `RuntimeContext` (`crates/horch/src/bootstrap.rs` builds it once) and pass it down |
| `arc_06_env_mutation_sites_reduced` | No `set_var(` or `remove_var(` in `horch-core` or `horch` | Put the variable on the child `Command` |
| `arc_22_no_error_string_matching` | No branch on an error's message text | Match a typed error (`downcast_ref`); pick an exit code from `crates/horch/src/exit.rs` by type |
| `arc_10_harness_match_only_in_harness` | Outside tests, `HarnessKind` variants are matched only under `harness/` and in `roster/validation.rs` | Add a field to `Capabilities` in `harness/capabilities.rs` and read it |
| `arc_25_no_shim_modules` | No file that only re-exports; each `pub use` names one of the file's own child modules | Import an item from the module that owns it; make an item `pub(crate)` when nothing outside the crate uses it |

## horch-core modules

`crates/horch-core/src/lib.rs` lists the modules. "Start at" is the file to
open first.

| module | owns | start at |
|---|---|---|
| `ids` | typed identities (`ExecutionId`, `RoleName`, `PaneId`, ...) | `crates/horch-core/src/ids.rs` |
| `clock` | the one clock horch reads (`HORCH_NOW` pins it) | `crates/horch-core/src/clock.rs` |
| `fsx` | durable file writes and the cross-process directory lock (`DirLock`) | `crates/horch-core/src/fsx.rs` |
| `runtime` | the process boundary: `RuntimeContext`, paths, binary overrides (`HORCH_*_BIN`), fault points | `crates/horch-core/src/runtime/context.rs`, `crates/horch-core/src/runtime/paths.rs`, `crates/horch-core/src/runtime/bins.rs` |
| `roster` | `teammates/` files: parsing, layering, `--check` rules, effort and permission mapping | `crates/horch-core/src/roster/repository.rs`, `crates/horch-core/src/roster/validation.rs` |
| `prompts` | rendering briefings from `teammates/` | `crates/horch-core/src/prompts.rs` |
| `skills` | skill catalogs, phase selection, activation plans, launch bundles | `crates/horch-core/src/skills/selection.rs`, `crates/horch-core/src/skills/catalog.rs` |
| `harness` | 1 adapter per agent CLI (claude, codex, OpenCode, pi, Prime, Antigravity), capabilities, the launch flow, the `agent-list` inventory, the headless judge command | `crates/horch-core/src/harness/mod.rs`, `crates/horch-core/src/harness/launch.rs` |
| `routing` | quota-aware routing: policy, quota pools, the spawn decision | `crates/horch-core/src/routing/decision.rs`, `crates/horch-core/src/routing/quota.rs` |
| `execution` | executions: model, ledger record and store, spawn plan, the spawn service, lifecycle (`done`) | `crates/horch-core/src/execution/service.rs`, `crates/horch-core/src/execution/store.rs` |
| `messaging` | the worker brief, the mailbox and message delivery | `crates/horch-core/src/messaging/brief.rs`, `crates/horch-core/src/messaging/mailbox.rs` |
| `workspace` | the herdr client, layout, tiling and balancing | `crates/horch-core/src/workspace/herdr.rs`, `crates/horch-core/src/workspace/tile.rs` |
| `telemetry` | live per-pane token telemetry and the collector | `crates/horch-core/src/telemetry/collect.rs`, `crates/horch-core/src/telemetry/readers.rs` |
| `usage` | what a run cost, read back from each harness's own records | `crates/horch-core/src/usage.rs` |
| `vcs` | typed git and worktrees for the dataset mode | `crates/horch-core/src/vcs/git.rs` |
| `measure` | dataset digests, events, the event store and projections | `crates/horch-core/src/measure/event.rs`, `crates/horch-core/src/measure/projection.rs` |
| `competition` | competitive mode: config, preflight, planner, round state table, coordinator, judging, promotion, budget | `crates/horch-core/src/competition/coordinator.rs`, `crates/horch-core/src/competition/state.rs` |
| `evaluation` | validation gates, judge input, judging and winner policy | `crates/horch-core/src/evaluation/validator.rs`, `crates/horch-core/src/evaluation/winner.rs` |
| `dataset` | the dataset outputs: export rows and readiness | `crates/horch-core/src/dataset/export.rs` |
| `teacher` | the System One decision-model seam (inert) | `crates/horch-core/src/teacher/mod.rs` |

The harness kinds are in `harness/mod.rs` (`HarnessKind`): `claude`, `codex`,
`opencode`, `pi`, `prime`, `antigravity`, and `none` (the smoke fake). Each
real kind has one adapter file in `harness/` and one `Capabilities` constant
in `harness/capabilities.rs`.

## The CLI layer

`crates/horch/src/cmd/` holds 1 file per `horch` command group:

| file | command |
|---|---|
| `agentlist.rs` | `horch agent-list` |
| `balancecmd.rs` | `horch balance` |
| `cost.rs` | `horch cost` |
| `doctor.rs` | `horch doctor` |
| `install.rs` | `horch install` (copies `horch` and `multi-herdr-dataset`) |
| `layoutcmd.rs` | `horch layout` |
| `ledgercmd.rs` | `horch sessions`, `horch ledger` |
| `marketplacecmd.rs` | `horch marketplace` |
| `messaging.rs` | `horch tell`, `inbox`, `assign`, `note`, `done` |
| `mod.rs` | the module list and `load_roster` |
| `quotacmd.rs` | `horch quota` |
| `recipes.rs` | `horch fleet`, `horch orchestration` |
| `route.rs` | `horch route` |
| `skillscmd.rs` | `horch skills` |
| `smoke.rs` | `horch smoke messaging`, `fleet`, `tile` |
| `spawn.rs` | `horch spawn` |
| `teammatescmd.rs` | `horch teammates` |
| `telemetry.rs` | `horch telemetry` |
| `tilecmd.rs` | `horch tile` |
| `usagecmd.rs` | `horch usage` |
| `worker.rs` | `horch worker` (hidden; runs inside a spawned pane) |

`crates/horch/src/dataset/` holds the `multi-herdr-dataset` commands:

| file | command |
|---|---|
| `cli.rs` | the clap definition of every subcommand |
| `mod.rs` | `dispatch` and the exit codes |
| `run.rs` | `run` and `resume`: config, preflight, the coordinator loop |
| `preflight.rs` | gathers the preflight facts (git, machine, harness `--version`) |
| `status.rs` | `status` |
| `export.rs` | `export` |
| `readiness.rs` | `readiness` |
| `outcome.rs` | `outcome` |
| `rebuild.rs` | `rebuild` |
| `judge_job.rs` | `judge-job` (hidden; the detached judge run) |
| `promote.rs` | `promote` (hidden) |
| `rollback.rs` | `rollback` (hidden) |
| `cleanup.rs` | `cleanup` (hidden) |
| `watch.rs` | `watch` (hidden; the root pane of the dataset workspace) |

Shared files: `crates/horch/src/bootstrap.rs` (the one read of the process
environment), `crates/horch/src/exit.rs` (exit codes), `crates/horch/src/output.rs`
(stdout without a panic on a closed pipe).

## Where state lives

| what | path | source |
|---|---|---|
| state root | `$HORCH_STATE_DIR`, else `${XDG_STATE_HOME:-~/.local/state}/horch` | `runtime/paths.rs` `state_root` |
| execution ledger | `<state root>/<slug of the project path>.json` | `execution/store.rs` `ExecutionStore::for_project` |
| dataset root | `<state root>/multi-herdr/<project slug>/` | `measure/paths.rs` (`DATASET_DIR`) |
| dataset events | `<dataset root>/events/YYYY-MM-DD.jsonl`, 1 file per UTC day | `measure/paths.rs` `events_file` |
| skill bundles per launch | `<state root>/skill-bundles/<execution id>/` | `skills.rs`, `harness/launch.rs` |
| marketplace skills (data root) | `$HORCH_DATA_DIR`, else `${XDG_DATA_HOME:-~/.local/share}/horch/` | `runtime/paths.rs` `data_root` |

`horch ledger path` prints the ledger path for the current project.

A herdr pane does not inherit the environment of the `horch` that split it.
So every horch process in a fleet gets the spawner's data root this way:

- The orchestrator pane command sets `HORCH_DATA_DIR` (`cmd/recipes.rs`
  `pane_command_for`, through `PaneShell::command_line_with_env`).
  `pane-launch` and its agent inherit it.
- Every other pane command sets `HORCH_DATA_DIR` the same way (`execution/service.rs`
  `run`, through `PaneShell::command_line_with_env`). The brief carries no
  `data_root` field. `horch worker` and its agent inherit the variable. See
  [command-flow.md](command-flow.md), section 2.

## How teammates and skills get into the binary

`crates/horch-core/build.rs` globs `teammates/` (every file without a leading
`_`, plus `teammates/_base/` and `teammates/_template.md`) and every file
under `skills/`, and compiles them in. Adding a file adds a teammate or a
skill. There is no Rust list to edit. An installed `horch` works without a
checkout.

At run time the roster is layered (`load_roster` in `crates/horch/src/cmd/mod.rs`):

1. the compiled-in files;
2. `~/.config/horch/teammates`;
3. `$HORCH_TEAMMATES_DIR`;
4. an explicit directory. A pane is a fresh shell that does not inherit
   `$HORCH_TEAMMATES_DIR`, so `horch fleet` passes the path to the pane
   outright (`roster/repository.rs` `load_layered`).

A later layer overrides an earlier one for the same teammate name. The `justfile` exports
`HORCH_TEAMMATES_DIR` to the repo's `teammates/`, so `just` recipes always use
your edits.

## Read next

- [command-flow.md](command-flow.md): the commands as diagrams.
- [testing-and-gates.md](testing-and-gates.md): the gate, oracles and fakes.
- The recipes in [README.md](README.md#recipes).
- The specs: [specs/architecture.md](specs/architecture.md) and
  [specs/dataset-competition.md](specs/dataset-competition.md).
