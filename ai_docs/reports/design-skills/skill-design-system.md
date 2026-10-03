# D04 skill-design-system: report

Unit: `skill-design-system`. Branch: `ds/skill-design-system`. Worker: opus-26.

## Result

| Skill | SKILL.md | Directory | References |
|---|---|---|---|
| `design-system` | 8,917 B | 95 KB (text) | tokens, components, shadcn-tailwind, styles, palettes, font-pairings, ux-guidelines, design-md-template |
| `brand-identity` | 7,344 B | 31 KB (text) | voice, visual-identity, guidelines-template |

Both are under the 12 KB body budget and the 160 KB directory budget without an exemption. Both are Markdown only: no CSV, no scripts, no images, no LICENSE files.

## Source inventory

Sources are under `_sources/design-skills/`, pinned in `PINS.txt`. All are MIT.

| Source | Files | Read | Use |
|---|---|---|---|
| `ui-ux-pro-max-skill/.claude/skills/ui-ux-pro-max/` | SKILL.md, 2 references, 13 data CSV/JSON, 22 stack CSVs, Python scripts and tests | SKILL.md, both references, and the data files in full: styles (88 rows), colors (192), typography (74), ux-guidelines (119), products (192), ui-reasoning (192), app-interface (32), stacks/shadcn (71). Schema and samples only: landing, motion, charts, icons, other stacks, google-fonts (1,934 rows), phosphor icons, font licenses. | design-system |
| `ui-ux-pro-max-skill/.claude/skills/design-system/` | SKILL.md, 7 references, 1 JSON template, 8 slide CSVs, 7 scripts | All references and the template in full; scripts and slide data skimmed | design-system |
| `ui-ux-pro-max-skill/.claude/skills/ui-styling/` | SKILL.md, 7 references, 2 scripts, 58 font files with OFL texts | SKILL.md and the shadcn, Tailwind customization, and responsive references in full; utilities and canvas references skimmed | design-system |
| `ui-ux-pro-max-skill/.claude/skills/brand/` | SKILL.md, 11 references, 1 template, 4 scripts | All references and the template in full; scripts skimmed | brand-identity |
| `ui-ux-pro-max-skill/.claude/skills/design/` | SKILL.md, 16 references, logo/CIP/icon data and scripts | SKILL.md, logo style guide, logo color psychology, CIP style and deliverable guides, logo industries data in full; image generation scripts skimmed | brand-identity (non-image parts), design-system (hue table) |
| `taste-skill/skills/stitch-skill/` | SKILL.md, DESIGN.md | Both in full | design-system (DESIGN.md template) |
| `taste-skill/skills/brandkit/` | SKILL.md | In full | brand-identity (non-image parts) |

## Source-to-skill map

### design-system

| Source file | Target |
|---|---|
| `ui-ux-pro-max/SKILL.md` | SKILL.md (priority order, "match product first"), `ux-guidelines.md` (category table) |
| `ui-ux-pro-max/references/quick-reference.md` | `ux-guidelines.md` |
| `ui-ux-pro-max/references/pro-rules.md` | `ux-guidelines.md` (pre-delivery pass, icon and dark-mode rules), `components.md` |
| `ui-ux-pro-max/data/ux-guidelines.csv` | `ux-guidelines.md` (merged with the quick reference, one rule each, a check per rule) |
| `ui-ux-pro-max/data/styles.csv` | `styles.md` (49 active or supplemental styles; deprecated and landing rows removed) |
| `ui-ux-pro-max/data/colors.csv` | `palettes.md` (38 of 192 rows, contrast-measured) |
| `ui-ux-pro-max/data/typography.csv` | `font-pairings.md` (56 of 74 rows) |
| `ui-ux-pro-max/data/stacks/shadcn.csv` | `shadcn-tailwind.md` (current shadcn conventions: `@theme inline`, Field, Sonner, `--dry-run`, component bases) |
| `design-system/SKILL.md`, `references/token-architecture.md`, `primitive-tokens.md`, `semantic-tokens.md` | `tokens.md` |
| `design-system/references/component-tokens.md`, `component-specs.md`, `states-and-variants.md` | `components.md` |
| `design-system/references/tailwind-integration.md` | `shadcn-tailwind.md` (v3 section) |
| `design-system/scripts/validate-tokens.cjs` | SKILL.md step 8 (its regex checks became grep commands) |
| `ui-styling/SKILL.md`, `references/shadcn-theming.md`, `shadcn-components.md`, `shadcn-accessibility.md`, `tailwind-customization.md`, `tailwind-responsive.md` | `shadcn-tailwind.md`, `components.md` |
| `design/references/logo-color-psychology.md` | `palettes.md` (hue table, harmony types) |
| `taste-skill/skills/stitch-skill/SKILL.md`, `DESIGN.md` | `design-md-template.md` |

