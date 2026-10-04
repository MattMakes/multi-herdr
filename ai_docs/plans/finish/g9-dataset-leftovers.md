# G9 dataset-leftovers: resume keeps the estimates, gates.required, cleanup dirs

Single-branch work: follow `ai_docs/plans/finish/01-single-branch.md`.
Source: `ai_docs/reports/finish/dataset-cli.md` "Not done" (G1) and
`ai_docs/reports/finish/budget-estimate.md` (G2).

## GOAL

1. A resumed round's budget meter uses the same per-candidate estimates
   PRE-09 resolved for the round: record them (an event or the round
   record, whichever the dataset design's event model fits) and read them
   on resume. Test: resume after a kill, the meter projects the recorded
   estimates, not the default.
2. `gates[].required` in `dataset.yaml` is parsed and never read. Read the
   dataset design for what it means. Implement it as designed, with tests;
   or, if the design does not define it, remove the key from the parser,
   the docs (`docs/dataset-config.md`) and the design, and say which in the
   report. Do not leave a parsed key that does nothing.
3. `horch`'s coordinator cleanup (`competition/cleanup.rs`) leaves empty
   `<root>/<experiment>/` and `<root>/_promote/` dirs; `run.rs` removes them
   only after a run. Make cleanup itself remove them (only when empty), so
   `cleanup`, `promote` and resume paths leave none. Test each path.
4. Note in `docs/dataset-config.md` (or the CLI docs) that a linked worktree
   as the cwd targets that worktree, not the main repository (G1 decision).

## FILES

own: `crates/horch-core/src/competition/` except pane command code,
`crates/horch/src/dataset/` except pane command code, `docs/dataset-config.md`,
the dataset design sections you change, the dataset tests,
`ai_docs/reports/finish/dataset-leftovers.md`.
do not touch: pane command builders (G8, opus in pane w2F:p3S, owns them:
if you need a change there, tell the orchestrator), `skills/`, `teammates/`.

## CHECKS

The tests you add, `cargo test -p horch --test dataset_cli`, horch lib
dataset, `cargo build --workspace --bins` then the dataset e2e tests,
clippy -D warnings, rustfmt on your files. COMMITTED per goal.

## REPORT

`horch tell orchestrator "NOTE: COMMITTED <sha>..."`, then `horch done`.
