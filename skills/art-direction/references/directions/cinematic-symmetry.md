# Cinematic symmetry

Symmetry as care; every object placed with intention. Source temperament: Wes Anderson (The Grand Budapest Hotel, Moonrise Kingdom, Asteroid City), with Jacques Tati's deadpan precision as an alternate. Whimsy is a serious practice.

## Fits / avoid

- Fits: boutique hotels, crafted consumer goods, bakeries and shops, museums, children's publishing, gift brands.
- Avoid: technical products, urgent or aggressive offers, brands that must feel modern-minimal.

## Research (if a web tool is available)

Study the chosen film for: the planimetric frame (camera square to a flat wall), the pastel palette of each location, chapter title cards and their typography, lateral tracking moves, overhead inserts of arranged objects. Record 3 signature techniques and their web translation.

## Signature moves

1. Centered, planimetric compositions: everything on the vertical axis, framed by symmetric borders.
2. Chapter cards: full-width solid-color title cards that open each part of the page.
3. Overhead inserts: objects arranged in a neat grid or a careful scatter on a flat color.

## Tokens

```css
:root {
  --color-bg: #F6E3DA;          /* hotel pink */
  --color-surface: #FBF1EA;
  --color-text: #3A2230;        /* plum ink, 11.7:1 on bg */
  --color-text-muted: #6E4F5C;  /* 5.8:1 on bg */
  --color-border: #3A2230;
  --color-accent: #A23B31;      /* bellhop red, 5.3:1 on bg; card colors below */
  --card-1: #E9B8A8;  --card-2: #9FB8AD;  --card-3: #E7C873;  --card-4: #8A5A7B;
  --font-display: "Futura PT", "Jost", "Josefin Sans", sans-serif;
  --font-body: "Archer", "Zilla Slab", Georgia, serif;
  --type-ratio: 1.333;
  --radius: 0;                  /* ornamental frames instead of radius */
  --border: 2px solid var(--color-border);
  --frame: double 6px var(--color-border);
  --dur-ui: 250ms;
  --dur-scene: 700ms;
  --ease-enter: cubic-bezier(0.45, 0, 0.55, 1); /* even, metronomic */
}
```

## Type

- Display: geometric sans in uppercase with wide tracking (`0.12em`), centered. Chapter numbers in words ("Part the Second").
- Body: slab or bookish serif, centered for short blocks, left for long ones.
- Labels: small caps, like signage.

## Layout

- Centered stack, bilateral mirror, title card sequence, crab-shot horizontal track of rooms or products.
- Double-rule or ornamental frames around key content; consistent margins on all sides.
- Composition families: symmetric, horizontal, typographic.

## Imagery

Flat-lit, frontal photography of objects and places, pastel backgrounds, objects arranged with order. Illustrations as diagrams or cutaways (a dollhouse section of a building).

## Motion character

Metronomic. Entrances: curtain wipe, iris, lateral crab move. One heavy interaction: a horizontal track that moves room by room as the user scrolls. Hover: a small frame appears around the item, 250 ms.

## Storyboard defaults

- Arc (default): prologue, establishing shot, world exploration, encounter, parallel stories, authority, quiet moment, invitation, farewell.
- Arc (nested): prologue, flashback, world exploration, montage, encounter, community voice, invitation, farewell.
- Forbidden beats: cold open, data bombardment, black screen, confrontation.
- Density cadence: title card, medium, medium, title card, medium (metronomic).

## Do not

- No asymmetric hero, no diagonal layouts, no off-axis scatter in main sections.
- No gradients or glass; color comes in flat fields.
- No more than 4 card colors across the site.
- No ironic or meta copy about the film.

## Checks

- First view: mirror the screenshot horizontally; the composition is the same apart from text.
- Every chapter card uses one of the `--card-*` tokens.
- `rg -n 'gradient\(|backdrop-filter' src/` returns nothing.
