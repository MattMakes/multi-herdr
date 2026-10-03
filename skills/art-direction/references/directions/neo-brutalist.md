# Neo-brutalist

Loud and honest. White ground, thick black borders, hard offset shadows, one electric accent, system-like grotesk type. Structure is visible on purpose. Not the same as `swiss-industrial`: this one is playful and blocky, that one is austere and precise.

## Fits / avoid

- Fits: indie products, creator tools, campaigns, events, developer products with personality, brands that refuse to look like enterprise SaaS.
- Avoid: luxury, healthcare, finance that must feel careful, long reading.

## Signature moves

1. 2 to 3 px solid black borders on every block, with a hard offset shadow (`4px 4px 0` black), no blur.
2. One electric accent used as large flat fills (a whole band, a sticker, a highlighted word), not as thin details.
3. Elements that look physically placed: slight rotations (1 to 3 degrees) on stickers and badges, buttons that drop into their shadow when pressed.

## Tokens

```css
:root {
  --color-bg: #FFFFFF;
  --color-surface: #F7F7F7;
  --color-text: #18181B;
  --color-text-muted: #52525B;  /* 7.7:1 on white */
  --color-border: #18181B;
  --color-accent: #FACC15;      /* yellow fill; text on it stays #18181B (11.6:1) */
  --font-display: "Space Grotesk", "Archivo", system-ui, sans-serif;
  --font-body: "Space Grotesk", system-ui, sans-serif;
  --font-mono: "JetBrains Mono", ui-monospace, monospace;
  --type-ratio: 1.5;
  --radius: 0;                  /* or 8px everywhere; choose one */
  --border: 3px solid var(--color-border);
  --shadow: 4px 4px 0 var(--color-border);
  --shadow-press: 0 0 0 var(--color-border);
  --dur-ui: 120ms;
  --dur-scene: 350ms;
  --ease-enter: cubic-bezier(0.34, 1.56, 0.64, 1); /* small overshoot */
}
```

## Type

- Display: bold grotesk, large, tight tracking, sentence case or uppercase. A highlighted word may sit on an accent block.
- Body: 16 to 18 px, `line-height: 1.55`.
- Labels: mono, uppercase, in bordered tags.

## Layout

- Blocky grids with visible borders, uneven cell sizes, and one cell in full accent.
- Marquee strips with bordered top and bottom are allowed once per page.
- Sticker-like badges overlap block edges.
- Composition families: broken grid, typographic, split.

## Imagery

Flat illustrations with black outlines, cut-out photos with a hard border, screenshots in a bordered frame with the hard shadow. No soft photography.

## Motion character

Snappy and physical. Entrances: smash cut, whip pan, a small overshoot pop for stickers. Press: the element moves by the shadow offset (`translate(4px, 4px)`) and the shadow goes to `--shadow-press`. Hover: accent fill appears instantly.

## Storyboard defaults

- Hero posture: big bordered headline block, a rotated sticker with the key fact, one accent-filled action.
- Arc: cold open, tutorial, montage, community voice, confrontation, mission, farewell.
- Density cadence: dense, spectacle, dense, medium.

## Do not

- No blurred shadows, gradients or translucency.
- No more than 1 accent hue; black, white and gray do the rest.
- No mixed radius values; pick 0 or one value.
- No thin 1 px gray borders.

## Checks

- `rg -n 'box-shadow:[^;]*[0-9]+px [0-9]+px [1-9][0-9]*px' src/` returns nothing (no blur radius).
- `rg -n 'gradient\(|rgba\(' src/` returns nothing.
- Each button has a visible press state.
