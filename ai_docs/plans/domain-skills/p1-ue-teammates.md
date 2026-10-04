# P-UE ue-teammates: the 11 Unreal Engine teammates

Unit slug: `ue-teammates`. Branch: `ds/ue-teammates`.
Starts after W1 (`ue-vendor`), W2 (`ue-own`), V1 (`skill-fields`) and V2
(`roster-offer`) are merged into `design-skills`. The orchestrator tells you
when, through the spawn.

## GOAL

Wave 1 and wave 2 teammates from `ai_docs/reports/unreal-engine-wave.md`
"Teammates to add" exist in `teammates/`, pass `horch teammates --check`,
carry the five persona rules, and are offered only on Unreal projects.

## CONTEXT

- Read first: `ai_docs/plans/domain-skills/00-conventions.md`, the report's
  "Teammates to add", "What the persona text must say", "Harness notes",
  `teammates/README.md` (role/effort table), `teammates/_template.md`,
  `teammates/architect-reviewer.md` (read-only shape) and
  `teammates/qa-engineer.md` (QA shape). Match their voice and comment density.
- Use the report's tables for name, `brief_description`, phase, model,
  effort and expected skills (`skills:` uses full names, e.g.
  `ue-cpp-foundations`). Add `ue-build-verify` to every implementation
  teammate and to `ue-qa-engineer`; add `ue-editor-scripting` to
  `ue-tools-engineer` and `ue-technical-artist` (both exist after W2).
- Rule 2 ("never edit .uasset/.umap bytes") changes now that
  `ue-editor-scripting` exists: for teammates that carry it, assets change
  only through editor Python or a commandlet, never by writing bytes; for
  the rest, keep the hand-back rule as written.
- Every UE teammate: `offer_when: ["*.uproject"]` (V2). Use
  `available_skills:` (V1) only for skills each one's expected skills name
  under "Related Skills"; never list all 31.
- Fallbacks: give each a `fallbacks:` list like its non-UE counterpart
  (look at `architect-reviewer`, `qa-engineer`, `backend-developer`).
- Adding teammates changes count-dependent tests. Known places:
  `SKIP_NEW_TEAMMATES` in `crates/horch-core/tests/baseline_oracles.rs` and
  `crates/horch-core/tests/skills_catalog.rs`, the phase arm and the Claude
  count in `crates/horch-core/src/roster/validation.rs` tests, and the
  orchestrator golden prompt (`golden_prompts.rs` sanctioned changes) if
  the roster lines change it. Run the gate and fix each failure at its
  root; do not bless an oracle you did not mean to change.

## FILES

own: `teammates/ue-*.md`, the test lists above, `teammates/README.md` (a
UE section), `ai_docs/reports/domain-skills/ue-teammates.md`.

## STEPS

0. Create the worktree.
1. Wave 1 (6 files). `horch teammates --check` with the built binary. Gate. Commit.
2. Wave 2 (5 files). Same checks. Commit.
3. Show, with the built binary and a temp dir containing `Game.uproject`,
   that the fleet orchestrator briefing lists the UE teammates, and without
   it, that it does not. Put the commands and output in the report.
4. Report the briefing bytes per teammate (`horch teammates --json` or the
   brief) next to the report's estimates.
