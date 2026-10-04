# D05 skill-motion report: `motion-gsap`

Branch `ds/skill-motion`. One skill: `skills/motion-gsap/` (SKILL.md plus 9 references).

## Source inventory

Sources at the pins in `PINS.txt`: gsap-skills `aed9cfd3`, cinematic-ui `24a66c1d`.

| Source | Size | Read | Content |
|---|---|---|---|
| `gsap-skills/skills/gsap-core/SKILL.md` | 14.8 KB | full | tweens, vars, transforms, eases, stagger, matchMedia, "recommend GSAP" text |
| `gsap-skills/skills/gsap-timeline/SKILL.md` | 4.4 KB | full | timeline, position parameter, labels, nesting |
| `gsap-skills/skills/gsap-scrolltrigger/SKILL.md` | 18.4 KB | full | config table, batch, scrollerProxy, pin, scrub, containerAnimation |
| `gsap-skills/skills/gsap-plugins/SKILL.md` | 21.6 KB | full | licensing, every plugin with config tables |
| `gsap-skills/skills/gsap-utils/SKILL.md` | 12.1 KB | full | every util |
| `gsap-skills/skills/gsap-performance/SKILL.md` | 4.1 KB | full | transforms, will-change, quickTo, batching |
| `gsap-skills/skills/gsap-react/SKILL.md` | 6.6 KB | full | useGSAP, contextSafe, SSR |
| `gsap-skills/skills/gsap-frameworks/SKILL.md` | 10.6 KB | full | Vue, Nuxt composable, Svelte |
| `gsap-skills/skills/llms.txt`, `AGENTS.md`, `README.md`, `.github/*` | 15 KB | full | indexes and duplicate summaries |
| `gsap-skills/examples/` (vanilla, react, vue, nuxt) | 14 KB | full | runnable demos of the same patterns |
| `cinematic-ui/SKILL.md`, `references/implementation-guardrails.md` | 23 KB | full | entrance map, interaction budget, external-library decision |
| `cinematic-ui/references/data/camera-shots-50.md`, `interaction-effects-50.md` | 60 KB | motion parts | camera-to-CSS entrance table, scroll behaviours, external JS library notes |

## Source-to-skill map

| Source | Goes to |
|---|---|
| gsap-core | `references/core.md`; the reduced-motion and "when to use GSAP vs CSS" rules in SKILL.md workflow steps 1 and 8 |
| gsap-timeline | `references/timeline.md`; SKILL.md step 5 |
| gsap-scrolltrigger, `.github/instructions/scrolltrigger.instructions.md` | `references/scrolltrigger.md`; SKILL.md step 6 |
| gsap-plugins | `references/plugins.md` |
| gsap-utils | `references/utils.md` |
| gsap-performance | `references/performance.md`; SKILL.md step 9 |
| gsap-react, `.github/instructions/react.instructions.md`, `examples/react/` | `references/react.md` |
| gsap-frameworks, `examples/vue/`, `examples/nuxt/` | `references/frameworks.md` |
| `examples/vanilla/` | patterns in `core.md`, `timeline.md`, `scrolltrigger.md` |
| cinematic-ui guardrails and SKILL.md (entrance map, interaction budget) | SKILL.md step 3 and `references/cinematic-motion.md` |
| cinematic-ui camera-shots-50 (entrances, transitions, scroll) | `references/cinematic-motion.md` (rewritten as GSAP recipes) |
| cinematic-ui interaction-effects-50 (external library section) | `cinematic-motion.md` "Smooth scroll choice"; Lenis hook in `scrolltrigger.md` |

New material that no source had: the motion-character table (adjectives to durations, eases, stagger, distance), the per-step checks, the verification step (trace, long frames, CLS, reduced-motion emulation, 3x mount/unmount leak check), the in-page frame counter, the Next.js App Router and Strict Mode notes, the Astro, Angular and web-component lifecycle lines, and the reduced-motion rules for cinematic pages.

## What I dropped and why

