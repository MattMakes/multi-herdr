# Clean-room spec: the `pixel-vfx` skill

This spec describes WHAT a skill must teach and WHAT its bundled tool must do.
It was written by someone who read an MIT-licensed reference project. You, the
implementer, have not read it and must not: write every sentence and every line
of code yourself, from this spec and your own knowledge of pixel art and game VFX.

## Purpose

A fleet worker (a Claude Code agent in a terminal pane, no subagents, no human
watching) uses this skill to make pixel-art visual effects and short sprite
animations for games: fire, explosions, impacts, slashes, projectiles, magic
(ice, lightning, holy, shadow, arcane), status auras, pickups, UI juice, weather,
screen transitions. Output: effect source code, rendered previews the agent looks
at, sprite sheets plus JSON for any engine, and wiring notes.

The core idea: effects are CODE, not images. Code is clay: draft it, render it,
look at the render with the image-reading tool, judge it like a picky pixel
artist, fix the worst thing, repeat. One-shot prompts make mediocre effects;
iterated, looked-at drafts make good ones.

## Repository rules the skill must obey (multi-herdr bundled skill)

- Directory `skills/pixel-vfx/`: `SKILL.md` plus `references/*.md`. Only `.md`
  files. `SKILL.md` <= 12 KB (12288 bytes). Whole directory <= 160 KB.
- Frontmatter: `name: pixel-vfx` and a `description:` (<= 1024 bytes, a strong
  trigger description: when to use it). Nothing else is needed.
- Self-contained: no runtime downloads, no `git clone`, no npm install, no other
  skill required, no machine paths, no `${CLAUDE_*}` variables, no model or
  harness assumptions beyond "you can run node and read a PNG".
- No subagents. Where a human decision is needed (e.g. picking between drafts),
  the worker sends the choice to the orchestrator as a message and continues
  with its best pick unless told otherwise; it never blocks waiting for a UI.
- Bundled tools are code blocks on `references/script-<name>.md` pages. The page
  starts with `# Script: <file>`, a one-line usage, a short paragraph on what it
  does, and says "Copy the block into a file and run it". Node 18+, standard
  library only (`node:zlib`, `node:fs`, `node:path`, `node:url`).
- Do not mention or link any third-party pixel-effect library or author. Write
  generic, original guidance.
- Plain, direct English. Short sentences. No marketing.

## The effect model (teach this in SKILL.md; implement it in the tool)

