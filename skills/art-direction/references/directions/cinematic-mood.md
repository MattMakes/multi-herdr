# Cinematic mood

Memory is unreliable and mood is everything. Source temperament: Wong Kar-wai (In the Mood for Love, Chungking Express, Fallen Angels), with Sofia Coppola's quiet loneliness as a softer alternate. Every frame is a painting you are not sure you saw.

## Fits / avoid

- Fits: hospitality, restaurants and bars, fashion, music, perfume, independent publishing, small luxury.
- Avoid: products that must explain how they work; data products; urgent offers.

## Research (if a web tool is available)

Study the chosen film for: saturated color in practical light (lamps, neon, signs), frames inside frames (doorways, corridors, mirrors), step-printed and slowed motion, recurring motifs (a clock, a song, a dress). Record 3 signature techniques and their web translation.

## Signature moves

1. Saturated warm color in deep shadow: red, amber and green lit spots on near-black.
2. Frames inside frames: content seen through a narrow vertical crop, a doorway shape, a mirror split.
3. Crossfades and blur layers: images dissolve into each other; foreground elements pass out of focus.

## Tokens

```css
:root {
  --color-bg: #140B0A;          /* lacquer dark */
  --color-surface: #24120F;
  --color-text: #F2E3D0;        /* warm paper, 15.4:1 on bg */
  --color-text-muted: #B59C84;  /* 7.4:1 on bg */
  --color-border: rgb(242 227 208 / 0.14);
  --color-accent: #C8312B;      /* cheongsam red, 3.6:1: large text and fills only; jade #2F6B4F decoration only */
  --grade: saturate(1.4) contrast(1.15) brightness(0.9) sepia(0.12);
  --font-display: "Cormorant Garamond", "Playfair Display", serif;
  --font-body: "EB Garamond", "Source Serif 4", Georgia, serif;
  --font-label: "Noto Sans TC", "Inter Tight", sans-serif;
  --type-ratio: 1.5;
  --radius: 0;
  --shadow: none;
  --dur-ui: 300ms;
  --dur-scene: 1100ms;
  --ease-enter: cubic-bezier(0.25, 0.1, 0.25, 1);
}
```

## Type

- Display: italic or high-contrast serif, medium size, often set in a narrow column or vertical (`writing-mode: vertical-rl`) for one short line.
- Body: serif, 17 to 19 px, short lines.
- Labels: small sans, wide tracking, like subtitles: bottom-centered, one line.

## Layout

- Mosaic of fragments (irregular tiles of one moment), narrow vertical crops beside wide empty fields, shot/reverse-shot pairs.
- Text sits like a subtitle at the bottom of an image, or alone in darkness.
- Composition families: broken grid, layered, split.

## Imagery

Night interiors and streets, practical light, shallow focus, motion blur, repeated objects. Grade warm and saturated. Mixed aspect ratios, including tall 4:5 and 9:16 crops.

## Motion character

Slow and dreamlike. Entrances: crossfade, rack focus, fade from black. Allowed showy reveal: an image sequence that steps like step-printed film (`steps()` with 3 to 4 frames). Hover: image warms and sharpens (`filter` change over 600 ms). Under reduced motion: instant swaps.

## Storyboard defaults

- Arc (default): establishing shot, encounter, quiet moment, montage, pivot, deep dive, invitation, farewell.
- Arc (restrained): establishing shot, quiet moment, encounter, quiet moment, loop.
- Forbidden beats: data bombardment, tutorial, authority, mission.
- Density cadence: quiet, medium, quiet, spectacle, quiet.

## Do not

- No stats counters, logo walls or feature grids.
- No bright white ground or cool blue palette.
- No fast or bouncy motion.
- No more than 1 saturated accent per view.

## Checks

- First view: in a grayscale screenshot, the brightest area is the focal point and covers under 25 percent of the frame.
- `rg -n -i 'counter|countup|logo-wall' src/` returns nothing.
- Body text contrast holds over every image it overlaps (measure the darkest and lightest pixels behind it).
