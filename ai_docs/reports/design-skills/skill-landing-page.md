# D03 skill-landing-page report

Unit: `skill-landing-page`. Branch: `ds/skill-landing-page`. Skill: `skills/landing-page/`.

## Result

| File | Bytes | Load when |
|---|---|---|
| `SKILL.md` | 7824 | Always (workflow, rules, checklist, index) |
| `references/sections.md` | 9160 | Step 3: page patterns, section catalog, layout rules |
| `references/copy.md` | 5997 | Step 4: message block, headline and CTA formulas, banned patterns, self-audit |
| `references/hero.md` | 8060 | Step 5: hero rules, paradigms, visual sources, art direction, banners |
| `references/performance-budget.md` | 5008 | Step 6: CWV and weight budgets, responsive, images, fonts, measuring |
| `references/review-loop.md` | 8886 | Step 8: direction note, review round, 15-item rubric, JS measurement snippets, critic request |

Directory: 56 KB (`du -sk`). Budgets: body at most 12 KB, directory at most 160 KB.

## Source inventory

Paths are relative to `_sources/design-skills/`.

| Source | Size | Read | What it holds |
|---|---|---|---|
| `ui-landingpage-generator-skill/SKILL.md` | 4.8 KB | full | Pipeline: context analysis, Gemini generation, Claude review, open in browser; style families; frontend-design rules |
| `ui-landingpage-generator-skill/design_prompt.txt` | 4.4 KB | full | Random-style 3-paragraph prompt (feeling, type and motion arc, abstract references) |
| `ui-landingpage-generator-skill/generate_landing.sh` | 25.8 KB | full, not run | Embedded context-to-brief prompt and review prompt (bug, CSS, a11y, responsive checks) |
| `ui-landingpage-generator-skill/README.md` | 6.8 KB | full | Install and usage of the same pipeline |
| `ui-ux-pro-max-skill/.claude/skills/banner-design/SKILL.md` | 7.1 KB | full | Banner workflow, sizes, top 10 styles, design rules |
| `ui-ux-pro-max-skill/.claude/skills/banner-design/references/banner-sizes-and-styles.md` | 5.0 KB | full | Full size tables, 22 styles, safe zones, CTA, typography, print |
| `ui-ux-pro-max-skill/.claude/skills/ui-ux-pro-max/data/landing.csv` | 25.4 KB | full | 34 landing page patterns: section order, CTA placement, conversion and a11y notes |
| `ui-ux-pro-max-skill/.claude/skills/ui-ux-pro-max/data/products.csv` | 75.6 KB | landing column | 192 product types mapped to a landing pattern |
| `ui-ux-pro-max-skill/.claude/skills/ui-ux-pro-max/references/quick-reference.md` | 24.5 KB | a11y, performance, layout parts | Rule names for contrast, targets, CLS, images, fonts, breakpoints |
| `taste-skill/skills/taste-skill/SKILL.md` | 87.3 KB | sections 0, 1, 4, 6, 9, 10 (hero), 13, 14 | Brief read, hero discipline, layout bans, copy audit, AI tells, CWV, pre-flight |

## Source-to-skill map

| Source part | Went to |
|---|---|
| felix-huber pipeline (analyze, generate, review, open) | `SKILL.md` workflow; `review-loop.md` sections 2 and 5 |
| felix-huber context-to-brief prompt (users, emotional journey, differentiation, conventions) | `SKILL.md` step 1 brief read |
| felix-huber 3-paragraph design prompt | `review-loop.md` section 1 (direction note) |
| felix-huber review prompt (bugs, CSS, a11y, responsive, design) | `review-loop.md` rubric items 4, 5, 7, 8, 9 |
| felix-huber style library (25 styles, 7 families) | `hero.md` section 4 family table (condensed to 7 families) |
| banner-design workflow, rules, sizes, styles | `hero.md` section 5 |
| `landing.csv` 34 patterns | `sections.md` section 2 (merged to 14 patterns) and section 3 per-section notes |
| `products.csv` landing column | `sections.md` "Starting pattern by product family" (12 families) |
| quick-reference a11y, performance, layout | `SKILL.md` accessibility rule; `performance-budget.md` sections 2-4 |
| taste-skill 0.A-0.B brief inference and design read | `SKILL.md` step 1 |
| taste-skill 4.2, 4.4, 4.11 color, shape and theme locks | `SKILL.md` rule "One theme, one accent, one radius system"; `sections.md` layout rules |
| taste-skill 4.5 CTA wrap, duplicate intent, contrast | `SKILL.md` rule "One primary action"; `copy.md` section 4 |
| taste-skill 4.7 hero and layout discipline | `hero.md` section 1; `sections.md` section 4 |
| taste-skill 4.8 image strategy, logo walls, fake UI ban | `hero.md` section 3; `sections.md` logo wall |
| taste-skill 4.9-4.10 content density, copy self-audit, quotes | `copy.md` sections 3, 6, 8; `sections.md` testimonials |
| taste-skill 6 performance and reduced motion | `performance-budget.md` sections 1, 6 |
| taste-skill 9 AI tells (hero, labels, dashes, copy) | `copy.md` section 7; `review-loop.md` rubric 7 and snippet |
| taste-skill 10 hero paradigms | `hero.md` section 2 |
| Original to this skill | `copy.md` headline formulas, CTA rules and frameworks; `performance-budget.md` numeric weight budgets; `review-loop.md` rubric scoring and JS snippets |

## What I dropped and why

