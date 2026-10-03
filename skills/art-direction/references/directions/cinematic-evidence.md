# Cinematic evidence

Every detail is evidence and nothing is accidental. Source temperament: David Fincher (Zodiac, The Social Network, Gone Girl, Mindhunter), with Errol Morris's documentary evidence layouts as an alternate. Clinical precision, cold light, the truth in the metadata.

## Fits / avoid

- Fits: fintech, legal and compliance, investigations and journalism, security, analytics, research products.
- Avoid: warm consumer brands, wellness, anything playful.

## Research (if a web tool is available)

Study the chosen film for: the teal-and-sodium grade, locked-off precise framing, documents and screens as story objects, how information is revealed in exact doses. Record 3 signature techniques and their web translation.

## Signature moves

1. The evidence wall: documents, numbers and images pinned in a precise grid with labels, case numbers and timestamps.
2. Desaturated teal-green ground with one sodium-yellow highlight that marks the single important fact.
3. Exact doses: each section reveals one fact, annotated, with its source.

## Tokens

```css
:root {
  --color-bg: #0F1514;          /* teal black */
  --color-surface: #18201F;
  --color-text: #DCE3DF;        /* 14.1:1 on bg */
  --color-text-muted: #8C9A95;  /* 6.3:1 on bg */
  --color-border: #2A3634;
  --color-accent: #E3B341;      /* sodium yellow: one fact per view */
  --color-alert: #D9534F;
  --grade: saturate(0.55) contrast(1.2) brightness(0.9) hue-rotate(-8deg);
  --font-display: "Neue Haas Grotesk Display", "Inter Tight", sans-serif;
  --font-body: "Source Serif 4", "Tiempos Text", Georgia, serif;
  --font-mono: "IBM Plex Mono", ui-monospace, monospace;
  --type-ratio: 1.333;
  --radius: 2px;
  --border: 1px solid var(--color-border);
  --shadow: none;
  --dur-ui: 150ms;
  --dur-scene: 600ms;
  --ease-enter: cubic-bezier(0.4, 0, 0.2, 1);
}
```

Light variant (case file): `--color-bg: #EEF0EC`, `--color-text: #141A19`, accent `#8A6400`.

## Type

- Display: tight neutral grotesk, medium weight, sentence case, precise.
- Body: a serif for reading, like a report.
- Metadata: mono, small, everywhere: case numbers, timestamps, file names, sources.

## Layout

- Evidence wall (masonry of documents with labels), sticky rail with a scrolling dossier, annotated figure with numbered callouts, comparison tables.
- Precise alignment; every element has a label.
- Composition families: sticky, split, broken grid (ordered).

## Imagery

Documents, screens, tight crops of hands and objects, all graded cold and desaturated. Charts are first-class images: thin lines, one highlighted series.

## Motion character

Clinical. Entrances: stagger cut for evidence items, curtain wipe for charts drawing in, a morph from thumbnail to full document. Showy reveal allowed: a redaction bar that slides off a key line. Hover: a document lifts 2 px and its label highlights in sodium yellow.

## Storyboard defaults

- Arc (default): cold open, evidence wall, deep dive, data bombardment, confrontation, mission, black screen.
- Arc (data story): open from the end, data bombardment, flashback, evidence wall, authority, black screen.
- Forbidden beats: establishing shot, world exploration, invitation.
- Density cadence: close-up, dense, dense, close-up, dense.

## Do not

- No warm colors except the sodium accent; no gradients except the grade on images.
- No unlabeled number or chart: each has a source line.
- No playful motion (overshoot, bounce).
- No invented statistics; mark sample data as sample.

## Checks

- `rg -n 'cubic-bezier\([^)]*1\.[0-9]' src/` returns nothing (no overshoot).
- Each number on the page has a source line within its group.
- Accent appears on at most 1 fact per view in screenshots.
