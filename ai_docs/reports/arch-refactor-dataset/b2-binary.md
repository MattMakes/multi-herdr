# U29 b2-binary: report

Branch `ard/b2-binary`. Phase B2 (binary part) and B6 (CLI part).
Requirements CMP-01, PRE-06, PRE-07, PRE-12, EXP-06 (CLI path). NFR-10
was covered already. `HORCH_REQUIRE_GIT=1 HORCH_REQUIRE_SQLITE=1 just gate`
is green after every commit. `check-req-coverage.sh --phase B2`: every ID ok.

## Commits

1. `B2: Add the multi-herdr-dataset binary and run preflight end to end`.
   It holds the plan's 2 commits "Add the binary" and "Run preflight end to
   end": the skeleton's `run` has nothing to call without the gatherer.
2. `B6: Add dataset data commands`
3. `B2: Install the dataset binary`
4. `B2: Add CMP-01 and PRE e2e tests`
5. This report.

## CLI surface (`crates/horch/src/dataset/cli.rs`)

```
multi-herdr-dataset run <task> [--candidates N] [--strategy diverse] [--budget-usd X]
                        [--judge auto] [--baseline T] [--promote-to B]
                        [--worktree-root D] [--allow-dirty]
multi-herdr-dataset status [<exp>]
multi-herdr-dataset export [--label-policy V]
multi-herdr-dataset readiness [--policy FILE] [--label-policy V]
multi-herdr-dataset outcome <round> --kind regression|revert|verified [--score S] [--note N]
multi-herdr-dataset rebuild <exp>
hidden: judge-job, promote, rollback, cleanup, watch  -> "not implemented in this build", exit 2
```

- Every `run` flag is optional, so `dataset.yaml` keeps its value when a
  flag is absent (`RunFlags` semantics). `--budget-usd` stays a string and
  goes through `parse_usd_micro`.
- `Cli` is in the library (`horch::dataset::cli`), so tests parse it
  without the binary. The binary (`src/bin/multi-herdr-dataset.rs`) is
  thin: parse, `bootstrap::context`, `dataset::dispatch`.
- Exit codes are in `horch::dataset::exit`: 0 ok, 1 error, 2 not
  implemented, 3 budget/quota refusal, 4 preflight failed, 5 needs
  intervention, 6 rejected. Today only 0, 1, 2 and 4 occur.
- `--score` defaults to 1.0 for `verified`, else 0.0
  (`SPEC-TODO(Spec B §outcome)`).

## The preflight fact gatherer (`dataset/preflight.rs`)

`run` does, in order, with no worktree and no model turn:

1. `config::load(project, flags)`. A relative `worktree_root` is joined to
   the project; the default is `<dataset>/worktrees/<exp>` (absolute, for
   PRE-13).
2. `ExperimentId::mint(now)`, `RoundId::mint(now)` (round index 0).
3. `plan_round` with the layered roster, `Policy::load` and
   `routing::snapshot::obtain(.., allow_probe = false, ..)`. No quota probe
   runs, because a probe could cost a turn.
4. `repo_facts` with `GitCli::new(ctx.bins.harness.git)`: toplevel, HEAD,
   dirty (`status --porcelain`), worktree support (`worktree list`
   succeeds), `git --version`, and every planned branch
   `mh/exp/<exp8>/r0/<label>` that already exists (namespace check).
5. `gather`: `harness_version` (`--version` only; `FORBIDDEN_ENV` stripped
   by `run_short`) for every candidate harness plus the judge harness
   (Claude), redacted. `DatasetPaths::ensure` then `storage_probe`.
   `machine::probe` on the worktree root's nearest existing ancestor, with
   `ctx.settings.machine_file` (this is its first consumer). Herdr
   reachability with `Herdr::with_bin(ctx.bins.harness.herdr)`.
   `horch_exe` when it is a file. Quota pools per candidate.
6. `evaluate`, then `record`: `experiment.created` (task, config, repo and
   environment digests), `preflight.completed{report}`, the manifest
   `experiments/<id>/manifest.json` (`create_immutable`, 0600), and on any
   Fail `experiment.aborted{reason: "preflight failed", failed_checks}`.
   Idempotency keys: `<kind>:<exp>`.
7. On a Fail: print the plan and report, `REFUSED: ...`, exit 4. On a pass:
   print them and "preflight passed. Rounds are not implemented in this
   build.", exit 0.

Size estimates:
- `checkout_bytes`: the files under the toplevel, without `.git`, `target`
  and `node_modules`, links not followed.
- `build_bytes`: the size of `<toplevel>/target` when it exists, else 0.
  `SPEC-TODO(Spec B §3)`.
- `artifacts_bytes`: N × (output cap + log cap × max(1, gates)).
- `local_model_bytes` 0 and `trusted_parents` empty, both `SPEC-TODO(Spec B §3)`.
  With no trusted parents, PRE-13 warns on every run.