- **The Gemini plus Claude shell pipeline** (`generate_landing.sh`, `gemini -y`, `claude --print`): the fleet rule bans nested agent CLIs. The worker now drafts, screenshots, critiques and revises in its own session, and may ask the orchestrator for a critic worker.
- **Random style selection** (`design_prompt.txt`): an unrelated random style ignores the audience. The style comes from the brief read. The 3-paragraph prompt shape stays as the direction note.
- **"Elite strategist" framing, "unforgettable", "forbidden" persuasion**: marketing voice, removed per authoring rules.
- **Fixed output file layout** (`result/latest.html`, timestamped raw outputs): pipeline artifacts. Replaced with `review/r<round>-<width>.png`.
- **AskUserQuestion requirements gathering and "iterate until approved"** (banner-design): interactive gates. Replaced with orchestrator questions only when a missing fact changes structure.
- **Pinterest research queries, "Security" block** (banner-design): needs a specific browsing service; the security block is generic boilerplate.
- **20 of the 34 `landing.csv` patterns**: duplicates and narrow variants (2 comparison patterns, 2 pricing patterns, AI personalization, 3D configurator, horizontal scroll, bento, marketplace, community, docs, real-time). Their rules went into the section catalog where they apply. The color hex suggestions were dropped: color belongs to art direction and the design system.
- **`products.csv` color and style columns**: many suggest the defaults the skill bans (for example AI purple for AI products). Only the landing-pattern mapping stayed.
- **taste-skill dials, design-system install appendix, GSAP skeletons, redesign protocol, block library, Tailwind class names**: they belong to `ui-taste`, `ui-redesign`, `motion-gsap` and `design-system` (D01, D04, D05). The landing skill names those skills as optional.
- **taste-skill font name lists and the serif ban**: font choice belongs to `ui-taste` and `art-direction`. The landing skill keeps only the checkable hero type rules.
- **taste-skill "picsum.photos" and Simple Icons CDN defaults**: runtime downloads from third-party hosts. Replaced with labeled placeholder slots and supplied assets.

## Conflicts between sources and the resolution

| Conflict | Resolution | Why |
|---|---|---|
| felix-huber asks for "high-impact" motion, gradient meshes, noise and dramatic shadows; taste-skill bans glows, decoration and motion for show | Motion is last, optional, one orchestrated device, with a reason per animation. Texture only when the direction note asks for it. | Intentional restraint is checkable; "high impact" is not. Both sources agree on "one orchestrated page load beats scattered effects". |
| felix-huber picks a random style; taste-skill says the audience picks | The brief read picks the aesthetic family. | A landing page has a conversion goal and an audience. |
| `landing.csv` puts the CTA in several places (hero, sticky, bottom); taste-skill bans duplicate CTA intent | Repeat the primary CTA freely, but with one label per intent. | Both rules hold together: repetition helps conversion, differing labels confuse. |
| taste-skill 9.D asks for "organic, messy data" (47.2%); taste-skill 4.9 and `landing.csv` ban unverified numbers | No invented numbers. Real numbers with a source, or a marked placeholder. | Fake-but-plausible data ships to production by accident and misleads visitors. |
| taste-skill makes an image-generation tool mandatory when present, then picsum | Supplied assets first, then real UI, then generated images if allowed, then CSS art, then a labeled placeholder. | Harness-neutral, no runtime download, and brand assets beat generated stock. |
| taste-skill makes dark mode mandatory for consumer pages; it also locks one theme per page | One page-level theme (light, dark or auto). If auto, review both. | Dual themes double the review work; the brief decides. |
| banner-design puts the CTA bottom right | Applies to banners only. In the page hero the CTA follows the reading order. | Different formats. |
| `landing.csv` lists "key features (3-5)" as cards; taste-skill bans 3 equal cards | Keep 3-6 features, but in a varied layout (bento, split rows, tabs). | The content is fine; the identical-card layout is the problem. |
| felix-huber bans Inter and system fonts outright; taste-skill allows Inter for neutral and public-sector briefs | Font choice is out of scope here and deferred to `ui-taste`. | Avoid duplicating D01. |

## Example trigger lines

1. "Build a landing page for our invoice tool from `docs/prd.md`; the goal is free-trial signups."
2. "Our waitlist page converts badly. Review it and rebuild the hero and the pricing section."
3. "Make the launch page and matching X header and LinkedIn banner for the October webinar."

## Gotchas

- `review-loop.md` measurement snippet needs `data-cta="primary"` on the main CTA for the fold check. The skill says so in the snippet comment.
- Layout-shift and LCP entries may need a buffered `PerformanceObserver` in some browsers; noted in the file.
- The banner size table will age. The skill says the brief's platform spec wins.

## Provenance and gate

- `skills/provenance.json` entry `landing-page` lists 9 sources (3 repositories) at the PINS.txt revisions, each with sha256 and `license: MIT`. `horch skills show landing-page` prints 9 `upstream:` lines.
- `skills/README.md` has one row in "Design skills".
- Gate: green on the 3rd run. Runs 1 and 2 failed in `crates/horch-e2e/tests/dataset.rs` (`sec_08_e2e_no_api_key_in_any_child`: "PRE-06 FAIL harness version unresolved: claude"; once `cmp_05_e2e_candidates_in_dataset_workspace`). The machine load average was about 7.7, with other worktrees building. These tests do not read skills. They pass alone. This is a timing flake to watch. After the rebase on D01 and D06, run 1 failed once more in the same file (`cmp_04_n_worktrees_same_base_modify_same_file`: empty numstat), at load average about 8.6. Run 2 was green.

## Follow-ups (outside my scope)

- D07 `landing-page-builder` persona: attach `landing-page`, and optionally `ui-taste`, `art-direction`, `motion-gsap`, `design-imagery`. The skill names those as optional.
- A critic persona (`design-critic`) can reuse `references/review-loop.md` section 3 as its rubric for landing pages.
