# G2 budget-estimate: PRE-09 projects a cost that fits the task

Single-branch work: follow `ai_docs/plans/finish/01-single-branch.md`.

## GOAL

PRE-09 projects each candidate's cost from configuration or measurements, not
a fixed `DEFAULT_TOKEN_ESTIMATE`, so `--budget-usd 2` with 2 cheap candidates
passes when the real cost is about $0.07 each.

## CONTEXT

Finding F1 of `ai_docs/reports/finish/acceptance-dataset.md`: PRE-09 projects
$1.60 per sonnet candidate and $1.72 per codex-terra candidate whatever the
task; no key changes it; real spend was about $0.07.

Design, in order of precedence:
1. `budget.expected_tokens` in `.multi-herdr/dataset.yaml`: global, and
   optionally per model (input, output, cache read, cache write, or one total
   with a documented split — pick the simplest that prices correctly with
   `usage::Price`).
2. Measured: the mean (or a high percentile — choose and give the reason) of
   the measured usage of earlier candidates of the same task in this
   project's dataset, when at least N exist (choose N).
3. The current default.
PRE-09's message names which source it used.

L2 (merged, f3b038f) added prices for Sonnet 5.5 and Gemini 3.1 Pro and
pinned every roster model to a price: use `usage::builtin_prices`.

## FILES

own: `crates/horch-core/src/competition/budget.rs`,
`crates/horch-core/src/competition/config.rs` (the new key only; opus-61
added idle keys there, keep them), the PRE-09 code in
`crates/horch-core/src/competition/preflight.rs` (PRE-09 only), the
measured-usage read (where the dataset measures usage; find it),
`crates/horch-core/tests/preflight.rs` (PRE-09 tests), config tests,
the dataset design budget section, `docs/` dataset config reference,
`ai_docs/reports/finish/budget-estimate.md`.

do not touch: `competition/coordinator.rs`, `competition/observe.rs`,
`crates/horch/src/dataset/` (G1 owns it), `harness/`.

## STEPS

1. Config key + parse tests (bad values rejected with a clear error).
2. Estimate source order + PRE-09 message + tests for each source.
3. Design and docs text.
4. Targeted checks (preflight, config, budget tests; clippy -D warnings on
   horch-core; rustfmt on your files). COMMITTED.

## DONE WHEN

A test shows 2 sonnet candidates with `expected_tokens` sized like the LA run
pass PRE-09 at `--budget-usd 2`, and one without the key still uses the
default.

## REPORT

`horch tell orchestrator "NOTE: COMMITTED <sha>..."`, then `horch done`.
