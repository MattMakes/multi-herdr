# Conventions for the finish run (spec closure, LFS, Blender, operator skills)

Read `ai_docs/plans/design-skills/00-conventions.md` first; all of it applies
(integration branch `design-skills`, worktrees under
`/Users/mascott/projects/multi-herdr/.worktrees/<unit>`, branch `ds/<unit>`,
the gate, the merge protocol). This run adds:

- **Never run the gate or tests under `git rebase -x`** or from a git hook.
  Rebase, then run the gate as its own command. (On 2026-10-03 an exported
  `GIT_DIR` let test fixtures write into the real repository.)
- Reports go to `ai_docs/reports/finish/<unit>.md`.
- PR #18 (`design-skills` → `main`) is the single PR for all this work.
  Do not push; the orchestrator pushes after each green merge.
- Operator decisions for this run (2026-10-04):
  - Every `SPEC-TODO` gets closed. The original Spec A / Spec B text is not
    available, so **the implemented, tested behaviour becomes the spec**.
  - Unreal Engine: the latest released version; version control is **Git LFS**
    (not Perforce).
  - App Store Connect key setup is documented in `README.md`.
  - Operator-local skills (Xcode 27 `xcrun agent skills export`) are wanted.
  - A Blender teammate with the Blender MCP server works with the UE teammates.

## Closing a SPEC-TODO (units T1–T6)

For each marker you own:
1. Find the implemented truth: the code, its tests, and the master plan
   (`ai_docs/plans/arch-refactor-dataset/00-master-plan.md`, read-only).
2. Replace the placeholder with that truth, written as the spec: the exact
   field list, check list, threshold, schema, state list or prose. Cite the
   code (`path:symbol`) and the test that pins it.
3. Where the code made no decision or the decision looks wrong against the
   master plan, decide, write the reason in one sentence, and change the
   code and tests to match. A behaviour change needs a test that fails
   before and passes after.
4. Remove the marker. In code comments, replace it with a pointer to the
   design section. Never leave `SPEC-TODO`, `TODO(spec)` or "verbatim" placeholders.
5. List every marker you closed in your report: location, what it now says,
   evidence, and whether code changed.
