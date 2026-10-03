# Skill map: which unit writes which skill from which sources

Source paths are relative to `/Users/mascott/projects/mh-wt/_sources/design-skills/`.
Any unit may read any source. Only the owner writes a skill directory.
If a source file fits another unit's skill better, send `QUESTION:` before you
move it.

| Unit | Writes `skills/<id>` | Primary sources |
|---|---|---|
| D01 skill-taste | `ui-taste`, `ui-redesign` | `hallmark/skills/hallmark/` (all files), `taste-skill/skills/taste-skill/`, `taste-skill/skills/taste-skill-v1/`, `taste-skill/skills/gpt-tasteskill/`, `taste-skill/skills/output-skill/`, `taste-skill/skills/redesign-skill/`, `taste-skill/research/` |
| D02 skill-art-direction | `art-direction` | `cinematic-ui/` (SKILL.md, directors-library.md, references/, agents/, docs/), `taste-skill/skills/minimalist-skill/`, `taste-skill/skills/brutalist-skill/`, `taste-skill/skills/soft-skill/`, `pencilplaybook/` (SKILL.md, presets/, references/, docs/; also decide on `.claude/skills/best-in-world-*`) |
| D03 skill-landing-page | `landing-page` | `ui-landingpage-generator-skill/` (SKILL.md, design_prompt.txt), `ui-ux-pro-max-skill/.claude/skills/banner-design/`, landing-page data in `ui-ux-pro-max-skill/` (search for landing, hero, conversion), the landing-page parts of `taste-skill/skills/taste-skill/` (read only) |
| D04 skill-design-system | `design-system`, `brand-identity` | `ui-ux-pro-max-skill/.claude/skills/{ui-ux-pro-max,design-system,ui-styling,design,brand}/` and their data files, `taste-skill/skills/stitch-skill/`, `taste-skill/skills/brandkit/` (non-image parts) |
| D05 skill-motion | `motion-gsap` | `gsap-skills/skills/*` (8 skills), `gsap-skills/examples/`, the motion parts of `cinematic-ui/` (read only) |
| D06 skill-imagery | `design-imagery` | `taste-skill/skills/{imagegen-frontend-web,imagegen-frontend-mobile,image-to-code-skill}/`, the image parts of `taste-skill/skills/brandkit/` and `ui-ux-pro-max-skill/.claude/skills/design/` (logo generation) (read only) |

Not carried unless a unit argues for it in its report:
`ui-ux-pro-max-skill/.claude/skills/slides/` (presentations, not frontend),
`pencilplaybook/.claude/skills/skill-creator/` (we already bundle
`skill-creator`), the `cli/` copies in `ui-ux-pro-max-skill` (duplicates).

## Personas that will use them (D07 builds these)

| Teammate | Phase | Skills |
|---|---|---|
| `design-director` (new) | research | art-direction, ui-taste, brand-identity |
| `design-critic` (new, reads only) | validation | ui-taste, ui-redesign |
| `landing-page-builder` (new) | implementation | landing-page, ui-taste, art-direction, motion-gsap |
| `design-system-engineer` (new) | implementation | design-system, ui-taste, brand-identity |
| `motion-engineer` (new) | implementation | motion-gsap, ui-taste |
| `visual-prototyper` (new, Codex) | implementation | design-imagery, ui-taste |
| `designer` (existing) | research | + ui-taste, art-direction |
| `frontend-developer` (existing) | implementation | + ui-taste, design-system |

Write each skill so that these personas can use it without the others.
