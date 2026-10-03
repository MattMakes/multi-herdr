# Color

Generated UI fails on color first: one saturated blue or purple everywhere, pure black and white, a purple-to-cyan gradient, accent on a third of the page.

## Rules

- **Use OKLCH for new palettes.** Lightness is perceptual, so tints and contrast are predictable. Keep the project's format if it already has one.
- **One accent.** Two at most, each with a stated role. The accent marks: the primary action, the active nav item, links, focus, a small mark beside a heading. It does not fill sections or giant buttons. About 5% of any view.
- **No pure extremes.** No `#000` or `#fff` as base surface or body text. Tint toward the anchor hue.
- **Tinted neutrals, one family.** Every gray carries chroma 0.005 to 0.015 at the anchor hue. A warm accent with cool grays reads wrong.
- **Text sets its own surface.** Any rule that changes `background` also sets `color`. Accent fills use an `--color-accent-ink` token checked for contrast.
- **Lock the theme.** One light or dark theme per page. A single deliberate full-bleed band is allowed when it is part of the system and its text flips with it.
- **Opaque tokens.** A named color token is opaque. Alpha is for overlays and shadows only.
- **Two stops.** Gradients have two stops in one hue family. Never on text. Never purple to blue, purple to pink, cyan to magenta, or orange to pink.
- **Never color alone.** Red and green status needs a label or icon too.

## Palette construction

Build in this order.

1. **Accent.** The brand color, converted to OKLCH, chroma 0.12 to 0.20. Without a brand color, derive the hue from the tone: warm 30 to 60, technical 220 to 250, botanical 130 to 160, night or neon 280 to 320, sunlit 60 to 80.
2. **Paper** (base surface). Light: L 95 to 98%, chroma 0.005 to 0.015 at the anchor hue. Dark: L 12 to 18%. A clinical or technical brand may go to L 99 to 100% if the grays carry the chroma.
3. **Paper-2, paper-3** (raised surfaces). Step 2 to 4% L from paper. In dark mode, higher surfaces are lighter.
4. **Ink** (body text). Light paper: L 16 to 24%. Dark paper: L 88 to 96%. Tinted.
5. **Ink-2, muted, rule** (secondary text, de-emphasized text, dividers). Step 6 to 10% L between paper and ink. Muted text must still pass 4.5:1 on every surface it sits on.
6. **Focus.** Accent hue, chroma 0.18 to 0.22, 3:1 or more against the element and the page.
7. **Accent-ink.** Text on an accent fill: ink if accent L > 50%, paper otherwise. Verify 4.5:1.
8. **Status.** Error, warning, success at moderate chroma, each with accessible text pairs.

```css
:root {
  --color-paper:      oklch(97% 0.010 80);
  --color-paper-2:    oklch(94% 0.012 80);
  --color-rule:       oklch(82% 0.010 80);   /* dividers only */
  --color-border:     oklch(62% 0.010 80);   /* input borders: 3:1 on paper */
  --color-muted:      oklch(48% 0.010 75);
  --color-ink:        oklch(20% 0.012 70);
  --color-accent:     oklch(66% 0.16 40);
  --color-accent-ink: oklch(20% 0.012 70);
  --color-focus:      oklch(56% 0.20 40);
  --color-error:      oklch(55% 0.19 25);
}
```

Checked: ink on paper 16.6:1, muted on paper-2 5.5:1, ink on accent 5.5:1, border on paper 3.3:1. Recheck every pair after any change.

## Contrast targets

| Content | Minimum (WCAG 2.x) | Target |
| --- | --- | --- |
| Body text, placeholder, helper text | 4.5:1 | 7:1 |
| Large text (24 px, or 18.66 px bold) | 3:1 | 4.5:1 |
| Icons, input borders, focus rings, UI boundaries | 3:1 | 4.5:1 |

The procedure is in `rubric.md`. The cases that fail most: muted text on a raised surface, text inherited into a dark section, white text on a light accent, a focus ring the same color as the button it surrounds.

## Dark mode

- Paper L 12 to 18%, ink L 88 to 96%. Never `#000` or `#fff`.
- Keep the hue. Only lightness and chroma move between modes.
- Accent: chroma down 0.02 to 0.04, L up 5 to 10%.
- Elevation by lightness (about +3% L per level), not by shadow. No glows.
- Body weight may drop one step (400 to 350) for light text on dark.
- Define both modes as token swaps (`[data-theme="dark"]` or `prefers-color-scheme`). Check both modes before handoff when the product has both. Do not add a second mode the brief did not ask for; do leave the tokens ready for it.

## Palette families to avoid as a default

- **The AI gradient:** purple, violet, or indigo into blue, cyan, or pink; neon glows on dark.
- **The "premium" default:** warm cream or bone paper (`#f5f1ea`, `#f7f5f1`, `#efeae0`, `#faf7f1`), with brass, clay, ochre, or oxblood accents (`#b08947`, `#b6553a`, `#9a2436`) and espresso text (`#1a1714`). Every generated "artisan" or "luxury" site uses it.
- **Slate-900 plus one blue:** the untouched framework default.

These pass only when the brand names them. Otherwise rotate to a different family: cold monochrome with one saturated pop, forest green with bone and amber, terracotta with slate, cobalt with a single neutral, olive with brick.

## Shadows

Shadows are rare. When used: one level, tinted to the surface hue (`0 1px 2px oklch(20% 0.01 <hue> / 0.06)`), consistent light direction. Never stacked colored glows. On dark surfaces, use a lighter surface instead.
