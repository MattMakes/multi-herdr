# Hero and banner art direction

Load at workflow step 5. The hero is the first viewport: it states the value proposition, shows a real visual, and offers the primary action.

## 1. Hero hard rules (checkable)

| Rule | Check at 1440 x 900 and 390 x 844 |
|---|---|
| Fits the first viewport | The primary CTA is fully visible without scrolling. |
| Headline length | At most 2 lines at 1440 px. A 3-4 line headline is a font-size or copy error. |
| Subtext | At most 20 words and at most 4 lines. |
| Text elements | At most 4: optional eyebrow or brand strip, headline, subtext, CTA group. |
| CTA group | 1 primary, at most 1 secondary. No tagline, trust strip, pricing teaser, avatar row or feature bullets in the hero. |
| Top padding | Content starts in the upper half of the viewport; desktop top padding at most about 96 px below the nav. |
| Next-content cue | Some part of the next section, or a clear edge, shows that the page continues. No "scroll" label. |
| Viewport units | Use `min-height: 100svh` or `100dvh`, not `100vh`, on mobile heroes. |
| Contrast over imagery | Text over a photo or video uses a scrim, a solid panel or a measured clear area; 4.5:1 for body text, 3:1 for large text. |

Font scale: plan the headline size and the visual size together. A long headline (more than 6 words) next to a large visual starts around 40-56 px at desktop; a 3-5 word headline may go larger. Use `clamp()` so 390 px does not overflow. For italic display words with descenders (g, j, p, q, y), use line-height at least 1.1 and some bottom padding so descenders do not clip.

## 2. Hero paradigms

Choose by brief, not by habit. Centered text over a gradient blob is the default to avoid.

| Paradigm | Composition | Fits | Avoid when |
|---|---|---|---|
| Asymmetric split | Copy on one side, visual on the other, unequal widths | Most products and services | The visual is weak or generic |
| Product-forward | Headline above a large real product screenshot or device | Software with a clear UI | There is no real UI to show |
| Editorial statement | Very large type, no image or one small image | Manifesto, agency, launch announcement | The audience needs to see the product |
| Full-bleed photo | Photo covers the viewport, copy on a scrim or panel | Hospitality, food, travel, lifestyle | The photo is stock-looking or low quality |
| Video or motion background | Muted loop with poster frame, copy over it | Experiences, events | Performance budget is tight; no pause control |
| Live demo | An interactive input or real component is the hero visual | AI tools, calculators, configurators | The demo needs login or is slow |
| Pinned or curtain | Hero stays while the next section slides over it | Brand story pages | The page is conversion-first and short |

Centered layouts are fine for editorial statements, minimal single-column pages and launches where the message is the design. Otherwise prefer left-aligned or split compositions.

## 3. The hero visual

Use the first source that exists:

1. **Supplied brand assets**: product screenshots, photography, illustration. Crop to the hero aspect ratio; do not distort.
2. **Real product UI**: a real screenshot, or an actual small working component embedded in the page.
3. **Generated image**: if the harness offers an image tool and the brief allows it. If the `design-imagery` skill is available, follow it. Keep text out of generated images; real copy stays in HTML.
4. **CSS-built art**: type as image, geometric composition, a pattern or a gradient that follows the art direction (not the default purple-blue glow).
5. **Placeholder slot**: a box at the exact aspect ratio, with visible text such as `Hero image 1600 x 1000: product in use` and a `TODO` comment. List it in the report.

Do not build a fake dashboard, terminal or task list from styled boxes. Do not overlay tags, fake captions or "Plate 03" labels on images.

## 4. Art direction for the hero

Write a 3-line direction before you build:

```
Mood:      <what the visitor should feel in the first 3 seconds>
Material:  <the visual vocabulary: type, photo, texture, color, depth>
Restraint: <the one thing this hero will not do>
```

Pick an aesthetic family from the brief. If the `art-direction` skill is available, use its catalog. A short palette of families to choose from:

| Family | Signals | Good for |
|---|---|---|
| Swiss / systematic | Grid, strong type scale, flat color, few images | Dev tools, B2B, public sector |
| Editorial | Large type contrast, generous margins, photography | Media, brands, agencies, luxury |
| Warm minimal | Natural tones, soft type, real photography, calm space | Wellness, home, hospitality |
| Bold / brutalist | Heavy type, hard edges, raw color, visible structure | Creative tools, youth brands, events |
| Tech dark | Dark theme, one bright accent, product glow kept small | Developer and AI products |
| Playful | Rounded shapes, bright palette, illustration, bouncy motion | Consumer apps, education, kids |
| Corporate trust | Navy or neutral base, clear hierarchy, certifications | Finance, legal, insurance, health |

Keep one accent color for the whole page. Avoid the defaults: purple-to-blue gradients, centered hero over a dark mesh, glass cards everywhere, neon glows, pure `#000` backgrounds.

## 5. Banners

Use this part when the brief also asks for social, ad or website banners from the same campaign.

### Workflow

1. Confirm the purpose, platform and size, the copy (headline, subtext, CTA, logo) and the brand assets.
2. Choose 2-3 art directions that differ in composition, not only in color.
3. Build each banner as an HTML element at the exact pixel size. The headline, CTA and logo stay HTML text over the visual.
4. Screenshot the element at the exact size if a browser tool is available. Otherwise deliver the HTML and mark the export as pending.
5. Check the exported pixel size, the safe zone, font loading and file size.
6. Name files `<style>-<width>x<height>.png` in one campaign folder, for example `assets/banners/<campaign>/`.

### Banner rules

- Keep critical content in the central 70-80% of the canvas, and at least 50 px from the edges.
- One CTA, high contrast, at least 44 px high, an action verb.
- At most 2 typefaces. Headline at least 32 px, body at least 16 px on digital banners.
- At most 7 words per line and 3 lines for ads. Keep text to a small share of ad area.
- Layout zones: logo or value prop at the top, message and visual in the middle, CTA at the bottom.
- Print: 300 DPI (150 DPI for very large formats), 3-5 mm bleed, CMYK handled by the print workflow.

### Common sizes (px)

| Platform | Use | Size | Ratio |
|---|---|---|---|
| Website | Full-width hero | 1920 x 600-1080 | about 3:1 to 16:9 |
| Website | Section banner | 1200 x 400 | 3:1 |
| Website | Blog or share image | 1200 x 628 | about 1.91:1 |
| Email | Header | 600 x 200 | 3:1 |
| X (Twitter) | Header | 1500 x 500 | 3:1 |
| LinkedIn | Personal banner | 1584 x 396 | 4:1 |
| LinkedIn | Company cover | 1128 x 191 | about 6:1 |
| Facebook | Cover (desktop) | 820 x 312 | about 2.6:1 |
| YouTube | Channel art | 2560 x 1440, safe area 1546 x 423 | 16:9 |
| Instagram | Post | 1080 x 1080 | 1:1 |
| Instagram | Story | 1080 x 1920 | 9:16 |
| Pinterest | Pin | 1000 x 1500 | 2:3 |
| Display ad | Medium rectangle | 300 x 250 | 6:5 |
| Display ad | Leaderboard | 728 x 90 | about 8:1 |
| Display ad | Half page | 300 x 600 | 1:2 |
| Display ad | Billboard | 970 x 250 | about 3.9:1 |
| Display ad | Mobile banner | 320 x 50 and 320 x 100 | 6.4:1, 3.2:1 |

Platform sizes change. If the brief names a platform spec, the brief wins.

### Banner styles

Minimalist, bold typography, gradient wash, full-bleed photo, illustrated, geometric, retro, duotone, editorial grid, collage, data-led (a real number as the hero), dark moody, flat color, organic, kinetic (built for animation). Choose by purpose: data-led and bold type for announcements, photo-based for lifestyle and products, editorial for media and luxury, geometric and minimal for software.
