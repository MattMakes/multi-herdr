# The 15 styles

Each entry gives the look, the motion signature and features that can be tested on rendered frames. Colours are starting points: a brand colour of the same role replaces them. `Renderer` names the cheapest route that reaches the look.

## 1. Frame-by-frame with line boil

- Look: warm paper, ink black with 2 or 3 flat colours. A paper texture multiplied at 10 to 15 %. Fills sit slightly off their outlines.
- Frame rate: 24 fps. Animate on 2s; on 1s for fast action; on 3s for holds.
- Motion: each line has 3 or 4 slightly different drawings that cycle every 2 frames. 1 smear frame, stretched to 150 to 300 %, carries a fast move. Drawn smoke, sparks and speed lines.
- Test: a held pose still changes every 2 frames; no move is interpolated between drawings.
- Renderer: html (Canvas). Seat: `video-illustrator`.

## 2. Isometric

- Look: a true isometric view (the camera looks down at 35.264 degrees, turned 45 degrees). Pastel faces with soft shadows.
- Faces from flat art: scale the height to 86.6 %, skew by 30 degrees, rotate by 30 degrees the other way. Draw order follows screen height.
- Motion: buildings grow from the base, walls rise with a small overshoot, the roof lands 2 or 3 frames later. Tiles drop in as a wave from the centre. The camera only slides; it never turns. Layers slide at ratios 1, 0.8 and 0.6.
- Test: every edge keeps the 2 to 1 pixel slope; the camera transform has no rotation.
- Renderer: html (SVG or CSS transforms), or webgl with an orthographic camera.

## 3. Flat vector

- Look: flat colour with no gradients; geometric characters. 4 saturated colours on a cream or deep navy ground.
- Motion: entrances have 3 phases: a small move back, a burst out, a rebound. Overshoot curve `cubic-bezier(0.34, 1.56, 0.64, 1)`. About 10 % squash and stretch. Stagger 2 to 4 frames. 1 subject moves at a time. Limbs rotate around joints.
- Transitions: a shape wipe, or an element that flies out and carries the cut.
- Test: no gradient in any frame; every entrance shows the 3 phases.
- Renderer: html.

## 4. Line art

- Look: 1 continuous line of constant width (2 to 4 px at 1080p) in a pale colour on a dark ground. Round caps. A glow on the pen tip.
- Motion: the line draws by stroke length. The end of each figure is the start of the next. Draw speed slows into corners (`cubic-bezier(0.65, 0, 0.35, 1)`). A fill spreads from the point where the line closed. Older strokes dim to 35 %. The camera follows the tip, then pulls out to show the whole.
- Test: the line never lifts; the width is constant.
- Renderer: html (SVG).

## 5. 3D render

- Look: soft studio 3D: gummy translucent material, glossy plastic, frosted glass. Environment light. Shallow depth of field. Real motion blur.
- Frame rate: 24 fps.
- Motion: a field of cloned shapes ripples as a wave passes. The hero drops in and jiggles to rest. Long-tail easing: 80 % of the travel in the first 20 % of the time.
- Test: shadows and reflections agree with 1 light setup; the blur follows the motion.
- Renderer: blender. Seat: `blender-cinematographer`.

## 6. Shape morph

- Look: 6 to 8 simple icons, 1 per beat, each a single flat shape on a block of colour.
- Motion: A turns into a circle or a capsule, then into B; each half takes 8 to 12 frames. 10 to 15 % squash and a slight turn during the morph. Curve `cubic-bezier(0.7, 0, 0.3, 1)` with 3 frames of rest at each end. The ground colour wipes on each morph.
- Build: both shapes need the same number of points and the same first point.
- Test: the middle frame of each morph has no knot or self-crossing.
- Renderer: html (SVG).

## 7. Sticker explainer

- Look: a very large canvas in a pale grey with 1 accent colour. Photo cut-outs with a white outline of 8 to 12 px and a soft shadow. Flat icons. Exact charts.
- Motion: 1 continuous camera path with long eases. Every sentence of the narration brings 1 new thing into view. Bars grow to scale, numbers count up, routes draw on.
- Type: 1 sans face at 1 weight, strictly aligned.
- Test: the camera never cuts; every chart value matches its source.
- Renderer: html.

## 8. HUD

- Look: near-black teal ground, cyan lines, 1 orange alert colour. A hexagon grid, a wireframe object, glow, a slight colour split, scan lines.
- Motion: lines draw on in order of importance. A radar sweep and concentric rings. Numbers flicker through seeded values for 3 to 5 frames, then settle. Text types with a cursor. Brackets snap from 1.4 to 1.0 around the target. Then everything flips to the alert colour.
- Test: every flicker is the same on 2 renders; the alert flip happens once.
- Renderer: html with a webgl wireframe. Seat for the wireframe: `video-3d-animator`.

## 9. Collage

