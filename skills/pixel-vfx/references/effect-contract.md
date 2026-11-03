# Effect contract

Every effect is one ES module next to `pxfx.mjs` (or with a fixed import path). The tool,
a game and a native port all read it the same way, so keep to this contract exactly.

## The module

```js
import { rng, particles, shade } from './pxfx.mjs';

export const meta = {
  name: 'hit-spark',          // file-safe; names the outputs
  size: [96, 64],             // canvas in px, whole numbers 8..512
  origin: [48, 40],           // canvas pixel where local (0, 0) sits
  duration: 0.9,              // seconds
  loop: false,                // true = seamless loop
  fps: 30,                    // default 30
  options: { palette: 'fire', seed: 1 },   // defaults; palette and seed are required
  tags: ['impact'],           // optional, for search
  timing: { windup: 0.12, hits: [{ t: 0, freeze: 0.05, shake: 0.12, amp: 2 }], recovery: 0.2 },
};

// EITHER pure:
export function draw(p, t, o) { /* frame at time t, from t only */ }
export function drawBack(p, t, o) { /* optional: behind the character */ }

// OR simulated:
export function create(o) {
  return { update(dt) {}, draw(p) {}, drawBack(p) {} };   // drawBack is optional
}
```

Export `draw` or `create`, never both. `check` fails on a module with both or neither.

## Pure or simulated

| | Pure `draw(p, t, o)` | Simulated `create(o)` |
| --- | --- | --- |
| State | none; the frame is a function of t | inside the closure |
| Best for | loops: auras, portals, torches, waterfalls, beams, idle glows | one-shots: bursts, debris, sparks that bounce |
| Looping | seamless if every term is periodic in t | hard; avoid simulated loops |
| Stepping | any t, any order | forward only, fixed 1/120 s steps, so physics does not depend on the frame rate |

A pure effect can still show particles: give each one a phase on a cycle with `cycle()`
(see the campfire below) and compute its position from the phase.

A simulated effect keeps its own clock: add `dt` to a local `t` in `update` and read it in
`draw`. Spawn particles in `create` with a `delay` for staggered events, or spawn later in
`update` when something happens (a rock lands, a fuse reaches the end).

## Coordinates

- Local (0, 0) is the anchor: feet, impact point, muzzle, torch base. x goes right, y goes
  down, 1 unit is 1 px. Shapes centred on whole numbers are centred on that pixel.
- Directional effects (slashes, projectiles, muzzle flashes, breath) face right. The caller
  sets `flip: true` to face left and `angle` (degrees, clockwise) to aim. The painter applies
  both, so the effect never reads them.
- Ground effects stand on y = 0. Floor rings, scorch marks and light pools sit on a flat
  ellipse around y = 0 and may go a few px below it for perspective.
- Size the canvas so everything stays inside it: glows, rising smoke, bounced sparks and
  sliding debris too. `check` counts every write outside the canvas (light counts when it is
  above 4 of 255).

## Options

- `o` is `meta.options` merged with the caller's options (`--opts` on the command line, or
  the second argument of `instance(mod, extra)` in a game).
- `palette` is a ramp, dark to bright: an array of hex colours or a built-in name. The
  effect must work with any length. Use `shade(o.palette, u)` for a step by brightness, or
  `ramp(o.palette, n)` when you need exactly n steps. Extra ramps are fine as extra options
  (`rock`, `dust`, `logs` below).
- `seed` feeds `rng(o.seed)`. Same seed, same frames. A new seed gives a new variant.
- Reserved names, never in `meta.options`: `x`, `y` (offset from the origin), `flip`,
  `angle`, `light` (glow strength multiplier). The runner applies them.

## Rules

- **Ends empty**: a one-shot draws nothing at `t = duration`, neither colour nor light. Make
  the duration long enough for the last smoke to clear, and fade everything before the end.
- **Seamless loop**: the frame at `t = duration` equals the frame at `t = 0`. Build every
  moving term from `phase = t / duration`: `sin(phase * TAU * n)` with whole `n`, `noise` with
  a whole period that scrolls exactly one period per loop, and `cycle()` particles.
