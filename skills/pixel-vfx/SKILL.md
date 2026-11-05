---
name: pixel-vfx
description: "Use when a game needs pixel-art visual effects or short effect animations: fire, explosions, impacts, hit sparks, slashes, projectiles, magic (ice, lightning, holy, shadow, arcane), status auras, pickups, UI juice, weather or screen transitions. You write each effect as a small JavaScript module, render it with a bundled Node tool, look at the contact sheet, GIF and an act strip where a stand-in character performs it on a target, fix it over several rounds, then export a sprite sheet and JSON (with hit-stop and shake timing) for Godot, Unity, Phaser or any engine, or run it live in a JS canvas game. Also use when an effect looks mushy, floaty, flat, washed out or speckled, when a loop has a seam, when a one-shot pops off, or when a hit has no impact in game. Use it even if nobody says VFX."
---

# Pixel VFX

Effects are code, not images. Code is clay: draft it, render it, look at the render, judge it
with a strict pixel artist's eye, repair the biggest flaw, and render again. One prompt gives a
mediocre effect. Three or more looked-at rounds give a good one.

The bundled tool is two files: `pxfx.mjs` (library and command line) and `pxact.mjs` (the
stand-in character, acts and moves), Node 18+, no dependencies. It renders contact sheets,
GIFs, act strips and side-by-side comparisons, writes sprite sheets with JSON, searches an
effect library, and checks the effect contract. You need only `node` and a way to read a PNG.

## When to use

- A game needs a new effect: impact, explosion, slash, projectile, spell, aura, pickup
  sparkle, UI flash, weather, screen wipe, or a whole move (wind-up, throw, hit).
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
  optional `tags`, `timing` and `actor`. An effect may also export `act` (below).
- **Local space**: (0,0) is the anchor (feet, impact point, muzzle, torch base). x right, y
  down, 1 unit = 1 px. Directional effects face right; callers mirror with `flip` and turn with
  `angle`. Ground effects stand on y = 0; floor rings may dip a little below it.
- **Options**: `o` is `meta.options` merged with the caller's. `palette` is a ramp, dark to
  bright (hex list or one of 27 built-in names); any length must work. `seed` drives `rng`.
  Reserved: `x`, `y`, `flip`, `angle`, `light`, `actor`.
- **Rules**: a one-shot draws nothing at `t = duration`. A loop's frame at `t = 0` equals the
  frame at `t = duration`. Never use `Math.random()` or `Date`. Stay inside the canvas.
- **Layers**: `drawBack` goes behind the character (far half of a floor ring, back row of
  flames); `draw` is the front.
- **Scale**: a character is about 24 x 38 px. Draw detail at 1 px at that scale. Canvases are
  sized to fit, usually 64 to 320 px wide; full-screen effects use the game's resolution
  (for example 320x180).

The full contract, with four complete examples, is in `references/effect-contract.md`.

## Workflow

0. **Set up.** Copy the blocks from `references/script-pxfx.md` and `references/script-pxact.md`
   into `pxfx.mjs` and `pxact.mjs`, side by side, in the project or a scratch folder. Run
   `node pxfx.mjs --help`.
1. **Search the library first.** The companion skill `pixel-vfx-library` holds ready effects
   as pages. `node pxfx.mjs lib <its references folder> list fire` finds them;
   `lib <dir> get fire/torch --to fx` extracts one (and the effects it uses) with its imports
   fixed. Look in the project too. Reuse with options (palette, seed, flip, size, text) before
   you write anything new.
2. **Brief in one line** naming the subject, the motion, the palette and the timing. For
   example: "frost nova: ice shards burst out flat along the floor, a pale ring races out and
   shatters, mist lingers, ice ramp, 0.4 s hit then 1.5 s melt."
3. **References.** Read the closest library and project effects. Borrow techniques, not
   designs. `references/techniques.md` has recipes.
4. **Draft.** `node pxfx.mjs new fx/<name>.mjs` (`--loop` for a pure loop) writes a starter
   that passes check. If the task leaves the look open, write 3 to 5 really different drafts
   (`<name>-a.mjs`, `<name>-b.mjs`, ...) and put them side by side:
   `node pxfx.mjs compare 'fx/<name>-*.mjs'` writes `pxfx-out/compare.png`. Pick one. Send
   the pick and the runner-up to the orchestrator (`horch tell orchestrator "[<role>] pick: ..."`)
   and continue with your pick. Do not wait.
