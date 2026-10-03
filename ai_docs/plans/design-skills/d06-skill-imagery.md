# D06 skill-imagery: write `design-imagery`

Unit slug: `skill-imagery`. Branch: `ds/skill-imagery`.

## GOAL

The bundled skill catalog has `design-imagery`, combined and rewritten from the sources
listed for D06 in `02-skill-map.md`, within the size budget, harness-neutral,
with provenance, and good enough that a design persona can follow it without
other context.

## CONTEXT

- Read first: `ai_docs/plans/design-skills/00-conventions.md`,
  `01-skill-authoring.md` (the rules for every skill), `02-skill-map.md`
  (your sources and the personas that will use your skills), and
  `skills/README.md`. Read 2 existing bundled skills for the house style:
  `skills/tdd/SKILL.md` and `skills/code-review/SKILL.md`.
- What to write:
- `design-imagery`: use image generation to explore and lock a visual
  design before code, then implement from it: write image prompts for web
  and mobile screen concepts (one image per screen or section), brand boards
  and logo explorations, analyze a reference image into a structured spec
  (layout grid, type, color, spacing, components), and implement the spec in
  code with a screenshot comparison loop.
- The skill must say clearly: use it only when the harness has an image
  generation tool or the operator supplies reference images. Without either,
  the worker uses the analysis and implementation steps with supplied
  screenshots, or reports that it cannot generate images.
- No specific image API (Gemini, MuAPI, Atlas Cloud) is required.
- D00 `skills-infra` runs in parallel and lands first. It defines the
  multi-source provenance schema and the size budget test. Write your content
  first; add provenance after you rebase on `design-skills` once D00 has
  merged (the orchestrator tells you).
- Other skill units write the other design skills in parallel. Do not
  duplicate their scope; refer to their skill by name as optional.

## FILES

own:
- `skills/design-imagery/**` (new)
- your entries in `skills/provenance.json` and your rows in `skills/README.md`
- `ai_docs/reports/design-skills/skill-imagery.md`

do not touch: every other file.

## STEPS

0. Create the worktree (conventions §3).
1. Read every primary source completely. Write a short source inventory in your report.
2. Write `design-imagery`: SKILL.md plus references (web-prompts, mobile-prompts, brand-board-prompts, image-to-spec, spec-to-code).
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
