# Quality bar

Judge every contact sheet against this page. Name the worst problem, fix that one, and
render again. A round that fixes three small things and leaves the big one is a wasted round.

## The bar

1. **Lines and light, not fills.** Build effects from 1 px rims, rings, trails, streaks and
   sparks. Let the LIGHT buffer carry brightness. A large flat area of saturated colour is
   almost always wrong. Keep white for the hottest core and the leading edges; use tinted
   middles and deep colour in the shadows.
2. **Light everywhere.** A wide, soft pool at low strength around the source. A tight, hot
   core. A `lit` pixel on each spark and glint. Lights that flare on the release and fall off
   over a few tenths of a second. A flattened, coloured pool on the floor (`glow` with
   `squash` about 0.3) while something burns, freezes or glows.
3. **Soft volumes.** Smoke, vapour, dust and mist are blended `puff` discs: a darker
   body with a lighter, smaller disc on top (lit from above). They swell and thin as they
   rise. Never draw smoke as a dither checkerboard.
4. **Real motion.** Sparks stretch along their velocity, fall under gravity and bounce on the
   floor. Debris tumbles, bounces, slides and settles. Fire rises and cools into smoke.
   Landings kick dust. Drag gives a fast burst, then a slow drift.
5. **Phased choreography.** Anticipation or charge (things gather inward, a glow builds),
   release (1 to 2 white flash frames, a flare, the burst), spread, hold (glints, embers,
   a lingering ring), break-up (rings dither out, shapes split), then a long decay. Many
   small events with varied delays beat one big synchronised event.
6. **Depth.** Floor rings are flat ellipses (ry about 0.25 of rx). The far half goes in
   `drawBack`, the near half in `draw`. Back rows are dimmer by one ramp step.
7. **Timing.** Fast in, slow out. A hit lasts 0.3 to 0.8 s including follow-through. Big
   effects stay 2 to 4 s until the smoke is gone. Most motion uses `outQuart` or `outExpo`.
8. **Reads at 1x.** One focal point and a clear silhouette. No lone speckle pixels inside
   shapes. Few colours: one ramp of 4 to 6 steps plus white is plenty.
9. **Original.** Generic motifs are fine (a ring, a star, a crescent slash). Never recreate a
   particular game's characters, sprites or logos.
10. **Clean.** Fits the canvas, ends empty or loops clean, deterministic, about 2 ms a frame.
    `check` proves all of this; your eyes prove the rest.

## How to look at a contact sheet

- Look at frame 0 or the hit frame first: is there a white flash frame and a flare?
- Squint (or look at the sheet at a small size): is there one clear shape per frame, or mush?
- Follow one particle across frames: does it accelerate, stretch, fall, bounce, fade?
- Look at the last third: does it decay slowly and end empty, or pop off?
- For loops, compare the last frame with the first: does the motion continue across the wrap?
- Render with `--bg light` once if the game has bright scenes, and with `--opts` for each
  palette the game uses.
- Look at every frame (`--all`, or `--frames a-b --zoom 6`) before you call it done. Samples
  hide one-frame glitches.
- For acted effects, read `act.png` as a flip book: ready, wind-up away, held coil, one
  release drawing, freeze with the target white, follow-through, settle.
- Put drafts and the best library effect of the category side by side with `compare`.

## Symptom -> fix

| Symptom | Cause | Fix |
| --- | --- | --- |
| Mush at 1x | Too many small shapes, too many mid tones, no focal point. | Fewer, bigger shapes. One bright focal point. Darker outer steps so the silhouette separates. |
| Lone speckle pixels inside shapes | Noise thresholds or dither inside a solid shape. | Threshold the noise with a margin, or clean single pixels: draw solid shapes at alpha 1 and dither only edges and fades. |
| Floaty, no punch | Linear motion, slow start, too long. | `outQuart`/`outExpo` for the burst, 1 or 2 white flash frames, a light flare, a shorter total. |
| Stiff, mechanical | Every particle has the same size, speed, delay and life. | Randomise each one from `rng`. Stagger delays. Add secondary motion: wobble, spin, a second smaller burst. |
| Washed out | Too much glow; light covers the shapes. | Lower glow strength, smaller radii, darker edge steps. Light should frame the shape, not hide it. |
| Flat | Uniform colour, no light direction. | Light from one side or from above: one brighter edge, one darker base step. Lit tops on smoke. |
| Vector-looking smooth gradients | Colour blended per pixel. | Hard ramp steps in colour (`shade`). Keep softness to light and smoke. |
| Checkerboard smoke | Smoke drawn with dithered alpha. | Use `puff` (blended), lit from above, growing as they fade. |
| Loop seam | A term that is not periodic in t, or a simulated loop. | Use only `sin(phase * TAU * n)` with whole n, looping noise that scrolls exactly one period, `cycle()` particles. Move the loop to a pure `draw`. |
| One-shot pops off | Things vanish at full size or brightness. | Shrink, dither out (alpha to 0), or let things fall away before the end. Lengthen the duration. |
| Ends-empty check fails | A puff or light still alive at the end. | Shorter lives or a longer duration; fade light to 0 before the end. |
| Writes outside the canvas | Glow radius, rising smoke or bounced debris leaves the canvas. | Bigger canvas, smaller or squashed glow, less speed, more drag. |
| Great effect, no impact in game | Nothing ties it to the hit. | Add `meta.timing`: windup, freeze, shake. Check that the game reads it. |
| Great effect on a dead character | No body performs it. | Add an `act` and judge `node pxfx.mjs act`. See `acts.md`. |
| Act feels weightless | Small pose changes, no hold, no stop. | Wind-up opposite the blow, a held coiled frame, a big silhouette change on release (`snap`), freeze up to 0.2 s, shake, follow-through. |
| Hit out of sync | The hit time was guessed from frames. | Take hit `t` from the effect's constant (`t: HIT`), in `timing` and in `act.hits`. |
| Sticker look | The effect ignores the scene. | Add a floor pool of light, a back layer behind the character, dust where it meets the floor. |
| Too busy | Every phase at full strength at once. | Stagger phases. Let the release own 2 to 3 frames, then quieten. |
| Slow frame time | Big glows every frame, per-pixel noise on a big area. | Fewer and smaller glows, coarser noise, skip pixels outside the flame profile early. |
