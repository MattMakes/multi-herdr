# D02 skill-art-direction: write `art-direction`

Unit slug: `skill-art-direction`. Branch: `ds/skill-art-direction`.

## GOAL

The bundled skill catalog has `art-direction`, combined and rewritten from the sources
listed for D02 in `02-skill-map.md`, within the size budget, harness-neutral,
with provenance, and good enough that a design persona can follow it without
other context.

## CONTEXT

- Read first: `ai_docs/plans/design-skills/00-conventions.md`,
  `01-skill-authoring.md` (the rules for every skill), `02-skill-map.md`
  (your sources and the personas that will use your skills), and
  `skills/README.md`. Read 2 existing bundled skills for the house style:
  `skills/tdd/SKILL.md` and `skills/code-review/SKILL.md`.
- What to write:
- `art-direction`: choose and hold a visual direction before any code:
  read the brief, pick a direction from a catalog (minimalist/editorial,
  brutalist/Swiss-terminal, soft/premium-agency, cinematic directors from
  cinematic-ui, and the strongest presets from pencilplaybook), write a
  storyboard-first page plan (sections as shots: framing, focal point,
  motion intent), define the direction's tokens (type, color, spacing,
  imagery, motion character), and keep a short "direction contract" file in
  the project (for example `DESIGN-DIRECTION.md`) that builders follow.
- Put each style or director in its own reference file under
  `references/directions/` so a worker loads only the one it picked.
- Include the perceptual-psychology principles from pencilplaybook (visual
  hierarchy, attention, grouping) as `references/perception.md`.
- Pencil.dev specifics: keep only what helps when a Pencil tool is available,
  in 1 optional reference file; the skill must work without Pencil.
- Decide whether `pencilplaybook/.claude/skills/best-in-world-*` adds value
  to design work. If not, drop it and say why in the report.
- D00 `skills-infra` runs in parallel and lands first. It defines the
  multi-source provenance schema and the size budget test. Write your content
  first; add provenance after you rebase on `design-skills` once D00 has
  merged (the orchestrator tells you).
- Other skill units write the other design skills in parallel. Do not
  duplicate their scope; refer to their skill by name as optional.

## FILES

own:
- `skills/art-direction/**` (new)
- your entries in `skills/provenance.json` and your rows in `skills/README.md`
- `ai_docs/reports/design-skills/skill-art-direction.md`

do not touch: every other file.

## STEPS

0. Create the worktree (conventions §3).
1. Read every primary source completely. Write a short source inventory in your report.
2. Write `art-direction`: SKILL.md, `references/directions/<name>.md` (one per direction; merge near-duplicates; aim for 8 to 14 directions), `references/storyboard.md`, `references/perception.md`, `references/direction-contract-template.md`.
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
