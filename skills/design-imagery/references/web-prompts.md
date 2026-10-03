# Web section prompts

Use this file to plan and write one image prompt per web section. Every image must show a developer how to build the section: layout, hierarchy, spacing, type scale, CTA priority, component style, and image treatment. Do not produce abstract mood art unless the task asks for it.

## 1. Count and order the sections

| Request | Sections |
|---|---|
| Hero only | 1 |
| Landing page, product page, portfolio | 6 |
| Full website, marketing site | 8 |
| Explicit number N | N |

Default packs:

| Size | Order |
|---|---|
| 4 | Hero, Features, Social proof, CTA |
| 6 | Hero, Trust bar, Features, Product showcase, Testimonials, CTA |
| 8 | Hero, Trust bar, Features, Product showcase, Use cases, Testimonials, Pricing, CTA |
| 12 | Hero, Trust bar, Feature grid, Product preview, Problem and solution, Benefits, Workflow, Proof or integrations, Testimonials, Pricing, FAQ, CTA and footer |

Every section has one job: hook, proof, educate, or convert. The last section closes with one strong CTA and one trust cue.

## 2. Read the brief into a direction

The brief always wins over these defaults.

| Brief says | Hero scale | Background modes | Gradients | Composition |
|---|---|---|---|---|
| minimalist, clean, Swiss, typography-only | Mini | solid, subtle texture, at most 1 color-blocked diptych | none or softest tonal | stacked center, wide negative space |
| editorial, magazine, fashion | Mid or Giant | editorial side image, duotone, graded photo | subtle tonal | off-grid offset, asymmetric |
| cinematic, atmospheric, luxury, bold | Giant | full-bleed image with tonal overlay, radial vignette, micro-noise | palette-matched, welcome | bottom-left over image, centered low, image as canvas |
| SaaS, product, dashboard, fintech, infra | Mid | solid with inline asset, flat block with detail crop | very subtle | clear product framing, trust anchors |
| agency, studio, portfolio | Giant or Mini (choose one) | vary boldly | editorial washes | off-grid, poster-like |
| e-commerce, shop, product page | Mid, product-led | full-bleed product photo, vignette with crop | subtle, never over the product | product-led, unmistakable CTA |
| silent on style | choose one decisively | confident variety | subtle | varied |

Hero scales:

- Giant: massive type, large image, dominant first viewport.
- Mid: balanced type and image, cinematic but not screen-filling.
- Mini: small logo, short statement, thin CTA, mostly negative space. Mini means confident restraint, not weakness.

## 3. Direction menu

Choose one item from each line and write the result into the direction brief. Do not mix several.

- Theme: pristine light (off-white, paper, sharp dark text); deep dark (charcoal, graphite); bold studio solid (oxblood, royal blue, forest, vermilion); quiet premium neutral (bone, sand, stone).
- Type character: clean grotesk; refined grotesk; expressive display; compressed statement; editorial serif with sans; Swiss rational sans.
- Section system: modular bento; alternating editorial blocks; poster-like stacked story; gallery cadence; Swiss grid; asymmetric marketing flow.
- Concept spine: artifact or specimen; journey with waypoints; precision instrument; living system; stage and spotlight; archive or dossier.
- Signature components (choose 4): staggered square masonry, cascading card deck, accordion slices, gapless bento grid, polaroid arc, vertical rhythm lines, off-grid editorial layout, product UI panel stack, split testimonial wall, oversized metrics strip, layered image crop frames.
- Motion cues (choose 2, as implied energy only): scrubbed text reveal, pinned narrative, staggered float-up, parallax image drift, accordion expansion, cinematic fade-through. If the motion-gsap skill is available, it builds these later.
- Second-read moment (choose exactly 1 for the whole page): an asymmetric bleed, one oversized numeral that carries structure, one material switch, a narrow side-rail note, a macro crop that carries the brand color. It must help scan order or brand recall.

## 4. Per-section choices

For each section choose 1 composition anchor, 1 background mode, and 1 CTA style.

Composition anchors:

- centered statement
- top-left lead with support bottom-right
- bottom-left text over a background image
- bottom-right CTA cluster
- left-third caption, right two-thirds visual (classic; never twice in a row)
- right-third caption, left two-thirds visual
- centered low (text in the lower 40% over an image)
- off-grid editorial offset
- stacked center (label, headline, sub, CTA)
- image as canvas, text in a clean safe area

Background modes:

- solid surface with inline asset
- subtle texture, paper, or grid
- full-bleed image with tonal overlay
- editorial side image (50/50, 60/40, 40/60)
- flat color block with a small detail crop
- cinematic tonal gradient, low chroma
- graded atmospheric photo
- duotone image, palette-locked
- soft radial vignette with product crop
- micro-noise gradient over solid
- color-blocked diptych

CTA styles: primary pill, outline or ghost, underlined inline link with arrow, full-width banner CTA, oversized headline with a small CTA, CTA as caption under a visual. Vary the style at least once per page. The primary action stays unmistakable; secondary actions look secondary.

Variety gate (reject and re-plan the set if any line is true):

- The same anchor appears in more than 2 sections in a row.
- The same background mode appears in more than 3 sections in a row.
- The brief is not minimalist and no section uses a full-bleed, duotone, or atmospheric background.
- The page has 4 or more sections and no calm, mini section.

## 5. Prompt template

Write each prompt from this template. Keep the direction brief identical in every prompt of the set.

```
Website section design reference, 1 of a set of N, one section only.
Section: <name> - job: <hook | proof | educate | convert>.
Canvas: horizontal <16:9 | 16:10 | 21:9>, desktop width 1440 px, flat screen render, no browser chrome.
Direction (same for every section): <theme>; palette primary <#hex>, secondary <#hex>, accent <#hex>, neutrals <#hex, #hex, #hex>; type <character>, strong scale contrast; radius <value>; image treatment <grade, framing>; texture <level>; concept <spine>.
Composition: <anchor>. Background: <mode>. CTA: <style>, label "<text>".
Copy (render exactly, large and readable): headline "<5 to 10 words>"; subline "<one sentence>"; <other labels>.
Imagery: <subject, crop, role in the layout>.
Spacing: generous, even section padding; one framing move; no nested cards.
Exclude: purple-blue AI gradients, mesh blobs, neon glow, gradient headline text, fake stats, pills, tiny labels, lorem ipsum, logo walls of illegible marks.
```

Hero additions: one focal point, headline of 1 to 3 lines, readable on a 1280 x 720 laptop viewport, no badges or system labels.

## 6. Set-level checks

Before you move to analysis, confirm:

1. The image count equals the section count.
2. Each image is horizontal and holds one section.
3. The palette, type family, CTA family, and image treatment match across the set.
4. At least 2 distinct image crops appear (for example macro product and wide environment).
5. Charts appear only when the product is about data. Otherwise proof is human: quotes, receipts, timelines, real workflow screenshots.
6. Foreground and background intensity changes at least twice down the page (lighter, richer, calmer).
7. The section-to-section spacing reads as even.
8. A hook, proof, action path is visible from first to last image.
