# Editorial minimal

A calm document on warm paper. Type does the work; color is scarce and means something. Restraint is the character, so one quiet entrance is allowed across the page.

## Fits / avoid

- Fits: productivity and writing tools, studios, consultancies, considered B2B, documentation-led products, personal sites.
- Avoid: products that sell spectacle or energy (games, events), briefs that ask for "bold" or "loud".

## Signature moves

1. An editorial serif headline at large size with tight tracking above a quiet sans body.
2. Macro white space: section padding of 96 to 128 px on desktop, content held to a 720 to 960 px column.
3. Hairline structure: 1 px rules and borders in a very light warm gray, no shadows.

## Tokens

```css
:root {
  --color-bg: #F7F6F3;          /* warm bone; #FFFFFF for surfaces */
  --color-surface: #FFFFFF;
  --color-text: #1F1E1C;        /* off-black, never #000 */
  --color-text-muted: #6B6A66;  /* 5.0:1 on --color-bg */
  --color-border: #E6E4DF;
  --color-accent: #1F1E1C;      /* the accent is ink; color only in tags */
  --tag-red-bg: #FDEBEC;   --tag-red-fg: #9F2F2D;
  --tag-blue-bg: #E1F3FE;  --tag-blue-fg: #1F6C9F;
  --tag-green-bg: #EDF3EC; --tag-green-fg: #346538;
  --tag-yellow-bg: #FBF3DB;--tag-yellow-fg: #855900;
  --font-display: "Newsreader", "Instrument Serif", "Lyon Text", Georgia, serif;
  --font-body: "Geist", "Switzer", "Helvetica Neue", system-ui, sans-serif;
  --font-mono: "Geist Mono", "JetBrains Mono", ui-monospace, monospace;
  --type-ratio: 1.333;
  --radius: 8px;                /* cards 8 to 12 px; buttons 4 to 6 px */
  --border: 1px solid var(--color-border);
  --shadow: none;               /* hover only: 0 2px 8px rgb(0 0 0 / 0.04) */
  --dur-ui: 200ms;
  --dur-scene: 600ms;
  --ease-enter: cubic-bezier(0.16, 1, 0.3, 1);
}
```

Neutral variant (a blank slate for monochrome brands): `--color-bg: #FAFAFA`, `--color-surface: #FFFFFF`, `--color-text: #0A0A0A`, `--color-text-muted: #737373` (4.7:1 on white), `--color-border: #E5E5E5`, `system-ui` for body.

## Type

- Display: serif, weight 400 to 500, `letter-spacing: -0.02em` to `-0.04em`, `line-height: 1.1`. Sentence case.
- Body: 16 to 18 px, `line-height: 1.6`, `max-width: 65ch`.
- Meta and keystrokes: mono, 12 to 13 px. Render shortcuts as `<kbd>` with a 1 px border and the bone background.

## Layout

- Asymmetric bento grids with 1 px borders and 24 to 40 px padding; vary cell sizes.
- FAQ and lists: no boxes, only a bottom rule per item and a plain `+` / `-` toggle.
- Product mockups sit in a plain window frame with a white top bar.
- Composition families: asymmetric, typographic, sticky.

## Imagery

Desaturated, warm-toned photography with a very light grain; or monochrome line illustrations with one muted pastel shape. No saturated stock photography.

## Motion character

Quiet. One entrance for content blocks: `opacity` plus `translateY(12px)` over 600 ms with `--ease-enter`, staggered 80 ms. Hover: a border darkens or a shadow of opacity 0.04 appears in 200 ms. Press: `scale(0.98)`. No ambient motion except, optionally, one very slow background light drift on a fixed layer.

## Storyboard defaults

- Hero posture: left-aligned serif statement, one supporting line, one primary action, a single real product frame or none.
- Arc: promise, encounter, tutorial, evidence wall, quiet moment, invitation, farewell.
- Density cadence: quiet, medium, dense, quiet.

## Do not

- No gradients, neon, glass panels or heavy shadows (`shadow-lg`, `rgba(0,0,0,0.2)` and up).
- No saturated background blocks for whole sections.
- No pill shapes for cards or primary buttons (tags may be pills).
- No pure black text or pure white page ground in the bone variant.

## Checks

- `rg -n 'gradient\(' src/` returns nothing outside an approved light spot.
- `rg -n 'box-shadow' src/` shows only the hover shadow token.
- Muted text passes 4.5:1 on both `--color-bg` and `--color-surface`.
