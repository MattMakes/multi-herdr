# Midnight glass

A dark instrument panel lit from within. Near-black depth, one luminous accent, thin light hairlines, and glass only where layers overlap. The risk of this direction is cliche; the contract must name what makes this project different.

## Fits / avoid

- Fits: AI products, developer platforms, SaaS with a technical audience, launch pages for software.
- Avoid: briefs that already look like every AI landing page; warm or human-centered brands; long reading.

## Signature moves

1. One light source: a single radial glow or mesh behind the focal point, not orbs everywhere.
2. Hairline structure: 1 px borders at white 8 to 12 percent, panels slightly lighter than the ground.
3. A real product surface as the hero object, cropped and lit, never a generic tilted screenshot.

## Tokens

```css
:root {
  --color-bg: #0B0D12;
  --color-surface: #12161E;
  --color-raised: #1A1F29;
  --color-text: #E6EDF3;        /* off-white body, 16.4:1 on bg */
  --color-text-muted: #8B949E;  /* 6.3:1 on bg */
  --color-border: rgb(255 255 255 / 0.10);
  --color-accent: #6CB6FF;      /* electric blue; or emerald #34D399 - one only */
  --font-display: "Geist", "Clash Grotesk", "Satoshi", sans-serif;
  --font-body: "Geist", "Satoshi", system-ui, sans-serif;
  --font-mono: "Geist Mono", "JetBrains Mono", ui-monospace, monospace;
  --type-ratio: 1.414;
  --radius: 12px;
  --border: 1px solid var(--color-border);
  --shadow: 0 0 0 1px var(--color-border), 0 20px 50px -20px rgb(0 0 0 / 0.6);
  --glow: radial-gradient(40% 40% at 50% 30%, rgb(108 182 255 / 0.18), transparent 70%);
  --dur-ui: 180ms;
  --dur-scene: 700ms;
  --ease-enter: cubic-bezier(0.16, 1, 0.3, 1);
}
```

## Type

- Display: geometric or grotesk sans, weight 500 to 600, `letter-spacing: -0.03em`. Headlines may be pure white; body stays off-white.
- Body: 15 to 17 px, `line-height: 1.6`.
- Code and data: mono, the accent color only for one highlighted token per block.

## Layout

- Centered statement plus a large product surface below, or an editorial split; bento grids only with real product content in each cell.
- Glass (`backdrop-filter: blur(16px)` with a translucent surface) only on fixed nav and on overlays.
- Composition families: layered, symmetric, sticky.

## Imagery

Product UI rendered crisp at 1x and 2x, abstract light, 3D renders with a single key light. No stock photos of people at laptops.

## Motion character

Precise and quick. Entrances: push in for the hero product, rack focus for text, stagger cut for grids. One heavy interaction at most: a pinned scroll scene that steps through the product, or a pointer-reactive light. Hover: border brightens to white 20 percent, 180 ms.

## Storyboard defaults

- Hero posture: statement and one action above, lit product surface below, glow behind the product only.
- Arc: promise, tutorial, deep dive, evidence wall, authority, mission, farewell.
- Density cadence: spectacle, medium, dense, medium, quiet.

## Do not

- No more than 1 glow source per view and no purple-to-blue gradient text.
- No pure white body text, no pure black ground.
- No glass on scrolling cards.
- No fake dashboards with invented numbers; use real or clearly sample data.

## Checks

- Count `radial-gradient` and `conic-gradient` per page; flag more than 2.
- `rg -n 'backdrop-filter' src/` hits only fixed or overlay elements.
- Grayscale wireframe test against the three most recent AI landing pages the team built.
