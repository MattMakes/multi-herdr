# O1 operator-skills: turn on Apple's Xcode skills without breaking other hosts

Unit slug: `operator-skills`. Branch: `ds/operator-skills`.

## GOAL

`swift-developer` and `apple-platform-developer` declare
`operator_skills: {dir: ~/.agents/skills, names: [swiftui-whats-new-27]}`
(and any other Xcode 27 skill the Swift report §7.2 recommends), and:
- on a host that has the exported skills, the launch materializes them;
- on a host that lacks the directory or a name, `horch teammates --check`
  prints a warning (not an error) and the launch skips the missing skill and
  says so in the worker's briefing ("operator skill X is not installed on
  this host: ask the operator to run `xcrun agent skills export`").
- A real error stays an error: a name that clashes with a bundled skill, a
  subagent skill (`device-interaction`), or a directory that exists but
  cannot be read.

## CONTEXT

- Read first: `ai_docs/plans/finish/00-conventions.md`,
  `ai_docs/reports/domain-skills/skill-fields.md` (V1, the field) and
  `ai_docs/reports/domain-skills/swift-teammates.md` (why it was left out:
  `SkillCatalog::with_operator_skills` bails on a missing dir and `--check`
  calls it).
- This Mac has Xcode 26.6; `xcrun agent skills export` exists only in
  Xcode 27, so no export is possible here. `~/.agents/skills` exists and
  holds unrelated skills. Test with a temp operator dir, never the real one.
- Check the exact `xcrun agent skills export` syntax and default output dir
  against Apple's Xcode 27 docs or release notes (web), and write it in the
  persona comments, `docs/recipes/add-teammate.md` and `README.md`
  (one short "Apple's Xcode skills" subsection).

## FILES

own: `crates/horch-core/src/skills/catalog.rs` (with_operator_skills),
`roster/validation.rs` (the check), `skills/briefing.rs` (the note), the two
teammate files, `README.md` (that subsection only), `docs/recipes/add-teammate.md`,
tests, `ai_docs/reports/finish/operator-skills.md`.

## STEPS

0. Worktree. 1. Warning + skip + briefing note, with tests (missing dir,
missing name, clash still errors). 2. Teammates + docs. 3. Gate. Report.
