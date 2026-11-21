# Painterly renderers

All code runs in the scene's `__seekToFrame` path and draws on a Canvas 2D context. `hash` is any integer hash that returns a number in `[0, 1)`; seed everything from it.

## The 2-pass method

**Pass 1, the scene.** Draw the block-in to 3 offscreen canvases of the output size:

| Map | Holds | Read as |
|---|---|---|
| Colour | flat fills and simple gradients | the paint to pick up |
| Region id | 1 flat value per object | where a contour belongs |
| Direction | an angle per region, or a function of position | which way a stroke runs |

Read the 3 maps once per stepped frame with `getImageData` (create those contexts with `willReadFrequently`).

**Pass 2, the medium.** Lay a grid over the frame. In each cell, draw 1 stroke:

```js
function strokeCell(ctx, maps, cx, cy, pass, seed) {
  const jx = (hash(cx, cy, pass, seed) - 0.5) * pass.cell;
  const jy = (hash(cy, cx, pass, seed + 1) - 0.5) * pass.cell;
  const x = cx * pass.cell + jx, y = cy * pass.cell + jy;
  const angle = maps.direction(x, y) + (hash(cx, cy, 7, seed) - 0.5) * pass.spread;
  const colour = maps.colour(x, y);
  ctx.save();
  ctx.translate(x, y);
  ctx.rotate(angle);
  ctx.fillStyle = colour;
  ctx.beginPath();
  ctx.ellipse(0, 0, pass.length / 2, pass.width / 2, 0, 0, Math.PI * 2);
  ctx.fill();
  ctx.restore();
}
```

- Run 2 or 3 passes from coarse to fine. Run the fine pass only where the colour map has detail.
- The jitter depends on the cell, the pass and the seed, not on time. So a stroke stays where it is while the colour under it changes.
- Draw a contour only where the region id differs across the stroke's normal. Then no outline shows through an object in front.
- For a bristle look, split each stroke into 3 to 6 thin lines with staggered ends, with a light edge on 1 side and a dark edge on the other.
- Step the paint time to 12 per second. Let the camera transform use the true time.

## Recipes by style

| Style | Marks | Numbers to start with | What moves |
|---|---|---|---|
| Flow-field strokes (post-impressionist) | long strokes along a direction field: walls vertical, floors to the vanishing point, sky along swirl tangents; a darker outline per stroke | general: cell 10, length 22, width 6.5; walls: cell 14, length 64, width 8; outline mixed 40 % toward deep blue | the swirl centres drift; the field turns slowly |
| Impressionist dabs | short ellipses at fixed places, with warm and cool offsets of the local colour | cell 8 to 12; 2 dabs per cell | light: the warm and cool balance shifts across the frame |
| Pointillism | equal dots of about 20 pure colours side by side, in the proportion that mixes to the local colour; complement dots in shadow | dot 5 to 7 px on a jittered grid | the proportions shift; dots do not move |
| Ink wash | paper first, then only added ink, in 5 tones (0.9, 0.8, 0.6, 0.35, 0.15 of black); a wash is a blurred layer multiplied on paper | resample paths to 4 px; dry-brush gaps grow toward the stroke's end | strokes write on; mist drifts; a wash blooms from its centre |
| Watercolour | transparent layers with a darker edge where a wash stops; visible paper grain | 3 to 5 layers, each at 20 to 40 % alpha | washes spread from wet edges; pigment settles |
| Woodblock print | flat colour areas with a key line; the colour sits off the line by about (3, 2) px; repeating pattern fills | 4 to 6 colours | layers slide; waves cycle through 3 or 4 held shapes |
| Mosaic | tiles laid along contour lines of each shape, computed once; grout between | tiles 8 to 28 % larger than half their spacing | colour flows through fixed tiles; a sweep of light crosses |
| Cut paper | flat shapes with a soft shadow; edges slightly uneven | shadow 4 to 8 px, 20 % black | pieces hinge and slide on a 12 fps grid |
| Pop halftone | flat colour with dot patterns at 45 degrees, thick black outlines | pattern from a small tile, not from single dots (about 10 times faster) | dot size pulses; panels slide |
| Cubist facets | overlapping semi-transparent planes with gradients; several views of 1 object | 12 to 30 planes | planes turn and re-order |
| Chiaroscuro | a bright version multiplied by a light map; most of the frame near black | 1 light source | the light moves; the scene holds |
| Soft realism (sfumato) | a blurred layer under a sharp layer at partial alpha; fine cracks; a warm varnish | blur 4 to 8 px | almost nothing: a slow light change |
| Gold leaf, illumination | flat colour, embossed lines offset by 1 px, a highlight only on gold areas | | a sheen crosses the gold |
| Nouveau line | variable-width outlines: offset both sides of a centre line, thick outside and thin inside; flat fills, ornament borders | | hair and cloth curves flow; borders grow |
| 8-bit | draw at 240 by 135 with about 14 colours and 1 px black outlines; scale up with nearest-neighbour | | whole-pixel moves at 15 to 30 changes per second |
| Tape and neon (vaporwave) | colour split, scan lines, a tracking band | split 1 to 2 px | the band rolls; the grid scrolls |
| Cel with light (anime sky) | flat cel colour; a shadow shape is the part minus a shifted copy of itself; strong light shafts | | clouds drift; light flares |
| Rubber hose | thick outlines, limbs as curves with no joints | 12 changes per second | everything bounces to 1 beat |
| Shadow puppet | flat dark silhouettes with cut patterns behind a warm screen | 10 changes per second | joints swing; the light flickers |

## Ink strokes in detail

A brush stroke is a ribbon whose half-width follows pressure along the stroke, where `q` runs from 0 at the start to 1 at the end:

```js
function inkPressure(q, head, tail, noise) {
  const rise = smoothstep(0, head, q);
  const fall = 1 - 0.7 * smoothstep(1 - tail, 1, q);
  return rise * fall * (0.85 + 0.15 * noise);
}
```

- The wet core narrows toward the end as the brush dries.
- Draw each bristle as 1 continuous line with flat caps. Round caps on each segment make a stroke look like bamboo joints.
- Break a bristle where a noise value falls under a dryness threshold that grows with `q`. That gives the flying-white gaps.
- Cut white areas out of the ink layer. Under a figure, knock the wash out first, so that a multiplied wash does not show through it.
- Write-on: reveal a stroke with `smoothstep(delay, delay + length, localTime / 0.45)`.

## Warps and light

- A whole-frame wave (an expressionist sky): shift each row by a sine of its `y` and the stepped time. Leave a margin as large as the biggest shift.
- Long shadows: draw the shape again through a shear transform in the shadow colour.
- Hard light patches: draw a bright and a dark version of the scene and clip between them by the light shapes.
- A drip: a round head on a narrowing tail, with profile `(1 - u * u) ** 0.3`.

## Pitfalls

- Clear or reset the context at the start of every frame. State that leaks from the frame before breaks reverse renders.
- Two overlapping parts in 1 path with opposite winding cancel under the non-zero fill rule. Fill each part by itself.
- `putImageData` ignores the current transform. Write to an offscreen canvas and draw that.
- A text glyph in a coarse medium needs strokes at least 2 marks wide, or it dissolves.
- Measuring motion: compare 2 frames 0.45 s apart, and skip the 0.35 s after a cut where the punch is still settling.
- Avoid a blue or purple cast over the whole film unless the palette asks for it: it is the default tint of machine-made images, and viewers read it that way.
