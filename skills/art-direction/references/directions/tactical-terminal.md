# Tactical terminal

A declassified console. Dark ground, white phosphor text, monospace everywhere, data packed tight, framed by brackets and crosshairs. One warning color. Dark ground only; for the light print variant use `swiss-industrial`.

## Fits / avoid

- Fits: developer tools, CLIs, infrastructure, observability, security products, technical changelogs.
- Avoid: consumer audiences who do not read code, brands that must feel warm.

## Signature moves

1. Monospace as the primary voice, including headlines; a heavy grotesk only for the single largest statement.
2. High-density tabular data in bordered compartments, next to wide empty fields.
3. Simulated hardware: faint scanlines, phosphor glow on one element, a blinking cursor.

## Tokens

```css
:root {
  --color-bg: #0C0C0C;          /* never pure #000 */
  --color-surface: #141414;
  --color-text: #E5E5E5;        /* phosphor white, 15.5:1 on bg */
  --color-text-muted: #8A8A8A;  /* 5.7:1 on bg */
  --color-border: #262626;      /* decorative rules; input borders use #6B6B6B (3.6:1) */
  --color-accent: #F59E0B;      /* amber; or hazard red #FF3B30 - choose one */
  --color-signal: #4AF626;      /* optional, one element only (a status readout) */
  --font-display: "JetBrains Mono", "IBM Plex Mono", ui-monospace, monospace;
  --font-body: "JetBrains Mono", "IBM Plex Mono", ui-monospace, monospace;
  --font-mono: var(--font-body);
  --type-ratio: 1.25;
  --space-unit: 4px;            /* tight: 6 / 12 / 20 / 28 */
  --radius: 0;
  --border: 1px solid var(--color-border);
  --shadow: none;
  --dur-ui: 120ms;
  --dur-scene: 400ms;
  --ease-enter: steps(6, end); /* for typing and boot sequences */
}
```

## Type

- Body and data: 13 to 15 px mono, `line-height: 1.4` for data rows and 1.5 for prose.
- Labels: 10 to 12 px, uppercase, `letter-spacing: 0.08em`.
- One display statement per page may use a heavy grotesk at large size, uppercase, tight tracking.

## Layout

- Grid of compartments with 1 px borders; `gap: 1px` on a bordered background draws the lines.
- Frame data points with ASCII: `[ STATUS ]`, `< 200 OK >`, `>>>`. Crosshairs `+` at intersections.
- Real terminal output (commands, logs) is content, not decoration. Use `<samp>`, `<kbd>`, `<output>`.
- Composition families: split, sticky, typographic.

## Imagery

Mostly none. Diagrams as ASCII or thin line art in the text color. If photos are needed: grayscale, dithered, low brightness.

## Motion character

Instant or stepped. Entrances: smash cut, stagger cut (lines appearing like a log), typing with `steps()`. Hover: invert the row (text color becomes ground). Scanlines: `repeating-linear-gradient(0deg, transparent 0 2px, rgb(0 0 0 / 0.12) 2px 4px)` on a fixed, `pointer-events: none` layer. Under reduced motion: no typing, no cursor blink.

## Storyboard defaults

- Hero posture: a live-looking terminal session that states the product in commands and output, beside one large statement.
- Arc: cold open, tutorial, data bombardment, deep dive, authority, mission, black screen.
- Density cadence: dense, dense, quiet, dense.

## Do not

- No rounded corners, gradients, glass or soft shadows.
- Terminal green is never a general text color.
- No fake code that would not run; use real commands from the product.
- No glow on more than one element per view.

## Checks

- `rg -n 'border-radius:\s*[1-9]' src/` returns nothing.
- Muted text and input borders meet the floors in `perception.md`.
- Each code sample in the page is copied from working product documentation.
