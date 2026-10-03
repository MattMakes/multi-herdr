# Soft premium

An expensive object on a light table. Airy surfaces, machined nested containers, shadows so diffuse they read as ambient light, and heavy, physical motion. For the dark glass variant use `midnight-glass`.

## Fits / avoid

- Fits: consumer apps, health and wellness, premium agencies, real estate, lifestyle, portfolio sites.
- Avoid: data-dense tools, technical audiences who want density, brands with a raw voice.

## Signature moves

1. The double bezel: an outer shell (tinted background, hairline ring, 6 to 8 px padding, large radius) holding an inner core with its own background, an inner top highlight, and a concentric smaller radius.
2. Floating island navigation: a pill detached from the top edge, centered, width fit to content.
3. Very large section spacing (96 to 160 px) with one eyebrow label above each major heading.

## Tokens

```css
:root {
  --color-bg: #FDFBF7;          /* warm cream; or silver-white #F4F5F7 */
  --color-surface: #FFFFFF;
  --color-shell: rgb(0 0 0 / 0.04);
  --color-text: #1A1A1A;
  --color-text-muted: #66625C;  /* 5.8:1 on bg */
  --color-border: rgb(0 0 0 / 0.06);
  --color-accent: #3D5A4C;      /* muted sage-green; or deep espresso #3B2A20 */
  --font-display: "PP Editorial New", "Clash Display", "Fraunces", serif;
  --font-body: "Plus Jakarta Sans", "Geist", system-ui, sans-serif;
  --type-ratio: 1.5;
  --radius-outer: 2rem;
  --radius-inner: calc(2rem - 0.375rem);
  --shadow: 0 24px 60px -20px rgb(20 20 20 / 0.10);
  --highlight: inset 0 1px 1px rgb(255 255 255 / 0.6);
  --dur-ui: 300ms;
  --dur-scene: 900ms;
  --ease-enter: cubic-bezier(0.32, 0.72, 0, 1);
}
```

## Type

- Display: a high-contrast variable serif or a wide display grotesk, large, `letter-spacing: -0.02em`.
- Eyebrow: 10 to 11 px, uppercase, `letter-spacing: 0.2em`, in a small pill.
- Body: 16 to 18 px, `line-height: 1.6`.

## Layout

- Asymmetric bento (an 8-column cell beside stacked 4-column cells), or a z-axis cascade of slightly overlapping cards rotated by 2 to 3 degrees, or an editorial split with large type left and scrolling image pills right.
- Primary buttons are full pills; a trailing arrow sits in its own small circle, flush with the inner padding.
- Below 768 px: single column, remove rotations and overlaps, keep the bezel.
- Use `min-height: 100dvh`, not `100vh`, for full-height sections.
- Composition families: asymmetric, layered, split.

## Imagery

Bright, soft natural light, shallow depth of field, objects and people with space around them. A fixed film grain at opacity 0.03 gives a paper feel.

## Motion character

Heavy and fluid, never linear. Entrances: rack focus (blur 8 px plus 24 px rise) over 800 to 900 ms, push in for hero objects. Hover on buttons: press to `scale(0.98)`, the arrow circle moves 1 to 2 px diagonally and scales to 1.05. Menu: the hamburger lines morph into an X; the overlay links rise in a 50 ms stagger. Apply `backdrop-filter` only to fixed elements (nav, overlays).

## Storyboard defaults

- Hero posture: large display line with an eyebrow, one pill action, one beautifully lit object in a bezel.
- Arc: establishing shot, promise, encounter, montage, community voice, quiet moment, invitation, farewell.
- Density cadence: quiet, medium, quiet, medium, quiet.

## Do not

- No harsh shadows (opacity above 0.15) and no gray 1 px borders on cards; use the bezel instead.
- No navigation bar glued edge to edge at the top.
- No `linear` or default `ease-in-out` transitions.
- No blur on scrolling content.

## Checks

- `rg -n 'transition[^;]*(linear|ease-in-out)' src/` returns nothing.
- `rg -n 'backdrop-filter' src/` hits only fixed or sticky elements.
- Inner radius equals outer radius minus shell padding on every bezel.