- **Deterministic**: no `Math.random()`, no `Date`, no module-level state that survives
  between runs. `check` renders twice and replaces `Math.random` and `Date` with throwers.
- **Inside the canvas**: no writes outside it.
- **Fast**: about 2 ms a frame. `check` warns above 2 ms and fails above 4 ms (median).

## Layers

`drawBack` is drawn behind the character, `draw` in front. In a preview both are drawn,
back first. Put in `drawBack`: the far (upper) half of floor rings, scorch marks and cracks
under the feet, the back row of flames, a portal's back rim. Put in `draw`: the near half of
floor rings, flashes, sparks, smoke. The sheet command can export each layer alone with
`--layer back` or `--layer front` so the engine can sort them around the character sprite.

## Timing

Anything that hits carries `meta.timing = { windup, hits: [{ t, freeze, shake, amp }], recovery }`.
Define the hit time as a constant in the effect (`const HIT = 0.12`) and use it both in the
drawing code and in `timing.hits`, so they never drift apart. `check` fails if hits are not
sorted, lie outside 0..duration, or freeze for more than 0.3 s. The sheet JSON copies the
block into `meta.vfx.timing` and adds the frame index of each hit.

| Weight | freeze | shake | amp |
| --- | --- | --- | --- |
| light jab, pickup | 0.03 | 0.06 | 1 |
| sword hit | 0.06 | 0.12 | 2 |
| heavy slam, big explosion | 0.1 to 0.2 | 0.25 to 0.4 | 3 to 4 |

## Example 1: ground slam (one-shot, simulated)

A heavy two-handed slam lands at t = 0. Two white flash frames (a flat flash on the floor and
a thin pillar). A floor shockwave races out with `outQuart` and breaks up into dither; its
far half is in `drawBack`. Cracks shoot out and fade. Eight rocks tumble as 2x2 chunks,
bounce, settle and kick a little dust where they land. Sixteen sparks fly and bounce. Dust
rolls out along the floor fast, then drifts and thins. A warm flare and a floor light pool
fall off over 0.4 s.

