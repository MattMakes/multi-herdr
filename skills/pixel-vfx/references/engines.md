# Engines

Three ways to get an effect into a game: run it live (JS canvas games), export a sprite sheet
(any engine), or port the logic (when neither fits).

## Live in a JS canvas or WebGL game

Plain canvas, Phaser, PixiJS and three.js games can run the effect module and the library
directly. The library part of `pxfx.mjs` does not touch Node at load time. If a bundler warns
about the `node:` imports in the CLI section, mark them external or delete the CLI section
from the game's copy.

```js
import { frame, clear, composite, instance } from './pxfx.mjs';
import * as slam from './fx/slam.mjs';

const W = 320, H = 180;                              // the game's pixel resolution
const fx = frame(W, H), img = new ImageData(W, H);
const live = [];                                     // running effects
export function spawnSlam(x, y, flip) { live.push({ inst: instance(slam, { flip }), x, y }); }

export function tick(dt, ctx, drawSprites) {
  clear(fx);
  for (const e of live) { e.inst.advance(dt); e.inst.draw(e.inst.paint(fx, e.x, e.y), 'back'); }
  drawSprites(fx);                                   // or draw sprites on a canvas between two layers
  for (const e of live) e.inst.draw(e.inst.paint(fx, e.x, e.y), 'front');
  img.data.set(composite(fx, null));                 // transparent where nothing is drawn
  ctx.putImageData(img, 0, 0);                       // then draw this canvas over the scene
  for (let i = live.length - 1; i >= 0; i--) if (live[i].inst.done) live.splice(i, 1);
}
```

- Several effects share one frame; draw order is call order.
- If the game draws its sprites on a canvas, use two effect canvases (back and front) and
  composite each, with the sprite canvas between them.
- Scale the canvas up with nearest-neighbour: CSS `image-rendering: pixelated` on the
  canvas, `ctx.imageSmoothingEnabled = false` when you draw it onto a bigger canvas.
- Hit-stop: when an effect's time passes a hit `t`, stop calling `advance` for every effect
  and character for `freeze` seconds, then shake the camera by `amp` px for `shake` s.
- Loops never finish (`done` stays false); remove them when their owner goes away.
- On-body effects (`meta.actor`) need your sprite: pass `actor: { w, h, x, y, mask, pixels,
  head, chest, hand }` in the options (local to the feet), call `inst.drawActor(p)` instead of
  drawing the sprite, or apply `inst.alpha()` to it yourself.

## Sprite sheets for any engine

`node pxfx.mjs sheet fx/<name>.mjs [--columns n] [--layer back|front]` writes:

- `<name>.png`: transparent frames, 1x, in a grid, left to right then top to bottom.
- `<name>.json`: Aseprite-style. `frames` is an array; each entry has `filename`,
  `frame {x, y, w, h}`, `rotated: false`, `trimmed: false`, `spriteSourceSize`, `sourceSize`
  and `duration` (ms). `meta` has `app`, `version`, `image`, `format: "RGBA8888"`, `size`,
  `scale: "1"`, `frameTags` (one tag over all frames), and a custom block:

```json
"vfx": {
  "fps": 30, "loop": false, "frames": 46,
  "frameSize": { "w": 128, "h": 72 }, "columns": 8,
  "pivot": { "x": 64, "y": 54 }, "layer": "both",
  "timing": { "windup": 0.22, "recovery": 0.4,
    "hits": [{ "t": 0, "freeze": 0.09, "shake": 0.3, "amp": 3, "frame": 0 }] }
}
```

Acted effects add `act` (poses, target, hits; the timing is derived from it), and
`actorAlpha` lists the character's opacity per frame for spawn and death effects.
`pivot` is the anchor in frame pixels: place it on the character's feet, the impact point or
the muzzle. `hits[].frame` is the frame index nearest each hit time.

Glow is baked into the sheet as colour with alpha, as if added over black. It reads as
additive over dark scenes with normal alpha blending. Over bright scenes it can look dull:
recolour to a darker ramp, put the effect on a layer with additive blending, or export it
with `--opts '{"light":0.5}'` (or `0` for no glow at all) for those scenes.

For effects that go both behind and in front of a character, export two sheets with
`--layer back` and `--layer front` and sort them around the character sprite.

### Godot 4

- Import the PNG with Filter set to Nearest (or the project's default texture filter set
  to Nearest), Compress Mode Lossless, Mipmaps off.
- Build a `SpriteFrames` resource from the grid for an `AnimatedSprite2D`: one animation,
  speed = `vfx.fps`, loop = `vfx.loop`, one `AtlasTexture` region per JSON frame. A
  `Sprite2D` with `hframes`/`vframes` = columns/rows plus an `AnimationPlayer` track on
  `frame` also works.
- Set `centered = false` and `offset = -pivot`, so the node's origin is the pivot.
- For a glow-only layer, use a `CanvasItemMaterial` with Blend Mode Add.
- Hit-stop: at a hit frame, set `Engine.time_scale` near 0 for `freeze` seconds (with a
  timer that ignores time scale), or pause the nodes involved; then run the camera shake.
- A small editor or tool script can read the JSON and build the `SpriteFrames`, so new
  effects need no hand setup.

### Unity

- Texture Type Sprite (2D and UI), Sprite Mode Multiple, Filter Mode Point (no filter),
  Compression None, Generate Mip Maps off.
- Sprite Editor: Slice, Grid By Cell Size = `frameSize`, Pivot Custom in pixels from `pivot`
  (Unity's y runs up: pivot y = h - pivot.y, or normalised (x / w, 1 - y / h)).
- Make an Animation clip with the slices at `vfx.fps` samples; set Loop Time from `vfx.loop`.
- Hit-stop with `Time.timeScale` for `freeze` seconds in unscaled time, then a camera shake.

### Phaser

- `this.load.atlas(key, '<name>.png', '<name>.json')` reads the JSON as an atlas.
- `this.anims.create({ key, frames: this.anims.generateFrameNames(key, { prefix: '<name>_', start: 0, end: n - 1, zeroPad: 3 }), frameRate: fps, repeat: loop ? -1 : 0 })`.
- Set `pixelArt: true` in the game config. Use `setOrigin(pivot.x / w, pivot.y / h)`.

### Other engines

GameMaker, Defold, LOVE, Bevy, Construct and others: slice the PNG by the grid
(`frameSize`, `columns`), use nearest filtering and no mipmaps, play at `vfx.fps`, place the
sprite so `pivot` is the anchor, and read `vfx.timing` for hit-stop and shake.

## Native port

Port the logic when the engine cannot run JS and a sheet is not enough: an effect that
tracks a target that moves, stretches between two points (a beam, a chain of lightning), or
needs a colour per instance.

- Keep the two buffers: COLOR (hard pixels, dithered alpha, blended puffs) and LIGHT
  (additive, saturating), composited at the end.
- Keep the 4x4 ordered-dither threshold table and the rule "draw where alpha beats the
  threshold" on screen pixels.
- Keep palette ramps as arrays and the same `shade` lookup, so recolours still work.
- Keep the fixed 1/120 s step for simulated effects and the seeded generator, so a port can
  reproduce the original frame for frame.
- Render the JS original (`render --all`) and the port side by side at the same zoom and
  compare frame by frame. Fix differences in motion first, then colour.
