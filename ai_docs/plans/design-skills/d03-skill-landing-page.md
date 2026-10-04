# D03 skill-landing-page: write `landing-page`

Unit slug: `skill-landing-page`. Branch: `ds/skill-landing-page`.

## GOAL

The bundled skill catalog has `landing-page`, combined and rewritten from the sources
listed for D03 in `02-skill-map.md`, within the size budget, harness-neutral,
with provenance, and good enough that a design persona can follow it without
other context.

## CONTEXT

- Read first: `ai_docs/plans/design-skills/00-conventions.md`,
  `01-skill-authoring.md` (the rules for every skill), `02-skill-map.md`
  (your sources and the personas that will use your skills), and
  `skills/README.md`. Read 2 existing bundled skills for the house style:
  `skills/tdd/SKILL.md` and `skills/code-review/SKILL.md`.
- What to write:
- `landing-page`: build a distinctive, conversion-aware landing page end
  to end: brief and audience, message hierarchy, section structure (hero,
  proof, features, objections, pricing, CTA, footer) with proven patterns,
  copy rules, hero and banner art direction, responsive and performance
  budgets, accessibility, and a generate-then-review loop with screenshots
  at 3 widths (for example 390, 768, 1440 px).
- The ui-landingpage-generator-skill source generates with Gemini CLI and reviews with Claude.
  Adapt that idea to the fleet rule: the worker generates and reviews in its
  own session (draft, screenshot, critique against a rubric, revise), and may
  ask the orchestrator for a separate critic worker. No nested agent CLI.
- Section patterns go to `references/sections.md`; copy formulas to
  `references/copy.md`; hero and banner direction to `references/hero.md`.
- D00 `skills-infra` runs in parallel and lands first. It defines the
  multi-source provenance schema and the size budget test. Write your content
  first; add provenance after you rebase on `design-skills` once D00 has
  merged (the orchestrator tells you).
- Other skill units write the other design skills in parallel. Do not
  duplicate their scope; refer to their skill by name as optional.

## FILES

own:
- `skills/landing-page/**` (new)
- your entries in `skills/provenance.json` and your rows in `skills/README.md`
- `ai_docs/reports/design-skills/skill-landing-page.md`

do not touch: every other file.

## STEPS

0. Create the worktree (conventions §3).
1. Read every primary source completely. Write a short source inventory in your report.
2. Write `landing-page`: SKILL.md plus references (sections, copy, hero, review-loop, performance-budget).
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
