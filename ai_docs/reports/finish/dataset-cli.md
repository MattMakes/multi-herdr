# G1 dataset-cli: report

Plan: `ai_docs/plans/finish/g1-dataset-cli.md`. Findings: F3, F6, F7 and F8
of `ai_docs/reports/finish/acceptance-dataset.md`, plus F7 of
`ai_docs/reports/finish/roster-resilience.md`.

## Findings and commits

| finding | change | test or doc | commit |
|---|---|---|---|
| F3 target repo | `dataset::resolve_project`: `--project <dir>` (global flag), then the git top level of the cwd, then `$HORCH_PROJECT_DIR`, then the cwd. `judge-job` keeps `$HORCH_PROJECT_DIR`. `run`, `resume`, `status`, `promote`, `rollback`, `cleanup` print `target: <repo> @ <HEAD 12>` first. | `f3_project_precedence_and_target_line` (`crates/horch/tests/dataset_cli.rs`) | 639c1e7 |
| F6 status state | `status::experiment_state`: an experiment with a round shows its latest round's state. Rule added to design §5. | `f6_status_shows_the_experiment_at_its_round_state` | 639c1e7 |
| F7 empty dirs | `cleanup::remove_empty_dirs`: non-recursive `remove_dir` of `<root>/<exp>/_promote` and `<root>/<exp>`, only for a dir named for the experiment. Called by `cleanup` and by `run`/`resume`/`promote` when the round is COMPLETE. | `cleanup_removes_only_empty_round_dirs` (lib), `cmp_04_n_worktrees_same_base_modify_same_file` (e2e) | 639c1e7 |
| F8 docs | `docs/command-flow.md`: "Which repo a dataset command targets", "Where the candidate worktrees go". Design §2.1 paragraph. | doc text | 639c1e7 |
| G2 live meter | `UsageMeter.estimates` (by model): `projected(model)` uses it, else the default. `run.rs` `meter_estimates` resolves each candidate as PRE-09 does (`resolve_estimate`: plan per-label, config model, config all, measured, default). | `cmp_10_configured_estimate_keeps_a_later_wave_running` (e2e; fails without the fix: C never starts), `meter_estimates_follow_pre_09_per_model` (lib) | 0d4776c |
| G2 docs | `docs/dataset-config.md`: every `dataset.yaml` key with its default, linked from `docs/README.md` and `docs/command-flow.md`. | doc text | 0d4776c |
| gate | `cmp_07_timeout` and `mea_10_every_spawn_has_terminal_event`: `candidate_deadline_s` 6 s to 20 s (the finishing candidate timed out under gate load). | the 2 tests | 0d4776c |
| roster F7 | `run.rs` `load_roster` prints each `Roster::load_warnings` line once to stderr. | `roster_warnings_print_once` (lib) | ae4458c |

## Decisions

- Path spelling: `git rev-parse --show-toplevel` and `getcwd` resolve
  symlinks (`/var` becomes `/private/var` on macOS). The ledger and the
  dataset dir are keyed on the path text. `spelled_as` keeps the
  `$HORCH_PROJECT_DIR` spelling when it names the same repo, so the
  execution ledger slug does not change.
- `watch --project` is now the global `--project` flag. The pane command
  in `run.rs` is unchanged and still parses.
- `status`, `render` and `experiment_state` are `pub` for the test.
- Meter key: the coordinator calls `meter.projected(model)` in
  `committed` (`coordinator.rs`, not mine), so the map is keyed on the
  model, not the label. 2 labels of 1 model with different estimates get the
  per-kind maximum. The CLI leaves the plan's per-label estimates empty, so
  today every estimate is per model anyway.
- `budget.rs` was granted by the orchestrator. The new field breaks 1
  struct literal in a test of `competition/preflight.rs`
  (`pre_09_projection_matches_the_live_budget`); I added
  `..UsageMeter::default()` there (1 line).
- `docs/dataset-config.md` states 2 facts that the code shows: `gates[].required`
  is parsed but not read (every gate must pass), and `log_cap_bytes` and
  `output_cap_bytes` only feed the preflight disk estimate.

## Checks

Run on a snapshot (HEAD plus my files, `.worktrees/_scratch/g1-src`),
because the shared tree did not compile during the run (another worker's
`Requirement::Godot` edit):

- `cargo test -p horch --test dataset_cli`: 4 of 4.
- `cargo test -p horch --lib dataset`: 14 of 14.
- `cargo test -p horch-core` (all targets): green.
- `cargo test -p horch-e2e --test dataset --test promotion --test usage_dataset --test judge`: 41 of 41.
- `cargo clippy -p horch-core -p horch -p horch-e2e --all-targets -- -D warnings`: clean.
- `rustfmt --check` on my files: clean.

## Not done / outside scope

- A resumed round's meter uses `budget.expected_tokens` and the default
  only. The measured usage that PRE-09 saw is not recorded, so `resume`
  cannot reuse it.
- `gates[].required` has no effect (see Decisions).

- The coordinator's own end-of-round cleanup (`competition/cleanup.rs`)
  still leaves the dirs; `run.rs` removes them after it returns.
- A linked worktree as cwd targets the linked worktree, not the main repo
  (the plan names the git top level).
