---
name: art-direction
description: Use when a site, page or screen needs a visual direction chosen and held before any UI code is written, when builders need a direction contract to follow, or when a build has drifted into generic template output and the direction must be restated.
---

# Art direction

Choose one visual direction, plan the page as a storyboard, fix the direction's tokens, and write a short direction contract that every builder follows. The contract comes before code. A builder who reads only the contract must be able to make the same decisions you would.

## When to use

- A new site, landing page, product page or app screen has a brief but no visual direction.
- Several workers will build pages of one site and need one shared direction.
- A build looks generic ("a template with a new palette") and needs a direction restated or replaced.
- A previous project for the same client exists and the new one must not reuse its shell.

Do not use it to build components (use the project's build workflow), to define a full design system (the `design-system` skill, if available, extends the contract into one), or to generate images (`design-imagery`, if available).

## Inputs

- The brief: product or subject, audience, goal of each page, page list, real content (copy, data, proof, images) and constraints (brand assets, framework, accessibility level, performance budget).
- Existing work in the project: current pages, an existing `DESIGN-DIRECTION.md`, brand files, screenshots.
- Visual references the requester gave, if any.
- Tools that may exist: a browser or screenshot tool, a web search tool, a Pencil design tool. The workflow works without all of them.

## Workflow

1. **Read the brief.** Write a brief summary of at most 10 lines: subject, audience, the one feeling the site must leave, page roles, content you have, content you lack, hard constraints.
   Check: the summary exists in your notes. Each missing fact that blocks a choice is either sent as a `QUESTION:` to the orchestrator or recorded as an explicit assumption.

2. **Audit prior work and references.** List the traits of existing or previous pages that a new design would repeat by habit (hero posture, nav posture, card style, palette, section order). Turn them into a shell-ban list. Split each reference into borrowable dimensions with `references/storyboard.md` ("Reference decomposition").
   Check: the shell-ban list has at least 3 items, also when no history exists (ban the common defaults you would drift toward).

3. **Choose the direction.** Shortlist 2 or 3 entries from the catalog below. For each, answer: which concrete visual quality solves this brief's problem; would it fit 3 unrelated briefs equally well (then it is too generic); am I picking it for its reputation. Pick one. Load only its file from `references/directions/`. When two candidates fit equally, pick by a stable hash of the project name, not by preference.
   Check: you can state the choice in one sentence that names a concrete quality (for example "monumental type and vast empty space, because the product sells scale").

4. **Define the tokens.** Start from the direction file's token block and adapt it to the brand: color roles, type families and scale, spacing scale, radius, borders and shadows, imagery treatment, motion character (durations, easing, entrance vocabulary). Apply the floors in `references/perception.md`.
   Check: every token has a value; every text/background pair you will use has a computed contrast ratio (body text at least 4.5:1, large text and non-text at least 3:1).

5. **Storyboard the pages.** Load `references/storyboard.md`. Write the site grammar first, then for each page: one big idea, a restraint statement, the beat arc, and each section as a shot (framing, focal point, motion intent, content, reason to exist). Lock each page's signature composition before any shared component.
   Check: the storyboard passes the storyboard checks in `references/storyboard.md` (entrance map, grid fallback test, beat count, interaction budget).

6. **Write the direction contract.** Copy `references/direction-contract-template.md` to the project root as `DESIGN-DIRECTION.md` (or the path the brief names) and fill it. Keep it short: builders read it before every page.
   Check: `grep -nE '<[a-z-]+>|TODO|TBD' DESIGN-DIRECTION.md` returns nothing; the token block parses as CSS custom properties.

7. **Hold the direction.** Report the contract path to the orchestrator. When a build is ready, review it with the checklist below and the contract's checks. Record each approved deviation in the contract's change log; reject unrecorded ones.
   Check: each review names the contract rule each finding breaks, with a screenshot, a measurement or a search result as evidence.

## Rules

- No UI code before the contract exists. A storyboard sketch in Markdown is allowed; markup is not.
- One direction per project. Do not blend two directions. You may borrow at most one named trait from a second direction; record it in the contract.
- The contract tokens are the only source of values. A color, font size, radius or duration that is not in the contract is drift.
- One big idea per page. Every section supports it or gives the eye a rest; no section introduces an unrelated idea.
- Grid and flex are alignment infrastructure. A section that would still work unchanged as a generic 3-column card grid is not directed yet.
- Interaction budget per page: at most 1 heavy interaction (pinned scroll scene, cursor-driven canvas, WebGL) and at most 2 showy reveals. If a motion can go without loss, remove it.
- Adjacent sections never share an entrance. `fade + translateY` appears at most 2 times per page, unless the direction file declares a single quiet entrance.
- UI feedback motion (hover, press, toggle, menu) takes 100 to 400 ms. Scene motion (section entrances, hero) follows the direction's motion character and never exceeds 1200 ms or blocks input. Every motion has a `prefers-reduced-motion` fallback.
- Keep process language out of the interface: no director names, film titles, "chapter", "scene", "treatment" or direction ids in visible copy unless the brief asks for it.
- The accessibility floors in `references/perception.md` apply to every direction. A direction changes taste, not legibility.
- Do not copy a reference whole. Borrow dimensions (rhythm, density, type attitude, image treatment, materiality, framing, navigation posture), never its section order, hero composition or grid skeleton.
- A cinematic direction uses a film as research input, not as a spec. Research the film with a web tool if one is available; otherwise say so in the contract and mark the research as inferred.
- Ask the orchestrator (`horch tell orchestrator "[<role>] QUESTION: ..."`) only for decisions that block you. Record other assumptions in the contract and continue. Do not wait for an interactive approval.
- Optional related skills: `ui-taste` (anti-generic checks on the build), `motion-gsap` (implementing the motion intent), `design-system` and `brand-identity` (extending the tokens), `landing-page` (conversion structure), `design-imagery` (images). This skill needs none of them.

## Review checklist

Run on the storyboard before handoff and on every build against the contract.

- [ ] 3-second test: after 3 seconds on the first view, a reviewer can name the one big idea.
- [ ] Logo test: with the logo removed, the page still reads as this direction, not as a generic template.
- [ ] Grayscale wireframe test: a grayscale, low-detail screenshot still shows the signature composition and differs from the shell-ban list and from the previous project.
- [ ] Squint test: a blurred screenshot (about 8 px blur) shows one dominant focal point per view and a clear second level.
- [ ] Token drift: search the build for raw values not in the contract, for example `rg -n '#[0-9a-fA-F]{3,8}\b' src/ | rg -v 'tokens|DESIGN-DIRECTION'`; every hit is a token or a recorded exception.
- [ ] Contrast: each text/background pair meets the floors in `references/perception.md`.
- [ ] Entrance map: no two adjacent sections share an entrance; the interaction budget holds.
- [ ] Process language: `rg -n -i 'director|chapter|scene [0-9]|treatment|storyboard' src/` finds no visible copy.
- [ ] Reduced motion: with `prefers-reduced-motion: reduce` emulated, content is complete and static.
- [ ] Responsive: the signature composition survives at 390 px wide; it adapts, it does not collapse into a plain stack of identical blocks.

## References

- `references/directions/*.md`: one file per direction. Load only the one you chose (step 3).
- `references/storyboard.md`: site grammar, beats, shot language, entrance vocabulary, reference decomposition, storyboard checks (steps 2 and 5).
- `references/perception.md`: hierarchy, attention, grouping and the measurable floors for type, color, motion, spacing and icons (steps 4 and 7).
- `references/direction-contract-template.md`: the `DESIGN-DIRECTION.md` template (step 6).
- `references/pencil.md`: only when a Pencil design tool is available and the brief asks for mockups on a Pencil canvas.

### Direction catalog

| File | Character | Fits |
|---|---|---|
| `editorial-minimal` | Warm monochrome, serif display, document calm | Tools, writing, studios, considered B2B |
| `swiss-industrial` | Light paper, heavy grotesk, visible grid, one red | Manufacturing, architecture, data editorial |
| `tactical-terminal` | Dark CRT, monospace data, bracket framing | Dev tools, infra, security, telemetry |
| `soft-premium` | Airy surfaces, nested bezels, diffuse depth | Consumer apps, wellness, premium agencies |
| `midnight-glass` | Near-black, luminous accent, glass layers | AI and SaaS products, developer platforms |
| `neo-brutalist` | White, thick black borders, hard shadow, one loud accent | Indie products, creator tools, campaigns |
| `bloom-playful` | Warm rose, rounded friendly shapes, generous space | Consumer social, education, lifestyle |
| `cinematic-monumental` | Vast scale, silence, fog, one decisive moment | Deep tech, space, architecture, launches |
| `cinematic-mood` | Saturated warm color, crossfades, fragments of memory | Hospitality, fashion, music, food |
| `cinematic-symmetry` | Centered planimetric frames, pastels, chapter cards | Boutiques, hotels, crafted consumer brands |
| `cinematic-evidence` | Desaturated teal, clinical precision, evidence wall | Fintech, legal, investigations, analytics |
| `cinematic-luminous` | Sky light, organic shapes, gentle drift | Climate, outdoors, education, community |
| `cinematic-noir` | High-contrast monochrome, one accent, hard shadow | Agencies, spirits, fashion, crime and mystery media |
