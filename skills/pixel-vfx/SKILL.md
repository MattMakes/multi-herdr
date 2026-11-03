---
name: pixel-vfx
description: "Use when a game needs pixel-art visual effects or short effect animations: fire, explosions, impacts, hit sparks, slashes, projectiles, magic (ice, lightning, holy, shadow, arcane), status auras, pickups, UI juice, weather or screen transitions. You write each effect as a small JavaScript module, render it with a bundled Node tool, look at the contact sheet, fix it over several rounds, then export a sprite sheet and JSON (with hit-stop and shake timing) for Godot, Unity, Phaser or any engine, or run it live in a JS canvas game. Also use when an effect looks mushy, floaty, flat, washed out or speckled, when a loop has a seam, when a one-shot pops off, or when a hit has no impact in game. Use it even if nobody says VFX."
---

# Pixel VFX

Effects are code, not images. Code is clay: draft it, render it, look at the render, judge it
with a strict pixel artist's eye, repair the biggest flaw, and render again. One prompt gives a mediocre
effect. Three or more looked-at rounds give a good one.

The bundled tool `pxfx.mjs` is a library and a command-line tool in one file (Node 18+, no
dependencies). It renders contact sheets, writes sprite sheets with JSON, and checks the
effect contract. You need only `node` and a way to read a PNG.

## When to use

- A game needs a new effect: impact, explosion, slash, projectile, spell, aura, pickup
  sparkle, UI flash, weather, screen wipe.
- An existing effect needs a new colour, size, direction or seed.
- An effect looks wrong (see the symptom table) or has no punch in game.

## When not to use

- The effect must be a GPU particle system or a shader in the engine. Port the look by hand.
- High-resolution, painted or 3D effects with no pixel grid.

## The effect model

- **Two buffers per frame.** COLOR holds hard RGBA pixels. Alpha below 1 is drawn with a 4x4
  ordered dither: a pixel appears only where alpha beats that cell's threshold. Only `puff`
  really blends, for smoke, vapour, dust and mist. LIGHT is additive RGB glow, saturating at
  255, added on top of colour and background at the end.
- **One ES module per effect** exports `meta` and either:
  - **pure** `draw(p, t, o)` (plus optional `drawBack`): the frame is a function of `t`
    only. Use it for loops (auras, portals, torches, beams), because they then wrap with no seam.
  - **simulated** `create(o)` returning `{ update(dt), draw(p), drawBack?(p) }`: use it for
    one-shots with particles and debris. The tool steps it at a fixed 1/120 s.
- **meta**: `name`, `size: [w, h]`, `origin: [x, y]` (canvas pixel of local 0,0), `duration`
  (s), `loop`, `fps` (default 30), `options` (defaults, at least `palette` and `seed`), and
  optional `tags` and `timing`.
- **Local space**: (0,0) is the anchor (feet, impact point, muzzle, torch base). x right, y
  down, 1 unit = 1 px. Directional effects face right; callers mirror with `flip` and turn with
  `angle`. Ground effects stand on y = 0; floor rings may dip a little below it.
- **Options**: `o` is `meta.options` merged with the caller's. `palette` is a ramp, dark to
  bright (hex list or a built-in name); any length must work. `seed` drives `rng`. Reserved:
  `x`, `y`, `flip`, `angle`, `light`.
- **Rules**: a one-shot draws nothing at `t = duration`. A loop's frame at `t = 0` equals the
  frame at `t = duration`. Never use `Math.random()` or `Date`. Stay inside the canvas.
- **Layers**: `drawBack` is what goes behind the character (far half of a floor ring, back
  row of flames). `draw` is the front.
- **Scale**: a character is about 24 x 38 px. Draw detail at 1 px at that scale. Canvases are
  sized to fit the effect, usually 64 to 320 px wide; full-screen effects use the game's
  resolution, for example 320x180.

The full contract, with two complete example effects, is in `references/effect-contract.md`.

## Workflow

0. **Set up.** Copy the block from `references/script-pxfx.md` into `pxfx.mjs` in the project
   (or a scratch folder) once. Run `node pxfx.mjs --help`. Look for existing effects in the
   project first. Reuse one with new options (palette, seed, flip, size) before you write a new one.
1. **Brief in one line** naming the subject, the motion, the palette and the timing. For
   example: "frost nova: ice shards burst out flat along the floor, a pale ring races out and
   shatters, mist lingers, ice ramp, 0.4 s hit then 1.5 s melt."
2. **References.** Read the closest effects in the project and any reference the task names.
   Borrow techniques, not designs. `references/techniques.md` has recipes.
