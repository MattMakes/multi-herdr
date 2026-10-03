# D05 skill-motion: write `motion-gsap`

Unit slug: `skill-motion`. Branch: `ds/skill-motion`.

## GOAL

The bundled skill catalog has `motion-gsap`, combined and rewritten from the sources
listed for D05 in `02-skill-map.md`, within the size budget, harness-neutral,
with provenance, and good enough that a design persona can follow it without
other context.

## CONTEXT

- Read first: `ai_docs/plans/design-skills/00-conventions.md`,
  `01-skill-authoring.md` (the rules for every skill), `02-skill-map.md`
  (your sources and the personas that will use your skills), and
  `skills/README.md`. Read 2 existing bundled skills for the house style:
  `skills/tdd/SKILL.md` and `skills/code-review/SKILL.md`.
- What to write:
- `motion-gsap`: one skill for web motion with GSAP: when to animate and
  when not, motion character from the design direction, core API, timelines
  and the position parameter, ScrollTrigger (pin, scrub, batch), plugins
  (Flip, SplitText, ScrollSmoother, Observer, Draggable, and the rest),
  utils, performance (transforms, will-change, layout thrash, batching),
  React (`useGSAP`, context, cleanup) and other frameworks (Vue, Svelte,
  cleanup on unmount), `prefers-reduced-motion` via `gsap.matchMedia()`, and
  a verification step (no layout shift, 60 fps check in a browser tool if
  available, reduced-motion check).
- The 8 GSAP skills become 1 SKILL.md with a routing table plus one
  reference per topic (`references/core.md`, `timeline.md`,
  `scrolltrigger.md`, `plugins.md`, `utils.md`, `performance.md`,
  `react.md`, `frameworks.md`). Keep exact API facts accurate.
- Add the cinematic motion vocabulary (from cinematic-ui) as
  `references/cinematic-motion.md` only if it adds something concrete.
- D00 `skills-infra` runs in parallel and lands first. It defines the
  multi-source provenance schema and the size budget test. Write your content
  first; add provenance after you rebase on `design-skills` once D00 has
  merged (the orchestrator tells you).
- Other skill units write the other design skills in parallel. Do not
  duplicate their scope; refer to their skill by name as optional.

## FILES

own:
- `skills/motion-gsap/**` (new)
- your entries in `skills/provenance.json` and your rows in `skills/README.md`
- `ai_docs/reports/design-skills/skill-motion.md`

do not touch: every other file.

## STEPS

0. Create the worktree (conventions §3).
1. Read every primary source completely, including `gsap-skills/examples/`. Write a short source inventory in your report.
2. Write `motion-gsap`: SKILL.md plus the topic references.
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
