# D00 skills-infra: multi-source provenance and a size budget for bundled skills

Unit slug: `skills-infra`. Branch: `ds/skills-infra`.

## GOAL

The bundled skill catalog accepts a skill adapted from several upstream
repositories (each with its own revision, path and sha256), checks a
size budget for every bundled skill, and no test hard-codes the number of
bundled skills. Six skill units rebase on this unit, so it must land first and
fast.

## CONTEXT

- Read `ai_docs/plans/design-skills/00-conventions.md` and
  `01-skill-authoring.md` (the budget numbers come from there).
- Today: `skills/provenance.json` has one top-level `source_repository` and
  `source_revision`, and each skill has 1 `source_path` and 1
  `source_sha256` (optional per-skill repository and revision).
  `crates/horch-core/src/skills/catalog.rs` (`ProvenanceFile`,
  `ProvenanceSkill`, `Provenance`, `SkillCatalog::bundled`) parses it.
  `crates/horch-core/build.rs` embeds `skills/` and `provenance.json`.
- `crates/horch-core/tests/skills_catalog.rs` line 61 asserts
  `catalog.len() == 16`; about line 93 checks provenance per skill.
- `horch skills show <id>` prints provenance (find the printer in
  `crates/horch/src/cmd/`).

## FILES

own:
- `crates/horch-core/src/skills/catalog.rs` (provenance types and parsing only)
- `skills/provenance.json` (schema only; existing entries keep their meaning)
- `skills/README.md` (a short "Multi-source skills" paragraph and an empty
  "Design skills" table with the header row; skill units add rows)
- the `skills show` printer in `crates/horch/src/cmd/` (print every source)
- `crates/horch-core/tests/skills_catalog.rs`
- `ai_docs/reports/design-skills/skills-infra.md`

do not touch: any `skills/<id>/` directory, teammates, oracles.

## STEPS

1. Create the worktree (conventions §3).
2. Schema: a skill entry may have `sources: [{repository, revision, path,
   sha256}]` in place of the single-source fields. Keep reading the
   old fields (they map to a 1-item list). `Provenance` gets
   `sources: Vec<SourceRef>` (keep the old public fields only if a caller
   needs them; prefer one clean shape and update callers). Every bundled
   skill must have a provenance entry, except `orchestrate` (repo-original;
   allow `sources: []` with an `adaptation` text for any repo-original skill).
3. Tests in `skills_catalog.rs`:
   - replace `assert_eq!(catalog.len(), 16)` with checks that do not need an
     edit when a skill is added (for example: every directory under `skills/`
     with a SKILL.md is in the catalog, and every catalog entry has a
     provenance entry or is listed as repo-original);
   - `skills_multi_source_provenance_parses` (a fixture with 2 sources);
   - `skills_bundled_size_budget`: every bundled `SKILL.md` is at most 12 KB
     and every skill directory is at most 160 KB, EXCEPT the existing 16
     skills (list them by name in an `EXEMPT_FROM_BUDGET` const if any of
     them is over; do not shrink them);
   - `skills_bundled_text_only`: every bundled file ends in `.md` or `.txt`
     (exempt `skill-creator` if it has other files; check first).
4. `horch skills show <id>` prints each source on its own line.
5. Gate. Commit `Skills: Accept multi-source provenance and check a size budget`.
   Write the report (the exact JSON shape with 1 example entry, for the skill
   units to copy). Follow conventions §7.

## DONE WHEN

- The gate is green; existing provenance still parses; the new tests pass.

## REPORT

- The JSON example in the report and in your `horch done` summary.
