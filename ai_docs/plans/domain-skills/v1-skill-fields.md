# V1 skill-fields: `available_skills:` and `operator_skills:` on teammates

Unit slug: `skill-fields`. Branch: `ds/skill-fields`.

## GOAL

A teammate can (1) list `available_skills:` that are materialized and named
under "Also available" in its briefing without their full description, and
(2) list `operator_skills: {dir, names}` that horch copies from an
operator-local directory (for example Apple's Xcode 27 skills exported with
`xcrun agent skills export`) into the launch bundle, for every harness that
exposes skills. `horch teammates --check` fails on a missing dir or name.

## CONTEXT

- Read first: `ai_docs/plans/domain-skills/00-conventions.md`,
  `ai_docs/reports/unreal-engine-wave.md` "Code changes" item 2, and
  `ai_docs/reports/swift-fleet-skills-2026-10.md` §7.2.
- Code: `crates/horch-core/src/roster/teammate.rs` (fields),
  `roster/validation.rs` (`--check`), `skills/activation.rs`
  (`plan_activation`: `activated` vs `available`), `skills/briefing.rs`,
  `skills/materialize.rs`, `skills/catalog.rs`, the harness adapters'
  exposure (`harness/*.rs`), `docs/recipes/add-teammate.md`.
- An operator skill is never compiled in and never copied into the repo.
  It is read from the operator's directory at launch, digest-checked like a
  marketplace skill if practical, and named `operator:<name>` in the
  briefing only if a name clash with a bundled skill is possible (decide).
- `~` in `dir` expands from the runtime context's home, not `std::env`.
- `device-interaction` and any skill whose body says it is a subagent skill
  must be refused by `--check` (search the body for "SUBAGENT skill" and
  "Agent tool"); say so in the error.

## FILES

own: the files above; tests in `crates/horch-core/tests/skills_catalog.rs`
(new tests only) and an e2e test in `crates/horch-e2e/tests/skills_exposure.rs`;
`teammates/_template.md` (document both fields); `docs/recipes/add-teammate.md`
(the fields); `ai_docs/reports/domain-skills/skill-fields.md`.

do not touch: `skills/<id>/`, other teammates, oracles. A new field must not
change any existing teammate's oracle.

## STEPS

0. Create the worktree.
1. `available_skills:` end to end with tests (activation, briefing,
   materialize). Gate. Commit.
2. `operator_skills:` end to end with tests (check errors; a temp operator
   dir in e2e; Claude and one non-Claude harness see the skill). Gate. Commit.
3. Docs. Report. Follow the merge protocol.
