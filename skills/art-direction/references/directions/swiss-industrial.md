# Swiss industrial

A 1960s corporate manual printed on unbleached paper. Heavy grotesk type at architectural scale, a visible grid, and one hazard red. Light ground only; for the dark terminal variant use `tactical-terminal`.

## Fits / avoid

- Fits: manufacturing, architecture, engineering firms, data journalism, design studios, portfolios that want rigor.
- Avoid: soft consumer products, wellness, anything that must feel gentle.

## Signature moves

1. Uppercase grotesk headlines at viewport scale (`clamp(4rem, 10vw, 15rem)`), line height 0.85 to 0.95, tight negative tracking.
2. Visible compartments: full-width rules and 1 to 2 px borders that divide the page into numbered zones.
3. Bimodal density: vast empty fields around one numeral or word, next to tight clusters of small uppercase metadata.

## Tokens

```css
:root {
  --color-bg: #F4F4F0;          /* matte paper; #EAE8E3 for alternate bands */
  --color-surface: #EAE8E3;
  --color-text: #111111;        /* carbon ink */
  --color-text-muted: #4A4A46;
  --color-border: #111111;
  --color-accent: #D91818;      /* hazard red: the only accent, 4.65:1 on bg */
  --font-display: "Neue Haas Grotesk Display", "Archivo Black", "Monument Extended", sans-serif;
  --font-body: "Neue Haas Grotesk Text", "Archivo", "Helvetica Neue", sans-serif;
  --font-mono: "IBM Plex Mono", "JetBrains Mono", ui-monospace, monospace;
  --type-ratio: 1.618;
  --radius: 0;
  --border: 1px solid var(--color-border);
  --shadow: none;
  --dur-ui: 150ms;
  --dur-scene: 500ms;
  --ease-enter: cubic-bezier(0.2, 0, 0, 1);
}
```

## Type

- Display: heaviest weight, uppercase, `letter-spacing: -0.03em` to `-0.06em`.
- Metadata: mono, 10 to 13 px, uppercase, `letter-spacing: 0.05em` to `0.1em`. Use it for nav, labels, unit ids, coordinates, dates.
- Contrast serif (optional, rare): one large serif word degraded with a halftone texture, as a texture against the clean sans.

## Layout

- Strict CSS grid; every element sits on a track. Use `gap: 1px` on a grid whose background is the ink color to draw razor-thin dividers.
- Full-width horizontal rules separate operational units. Number the units (`01`, `02`).
- Oversized numerals or letters may bleed off the viewport edge.
- Industrial marks as structure: registration marks, crosshairs `+` at grid intersections, `[ BRACKETED LABELS ]`, `>>>` and `///` as direction markers, revision strings (`REV 2.6`).
- Use semantic tags for data: `<data>`, `<dl>`, `<output>`, `<samp>`.
- Composition families: typographic, asymmetric, split.

## Imagery

Black and white photography with high contrast, or halftone and 1-bit dither treatments (`mix-blend-mode: multiply` over a dot pattern). Diagrams over photos where possible.

## Motion character

Mechanical and brief. Entrances: curtain wipe, stagger cut, smash cut. No easing that overshoots, no floating. Hover: invert colors (ink ground, paper text) instantly or in 150 ms. A global, fixed, low-opacity noise layer is allowed.

## Storyboard defaults

- Hero posture: one giant uppercase word or numeral against an empty field, with a dense metadata strip at the bottom edge.
- Arc: cold open, data bombardment, deep dive, evidence wall, authority, mission, black screen.
- Density cadence: spectacle, dense, dense, quiet, dense.

## Do not

- No `border-radius` above 0, no gradients, no soft shadows, no translucency.
- No second accent color. Red is for strike-throughs, structural rules and vital data only.
- No mixing light and dark grounds within the site.
- No centered body text.

## Checks

- `rg -n 'border-radius:\s*[1-9]' src/` returns nothing.
- `rg -n -i 'rgba\(|gradient\(|blur\(' src/` returns only the noise layer.
- Count accent uses per page; flag more than 5.
