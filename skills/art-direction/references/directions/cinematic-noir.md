# Cinematic noir

Hard light, deep shadow, one color that matters. Source temperament: classic film noir (Billy Wilder's Double Indemnity, Carol Reed's The Third Man) and its modern single-color descendants (Robert Rodriguez's Sin City, Johnnie To's precise compositions). Tension through what the light does not show.

## Fits / avoid

- Fits: creative agencies, spirits and bars, fashion, crime and mystery media, photographers, premium audio.
- Avoid: friendly consumer products, health, education for children.

## Research (if a web tool is available)

Study the chosen film for: venetian-blind light patterns, diagonal shadows across faces and walls, silhouettes in doorways, the single color (when there is one) and what it marks. Record 3 signature techniques and their web translation.

## Signature moves

1. High-contrast monochrome with one accent color reserved for the single most important element per view.
2. Diagonal light: striped or angled light patterns across sections, shadows that cut the frame.
3. Silhouette and reveal: the subject first appears in shadow, then light reaches it.

## Tokens

```css
:root {
  --color-bg: #0B0B0B;
  --color-surface: #161616;
  --color-text: #EDEDED;        /* 16.8:1 on bg */
  --color-text-muted: #9A9A9A;  /* 7.0:1 on bg */
  --color-border: #2B2B2B;
  --color-accent: #C1121F;      /* the one color, 3.2:1: large text and fills only; alternate brass #B08D57 */
  --blinds: repeating-linear-gradient(-12deg, rgb(255 255 255 / 0.06) 0 18px, transparent 18px 46px);
  --grade: grayscale(0.95) contrast(1.3);
  --font-display: "Playfair Display", "Bodoni Moda", serif;
  --font-body: "IBM Plex Sans", "Inter Tight", sans-serif;
  --font-mono: "Courier Prime", "IBM Plex Mono", monospace;
  --type-ratio: 1.5;
  --radius: 0;
  --shadow: none;
  --dur-ui: 200ms;
  --dur-scene: 900ms;
  --ease-enter: cubic-bezier(0.65, 0, 0.35, 1);
}
```

## Type

- Display: high-contrast serif, large, often italic for one line; or condensed uppercase sans for titles like a poster.
- Body: clean sans, 16 to 17 px.
- Typewriter mono for captions and case details.

## Layout

- Diagonal clash, split by a hard light edge, full-bleed black-and-white image with text in the shadow area, comic-panel grids (for the Sin City variant).
- Precise symmetric frames for calm sections (the Johnnie To variant).
- Composition families: split, full-bleed, symmetric.

## Imagery

Black and white or near-monochrome photography with hard key light, smoke, rain on glass, silhouettes. The accent color appears only in the image or the element that matters.

## Motion character

Tense and controlled. Entrances: venetian blind reveal, fade from black, light sweep (a mask moving across the subject). One heavy interaction: a pointer-driven spotlight that reveals the subject in a dark section (with a static fallback). Hover: light reaches the element (brightness and contrast increase).

## Storyboard defaults

- Arc (default): cold open, encounter, deep dive, pivot, confrontation, mission, black screen.
- Arc (calm precision): establishing shot, encounter, quiet moment, evidence wall, invitation, farewell.
- Forbidden beats: montage, community voice shown as a carousel.
- Density cadence: spectacle, quiet, medium, spectacle, quiet.

## Do not

- No second accent color and no colored backgrounds.
- No rounded corners, no soft drop shadows, no glass.
- No pure white large areas; light is shaped, not flat.
- No spotlight interaction without a reduced-motion and touch fallback.

## Checks

- In a screenshot, accent-colored pixels cover under 5 percent of the view.
- `rg -n 'border-radius:\s*[1-9]' src/` returns nothing.
- The spotlight section is fully readable with JavaScript off.
