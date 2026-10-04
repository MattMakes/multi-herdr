# G7 data-dir report

Plan: `ai_docs/plans/finish/g7-data-dir.md`. Worker opus-85, 2026-10-04.

## Outcome

Every horch process in a fleet now resolves the skill store (data root) of
the `horch fleet` or `horch spawn` that created it.

| Process | How it gets the store | Test |
|---|---|---|
| orchestrator pane (`horch pane-launch`) | pane command sets `HORCH_DATA_DIR` | `recipes::tests::pane_command_sets_the_spawners_data_dir`, e2e below |
| orchestrator's agent and its `horch` commands | inherits the pane-launch environment | e2e below (pane command run in a shell without `XDG_DATA_HOME`) |
| worker wrapper (`horch worker`) | brief `data_root` (G3, unchanged) | e2e below |
| worker's agent and its `horch` commands | `Brief::transport_env` sets `HORCH_DATA_DIR` | `brief::tests::transport_env_carries_identity_paths_and_every_override`, e2e below |

Code commit: `bd1e224`.

## Changes

- `runtime/paths.rs`: `data_root` reads `HORCH_DATA_DIR` first, then
  `XDG_DATA_HOME`, then `$HOME/.local/share`. `HORCH_DATA_DIR` names the
  store itself: horch does not append `horch`. An empty value counts as
  unset. Test: `data_root_prefers_horch_data_dir_then_xdg_then_home`.
- `workspace/paneshell.rs`: new `PaneShell::command_line_with_env`.
  `command_line` calls it with no variables, so all other callers are
  unchanged.
  - Posix: `/usr/bin/env -u ANTHROPIC_API_KEY 'HORCH_DATA_DIR=<root>' '<horch>' ...`.
  - PowerShell: `Remove-Item Env:ANTHROPIC_API_KEY ...; $env:HORCH_DATA_DIR = '<root>'; & '<horch>' ...`.
  - Test: `command_line_with_env_sets_each_value_for_the_binary` (both
    dialects, with an embedded `'`).
- `cmd/recipes.rs` `pane_command_for`: the orchestrator pane command (and
  the fixed orchestration recipe panes) always set
  `HORCH_DATA_DIR=<ctx.paths.data_root>`.
- `messaging/brief.rs` `transport_env`: `HORCH_DATA_DIR=<brief.data_root>`
  when the brief has it. A schema 1 brief has no `data_root`, so the agent
  gets no `HORCH_DATA_DIR`, and keeps the worker's own store.
- `crates/horch-e2e/tests/skills_exposure.rs`
  `worker_reads_the_skill_store_its_spawner_read` (the G3 LA-5 test) now
  also checks:
  - the agent (a probe in front of fake claude) has `HORCH_DATA_DIR=<store>`
    and no `XDG_DATA_HOME`;
  - `horch marketplace list --json` with only that `HORCH_DATA_DIR` lists
    the store and the `demo` skill;
  - the `horch fleet` pane-launch line holds `'HORCH_DATA_DIR=<store>'`, and
    run in `/bin/sh` without `XDG_DATA_HOME` the probe in place of horch
    sees `HORCH_DATA_DIR=<store>`.
- `docs/architecture.md`: the data root row, and how panes get it.

## Decision: brief `data_root` stays

`HORCH_DATA_DIR` does not make the brief's `data_root` redundant today. The
worker pane command is built in `execution/service.rs` `run`, which is not
in my file list. So each pane type has one mechanism:

- orchestrator pane: the pane command (`HORCH_DATA_DIR`), because
  `pane-launch` has no brief;
- worker pane: the brief (`data_root`), the same channel as `state_dir` and
  `teammates_dir`. The worker passes it on as `HORCH_DATA_DIR`.

To remove the brief field later: make `service.rs` `run` use
`command_line_with_env` with `HORCH_DATA_DIR`, then drop the `lifecycle.rs`
`enter_context` lines and the brief field. Keep reading the field for old
briefs, or accept that an old brief keeps the worker's own store.

## Checks

- `cargo build --workspace --bins`: pass.
- e2e `lifecycle` 13 of 13, `e2e` 25 of 25, `skills_exposure` 17 of 17: pass.
- `arch_scan` 13 of 13: pass.
- `horch-core --lib`: 433 pass with `--skip quota_probe`. The 3
  `quota_probe` tests pass alone. `a_crashing_harness_is_broken_until_it_recovers`
  failed 1 time in a full parallel run under load (not my file).
- `horch` bin unit tests 75 of 75: pass.
- `cargo clippy -p horch-core -p horch -p horch-e2e --all-targets -- -D warnings`: pass.
- `rustfmt --check` on my files: pass.

## Not done / outside scope

- The worker test is now `#[cfg(unix)]`: the probes are `/bin/sh` scripts.
  Before, the G3 part also ran on Windows.
- Other pane commands (`dataset/run.rs` watch pane, `cmd/telemetry.rs`,
  `cmd/smoke.rs`) do not set `HORCH_DATA_DIR`. They do not read skills
  today.
- `docs/command-flow.md` documents the state dir only. It has another
  worker's uncommitted edits, so I did not add the data root there.
- `routing::quota_probe` `a_crashing_harness_is_broken_until_it_recovers`
  is flaky under a loaded parallel run.
