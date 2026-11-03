# Clean-room spec 2: extend the `pixel-vfx` skill and its tool

Written by the person who read the reference project. You have not read it and
must not. Build everything from this spec and your own knowledge. Same rules as
spec 1 (`1-skill-and-tool.md`; read it too for context): `.md` files only,
SKILL.md <= 12288 bytes, whole `skills/pixel-vfx/` dir <= 163840 bytes (it is
~91.5 KB now, so you have ~70 KB; spend it carefully and trim prose before you
drop features), no third-party library or author named, no downloads, no
subagents, plain English.

You extend the EXISTING skill at `skills/pixel-vfx/` (written by an earlier clean
writer; read all of it first). Keep its API working: existing effects
(`references/effect-contract.md` examples) must still pass `check` unchanged.
You may split the tool into two script pages if one gets too big:
`references/script-pxfx.md` (core, unchanged file name `pxfx.mjs`) and
`references/script-pxact.md` (`pxact.mjs`: actor, acts, chain, previews), as
long as the user still runs one command line (`node pxfx.mjs ...` may import
`./pxact.mjs` lazily) and SKILL.md says to copy both.

## 1. A stand-in character and a target (the biggest gap)

A hit effect alone looks like a sticker. Previews must be able to show a body
performing the effect and a target taking it.

- **Stock character**: an original, simple pixel figure about 24 x 38 px,
  facing right, three-quarter view, standing on y = 0 with its feet at the
  effect origin. Draw it from part sprites (head, torso, near arm, far arm, near
  leg, far leg) defined as small string grids plus a palette, so it stays tiny.
  Design it yourself: readable at 1x, dark outline, 4-6 colours.
- **Poses**: a pose is a small object of numbers that differ from standing:
  `x` (feet offset), `crouch` (px hips drop), `lean` (px torso shift, + forward),
  near hand `hx, hy` and far hand `bx, by` (px from the feet, y negative up),
  `weapon` (none | sword | staff | hammer | bow | gun) with angle `wa` (radians,
  0 = forward), `ghost` (leave 2-3 fading after-images). Arms are drawn as
  1-2 px lines (two segments with a fixed-length bend you solve with simple
  2-bone IK) from shoulder to hand, legs bend with crouch. Keep it 2D; no 3D rig.
  Provide named starter poses: STAND, READY, WINDUP_BACK, STRIKE_FORWARD,
  CAST_UP, CAST_FORWARD, CROUCH, HURT, plus a short field reference.
- **Act data** on an effect: `export const act = { poses: [[t, pose, how], ...],
  target: { x } | false, hits: [{ t, freeze, shake, amp, push, flash, tint }] }`.
  `t` is effect time; negative keys are the wind-up. `how` is `smooth`
  (default), `snap` (hold then jump), `out` (fast then settle), `in`, `line`.
  Limbs move on whole 12 fps steps between keys (held drawings); feet position
  moves smoothly. Keep `meta.timing` from spec 1 working: if an effect has
  `act` but no `meta.timing`, derive timing (windup = -first key t, hits,
  recovery = last key t - last hit t); the sheet JSON carries both.
- **Target**: a training dummy (post, round body, springy) at `target.x`. A hit
  pushes it (px, + away), it springs back with a damped wobble, `flash` turns it
  white for n frames, `tint` recolours it for a while (e.g. ice blue, poison
  green, shock yellow, burn red; `null` clears).
- **Freeze and shake** in previews: during a hit's freeze everything (effect,
  character, target) holds; then the whole picture shakes `amp` px for `shake` s
  with a decaying seeded jitter.
- **On-body effects** (status, spawn, death, teleport, auras): when
  `meta.actor: true`, the effect receives `o.actor = { mask, pixels, w, h, x, y,
  head, chest }`: the character's silhouette (1 where opaque) and its own
  colours, in local space with feet at (0,0), plus head and chest points.
  Effects can recolour `pixels` (frozen, stone, poison), outline `mask` (auras),
  and export `actorAlpha(t, o)` (0..1) to fade the character in or out
  (spawn, death, blink). Previews draw the character using the effect's
  recoloured pixels when the effect returns them (define a clean way: e.g. the
  painter gets `p.actorPixels(pixels)` or the effect exports
  `actorPaint(p, t, o)`; pick one and document it).
- **CLI**: `act <fx.mjs>` renders a strip of CONSECUTIVE 12 fps steps from the
  first key to the last (plus a short hold), with the character, the target,
  freezes and shake, to `<out>/<name>/act.png`, frames labelled with effect time
  and an `F` mark on frozen frames. `--sheet` bakes the act into a sprite sheet
  + JSON. `render` gains `--actor` (draw the stock character standing under the
  effect; on by default when `meta.actor` is true).

