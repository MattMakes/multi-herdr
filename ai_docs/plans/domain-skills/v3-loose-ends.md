# V3 loose-ends: finish what V0, V1 and V2 left open

Unit slug: `loose-ends`. Branch: `ds/loose-ends`.

## GOAL

No follow-up from the V0–V2 reports stays open:
1. The execution ledger record lists the operator skills a launch used.
2. `horch teammates --matrix` shows `available_skills`, `operator_skills`,
   `offer_when` and `requires`.
3. `skill-creator` uses the multi-source provenance shape (`sources`,
  , `vendored: true`), so `EXEMPT_FROM_BUDGET` and the
   pin-check skip for it go away (keep only a text-only exemption if its
   scripts need one).
4. The two fake-opencode e2e flakes have a root cause and a fix:
   `fake_opencode_writes_transcript_rows_when_asked` (`launch.status.success()`
   false) and `fake_opencode_session_list_matches_cwd` (`opencode --version`
   gave empty stdout). Both were seen under load on 2026-10-03, after the
   hard-link fix a4a0640.

## CONTEXT

- Read first: `ai_docs/plans/domain-skills/00-conventions.md`, then the
  reports `ai_docs/reports/domain-skills/{vendoring-infra,skill-fields,roster-offer}.md`
  ("Decisions", "Gotchas", "Follow-ups").
- Item 1: skill-fields says operator skills are added per teammate by
  `SkillCatalog::with_operator_skills(teammate, home)` after planning; the
  follow-up is to pass the home into `PlanInputs` so `plan_launch` sees
  them. Keep planning free of I/O if the design requires it (read
  `ai_docs/designs/` for the spawn-service rule): the CLI can read the
  operator dir and hand the entries in.
- Item 4: `ai_docs/reports/design-skills/followups.md` rows 3–4 describe the
  earlier fake fixes. Reproduce under load (a loop of 30+ runs while
  `yes > /dev/null` runs on most cores). Fix the cause; do not raise a
  timeout without a measurement that justifies the number.
- D18 (opus-45, `ds/flake-and-durable`) is changing `execution/store.rs` and
  `execution/service.rs`. Do not edit those files; if item 1 needs them,
  tell the orchestrator first.

## FILES

own: `crates/horch-core/src/execution/plan.rs` and `routing/` only for item 1,
`crates/horch/src/cmd/teammatescmd.rs`, the `skill-creator` entry in
`skills/provenance.json`, `crates/horch-core/tests/skills_catalog.rs` (the
exemptions only), `crates/horch-e2e/` fakes and their tests,
`ai_docs/reports/domain-skills/loose-ends.md`.

do not touch: `teammates/`, other skills, `execution/store.rs`,
`execution/service.rs`.

## STEPS

0. Create the worktree.
1. Items 2 and 3 (small). Gate. Commit each.
2. Item 1 with a test that the record lists an operator skill. Commit.
3. Item 4: reproduce, root cause, fix, before/after rates. Commit.
4. Gate. Report. Follow the merge protocol.
