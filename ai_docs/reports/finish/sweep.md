# G8 sweep report

Plan: `ai_docs/plans/finish/g8-sweep.md`. Worker opus-88, 2026-10-04.

## Outcome

Every pane horch starts sets `HORCH_DATA_DIR`. The brief no longer has
`data_root`. `docs/command-flow.md` names the data root and what each pane
command carries. The `quota_probe` crash test waits for its fake to exit,
and 20 of 20 loaded runs pass.

Commits:

- `9926f46`: goal 4, the `quota_probe` test.
- `8277d59`: goals 1 and 2. `execution/service.rs` holds both goals, so they
  are 1 commit.
- `74e7362`: goal 3, `docs/command-flow.md`.

## Goal 1: `HORCH_DATA_DIR` in every pane

These callers of `PaneShell::command_line` now use `command_line_with_env`
with `HORCH_DATA_DIR=<ctx.paths.data_root>`:

| Pane | File |
|---|---|
| worker (and dataset candidate) `horch worker <role>` | `crates/horch-core/src/execution/service.rs` `run` |
| dataset round root pane `watch` | `crates/horch/src/dataset/run.rs` `coordinate` |
| telemetry collector | `crates/horch/src/cmd/telemetry.rs` |
| smoke `register` panes (3 calls) | `crates/horch/src/cmd/smoke.rs`, new `horch_line` helper |

The orchestrator pane (`cmd/recipes.rs`) already did it (G7). After this
change no caller of `command_line` remains outside `paneshell.rs` tests.

Dataset candidates start through `ExecutionService`, so `service.rs` covers
them. The dataset judge is not a pane: `judge-job` is a detached child of
the coordinator and inherits its environment.

## Goal 2: the brief's `data_root` is gone

- `messaging/brief.rs`: the field and the `transport_env` line are removed.
  The worker's agent inherits `HORCH_DATA_DIR` from the worker process,
  because the agent command only removes `FORBIDDEN_ENV`.
- `execution/lifecycle.rs` `enter_context`: the reader is removed.
- `execution/service.rs`: the brief writer no longer sets it.
- An old brief with a `data_root` key still parses: `Brief` does not deny
  unknown fields. That worker keeps the store of its own environment.
- Struct literals: `messaging/mailbox.rs` (test) and
  `crates/horch-core/tests/execution_plan.rs` lose 1 line each. These 2
  files are not in the plan's list. The field removal does not compile
  without this change.
- Brief tests: a written brief has no `data_root` key, and `transport_env`
  has no `HORCH_DATA_DIR`.
- e2e `skills_exposure` `worker_reads_the_skill_store_its_spawner_read`:
  the brief has no `data_root`. The test finds the worker pane command in
  the fake herdr log and checks for `'HORCH_DATA_DIR=<store>'`. Then it runs
  that exact line in `/bin/sh`, with no `XDG_DATA_HOME`. The worker finds
  the skill, and the agent sees `HORCH_DATA_DIR=<store>`. I checked that
  the test reaches this part (git present, no early return).

## Goal 3: `docs/command-flow.md`

- Section 1: the data root (`$HORCH_DATA_DIR`, else
  `${XDG_DATA_HOME:-~/.local/share}/horch`).
- Section 2, new "What a pane command carries": what every pane command
  removes and sets, a table of the 5 pane kinds and their builders, the
  judge note, and how the state dir travels.

## Goal 4: `quota_probe` `a_crashing_harness_is_broken_until_it_recovers`

Timing assumption: `probe_version` gives `NoAnswer` when the child does not
exit within `VERSION_TIMEOUT` (15 s of wall clock). The test needs an
answer from its fake, so a starved fake fails the test.

Fix: `probe_version_within(bin, timeout)` and a `timeout` parameter on
`probe_harnesses`. Production passes `VERSION_TIMEOUT`, so behaviour does
not change. The test passes 600 s. The fake always exits, so a passing run
does not wait longer.

Other causes I checked and excluded:

- ETXTBSY: macOS runs a script that another process has open for writing.
- Process limit: 1269 of 10666 processes in use.
- Shared temp cleanup, `waitpid(-1)`, `set_current_dir`, `set_var`: no
  horch-core code does these.

I could not reproduce the original failure. Before the fix: 18 of 18 full
`horch-core --lib` runs passed (6 without load, 12 with a workspace build
in parallel).

Result after the fix, with `cargo test -p horch -j 16` in a loop in
parallel for the whole time:

- full `horch-core --lib` at `--test-threads 64`: 20 of 20 pass;
- the test alone: 20 of 20 pass.

## Checks

On HEAD `74e7362`, in a scratch copy:

- `cargo build --workspace --bins`: pass.
- e2e `lifecycle` 13 of 13, `skills_exposure` 17 of 17: pass.
- `horch-core --lib` 438 of 438, `execution_plan` 15 of 15, `arch_scan`
  13 of 13, `horch` bin 75 of 75: pass.
- `cargo clippy -p horch-core -p horch -p horch-e2e --all-targets -- -D warnings`: pass.
- `rustfmt --check` on my files: pass.

Before the goal 1 and 2 commit (HEAD plus my changes): e2e `dataset` 25 of
25, `e2e` 25 of 25, `brief` 1 of 1, `messaging` 2 of 2: pass. On the shared
tip `74e7362`: e2e `dataset` 25 of 25: pass.

I ran the checks in `.worktrees/_scratch/g8` (a `git archive HEAD` copy).
The shared checkout did not compile for most of this work, because of
uncommitted `build.rs` / `catalog.rs` / `skillscmd.rs` edits that are not
mine.

## Seen, outside scope

- `fsx::tests::dirlock_paused_breaker_blocks_other_breakers` failed 1 time
  in 6 full `horch-core --lib` runs at `--test-threads 64`
  (`crates/horch-core/src/fsx.rs:826`).
- The set-aside provenance edits came back at 2026-10-04 during this work:
  `crates/horch-core/build.rs` and `tests/skills_catalog.rs` are modified,
  and the index holds staged deletions of `skills/provenance.json`, about
  110 `skills/*/LICENSE` files and `crates/horch-e2e/tests/routing_provenance.rs`.
  I told the orchestrator.
