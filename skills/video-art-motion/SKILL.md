---
name: video-art-motion
description: Use when a video scene must look painted or hand-drawn and move - a living painting in a named art style (ink wash, watercolour, impressionist dabs, flow-field strokes, pointillism, woodblock, mosaic, cut-paper, pop halftone, chiaroscuro and more), a doodle or sketch that draws itself on, a line-boil loop, a seamless sticker or logo loop, or a film that shows a drawing being made. Covers the 2-pass stroke renderer, stable seeded strokes, stepped time, pen ribbons with pressure and taper, paper and grain, and the checks for flicker, dead air and loop seams.
---

# Art motion: paintings and doodles that move

Two families of scene live here. A **living painting** renders the whole frame in the marks of 1 art style and keeps it alive. A **doodle** is made of pen strokes that draw on, boil and loop. Both are Canvas 2D code, both follow the seek contract of `video-motion`, and both get their timing from `video-motion-craft`.

No image model and no asset download is needed. A stroke is code. Texture is seeded noise. When the operator supplies art (a painting, a character sheet), it enters as 1 more layer.

## When to use

- `DIRECTION.md` names a painterly or hand-drawn style, or the storyboard asks for "a painting that moves", "a sketch that draws itself", "a doodle loop".
- A flat scene looks like a template and the brand allows a hand-made surface.
- A logo or sticker needs a seamless loop.

## Rules that keep it deterministic

1. Every pixel comes from the frame index. Randomness comes from a hash of what the stroke is: `hash(cell, pass, seed)`. A stroke that did not change keeps its place and shape from frame to frame.
2. Step time on purpose. Paint "on 12s" with `Math.floor(t * 12) / 12` while the camera uses the true `t`. Line boil changes every 4 frames at 30 fps. Declare each stepped rate in `MOTION.md`.
3. Avoid `ctx.filter`. Blur by drawing to a smaller canvas and scaling back, or by layered alpha. Where you do use a filter, the forward and reverse sample frames must still be byte-identical.
4. Draw to an offscreen canvas first when you write pixels directly; direct pixel writes ignore the camera transform.
5. A cache is allowed only when its key names everything the pixels depend on. Check a cold render against a warm one.
6. Never scale a bitmap up. Draw at the output size.
7. No mixing of GPU and CPU rasterising in 1 scene; the 2 round differently.

## A living painting

Workflow:

1. **Lock 3 lines** in `MOTION.md` before any code: the 1 style, the palette of 3 to 6 colours, and the character policy (no figures; hands and backs only; or supplied character frames).
2. **Skeleton.** Lay out the scene as flat shapes with a constants table of positions. This block-in is the colour source for the next pass, and it stays the same if the style changes.
3. **Medium pass.** Redraw the whole frame in the style's marks, reading 3 maps from the block-in: colour, region id, and stroke direction per region. `references/painterly-renderers.md` gives the recipe per style.
4. **Small props** on their own layer. **Figures** go through the same medium pass, or they look pasted on. For a coarse medium, lay the original figure over the painted one at partial alpha so that a face stays readable.
5. **Life.** 1 continuous main action that runs on film time and never restarts, plus 2 to 4 small loops taken from the picture's own motifs: water, leaves, a lamp, smoke, cloth.
6. **Camera.** A slow push of 10 to 40 % over 0.6 to 1.5 s, or a drift of 1 to 4 % per second. No more than 1 shot in 3 uses an accent move (snap, slam, whip).
7. **Paper.** A texture fixed to the screen, multiplied on top. It does not move with the camera.

What makes it read as alive and not as flicker: stroke positions stay fixed, and the direction field, the colour and the light change slowly under them. Reseeding the whole frame several times per second reads as noise.

## A doodle

`references/doodle-lines.md` holds the pen model, the boil, draw-on and the loop rules. In short:

- A line is a ribbon polygon around a smoothed path, with taper at both ends and a pressure curve. It is not a canvas stroke of constant width.
- Static wobble moves the control points once, by about 1 px. Boil adds a second jitter of about 0.5 px that changes every 4 frames.
- Draw-on cuts the ribbon along its centre line, so the moving tip keeps its taper.
- Marks appear in the order a hand would make them. Long strokes go fast, fiddly ones slow. A rest is at most 0.5 s.
- A loop is driven by `sin` and `cos` of whole cycles of `t / duration`, so the last frame leads into the first.

## Shot rules for an art film

- A shot lasts 2 to 6 s. The first 3 s show a concrete small scene, not a title. The last shot is not a text card.
- Each row of the shot list names a concrete noun, a verb, a camera move, at most a few words of text, and the method.
- Text-led shots are at most 30 % of the film.
- Something moves in every second.
- A change of style, when the story has one, enters in the new style's own language: tiles flip for a mosaic, a page turns for a manuscript, ink blooms for a wash. On the first frame of the change, punch both layers to 1.03 and ease back over about 20 frames. A complex change needs about 0.5 s.

## Gates

1. The `video-motion` gates: forward and reverse sample frames are byte-identical; `scan_linear_motion.py` exits 0.
2. Flicker: render 2 neighbouring frames of a still passage. The share of pixels that change by more than 12 of 255 stays under the value in `MOTION.md` (start with 2 % for boil, 0.5 % for a painting between stepped frames).
3. Dead air: no window of 1 s in which under 0.02 % of pixels change.
4. Loop seam, for a loop: the step from the last frame to the first is no larger than 1.5 times the median step inside the loop.
5. Frame cost: 1 frame renders in 150 ms or less at the output size, or the editor's render time grows past use. Report the measured cost.
6. Text: every glyph in a coarse style has strokes at least 2 marks wide; text stays out of the subtitle band.

Report each gate with its number in `DONE:`.