## No secret persisted (PRE-07, SEC-02)

- The binary reads only `LANG`, `LC_ALL`, `TERM`, `SHELL`, `TZ` by name.
  The `HORCH_BALANCE`, `HORCH_TEAMMATES_DIR` and `HORCH_*_BIN` values come
  from the context. `envsnap::env_snapshot` filters and redacts them.
- Harness and git versions pass through `redact`. The task in the manifest
  passes through `redact`; the events hold only its digest.
- `pre_07_probe_no_secret_persisted` scans every file under the state dir.

## Data commands

- `status`: fold, then one line per experiment and round with the state,
  the winner label, anomalies and torn lines.
- `export`: `export` + `write_export`. `StoreFacts` adapts
  `ExecutionStore` (the project's ledger) to `ExecutionFactsSource` by
  `record_id == execution_id`.
- `readiness`: the same rows, `ReadinessThresholds::load(--policy)`,
  verdict, arms and gaps.
- `outcome`: refuses unless the round is DECIDED, PROMOTED or COMPLETE
  with a winner and no rejection (`check_round`), then `record_outcome`.
  The refusal exits 1 with the state in the message.
- `rebuild`: the experiment's events, `fold` versus `Projection::apply` one
  event at a time, canonical JSON compared byte for byte. Exit 1 when they
  differ. It prints only; it writes no projection file.

## Install

`horch install` installs `multi-herdr-dataset` from the directory of the
real horch binary (a link is resolved first). When it is not built there,
it prints how to build it and continues. `just install` builds both bins.

## What the B3 coordinator unit must replace

- The end of `dataset::run::run` after the passed-preflight branch
  (marked `// B3:`). The round id and plan already exist there; B3 must
  emit `round.created` and `candidate.planned` from them.
- `StoreFacts::facts_of`: records carry no tokens and no cost, so every
  run exports `Unpriced` with 0 tokens and no transcript ref. The usage
  meter must fill them (in the record or in a new source).
- Exit codes 3, 5 and 6 are defined but never returned.
- The hidden `judge-job`, `promote`, `rollback`, `cleanup`, `watch`
  placeholders (`dataset/mod.rs`).

## Decisions and deviations

- `HarnessKind::binary(&HarnessBins) -> Option<PathBuf>` is new in
  `harness/mod.rs`, on the orchestrator's answer: A4's `arc_10` forbids a
  harness match outside `harness/`. The gatherer calls it for `--version`.
  Unit test: `harness::tests::binary_names_each_harness_cli`.

- `JudgeConfig.policy` is now `evaluation::winner::WinnerPolicy` (default
  `min_confidence 0.7`, `tie_break disabled`). `WinnerPolicy` has no
  `deny_unknown_fields`; I did not change `winner.rs` (not my file).
- A PRE-09 failure exits 4, not 3: it is a preflight failure.
- The plan's file list in the master plan has one module per command for
  `watch`, `judge_job`, `promote`, `rollback`, `cleanup`. I did not create
  empty files; the later units add them.
- `exp_06_outcome_cli_refuses_wrong_state` is in `crates/horch/tests/dataset_cli.rs`.

## Gotchas

- `HORCH_PROJECT_DIR` wins over the current directory. A fleet pane has it
  set to the integration tree, so a manual `run` from another directory
  reads that project. Set `HORCH_PROJECT_DIR` when you test by hand.
- The core `evaluate` still treats PRE-12 as PRE-06 (placeholder from
  b2-preflight). The real PRE-12 guarantee is the e2e test: nothing before
  the refusal creates a worktree or launches a model.
- B5 made every `DatasetPaths` accessor that takes an id return `Result`.
  `paths.experiment_dir`, `paths.manifest` and `paths.default_worktree_root`
  in `dataset/preflight.rs` and `dataset/run.rs` use `?` (rebased on B5).
- The e2e tests find the binary next to the fakes (`bin_dir()`), so
  `cargo build --workspace --bins` must run first, as for every e2e test.

## Tests added: 8

- `crates/horch/tests/dataset_cli.rs`: `cmp_01_cli_args`,
  `exp_06_outcome_cli_refuses_wrong_state`.
- `crates/horch-e2e/tests/dataset.rs`: `pre_06_harness_resolution_before_worktree`,
  `pre_07_probe_no_secret_persisted`, `pre_12_e2e_refuses_before_worktree_or_model`.
- `crates/horch-core/src/harness/mod.rs`: `binary_names_each_harness_cli`.
- `crates/horch/src/cmd/install.rs`: `install_puts_the_dataset_binary_next_to_horch`,
  `dataset_sibling_follows_a_link_to_horch`.

## Outside my scope (not fixed)

- `crates/horch-core/tests/execution_store.rs:15` has an unused import
  warning (`KIND_ORCHESTRATOR`).
- LA-1 (`just install` installs both binaries; `horch doctor` passes) and
  LA-6 (preflight numbers match Activity Monitor) need the operator.
