# D01 skill-taste report: `ui-taste`, `ui-redesign`

Unit: D01 `skill-taste`. Branch: `ds/skill-taste`. Worker: opus-23.

## Result

| Skill | SKILL.md | Directory (bytes of text) | Files |
| --- | --- | --- | --- |
| `ui-taste` | 7,347 B | 62,342 B | SKILL.md + 8 references |
| `ui-redesign` | 7,027 B | 28,773 B | SKILL.md + 4 references |

Both are under the 12 KB body budget and the 160 KB directory budget. `cargo run --quiet --bin horch -- skills show <id>` prints both.

`ui-taste` references: `rubric.md` (hard gates with measurement procedures, six-axis score, report format), `anti-patterns.md` (named tells with ripgrep searches), `typography.md`, `color.md`, `layout-and-space.md`, `states-and-interaction.md`, `copy.md`, `imagery.md`.

`ui-redesign` references: `audit-checklist.md` (standalone audit and fix priority), `preservation.md` (do-not-change list, non-destructive editing, multi-page system, increments), `extraction.md` (design DNA from URL or screenshot with safety and refusal checks), `evidence.md` (before/after screenshot protocol, measurements, report template).

## Source inventory

Paths are relative to `_sources/design-skills/`. Pins: hallmark `13ac0ec7e148655948100b6396439e481361d690`, taste-skill `ce26fc25c0e5e8cab638f883de62d9a86ee5e45b`.

| File | Size | Main ideas |
| --- | --- | --- |
| `hallmark/skills/hallmark/SKILL.md` | 67 KB | Design flow (pre-flight scan, context gate, macrostructure pick, theme catalog, preview, build, 58-gate slop test); verbs audit, redesign, study; six disciplines (self-critique, honest copy, locked tokens, no re-drawn chrome, mobile widths, no italic headers); component-scope 8-state rule. |
| `hallmark/.../references/anti-patterns.md` | 26 KB | Named tells by severity with fixes; microinteraction tells; audit report format. |
| `hallmark/.../references/slop-test.md` | 31 KB | Six-axis pre-emit critique; 58 gates incl. contrast math, input states, nav/footer fingerprints, hero fit at 1280x800, mobile gates 50-57. |
| `hallmark/.../references/typography.md` | 18 KB | 2+1 font rule, banned defaults, free font catalog and tone pairings, 1.25 scale, display cap, hero sizing by length. |
| `hallmark/.../references/color.md` | 4 KB | OKLCH, one accent at 3-5%, tinted neutrals, palette layers, contrast table, dark mode recipe, bans. |
| `hallmark/.../references/layout-and-space.md` | 7 KB | 4 pt named scale, asymmetry, grid breaks, depth, z-scale, overflow-x clip. |
| `hallmark/.../references/interaction-and-states.md` | 14 KB | 8 states, focus rings, hit targets, forms, exhaustive input checklist, overlays, undo over confirm, contrast discipline. |
| `hallmark/.../references/microinteractions.md` | 20 KB | Timing and easing canon, recipes (toast, modal, tooltip, copy, optimistic update), 20 microinteraction tells, a11y ground truth. |
| `hallmark/.../references/motion.md` | 4 KB | transform/opacity only, three easings, durations, page-load stagger, reduced motion. |
| `hallmark/.../references/responsive.md` | 6 KB | 320/375/414/768 floor, clickable text never wraps, breakpoints, dvh, safe areas, i18n. |
| `hallmark/.../references/copy.md` | 12 KB | Specific verbs, errors, empty states, banned openings, voice samples per tone. |
| `hallmark/.../references/structure.md` | 16 KB | Six structural axes, fingerprint anti-repetition, domain-to-macrostructure trios. |
| `hallmark/.../references/verbs/audit.md`, `verbs/redesign.md` | 3 KB, 14 KB | Audit grouping; non-destructive redesign; single vs multi-page; design.md-first for apps. |
| `hallmark/.../references/study.md` | 43 KB | Image vs URL mode, URL safety, refusal lists, junk detection, five-step protocol, schema, diagnosis templates, design.md emission attestation. |
| `hallmark/.../references/design-md.md`, `export-formats.md`, `contract.md` | 7, 15, 2 KB | Portable design-system file, token export formats, output contract (append-only global stylesheet). |
| `hallmark/.../references/hero-enrichment.md`, `custom-craft.md`, `assets.md`, `imagery-kit.md` | 32, 34, 25, 9 KB | Image-need detection, enrichment tiers, hero space discipline, asset sources, placeholder strategy. |
| `hallmark/.../references/custom-theme.md`, `themes/*` (5), `genres/*` (4) | 24 KB, 106 KB, 19 KB | Custom OKLCH palette construction; per-theme and per-genre style catalogs. |
| `hallmark/.../references/macrostructures.md` + 21 files, `component-cookbook.md` + 50 files, `floating-nav.md`, `preview-examples.md` | 12 KB + 81 KB + 31 KB + 8 KB | Page-shape and component archetype catalogs, knobs, mobile collapse. |
| `taste-skill/skills/taste-skill/SKILL.md` | 87 KB | Design read, three dials, design-system map, stack conventions, bias-correction rules (serif discipline, palette bans, layout hard rules, eyebrow count, CTA rules), image strategy, content density, theme lock, AI tells, em-dash ban, redesign protocol, 50-item pre-flight check, design-system appendices. |
| `taste-skill/skills/taste-skill-v1/SKILL.md` | 21 KB | Original dials, anti-center, states, perpetual-motion bento, creative arsenal. |
| `taste-skill/skills/gpt-tasteskill/SKILL.md` | 8 KB | Simulated RNG layout choice, AIDA, 2-line hero rule, gapless bento, GSAP patterns, meta-label ban. |
| `taste-skill/skills/output-skill/SKILL.md` | 3 KB | Complete output: banned stub patterns, scope count, clean pause on limits. |
| `taste-skill/skills/redesign-skill/SKILL.md` | 15 KB | Scan, diagnose, fix; audit by category; upgrade techniques; fix priority; work within the stack. |
| `taste-skill/research/` (12 files) | 30 KB | Why models truncate output; prompt and parameter remedies. |