```js
// slam: a heavy ground slam. White flash frame, a floor shockwave (far half behind the
// character), cracks, rocks that tumble, bounce and kick dust, sparks, dust that rolls out.
import { rng, particles, ease, clamp, shade, lerp } from './pxfx.mjs';

const HIT = 0;        // the blow lands at t = 0 (the game plays the windup before it)
const FLASH = 0.05;   // white flash frames (2 frames at 30 fps)
const WAVE = 0.5;     // shockwave life
const FLARE = 0.4;    // light flare life
const TILT = 0.26;    // floor plane squash: ry = rx * TILT

export const meta = {
  name: 'slam',
  size: [128, 72],
  origin: [64, 54],
  duration: 1.5,
  loop: false,
  fps: 30,
  options: {
    palette: ['#5a240c', '#a4501a', '#e8962c', '#ffd65c', '#fff6dc'],
    rock: ['#2a221d', '#4a3d32', '#6e5d4b', '#97846a', '#bfae90'],
    dust: ['#2b2622', '#463e37', '#655a50', '#857a6e', '#a59a8c'],
    seed: 3,
  },
  tags: ['impact', 'ground', 'heavy'],
  timing: { windup: 0.22, hits: [{ t: HIT, freeze: 0.09, shake: 0.3, amp: 3 }], recovery: 0.4 },
};

export function create(o) {
  const r = rng(o.seed), ps = particles(r);
  for (let i = 0; i < 8; i++) {
    const side = i % 2 ? 1 : -1;
    ps.spawn(side * r.range(2, 8), -1, side * r.range(15, 60), -r.range(90, 170), r.range(1.0, 1.3),
      { kind: 'debris', ramp: o.rock, size: r.int(2, 3), bounce: r.range(0.25, 0.4), delay: r.range(0, 0.04) });
  }
  for (let i = 0; i < 16; i++) {
    const a = -Math.PI * r.range(0.06, 0.94), s = r.range(80, 200);
    ps.spawn(r.range(-3, 3), -2, Math.cos(a) * s, Math.sin(a) * s * 0.8, r.range(0.25, 0.6),
      { kind: 'spark', ramp: o.palette, bounce: 0.5 });
  }
  for (let i = 0; i < 16; i++) {
    const side = i % 2 ? 1 : -1, fast = r.range(0, 1);
    ps.spawn(side * r.range(3, 10), -r.range(1, 3), side * lerp(30, 150, fast), -r.range(2, 12), lerp(1.3, 0.7, fast),
      { kind: 'smoke', ramp: o.dust, size: lerp(4.5, 2, fast), grow: 1.8, drag: 3.5, gravity: -6, alpha: 0.55, delay: r.range(0, 0.1) });
  }
  const cracks = Array.from({ length: 7 }, (_, i) => {
    const a = (i / 7) * Math.PI * 2 + r.range(-0.3, 0.3), len = r.range(10, 22);
    const kink = r.range(0.4, 0.7), bend = r.range(-0.35, 0.35);
    return { a, len, kink, bend };
  });
  let t = 0;
  // The floor ring: one ellipse split into a far half (behind) and a near half (in front).
  const wave = (p, half) => {
    const u = clamp((t - HIT) / WAVE);
    if (u >= 1) return;
    const rx = lerp(6, 58, ease.outQuart(u)), a = u < 0.55 ? 1 : 1 - (u - 0.55) / 0.45;
    const dim = half === 'back' ? 0.2 : 0;
    p.oval(0, 0, rx, rx * TILT, shade(o.palette, 0.95 - u * 0.75 - dim), a, { width: u < 0.3 ? 2 : 1, half });
    if (u > 0.06) p.oval(0, 0, rx * 0.7, rx * 0.7 * TILT, shade(o.palette, 0.55 - u * 0.4 - dim), a, { width: 1, half });
  };
  const floorPoint = (a, d) => [Math.cos(a) * d, Math.sin(a) * d * TILT];
  return {
    update(dt) {
      t += dt;
      ps.update(dt);
      for (const q of ps.list) {
        if (q.kind === 'debris' && q.grounded && !q.kicked) {   // landings kick a little dust
          q.kicked = true;
          ps.spawn(q.x, -1, q.vx * 0.3, -6, 0.5, { kind: 'smoke', ramp: o.dust, size: 1.5, grow: 1.6, alpha: 0.45 });
        }
      }
    },
    drawBack(p) {
      const s = 1 - clamp((t - 0.5) / 0.8);
      if (s <= 0) return;
      p.oval(0, 0, 12, 3, shade(o.rock, 0), s);
      for (const c of cracks) {
        const grow = ease.outExpo(t / 0.12);
        const [x1, y1] = floorPoint(c.a, c.len * c.kink * grow);
        const [x2, y2] = floorPoint(c.a + c.bend, c.len * grow);
        p.line(0, 0, x1, y1, shade(o.rock, 0), s);
        p.line(x1, y1, x2, y2, shade(o.rock, 0), s * 0.8);
        if (t < 0.3) p.lit(x1, y1, shade(o.palette, 0.8), 1 - t / 0.3);
      }
      wave(p, 'back');
    },
    draw(p) {
      wave(p, 'front');
      const f = clamp(t / FLARE);
      if (f < 1) {
        p.glow(0, -8, 40, shade(o.palette, 0.6), 0.9 * (1 - f) ** 2, 0.55);
        p.glow(0, 0, 44, shade(o.palette, 0.4), 0.6 * (1 - f), 0.3);
        p.glow(0, -3, 10, shade(o.palette, 1), 1.2 * (1 - f) ** 3);
      }
      if (t < FLASH) {
        p.oval(0, -1, 16, 4, '#ffffff');
        p.rect(-1, -26, 3, 26, '#ffffff');
        p.line(0, -34, 0, -26, '#ffffff');
      }
      ps.draw(p);
    },
  };
}
```

Things to copy from it: the named constants (`HIT`, `FLASH`, `WAVE`) used by both drawing
and `timing`; one helper that draws the ring for either half; extra ramps (`rock`, `dust`)
as options; the landing check in `update` that spawns new particles from events; `lerp` on
one random number (`fast`) so fast dust puffs are also small and short-lived.

