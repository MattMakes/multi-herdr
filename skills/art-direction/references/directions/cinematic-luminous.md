# Cinematic luminous

Light connects, and the journey is the destination. Source temperament: Makoto Shinkai (Your Name, Weathering With You) for sky and light, Hayao Miyazaki (My Neighbor Totoro, Spirited Away) for organic warmth and patience. Rushing is violence.

## Fits / avoid

- Fits: climate and energy transition, outdoors and travel, education, community and nonprofit, family products, wellness with optimism.
- Avoid: hard-sell offers, data-heavy products, dark or edgy brands.

## Research (if a web tool is available)

Study the chosen film for: sky gradients at specific times of day, light rays and lens flares, wind in grass and cloth, small everyday objects drawn with care. Record 3 signature techniques and their web translation.

## Signature moves

1. A painted sky as the ground: a vertical gradient that changes with the page's time of day (morning top, dusk at the farewell).
2. Light as the focal device: a soft ray or bloom lands on the subject; everything else is slightly cooler.
3. Organic shapes and gentle drift: rounded clouds, leaves or particles moving slowly on a fixed layer.

## Tokens

```css
:root {
  --sky-top: #3F7FD1;
  --sky-mid: #8CC1EA;
  --sky-low: #F6D9B8;           /* warm horizon */
  --color-bg: #FBF8F1;          /* paper for reading sections */
  --color-surface: #FFFFFF;
  --color-text: #1E2A36;        /* 13.7:1 on bg */
  --color-text-muted: #546372;  /* 5.8:1 on bg */
  --color-border: #DCE4EA;
  --color-accent: #2E7D4F;      /* field green; white text on it 5.0:1 */
  --color-warm: #F2A541;        /* sunlight, decoration only */
  --font-display: "Fraunces", "Recoleta", Georgia, serif;
  --font-body: "Nunito Sans", "Source Sans 3", system-ui, sans-serif;
  --type-ratio: 1.333;
  --radius: 16px;
  --shadow: 0 16px 40px -18px rgb(30 42 54 / 0.25);
  --dur-ui: 250ms;
  --dur-scene: 1000ms;
  --ease-enter: cubic-bezier(0.22, 1, 0.36, 1);
}
```

## Type

- Display: soft serif with warm curves, large, sentence case. White on sky only where contrast passes; otherwise set on paper.
- Body: humanist sans, 17 to 18 px, `line-height: 1.65`.

## Layout

- Parallax depth stage (sky, far hills, near objects, content), z-split sections on paper, a single wide image band between sections.
- Section transitions follow the sky: each band shifts the time of day.
- Composition families: layered, full-bleed, split.

## Imagery

Sky, landscapes and everyday places in warm light; illustrations with painted texture. People small in the landscape or in warm close-ups. Saturated but soft.

## Motion character

Gentle and continuous. Entrances: rack focus, slow push in, light bloom (brightness from 1.4 to 1). One heavy interaction: parallax depth layers or a sky that shifts with scroll. Ambient: slow particles (petals, rain, dust) on a fixed layer at low density; stopped under reduced motion.

## Storyboard defaults

- Arc (default): establishing shot, world exploration, encounter, quiet moment, montage, community voice, invitation, farewell.
- Arc (time split): establishing shot, parallel stories, encounter, pivot, flashback, quiet moment, loop.
- Forbidden beats: cold open, data bombardment, mission, black screen.
- Density cadence: spectacle, quiet, medium, quiet, medium, quiet.

## Do not

- No dark ground for the main experience; dusk only at the farewell.
- No sharp corners or hard shadows.
- No fast motion, no urgency copy, no countdowns.
- No text directly on a busy sky without a contrast check.

## Checks

- Measure contrast of every headline over the sky at its lightest point behind the text.
- Particles: count under 40 on screen at once; none under reduced motion.
- `rg -n -i 'countdown|limited time|hurry' src/` returns nothing.