Reading depth: every file was read in full except the catalog trees (`components/*`, `macrostructures/*`, `themes/*`, `custom-craft.md`, `export-formats.md`, `imagery-kit.md`). For those I read the index files, all headings, and the sections that carry taste rules (theme "not-AI discipline", palette construction, hero space discipline, image-need detection, imagery anti-patterns). Their catalog bodies are page-building material owned by D02, D03, D04, and D06.

## Source-to-skill map

| Destination | Sources |
| --- | --- |
| `ui-taste/SKILL.md` | hallmark SKILL.md (disciplines, pre-flight scan, design read via context gate, self-critique), taste-skill §0 (design read), §14 (pre-flight), output-skill (complete-output rule), hallmark `contract.md` (respect the project, append-only stylesheet) |
| `ui-taste/references/rubric.md` | hallmark `slop-test.md` (six axes, gates 1-57), `interaction-and-states.md` (contrast computation), `responsive.md` (widths); taste-skill §4.7 (hero fit, nav one line), §14 (mechanical checks, eyebrow count); output-skill (stub check) |
| `ui-taste/references/anti-patterns.md` | hallmark `anti-patterns.md`, `microinteractions.md` (named tells); taste-skill §9 (AI tells, production-test tells), §4.7 (split header, zigzag cap); taste-v1 §7; gpt-taste §7 (meta labels); redesign-skill (component patterns) |
| `ui-taste/references/typography.md` | hallmark `typography.md`, `hero-enrichment.md` (hero headline leading); taste-skill §4.1 (serif discipline, emphasis), gpt-taste §3 (wide container fixes 5-line headlines); redesign-skill (text-wrap balance) |
| `ui-taste/references/color.md` | hallmark `color.md`, `custom-theme.md` §B (palette construction order); taste-skill §4.2 (one accent, consistency lock, premium-consumer palette ban, rotation), §8 (dark mode); redesign-skill (shadows) |
| `ui-taste/references/layout-and-space.md` | hallmark `layout-and-space.md`, `structure.md`, `hero-enrichment.md` (hero space), `responsive.md`, `component-cookbook.md` (mobile collapse rules), `themes/hum.md` (variety levers); taste-skill §4.3, §4.4, §4.7, §4.9, §7 (density); gpt-taste §4 (gapless bento) |
| `ui-taste/references/states-and-interaction.md` | hallmark `interaction-and-states.md`, `microinteractions.md`, `motion.md`; taste-skill §4.5, §4.6, §5 (motion motivated, marquee max one), §6 |
| `ui-taste/references/copy.md` | hallmark `copy.md`; taste-skill §4.9 (copy self-audit, fake precision, one register), §4.10 (quotes), §9.D, §9.G (dash discipline); redesign-skill (content) |
| `ui-taste/references/imagery.md` | hallmark `hero-enrichment.md` (image-need detection), `assets.md` (placeholders, icons, logos, video, backgrounds), `anti-patterns.md` (re-drawn chrome); taste-skill §4.8 (real images, logo-only walls, no div screenshots), §3.C (icons) |
| `ui-redesign/SKILL.md` | redesign-skill (scan, diagnose, fix; rules); hallmark `verbs/redesign.md` (non-destructive rule, single vs multi-page); taste-skill §11 (modes, audit first, what never changes) |
| `ui-redesign/references/audit-checklist.md` | redesign-skill (design audit categories, fix priority); taste-skill §11.D-E (levers, decision tree); hallmark `verbs/audit.md` |
| `ui-redesign/references/preservation.md` | taste-skill §11.C, §11.F; hallmark `verbs/redesign.md`, `contract.md`, `design-md.md` (system-first multi-page) |
| `ui-redesign/references/extraction.md` | hallmark `study.md` (all protocol parts), SKILL.md study pipeline |
| `ui-redesign/references/evidence.md` | New: no source has a before/after protocol. Built from the rubric widths, hallmark gate 34 (scroll check), taste-skill §6.D (Core Web Vitals). |