- A frame has two buffers of the same size:
  - COLOR: RGBA pixels. Hard pixels; partial alpha is rendered by ORDERED
    DITHERING with a 4x4 Bayer matrix (a pixel is drawn only where alpha beats
    that cell's threshold), except an explicit "translucent blend" call for soft
    volumes (smoke, vapour, mist) that really blends.
  - LIGHT: additive RGB glow, accumulated with saturation, composited on top of
    color (and the background) at the end.
- An effect is one ES module file exporting `meta` and EITHER:
  - pure: `draw(p, t, o)` (optional `drawBack(p, t, o)`): the frame at time t is
    a function of t only. Right for loops (auras, portals, torches, waterfalls,
    beams) because loops then wrap with no seam.
  - simulated: `create(o)` returns `{ update(dt), draw(p), drawBack?(p) }`.
    Right for one-shots with particles and debris. The runner steps it at a
    fixed small step (e.g. 1/120 s) so physics does not depend on frame rate.
- `meta`: `name`, `size: [w, h]` (canvas px), `origin: [x, y]` (where local
  0,0 sits in the canvas), `duration` (s), `loop` (bool), `fps` (default 30),
  `options` (defaults; at least `palette` and `seed`), optional `tags`,
  optional `timing` (game-feel timing, below).
- Local coordinates: (0,0) is the anchor (feet, impact point, muzzle, base of a
  torch). x right, y down, 1 unit = 1 px. Directional effects face RIGHT; the
  caller mirrors with `flip` and rotates with `angle`. Ground effects stand on
  y = 0. Floor rings/scorch can go a little below 0 for perspective.
- Options `o` = meta.options merged with caller options. `palette` is a ramp,
  dark to bright, as hex strings; any length must work (resample the ramp to the
  number of steps the effect needs). `seed` drives a seeded RNG. Reserved names:
  `x`, `y`, `flip`, `angle`, `light` (glow strength multiplier).
- Rules: one-shots draw NOTHING at t = duration (make duration long enough for
  the last smoke to clear). Loops are seamless (frame at t=0 equals frame at
  t=duration). Deterministic: never `Math.random()` or `Date`. Stay inside the
  canvas.
- Two layers: `drawBack` for what goes behind a character (far half of a floor
  ring, back row of flames), `draw` for the front.
- Scale reference: a typical character is about 24 x 38 px; draw detail at the
  1 px level at that scale. Canvases are as big as the effect needs (often 64 to
  320 px wide); full-screen transitions use the game's resolution (e.g. 320x180).

## Game-feel timing (replaces any character rig; keep it data only)

Effects look like stickers without a body behind them. The skill teaches the
worker to give attack/hit effects timing data the game uses:
`meta.timing = { windup, hits: [{ t, freeze, shake, amp }], recovery }`.
- windup (s): the character's anticipation BEFORE t = 0, opposite to the blow.
- hits: at effect time t, the game freezes EVERYTHING (effect and characters)
  for `freeze` s (0.03 light to 0.2 heavy), then shakes the camera `amp` px for
  `shake` s. Hit times come from the effect's own constants, never eyeballed.
- recovery (s): settle after the release; follow-through overshoots.
The sprite-sheet JSON carries this timing so any engine can apply hit-stop.

## The quality bar (teach in SKILL.md, in your own words)

1. Lines and light, not fills: 1 px rims, rings, trails, sparks; light carries
   brightness. Large flat saturated areas are almost always wrong. White only in
   the hottest core and edges, tinted middles, deep colour in shadow.
2. Light everywhere: a wide soft pool at low strength, a tight hot core, light on
   sparks and glints, lights that flare on impact and fall off, a coloured pool
   on the floor while something burns or freezes.
3. Translucent volumes for smoke/vapour/dust/mist: soft blended discs, lit on
   top, swelling and thinning. Never a dither checkerboard for smoke.
4. Real motion: sparks stretch with speed, fall under gravity and bounce; debris
   tumbles, bounces and settles; fire rises and cools to smoke; landings kick
   dust; drag gives fast burst then slow drift.
5. Phased choreography: anticipation/charge (things gathering inward), release
   (a 1-2 frame white "hero" frame, flare, burst), spread, hold (glints,
   embers), break-up, long decay. Many small varied events.
6. Depth: floor rings are flat ellipses, far half behind the character, near
   half in front; back rows dimmer.
7. Timing: fast in, slow out. Hits 0.3-0.8 s with follow-through; big effects
   linger 2-4 s while smoke clears.
8. Reads at 1x: clear silhouette and one focal point, no speckle noise inside
   shapes, few colours.
9. Original: generic motifs are fine; never recreate a specific game's sprites,
   characters or logos.
10. Clean: fits the canvas, ends empty or loops clean, deterministic, fast
    (a frame should render in about 2 ms).

## Symptom -> fix table (write your own version; cover at least these)

mush at 1x; lone speckle pixels inside shapes; floaty / no punch (use outQuart/
outExpo easing, a white hero frame, shorter total); stiff / mechanical (vary
every particle's size, speed, delay, life; add secondary motion); washed out
(less glow, darker edge steps); flat (light from one side, darker base step);
vector-looking smooth gradients (hard ramp steps in colour; softness only in
light and smoke); checkerboard smoke (translucent puffs instead); loop seam
(time-periodic functions: sin(phase*2pi*n), looping noise, particles on cycles);
one-shot that pops off (shrink, dither out or fall away before the end); great
effect but no impact in game (add timing: windup, freeze, shake); hit out of sync
(derive hit t from the effect's constant).

## The workflow (SKILL.md's main section)

0. Set up: copy the tool from `references/script-pxfx.md` into the project (or a
   scratch folder) once; check `node pxfx.mjs --help` runs. Find existing
   effects in the project first; reuse with new options (palette, seed, size,
   flip) before writing a new one.
1. Brief in one line with art direction: what it is, how it moves, palette,
   timing. Example of the shape (write your own example).
2. References: the closest existing effects in the project, any reference the
   task names. Borrow techniques, not designs.
3. Draft. When the direction is open, write 3 to 5 genuinely different drafts
   (`<name>-a.mjs` ...), render them all, look at every contact sheet, pick one.
   Report the pick and the runner-up to the orchestrator.
4. Look, judge, fix: render, open the contact sheet PNG with the image-reading
   tool, judge, fix the worst thing, render again. Minimum 3 rounds; showpieces
   more. Look at all frames for motion problems, not only samples.
5. Timing: add `meta.timing` for anything that hits.
6. Check: run the tool's check until it passes clean. Delete unused drafts.
7. Export and wire up (below). DONE report: effect files, preview paths, sheet
   + JSON paths, check output, number of refine rounds and what each fixed.

## Engine integration (a references page)

- JS canvas / WebGL games (plain canvas, Phaser, PixiJS, three.js): the effect
  module and the tool's runtime functions can run live: create a frame at the
  game's pixel resolution, step and draw each tick (back layer, then the game's
  sprites, then front layer), composite to ImageData, scale the canvas up with
  nearest-neighbour (`image-rendering: pixelated`). Several effects can share a
  frame.
- Any engine via sprite sheets: transparent frames in a grid PNG plus an
  Aseprite-style JSON (array `frames` with `filename`, `frame {x,y,w,h}`,
  `rotated:false`, `trimmed:false`, `spriteSourceSize`, `sourceSize`, `duration`
  in ms; `meta` with `app`, `image`, `format: "RGBA8888"`, `size`, `scale: "1"`,
  `frameTags`, and a custom block `meta.vfx = { fps, loop, frames, frameSize,
  columns, pivot, timing? }`). Glow is baked into the sheet as colour with alpha
  (reads as additive over dark scenes; for bright scenes recolour or put the
  effect on a dark/additive layer).
  - Godot 4: import with Filter Nearest, compression Lossless, no mipmaps; build
    `SpriteFrames` from the grid (AnimatedSprite2D, `hframes`/`vframes` on a
    Sprite2D plus AnimationPlayer also works); offset so the pivot is the node
    origin; a glow-only layer can use a CanvasItemMaterial with Add blend; hit
    stop via `Engine.time_scale` briefly or pausing the relevant nodes.
  - Unity: Point (no filter), no compression, Sprite Mode Multiple, slice Grid
    By Cell Size, custom pivot in pixels; Animation clip at meta fps.
  - Phaser: atlas loader reads the JSON; set `pixelArt: true`.
  - GameMaker, Defold, LOVE, Bevy, Construct: slice by grid, nearest filtering.
- Native port (when the engine can't run JS and a sheet is not enough, e.g. an
  effect that follows a moving target or needs per-instance colour): port the
  logic, keep the two buffers, the 4x4 Bayer threshold and palette ramp lookups;
  render the JS original and the port side by side and compare.

## The bundled tool: `pxfx.mjs` (one file, Node 18+, no dependencies)

Write it carefully, run it, and test it. It is both a library (imported by
effect files) and a CLI.

Library exports (names are yours to pick, but keep them short and documented):
- frame create/clear; composite(frame, background|null) -> RGBA bytes.
- Painter with placement (origin, flip, angle) and local->screen transforms.
  Drawing calls, each taking colour + alpha (dithered by default): pixel, line,
  filled circle, ring (thickness), arc band, ellipse, rect, translucent disc
  (true blend), additive glow (radial falloff) and additive light pixel.
  Colours may be hex strings or packed numbers; be consistent and document it.
- Seeded RNG (e.g. mulberry32): next(), range(a,b), int, pick.
- Easing set (linear, inQuad, outQuad, outQuart, outExpo, inOutSine at least),
  clamp, lerp, smoothstep.
- Palette ramp helpers: parse hex, resample a ramp to n steps, pick by 0..1.
  A small set of named ramps (fire, ice, poison, holy, shadow, electric, smoke,
  gold, blood, water, arcane) as hex arrays dark->bright.
- A small particle system: spawn(x,y,vx,vy,life,{size,color ramp,gravity,drag,
  bounce,kind}), update(dt), draw(p). Kinds at least: spark (speed-stretched
  streak, falls, bounces on y=0, glows), ember (rises, wanders, cools), smoke
  (translucent puff, rises, swells, thins), debris (falls, bounces, settles,
  fades). Deterministic given the RNG.
- A loop helper so pure loops can place N particles on a cycle (each particle's
  phase = (t/duration + i/N) mod 1) for seamless looping particles.

CLI (`node pxfx.mjs <command> <effect.mjs> [flags]`):
- `render`: renders all frames (n = round(duration*fps), +1 for one-shots so the
  empty last frame shows) and writes `<out>/<name>/contact.png`: frames in a grid
  over a dark checker/backdrop, upscaled by `--zoom` (default 3, nearest), each
  labelled with frame index if you can do it simply (a tiny 3x5 digit font is
  fine; optional). `--all` shows every frame; default caps at ~24 evenly spaced.
  `--bg dark|light|none`. `--opts '<json>'` overrides options.
- `sheet`: writes `<name>.png` (transparent frames, grid, 1x) and `<name>.json`
  (Aseprite-style, as above). `--columns n` optional.
- `check`: prints PASS/FAIL lines and exits 1 on any FAIL:
  - meta valid (size whole pixels 8..512, origin inside, duration > 0, fps > 0,
    no reserved option names in meta.options);
  - stays inside the canvas (count pixels the painter tried to write outside);
  - one-shot ends empty (last frame has no colour and no light above a small
    threshold);
  - loop seam (frame at t=0 vs t=duration differ by at most a few pixels;
    report the count);
  - deterministic (render twice with the same seed; frames identical) and does
    not call Math.random (temporarily replace Math.random with a thrower while
    rendering);
  - frame time (median ms per frame; FAIL above e.g. 4 ms, WARN above 2 ms);
  - timing, when present: hits sorted, inside 0..duration, freeze 0..0.3.
- `new <path.mjs> [--loop]`: writes a working starter effect (one-shot simulated
  or pure loop) that passes `check`.
- Output dir default `pxfx-out/` next to the cwd; `--out` overrides.
- PNG writer: RGBA8, filter 0 per row, zlib deflate, correct CRC32. Verify the
  files open (e.g. read one back with the Read tool, or check the signature).

## Reference pages to write (suggested; you may reorganise)

- `references/script-pxfx.md` — the tool (largest file).
- `references/effect-contract.md` — meta, pure vs simulated, coordinates,
  options, layers, determinism, timing; with two short complete example
  effects (one one-shot simulated impact, one pure seamless loop) that pass
  check.
- `references/quality-bar.md` — the bar and the symptom -> fix table in full
  (keep a short version in SKILL.md).
- `references/engines.md` — integration notes above.
- `references/techniques.md` — recipes in your own words: flames from scrolling
  looping noise quantised to the ramp; sparks; shockwave rings on a tilted plane
  with far half in drawBack; crystals/shards; lightning bolts (midpoint
  displacement, seeded, re-rolled every few frames, with a glow line); slash
  arcs (a crescent band whose width tapers, a white leading edge, fading trail);
  pickups (sparkle crosses that twinkle); screen transitions (dither wipes
  driven by the Bayer threshold); recolouring (palette swap via ramp).

## Acceptance

- Sizes within budget; only .md files; frontmatter valid.
- You extracted the tool from the markdown into a scratch folder, ran `new`,
  `render`, `sheet` and `check` on both example effects, and every check PASSes.
- You opened at least the two contact sheets with the image-reading tool and
  they look like what the examples claim (describe them in your report).
- No text or code from any third-party pixel effect project.
