# G1 dataset-cli: target repo, status, cleanup and worktree docs

Single-branch work: follow `ai_docs/plans/finish/01-single-branch.md`.

## GOAL

`multi-herdr-dataset` targets the repository the operator means, says which
one it targets, reports the experiment state that is true, leaves no empty
directories after cleanup, and its docs describe the worktree layout.

## CONTEXT

Findings F3, F6, F7 and F8 of `ai_docs/reports/finish/acceptance-dataset.md`
("Findings for other units"). Read them first.

- F3: in a fleet pane `HORCH_PROJECT_DIR` overrides the cwd, so `run` from a
  fleet pane targets the fleet's repo. Wanted precedence: an explicit
  `--project <dir>` flag, then the git top level of the cwd, then
  `HORCH_PROJECT_DIR` only when the cwd is in no git repo. `run` (and
  `preflight`, `status`, `cleanup`, `promote` if they resolve a project)
  prints `target: <repo> @ <HEAD short sha>` as its first line, before
  preflight.
- F6: `status` shows the experiment PLANNED while its round runs or is
  COMPLETE. Derive the experiment state from its rounds (or record the
  transitions), whichever the dataset design says. Read the design state
  table first and follow it; if the design is silent, add the rule to it.
- F7: after `cleanup`, `<root>/<experiment>/` and `<root>/_promote/` stay as
  empty directories. Remove a directory only when it is empty (never a
  recursive delete of something not created by cleanup).
- F8: docs say nothing about `--worktree-root`; the layout is
  `<root>/<experiment>/<label>`.
- opus-61 (F5) edits `competition/coordinator.rs` and
  `competition/observe.rs` now. Do not touch them.

## FILES

own: `crates/horch/src/dataset/` except `preflight.rs` and `judge_job.rs`
(the CLI, `run.rs`, status, cleanup, promote), `crates/horch/tests/dataset_cli.rs`,
the dataset e2e tests for these, `docs/` pages for the dataset CLI,
the dataset design doc sections for project resolution and experiment state
(`ai_docs/designs/2026-10-02-dataset-competition-design.md`; opus-61 edits
section 4.11.4 and the B3 observation table — not those),
`ai_docs/reports/finish/dataset-cli.md`.

do not touch: `competition/coordinator.rs`, `competition/observe.rs`,
`competition/config.rs`, `competition/budget.rs`, `competition/preflight.rs`,
`crates/horch/src/dataset/preflight.rs`, `crates/horch/src/dataset/judge_job.rs`.

## STEPS

1. F3 precedence + `target:` line. Tests: a cwd repo beats
   `HORCH_PROJECT_DIR`; `--project` beats both; no repo falls back to the env.
2. F6 state. Test: status during a running round and after COMPLETE.
3. F7 empty-dir removal. Test: cleanup leaves no empty dirs, keeps non-empty.
4. F8 docs + design text.
   Also: `run.rs:447` calls `Roster::load_layered` and prints no roster
   warning (F7 of the finish wave, `ai_docs/reports/finish/roster-resilience.md`).
   Print each `Roster::load_warnings` line to stderr once, as
   `crates/horch/src/cmd/mod.rs` `load_roster` does. Test it.
5. Targeted checks (`cargo test -p horch --test dataset_cli`, the dataset
   lib tests, the dataset e2e tests you changed, clippy -D warnings on
   `horch`, rustfmt on your files only). COMMITTED.

## DONE WHEN

The 4 findings have tests or doc text, the report lists each with its commit.

## REPORT

`horch tell orchestrator "NOTE: COMMITTED <sha>..."`, then `horch done` with
the summary: files, decisions, checks, anything left.
