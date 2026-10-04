# G3 worker-env report

Plan: `ai_docs/plans/finish/g3-worker-env.md`. Worker opus-71, 2026-10-04.

## Outcome

| Goal | Commit | Tests |
|---|---|---|
| 1. No pane process holds a `FORBIDDEN_ENV` name | `48c7a13` | `paneshell::tests::every_command_line_first_removes_every_forbidden_name`, the 2 changed exact-line tests, e2e `lifecycle::pane_command_removes_a_forbidden_key_before_horch_starts` |
| 2. The worker reads the skill store its spawner read | `8873d0d` | `brief::tests::arc_07_brief_v1_readable`, `brief::tests::a_v2_brief_round_trips_and_keeps_the_v1_keys`, e2e `skills_exposure::worker_reads_the_skill_store_its_spawner_read` |
| 3. Every roster load prints the F7 warnings | see `git log -- ai_docs/reports/finish/worker-env.md` | `judge_job::tests::roster_warnings_are_printed`, e2e `lifecycle::worker_warns_once_about_a_teammate_file_that_does_not_load` |

## 1. `env -u` in every pane command

`PaneShell::command_line` (`crates/horch-core/src/workspace/paneshell.rs`)
now starts every line with the removal of each `FORBIDDEN_ENV` name:

- Posix: `/usr/bin/env -u ANTHROPIC_API_KEY '<horch>' ...`. The path is
  absolute because a pane's `PATH` is not the spawner's `PATH` (the e2e
  harness `PATH` holds only the fakes). BSD, GNU and busybox `env` take `-u`.
- PowerShell: `Remove-Item Env:ANTHROPIC_API_KEY -ErrorAction SilentlyContinue; & '<horch>' ...`.
  `Env:` is the PowerShell process's own environment, so the pane shell
  also loses the variable.

All callers go through this one function, so all get the prefix:

- worker panes: `execution/service.rs` `run` (`horch spawn`, and the
  dataset candidate panes, which the coordinator starts through the spawn
  service);
- the orchestrator pane: `cmd/recipes.rs` `pane_command_for`;
- the dataset watch pane: `dataset/run.rs`;
- the telemetry collector pane: `cmd/telemetry.rs`;
- the smoke panes: `cmd/smoke.rs`.

No file outside my list changed for this: `coordinator.rs` needed no edit.

The e2e test takes the line that `horch spawn` gave to `herdr pane run`. It
puts a probe in place of the horch binary and runs the line in `/bin/sh -c`
with `ANTHROPIC_API_KEY=SENTINEL` in the shell environment. The probe sees
`HORCH_FAKE_LOG` but not the key.

Limit: on Posix the pane shell itself still holds the key (herdr gives it
to the shell). Every process horch starts in the pane does not.

## 2. `data_root` in the brief

- `Brief.data_root: Option<String>` (`messaging/brief.rs`), serde default,
  omitted when `None`. Older briefs read as `None`, and the worker keeps its
  own store.
- `horch spawn` writes `ctx.paths.data_root` (`execution/service.rs`).
- `horch worker` sets `ctx.paths.data_root` from the brief in
  `enter_context` (`execution/lifecycle.rs`), before the launch installs
  skills.
- The e2e test installs a skill from a local bare repo into
  `XDG_DATA_HOME=<root>/other-data`, spawns a teammate that names it, and
  runs the worker without `XDG_DATA_HOME`. The agent sees `demo` and `tdd`.
  Without the fix the worker fails with
  `teammate 'g3-market': unknown bundled skill 'demo'`, the LA-5 error.
- With orchestrator approval I added `data_root: None` to the test struct
  literals in `messaging/mailbox.rs` and `tests/execution_plan.rs`.

## 3. Roster warnings

- `execution/lifecycle.rs` `launch`: the worker prints each
  `Roster::load_warnings` line as `warning: <line>` to stderr, in its pane.
  A worker loads the roster once.
- `dataset/judge_job.rs` `run_judge`: the same, through a small `warn`
  helper. A judge job loads the roster once.

## Outside scope (not fixed)

- The orchestrator pane (`horch fleet`) and a worker's agent do not get
  `data_root`. A `horch spawn` run by an agent in a pane without
  `XDG_DATA_HOME` uses the default store. A fix needs a `--data-dir` flag on
  `pane-launch` (`crates/horch/src/main.rs`) or a `HORCH_DATA_DIR` variable
  in `runtime/paths.rs`, and then a transport-env entry.
- fake-herdr runs pane commands with `cmd /C` on Windows. The PowerShell
  form (also the `&` form before this change) does not run there, so the
  Windows `exec` e2e path does not match real herdr panes.
- Commit `8873d0d` also contains about 110 lines of another worker's
  uncommitted plugin-skill tests in `crates/horch-e2e/tests/skills_exposure.rs`.
  I reported this to the orchestrator. The tests pass.