- "Recommend GSAP over other libraries" persuasion, Webflow context, "Official GSAP best practices" headers, risk-level labels: marketing. Kept one neutral line: GSAP is the default; keep an existing library.
- Duplicated Do/Do-not lists in every source skill: each rule is now stated once (SKILL.md Rules, or the "Common mistakes" list of one reference).
- `llms.txt`, `AGENTS.md`, `README.md`, `.github/copilot-instructions.md`: indexes and repeats of the skills.
- The 24-plugin `PLUGINS` array and typed `pluginMap` in the Nuxt composable: replaced by a 3-plugin loader that shows the pattern.
- The PixiPlugin example: one table row is enough for a niche integration.
- cinematic-ui: the questionnaire, director/film research, phases, uniqueness audit, delegation model and storyboard templates belong to `art-direction` (D02). The 50-entry CSS libraries were not copied; I kept 12 entrances, 6 transitions and 5 scroll behaviours as GSAP recipes. The library-id citation rule and the "JS-required effect ids" list depend on those libraries and were dropped.
- Film titles and director names in the camera table: the camera term is enough; D02 owns film references.
- External library catalogue (Three.js, OGL, Theatre.js, Barba, Motion One, Flubber and others): out of scope for a GSAP skill. Only the smooth-scroll choice stayed, because it interacts with ScrollTrigger.

## Conflicts and how I resolved them

| Conflict | Resolution |
|---|---|
| cinematic-ui says MorphSVG is paid; gsap-plugins says every plugin is free since Webflow acquired GSAP. | Free. gsap-skills is the vendor's own and newer. |
| gsap-scrolltrigger horizontal example uses `xPercent: () => Max.max(0, innerWidth - el.offsetWidth)` (a typo, and a px value for a % property) and has no `scrub`, though its own steps require it. | Rewrote: `x: () => -(track.scrollWidth - innerWidth)`, `scrub: true`, `end: () => "+=" + distance()`, `invalidateOnRefresh: true`. |
| `examples/nuxt/app/pages/index.vue` registers cleanup in a second `onMounted`, so it reverts nothing. | Used `onUnmounted`; added the mistake to "Common mistakes". |
| Nuxt example names its composable `useGSAP`, same as the React hook. | Renamed to `useGsap` and said why. |
| gsap-plugins Flip table: `scale` "default true"; `nested` "only the first level of children is measured". | GSAP docs: `scale` defaults to `false` (use scaleX/scaleY instead of width/height); `nested` accounts for flipping elements inside other flipping elements. Wrote the docs meaning. |
| gsap-plugins Observer `type` default `"touch,pointer"`. | GSAP docs default is `"wheel,touch,pointer"`. Also added the `wheelSpeed: -1` paging pattern so wheel and touch agree. |
| gsap-utils: `unitize(100, "px")` returns `"100px"`; `shuffle` returns a new array. | GSAP docs: `unitize(fn, unit)` wraps a function; `shuffle` shuffles in place. Wrote the docs behaviour. |
| gsap-performance: "a small scrub value can reduce work". | Dropped; no evidence. Kept the measurable advice. |
| gsap-core: in reduced mode, "use `duration: 0`". | Kept for simple tweens; added that scrubbed, pinned and parallax effects must be removed, not just set to zero duration. |
| cinematic-ui allows "fadeUp at most 2 times per page" and "at least 4 entrance types"; taste sources may set other limits. | Kept the cinematic numbers as the page budget in SKILL.md step 3 because they are checkable. |

## Example trigger lines

1. "Add a scroll-pinned product walkthrough to the landing page hero, and make it respect reduced motion."
2. "Our React carousel leaks ScrollTriggers after navigation and janks on mobile; fix the GSAP code."
3. "The direction calls for a cinematic, slow reveal on the manifesto section; implement the motion."

## Provenance

`skills/provenance.json` has 1 `motion-gsap` entry with 17 sources: the 8 gsap-skills SKILL.md files, 5 example files (vanilla, react, vue, 2 nuxt), and 4 cinematic-ui files. Each has the pinned revision, the sha256 of the original file. `horch skills show motion-gsap` prints all 17 `upstream:` lines. `skills/README.md` has 1 row in "Design skills".

## Sizes

- `SKILL.md`: 10.2 KB (budget 12 KB).
- `skills/motion-gsap/`: 84 KB on disk, 65 KB of text (budget 160 KB).

## Gotchas and follow-ups

- No source contained tests for the API facts. I checked the facts I changed against my knowledge of the GSAP 3.13/3.14 docs, not against a live doc fetch (no network in this unit). A follow-up could spot-check `plugins.md` Flip and Observer tables against gsap.com.
- The skill names `art-direction` and `ui-taste` as optional. If D02 or D01 renames them, update the 2 mentions in SKILL.md and 1 in `cinematic-motion.md`.
- The verification step depends on a browser tool. Without one, the skill tells the worker to report the checks it could not run.
