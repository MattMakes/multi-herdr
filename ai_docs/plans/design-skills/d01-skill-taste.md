# D01 skill-taste: write `ui-taste`, `ui-redesign`

Unit slug: `skill-taste`. Branch: `ds/skill-taste`.

## GOAL

The bundled skill catalog has `ui-taste`, `ui-redesign`, combined and rewritten from the sources
listed for D01 in `02-skill-map.md`, within the size budget, harness-neutral,
with provenance, and good enough that a design persona can follow it without
other context.

## CONTEXT

- Read first: `ai_docs/plans/design-skills/00-conventions.md`,
  `01-skill-authoring.md` (the rules for every skill), `02-skill-map.md`
  (your sources and the personas that will use your skills), and
  `skills/README.md`. Read 2 existing bundled skills for the house style:
  `skills/tdd/SKILL.md` and `skills/code-review/SKILL.md`.
- What to write:
- `ui-taste`: the core of good taste for any interface: anti-generic ("AI
  slop") rules, typography, spacing and rhythm, color, hierarchy, layout
  variance, copy tone, imagery choices, states, and a scored critique rubric
  with concrete checks (banned patterns to search for, contrast ratios,
  type-scale ratios, max line length, spacing scale). It must serve 2 uses:
  building (rules to follow) and reviewing (rubric to score against).
  Include the "complete output" rule from `output-skill` (no placeholders,
  no truncation) as a short rule, not a ritual.
- `ui-redesign`: upgrade an existing site or app: audit (screenshots and
  code), identify generic patterns, prioritize fixes, extract a design from
  a URL or a screenshot, apply changes in safe increments, and prove the
  improvement with before/after screenshots.
- Hallmark and taste-skill overlap a lot. Merge them; keep the strongest,
  most checkable version of each rule.
- D00 `skills-infra` runs in parallel and lands first. It defines the
  multi-source provenance schema and the size budget test. Write your content
  first; add provenance after you rebase on `design-skills` once D00 has
  merged (the orchestrator tells you).
- Other skill units write the other design skills in parallel. Do not
  duplicate their scope; refer to their skill by name as optional.

## FILES

own:
- `skills/ui-taste/**` (new)
- `skills/ui-redesign/**` (new)
- your entries in `skills/provenance.json` and your rows in `skills/README.md`
- `ai_docs/reports/design-skills/skill-taste.md`

do not touch: every other file.

## STEPS

0. Create the worktree (conventions §3).
1. Read every primary source completely. Write a short source inventory in your report (file, size, main ideas).
2. Write `ui-taste`: SKILL.md plus references (for example `references/rubric.md`, `references/typography.md`, `references/anti-patterns.md`).
3. Write `ui-redesign`: SKILL.md plus references (for example `references/audit-checklist.md`, `references/extraction.md`).
4. Self-review each skill against `01-skill-authoring.md` line by line. Fix
   every miss. Check sizes: `wc -c skills/<id>/SKILL.md` and
   `du -sk skills/<id>`.
5. Rebase on `design-skills` after D00 merged. Add provenance and README rows.
6. Gate. Commit `Skills: Add <id>` per skill. Write and commit the report.
   Follow conventions §7.

## DONE WHEN

- `horch skills show <id>` works for each skill; the gate is green; the size
  budget test passes without an exemption for your skills.

## REPORT

- The source-to-skill map, what you dropped and why, 3 example trigger
  lines per skill, and conflicts between sources and how you resolved them.