## Example 2: campfire (pure loop)

A small campfire loop at 15 fps. The flame is looping noise, scrolled up exactly one noise
period per loop, shaped by a teardrop profile and quantised to the fire ramp. The tip sways
with a whole number of sine periods. Two crossed logs with glowing coals sit in front of
the base. Six embers rise on a cycle and cool. A flickering glow and a flat floor pool give
light.

```js
// campfire: a seamless campfire loop. Scrolling looping noise quantised to the fire ramp,
// embers on a cycle, a flickering light pool on the floor.
import { fbm, cycle, shade, clamp, TAU } from './pxfx.mjs';

const H = 26;      // flame height in px
const P = 4;       // noise period in lattice cells; the noise scrolls exactly P cells per loop

export const meta = {
  name: 'campfire',
  size: [48, 56],
  origin: [24, 44],
  duration: 1.2,
  loop: true,
  fps: 15,
  options: { palette: 'fire', logs: ['#2a1a12', '#4a2e1c', '#6e4628', '#94643a'], seed: 5 },
  tags: ['fire', 'loop', 'light'],
};

export function draw(p, t, o) {
  const ph = t / meta.duration;
  const flick = 0.5 + 0.25 * Math.sin(ph * TAU * 3) + 0.25 * Math.sin(ph * TAU * 5 + 1);
  p.glow(0, -10, 22, shade(o.palette, 0.55), 0.3 + 0.12 * flick, 0.85);
  p.glow(0, 0, 22, shade(o.palette, 0.4), 0.3 + 0.1 * flick, 0.3);
  for (let y = -H; y <= 0; y++) {
    const h = -y / H;                                   // 0 at the base, 1 at the tip
    const half = 8 * (1 - h) ** 0.75 + 0.5;
    for (let x = -9; x <= 9; x++) {
      const n = fbm(x * 0.28 + o.seed, y * 0.16 + ph * P, o.seed, 3, 0, P);
      const sway = Math.sin(ph * TAU * 2 + h * 3) * h * 1.5;
      const edge = 1 - Math.abs(x - sway) / half;
      const heat = edge * (1.15 - h * 0.6) - (n - 0.5) * (0.8 + h * 1.2) - h * 0.35;
      if (heat > 0.08) p.px(x, y, shade(o.palette, clamp(0.15 + heat * 0.95)));
    }
  }
  // Two crossed logs in front of the flame base: dark underside, lit top, glowing ends.
  const L = o.logs;
  p.line(10, 0, -7, -4, L[0]); p.line(10, -1, -7, -5, L[2]);
  p.line(-10, 1, 7, -3, L[1]); p.line(-10, 0, 7, -4, L[3]);
  p.px(-10, 0, L[0]); p.px(-10, 1, L[0]);
  for (const [x, y] of [[-6, -2], [5, -3], [-1, -3]]) {
    p.px(x, y, shade(o.palette, 0.6 + 0.3 * flick));
    p.lit(x, y, shade(o.palette, 0.8), 0.5);
  }
  cycle(t, meta.duration, 6, (q, i, r) => {
    const x0 = r.range(-5, 5), y = -4 - q * (H + 8), x = x0 + Math.sin(q * TAU * r.int(1, 2) + i) * 3;
    if (q < 0.9) { p.px(x, y, shade(o.palette, 1 - q)); p.lit(x, y, shade(o.palette, 0.8), 0.7 * (1 - q)); }
  }, { seed: o.seed });
}
```

Things to copy from it: `P` (the noise period) and the scroll `ph * P` make the loop exact;
every flicker term uses `ph * TAU * n` with whole n; `cycle()` gives particles that wrap.

## Running both

```sh
node pxfx.mjs check slam.mjs          # every line PASS
node pxfx.mjs render slam.mjs         # pxfx-out/slam/contact.png
node pxfx.mjs sheet slam.mjs --columns 8
node pxfx.mjs render campfire.mjs --opts '{"palette":"ice"}' --out recolour
```
