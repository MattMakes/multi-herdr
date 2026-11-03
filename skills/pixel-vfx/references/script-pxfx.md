# Script: pxfx.mjs

Usage: `node pxfx.mjs <new|render|sheet|check|act|compare|lib> <target> [--out dir] [--opts '<json>'] [--zoom n] [--all] [--frames 3-8] [--bg dark|ground|sky|light|black|none] [--columns n] [--layer both|back|front] [--actor] [--gif] [--mp4] [--sheet]`

`pxfx.mjs` is the effect library and the command-line tool in one file. Effect modules import
the library from it. The command line renders contact sheets, GIFs, act strips and
side-by-side comparisons to look at, writes sprite sheets with Aseprite-style JSON, searches and
extracts library effects, writes starter effects, and checks the effect contract with PASS,
WARN, INFO and FAIL lines (exit code 1 on any FAIL). Node 18 or newer, standard library only.
The library part does not touch Node at load time, so a browser game can import it as is. The
character, acts and moves live in `pxact.mjs` (`references/script-pxact.md`); the command line
loads it from the same folder when it needs it. Copy the block into a file and run it, for
example `pxfx.mjs` next to your effects with `pxact.mjs` beside it (a starter from `new`
computes its import path; hand-written effects import `./pxfx.mjs`).

## Commands