### brand-identity

| Source file | Target |
|---|---|
| `brand/SKILL.md`, `references/update.md` | SKILL.md workflow (audit, sync to tokens became step 7) |
| `brand/references/messaging-framework.md`, `voice-framework.md` | `voice.md` |
| `brand/references/visual-identity.md`, `logo-usage-rules.md`, `color-palette-management.md`, `typography-specifications.md` | `visual-identity.md` |
| `brand/references/brand-guideline-template.md`, `templates/brand-guidelines-starter.md` | `guidelines-template.md` |
| `brand/references/consistency-checklist.md`, `approval-checklist.md`, `asset-organization.md` | `guidelines-template.md` (checklists, naming) |
| `design/references/logo-style-guide.md` | `visual-identity.md` (logo types, scalability checks) |
| `design/references/logo-color-psychology.md`, `design/data/logo/industries.csv` | `visual-identity.md` (industry cues) |
| `taste-skill/skills/brandkit/SKILL.md` | `visual-identity.md` (brand idea questions, symbol logic, 5 concept methods, mark standard, identity modes, tagline style, color discipline) |

## What I dropped and why

- The Python search engine, BM25 index, `--design-system` generator, design dials CLI, and all scripts (`generate-tokens.cjs`, `validate-tokens.cjs`, `sync-brand-to-tokens.cjs`, `inject-brand-context.cjs`, `extract-colors.cjs`, `validate-asset.cjs`, `shadcn_add.py`, `tailwind_config_gen.py`): the rules forbid scripts. Their useful logic became workflow steps and grep checks.
- All CSV files: converted to curated Markdown tables. Palettes: 192 rows were heavily duplicated (about 30 rows share `#2563EB`), so I kept 38 distinct rows. Font pairings: dropped the 17 mobile tri-stack rows tied to supplemental mobile styles and 1 duplicate Inter row; kept 56.
- Google Fonts catalog (1,934 rows), font license JSON, Phosphor icon catalog, icons.csv: catalogs a worker can query online; too large; low decision value.
- Slide system (8 slide CSVs, slide scripts, Chart.js deck rules) and `design/references/slides*`: presentations are out of scope per the skill map.
- `ui-styling` canvas design system and the 58 bundled font files: poster art direction and binaries; not part of a code design system.
- `tailwind-utilities.md` (generic utility class docs): the worker can read Tailwind docs; nothing system-specific.
- Stack CSVs other than shadcn (22 stacks, 1,271 rows), `react-performance.csv`, `charts.csv`, `motion.csv`, `landing.csv`, `icons.csv`, `app-interface.csv`: framework guidance outside this skill's scope; motion goes to `motion-gsap` (D05), landing to `landing-page` (D03). The chart and native-app rules that matter are folded into `ux-guidelines.md`.
- `products.csv` and `ui-reasoning.csv`: read; their product-to-style mapping duplicates the "Fits" column of `styles.md` and the palette groups, so no separate table.
- Logo, CIP, icon, banner, and social image generation (`design/` scripts, prompts, mockup data) and the brandkit board layouts and prompt template: image work belongs to `design-imagery` (D06).
- `brand/references/update.md` color presets and "AI image generation" section of the starter template: presets duplicate the palette table; image prompts belong to D06.
- Approval sign-off tables with names and dates: people sign off outside the worker session; the skill reports readiness to the orchestrator.
- Interactive gates (`AskUserQuestion`, "ask the user about an HTML preview"), `--force` rituals, provider names (Gemini, Atlas, MuAPI), machine paths (`${CLAUDE_PLUGIN_ROOT}`), and "elite art director" framing.

## Conflicts between sources and how I resolved them

