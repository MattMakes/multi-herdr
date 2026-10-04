# G9 dataset-leftovers: report (opus-89)

Plan: `ai_docs/plans/finish/g9-dataset-leftovers.md`. Sources: "Not done" of
`ai_docs/reports/finish/dataset-cli.md` (G1) and
`ai_docs/reports/finish/budget-estimate.md` (G2).

## Goals and commits

| goal | change | test | commit |
|---|---|---|---|
| 1 resume keeps the estimates | PRE-09's `measured` gets `estimates`: `{label: {model, tokens}}` (`competition/preflight.rs`, `RecordedEstimate`). `preflight::recorded_estimates(report)` reads them. `resume` (`dataset/run.rs` `resumed_estimates`) gives the meter the recorded tokens of each label whose model did not change; another label falls back to `budget.expected_tokens`, then the default. | `pre_09_records_each_label_estimate` (`horch-core/tests/preflight.rs`), `resume_meter_uses_the_recorded_estimates` (horch lib) | ea92a81 |
| 2 `gates[].required` | Implemented as the design defines it (§4.6: `eligible` is "every required gate passed"). `GateSpec.required` (new, `evaluation/validator.rs`); `CommandValidator::validate` sets `eligible` from the required gates only. `mechanical_score` still counts every gate. `run.rs` passes `g.required`. Promotion's revalidation uses the same validator, so the same rule. | `cmp_09_only_a_required_gate_decides_eligibility` (`horch-core/tests/vcs.rs`), `a_gate_is_required_by_default` (`competition/config.rs`) | 9f9a0e7 |
| 3 cleanup dirs | `remove_empty_dirs` moved from `dataset/cleanup.rs` into `competition/cleanup.rs`. `RoundCleanup::run` calls it after the worktree removals, before `round.completed`. The 2 CLI calls (`run.rs`, `dataset/cleanup.rs`) are gone. | `cleanup_removes_only_empty_round_dirs` (moved to core lib); e2e `assert_no_round_dirs` in `cmp_04` (run), `crash_and_resume_in` for every COMPLETE crash point (resume), `pro_cleanup_needs_force_from_intervention` (`cleanup --force`), `pro_08_promote_to_and_promote_cmd` (`promote`) | f677552 |
| 4 linked worktree note | `docs/dataset-config.md`: a linked worktree as cwd targets that worktree (its `dataset.yaml`, its HEAD, its own dataset dir); `--project <main repository>` targets the main one. | doc text | 1bc6e88 |

## Decisions

- **Where the estimates are recorded.** In the PRE-09 check's `measured`
  JSON. The report is already in `preflight.completed` and in the manifest,
  and `resume` already reads it from the projection. No new event kind and
  no schema change. A report written before this has no key and reads as
  empty (old behaviour).
- **Label versus model.** The recorded estimate is used only while the label
  keeps its model. A round that `resume` plans again can put a label on
  another model; that label gets the config or the default.
- **`required` default: `true`.** The design names the field
  (`GateConfig.required: bool`) but gives no default. The parser already
  defaulted to `true`. A gate that the operator adds must keep a broken
  candidate out of the judge unless the operator opts out, so `true` is the
  safe default.
- **validator.rs and tests/vcs.rs** were outside FILES. The orchestrator
  granted them (option 1). Both had no diff before my edit.
- **Mutation check (goal 3).** With the core call disabled, the 4 e2e paths
  fail (cmp_04, cmp_13 crash points, procleanup, pro08). Restored after.

## Checks (on the tip 1bc6e88, snapshot `.worktrees/_scratch/g9-src`)

The shared tree did not compile during my run: another worker's
`skills/catalog.rs` edit imports `BUNDLED_VENDORED`, which does not exist yet.
So I ran every check on a `git archive` of the tip.

- `cargo test -p horch --test dataset_cli`: 4 of 4.
- `cargo test -p horch --lib dataset`: 14 of 14.
- `cargo test -p horch-core --lib`: 438 of 438.
- `cargo test -p horch-core --test preflight --test vcs --test promotion --test coordinator`: green.
- `cargo build --workspace --bins`, then `cargo test -p horch-e2e --test dataset --test promotion --test usage_dataset --test judge`: 41 of 41.
- `cargo clippy -p horch-core -p horch -p horch-e2e --all-targets -- -D warnings`: clean.
- `rustfmt --check` on my files: clean.

## Gotchas

- `crates/horch/src/dataset/run.rs` holds an uncommitted G8 hunk (the watch
  pane command, `HORCH_DATA_DIR`). I staged `run.rs` without that hunk each
  time (`git hash-object` plus `git update-index --cacheinfo`), so my
  commits do not contain it. The hunk is still in the working tree for G8.

## Not done / outside scope

- No e2e for goal 1. The e2e harness cannot make the recorded estimate
  differ from the config one without 3 earlier measured runs of the same
  task. The lib test reads the estimate back from a recorded
  `preflight.completed` event.
- `docs/command-flow.md` (not mine) says "Cleanup ... also removes" the
  empty dirs. The text is still true.
