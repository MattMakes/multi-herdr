# Layout, space, and hierarchy

Generated pages are caught by structure before color: hero, three cards, CTA band, link-farm footer; everything centered; every gap 24 px. Two pages for two briefs must look like two sites, not two color swaps.

## Hierarchy

- Each view has one primary element. Size, weight, contrast, and position all point at it. A reader names the primary, secondary, and tertiary items within 2 seconds.
- Create depth with scale, weight, and lightness before shadow. Use at most one shadow style.
- Group by proximity: space inside a group is smaller than space between groups (roughly half).
- Align to a small number of edges. Accidental misalignment (a centered narrow heading over a left-aligned wide grid) reads as a mistake; a deliberate break reads as design. Choose on purpose.

## Spacing scale

One scale on a 4 px base, named by role. Every padding, margin, gap, and inset uses it.

```css
:root {
  --space-3xs: 0.125rem; /* 2  */
  --space-2xs: 0.25rem;  /* 4  */
  --space-xs:  0.5rem;   /* 8  */
  --space-sm:  0.75rem;  /* 12 */
  --space-md:  1rem;     /* 16 */
  --space-lg:  1.5rem;   /* 24 */
  --space-xl:  2.5rem;   /* 40 */
  --space-2xl: 4rem;     /* 64 */
  --space-3xl: 6rem;     /* 96 */
  --space-4xl: 9rem;     /* 144 */
}
```

In Tailwind, extend the theme instead of using arbitrary values. Use `gap` for siblings; use `margin` only for optical correction or breaking out of flow.

Vary the rhythm: tighten one section, open another. Sections with identical padding top to bottom read as a template. A section's bottom padding is often larger than its top.

## Density

Choose a density in the design read and hold it:

| Density | Section spacing | Use |
| --- | --- | --- |
| Airy | `--space-3xl` to `--space-4xl` | brand, portfolio, editorial, luxury |
| Medium | `--space-2xl` to `--space-3xl` | most marketing and product pages |
| Dense | `--space-xl` to `--space-2xl`; rules instead of cards; mono or tabular numbers | dashboards, data, docs |

## Layout variety

- **One primary axis per layout.** Left-biased, right-biased, or a deliberate asymmetric grid. Centered is a choice for statements, not a default.
- **Break the grid once.** One element that crosses a column edge (a pull quote, an image, a number) makes a page feel composed.
- **Vary column widths.** `grid-template-columns: 1.2fr 1fr 0.8fr`, or a 12-column grid with different spans, instead of equal thirds.
- **Do not repeat a section layout family.** A page with 8 sections uses at least 4 different families (split, grid, list, full-bleed statement, table, quote, sticky stack, marquee). No more than 2 image-text zigzag sections in a row.
- **Cards only for real objects.** A card is a thing a user can pick, open, or move. Otherwise group with spacing, a rule, or a tinted band. Never a card inside a card.
- **Bento grids:** exactly as many cells as you have content, no empty cells, varied cell sizes, and at least two cells with real visual variation (an image, a chart, a tint). Use `grid-auto-flow: dense` and check the spans interlock.
- **Section heads:** a heading alone is usually enough. A label above it is optional and stacked in the same column. A paragraph under the heading is stacked, at most 65 ch.
- **Long lists (more than 5 items):** group into 2 or 3 clusters, use cards with a visual each, columns, tabs, or a disclosure, instead of one long divided list.

## Hero

- Height follows content: about 60 to 88% of the first view. Not `min-height: 100vh`.
- At 1280 x 800, the headline, one supporting line (20 words or fewer), and the primary action are visible without scrolling.
- At most 4 text elements: an optional label, the headline, the supporting line, the actions (one primary, at most one secondary). Logos, pricing hints, feature bullets, and trust strips go in the next section.
- Bottom padding at least 1.3 times top padding; top padding at most about 6rem on desktop.
- Plan headline size and visual size together (`typography.md`, hero sizing).

## Navigation and footer

- The nav fits on one line at 1024 px and is 80 px tall or less (64 to 72 px typical). It shows the current page.
- Shape the nav and the footer for the site: a two-link bar for a two-destination site, a floating pill, a masthead for editorial, a side rail for long docs, a search-first bar for search-heavy products. Use the four-column link footer only for real hubs.
- One label per intent across nav, hero, and footer ("Start free" everywhere, not three variants).

## Z-index

A named scale; nothing outside it.

```css
:root {
  --z-raised: 10; --z-dropdown: 100; --z-sticky: 200;
  --z-sticky-nav: 300; --z-modal: 400; --z-toast: 500; --z-tooltip: 600;
}
```

A second sticky element under a sticky nav uses `top: var(--nav-height)`, not `top: 0`.

## Responsive floor

Every page works at 320, 375, 414, and 768 px, and at 1280 and 1440 px.

- Mobile-first: base styles for the smallest width, `min-width` queries upward, breakpoints in `rem` where the content breaks.
- `html, body { overflow-x: clip; }` (not `hidden`, which breaks `position: sticky`). This is a safety net; still find the element that overflows.
- Image-bearing grid tracks: `minmax(0, 1fr)`, not `1fr`.
- Display headings: `overflow-wrap: anywhere; min-width: 0`.
- Asymmetric desktop layouts collapse to one column below about 768 px, declared in the same component.
- Clickable labels never wrap: shorten the label first, then `white-space: nowrap`, then collapse the nav into a menu.
- Container: `max-width` about 1200 to 1440 px with fluid side padding, `padding-inline: clamp(1rem, 4vw, 4rem)`.
- `dvh` or `svh` for full-height sections; never `100vh` or `100vw`.
- Hover effects inside `@media (hover: hover)`; touch has an equivalent.
- `width` and `height` attributes on every image; `srcset` for responsive images.
- Use logical properties (`margin-inline`, `padding-block`) so RTL works; leave 30% slack for longer translations.

## When it looks fine but flat

Do one of these before handoff:

1. Add one element that breaks the grid.
2. Make one column wider than the others.
3. Move the primary action off the center axis.
4. Remove a card and leave the space empty.
5. Change one section's padding so the rhythm is uneven.