## 2. Moves: effects made of effects

`chain(cues)` returns a `create(o)` that plays several effect modules as one:
each cue `{ fx, t, until, x, y, flip, ...options }`; `x` and `y` may be numbers
or functions of the move's time (so a projectile is just an effect whose x moves:
`x: (t) => 20 + (t - THROW) * SPEED`). Each cue gets its own seed (move seed +
index). Cue placement respects the move's own flip. The move is an ordinary
effect from outside: it renders, checks, exports. Also `shiftKeys(keys, dt, dx)`
and `shiftHits(hits, dt)` to reuse other effects' act keys inside a longer move.

## 3. Seeing motion

- `render --gif` writes `<out>/<name>/anim.gif` (upscaled by `--zoom`, real
  frame delays, loops forever). Write a GIF89a encoder with LZW in plain JS: a
  per-file palette of the used colours (quantise to <= 256 by a simple method if
  needed; transparency not required since previews have a backdrop).
- `render --mp4`: only if `ffmpeg` is on PATH (pipe raw RGBA frames to it);
  otherwise print a one-line note and skip. Never fail because of it.
- `compare <a.mjs> <b.mjs> ... [--frames n]`: one row per effect, the same n
  evenly spaced moments (normalised time) in each column, labelled, to
  `<out>/compare.png`. Accept a glob-like `name-*` pattern (implement simple `*`
  matching yourself; no glob dependency). This is how drafts are judged
  side by side.
- Backdrops for previews: `--bg dark|ground|sky|light|black|none`
  (`ground` = a stone floor from the origin row down; `sky` = dithered night
  gradient). Full-screen effects (width >= 320 and ~16:9) fill the frame.

## 4. Toolkit breadth

Add to the library (document each in one table row; keep code compact):
- Particle kinds (in addition to spark, ember, smoke, debris): `mote` (short
  streak with a dark tail that dithers out), `streak` (accelerating dotted line
  for energy gathering INWARD), `flake` (drifting speck; a + cross when big),
  `vapour` (soft blended puff that swells and thins, optional coloured haze
  glow), `dust` (denser low puff that rolls along the floor), `fire` (a flame
  ball cooling down a fire ramp as it rises; flag `smokes` turns it to smoke when
  it dies), `brand` (a burning chunk that trails flame and calls `onLand(x,y)`),
  `grit` (tiny falling bits, bounce once and settle), `star` (twinkling cross).
  Flag `back` draws a particle in the back layer.
- `lights()`: `add(x, y, r, color, k, life)`; lights flare and fall off with
  life squared; `update(dt)`, `draw(p)`.
- `blazes(ps)`: `add(x, y, life, power)` small fires that keep feeding flame
  particles where something landed.
- Shape helpers: `sparkle(p, x, y, arm, c, tip)` (4-point star glint),
  `flare(p, x, y, len, k, ramp)` (8-ray flare), `band(p, cx, cy, r, squash,
  thick, alpha, ramp, keep)` (a ring on a tilted floor plane; `keep(angle)` picks
  the far or near half), `crystal(p, x, base, h, w, lean, ramp)` (faceted spike
  with a lit ridge), `bolt(r, x0, y0, x1, y1, rough, depth)` (midpoint-displaced
  lightning points) and `forks(...)`, `trail(points, ...)`, `ngon(n, r, rot)`,
  `star(points, r1, r2, rot)`.
- Painter additions: `p.polyline(pts, c, a)`, `p.beam(x0, y0, x1, y1, w, c, a)`
  (thick capsule), `p.tri`, `p.poly(pts, c, a)`, `p.shade(x0, y0, x1, y1, fn)`
  (per-pixel shader: `fn(x, y)` returns null or `{ c, a, l, k }` for colour,
  alpha, light colour, light strength), `p.sprite(rows, pal, x, y, { scale, a,
  flip, tint })` from string grids, `p.text(str, x, y, c, { font, scale, align,
  outline, shadow, a })` and `p.measure(str, font)` with two original bitmap
  fonts you design: `small` 3x5 and `big` 5x7 (digits, A-Z, a few symbols:
  `+ - ! ? . , : / % x`), `p.mask(m, x, y, c, a)`, `p.outline(m, x, y, c, a)`,
  `p.get(x, y)`.
- Noise for loops: `loopNoise(x, phase, ...)` (noise that is periodic in phase)
  and `tileFbm(x, y, period...)` for scrolling seamless flames and water, if
  the existing `noise`/`fbm` periodic arguments do not already cover it (then
  just document how).