| Command | Writes | Notes |
| --- | --- | --- |
| `new <path.mjs> [--loop]` | a starter effect | A one-shot simulated impact, or with `--loop` a pure loop. Both pass `check`. Never overwrites. |
| `render <fx.mjs\|dir>` | `<out>/<name>/contact.png`; `anim.gif` with `--gif`; `anim.mp4` with `--mp4` | Frames in a grid over the backdrop, scaled by `--zoom` (default 3; 1 for full-screen effects, 320 wide or more and about 16:9), labelled with their index; hit frames in yellow. At most 24 evenly spaced frames unless `--all` or `--frames`. `--actor` (on by default when `meta.actor` is true) draws the stock character between the two layers. The GIF has every frame, zoomed, real delays, looping. The mp4 needs `ffmpeg` on PATH; without it the tool prints a note and goes on. |
| `sheet <fx.mjs\|dir>` | `<out>/<name>.png`, `<out>/<name>.json`; a folder adds `<out>/index.json` | Transparent frames at 1x in a grid (`--columns`, default as many as fit in 2048 px). `meta.vfx` has `fps`, `loop`, `frames`, `frameSize`, `columns`, `pivot`, `layer`, `timing` (with each hit's `frame`), `act`, and `actorAlpha` per frame when exported. `index.json` lists name, file, json, fps, loop, frames, frameSize, pivot and timing per sheet. |
| `check <fx.mjs\|dir>` | nothing | meta, move cues, runs (no `Math.random`, no `Date`), inside canvas, light spill (bright light off the canvas FAILs, faint WARNs), draws something, ends empty or loop seam, deterministic, frame time, actor (`actorAlpha` in 0..1), timing, act keys (sorted, known fields and weapons), act hits (sorted, inside the act, freeze 0..0.3), act hands (no move over 10 px per 12 fps step without `snap`). |
| `act <fx.mjs\|dir>` | `<out>/<name>/act.png`; `act.gif` with `--gif`; `<out>/<name>-act.png` and `.json` with `--sheet` | Consecutive 12 fps steps from the first key to the last plus 0.25 s, with the character, the target, freezes and shake. Labels are effect time; `F` marks frozen frames; yellow marks the first frame of each hit. |
| `compare <fx.mjs\|name-*.mjs>...` | `<out>/compare.png` | One row per effect, `--frames n` (default 8) columns at the same fractions of each effect's duration, aligned on the origin, zoom 2. A `*` in a file name matches any text (quote it). |
| `lib <dir> list [words]` | nothing | Prints `id  kind  duration  size  description` for each effect whose id, tags and description contain all the words. Kind is `loop` or `once` plus `+act`, `+body` (`meta.actor`), `+move` (`chain`). Write `duration` and `size` as literals so they show. |
| `lib <dir> get <id>... [--to fx] [--tool path]` | `<to>/<category>-<name>.mjs` | Extracts the code, points `./pxfx.mjs` and `./pxact.mjs` at the tool (`--tool`, default this file), and extracts the library effects it imports, recursively. |

`--out` defaults to `pxfx-out`. `--opts` merges options over `meta.options`, for example
`--opts '{"palette":"ice","seed":4,"flip":true}'`. `--layer back` or `front` draws one layer
and adds `-back` or `-front` to the output name. Backdrops: `dark` (checks, origin marked),
`ground` (stone floor from the origin row down), `sky` (dithered night gradient), `light`,
`black`, `none` (transparent). A folder target runs every `*.mjs` effect in it.

Frame count: `n = round(duration * fps)`. A loop renders `t = i * duration / n` for `i < n`.
A one-shot adds frame `n` at `t = duration`, which must be empty.

A library page (`lib`) is one category: `# Title`, a line or two, then per effect a heading
`## <category>/<name>`, one line of description, and a fenced `js` block whose first line is
`// id: <category>/<name>`. Library effects import `./pxfx.mjs`, `./pxact.mjs` and other
library effects as `./<category>-<name>.mjs`.

## Library

Colours are `'#rrggbb'` or `'#rgb'` strings, `0xRRGGBB` numbers, or `[r, g, b]` arrays,
everywhere. Painter alpha `a` (default 1) below 1 is drawn with the 4x4 ordered dither;
`puff` alpha really blends. Angles are radians, clockwise on screen, 0 = +x; the placement
`angle` option is in degrees.

| Export | What it does |
| --- | --- |
| `frame(w, h)`, `clear(f)` | A frame: `color` (RGBA bytes), `light` (RGB floats), counters `outside`, `spill`, `faint`. |
| `composite(f, bg)` | RGBA bytes. `bg = null` keeps transparency and bakes light into colour and alpha; `bg` may be a colour or `(x, y) => [r, g, b]`. |
| `painter(f, { x, y, flip, angle, light })` | Draws in local pixels. Shapes: `p.px(x, y, c, a)`, `p.line(x0, y0, x1, y1, c, a)`, `p.polyline(pts, c, a)`, `p.disc(x, y, r, c, a)`, `p.ring(x, y, r, width, c, a)`, `p.arc(x, y, r, a0, a1, width, c, a)` (width may be a function of u for tapered bands), `p.oval(x, y, rx, ry, c, a, { width, half })` (`half: 'back'` or `'front'`), `p.rect(x, y, w, h, c, a)`, `p.beam(x0, y0, x1, y1, w, c, a)` (capsule), `p.tri(x0, y0, x1, y1, x2, y2, c, a)`, `p.poly(pts, c, a)`. |
| | Soft and light: `p.puff(x, y, r, c, a)` (blended), `p.glow(x, y, r, c, s, squash)` (additive, radial falloff), `p.lit(x, y, c, s)` (additive pixel), `p.shade(x0, y0, x1, y1, fn)` (per pixel: `fn(x, y)` returns `null` or `{ c, a, l, k }`, colour and alpha and/or light colour and strength). |
| | Sprites and text: `p.sprite(rows, pal, x, y, { scale, a, flip, tint })` (string rows, characters missing from `pal` are clear, `tint` paints every pixel one colour), `p.text(str, x, y, c, { font, scale, align, outline, shadow, a })` (fonts `'small'` 3x5 and `'big'` 5x7: digits, A-Z, `+ - ! ? . , : / % x`; returns the width), `p.measure(str, font)`. |
| | Masks and bodies: `p.mask(m, x, y, c, a)` and `p.outline(m, x, y, c, a)` (m is any `{ w, h, mask }`, such as `o.actor` or a `textMask`), `p.body(actor, fn, a)` (an actor's own pixels; `fn(rgb, x, y)` may return a new colour or null), `p.get(x, y)` (`[r, g, b, a]` under a point), `p.sub(x, y, flip, light)` (a painter placed inside this one). Also `p.screen(x, y)`, `p.local(X, Y)`, `p.w`, `p.h`. |
| `rng(seed)`, `hash(a, b, c)` | Seeded random: `next()`, `range(a, b)`, `int(a, b)` (inclusive), `pick(list)`, `chance(p)`, `sign()`. `hash`: a stable number in [0, 1) from integers. |
| `noise(x, y, seed, px, py)`, `fbm(x, y, seed, octaves, px, py)`, `loopNoise(x, phase, seed, P)`, `tileFbm(x, y, px, py, seed)` | Value noise; `px`/`py` make it repeat. `loopNoise` repeats when phase goes 0 to 1; `tileFbm` tiles for seamless scrolling. |
| `ease.*`, `clamp`, `lerp`, `smoothstep`, `fract`, `TAU` | `linear`, `inQuad`, `outQuad`, `inOutQuad`, `inCubic`, `outCubic`, `outQuart`, `outExpo`, `inOutSine`, `outBack`. Inputs are clamped to 0..1. |
| `fly(t, x0, y0, vx, vy, g, drag)`, `burst(n, r, s0, s1, a0, a1, jitter)` | Closed-form `[x, y]` of a thrown point with gravity and linear drag. `n` launch velocities `[vx, vy]` spread over angles a0..a1 (r is an rng). |
| `RAMPS`, `palette(p)`, `ramp(p, n, mix)`, `shade(p, u)`, `rgb(c)`, `toHex(c)` | 27 ramps, dark to bright: fire ember lava smoke dust ice water electric volt poison holy heal arcane shadow blood earth stone gold leaf wood metal bone neon cyber matrix candy night. `ramp` resamples to n steps; `shade(p, u)` picks the hard step for u in 0..1. |
| `particles(r)` | `spawn(x, y, vx, vy, life, opts)`, `update(dt)`, `draw(p)` (front), `drawBack(p)` (particles with `back: true`), `list`, `alive`. Opts: `kind`, `size`, `ramp`, `gravity`, `drag` (negative accelerates), `bounce`, `floor` (default 0; `null` none), `delay`, `glow`, `wander`, `grow`, `alpha`, `spin`, `shade`, `back`, `tail`, `to`, `haze`, `smokes`, `onLand(x, y)`. |
| | Kinds: `spark` (speed-stretched streak, falls, bounces, glows), `ember` (rises, wanders, cools), `smoke` (blended puff, rises, swells, thins), `debris` (tumbles, bounces, settles), `mote` (head and a dark tail that dithers out), `streak` (accelerating dotted line; aim it inward with `to: [x, y]` for gathering), `flake` (drifting speck, a cross at size 2), `vapour` (soft puff that swells and thins; `haze` adds glow), `dust` (dense low puff that rolls along the floor), `fire` (flame ball cooling as it rises; `smokes` turns it to smoke), `brand` (burning chunk trailing flame; `onLand`), `grit` (tiny bits, bounce once, settle), `star` (twinkling cross). |
| `lights()`, `blazes(ps, ramp)` | `lights`: `add(x, y, r, c, k, life)` flares a glow that falls off with life squared; `update(dt)`, `draw(p)`. `blazes`: `add(x, y, life, power)` keeps feeding `fire` particles into `ps` with a floor glow; `update(dt)`, `draw(p)`. |
| `sparkle(p, x, y, arm, c, tip, a)`, `flare(p, x, y, len, k, ramp)` | A 4-point glint (arm ends in `tip`); an 8-ray light flare. |
| `band(p, cx, cy, r, squash, thick, a, ramp, keep)`, `FAR`, `NEAR` | A ring on a tilted floor plane, near side brighter; `keep(angle)` picks a part: `FAR` in `drawBack`, `NEAR` in `draw`. |
| `crystal(p, x, base, h, w, lean, ramp)` | A faceted spike: dark edge, two facets, a lit ridge, a glint at the tip. |
| `bolt(r, x0, y0, x1, y1, rough, depth)`, `forks(r, pts, n, len, rough, depth)` | Lightning points by midpoint displacement; side branches of a bolt. Draw with `p.polyline`. |
| `trail(p, pts, ramp, w, a)`, `ngon(n, r, rot)`, `star(n, r1, r2, rot)` | A line through points (tail first) that thins, darkens and dithers out at the tail; corner lists around (0, 0) for `p.poly`. |
| `textMask(str, font, scale)`, `measure(str, font)` | Text as a `{ w, h, mask }`; text width. |
| `cycle(t, duration, n, fn, { seed, speed })` | n looping particles for pure effects: `fn(phase, i, r)` with `phase = (t / duration * speed + i / n) mod 1`; `r` is the same rng for particle i every frame. `speed` must be whole. |
| `options(meta, extra)`, `instance(mod, extra)`, `frameTimes(meta)`, `timingOf(mod)`, `STEP` | Run an effect: `inst.seek(t)`, `inst.advance(dt)`, `inst.draw(p, layer)`, `inst.drawActor(p)`, `inst.alpha()`, `inst.paint(f, x, y)`, `inst.done`. `timingOf` returns `meta.timing` or the timing derived from `act`. |
| `bayer(x, y)` | The dither threshold of a screen pixel, for custom wipes. |
| `pngBytes(w, h, rgba, deflateSync)`, `gifBytes(w, h, frames, fps)` | An RGBA8 PNG; a looping GIF89a from opaque RGBA frames (one palette, quantised if over 256 colours). Node `Buffer`s. |

```js
// pxfx.mjs - pixel-art VFX library and command-line tool. No dependencies.
// Library: import { frame, painter, particles, rng, ease, shade } from './pxfx.mjs'
// CLI:     node pxfx.mjs <command> ... [flags]   (Node 18+; act and actor features load ./pxact.mjs)
// Colours: '#rrggbb' or '#rgb' strings, 0xRRGGBB numbers, or [r, g, b] arrays.
// Alpha below 1 is drawn with 4x4 ordered dithering, except puff() which really blends.

// ---------------------------------------------------------------- math and motion
export const TAU = Math.PI * 2;
export const clamp = (x, a = 0, b = 1) => (x < a ? a : x > b ? b : x);
export const lerp = (a, b, u) => a + (b - a) * u;
export const fract = (x) => x - Math.floor(x);
export const smoothstep = (e0, e1, x) => {
  const u = clamp((x - e0) / (e1 - e0));
  return u * u * (3 - 2 * u);
};
const E = (f) => (u) => f(clamp(u));
export const ease = {
  linear: E((u) => u),
  inQuad: E((u) => u * u),
  outQuad: E((u) => 1 - (1 - u) * (1 - u)),
  inOutQuad: E((u) => (u < 0.5 ? 2 * u * u : 1 - 2 * (1 - u) * (1 - u))),
  inCubic: E((u) => u * u * u),
  outCubic: E((u) => 1 - (1 - u) ** 3),
  outQuart: E((u) => 1 - (1 - u) ** 4),
  outExpo: E((u) => (u >= 1 ? 1 : 1 - 2 ** (-10 * u))),
  inOutSine: E((u) => 0.5 - 0.5 * Math.cos(Math.PI * u)),
  outBack: E((u) => 1 + 2.70158 * (u - 1) ** 3 + 1.70158 * (u - 1) ** 2),
};
export function fly(t, x0, y0, vx, vy, g = 0, drag = 0) {
  if (drag <= 0) return [x0 + vx * t, y0 + vy * t + 0.5 * g * t * t];
  const e = (1 - Math.exp(-drag * t)) / drag;
  return [x0 + vx * e, y0 + (vy - g / drag) * e + (g / drag) * t];
}

// ---------------------------------------------------------------- seeded random
function mix32(h) {
  h ^= h >>> 16; h = Math.imul(h, 0x7feb352d);
  h ^= h >>> 15; h = Math.imul(h, 0x846ca68b);
  return (h ^ (h >>> 16)) >>> 0;
}
export const hash = (a, b = 0, c = 0) =>
  mix32(Math.imul(a | 0, 0x27d4eb2d) ^ mix32(Math.imul(b | 0, 0x165667b1) ^ mix32((c | 0) + 0x9e3779b9))) / 4294967296;
function seedOf(seed) {
  if (typeof seed === 'string') { let h = 2166136261; for (const ch of seed) h = Math.imul(h ^ ch.charCodeAt(0), 16777619); return h >>> 0; }
  return mix32(Math.floor(Number(seed) || 0) + 1);
}
export function rng(seed = 1) {
  let s = seedOf(seed);
  const next = () => {
    s = (s + 0x6d2b79f5) | 0;
    let t = Math.imul(s ^ (s >>> 15), s | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
  return {
    next,
    range: (a, b) => a + (b - a) * next(),
    int: (a, b) => a + Math.floor(next() * (b - a + 1)),
    pick: (list) => list[Math.floor(next() * list.length)],
    chance: (p) => next() < p,
    sign: () => (next() < 0.5 ? -1 : 1),
  };
}
export function burst(n, r, s0 = 40, s1 = 80, a0 = 0, a1 = TAU, jitter = 0.4) {
  const step = (a1 - a0) / (Math.abs(a1 - a0) >= TAU - 1e-9 ? n : Math.max(1, n - 1));
  return Array.from({ length: n }, (_, i) => {
    const a = a0 + step * (i + r.range(-jitter, jitter) / 2), s = r.range(s0, s1);
    return [Math.cos(a) * s, Math.sin(a) * s];
  });
}

// ---------------------------------------------------------------- noise (can loop)
export function noise(x, y, seed = 0, px = 0, py = 0) {
  const xi = Math.floor(x), yi = Math.floor(y), fx = x - xi, fy = y - yi;
  const wrap = (i, p) => (p > 0 ? ((i % p) + p) % p : i);
  const g = (i, j) => hash(wrap(i, px), wrap(j, py), seed);
  const sx = fx * fx * (3 - 2 * fx), sy = fy * fy * (3 - 2 * fy);
  return lerp(lerp(g(xi, yi), g(xi + 1, yi), sx), lerp(g(xi, yi + 1), g(xi + 1, yi + 1), sx), sy);
}
// fbm: octaves of noise. Whole-number periods stay whole at every octave, so loops stay seamless.
export function fbm(x, y, seed = 0, octaves = 3, px = 0, py = 0) {
  let sum = 0, amp = 0.5, f = 1, norm = 0;
  for (let k = 0; k < octaves; k++) {
    sum += amp * noise(x * f, y * f, seed + k * 101, px * f, py * f);
    norm += amp; amp *= 0.5; f *= 2;
  }
  return sum / norm;
}
export const loopNoise = (x, phase, seed = 0, P = 4) => fbm(x, phase * P, seed, 3, 0, P);
export const tileFbm = (x, y, px, py, seed = 0) => fbm(x, y, seed, 3, px, py);

// ---------------------------------------------------------------- colour and palettes
export const RAMPS = {
  fire: ['#2e0d12', '#6b1717', '#b3361b', '#e8701e', '#f9b233', '#fff1a8'],
  ice: ['#0f2440', '#1d4f80', '#3189c4', '#6cc6ec', '#bdf0ff', '#ffffff'],
  poison: ['#16220f', '#2c4a16', '#4c7d1c', '#86bd2a', '#cdf27a'],
  holy: ['#3d2a10', '#8a5c1a', '#d69a2e', '#f6d567', '#fff6d2', '#ffffff'],
  shadow: ['#0b0712', '#1f1233', '#3b1d5e', '#633394', '#9d6bd0'],
  electric: ['#0e1640', '#2335a3', '#3b74f5', '#78c8ff', '#d4f4ff', '#ffffff'],
  smoke: ['#1b1a1f', '#312f37', '#4a4752', '#67636e', '#8a8691'],
  gold: ['#36210a', '#74490f', '#b9831b', '#e8bf3c', '#fff096'],
  blood: ['#260508', '#550b11', '#8c1520', '#c22637', '#ec5866'],
  water: ['#091a36', '#113d73', '#1c69b0', '#45a2e3', '#a3dbff'],
  arcane: ['#180a2c', '#38136a', '#7228ae', '#bb55e3', '#efaefc'],
  ember: ['#1c0a08', '#4a1410', '#8a2a12', '#c8501a', '#f08a30'],
  lava: ['#200806', '#5a120a', '#a8280c', '#e8601a', '#ffb040', '#fff0a0'],
  dust: ['#2a241e', '#4a4036', '#6e6252', '#968872', '#beb298'],
  volt: ['#1a1a08', '#4a4410', '#9a8c18', '#e8d830', '#fff890', '#ffffff'],
  heal: ['#0c2414', '#16502a', '#2a8a40', '#62c860', '#b8f0a0', '#f4fff0'],
  earth: ['#1e140c', '#3e2a18', '#664428', '#8e6a40', '#b8946a'],
  stone: ['#18181c', '#34343c', '#55555e', '#7a7a84', '#a6a6ae'],
  leaf: ['#0e200c', '#1e4416', '#346e22', '#58a032', '#9cd85a'],
  wood: ['#1e1008', '#3e2410', '#64401e', '#8c6030', '#b8884c'],
  metal: ['#16181e', '#363c48', '#5e6878', '#96a2b4', '#d4dce8', '#ffffff'],
  bone: ['#2a2420', '#5a4e42', '#8e8270', '#c4baa4', '#eee8d6'],
  neon: ['#1a0624', '#4a0c5a', '#9a14a0', '#f030c8', '#ff8ce8', '#ffffff'],
  cyber: ['#061a24', '#0c3a4e', '#127484', '#1cbcc0', '#7cf4e8', '#ffffff'],
  matrix: ['#020c04', '#063414', '#0c6624', '#20a838', '#70f070', '#d8ffd8'],
  candy: ['#3a0c28', '#7a1e54', '#c23c84', '#f07ab4', '#ffc4e0', '#ffffff'],
  night: ['#04040e', '#0a0c24', '#141a40', '#24305e', '#3c4c80'],
};
const colCache = new Map();
export function rgb(c) {
  if (Array.isArray(c)) return c;
  let v = colCache.get(c);
  if (v) return v;
  let n;
  if (typeof c === 'number') n = c;
  else {
    let s = String(c).replace(/^#/, '');
    if (s.length === 3) s = s.replace(/./g, '$&$&');
    n = parseInt(s, 16);
    if (s.length !== 6 || Number.isNaN(n)) throw new Error(`bad colour: ${c}`);
  }
  v = [(n >> 16) & 255, (n >> 8) & 255, n & 255];
  colCache.set(c, v);
  return v;
}
export const toHex = (c) => '#' + rgb(c).map((v) => Math.round(v).toString(16).padStart(2, '0')).join('');
export function palette(p) {
  if (Array.isArray(p) && p.length) return p;
  if (typeof p === 'string' && RAMPS[p]) return RAMPS[p];
  throw new Error(`bad palette: ${JSON.stringify(p)} (use an array of colours or one of ${Object.keys(RAMPS).join(', ')})`);
}
export function ramp(p, n, mix = false) {
  p = palette(p);
  if (n <= 1) return [p[p.length - 1]];
  const out = [];
  for (let i = 0; i < n; i++) {
    const u = (i / (n - 1)) * (p.length - 1);
    if (!mix) { out.push(p[Math.round(u)]); continue; }
    const i0 = Math.floor(u), i1 = Math.min(p.length - 1, i0 + 1), a = rgb(p[i0]), b = rgb(p[i1]);
    out.push(toHex(a.map((v, k) => lerp(v, b[k], u - i0))));
  }
  return out;
}
export function shade(p, u) {
  p = palette(p);
  return p[Math.min(p.length - 1, Math.floor(clamp(u) * p.length))];
}

// ---------------------------------------------------------------- frame and dithering
// A frame has two buffers: color (RGBA bytes) and light (additive RGB floats, 0..255).
export function frame(w, h) {
  return { w, h, color: new Uint8ClampedArray(w * h * 4), light: new Float32Array(w * h * 3), outside: 0, spill: 0, faint: 0 };
}
export function clear(f) { f.color.fill(0); f.light.fill(0); f.outside = f.spill = f.faint = 0; return f; }
const BAYER = [0, 8, 2, 10, 12, 4, 14, 6, 3, 11, 1, 9, 15, 7, 13, 5];
export const bayer = (x, y) => (BAYER[((y & 3) << 2) | (x & 3)] + 0.5) / 16;

// composite(f, bg, out): RGBA bytes. bg = null keeps transparency and bakes light into colour
// and alpha (reads as additive over dark scenes). bg can also be a colour or a (x, y) => [r,g,b].
export function composite(f, bg = null, out = new Uint8ClampedArray(f.w * f.h * 4)) {
  const { w, h, color, light } = f;
  const flat = bg == null || typeof bg === 'function' ? null : rgb(bg);
  for (let y = 0, i = 0; y < h; y++) {
    for (let x = 0; x < w; x++, i++) {
      const j = i * 4, k = i * 3, a = color[j + 3] / 255;
      if (bg == null) {
        const r = Math.min(255, color[j] * a + light[k]);
        const g = Math.min(255, color[j + 1] * a + light[k + 1]);
        const b = Math.min(255, color[j + 2] * a + light[k + 2]);
        const oa = Math.max(a, r / 255, g / 255, b / 255);
        if (oa <= 0) { out[j] = out[j + 1] = out[j + 2] = out[j + 3] = 0; continue; }
        out[j] = r / oa; out[j + 1] = g / oa; out[j + 2] = b / oa; out[j + 3] = oa * 255;
      } else {
        const B = flat || bg(x, y);
        out[j] = color[j] * a + B[0] * (1 - a) + light[k];
        out[j + 1] = color[j + 1] * a + B[1] * (1 - a) + light[k + 1];
        out[j + 2] = color[j + 2] * a + B[2] * (1 - a) + light[k + 2];
        out[j + 3] = 255;
      }
    }
  }
  return out;
}

// ---------------------------------------------------------------- bitmap fonts
// small 3x5 and big 5x7. One glyph per CH character; each row is one digit in base 8 or 32.
const CH = '0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ+-!?.,:/%x ';
const FONTS = {
  small: [3, 5, 8, ('75557 26227 71747 71717 55711 74717 74757 71222 75757 75717 25755 65656 34443 65556 74647 74644 ' +
    '34553 55755 72227 11152 55655 44447 57755 65555 25552 65644 25563 65655 34216 72222 55557 55552 55775 55255 55222 ' +
    '71247 02720 00700 22202 61202 00002 00024 02020 11244 51245 05250 00000').split(' ')],
  big: [5, 7, 32, ('ehjlphe 4c4444e eh1248v u11e11u 26aiv22 vgu11he 68guhhe v124888 ehhehhe ehhf12c ehhvhhh uhhuhhu ' +
    'ehggghe sihhhis vgguggv vgguggg ehgnhhf hhhvhhh e44444e 72222ic hikokih ggggggv hrllhhh hhpljhh ehhhhhe uhhuggg ' +
    'ehhhlid uhhukih fgge11u v444444 hhhhhhe hhhhha4 hhhllla hha4ahh hha4444 v1248gv 044v440 000v000 4444404 eh12404 ' +
    '00000cc 0000c48 0cc0cc0 11248gg pq248bj 00ha4ah 0000000').split(' ')],
};
export const measure = (str, font = 'small') => Math.max(0, String(str).length * ((FONTS[font] ?? FONTS.small)[0] + 1) - 1);
export function textMask(str, font = 'small', scale = 1) {
  const [gw, gh, base, glyphs] = FONTS[font] ?? FONTS.small, s = String(str);
  const w = measure(s, font) * scale, h = gh * scale, mask = new Uint8Array(Math.max(1, w * h));
  [...s].forEach((ch, n) => {
    let i = CH.indexOf(ch); if (i < 0) i = CH.indexOf(ch.toUpperCase());
    const g = glyphs[i < 0 ? CH.length - 1 : i];
    for (let j = 0; j < gh * scale; j++) for (let k = 0; k < gw * scale; k++)
      if ((parseInt(g[(j / scale) | 0], base) >> (gw - 1 - ((k / scale) | 0))) & 1) mask[j * w + n * (gw + 1) * scale + k] = 1;
  });
  return { w, h, mask };
}

// ---------------------------------------------------------------- painter
// painter(f, { x, y, flip, angle, light }): draws in local pixel coordinates.
// (x, y) is the canvas pixel where local (0, 0) sits. flip mirrors x. angle is in degrees,
// clockwise on screen. light multiplies every glow and lit call.
// Local units are pixels; a shape centred on (0, 0) is centred on that pixel.
export function painter(f, place = {}) {
  const { w, h, color, light } = f;
  const ox = place.x ?? 0, oy = place.y ?? 0, fl = place.flip ? -1 : 1;
  const an = ((place.angle ?? 0) * Math.PI) / 180, ca = Math.cos(an), sa = Math.sin(an);
  const L = place.light ?? 1;
  const screen = (x, y) => { const lx = x * fl; return [ox + lx * ca - y * sa, oy + lx * sa + y * ca]; };
  const local = (X, Y) => { const dx = X - ox, dy = Y - oy; return [(dx * ca + dy * sa) * fl, -dx * sa + dy * ca]; };
  const plot = (X, Y, c, a) => {
    if (a <= 0 || (a < 1 && a <= bayer(X, Y))) return;
    if (X < 0 || Y < 0 || X >= w || Y >= h) { f.outside++; return; }
    const i = (Y * w + X) * 4;
    color[i] = c[0]; color[i + 1] = c[1]; color[i + 2] = c[2]; color[i + 3] = 255;
  };
  const blend = (X, Y, c, a) => {
    if (a <= 0.004) return;
    if (X < 0 || Y < 0 || X >= w || Y >= h) { if (a > 0.05) f.outside++; return; }
    const i = (Y * w + X) * 4, da = color[i + 3] / 255, oa = a + da * (1 - a);
    for (let k = 0; k < 3; k++) color[i + k] = (c[k] * a + color[i + k] * da * (1 - a)) / oa;
    color[i + 3] = oa * 255;
  };
  const add = (X, Y, c, s) => {
    if (s <= 0) return;
    const r = c[0] * s, g = c[1] * s, b = c[2] * s;
    if (X < 0 || Y < 0 || X >= w || Y >= h) { if (r + g + b > 90) f.spill++; else if (Math.max(r, g, b) >= 4) f.faint++; return; }
    const i = (Y * w + X) * 3;
    light[i] = Math.min(255, light[i] + r);
    light[i + 1] = Math.min(255, light[i + 1] + g);
    light[i + 2] = Math.min(255, light[i + 2] + b);
  };
  // Visit every screen pixel whose centre maps inside a local box around (cx, cy).
  // test(dx, dy) gets the local offset from (cx, cy) and returns a coverage 0..1.
  const fill = (cx, cy, hx, hy, test, put) => {
    const [sx, sy] = screen(cx, cy);
    const ex = Math.abs(ca) * hx + Math.abs(sa) * hy + 1, ey = Math.abs(sa) * hx + Math.abs(ca) * hy + 1;
    const x0 = Math.floor(sx - ex), x1 = Math.ceil(sx + ex), y0 = Math.floor(sy - ey), y1 = Math.ceil(sy + ey);
    for (let Y = y0; Y <= y1; Y++) {
      for (let X = x0; X <= x1; X++) {
        const [lx, ly] = local(X, Y), k = test(lx - cx, ly - cy);
        if (k > 0) put(X, Y, k);
      }
    }
  };
  const R = (v) => Math.floor(v + 0.5);
  const p = {
    w, h, frame: f, flip: fl < 0, angle: place.angle ?? 0, screen, local,
    px(x, y, c, a = 1) { const [X, Y] = screen(x, y); plot(R(X), R(Y), rgb(c), a); },
    line(x0, y0, x1, y1, c, a = 1) {
      const [X0, Y0] = screen(x0, y0), [X1, Y1] = screen(x1, y1), col = rgb(c);
      const n = Math.max(1, Math.ceil(Math.max(Math.abs(X1 - X0), Math.abs(Y1 - Y0))));
      let lastX = NaN, lastY = NaN;
      for (let i = 0; i <= n; i++) {
        const X = R(lerp(X0, X1, i / n)), Y = R(lerp(Y0, Y1, i / n));
        if (X !== lastX || Y !== lastY) plot(X, Y, col, a);
        lastX = X; lastY = Y;
      }
    },
    polyline(pts, c, a = 1) { for (let i = 1; i < pts.length; i++) p.line(...pts[i - 1], ...pts[i], c, a); },
    disc(x, y, r, c, a = 1) {
      if (r < 0.5) return p.px(x, y, c, a);
      const col = rgb(c), rr = (r + 0.5) ** 2;
      fill(x, y, r + 1, r + 1, (dx, dy) => (dx * dx + dy * dy <= rr ? 1 : 0), (X, Y) => plot(X, Y, col, a));
    },
    ring(x, y, r, width, c, a = 1) { p.arc(x, y, r, 0, TAU, width, c, a); },
    // arc: the band between radius r - width and r, from angle a0 to a1 (radians, clockwise
    // on screen, 0 = +x). width can be a function of u (0 at a0, 1 at a1) for tapered bands.
    arc(x, y, r, a0, a1, width, c, a = 1) {
      const col = rgb(c), span = a1 - a0, wf = typeof width === 'function' ? width : () => width;
      fill(x, y, r + 1, r + 1, (dx, dy) => {
        const d = Math.hypot(dx, dy);
        if (d > r + 0.5) return 0;
        let u = 1;
        if (span < TAU - 1e-9) {
          const rel = (((Math.atan2(dy, dx) - a0) % TAU) + TAU) % TAU;
          if (rel > span) return 0;
          u = rel / span;
        }
        return d > r + 0.5 - wf(u) ? 1 : 0;
      }, (X, Y) => plot(X, Y, col, a));
    },
    // oval: filled ellipse, or a ring when opt.width is set. opt.half = 'back' keeps the far
    // (upper) half, 'front' the near half. Use for floor rings on a tilted plane.
    oval(x, y, rx, ry, c, a = 1, opt = {}) {
      const col = rgb(c), ax = rx + 0.5, ay = ry + 0.5, wd = opt.width ?? 0, half = opt.half;
      fill(x, y, rx + 1, ry + 1, (dx, dy) => {
        if (half === 'back' && dy >= 0) return 0;
        if (half === 'front' && dy < 0) return 0;
        const f = (dx / ax) ** 2 + (dy / ay) ** 2 - 1;
        if (f > 0) return 0;
        if (wd <= 0) return 1;
        // first-order distance to the edge keeps a ring of even width on flat ellipses
        const g = 2 * Math.hypot(dx / (ax * ax), dy / (ay * ay));
        return g > 0 && -f / g < wd ? 1 : 0;
      }, (X, Y) => plot(X, Y, col, a));
    },
    // rect: covers local pixels x .. x+w-1, y .. y+h-1.
    rect(x, y, rw, rh, c, a = 1) {
      const col = rgb(c), cx = x + (rw - 1) / 2, cy = y + (rh - 1) / 2;
      fill(cx, cy, rw / 2, rh / 2, (dx, dy) => (Math.abs(dx) < rw / 2 && Math.abs(dy) < rh / 2 ? 1 : 0), (X, Y) => plot(X, Y, col, a));
    },
    beam(x0, y0, x1, y1, wd, c, a = 1) {
      const col = rgb(c), dx = x1 - x0, dy = y1 - y0, L2 = dx * dx + dy * dy || 1, rr = (wd / 2) ** 2 + 0.05;
      fill((x0 + x1) / 2, (y0 + y1) / 2, Math.abs(dx) / 2 + wd, Math.abs(dy) / 2 + wd, (ex, ey) => {
        const qx = ex + dx / 2, qy = ey + dy / 2, u = clamp((qx * dx + qy * dy) / L2);
        return (qx - u * dx) ** 2 + (qy - u * dy) ** 2 <= rr ? 1 : 0;
      }, (X, Y) => plot(X, Y, col, a));
    },
    poly(pts, c, a = 1) {
      const col = rgb(c), xs = pts.map((q) => q[0]), ys = pts.map((q) => q[1]);
      const x0 = Math.min(...xs), x1 = Math.max(...xs), y0 = Math.min(...ys), y1 = Math.max(...ys), cx = (x0 + x1) / 2, cy = (y0 + y1) / 2;
      fill(cx, cy, (x1 - x0) / 2, (y1 - y0) / 2, (dx, dy) => inPoly(pts, cx + dx, cy + dy), (X, Y) => plot(X, Y, col, a));
    },
    tri(x0, y0, x1, y1, x2, y2, c, a = 1) { p.poly([[x0, y0], [x1, y1], [x2, y2]], c, a); },
        shade(x0, y0, x1, y1, fn) {
      let r = null;
      const cx = (x0 + x1) / 2, cy = (y0 + y1) / 2;
      fill(cx, cy, (x1 - x0) / 2, (y1 - y0) / 2, (dx, dy) => {
        const lx = R(cx + dx), ly = R(cy + dy);
        r = lx < x0 || lx > x1 || ly < y0 || ly > y1 ? null : fn(lx, ly);
        return r ? 1 : 0;
      }, (X, Y) => { if (r.c != null) plot(X, Y, rgb(r.c), r.a ?? 1); if (r.l != null) add(X, Y, rgb(r.l), (r.k ?? 1) * L); });
    },
    sprite(rows, pal, x, y, { scale = 1, a = 1, flip = false, tint } = {}) {
      rows.forEach((row, j) => [...row].forEach((ch, i) => {
        if (pal[ch] == null) return;
        const k = flip ? row.length - 1 - i : i;
        p.rect(x + k * scale, y + j * scale, scale, scale, tint ?? pal[ch], a);
      }));
    },
    text(str, x, y, c, { font = 'small', scale = 1, align = 'left', outline, shadow, a = 1 } = {}) {
      const m = textMask(str, font, scale), x0 = R(x - (align === 'center' ? m.w / 2 : align === 'right' ? m.w : 0));
      if (shadow != null) p.mask(m, x0 + scale, y + scale, shadow, a);
      if (outline != null) p.outline(m, x0, y, outline, a);
      p.mask(m, x0, y, c, a);
      return m.w;
    },
    measure,
    mask(m, x, y, c, a = 1) { for (let j = 0; j < m.h; j++) for (let i = 0; i < m.w; i++) if (m.mask[j * m.w + i]) p.px(x + i, y + j, c, a); },
    outline(m, x, y, c, a = 1) {
      const on = (i, j) => i >= 0 && j >= 0 && i < m.w && j < m.h && m.mask[j * m.w + i];
      for (let j = -1; j <= m.h; j++) for (let i = -1; i <= m.w; i++)
        if (!on(i, j) && (on(i - 1, j) || on(i + 1, j) || on(i, j - 1) || on(i, j + 1))) p.px(x + i, y + j, c, a);
    },
    body(m, fn = null, a = 1) {
      for (let j = 0; j < m.h; j++) for (let i = 0; i < m.w; i++) {
        const k = j * m.w + i;
        if (!m.mask[k]) continue;
        const c = fn ? fn(m.pixels[k], m.x + i, m.y + j) : m.pixels[k];
        if (c != null) p.px(m.x + i, m.y + j, c, a);
      }
    },
    get(x, y) {
      const [X, Y] = screen(x, y), i = (R(Y) * w + R(X)) * 4;
      return R(X) < 0 || R(Y) < 0 || R(X) >= w || R(Y) >= h ? null : [...color.subarray(i, i + 4)];
    },
    sub(x, y, flip = false, light = 1) {
      const [X, Y] = screen(x, y);
      return painter(f, { x: X, y: Y, flip: fl < 0 !== !!flip, angle: place.angle, light: L * light });
    },
    puff(x, y, r, c, a = 0.5) {
      const col = rgb(c);
      if (r < 0.5) { const [X, Y] = screen(x, y); return blend(R(X), R(Y), col, a); }
      const rr = (r + 0.5) ** 2, inner = Math.max(0, r - 0.5) ** 2;
      fill(x, y, r + 1, r + 1, (dx, dy) => { const d = dx * dx + dy * dy; return d <= inner ? 1 : d <= rr ? 0.55 : 0; },
        (X, Y, k) => blend(X, Y, col, a * k));
    },
    // glow: additive light with a soft radial falloff. squash < 1 flattens it (floor pools).
    glow(x, y, r, c, s = 1, squash = 1) {
      const col = rgb(c), st = s * L;
      if (st <= 0 || r <= 0) return;
      const ry = Math.max(0.5, r * squash);
      fill(x, y, r, ry, (dx, dy) => { const d = Math.hypot(dx / r, dy / ry); return d < 1 ? (1 - d) * (1 - d) : 0; },
        (X, Y, k) => add(X, Y, col, st * k));
    },
    lit(x, y, c, s = 1) { const [X, Y] = screen(x, y); add(R(X), R(Y), rgb(c), s * L); },
  };
  return p;
}
function inPoly(pts, x, y) {
  let k = 0;
  for (let i = 0, j = pts.length - 1; i < pts.length; j = i++) {
    const [xi, yi] = pts[i], [xj, yj] = pts[j];
    if (yi > y !== yj > y && x < ((xj - xi) * (y - yi)) / (yj - yi) + xi) k ^= 1;
  }
  return k;
}

// ---------------------------------------------------------------- shapes
const D4 = [[1, 0], [-1, 0], [0, 1], [0, -1]];
export function sparkle(p, x, y, arm, c, tip = c, a = 1) {
  arm = Math.round(arm);
  if (arm < 0) return;
  p.px(x, y, c, a); p.lit(x, y, c, 0.5 * a);
  for (let k = 1; k <= arm; k++) for (const [dx, dy] of D4) p.px(x + dx * k, y + dy * k, k === arm ? tip : c, a);
}
export function flare(p, x, y, len, k = 1, rmp = 'holy') {
  for (let d = 0; d < 8; d++) {
    const a = (d * Math.PI) / 4, L = d % 2 ? len / 2 : len;
    for (let i = 1; i <= L; i++) p.lit(x + Math.cos(a) * i, y + Math.sin(a) * i, shade(rmp, 1 - i / L), k * (1 - i / L));
  }
  p.glow(x, y, len * 0.6, shade(rmp, 0.8), k * 0.5);
  p.lit(x, y, '#ffffff', k);
}
export const FAR = (a) => Math.sin(a) < 0, NEAR = (a) => Math.sin(a) >= 0;
export function band(p, cx, cy, r, squash = 0.3, thick = 1, a = 1, rmp = 'holy', keep = null) {
  const n = Math.ceil(TAU * r * 1.3) + 8;
  for (let i = 0; i < n; i++) {
    const t = (i / n) * TAU, s = Math.sin(t);
    if (keep && !keep(t)) continue;
    for (let k = 0; k < thick; k++) p.px(cx + Math.cos(t) * (r - k), cy + s * Math.max(0, r * squash - k), shade(rmp, 0.6 + 0.4 * s - k * 0.2), a);
  }
}
export function crystal(p, x, base, h, w, lean = 0, rmp = 'ice') {
  for (let j = 0; j <= h; j++) {
    const f = j / h, cx = x + lean * f, half = (w / 2) * (1 - f) ** 0.8, ridge = R0(cx + half * 0.25);
    for (let xx = R0(cx - half); xx <= R0(cx + half); xx++)
      p.px(xx, base - j, shade(rmp, xx === ridge ? 0.95 : xx > ridge ? 0.6 : xx === R0(cx - half) ? 0.1 : 0.35));
  }
  p.lit(x + lean, base - h, shade(rmp, 1), 0.8);
}
const R0 = (v) => Math.floor(v + 0.5);
export function bolt(r, x0, y0, x1, y1, rough = 0.3, depth = 5) {
  let pts = [[x0, y0], [x1, y1]], amp = Math.hypot(x1 - x0, y1 - y0) * rough;
  for (let l = 0; l < depth; l++, amp /= 2) {
    const out = [pts[0]];
    for (let i = 1; i < pts.length; i++) {
      const [ax, ay] = pts[i - 1], [bx, by] = pts[i], d = Math.hypot(bx - ax, by - ay) || 1, off = r.range(-amp, amp);
      out.push([(ax + bx) / 2 - ((by - ay) / d) * off, (ay + by) / 2 + ((bx - ax) / d) * off], pts[i]);
    }
    pts = out;
  }
  return pts;
}
export function forks(r, pts, n = 2, len = 0.4, rough = 0.35, depth = 3) {
  const total = Math.hypot(pts.at(-1)[0] - pts[0][0], pts.at(-1)[1] - pts[0][1]);
  return Array.from({ length: n }, () => {
    const i = r.int(1, pts.length - 2), [ax, ay] = pts[i], [bx, by] = pts[i + 1];
    const a = Math.atan2(by - ay, bx - ax) + r.sign() * r.range(0.4, 0.9), L = total * len * r.range(0.5, 1);
    return bolt(r, ax, ay, ax + Math.cos(a) * L, ay + Math.sin(a) * L, rough, depth);
  });
}
export function trail(p, pts, rmp, wd = 1, a = 1) {
  const n = pts.length - 1;
  for (let i = 1; i <= n; i++) {
    const u = i / n, c = shade(rmp, 0.2 + 0.8 * u), al = a * (0.3 + 0.7 * u);
    if (wd * u > 1.2) p.beam(...pts[i - 1], ...pts[i], wd * u, c, al); else p.line(...pts[i - 1], ...pts[i], c, al);
  }
}
export const ngon = (n, r, rot = 0) => Array.from({ length: n }, (_, i) => [Math.cos(rot + (i * TAU) / n) * r, Math.sin(rot + (i * TAU) / n) * r]);
export const star = (n, r1, r2, rot = 0) => Array.from({ length: n * 2 }, (_, i) => {
  const a = rot + (i * Math.PI) / n, r = i % 2 ? r2 : r1;
  return [Math.cos(a) * r, Math.sin(a) * r];
});

// ---------------------------------------------------------------- particles
const KINDS = {
  spark: { gravity: 260, drag: 1.2, bounce: 0.45, ramp: 'fire', size: 1, glow: 1, stretch: 0.03 },
  ember: { gravity: -35, drag: 1.5, bounce: 0, ramp: 'fire', size: 1, glow: 0.6, wander: 30 },
  smoke: { gravity: -14, drag: 2.5, bounce: 0, ramp: 'smoke', size: 3, grow: 2.2, alpha: 0.55 },
  debris: { gravity: 380, drag: 0.3, bounce: 0.35, ramp: 'smoke', size: 2, spin: 14 },
  mote: { gravity: 0, drag: 1.5, ramp: 'arcane', size: 1, glow: 0.5, tail: 3, floor: null },
  streak: { gravity: 0, drag: -2.5, ramp: 'arcane', size: 1, glow: 0.4, floor: null },
  flake: { gravity: 12, drag: 2, ramp: 'ice', size: 1, glow: 0.15, wander: 14, floor: null },
  vapour: { gravity: -10, drag: 2, ramp: 'ice', size: 3, grow: 2.5, alpha: 0.35, floor: null },
  dust: { gravity: 40, drag: 3, ramp: 'dust', size: 2.5, grow: 2, alpha: 0.7 },
  fire: { gravity: -45, drag: 2, ramp: 'fire', size: 2.5, glow: 0.5, wander: 20, floor: null },
  brand: { gravity: 300, drag: 0.4, bounce: 0.3, ramp: 'fire', size: 2, glow: 0.6, spin: 10 },
  grit: { gravity: 320, drag: 0.5, bounce: 0.35, ramp: 'dust', size: 1, once: true },
  star: { gravity: 0, drag: 2.5, ramp: 'holy', size: 2, glow: 0.6, floor: null },
};
export function particles(r) {
  const list = [];
  const sys = {
    list, r,
    get alive() { return list.length; },
    spawn(x, y, vx, vy, life, opts = {}) {
      const kind = opts.kind ?? 'spark';
      if (!KINDS[kind]) throw new Error(`unknown particle kind: ${kind}`);
      const q = { kind, x, y, vx, vy, life, age: 0, floor: 0, delay: 0, bounce: 0, ...KINDS[kind],
        phase: r.next() * TAU, rot: r.next() * TAU, jitter: r.next() };
      for (const k in opts) if (opts[k] !== undefined) q[k] = opts[k];
      q.ramp = palette(q.ramp);
      list.push(q);
      return q;
    },
    update(dt) {
      let n = 0;
      const born = [];
      for (const q of list) {
        if (q.delay > 0) { q.delay -= dt; list[n++] = q; continue; }
        q.age += dt;
        if (q.age >= q.life || (q.to && Math.hypot(q.x - q.to[0], q.y - q.to[1]) < 2)) {
          if (q.smokes) born.push([q.x, q.y, q.vx * 0.3, -10, q.life * 1.6, { kind: 'smoke', size: q.size * 0.8, back: q.back }]);
          continue;
        }
        const k = Math.exp(-q.drag * dt);
        q.vx *= k; q.vy *= k;
        q.vy += q.gravity * dt;
        if (q.wander) q.vx += Math.sin(q.age * 7 + q.phase) * q.wander * dt;
        q.x += q.vx * dt; q.y += q.vy * dt;
        if (q.floor != null && q.y > q.floor && q.vy > 0) {
          q.y = q.floor;
          q.vy = -q.vy * q.bounce; q.vx *= 0.6;
          if (Math.abs(q.vy) < 12) q.vy = 0;
          if (!q.grounded) q.onLand?.(q.x, q.y);
          q.grounded = true;
          if (q.once) q.bounce = 0;
        }
        if (q.spin) q.rot += q.spin * dt * Math.sign(q.vx || 1) * Math.min(1, Math.hypot(q.vx, q.vy) / 25);
        if (q.kind === 'brand' && (q.acc = (q.acc ?? 0) + dt * (q.grounded ? 10 : 40) * (1 - q.age / q.life)) >= 1) {
          q.acc = 0;
          born.push([q.x, q.y - 1, r.range(-8, 8), r.range(-30, -10), r.range(0.15, 0.35), { kind: 'fire', ramp: q.ramp, size: r.range(1, 2), back: q.back }]);
        }
        list[n++] = q;
      }
      list.length = n;
      for (const b of born) sys.spawn(...b);
    },
    draw(p) { for (const q of list) if (q.delay <= 0 && !q.back) drawParticle(p, q); },
    drawBack(p) { for (const q of list) if (q.delay <= 0 && q.back) drawParticle(p, q); },
  };
  return sys;
}
function drawParticle(p, q) {
  const u = clamp(q.age / q.life), fade = u < 0.7 ? 1 : 1 - (u - 0.7) / 0.3, K = q.kind;
  if (K === 'spark') {
    const sp = Math.hypot(q.vx, q.vy), len = Math.min(6 * q.size, sp * q.stretch);
    const hot = shade(q.ramp, 1 - u * 0.8), tail = shade(q.ramp, 0.75 - u * 0.75);
    if (len >= 1) p.line(q.x - (q.vx / sp) * len, q.y - (q.vy / sp) * len, q.x, q.y, tail, fade);
    p.px(q.x, q.y, hot, fade);
    p.lit(q.x, q.y, hot, 0.5 * q.glow * fade);
    if (q.glow > 0) p.glow(q.x, q.y, 3, hot, 0.12 * q.glow * fade);
  } else if (K === 'ember') {
    const c = shade(q.ramp, 1 - u);
    p.px(q.x, q.y, c, fade);
    if (q.size > 1 && u < 0.5) p.px(q.x, q.y + 1, shade(q.ramp, 0.6 - u), fade);
    p.lit(q.x, q.y, c, q.glow * (1 - u));
  } else if (K === 'smoke') {
    const rad = q.size * (1 + (q.grow - 1) * ease.outQuad(u));
    const a = q.alpha * (1 - u) ** 1.3 * Math.min(1, q.age / 0.08);
    p.puff(q.x, q.y, rad, shade(q.ramp, 0.25 + 0.2 * q.jitter), a);
    p.puff(q.x - rad * 0.2, q.y - rad * 0.35, rad * 0.6, shade(q.ramp, 0.75), a * 0.7);
  } else if (K === 'mote' || K === 'streak') {
    // mote: head plus a short tail that darkens and dithers out; streak: a dotted line that
    // grows with speed (spawn it outside and aim it inward to show energy gathering).
    const sp = Math.hypot(q.vx, q.vy) || 1, c = shade(q.ramp, 1 - u * 0.5);
    const n = K === 'mote' ? q.tail : Math.min(12, Math.round(sp * 0.06));
    for (let k = 1; k <= n; k++) {
      const v = 1 - k / (n + 1);
      if (K === 'mote' || k % 2 === 0) p.px(q.x - (q.vx / sp) * k, q.y - (q.vy / sp) * k, shade(q.ramp, v * (1 - u * 0.5)), fade * v);
    }
    p.px(q.x, q.y, c, fade);
    p.lit(q.x, q.y, c, q.glow * fade);
  } else if (K === 'flake' || K === 'star') {
    const c = shade(q.ramp, 1 - u * 0.5), mid = shade(q.ramp, 0.6 - u * 0.3);
    const arm = K === 'star' ? Math.round(q.size * (0.5 + 0.5 * Math.sin(q.age * 18 + q.phase)) * (1 - u * 0.5)) : q.size >= 2 ? 1 : 0;
    sparkle(p, q.x, q.y, arm, c, mid, fade);
    if (q.glow) p.lit(q.x, q.y, c, q.glow * fade);
  } else if (K === 'vapour' || K === 'dust') {
    const rad = q.size * (1 + (q.grow - 1) * ease.outQuad(u)), d = K === 'dust';
    const a = q.alpha * (1 - u) ** 1.2 * Math.min(1, q.age / 0.06), y = d ? q.y - rad * 0.6 : q.y;
    p.puff(q.x, y, rad, shade(q.ramp, d ? 0.3 + 0.2 * q.jitter : 0.55), a);
    p.puff(q.x - rad * 0.2, y - rad * 0.35, rad * 0.6, shade(q.ramp, d ? 0.7 : 0.9), a * 0.6);
    if (q.haze) p.glow(q.x, y, rad * 2, shade(q.ramp, 0.8), q.haze * (1 - u));
  } else if (K === 'fire') {
    // a flame ball that cools down the ramp as it rises: hot core low, cooler shell
    const rad = q.size * (1 - u * 0.6), c = shade(q.ramp, 1 - u);
    p.disc(q.x, q.y, rad, shade(q.ramp, 0.85 - u), fade);
    if (rad > 1) p.disc(q.x, q.y + rad * 0.3, rad * 0.5, c, fade);
    p.lit(q.x, q.y, c, q.glow * (1 - u));
  } else if (K === 'grit') {
    p.px(q.x, q.y, shade(q.ramp, q.shade ?? 0.4 + 0.5 * q.jitter), fade);
  } else {
    // debris and brand: 1 px speck, 2 px stick that turns, or a 2x2 chunk whose lit corner turns
    const br = K === 'brand', c = shade(q.ramp, br ? 0.1 : q.shade ?? 0.3 + 0.35 * q.jitter);
    const hi = shade(q.ramp, br ? 0.9 - u * 0.5 : 1), lo = shade(q.ramp, 0);
    const x = Math.round(q.x), y = Math.round(q.y);
    if (br) p.lit(x, y, hi, q.glow * (1 - u));
    if (q.size < 2) return p.px(x, y, br ? hi : c, fade);
    if (q.size < 3) {
      const dx = Math.round(Math.cos(q.rot)), dy = Math.round(Math.sin(q.rot));
      p.px(x, y, hi, fade); p.px(x + dx, y + dy, c, fade);
      return;
    }
    const k = ((Math.floor((q.rot / TAU) * 4) % 4) + 4) % 4, cells = [[0, -1], [1, -1], [1, 0], [0, 0]];
    cells.forEach(([cx, cy], j) => p.px(x + cx, y + cy, j === k ? hi : j === (k + 2) % 4 ? lo : c, fade));
  }
}
export function lights() {
  const list = [];
  return {
    list,
    add(x, y, r, c, k = 1, life = 0.3) { list.push({ x, y, r, c, k, life, age: 0 }); },
    update(dt) { for (let i = list.length; i--;) if ((list[i].age += dt) >= list[i].life) list.splice(i, 1); },
    draw(p) { for (const l of list) p.glow(l.x, l.y, l.r, l.c, l.k * (1 - l.age / l.life) ** 2); },
  };
}
export function blazes(ps, rmp = 'fire') {
  const list = [], r = ps.r;
  return {
    list,
    add(x, y, life = 1, power = 1) { list.push({ x, y, life, power, age: 0, acc: 0 }); },
    update(dt) {
      for (let i = list.length; i--;) {
        const b = list[i], k = 1 - (b.age += dt) / b.life;
        if (k <= 0) { list.splice(i, 1); continue; }
        for (b.acc += dt * 30 * b.power * k; b.acc >= 1; b.acc--)
          ps.spawn(b.x + r.range(-2, 2) * b.power, b.y, r.range(-6, 6), -r.range(12, 30), r.range(0.2, 0.45),
            { kind: 'fire', ramp: rmp, size: r.range(1, 2.2) * (0.5 + 0.5 * k) * b.power, smokes: r.chance(0.15) });
      }
    },
    draw(p) { for (const b of list) p.glow(b.x, b.y - 2, 10 * b.power, shade(rmp, 0.6), 0.35 * (1 - b.age / b.life), 0.5); },
  };
}

// cycle(t, duration, n, fn, opt): n looping particles for pure effects. Calls fn(phase, i, r)
// with phase = (t / duration * speed + i / n) mod 1 and r an rng that is the same for particle
// i on every frame. speed must be a whole number for a seamless loop.
export function cycle(t, duration, n, fn, { seed = 0, speed = 1 } = {}) {
  for (let i = 0; i < n; i++) {
    const r = rng(hash(i, seed, 7) * 4294967296);
    const phase = fract((t / duration) * speed + i / n);
    fn(phase, i, r);
  }
}

// ---------------------------------------------------------------- running an effect
const RESERVED = ['x', 'y', 'flip', 'angle', 'light', 'actor'];
export function options(meta, extra = {}) {
  const o = { ...(meta.options || {}), ...extra };
  if (o.palette !== undefined) o.palette = palette(o.palette);
  return o;
}
export const STEP = 1 / 120;
// instance(mod, extra): one running copy of an effect module, for games and for the CLI.
// seek(t) moves forward to time t (simulated effects step at a fixed 1/120 s);
// draw(p, layer) draws 'back', 'front' or 'both'. paint(f, x, y) makes a painter for it.
// drawActor(p) draws o.actor (the effect's actorPaint, or its pixels at actorAlpha).
// Loops wrap t into 0..duration; wrap: false passes t through (the seam check uses that).
export function instance(mod, extra = {}, { wrap = true } = {}) {
  const meta = mod.meta, o = options(meta, extra), sim = mod.create ? mod.create(o) : null;
  let t = 0, steps = 0;
  const tt = () => (!meta.loop ? Math.min(t, meta.duration) : wrap ? fract(t / meta.duration) * meta.duration : t);
  const inst = {
    meta, o,
    get t() { return t; },
    get done() { return !meta.loop && t >= meta.duration; },
    seek(time) {
      t = time;
      if (!sim) return;
      const target = Math.floor(t / STEP + 1e-6);
      while (steps < target) { sim.update(STEP); steps++; }
    },
    advance(dt) { inst.seek(t + dt); },
    draw(p, layer = 'both') {
      const u = tt();
      if (layer !== 'front') { if (sim) sim.drawBack?.(p); else mod.drawBack?.(p, u, o); }
      if (layer !== 'back') { if (sim) sim.draw(p); else mod.draw(p, u, o); }
    },
    alpha() { return mod.actorAlpha ? clamp(mod.actorAlpha(tt(), o)) : 1; },
    drawActor(p) {
      if (!o.actor) return;
      if (mod.actorPaint) mod.actorPaint(p, tt(), o); else p.body(o.actor, null, inst.alpha());
    },
    paint(f, x, y) { return painter(f, { x: x + (o.x ?? 0), y: y + (o.y ?? 0), flip: o.flip, angle: o.angle, light: o.light }); },
  };
  return inst;
}
export function frameTimes(meta) {
  const fps = meta.fps ?? 30, n = Math.max(1, Math.round(meta.duration * fps));
  const count = meta.loop ? n : n + 1;
  return Array.from({ length: count }, (_, i) => (i * meta.duration) / n);
}
export function timingOf(mod) {
  const m = mod.meta, k = mod.act?.poses;
  if (m.timing || !k?.length) return m.timing;
  const hits = mod.act.hits ?? [], last = hits.length ? hits[hits.length - 1].t : 0;
  return { windup: Math.max(0, -k[0][0]), hits: hits.map((x) => ({ t: x.t, freeze: x.freeze ?? 0, shake: x.shake ?? 0, amp: x.amp ?? 0 })),
    recovery: Math.max(0, +(k[k.length - 1][0] - last).toFixed(4)) };
}
function renderAll(mod, extra, times, { layer = 'both', timed = null, wrap = true, actor = false, box = null } = {}) {
  const [w, h, ox, oy] = box ?? [...mod.meta.size, ...mod.meta.origin];
  const inst = instance(mod, extra, { wrap }), out = [];
  for (const time of times) {
    const f = frame(w, h), t0 = timed ? process.hrtime.bigint() : 0n;
    inst.seek(time);
    const p = inst.paint(f, ox, oy);
    if (actor) { inst.draw(p, 'back'); inst.drawActor(p); inst.draw(p, 'front'); } else inst.draw(p, layer);
    if (timed) timed.push(Number(process.hrtime.bigint() - t0) / 1e6);
    out.push(f);
  }
  return out;
}

// ---------------------------------------------------------------- PNG and GIF writers
const CRC = (() => {
  const t = new Uint32Array(256);
  for (let n = 0; n < 256; n++) { let c = n; for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1; t[n] = c >>> 0; }
  return t;
})();
function crc32(buf) { let c = 0xffffffff; for (const b of buf) c = CRC[(c ^ b) & 255] ^ (c >>> 8); return (c ^ 0xffffffff) >>> 0; }
export function pngBytes(w, h, rgba, deflateSync) {
  const raw = Buffer.alloc((w * 4 + 1) * h);
  for (let y = 0; y < h; y++) Buffer.from(rgba.buffer, rgba.byteOffset + y * w * 4, w * 4).copy(raw, y * (w * 4 + 1) + 1);
  const chunk = (type, data) => {
    const len = Buffer.alloc(4); len.writeUInt32BE(data.length);
    const td = Buffer.concat([Buffer.from(type, 'ascii'), data]);
    const crc = Buffer.alloc(4); crc.writeUInt32BE(crc32(td));
    return Buffer.concat([len, td, crc]);
  };
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(w, 0); ihdr.writeUInt32BE(h, 4); ihdr[8] = 8; ihdr[9] = 6;
  return Buffer.concat([Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]), chunk('IHDR', ihdr),
    chunk('IDAT', deflateSync(raw, { level: 9 })), chunk('IEND', Buffer.alloc(0))]);
}
export function gifBytes(w, h, frames, fps) {
  let sh = 0, map;
  const key = (f, i) => ((f[i] >> sh) << 16) | ((f[i + 1] >> sh) << 8) | (f[i + 2] >> sh);
  for (;; sh++) {
    map = new Map();
    for (const f of frames) for (let i = 0; i < f.length && map.size <= 256; i += 4) { const k = key(f, i); if (!map.has(k)) map.set(k, map.size); }
    if (map.size <= 256) break;
  }
  let bits = 1;
  while (1 << bits < map.size) bits++;
  const out = [], u16 = (v) => out.push(v & 255, v >> 8), half = sh ? 1 << (sh - 1) : 0, min = Math.max(2, bits);
  out.push(...Buffer.from('GIF89a')); u16(w); u16(h); out.push(0xf0 | (bits - 1), 0, 0);
  const pal = new Array(3 << bits).fill(0);
  for (const [k, i] of map) for (let c = 0; c < 3; c++) pal[i * 3 + c] = Math.min(255, (((k >> (16 - 8 * c)) & 255) << sh) | half);
  out.push(...pal, 0x21, 0xff, 11, ...Buffer.from('NETSCAPE2.0'), 3, 1, 0, 0, 0);
  frames.forEach((f, n) => {
    out.push(0x21, 0xf9, 4, 0); u16(Math.round(((n + 1) * 100) / fps) - Math.round((n * 100) / fps)); out.push(0, 0, 0x2c);
    u16(0); u16(0); u16(w); u16(h); out.push(0, min);
    const idx = new Uint8Array(w * h);
    for (let i = 0; i < idx.length; i++) idx[i] = map.get(key(f, i * 4));
    const data = lzw(idx, min);
    for (let i = 0; i < data.length; i += 255) { const s = data.slice(i, i + 255); out.push(s.length); for (const b of s) out.push(b); }
    out.push(0);
  });
  out.push(0x3b);
  return Buffer.from(out);
}
// lzw: GIF-flavoured LZW, variable code size up to 12 bits, clear code when the table is full.
function lzw(idx, min) {
  const out = [], clr = 1 << min;
  let size = min + 1, next = clr + 2, dict = new Map(), cur = 0, nb = 0, prefix = idx[0];
  const emit = (code) => { cur |= code << nb; nb += size; while (nb >= 8) { out.push(cur & 255); cur >>>= 8; nb -= 8; } };
  emit(clr);
  for (let i = 1; i < idx.length; i++) {
    const k = idx[i], key = (prefix << 8) | k, code = dict.get(key);
    if (code !== undefined) { prefix = code; continue; }
    emit(prefix);
    if (next < 4096) { dict.set(key, next++); if (next > 1 << size && size < 12) size++; }
    else { emit(clr); dict = new Map(); size = min + 1; next = clr + 2; }
    prefix = k;
  }
  emit(prefix); emit(clr + 1);
  if (nb > 0) out.push(cur & 255);
  return out;
}

// ---------------------------------------------------------------- CLI
const HELP = `pxfx - pixel VFX tool. usage: node pxfx.mjs <command> <target> [flags]
  new <path.mjs> [--loop]   starter effect (one-shot simulated, or pure loop)
  render <fx.mjs|dir>       <out>/<name>/contact.png  (--gif anim.gif, --mp4 anim.mp4 with ffmpeg)
  sheet <fx.mjs|dir>        <out>/<name>.png + .json at 1x; a dir also writes index.json
  check <fx.mjs|dir>        PASS/WARN/FAIL lines, exit code 1 on FAIL
  act <fx.mjs|dir>          <out>/<name>/act.png at 12 fps with character and target (--gif, --sheet)
  compare <fx.mjs|name-*.mjs>...   <out>/compare.png, one row per effect (--frames n, default 8)
  lib <dir> list [words]    search effect pages <dir>/*.md
  lib <dir> get <id>... [--to fx] [--tool pxfx.mjs]   extract effects and the ones they import
flags: --out dir (pxfx-out)  --opts '<json>'  --zoom n (3; compare 2; full screen 1)  --all
  --frames 3-8,12  --columns n  --layer both|back|front  --actor (stock character under the effect)
  --bg dark|ground|sky|light|black|none
palettes: ${Object.keys(RAMPS).join(' ')}`;

function parseArgs(argv) {
  const a = { _: [] };
  for (let i = 0; i < argv.length; i++) {
    const s = argv[i];
    if (/^(--(all|loop|help|gif|mp4|sheet|actor)|-h)$/.test(s)) a[s.replace(/^-+/, '')] = true;
    else if (s.startsWith('--')) a[s.slice(2)] = argv[++i];
    else a._.push(s);
  }
  return a;
}
function stamp(img, W, x, y, str, col) {
  const m = textMask(str, 'small', 2);
  for (let j = 0; j < m.h; j++) for (let i = 0; i < m.w; i++) if (m.mask[j * m.w + i] && x + i < W) img.set([...col, 255], ((y + j) * W + x + i) * 4);
}
function board(cells, w, h, zoom, cols, titles = []) {
  const cw = w * zoom, ch = h * zoom, lab = 14, gap = 6, tH = titles.length ? 14 : 0, rows = Math.ceil(cells.length / cols);
  const W = Math.max(gap + cols * (cw + gap), 200), H = gap + rows * (ch + lab + gap + tH), img = new Uint8ClampedArray(W * H * 4);
  for (let i = 0; i < W * H; i++) img.set([10, 10, 14, 255], i * 4);
  cells.forEach((c, n) => {
    const col = n % cols, row = Math.floor(n / cols), cx = gap + col * (cw + gap), cy = gap + row * (ch + lab + gap + tH) + tH;
    if (col === 0 && titles[row]) stamp(img, W, gap, cy - tH + 1, titles[row], [225, 225, 235]);
    stamp(img, W, cx, cy + 1, c.label, c.col ?? [150, 150, 170]);
    for (let y = 0; y < ch; y++) for (let x = 0; x < cw; x++) {
      const s = (Math.floor(y / zoom) * w + Math.floor(x / zoom)) * 4, d = ((cy + lab + y) * W + cx + x) * 4, a = c.rgba[s + 3] / 255;
      const base = ((x >> 3) + (y >> 3)) & 1 ? 40 : 60;
      for (let k = 0; k < 3; k++) img[d + k] = c.rgba[s + k] * a + base * (1 - a);
      img[d + 3] = 255;
    }
  });
  return { W, H, img };
}
const isFull = ([w, h]) => w >= 320 && Math.abs(w / h - 16 / 9) < 0.25;
const SKY = [[8, 8, 24], [13, 15, 38], [20, 24, 56], [30, 36, 78]];
function backdrop(mode, [w, h], [ox, oy]) {
  if (mode === 'none') return null;
  if (mode === 'black') return [0, 0, 0];
  if (mode === 'sky') return (x, y) => (hash(x, y, 9) > 0.997 ? [150, 150, 190] : SKY[Math.min(3, Math.floor((y / h) * 3 + bayer(x, y)))]);
  if (mode === 'ground') {
    return (x, y) => {
      if (y < oy) return [22, 22, 32];
      const d = y - oy, row = d >> 2, bx = (x + (row & 1) * 6 + 600) % 12;
      if (d === 0) return [96, 92, 104];
      if ((d & 3) === 0 || bx === 0) return [28, 26, 34];
      const v = 66 - Math.min(26, d * 1.3) + hash(Math.floor((x + (row & 1) * 6 + 600) / 12), row, 3) * 12;
      return [v, v - 3, v + 6];
    };
  }
  const back = mode === 'light' ? [[200, 200, 208], [220, 220, 226], [176, 176, 186]] : [[22, 22, 30], [29, 29, 39], [44, 44, 58]], full = isFull([w, h]);
  return (x, y) => (!full && (y === oy || (x === ox && Math.abs(y - oy) <= 2)) ? back[2] : back[((x >> 2) + (y >> 2)) & 1]);
}
function zoomUp(src, w, h, z) {
  const out = new Uint8ClampedArray(w * z * h * z * 4);
  for (let y = 0; y < h * z; y++) for (let x = 0; x < w * z; x++) { const i = (((y / z) | 0) * w + ((x / z) | 0)) * 4; out.set(src.subarray(i, i + 4), (y * w * z + x) * 4); }
  return out;
}
function frameList(spec, n) {
  const out = [];
  for (const part of String(spec).split(',')) {
    const [lo, hi = lo] = part.split('-').map((v) => parseInt(v, 10));
    for (let i = lo; i <= hi; i++) if (i >= 0 && i < n) out.push(i);
  }
  if (!out.length) { console.error(`--frames ${spec}: no frames in 0..${n - 1}`); process.exit(2); }
  return out;
}
function hitFrameSet(timing, times) {
  const s = new Set();
  for (const hit of timing?.hits ?? []) {
    let best = 0;
    times.forEach((t, i) => { if (Math.abs(t - hit.t) < Math.abs(times[best] - hit.t)) best = i; });
    s.add(best);
  }
  return s;
}

function checkMeta(mod) {
  const m = mod.meta, bad = [];
  if (!m || typeof m !== 'object') return ['no exported meta'];
  if (typeof m.name !== 'string' || !/^[A-Za-z0-9][A-Za-z0-9_-]*$/.test(m.name)) bad.push('name must be a file-safe string');
  const [w, h] = Array.isArray(m.size) ? m.size : [];
  if (!Number.isInteger(w) || !Number.isInteger(h) || w < 8 || h < 8 || w > 512 || h > 512) bad.push('size must be [w, h], whole pixels 8..512');
  const [ox, oy] = Array.isArray(m.origin) ? m.origin : [];
  if (!Number.isInteger(ox) || !Number.isInteger(oy) || ox < 0 || oy < 0 || ox >= w || oy >= h) bad.push('origin must be whole pixels inside the canvas');
  if (!(m.duration > 0)) bad.push('duration must be > 0');
  if (m.fps !== undefined && !(m.fps > 0)) bad.push('fps must be > 0');
  if (m.loop !== undefined && typeof m.loop !== 'boolean') bad.push('loop must be true or false');
  const o = m.options || {};
  if (!('palette' in o) || !('seed' in o)) bad.push('options need at least palette and seed');
  const res = RESERVED.filter((k) => k in o);
  if (res.length) bad.push(`reserved option names in meta.options: ${res.join(', ')}`);
  if (typeof mod.draw !== 'function' && typeof mod.create !== 'function') bad.push('export draw(p, t, o) or create(o)');
  if (typeof mod.draw === 'function' && typeof mod.create === 'function') bad.push('export draw OR create, not both');
  try { if ('palette' in o) palette(o.palette); } catch (e) { bad.push(e.message); }
  return bad;
}

function checkTiming(t, duration) {
  const bad = [];
  if (!(t.windup >= 0)) bad.push('windup must be >= 0');
  if (!(t.recovery >= 0)) bad.push('recovery must be >= 0');
  const hits = t.hits ?? [];
  if (!Array.isArray(hits)) return ['hits must be an array'];
  hits.forEach((x, i) => {
    if (!(x.t >= 0 && x.t <= duration)) bad.push(`hit ${i}: t outside 0..duration`);
    if (!(x.freeze >= 0 && x.freeze <= 0.3)) bad.push(`hit ${i}: freeze outside 0..0.3`);
    if (!(x.shake >= 0) || !(x.amp >= 0)) bad.push(`hit ${i}: shake and amp must be >= 0`);
    if (i > 0 && x.t < hits[i - 1].t) bad.push(`hit ${i}: hits not sorted by t`);
  });
  return bad;
}

async function runCheck(mod, extra, loadAct) {
  const meta = mod.meta, lines = [];
  let fail = false;
  const line = (st, name, msg) => { lines.push(`${st.padEnd(4)} ${name}: ${msg}`); if (st === 'FAIL') fail = true; };
  const bad = checkMeta(mod);
  line(bad.length ? 'FAIL' : 'PASS', 'meta', bad.length ? bad.join('; ') : `${meta.name} ${meta.size.join('x')} ${meta.loop ? 'loop' : 'one-shot'} ${meta.duration}s @${meta.fps ?? 30}fps`);
  if (bad.length) return { lines, fail };
  const cues = mod.create?.cues;
  if (cues) {
    const cb = cues.flatMap((c, i) => (c.fx?.meta ? checkMeta(c.fx).map((m) => `cue ${i} (${c.fx.meta.name}): ${m}`) : [`cue ${i}: fx is not an effect module`]));
    line(cb.length ? 'FAIL' : 'PASS', 'move cues', cb.length ? cb.join('; ') : `${cues.length} cues, each effect's meta is valid`);
    if (cb.length) return { lines, fail };
  }
  const times = frameTimes(meta);
  const origRandom = Math.random, OrigDate = globalThis.Date;
  let runA, runB, err = null;
  const ms = [];
  Math.random = () => { throw new Error('Math.random() called; use rng(o.seed)'); };
  globalThis.Date = class { constructor() { throw new Error('Date used; effects must depend on t only'); } static now() { throw new Error('Date.now() used'); } };
  try {
    renderAll(mod, extra, times);
    runA = renderAll(mod, extra, times, { timed: ms });
    runB = renderAll(mod, extra, times);
    if (extra.actor) renderAll(mod, extra, times, { actor: true });
  } catch (e) { err = e; } finally { Math.random = origRandom; globalThis.Date = OrigDate; }
  if (err) { line('FAIL', 'runs', String(err.message || err)); return { lines, fail }; }
  line('PASS', 'runs', `${times.length} frames, no Math.random or Date`);
  const sum = (k) => runA.reduce((s, f) => s + f[k], 0), outside = sum('outside');
  const worst = runA.reduce((b, f, i) => (f.outside > runA[b].outside ? i : b), 0);
  line(outside ? 'FAIL' : 'PASS', 'inside canvas', outside ? `${outside} pixel writes outside the canvas (worst frame ${worst})` : 'no writes outside the canvas');
  const spill = sum('spill'), faint = sum('faint');
  line(spill ? 'FAIL' : faint ? 'WARN' : 'PASS', 'light spill', spill ? `${spill} bright light writes off the canvas (bigger canvas or smaller glow)` : faint ? `${faint} faint light writes off the canvas` : 'no light off the canvas');
  const rgba = runA.map((f) => composite(f));
  const lit = rgba.reduce((s, b) => { let n = 0; for (let i = 3; i < b.length; i += 4) if (b[i] > 8) n++; return s + n; }, 0);
  line(lit ? 'PASS' : 'FAIL', 'draws something', `${lit} visible pixels over all frames`);
  if (!meta.loop) {
    const f = runA[runA.length - 1];
    let col = 0, maxL = 0;
    for (let i = 3; i < f.color.length; i += 4) if (f.color[i] > 0) col++;
    for (const v of f.light) if (v > maxL) maxL = v;
    const ok = col === 0 && maxL < 6;
    line(ok ? 'PASS' : 'FAIL', 'ends empty', `last frame: ${col} colour pixels, max light ${maxL.toFixed(1)}${ok ? '' : ' (fade out sooner or make duration longer)'}`);
  } else {
    const [a, b] = renderAll(mod, extra, [0, meta.duration], { wrap: false }).map((f) => composite(f));
    let diff = 0;
    for (let i = 0; i < a.length; i += 4) {
      if (Math.abs(a[i] - b[i]) > 8 || Math.abs(a[i + 1] - b[i + 1]) > 8 || Math.abs(a[i + 2] - b[i + 2]) > 8 || Math.abs(a[i + 3] - b[i + 3]) > 8) diff++;
    }
    line(diff <= 4 ? 'PASS' : 'FAIL', 'loop seam', `t=0 vs t=duration differ in ${diff} pixels (max 4)`);
  }
  let same = true;
  for (let i = 0; i < runA.length && same; i++) {
    const x = rgba[i], y = composite(runB[i]);
    for (let k = 0; k < x.length; k++) if (x[k] !== y[k]) { same = false; break; }
  }
  line(same ? 'PASS' : 'FAIL', 'deterministic', same ? 'two runs with the same seed match' : 'two runs with the same seed differ (module state? time-based code?)');
  const sorted = [...ms].sort((p, q) => p - q), med = sorted[Math.floor(sorted.length / 2)];
  line(med > 4 ? 'FAIL' : med > 2 ? 'WARN' : 'PASS', 'frame time', `median ${med.toFixed(2)} ms per frame (warn > 2, fail > 4)`);
  if (mod.actorAlpha || mod.actorPaint || meta.actor) {
    const o = options(meta, extra), al = times.map((t) => (mod.actorAlpha ? mod.actorAlpha(Math.min(t, meta.duration), o) : 1));
    const okA = al.every((v) => v >= 0 && v <= 1);
    line(okA && extra.actor ? 'PASS' : 'FAIL', 'actor', !extra.actor ? 'actor features need o.actor (meta.actor: true)' : okA ? `actorAlpha ${Math.min(...al).toFixed(2)}..${Math.max(...al).toFixed(2)}${mod.actorPaint ? ', actorPaint runs' : ''}` : 'actorAlpha must return 0..1');
  }
  const tm = timingOf(mod), tb = tm && checkTiming(tm, meta.duration);
  if (tb) line(tb.length ? 'FAIL' : 'PASS', 'timing', tb.length ? tb.join('; ') : `windup ${tm.windup}s, ${(tm.hits ?? []).length} hit(s), recovery ${tm.recovery}s${meta.timing ? '' : ' (from act)'}`);
  else line('INFO', 'timing', 'no meta.timing or act (add one to anything that hits)');
  if (mod.act) for (const [st, name, msg] of (await loadAct()).checkAct(mod)) line(st, name, msg);
  return { lines, fail };
}

const STARTER_ONESHOT = (name, lib) => `// ${name}: starter one-shot (simulated). Brief: <what, how it moves, palette, timing>.
import { rng, particles, burst, ease, clamp, shade } from '${lib}';

const RING = 0.28; // the ring expands for this long; the first frame is a white flash

export const meta = {
  name: '${name}', size: [80, 56], origin: [40, 44], duration: 1.2, loop: false, fps: 30,
  options: { palette: 'fire', seed: 1 }, tags: ['impact'],
  timing: { windup: 0.15, hits: [{ t: 0, freeze: 0.05, shake: 0.12, amp: 2 }], recovery: 0.25 },
};

export function create(o) {
  const r = rng(o.seed), ps = particles(r);
  for (const [vx, vy] of burst(12, r, 50, 110, -2.8, -0.3)) ps.spawn(0, -10, vx, vy, r.range(0.3, 0.6), { kind: 'spark', ramp: o.palette });
  for (let i = 0; i < 3; i++) ps.spawn(r.range(-4, 4), -10, r.range(-12, 12), r.range(-14, -6), r.range(0.6, 0.9), { kind: 'smoke', size: r.range(2, 3), delay: 0.05 + i * 0.04 });
  let t = 0;
  return {
    update(dt) { t += dt; ps.update(dt); },
    draw(p) {
      const u = clamp(t / RING);
      if (u < 1) {
        p.ring(0, -10, 2 + 12 * ease.outQuart(u), u < 0.5 ? 2 : 1, shade(o.palette, 1 - u), 1 - u);
        p.glow(0, -10, 18, shade(o.palette, 0.8), 0.7 * (1 - u) ** 2);
      }
      if (t < 1 / 30) p.disc(0, -10, 4, '#ffffff');
      ps.draw(p);
    },
  };
}
`;
const STARTER_LOOP = (name, lib) => `// ${name}: starter loop (pure). Brief: <what, how it moves, palette, timing>.
import { cycle, shade, TAU } from '${lib}';

export const meta = {
  name: '${name}', size: [48, 48], origin: [24, 24], duration: 1, loop: true, fps: 24,
  options: { palette: 'arcane', seed: 1 }, tags: ['aura'],
};

export function draw(p, t, o) {
  const pulse = 0.5 + 0.5 * Math.sin((t / meta.duration) * TAU);
  p.glow(0, 0, 16, shade(o.palette, 0.7), 0.25 + 0.15 * pulse);
  p.disc(0, 0, 3 + pulse, shade(o.palette, 0.5));
  p.disc(0, -1, 1.5, shade(o.palette, 1));
  cycle(t, meta.duration, 6, (q, i, r) => {
    const a = q * TAU + r.range(0, 0.4), rad = 9 + 2 * Math.sin(q * TAU * 2 + i), x = Math.cos(a) * rad, y = Math.sin(a) * rad * 0.6;
    p.px(x, y, shade(o.palette, 0.9));
    p.lit(x, y, shade(o.palette, 1), 0.6);
  }, { seed: o.seed });
}
`;

// lib: effect pages are <dir>/*.md; each effect is '## <category>/<name>', one line of
// description, then a ```js block whose first line is '// id: <category>/<name>'.
function libRead(fs, path, dir) {
  const all = [];
  for (const n of fs.readdirSync(dir).filter((f) => f.endsWith('.md')).sort()) {
    const txt = fs.readFileSync(path.join(dir, n), 'utf8');
    for (const m of txt.matchAll(/^## ([\w-]+)\/([\w-]+)[ \t]*\n\s*(.*)\n\s*```js\n(\/\/ id: (\S+)[\s\S]*?)\n```/gm)) {
      const id = `${m[1]}/${m[2]}`, code = m[4], g = (re) => code.match(re)?.[1];
      if (m[5] !== id) { console.error(`warning: ${n}: heading ${id} has '// id: ${m[5]}'`); continue; }
      all.push({ id, file: `${m[1]}-${m[2]}`, desc: m[3].trim(), code, tags: g(/tags:\s*\[([^\]]*)\]/) ?? '',
        kind: (/loop:\s*true/.test(code) ? 'loop' : 'once') + (/export const act\b/.test(code) ? '+act' : '') + (/actor:\s*true/.test(code) ? '+body' : '') + (/chain\(/.test(code) ? '+move' : ''),
        dur: (g(/duration:\s*([\d.]+)/) ?? '?') + 's', size: (g(/size:\s*\[([^\]]*)\]/) ?? '?').replace(/\s/g, '').replace(',', 'x') });
    }
  }
  return all;
}

async function cli(argv) {
  const fs = await import('node:fs'), path = await import('node:path');
  const { pathToFileURL, fileURLToPath } = await import('node:url'), { deflateSync } = await import('node:zlib');
  const a = parseArgs(argv), [cmd, file] = a._, me = fileURLToPath(import.meta.url);
  const die = (msg) => { console.error(msg); process.exit(2); };
  const save = (f, buf) => { fs.mkdirSync(path.dirname(f), { recursive: true }); fs.writeFileSync(f, buf); console.log(`wrote ${f}`); };
  if (!cmd || a.help || !file) { console.log(HELP); process.exit(cmd && !a.help ? 2 : 0); }
  if (cmd === 'new') {
    if (fs.existsSync(file)) die(`${file} exists; not overwriting`);
    const name = path.basename(file).replace(/\.m?js$/, '');
    let lib = path.relative(path.dirname(path.resolve(file)), me).split(path.sep).join('/');
    if (!lib.startsWith('.')) lib = './' + lib;
    save(file, (a.loop ? STARTER_LOOP : STARTER_ONESHOT)(name, lib));
    return;
  }
  if (cmd === 'lib') {
    const [, dir, sub = 'list', ...rest] = a._, all = libRead(fs, path, dir);
    if (sub === 'list') {
      const words = rest.map((w) => w.toLowerCase()), hit = all.filter((e) => words.every((w) => `${e.id} ${e.tags} ${e.desc}`.toLowerCase().includes(w)));
      for (const e of hit) console.log(`${e.id.padEnd(28)} ${e.kind.padEnd(15)} ${e.dur.padEnd(6)} ${e.size.padEnd(8)} ${e.desc}`);
      console.log(`${hit.length} of ${all.length} effects`);
      return;
    }
    if (sub !== 'get' || !rest.length) die('usage: lib <dir> list [words] | lib <dir> get <id>... [--to dir] [--tool path/pxfx.mjs]');
    const to = a.to ?? 'fx', tool = path.resolve(a.tool ?? me), want = [...rest], done = new Set();
    const rel = (f) => { const r = path.relative(path.resolve(to), f).split(path.sep).join('/'); return r.startsWith('.') ? r : './' + r; };
    while (want.length) {
      const id = want.shift(), e = all.find((x) => x.id === id);
      if (done.has(id)) continue;
      if (!e) die(`lib: no effect '${id}' in ${dir} (try: lib ${dir} list)`);
      done.add(id);
      const code = e.code.replace(/(['"])\.\/(pxfx|pxact)\.mjs\1/g, (_, q, n) => `'${rel(path.join(path.dirname(tool), n + '.mjs'))}'`);
      for (const m of code.matchAll(/from\s+['"]\.\/([\w-]+)\.mjs['"]/g)) {
        const d = all.find((x) => x.file === m[1]);
        if (d) want.push(d.id); else console.error(`warning: ${id} imports ./${m[1]}.mjs, which is not in the library`);
      }
      save(path.join(to, e.file + '.mjs'), code + '\n');
    }
    return;
  }
  let extra = {};
  if (a.opts) { try { extra = JSON.parse(a.opts); } catch (e) { die(`--opts is not JSON: ${e.message}`); } }
  const outDir = a.out ?? 'pxfx-out', layer = a.layer ?? 'both', bgm = a.bg ?? 'dark', suffix = layer === 'both' ? '' : `-${layer}`;
  if (!['both', 'back', 'front'].includes(layer)) die('--layer must be both, back or front');
  if (!['dark', 'ground', 'sky', 'light', 'black', 'none'].includes(bgm)) die('--bg must be dark, ground, sky, light, black or none');
  let A = null;
  const loadAct = async () => (A ??= await import(new URL('./pxact.mjs', import.meta.url).href)
    .catch((e) => die(`cannot load pxact.mjs next to pxfx.mjs (copy it from references/script-pxact.md): ${e.message}`)));
  const load = (f) => import(pathToFileURL(path.resolve(f)).href);
  const extraFor = async (mod) => (mod.meta?.actor || a.actor ? { actor: (await loadAct()).rig(), ...extra } : extra);
  const cols0 = a.columns ? Math.max(1, parseInt(a.columns, 10)) : 0;
  const writeSheet = (name, rgbas, w, h, fps, vfx) => {
    const n = rgbas.length, c = cols0 || Math.min(n, Math.max(1, Math.floor(2048 / w))), W = c * w, H = Math.ceil(n / c) * h;
    const img = new Uint8ClampedArray(W * H * 4), json = { frames: [], meta: {} };
    rgbas.forEach((rgba, i) => {
      const x = (i % c) * w, y = Math.floor(i / c) * h;
      for (let yy = 0; yy < h; yy++) img.set(rgba.subarray(yy * w * 4, (yy + 1) * w * 4), ((y + yy) * W + x) * 4);
      json.frames.push({ filename: `${name}_${String(i).padStart(3, '0')}`, frame: { x, y, w, h }, rotated: false, trimmed: false,
        spriteSourceSize: { x: 0, y: 0, w, h }, sourceSize: { w, h }, duration: Math.round(1000 / fps) });
    });
    json.meta = { app: 'pxfx', version: '2', image: `${name}.png`, format: 'RGBA8888', size: { w: W, h: H }, scale: '1',
      frameTags: [{ name, from: 0, to: n - 1, direction: 'forward' }], vfx: { fps, frames: n, frameSize: { w, h }, columns: c, ...vfx } };
    save(path.join(outDir, `${name}.png`), pngBytes(W, H, img, deflateSync));
    save(path.join(outDir, `${name}.json`), JSON.stringify(json, null, 1) + '\n');
    return json.meta.vfx;
  };
  const anim = async (dir, base, rgbas, w, h, zoom, fps) => {
    const fr = rgbas.map((r) => zoomUp(r, w, h, zoom)), W = w * zoom, H = h * zoom;
    if (a.gif) save(path.join(dir, `${base}.gif`), gifBytes(W, H, fr, fps));
    if (!a.mp4) return;
    const { spawnSync } = await import('node:child_process'), f = path.join(dir, `${base}.mp4`);
    const r = spawnSync('ffmpeg', ['-y', '-loglevel', 'error', '-f', 'rawvideo', '-pix_fmt', 'rgba', '-s', `${W}x${H}`, '-r', String(fps), '-i', '-',
      '-vf', 'pad=ceil(iw/2)*2:ceil(ih/2)*2', '-pix_fmt', 'yuv420p', f], { input: Buffer.concat(fr.map((x) => Buffer.from(x.buffer, x.byteOffset, x.length))), maxBuffer: 1 << 28 });
    console.log(r.error || r.status ? 'note: no working ffmpeg on PATH; skipped the mp4' : `wrote ${f}`);
  };
  if (cmd === 'compare') {
    const expand = (pat) => {
      if (!pat.includes('*')) return [pat];
      const dir = path.dirname(pat), b = path.basename(pat).replace(/\.mjs$/, '');
      const re = new RegExp('^' + b.replace(/[.+?^${}()|[\]\\]/g, '\\$&').replace(/\*/g, '.*') + '\\.mjs$');
      return fs.readdirSync(dir).filter((n) => re.test(n)).sort().map((n) => path.join(dir, n));
    };
    const mods = [];
    for (const f of a._.slice(1).flatMap(expand)) {
      const m = await load(f);
      if (m.meta && !checkMeta(m).length) mods.push([f, m]); else console.error(`skipped ${f}: no valid meta`);
    }
    if (!mods.length) die('compare: no effects matched');
    const n = Math.max(1, parseInt(a.frames ?? '8', 10)), mx = (fn) => Math.max(...mods.map(([, m]) => fn(m.meta)));
    const L = mx((m) => m.origin[0]), T = mx((m) => m.origin[1]), w = L + mx((m) => m.size[0] - m.origin[0]), h = T + mx((m) => m.size[1] - m.origin[1]);
    const cells = [], titles = [], bg = backdrop(bgm, [w, h], [L, T]);
    for (const [f, m] of mods) {
      const ex = await extraFor(m), times = Array.from({ length: n }, (_, i) => (i * m.meta.duration) / n);
      renderAll(m, ex, times, { actor: !!ex.actor, box: [w, h, L, T] }).forEach((q, i) => cells.push({ rgba: composite(q, bg), label: times[i].toFixed(2) }));
      titles.push(path.basename(f, '.mjs'));
    }
    const { W, H, img } = board(cells, w, h, Math.max(1, parseInt(a.zoom ?? '2', 10)), n, titles);
    save(path.join(outDir, 'compare.png'), pngBytes(W, H, img, deflateSync));
    return;
  }
  if (!['render', 'sheet', 'check', 'act'].includes(cmd)) die(`unknown command: ${cmd}\n${HELP}`);
  const isDir = fs.statSync(file).isDirectory(), index = [];
  const files = isDir ? fs.readdirSync(file).filter((n) => n.endsWith('.mjs') && !/^px(fx|act)\.mjs$/.test(n)).sort().map((n) => path.join(file, n)) : [file];
  let failed = false;
  for (const f of files) {
    const mod = await load(f);
    if (!mod.meta) { if (isDir) continue; die(`${f}: no exported meta`); }
    if (isDir) console.log(`== ${f}`);
    const ex = await extraFor(mod), meta = mod.meta;
    if (cmd === 'check') {
      const { lines, fail } = await runCheck(mod, ex, loadAct);
      console.log(lines.join('\n') + `\nRESULT: ${fail ? 'FAIL' : 'PASS'}`);
      failed ||= fail;
      continue;
    }
    const bad = checkMeta(mod);
    if (bad.length) { console.error(`meta: ${bad.join('; ')}`); failed = true; continue; }
    const [w, h] = meta.size, fps = meta.fps ?? 30, dir = path.join(outDir, meta.name), timing = timingOf(mod);
    const zoom = Math.max(1, parseInt(a.zoom ?? (isFull(meta.size) ? '1' : '3'), 10));
    if (cmd === 'act') {
      if (!mod.act) { if (!isDir) { console.error(`${f}: no exported act`); failed = true; } continue; }
      const S = (await loadAct()).actFrames(mod, ex), fr = S.frames;
      const rgbas = fr.map((q) => composite(q.f, backdrop(bgm, [S.w, S.h], [q.ox, q.oy])));
      const cells = fr.map((q, i) => ({ rgba: rgbas[i], label: (q.t < 0 ? '-' : '') + Math.abs(q.t).toFixed(2) + (q.frozen ? ' F' : ''),
        col: q.hot ? [255, 200, 60] : q.frozen ? [110, 210, 255] : undefined }));
      const { W, H, img } = board(cells, S.w, S.h, zoom, cols0 || Math.max(1, Math.min(fr.length, Math.floor(1400 / (S.w * zoom + 6)))));
      save(path.join(dir, 'act.png'), pngBytes(W, H, img, deflateSync));
      console.log(`  ${fr.length} steps at 12 fps, t ${fr[0].t.toFixed(2)}..${fr.at(-1).t.toFixed(2)}, ${fr.filter((q) => q.frozen).length} frozen, stage ${S.w}x${S.h}`);
      await anim(dir, 'act', rgbas, S.w, S.h, zoom, 12);
      if (a.sheet) writeSheet(`${meta.name}-act`, fr.map((q) => composite(q.f)), S.w, S.h, 12, { loop: false, pivot: S.pivot, timing, act: mod.act });
      continue;
    }
    const times = frameTimes(meta), ms = [];
    const frames = renderAll(mod, ex, times, { layer, timed: ms, actor: !!ex.actor && layer === 'both' && (cmd === 'render' || a.actor) });
    if (cmd === 'render') {
      const n = times.length, cap = 24, hits = hitFrameSet(timing, times), rgbas = frames.map((q) => composite(q, backdrop(bgm, meta.size, meta.origin)));
      let idx = a.all || n <= cap ? times.map((_, i) => i) : [...new Set(Array.from({ length: cap }, (_, i) => Math.round((i * (n - 1)) / (cap - 1))))];
      if (a.frames) idx = frameList(a.frames, n);
      const cols = cols0 || Math.max(1, Math.min(idx.length, Math.floor(1200 / (w * zoom + 6)) || 1));
      const { W, H, img } = board(idx.map((i) => ({ rgba: rgbas[i], label: String(i), col: hits.has(i) ? [255, 200, 60] : undefined })), w, h, zoom, cols);
      save(path.join(dir, `contact${suffix}.png`), pngBytes(W, H, img, deflateSync));
      const med = [...ms].sort((p, q) => p - q)[Math.floor(ms.length / 2)];
      console.log(`  ${idx.length} of ${n} frames, median ${med.toFixed(2)} ms/frame`);
      if (a.gif || a.mp4) await anim(dir, `anim${suffix}`, bgm === 'none' ? frames.map((q) => composite(q, backdrop('dark', meta.size, meta.origin))) : rgbas, w, h, zoom, fps);
      continue;
    }
    const o = options(meta, ex), name = meta.name + suffix, pivot = { x: meta.origin[0] + (o.x ?? 0), y: meta.origin[1] + (o.y ?? 0) };
    const tm = timing && { ...timing, hits: (timing.hits ?? []).map((x) => ({ ...x, frame: [...hitFrameSet({ hits: [x] }, times)][0] })) };
    const vfx = writeSheet(name, frames.map((q) => composite(q)), w, h, fps, { loop: !!meta.loop, pivot, layer, ...(tm ? { timing: tm } : {}),
      ...(mod.act ? { act: mod.act } : {}), ...(mod.actorAlpha ? { actorAlpha: times.map((t) => +clamp(mod.actorAlpha(t, o)).toFixed(3)) } : {}) });
    index.push({ name, file: `${name}.png`, json: `${name}.json`, fps, loop: vfx.loop, frames: vfx.frames, frameSize: vfx.frameSize, pivot, timing: tm ?? null });
  }
  if (cmd === 'sheet' && isDir) save(path.join(outDir, 'index.json'), JSON.stringify(index, null, 1) + '\n');
  if (failed) process.exit(1);
}

if (typeof process !== 'undefined' && process.argv?.[1]) {
  const { pathToFileURL } = await import('node:url'), { realpathSync } = await import('node:fs');
  let me = '';
  try { me = pathToFileURL(realpathSync(process.argv[1])).href; } catch { /* not a file */ }
  // Not awaited: the effect module imports this file, which must finish loading first.
  if (me === import.meta.url) cli(process.argv.slice(2)).catch((e) => { console.error(e); process.exit(1); });
}
```