| Conflict | Resolution | Why |
|---|---|---|
| Stitch bans Inter (and generic serifs) everywhere; ui-ux-pro-max recommends Inter for dashboards, docs, Swiss, and spatial UIs | `font-pairings.md` keeps Inter for dense product UI and lists distinctive alternatives for brand and marketing surfaces. Hard taste bans are left to `ui-taste`. | Inter is a strong UI face at small sizes; the ban is a taste rule for expressive surfaces, not a system rule. |
| Stitch requires "perpetual micro-loops" on active components and spring physics everywhere; ui-ux-pro-max limits motion to 1 or 2 key elements and says continuous animation is for loading only | Continuous animation only for loading, unless the brief asks for ambient motion with a reduced-motion path. Durations and easings are tokens. | Accessibility and focus; detailed motion belongs to `motion-gsap`. |
| Stitch bans centered heroes and 3-column card rows; ui-ux-pro-max landing patterns use both | Not in `design-system`. `DESIGN.md` records the project's own bans; layout taste goes to `ui-taste` and `landing-page`. | Layout taste is not a token decision. |
| Stitch bans pure `#000000`; the styles data uses `#000000` for OLED and brutalist styles | Off-black is the default for text on light surfaces; pure black stays available when the chosen style needs it. | Both are true in their contexts. |
| Stitch: "no circular spinners, skeletons only"; ui-ux-pro-max: spinners for short actions, skeletons for longer waits | Under about 1 s no indicator, 1 to 10 s skeleton or inline spinner for small actions, over 10 s progress with text. | Matches platform guidance and keeps button spinners. |
| Token sources use HSL channels (Tailwind v3, older shadcn); the shadcn stack data and Tailwind customization reference use OKLCH and `@theme inline` (v4) | `shadcn-tailwind.md` leads with v4 and keeps a v3 section; the skill says to follow the version the project uses. | Both versions exist in real repositories. |
| Toast duration: 3 to 5 s (quick reference) vs. no number (others) | 3 to 5 s with pause on hover and focus. | One value, stated once. |
| Disabled opacity 0.38 to 0.5 (quick reference) vs. 0.5 (states reference) | 0.5 or a muted fill. | Common shadcn default; within both ranges. |
| Large text: "18px+" (several sources) | WCAG definition: 24 px, or 18.66 px bold. | The sources used the point value as pixels. |
| Logo minimum width: 80 px (template) vs. 120 px (logo usage rules) | 120 px for the full lockup, 80 px as an absolute floor. | Keeps both with clear meaning. |
| Shadcn default neutrals were presented as accessible; measurement shows `muted-foreground` on `muted` 4.35:1 and `input` and `ring` under 3:1 | `shadcn-tailwind.md` states the measured values and what to darken. | Verification over trust. |

## Verification done

- Every palette row was measured with the WCAG formula (script run in this session): primary on its "on" color, accent on its "on" color, foreground on background, and muted text on background are all at least 4.5:1. One source row (Spatial Computing) failed and is excluded. The "Primary on bg" column is measured.
- Every contrast number quoted in `tokens.md`, `shadcn-tailwind.md`, `palettes.md`, and the `DESIGN.md` example was computed, not copied. One source example (`--input: gray-400`) failed 3:1 and was changed to `gray-500`.
- The two grep commands in `design-system` step 8 were run against a fixture and match hex, `rgb()`, and arbitrary pixel utilities.
- `horch skills show design-system` and `horch skills show brand-identity` print both skills.

## Example trigger lines

`design-system`:
1. "Set up design tokens and a Tailwind v4 theme for the new dashboard, with dark mode."
2. "Our components use raw hex colors everywhere; move them onto a token system and write a DESIGN.md."
3. "Spec the button, input, and dialog states for the component library and wire them into shadcn/ui."

`brand-identity`:
1. "Define the voice, tone, and messaging for our developer tool before we write the launch site."
2. "Write brand guidelines: logo usage, colors, type, imagery, for the new product name."
3. "Audit the marketing site and onboarding emails for brand consistency and list what to fix."

## Gotchas and follow-ups

- Before the orchestrator's test fix, 4 tests assumed 16 skills from one upstream (catalog count, `.md`-only source paths, the `horch skills` listing golden and oracle). The orchestrator fixed them on `design-skills`; this branch did not edit them.
- `horch-e2e --test dataset` failed twice in full gate runs under machine load (load average about 7.7): `mea_10_every_spawn_has_terminal_event` (stale ledger lock race) and `sec_03_no_transcript_copies_by_default` (exit 4). Both passed alone, the whole target passed 21 of 21 alone, and the third full gate run was green. The flake is unrelated to skills; it is a follow-up for the dataset owner.
- `design-system` names `ui-taste`, `art-direction`, `motion-gsap`, `landing-page`, and `brand-identity` as optional; `brand-identity` names `design-imagery`, `design-system`, and `art-direction` as optional. If D01 to D06 rename a skill, update these lines.
- The palettes and font pairings are starting points. Personas should be told that brand inputs override them.
- Fontshare families (Satoshi, Cabinet Grotesk, General Sans) carry a license note; D07 personas should not treat them as Google Fonts.
