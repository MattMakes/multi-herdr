# D06 skill-imagery report

Unit: `skill-imagery`. Branch: `ds/skill-imagery`. Skill written: `design-imagery`.

## Result

- `skills/design-imagery/SKILL.md`: 8756 bytes (budget 12 KB).
- 5 references: `web-prompts.md`, `mobile-prompts.md`, `brand-board-prompts.md`, `image-to-spec.md`, `spec-to-code.md`.
- Whole directory: 42214 bytes of text (`du -sk` = 52 KB), budget 160 KB.
- Description: 232 characters, starts with "Use when".
- `horch skills show design-imagery` prints the skill.

## Source inventory

Paths are relative to `_sources/design-skills/`. All MIT.

| Source file | Bytes | Content |
|---|---|---|
| `taste-skill/skills/imagegen-frontend-web/SKILL.md` | 36854 | Web section image direction: one image per section, dials, variation menus, composition anchors, background modes, slop bans, section packs, clarity check |
| `taste-skill/skills/imagegen-frontend-mobile/SKILL.md` | 40326 | Mobile screen image direction: platform mode, screen count, design bible, flow logic, device frame, safe areas, category bias, regeneration triggers |
| `taste-skill/skills/image-to-code-skill/SKILL.md` | 36442 | Generate, analyze, implement workflow; extraction rules for text, type, spacing, buttons, color; anti-drift; missing-detail order |
| `taste-skill/skills/brandkit/SKILL.md` | 15992 | Brand board images: strategy first, symbol table, 5 logo methods, 3x3 and 2x3 layouts, visual modes, prompt template |
| `ui-ux-pro-max-skill/.claude/skills/design/SKILL.md` | 14126 | Router for logo, CIP, slides, banner, icon, social; script commands |
| `.../design/references/logo-design.md` | 4253 | Logo styles, color and industry defaults, script workflow |
| `.../design/references/logo-prompt-engineering.md` | 4314 | Logo prompt structure, style keywords, negative prompt, variation template, pitfalls |
| `.../design/references/logo-style-guide.md` | 3435 | 7 logo types, aesthetic styles, scalability checklist |
| `.../design/references/logo-color-psychology.md` | 3341 | Color meanings, industry palettes, contrast note |
| `.../design/references/cip-design.md` | 4301 | Identity mockup deliverables and scripts |
| `.../design/references/cip-prompt-engineering.md` | 2493 | Mockup prompt structure, lighting and context modifiers |
| `.../design/references/cip-style-guide.md` | 2357 | Identity mockup style presets |
| `.../design/references/cip-deliverable-guide.md` | 1735 | Deliverable specifications |
| `.../design/references/icon-design.md` | 4151 | SVG icon generation with a text model |
| `.../design/scripts/logo/generate.py` | 27346 | Read only: the logo prompt template and style modifiers |

## Source-to-skill map

| Target | Sources |
|---|---|
| `SKILL.md` When to use, Workflow | image-to-code (§2, §10, §11, §28, §36), web (§19), mobile (§36) |
| `SKILL.md` Rules | web (hard output rule, hero bias, §8), mobile (§5, §29), image-to-code (§5, §14, §16, §29); contrast numbers from logo-color-psychology |
| `SKILL.md` Review checklist | web §17, mobile §35, image-to-code §35, logo-style-guide scalability checklist |
| `references/web-prompts.md` | imagegen-frontend-web (§1, §2, §5, §15, §18); prompt template is new |
| `references/mobile-prompts.md` | imagegen-frontend-mobile (§2 to §34); prompt template is new |
| `references/brand-board-prompts.md` | brandkit (strategy, logo methods, layouts, modes, prompt), logo-style-guide, logo-prompt-engineering, generate.py template and modifiers, cip-prompt-engineering (mockup modifiers) |
| `references/image-to-spec.md` | image-to-code (§8, §9, §21 to §25); scale method, YAML spec template, and color sampling rules are new |
| `references/spec-to-code.md` | image-to-code (§26 to §28); the screenshot loop and its numeric checks are new |

## Dropped, and why

- All script commands, setup steps, API keys, and named providers (Gemini, Atlas Cloud, MuAPI) in `design/`. The plan requires no specific image API. The worker uses its harness's image tool.
- Numeric dials (DESIGN_VARIANCE and similar, 8 to 19 per source). They duplicate the direction menus and the ui-taste skill's scope. The brief-to-direction table carries their effect.
- "Elite art director" framing, "aggressively break defaults", "final goal" lists, repeated count tables. Marketing and repetition.
- "Announce the section count out loud" and "Section X of N" as a response ritual. Kept only as an image label, which helps the spec step.
- `AskUserQuestion` gates in logo and banner workflows. Replaced by an orchestrator `QUESTION:` for operator-owned choices only.
- "In Codex" rules. Made harness-neutral; the per-section rule now applies everywhere.
- Logo color psychology table and industry palettes. They suggest purple-blue tech gradients that the taste sources ban, and brand color choice belongs to the brand-identity skill (D04).
- Icon SVG generation (`icon-design.md`). It is a text-model task, not imagery. If needed, it fits design-system or brand-identity.
- CIP deliverable specifications (card sizes, paper weights), slides, banners, social photos. Out of the D06 scope; banners belong to landing-page (D03). Identity mockups keep a short optional section.
- The 55-row logo CSV data. Reduced to a 7-row logo type table and 11 style modifiers.

## Conflicts and resolutions

| Conflict | Resolution |
|---|---|
| The mobile skill says "images only, never code"; image-to-code says generate, then build. | One workflow with separate steps. Prompt steps produce images only; implementation is steps 6 and 7. |
| Image-to-code allows multi-section boards "outside Codex"; the web skill forbids them. | One image per section everywhere. Extraction quality needs it on every harness. |
| Logo sources require a white background; brandkit prefers a dark charcoal board canvas. | Logo explorations: plain white, 1:1, for clean silhouettes. Brand boards: dark or light, chosen to fit the brand. |
| Mobile defaults to a visible phone frame; measurement needs the bare screen. | Concepts use the frame. Extraction images are flat at 390 or 412 px. |
| Logo industry palettes recommend blue-to-purple gradients for tech; the taste sources ban them. | The ban wins. Color choice comes from the direction brief. |
| Web: "do not ask follow-up questions"; design: "always ask the user". | Proceed on a strong interpretation. Ask the orchestrator only for operator-owned decisions. |
| Web encourages full-bleed and gradients; minimalist briefs want none. | The brief always wins; the variety gate is suspended for minimalist briefs, as the web source says. |

## Example trigger lines

1. "Explore 3 visual directions for the new pricing page as images before we build it."
2. "Here is a screenshot of the competitor's onboarding flow. Write a spec and build our version to match its layout."
3. "Generate a brand board and 5 logo directions for Harbor, a compliance tool for small clinics."

## Optional skills referenced by name

ui-taste, art-direction (direction choice), motion-gsap (motion cues), design-system (tokens), brand-identity (strategy and tokens). None is required.

## Gotchas

- Image models misspell text. The brand reference tells the worker to generate marks without text and set the wordmark in a real font.
- The provenance entry lists 8 adapted files, including `scripts/logo/generate.py` (read only, for its prompt template). The other design references I read are in the inventory but contributed no retained content.

## Status

- Content complete and self-reviewed against `01-skill-authoring.md`.
- Rebased on `design-skills` after D00. Provenance entry (8 sources) and README row added.
- `horch skills show design-imagery` prints all 8 upstream sources.
- Gate: `HORCH_REQUIRE_GIT=1 HORCH_REQUIRE_SQLITE=1 just gate` is green. The size budget test passes with no exemption.