5. **Look, judge, fix.** `node pxfx.mjs render fx/<name>.mjs --gif`, then open
   `pxfx-out/<name>/contact.png` with the image-reading tool. Judge it against the quality bar,
   fix the worst thing, render again. At least 3 rounds; showpieces need more. Look at every
   frame for motion (`--all`, or `--frames 0-8 --zoom 6`), not only samples. `--bg ground`
   or `--bg sky` shows it in a scene; the GIF is for people who watch it move.
6. **Act it out.** Anything a character does (melee, casts, throws, hits it takes) gets
   `export const act` (character poses, a target, hits with freeze and shake). Judge it with
   `node pxfx.mjs act fx/<name>.mjs` and open `act.png`: CONSECUTIVE 12 fps frames, not
   samples. It must read as wind-up, hit, recovery at 1x. Status, spawn and death effects set
   `meta.actor: true` and work on the body through `o.actor`. See `references/acts.md`.
7. **Benchmark.** Put your draft next to the best library effect of the same category at the
   same size: `node pxfx.mjs compare fx/<name>.mjs fx/<lib-effect>.mjs`. If the library one
   has more detail, light or life, yours is not done.
8. **Check.** `node pxfx.mjs check fx/<name>.mjs` until every line passes. Delete unused drafts.
9. **Export and wire up.** `node pxfx.mjs sheet fx/<name>.mjs` (or a whole folder, which adds
   `index.json`) writes `<name>.png` and `<name>.json`. Follow `references/engines.md`. The
   DONE report lists the effect files, contact sheet, act strip and GIF paths, sheet and JSON
   paths, the check output, and the refine rounds with what each one fixed.

**Moves.** A move chains effects: `export const create = chain([{ fx: charge, t: 0 },
{ fx: bolt, t: 0.3, until: 0.6, x: (t) => 8 + (t - 0.3) * 200 }, ...])`. It renders, checks
and exports like any effect. See `references/moves.md`.

## Game-feel timing

Without a body behind it, an effect looks like a sticker. Attack and hit effects carry
timing for the game: `meta.timing = { windup, hits: [{ t, freeze, shake, amp }], recovery }`,
or it is derived from `act` (windup = minus the first key time, recovery = last key minus
last hit).

- `windup` (s): the character's anticipation before t = 0, moving away from the blow.
- `hits`: at effect time `t` the game freezes everything for `freeze` s (0.03 light to 0.2
  heavy), then shakes the camera `amp` px for `shake` s. Take hit times from the effect's
  own constants (`t: HIT`), never from eyeballing frames.
- `recovery` (s): the settle after the release. Follow-through overshoots, then settles.

The sheet JSON carries the timing (with the frame of each hit) and the act.

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
| Checkerboard smoke | Use `puff` or `vapour`/`dust` particles, not dithered discs. |
| Loop seam | Use only time-periodic terms: `sin(phase * TAU * n)`, looping noise, `cycle`. |
| One-shot pops off | Shrink, dither out or fall away before the end. |
| Great effect on a dead character | Add an `act`. |
| Act feels weightless | Wind-up opposite the blow, freeze up to 0.2 s, bigger pose change, shake. |

The full table is in `references/quality-bar.md`.

## References

- `references/script-pxfx.md`, `references/script-pxact.md`: the tool. Copy both blocks.
- `references/effect-contract.md`: read before you write an effect. Four complete examples.
- `references/acts.md`: the character, poses, acts, target, on-body effects, how to sell a hit.
- `references/moves.md`: chains, projectiles, reusing act keys.
- `references/quality-bar.md`: read when you judge a contact sheet or act strip.
- `references/techniques.md`: read when you draft: flames, sparks, shockwaves, shards,
  lightning, slashes, pickups, wipes, recolours, the particle kinds.
- `references/engines.md`: JS canvas, Godot, Unity, Phaser and others, native ports.

## Related skills

- `pixel-vfx-library`: the example effects (search them with `lib list`).
- `godot-particles-vfx`: GPU particles and shaders inside Godot.
- `godot-pixel-art-pipeline`: palette-locked sprites, atlases and their checks.