- Look: photo cut-outs with a white scissor edge of 2 to 4 px, halftone dots, newsprint, tape. Bold diagonal type in red and black.
- Frame rate: rendered at 30 fps on a 12 fps grid; holds on a 6 fps grid.
- Motion: each placement jitters by 2 px. Puppet joints at shoulder, elbow and jaw. The mouth swaps between 2 or 3 drawings. Transitions: an element is slapped on, a page flips, paper tears.
- Layer order: paper, colour blocks, photos, doodles and tape, grain.
- Test: nothing moves smoothly between grid frames; no squash and stretch.
- Renderer: html. Seat: `video-illustrator`.

## 10. Aurora and glass

- Look: deep indigo ground. 3 to 5 blobs of neighbouring hues, blurred by 80 to 150 px, drifting on paths with periods of 20 to 40 s. 3 to 5 % noise against banding. A glass card: background blur 20 to 40 px, a white fill at 5 to 10 %, a 1 px inner stroke at 30 % white, a large radius.
- Motion: text enters with opacity and an 8 px rise only. Sine easing; everything is slow. The hero is a lens that bends the light behind it.
- Test: no banding at full size; no move is faster than the blobs allow.
- Renderer: html. A refracting lens needs webgl.

## 11. Bauhaus

- Look: cream ground, red, yellow, blue and black. A module grid (240 px at 1080p).
- Motion: every move is a whole number of modules and lands on a grid line. 1 action per beat at 120 beats per minute. Rotations pivot in turn on the shape's centre, a corner, and the frame centre. In-out or linear curves; never an overshoot. A phase-shifted cascade at the climax. The end frame is a poster.
- Test: every resting position is on the grid; no curve passes its end value.
- Renderer: html.

## 12. Synthwave

- Look: a perspective grid that scrolls toward the viewer, a banded sun, wireframe mountains. A double glow: tight (about 4 px) and wide (about 30 px, faint). A chrome title with a glint that sweeps across.
- Motion: a neon word writes on with a flicker. The title slams in with a flare.
- Tape finish, in this order: colour split of 1 to 2 px, scan lines, 1 or 2 tracking jitters per second from a seeded list, a rounded 4 to 3 mask.
- Test: the grid loop has no seam; the jitters fall on the same frames in 2 renders.
- Renderer: html, or webgl for the grid.

## 13. Pixel art

- Look: an internal canvas of 320 by 180, scaled by a whole number with nearest-neighbour sampling. 16 to 32 colours with dithered gradients. Sprites of 16 to 24 px.
- Motion: whole-pixel positions only. 3 or 4 parallax layers. A run cycle of 6 to 8 drawings at 10 to 12 fps; breathing in 2 to 4 drawings. Stepped timing only: never a smooth ease on a sprite.
- Finish: a tube-screen pass (scan lines, a slight barrel curve) that does not smear the pixels.
- Test: every pixel edge is sharp at the output size; no sprite sits on a half pixel.
- Renderer: html (Canvas). The `pixel-vfx` skill covers effects in this look.

## 14. Liquid

- Look: a dark ground with glossy liquid in 2 or 3 warm hues, with highlights and a bright rim.
- Build: blobs joined by a smooth minimum in a shader; or the cheap way, blur the shapes by 20 px or more and cut the alpha at a threshold.
- Motion: a neck stretches, snaps, and both ends rebound for 2 or 3 frames. Small drops follow 2 or 3 frames late. A landing squashes. A wave front with 3 to 5 bulges crosses as a transition. Curve `cubic-bezier(0.22, 1, 0.36, 1)`: flung, never even.
- Test: the liquid never moves at constant speed; the logo forms out of the same field.
- Renderer: webgl. Seat: `video-3d-animator`.

## 15. Variety captions

- Look: vertical frame (1080 by 1920). Heavy rounded type in 3 layers: a gradient fill, a thick white stroke, a coloured outer stroke or shadow. Pink, yellow and cyan.
- Motion: a word pops from 0 to 1.15 to 1.0 in about 8 frames, 2 or 3 frames after the sound it marks. Letters bounce with a 2 frame stagger. A slight tilt, and a wiggle while the word holds.
- Decoration: burst lines, sweat drops, sparkles, a stamp. Under it, photos or footage with a slow push, a small shake and punch-ins.
- Test: every caption follows its sound by 2 or 3 frames; text stays out of the platform's interface zones (top 14 %, bottom 25 %, right 14 %).
- Renderer: html.

## Choosing between them

| The film must feel | Try |
|---|---|
| Warm, human, hand-made | 1, 9, the painterly styles of `video-art-motion` |
| Clear and friendly | 3, 6, 7 |
| Precise, technical | 2, 4, 8, 11 |
| Premium, calm | 4, 5, 10 |
| Loud, fast, social | 12, 14, 15 |
| Nostalgic | 12, 13, 1 |

A product demo with real interface captures suits 3, 7, 10 or "house". Styles 1, 9, 13 and 15 change how a capture must be treated; say so in `DIRECTION.md`.