3. **Draft.** `node pxfx.mjs new fx/<name>.mjs` (add `--loop` for a pure loop) writes a
   starter that passes check. When the direction is open, write 3 to 5 really different drafts
   (`<name>-a.mjs`, `<name>-b.mjs`, ...), render all, look at every contact sheet and pick one.
   Send the pick and the runner-up to the orchestrator
   (`horch tell orchestrator "[<role>] pick: ..."`) and continue with your pick. Do not wait.
4. **Look, judge, fix.** `node pxfx.mjs render fx/<name>.mjs`, then open
   `pxfx-out/<name>/contact.png` with the image-reading tool. Judge it against the quality bar,
   fix the worst thing, and render again. Do at least 3 rounds; showpieces need more. Look at
   every frame for motion problems (`--all`, or `--frames 0-8 --zoom 6` for a close look),
   not only the samples.
5. **Timing.** Add `meta.timing` to anything that hits (below).
6. **Check.** Run `node pxfx.mjs check fx/<name>.mjs` until every line passes. Delete unused
   drafts.
7. **Export and wire up.** `node pxfx.mjs sheet fx/<name>.mjs` writes `<name>.png` and
   `<name>.json`. Follow `references/engines.md` for the engine. The DONE report lists the
   effect files, contact sheet paths, sheet and JSON paths, the check output, and the number
   of refine rounds with what each one fixed.

## Game-feel timing

Without a body behind it, an effect looks like a sticker. Give attack and hit effects timing
data for the game: `meta.timing = { windup, hits: [{ t, freeze, shake, amp }], recovery }`.

- `windup` (s): the character's anticipation before t = 0, moving away from the blow.
- `hits`: at effect time `t` the game freezes everything (effect and characters) for
  `freeze` s (0.03 light to 0.2 heavy), then shakes the camera `amp` px for `shake` s. Take
  hit times from the effect's own constants (`t: HIT`), never from eyeballing frames.
- `recovery` (s): the settle after the release. Follow-through overshoots, then settles.

The sheet JSON carries this block, plus the frame index of each hit.

## Quality bar (short)

1. Lines and light, not fills: 1 px rims, rings, trails and sparks. Light carries the
   brightness. White only in the hottest core and edges, tinted middles, deep colour in shadow.
2. Light everywhere: a wide soft pool, a tight hot core, light on sparks, flares that fall off,
   a tinted patch of light on the ground under anything burning or frozen.
3. Smoke, dust and mist are blended puffs, lit from above; they grow and fade. Never a
   dither checkerboard.
4. Real motion: sparks stretch and bounce, debris tumbles and settles, fire rises and cools,
   landings kick dust, drag gives a fast burst then a slow drift.
5. Phases: gather, release (1 to 2 white flash frames), spread, hold, break-up, long decay.
6. Depth: floor rings are flat ellipses, far half behind; back rows dimmer.
7. Fast in, slow out. Hits 0.3 to 0.8 s; large effects stay 2 to 4 s until the smoke is gone.
8. Reads at 1x: one focal point, a clear silhouette, few colours, no speckle inside shapes.
9. Original: generic motifs only; never copy a specific game's sprites or logos.
10. Clean: fits the canvas, ends empty or loops clean, deterministic, about 2 ms a frame.

## Symptom -> fix (short)

| Symptom | Fix |
| --- | --- |
| Mush at 1x | Fewer, bigger shapes; 1 focal point; darker outer steps. |
| Floaty, no punch | `outQuart`/`outExpo`, a white flash frame, a shorter total. |
| Stiff, mechanical | Vary every particle's size, speed, delay and life; add secondary motion. |
| Checkerboard smoke | Use `puff`, not dithered discs. |
| Loop seam | Use only time-periodic terms: `sin(phase * TAU * n)`, looping noise, `cycle`. |
| One-shot pops off | Shrink, dither out or fall away before the end. |
| No impact in game | Add `meta.timing`: windup, freeze, shake. |

The full table is in `references/quality-bar.md`.

## References

- `references/script-pxfx.md`: the tool. Copy the block into `pxfx.mjs`.
- `references/effect-contract.md`: read before you write an effect. Meta, pure vs simulated,
  coordinates, options, layers, timing, and two complete examples.
- `references/quality-bar.md`: read when you judge a contact sheet.
- `references/techniques.md`: read when you draft. Flames, sparks, shockwaves, shards,
  lightning, slashes, pickups, screen wipes and recolours.
- `references/engines.md`: read when you export or wire up: JS canvas, Godot, Unity,
  Phaser and others, and native ports.

## Related skills

- `godot-particles-vfx`: GPU particles and shaders inside Godot.
- `godot-pixel-art-pipeline`: palette-locked sprites, atlases and their checks.