## Dropped, and why

- **Hallmark rituals:** the CSS stamp comment, `.hallmark/log.json` and `preflight.json`, the preview block with a version line, the "Powered by" line, the always-ask three-question gate. They are fixed response rituals or interactive approval gates. Kept instead: the design read line and one orchestrator question when two readings diverge.
- **Hallmark catalogs:** 21 named themes, 21 macrostructures, 50 component archetypes, 4 genres, enrichment tiers, custom-theme axes, diversification rotation. They are page-building and art-direction catalogs owned by D02 (`art-direction`) and D03 (`landing-page`). Kept: the variety rules that are checkable (no repeated section layout family, zigzag cap, nav and footer shaped for the site).
- **Hallmark design.md format and export formats:** owned by D04 (`design-system`). `ui-redesign` refers to it as optional.
- **Hallmark asset vendor catalogs** (generators, illustration libraries, mockup tools, video sources, ffmpeg commands): owned by D06 (`design-imagery`). Kept: the image-need decision, placeholder discipline, and the never-fake list.
- **taste-skill design-system map and appendices** (Fluent, Carbon, Material install commands, Liquid Glass): owned by D04. Kept: "follow the project's system; do not recreate it".
- **taste-skill stack conventions** (React Server Components, Tailwind v4 config, Motion imports, GSAP skeletons, block library schema): framework-specific or motion choreography. Motion choreography goes to D05 (`motion-gsap`). `ui-taste` stays stack-neutral.
- **taste-skill and v1 dials** as numeric configuration: replaced by the density field in the design read; the 1-10 values were not checkable.
- **taste-skill reference vocabulary and creative arsenal** (dock magnification, gooey menus, holographic cards): effects catalogs that conflict with restraint; art direction territory.
- **taste-v1 "Motion-Engine" bento** (perpetual loops on every card, fixed palette and radius): conflicts with motivated motion; a fixed recipe is itself a template.
- **gpt-taste simulated Python randomization, AIDA mandate, "elite" framing, `<design_plan>` block:** rituals and persuasion. Its checkable parts were kept (2 to 3 line hero headline via wider container, gapless bento, meta-label ban, button contrast).
- **taste-skill research folder:** background on model truncation, stimulus prompts ("$200 tip"), vendor parameters. Persuasion and vendor-specific. Only the complete-output rule was kept, as one rule.
- **output-skill pause marker** `[PAUSED — X of Y]`: a fixed ritual. Kept as "stop at a clean boundary and state exactly what remains".
- **Provider tool names** (WebFetch, image-generation tools) and machine paths: replaced with "a browser tool, if available" and "a fetch tool".

## Conflicts between sources and resolutions

