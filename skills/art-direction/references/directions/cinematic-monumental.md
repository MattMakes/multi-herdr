# Cinematic monumental

Scale that humbles, silence that speaks. Source temperament: Denis Villeneuve (Arrival, Blade Runner 2049, Dune), with the long seamless takes of Alfonso Cuaron and the minimal stillness of Jean-Pierre Melville as alternates. The less the page shows, the more the viewer imagines.

## Fits / avoid

- Fits: deep tech, space and energy, architecture, infrastructure, large launches, cultural institutions.
- Avoid: products with many features to list, fast promotional offers, playful brands.

## Research (if a web tool is available)

Study the chosen film for: horizon placement and the ratio of empty field to subject, how fog and dust scale the subject, the length of held shots, the color of shadow. Record 3 signature techniques and their web translation in the contract. Without web access, write "inferred" and work from the notes here.

## Signature moves

1. Massive type or one massive object, small text, nothing else in the frame: 100vh sections with the subject occupying under a third of the area.
2. Atmosphere as depth: a fog or dust gradient between ground and subject, darkest at the edges.
3. Long holds: sections that do almost nothing for a full viewport, then one decisive reveal.

## Tokens

```css
:root {
  --color-bg: #0E0D0B;          /* warm black of a dim interior */
  --color-surface: #1A1815;
  --color-text: #ECE6DC;        /* 15.6:1 on bg */
  --color-text-muted: #9C958A;  /* 6.5:1 on bg */
  --color-border: rgb(236 230 220 / 0.12);
  --color-accent: #C8A26B;      /* desert amber; alternate sodium orange #E07A2E */
  --fog: linear-gradient(to top, #0E0D0B 0%, rgb(14 13 11 / 0) 60%);
  --grade: sepia(0.25) saturate(0.6) contrast(1.15); /* images */
  --font-display: "Monument Extended", "Neue Machina", "Archivo Expanded", sans-serif;
  --font-body: "Suisse Intl", "Inter Tight", system-ui, sans-serif;
  --type-ratio: 1.618;
  --radius: 0;
  --shadow: none;
  --dur-ui: 250ms;
  --dur-scene: 1200ms;
  --ease-enter: cubic-bezier(0.22, 1, 0.36, 1);
}
```

Light variant (Arrival fog): `--color-bg: #D9D6CF`, `--color-text: #1C1B19`, accent `#2F3A3D`.

## Type

- Display: wide, uppercase, very large (`clamp(3rem, 9vw, 11rem)`), wide or neutral tracking; at most 6 words.
- Body: small (15 to 16 px), generous leading, short paragraphs placed far from the headline.

## Layout

- Full-viewport sections; letterbox strips (content in a band with empty space above and below); center-weighted monument; widescreen panorama strips.
- No visible dividers: sections dissolve into each other through the ground color.
- Composition families: full-bleed, symmetric, typographic.

## Imagery

Vast landscapes and architecture with a tiny human for scale, graded warm and desaturated. Full-bleed, 21:9 or wider. No people close up.

## Motion character

Slow and inevitable. Entrances: fade from black, crane down, slow push in on the hero image. One heavy interaction: a pinned scroll scene where scale changes (pull out from a detail to the whole). Hover: opacity change only.

## Storyboard defaults

- Arc (default): establishing shot, quiet moment, deep dive, evidence wall, invitation, farewell.
- Arc (puzzle): promise, deep dive, pivot, flashback, loop.
- Forbidden beats: montage, open from the end, community voice.
- Density cadence: quiet, quiet, medium, spectacle, quiet.

## Do not

- No cards, no grids of features visible as grids, no rounded corners.
- No more than 2 text sizes in the first view.
- No busy hover effects or cursor followers.
- No director names or film titles in visible copy.

## Checks

- First view: the subject occupies less than 35 percent of the frame area in a screenshot.
- `rg -n 'border-radius:\s*[1-9]' src/` returns nothing.
- Every section boundary is a dissolve, not a rule.
