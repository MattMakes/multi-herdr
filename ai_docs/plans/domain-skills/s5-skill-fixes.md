# S5 skill-fixes: close the gaps the skill units reported

Unit slug: `skill-fixes`. Branch: `ds/skill-fixes`.
Starts after S3, S4 and W2 are merged (they all write `skills/provenance.json`).

## GOAL

1. `swiftdata-pro` covers `@ModelActor`: add vanab's
   `references/concurrency-and-actors.md`
   (`/Users/mascott/projects/multi-herdr/.worktrees/_sources/swift/vanab_swiftdata-agent-skill/swiftdata-expert-skill/references/`)
   as a reference, checked against report §5 (MV default, `@concurrent` only
   for CPU-bound work, CloudKit rules), with its own provenance source.
   Link it from SKILL.md.
2. `swiftui-liquid-glass` stops using `.font(.system(size:))` on SF Symbol
   images; use a text style (`.font(.title2)`) or `.imageScale`, so symbols
   follow Dynamic Type (report §5 "Sizes").
3. `skills/provenance.json` entries are sorted by name, every one, and a
   test in `crates/horch-core/tests/skills_catalog.rs` keeps them sorted
   (and `skills/README.md` tables sorted by first column, if a test can do
   that cheaply; otherwise say why not).
4. `skills/README.md` has one "Unreal Engine skills" table and one "Swift
   and Apple skills" table (merges may have left duplicated headings or
   rows). Every bundled skill has exactly one row.

## CONTEXT

- Read first: `ai_docs/plans/domain-skills/00-conventions.md`, the reports
  `ai_docs/reports/domain-skills/{swift-core,ue-own,ue-vendor}.md`.
- Re-hash every file you change and update its provenance sha256 entries
  where the provenance records the upstream file (upstream hashes stay the
  upstream hashes; record the edit in `adaptation`).

## FILES

own: `skills/swiftdata-pro/`, `skills/swiftui-liquid-glass/`,
`skills/provenance.json` (order and these 2 entries), `skills/README.md`,
the new sort test, `ai_docs/reports/domain-skills/skill-fixes.md`.

## STEPS

0. Create the worktree. 1–4 above, a commit each. Gate. Report. Follow the
   merge protocol.