| Topic | Sources disagree | Resolution | Why |
| --- | --- | --- | --- |
| Serif display | Hallmark recommends Fraunces and Instrument Serif; taste-skill bans both as defaults and discourages serif generally. | Choose by tone; Fraunces and Instrument Serif and serif display in general need a stated reason; never reuse the last build's display face. | Both sources agree the problem is the reflex, not the face. A stated reason is checkable; a blanket ban is not right for editorial briefs. |
| Italic emphasis in headings | Hallmark bans all italic headings; taste-skill allows same-family italic emphasis. | Ban (headings roman). | Stronger and searchable; hallmark documents it as a top tell. |
| Em dashes | taste-skill: zero anywhere; hallmark: use em dashes correctly. | Zero in headings, labels, buttons, captions, nav, attribution; at most one per paragraph in prose. | Keeps the searchable ban where the tell lives; a total ban in prose damages normal writing. |
| Icon library | taste-skill discourages Lucide; hallmark makes Lucide the default. | One library per project; use the one installed; choose by need. | "Lucide is an AI tell" is not checkable; one library and one stroke width is. |
| Images | taste-skill: every landing page needs real images, no hand-rolled SVG; hallmark: typography-only by default, hand-build SVG. | Decide by subject (image-need table); never fake chrome or div dashboards; simple explanatory SVG is fine; illustration goes to `design-imagery`. | The subject decides; both sources agree on the never-fake list. |
| Centered hero | gpt-taste prefers "cinematic center"; hallmark and taste-skill avoid centered heroes. | Not by default; allowed for manifesto, launch, canvas-led brands with at most two centered elements. | Majority and stronger evidence; the exception is checkable. |
| Hover effects | gpt-taste: `group-hover:scale-105` on every card; hallmark bans uniform hover scale. | One hover signal per element; no uniform scale. | gpt-taste's rule produces the named tell. |
| `transition: all` | taste dial text uses `transition: all 0.3s`; hallmark bans it. | Ban. | Performance and focus-ring correctness; searchable. |
| Motion amount | taste-v1: perpetual loops everywhere; taste-skill: "motion claimed, motion shown"; hallmark: cut motion, at most three primitives. | Motivated motion only, at most three primitives, one entrance, one marquee. | Checkable limits; consistent with reduced-motion rules. |
| Eyebrows | Hallmark: default off, 1-2 per page; taste-skill: at most one per three sections. | Default off; at most one per three sections; never beside the heading. | taste-skill's count is mechanical; hallmark's placement ban is checkable. |
| Full-height hero | taste-skill: `min-h-[100dvh]`; hallmark: no full-viewport hero. | Hero height follows content (60-88%); if a section is full-height, use `dvh`/`svh`, never `vh`. | Combines both: no template hero, no mobile jump. |
| Overflow safety | gpt-taste: `overflow-x-hidden` on `main`; hallmark: `overflow-x: clip` on `html, body`. | `clip`, as a safety net; still find the overflowing element. | `hidden` breaks sticky positioning. |
| Pure white | Hallmark allows `#fff` for modern-minimal; taste-skill bans pure black and white. | No pure base surfaces unless the brand system says so. | One rule with an explicit exception path. |
| Dark mode | taste-skill: always design both modes for consumer pages; hallmark: not required. | Tokens ready for a second mode; test both when the product has both; do not add an unrequested mode. | Avoids scope growth while keeping the token discipline. |
| Theme switch mid-page | taste-skill: no section flips; hallmark: bleed-color blocks allowed. | One theme per page; one deliberate full-bleed band allowed when it is part of the system and its text flips. | Keeps the readability gate and the legitimate design device. |
| Type sizes | Hallmark: at most 5 sizes per page; taste-skill silent. | At most 6 sizes, ratio 1.2 or more between steps. | Real pages need a label size besides five text levels; the ratio is the stronger check. |
| Easing | taste-v1: springs on all interactive elements; hallmark: ease-out for UI state, springs for physical interactions. | Hallmark. | Bounce on UI state is a named tell. |
| Asking questions | Hallmark: always ask three questions; taste-skill: ask one question only when the read diverges. | Ask the orchestrator once, only when the answer changes the work. | Authoring rules forbid interactive gates. |

## Example trigger lines

`ui-taste`:
1. "Build the pricing section for the billing page; it must not look like a template."
2. "Critique the new onboarding screens and give a scored list of what to fix."
3. "Style this settings form: inputs, buttons, error and loading states."

`ui-redesign`:
1. "Our marketing site looks generic and dated. Modernize it without breaking routes or analytics."
2. "Audit the dashboard UI and tell us what to fix first, with screenshots."
3. "We like the layout of <public reference URL>; apply that direction to our landing page and show before and after."

## Gotchas

- In `anti-patterns.md` the searches are in a code block, not a table: inside a Markdown table the `|` alternation must be escaped as `\|`, which ripgrep reads as a literal pipe. Every search was compiled with `rg` against a sample file.
- The example palette in `color.md` was computed: ink on accent first measured 4.27:1 and failed its own rule; the accent moved to `oklch(66% 0.16 40)` (5.5:1). An input-border token at 3.3:1 was added because the divider token (1.6:1) cannot serve as an input border.
- `TODO(image)` placeholder slots are the one allowed TODO; the stub gate (S6) passes them only when the handoff lists them.

- Gate: the first full run failed 2 tests in `crates/horch-e2e/tests/dataset.rs` (`cmp_05`, `cmp_13`) with "harness version unresolved: codex" in preflight. They are unrelated to skills. The test file passed 21 of 21 on a rerun, and the second full gate run was green. The likely cause is timing under load from parallel worktree gates.

## Follow-ups (outside this unit)

- D07 personas: `design-critic` should run `ui-taste` in review mode and `ui-redesign` steps 1 to 4 only; both skills say "do not edit" for those paths.
- D03 `landing-page` and D02 `art-direction` can carry the hallmark macrostructure, component-archetype, and theme catalogs that this unit dropped.
- D04 `design-system` owns the `design.md` format that `ui-redesign` refers to.
