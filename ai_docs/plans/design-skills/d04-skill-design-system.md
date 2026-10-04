# D04 skill-design-system: write `design-system`, `brand-identity`

Unit slug: `skill-design-system`. Branch: `ds/skill-design-system`.

## GOAL

The bundled skill catalog has `design-system`, `brand-identity`, combined and rewritten from the sources
listed for D04 in `02-skill-map.md`, within the size budget, harness-neutral,
with provenance, and good enough that a design persona can follow it without
other context.

## CONTEXT

- Read first: `ai_docs/plans/design-skills/00-conventions.md`,
  `01-skill-authoring.md` (the rules for every skill), `02-skill-map.md`
  (your sources and the personas that will use your skills), and
  `skills/README.md`. Read 2 existing bundled skills for the house style:
  `skills/tdd/SKILL.md` and `skills/code-review/SKILL.md`.
- What to write:
- `design-system`: build or extend a design system in code: 3-layer tokens
  (primitive, semantic, component) as CSS variables and Tailwind theme,
  type and spacing scales, color system with contrast checks, component specs
  (states, variants, sizes), shadcn/ui and Radix patterns, dark mode, and a
  `DESIGN.md` that agents and humans follow (from stitch-skill). Convert the
  useful ui-ux-pro-max data (styles, palettes, font pairings, UX guidelines)
  into compact curated Markdown tables in references; do not ship CSV or the
  Python search script.
- `brand-identity`: define a brand's voice and visual identity: positioning,
  voice and tone rules, logo usage rules, color and type roles, imagery
  style, and a brand guidelines document. Image generation of brand boards
  belongs to `design-imagery` (D06); keep only the non-image method here.
- D00 `skills-infra` runs in parallel and lands first. It defines the
  multi-source provenance schema and the size budget test. Write your content
  first; add provenance after you rebase on `design-skills` once D00 has
  merged (the orchestrator tells you).
- Other skill units write the other design skills in parallel. Do not
  duplicate their scope; refer to their skill by name as optional.

## FILES

own:
- `skills/design-system/**` (new)
- `skills/brand-identity/**` (new)
- your entries in `skills/provenance.json` and your rows in `skills/README.md`
- `ai_docs/reports/design-skills/skill-design-system.md`

do not touch: every other file.

## STEPS

0. Create the worktree (conventions §3).
1. Read every primary source completely, including the data files. Write a short source inventory in your report.
2. Write `design-system`: SKILL.md plus references (tokens, components, shadcn-tailwind, palettes, font-pairings, ux-guidelines, design-md-template).
3. Write `brand-identity`: SKILL.md plus references (voice, guidelines-template).
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
