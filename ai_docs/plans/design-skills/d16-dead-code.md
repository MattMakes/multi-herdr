# D16 dead-code: delete, wire or document every public item with no caller; drop stale workarounds

Unit slug: `dead-code`. Branch: `ds/dead-code`.

## GOAL

No public item in `horch-core` or `horch` exists without a reason a junior
can read: each item from the A12 "no caller" list is deleted, wired where a
caller is missing, or kept with a doc comment that names the seam it serves.
Stale test workarounds are gone.

## CONTEXT

- Read first: `00-conventions.md`,
  `ai_docs/reports/arch-refactor-dataset/a12-cleanup.md` (the list under
  "These items have no caller", about line 160), `b3-coordinator.md` and
  `a6b-service.md` (the idempotency design).
- This unit runs after D12 and D13 merged. It may touch many files; no other
  code unit runs at the same time (D14 and D15 touch only skills and docs).
- Decide per item, and record the decision in the report table:
  - `execution::store::ExecutionStore::find_by_idempotency`: the B3 design
    said the coordinator calls it before it plans a spawn. Check how the
    coordinator adopts an existing candidate execution today. If an
    idempotency path is missing, wire this call and add a test; if the
    events already guarantee it, delete the method.
  - `teacher::*` (OD4, the inert Clef/Laya decision): designed seam; keep,
    with a doc comment that points at `ai_docs/designs/2026-10-02-dataset-competition-design.md` §1.3.
  - `horch::dataset::NOT_IMPLEMENTED`: delete if every placeholder is filled.
  - every other item: delete unless a test or a documented seam needs it.
- Stale workaround: `crates/horch-e2e/tests/skills_exposure.rs` about line
  265, `skl_06_e2e_marketplace_offline` edits the brief after spawn because
  spawn once checked only the compiled-in catalog. Spawn now uses
  `ensure_supported_in` with the installed catalog: remove the workaround
  and the stale comment; the test must still prove offline use.
- After the deletions, `cargo build` must have 0 warnings.

## FILES

own: the files that hold the listed items and their tests;
`crates/horch-e2e/tests/skills_exposure.rs`;
`ai_docs/reports/design-skills/dead-code.md`.

do not touch: oracles, goldens, teammates, skills.

## STEPS

0. Create the worktree (conventions §3) after the orchestrator says D12 and D13 merged.
1. For each item: grep callers (all crates, tests included), decide, act. Gate per group. Commit per group.
2. The skl_06 workaround. Gate. Commit.
3. Report: the decision table (item, decision, reason). Follow conventions §7.