- Motion helpers: `fly(t, x0, y0, vx, vy, g, drag)` (closed-form position),
  `burst(n, r, ...)` (radial spawn pattern).
- Named ramps: extend `RAMPS` to about 28, dark to bright, your own colours:
  fire ember lava smoke dust ice water electric volt poison holy heal arcane
  shadow blood earth stone gold leaf wood metal bone neon cyber matrix candy
  night (keep the existing 11 colours as they are).

## 5. Check additions

- Light spill: bright light (sum of channels > ~90) outside the canvas.
- Act: keys sorted by time, hits sorted and inside the act span, freeze 0..0.3,
  unknown pose fields (typos), hands that jump more than ~10 px between
  consecutive 12 fps steps without a `snap` (teleporting hands), weapon
  value valid.
- Actor effects: `actorAlpha` returns 0..1.
- A move: every cue's effect passes its own meta check.

## 6. Batch and library hooks

- `render <dir>` and `sheet <dir>`: every `*.mjs` effect in a folder;
  `sheet <dir>` also writes `<out>/index.json` listing each sheet (name, file,
  fps, loop, frames, frameSize, pivot, timing).
- `lib <library-dir> list [words...]`: search a LIBRARY of effects stored as
  Markdown pages (`<library-dir>/*.md`; format below), matching ids, names, tags
  and one-line descriptions; print `id  kind  duration  size  description`.
- `lib <library-dir> get <id> [--to <dir>] [--tool <path-to-pxfx.mjs>]`: extract
  the effect's code block into `<dir>/<category>-<name>.mjs` (default dir
  `fx/`), rewriting its tool import to the right relative path, and do the same
  recursively for the library effects it depends on (moves).
- Library page format (the library itself is written by others, to this
  format): each page is one category; each effect is a level-2 heading
  `## <category>/<name>` followed by one line of description, then a fenced `js`
  block whose first line is `// id: <category>/<name>`. Library effects import
  the tool as `./pxfx.mjs` (and `./pxact.mjs` if split) and import other library
  effects as `./<category>-<name>.mjs`. Tags come from `meta.tags`.

## 7. Docs

- SKILL.md: update the workflow. Step 1 becomes "search the library first"
  (`lib list`) and reuse with options; mention the companion skill
  `pixel-vfx-library` (it will exist; it holds the effects). Draft step uses
  `compare` for drafts. Look step mentions `--gif`. A new short "Acting it out"
  step: add `act`, judge with `node pxfx.mjs act`, consecutive frames not
  samples. A short "Moves" note. Benchmark: put the draft next to the best
  library effect of the same category at the same size with `compare`; if the
  library one has more detail, light or life, yours is not done.
- New or extended references: `references/acts.md` (character, poses with a
  field table, act data, target, freeze/shake, on-body effects, how to make it
  sell: wind-up opposite the blow, big silhouette change on the release frame,
  longer freeze for heavier hits, follow-through overshoot, line the hand up with
  where the effect fires from, arms reach ~15 px so step the body in, blade lags
  the hand on a swing), `references/moves.md` (chain, projectiles, shiftKeys),
  and add the new toolkit to `script-pxfx.md`'s library table and to
  `techniques.md` where it helps. Add these symptom rows to the quality-bar
  table: "great effect on a dead character" -> add an act; "act feels
  weightless" -> opposite wind-up, longer freeze (to 0.2 s), bigger pose change,
  shake; "hit out of sync" -> take hit t from the effect's constant.
- Add to effect-contract.md a third complete example: a short melee slash WITH
  an act (character wind-up, release, freeze on contact, target reacts,
  recovery), and a fourth: a status effect using `o.actor` (e.g. frozen:
  recolours the body icy, frost outline, flakes), both passing `check` and `act`.

## Acceptance

- Re-extract every script and example from the Markdown into the scratch folder
  a scratch folder outside the repo
  and run: `check` on all 4 examples (all PASS), `render --gif`, `act` on the
  slash, `compare` on two drafts, `sheet <dir>` with index.json, and `lib list`
  / `lib get` against a tiny sample library page you write in the scratch folder
  (not in the repo). Open act.png, compare.png and at least one contact sheet
  with the Read tool and judge them honestly; iterate until the character reads
  as a person at 1x and the act reads as wind-up, hit, recovery.
- Verify the GIF opens (header GIF89a, decode it back with a tiny decoder or
  check the frame count by parsing blocks).
- Sizes within budget. No line starting with the PENDING-colon form, no
  spec-todo token. `godot-<name>` mentions must name real skills.
