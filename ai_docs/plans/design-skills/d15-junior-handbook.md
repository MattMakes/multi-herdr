# D15 junior-handbook: docs a junior engineer can work from

Unit slug: `junior-handbook`. Branch: `ds/junior-handbook`.

## GOAL

A junior engineer who clones this repo can, from `docs/` alone: understand
what horch and multi-herdr-dataset do, find where each concern lives in the
code, run and read the gate, and follow a step-by-step recipe to add a
harness, a bundled skill, a teammate persona, a CLI command, or a dataset
event, and know which tests and oracles each recipe touches.

## CONTEXT

- Read first: `00-conventions.md`, `README.md` (Architecture section),
  `crates/horch-core/src/lib.rs` (module table), the designs in
  `ai_docs/designs/2026-10-02-*.md`, the reports in
  `ai_docs/reports/arch-refactor-dataset/` and `ai_docs/reports/design-skills/`
  (they hold the real decisions and gotchas), `teammates/README.md`,
  `teammates/_template.md`, `skills/README.md`.
- The operator's flowchart file `/Users/mascott/projects/multi-herdr/ai_docs/command-flow.md`
  (Mermaid; untracked in the main checkout). Copy it into
  `docs/command-flow.md`, check every claim against the code, fix what is
  wrong, and add `horch agent-list` and Antigravity.
- Every recipe step must name the exact files and the check (test name or
  command). Verify each recipe against the code; do not describe what you
  have not confirmed.
- Short pages beat one long page. Link between them.

## FILES

own:
- `docs/README.md` (index), `docs/command-flow.md`, `docs/architecture.md`,
  `docs/testing-and-gates.md` (gate, oracles, goldens, HORCH_BLESS rules,
  fakes, e2e gotchas such as building bins first and never writing into a
  harness bin entry), `docs/recipes/{add-harness,add-skill,add-teammate,add-command,add-dataset-event}.md`,
  `docs/fleet-workflow.md` (how the orchestrator runs units: plans,
  worktrees, merge protocol, STATUS files)
- `README.md` (1 link to `docs/`)
- `ai_docs/reports/design-skills/junior-handbook.md`

do not touch: code, teammates, skills.

## STEPS

0. Create the worktree (conventions §3).
1. Write the pages. For every recipe, walk it against the code and list the
   files it touches.
2. Gate (docs only; it must stay green). Commit `Docs: Add the junior handbook`.
3. Report: the pages and what you verified. Follow conventions §7.
