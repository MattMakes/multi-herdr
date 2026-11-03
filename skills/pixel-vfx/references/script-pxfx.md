# Script: pxfx.mjs

Usage: `node pxfx.mjs <new|render|sheet|check> <effect.mjs> [--out dir] [--opts '<json>'] [--zoom n] [--all] [--frames 3-8] [--bg dark|light|none] [--columns n] [--layer both|back|front]`

`pxfx.mjs` is the effect library and the command-line tool in one file. Effect modules import
the library from it. The command line renders contact sheets to look at, writes sprite sheets
with Aseprite-style JSON for any engine, writes starter effects, and checks the effect
contract with PASS, WARN, INFO and FAIL lines (exit code 1 on any FAIL). It needs Node 18 or
newer and uses only the standard library (`node:zlib`, `node:fs`, `node:path`, `node:url`).
The library part does not touch Node at load time, so a browser game can import it as is.
Copy the block into a file and run it, for example `pxfx.mjs` next to your effects (a starter
from `new` computes its import path; hand-written effects import `./pxfx.mjs`, so fix the
path if the tool lives elsewhere).

## Commands

| Command | Writes | Notes |
| --- | --- | --- |
| `new <path.mjs> [--loop]` | a starter effect | A one-shot simulated impact, or with `--loop` a pure loop. Both pass `check`. Never overwrites. |
| `render <fx.mjs>` | `<out>/<name>/contact.png` | Frames in a grid over a checker backdrop, scaled by `--zoom` (default 3), each labelled with its index. Hit frames have yellow labels. A faint line marks the origin row. At most 24 evenly spaced frames unless `--all` or `--frames`. |
| `sheet <fx.mjs>` | `<out>/<name>.png`, `<out>/<name>.json` | Transparent frames at 1x in a grid. `--columns` sets frames per row (default: as many as fit in 2048 px). |
| `check <fx.mjs>` | nothing | meta, runs (no `Math.random`, no `Date`), inside canvas, draws something, ends empty (one-shot) or loop seam (loop), deterministic, frame time, timing. |

`--out` defaults to `pxfx-out` in the current folder. `--opts` merges options over
`meta.options`, for example `--opts '{"palette":"ice","seed":4,"flip":true}'`. `--layer back`
or `front` draws one layer only and adds `-back` or `-front` to the output name.

Frame count: `n = round(duration * fps)`. A loop renders frames at `t = i * duration / n` for
`i < n`. A one-shot adds frame `n` at `t = duration`, which must be empty.

## Library

Colours are `'#rrggbb'` or `'#rgb'` strings, `0xRRGGBB` numbers, or `[r, g, b]` arrays,
everywhere. Painter alpha `a` (default 1) below 1 is drawn with the 4x4 ordered dither;
`puff` alpha really blends. Angles in `arc` are radians, clockwise on screen, 0 = +x. The
placement `angle` (an option) is in degrees.

| Export | What it does |
| --- | --- |
| `frame(w, h)`, `clear(f)` | A frame: `color` (RGBA bytes), `light` (RGB floats), `outside` (count of writes off the canvas). |
| `composite(f, bg)` | RGBA bytes. `bg = null` keeps transparency and bakes light into colour and alpha; `bg` may be a colour or `(x, y) => [r, g, b]`. |
| `painter(f, { x, y, flip, angle, light })` | Drawing in local pixels. `p.px`, `p.line`, `p.disc`, `p.ring(x, y, r, width)`, `p.arc(x, y, r, a0, a1, width)` (width may be a function of u for tapered bands), `p.oval(x, y, rx, ry, c, a, { width, half })` (`half: 'back'` or `'front'`), `p.rect(x, y, w, h)`, `p.puff(x, y, r, c, a)` (blended), `p.glow(x, y, r, c, s, squash)` (additive, radial falloff), `p.lit(x, y, c, s)` (additive pixel). Also `p.screen(x, y)`, `p.local(X, Y)`, `p.w`, `p.h`. |
| `rng(seed)` | Seeded random: `next()`, `range(a, b)`, `int(a, b)` (inclusive), `pick(list)`, `chance(p)`, `sign()`. |
| `hash(a, b, c)` | Stable number in [0, 1) from integers. |
| `noise(x, y, seed, px, py)`, `fbm(...)` | Value noise; `px`/`py` make it repeat, for seamless loops. |
| `ease.*`, `clamp`, `lerp`, `smoothstep`, `fract`, `TAU` | `linear`, `inQuad`, `outQuad`, `inOutQuad`, `inCubic`, `outCubic`, `outQuart`, `outExpo`, `inOutSine`, `outBack`. Inputs are clamped to 0..1. |
| `RAMPS`, `palette(p)`, `ramp(p, n, mix)`, `shade(p, u)`, `rgb(c)`, `toHex(c)` | Built-in ramps: fire, ice, poison, holy, shadow, electric, smoke, gold, blood, water, arcane. `ramp` resamples to n steps (palette colours only, unless `mix`). `shade(p, u)` picks the hard step for u in 0..1. |
| `particles(r)` | `spawn(x, y, vx, vy, life, opts)`, `update(dt)`, `draw(p)`, `list`, `alive`. Kinds: `spark` (speed-stretched streak, gravity, bounces on `floor`, glows), `ember` (rises, wanders, cools), `smoke` (blended puff, rises, swells, thins), `debris` (falls, tumbles, bounces, settles, fades). Opts: `kind`, `size`, `ramp`, `gravity`, `drag`, `bounce`, `floor` (default 0; `null` for none), `delay`, `glow`, `wander`, `grow`, `alpha`, `spin`, `shade`. |
| `cycle(t, duration, n, fn, { seed, speed })` | n looping particles for pure effects: `fn(phase, i, r)` with `phase = (t / duration * speed + i / n) mod 1`; `r` is the same rng for particle i on every frame. `speed` must be whole. |
| `options(meta, extra)`, `instance(mod, extra)`, `frameTimes(meta)`, `STEP` | Run an effect: `inst.seek(t)`, `inst.advance(dt)`, `inst.draw(p, layer)`, `inst.paint(f, x, y)`, `inst.done`. |
| `bayer(x, y)` | The dither threshold of a screen pixel, for custom wipes. |
| `pngBytes(w, h, rgba, deflateSync)` | An RGBA8 PNG file as a Buffer (Node only). |

```js
// pxfx.mjs - pixel-art VFX library and command-line tool. One file, no dependencies.
// Library: import { frame, painter, particles, rng, ease, shade } from './pxfx.mjs'
// CLI:     node pxfx.mjs <new|render|sheet|check> <effect.mjs> [flags]   (Node 18+)
// Colours: '#rrggbb' or '#rgb' strings, 0xRRGGBB numbers, or [r, g, b] arrays.
// Alpha below 1 is drawn with 4x4 ordered dithering, except puff() which really blends.

// ---------------------------------------------------------------- math
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

// ---------------------------------------------------------------- seeded random
function mix32(h) {
  h ^= h >>> 16; h = Math.imul(h, 0x7feb352d);
  h ^= h >>> 15; h = Math.imul(h, 0x846ca68b);
  return (h ^ (h >>> 16)) >>> 0;
}
// hash(a, b, c): integers in, a stable number in [0, 1) out.
export const hash = (a, b = 0, c = 0) =>
  mix32(Math.imul(a | 0, 0x27d4eb2d) ^ mix32(Math.imul(b | 0, 0x165667b1) ^ mix32((c | 0) + 0x9e3779b9))) / 4294967296;
function seedOf(seed) {
  if (typeof seed === 'string') { let h = 2166136261; for (const ch of seed) h = Math.imul(h ^ ch.charCodeAt(0), 16777619); return h >>> 0; }
  return mix32(Math.floor(Number(seed) || 0) + 1);
}
// rng(seed): mulberry32. next() in [0,1), range(a,b), int(a,b) inclusive, pick(list), chance(p), sign().
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

// ---------------------------------------------------------------- noise (can loop)
// noise(x, y, seed, px, py): value noise in [0,1]. px/py > 0 make it repeat every px/py units.
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
};
const colCache = new Map();
// rgb(c): any colour form -> [r, g, b].
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
// palette(p): a RAMPS name or an array -> array of colours, dark to bright.
export function palette(p) {
  if (Array.isArray(p) && p.length) return p;
  if (typeof p === 'string' && RAMPS[p]) return RAMPS[p];
  throw new Error(`bad palette: ${JSON.stringify(p)} (use an array of colours or one of ${Object.keys(RAMPS).join(', ')})`);
}
// ramp(p, n, mix): resample to n steps. mix=false keeps only palette colours; mix=true blends.
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
// shade(p, u): the palette step for u in 0..1 (0 = darkest, 1 = brightest). Hard steps, no blend.
export function shade(p, u) {
  p = palette(p);
  return p[Math.min(p.length - 1, Math.floor(clamp(u) * p.length))];
}

// ---------------------------------------------------------------- frame and dithering
// A frame has two buffers: color (RGBA bytes) and light (additive RGB floats, 0..255).
export function frame(w, h) {
  return { w, h, color: new Uint8ClampedArray(w * h * 4), light: new Float32Array(w * h * 3), outside: 0 };
}
export function clear(f) { f.color.fill(0); f.light.fill(0); f.outside = 0; return f; }
const BAYER = [0, 8, 2, 10, 12, 4, 14, 6, 3, 11, 1, 9, 15, 7, 13, 5];
// bayer(x, y): the ordered-dither threshold of a screen pixel, in (0, 1).
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
    if (X < 0 || Y < 0 || X >= w || Y >= h) { if (Math.max(r, g, b) >= 4) f.outside++; return; }
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
    // puff: a translucent disc that really blends (smoke, vapour, dust, mist). No dither.
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
    // lit: additive light on one pixel (sparks, glints, hot cores).
    lit(x, y, c, s = 1) { const [X, Y] = screen(x, y); add(R(X), R(Y), rgb(c), s * L); },
  };
  return p;
}

// ---------------------------------------------------------------- particles
const KINDS = {
  spark: { gravity: 260, drag: 1.2, bounce: 0.45, ramp: 'fire', size: 1, glow: 1, stretch: 0.03 },
  ember: { gravity: -35, drag: 1.5, bounce: 0, ramp: 'fire', size: 1, glow: 0.6, wander: 30 },
  smoke: { gravity: -14, drag: 2.5, bounce: 0, ramp: 'smoke', size: 3, grow: 2.2, alpha: 0.55 },
  debris: { gravity: 380, drag: 0.3, bounce: 0.35, ramp: 'smoke', size: 2, spin: 14 },
};
// particles(r): r is an rng. spawn(x, y, vx, vy, life, opts) adds one particle (px and px/s).
// opts: kind (spark|ember|smoke|debris), size, ramp (palette, cools bright -> dark over life),
// gravity, drag (per second), bounce (0..1), floor (y it bounces on, default 0; null = none),
// delay (s before it starts), glow, wander, grow, alpha, spin, shade (fixed 0..1 for debris).
export function particles(r) {
  const list = [];
  const sys = {
    list,
    get alive() { return list.length; },
    spawn(x, y, vx, vy, life, opts = {}) {
      const kind = opts.kind ?? 'spark';
      if (!KINDS[kind]) throw new Error(`unknown particle kind: ${kind}`);
      const q = { kind, x, y, vx, vy, life, age: 0, floor: 0, delay: 0, ...KINDS[kind],
        phase: r.next() * TAU, rot: r.next() * TAU, jitter: r.next(), ...opts };
      q.ramp = palette(q.ramp);
      list.push(q);
      return q;
    },
    update(dt) {
      let n = 0;
      for (const q of list) {
        if (q.delay > 0) { q.delay -= dt; list[n++] = q; continue; }
        q.age += dt;
        if (q.age >= q.life) continue;
        const k = Math.exp(-q.drag * dt);
        q.vx *= k; q.vy *= k;
        q.vy += q.gravity * dt;
        if (q.wander) q.vx += Math.sin(q.age * 7 + q.phase) * q.wander * dt;
        q.x += q.vx * dt; q.y += q.vy * dt;
        if (q.floor != null && q.y > q.floor && q.vy > 0) {
          q.y = q.floor;
          q.vy = -q.vy * q.bounce; q.vx *= 0.6;
          if (Math.abs(q.vy) < 12) q.vy = 0;
          q.grounded = true;
        }
        if (q.spin) q.rot += q.spin * dt * Math.sign(q.vx || 1) * Math.min(1, Math.hypot(q.vx, q.vy) / 25);
        list[n++] = q;
      }
      list.length = n;
    },
    draw(p) { for (const q of list) if (q.delay <= 0) drawParticle(p, q); },
  };
  return sys;
}
function drawParticle(p, q) {
  const u = clamp(q.age / q.life), fade = u < 0.7 ? 1 : 1 - (u - 0.7) / 0.3;
  if (q.kind === 'spark') {
    const sp = Math.hypot(q.vx, q.vy), len = Math.min(6 * q.size, sp * q.stretch);
    const hot = shade(q.ramp, 1 - u * 0.8), tail = shade(q.ramp, 0.75 - u * 0.75);
    if (len >= 1) p.line(q.x - (q.vx / sp) * len, q.y - (q.vy / sp) * len, q.x, q.y, tail, fade);
    p.px(q.x, q.y, hot, fade);
    p.lit(q.x, q.y, hot, 0.5 * q.glow * fade);
    if (q.glow > 0) p.glow(q.x, q.y, 3, hot, 0.12 * q.glow * fade);
  } else if (q.kind === 'ember') {
    const c = shade(q.ramp, 1 - u);
    p.px(q.x, q.y, c, fade);
    if (q.size > 1 && u < 0.5) p.px(q.x, q.y + 1, shade(q.ramp, 0.6 - u), fade);
    p.lit(q.x, q.y, c, q.glow * (1 - u));
  } else if (q.kind === 'smoke') {
    const rad = q.size * (1 + (q.grow - 1) * ease.outQuad(u));
    const a = q.alpha * (1 - u) ** 1.3 * Math.min(1, q.age / 0.08);
    p.puff(q.x, q.y, rad, shade(q.ramp, 0.25 + 0.2 * q.jitter), a);
    p.puff(q.x - rad * 0.2, q.y - rad * 0.35, rad * 0.6, shade(q.ramp, 0.75), a * 0.7);
  } else {
    // debris: 1 px speck, 2 px stick that turns, or a 2x2 chunk whose lit corner turns
    const c = shade(q.ramp, q.shade ?? 0.3 + 0.35 * q.jitter), hi = shade(q.ramp, 1), lo = shade(q.ramp, 0);
    const x = Math.round(q.x), y = Math.round(q.y);
    if (q.size < 2) return p.px(x, y, c, fade);
    if (q.size < 3) {
      const dx = Math.round(Math.cos(q.rot)), dy = Math.round(Math.sin(q.rot));
      p.px(x, y, hi, fade); p.px(x + dx, y + dy, c, fade);
      return;
    }
    const k = ((Math.floor((q.rot / TAU) * 4) % 4) + 4) % 4, cells = [[0, -1], [1, -1], [1, 0], [0, 0]];
    cells.forEach(([cx, cy], j) => p.px(x + cx, y + cy, j === k ? hi : j === (k + 2) % 4 ? lo : c, fade));
  }
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
const RESERVED = ['x', 'y', 'flip', 'angle', 'light'];
// options(meta, extra): meta.options merged with the caller's options; palette names resolved.
export function options(meta, extra = {}) {
  const o = { ...(meta.options || {}), ...extra };
  if (o.palette !== undefined) o.palette = palette(o.palette);
  return o;
}
export const STEP = 1 / 120;
// instance(mod, extra): one running copy of an effect module, for games and for the CLI.
// seek(t) moves forward to time t (simulated effects step at a fixed 1/120 s);
// draw(p, layer) draws 'back', 'front' or 'both'. paint(f, x, y) makes a painter for it.
// Loops wrap t into 0..duration; wrap: false passes t through (the seam check uses that).
export function instance(mod, extra = {}, { wrap = true } = {}) {
  const meta = mod.meta, o = options(meta, extra), sim = mod.create ? mod.create(o) : null;
  let t = 0, steps = 0;
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
      const tt = !meta.loop ? Math.min(t, meta.duration) : wrap ? fract(t / meta.duration) * meta.duration : t;
      if (layer !== 'front') { if (sim) sim.drawBack?.(p); else mod.drawBack?.(p, tt, o); }
      if (layer !== 'back') { if (sim) sim.draw(p); else mod.draw(p, tt, o); }
    },
    paint(f, x, y) { return painter(f, { x: x + (o.x ?? 0), y: y + (o.y ?? 0), flip: o.flip, angle: o.angle, light: o.light }); },
  };
  return inst;
}
// frameTimes(meta): the sample times. One-shots add a last frame at t = duration (must be empty).
export function frameTimes(meta) {
  const fps = meta.fps ?? 30, n = Math.max(1, Math.round(meta.duration * fps));
  const count = meta.loop ? n : n + 1;
  return Array.from({ length: count }, (_, i) => (i * meta.duration) / n);
}
function renderAll(mod, extra, times, layer = 'both', timed = null, wrap = true) {
  const [w, h] = mod.meta.size, [ox, oy] = mod.meta.origin;
  const inst = instance(mod, extra, { wrap }), out = [];
  for (const time of times) {
    const f = frame(w, h), t0 = timed ? process.hrtime.bigint() : 0n;
    inst.seek(time);
    inst.draw(inst.paint(f, ox, oy), layer);
    if (timed) timed.push(Number(process.hrtime.bigint() - t0) / 1e6);
    out.push(f);
  }
  return out;
}

// ---------------------------------------------------------------- PNG writer
const CRC = (() => {
  const t = new Uint32Array(256);
  for (let n = 0; n < 256; n++) { let c = n; for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1; t[n] = c >>> 0; }
  return t;
})();
function crc32(buf) { let c = 0xffffffff; for (const b of buf) c = CRC[(c ^ b) & 255] ^ (c >>> 8); return (c ^ 0xffffffff) >>> 0; }
// pngBytes(w, h, rgba, deflateSync): RGBA8 PNG, filter 0 on every row.
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

// ---------------------------------------------------------------- CLI
const HELP = `pxfx - pixel VFX tool
usage: node pxfx.mjs <command> <effect.mjs> [flags]
  new <path.mjs> [--loop]   write a starter effect (one-shot simulated, or a pure loop)
  render <effect.mjs>       write <out>/<name>/contact.png (frames in a grid, zoomed)
  sheet <effect.mjs>        write <out>/<name>.png (1x, transparent) and <out>/<name>.json
  check <effect.mjs>        PASS/FAIL lines; exit code 1 on any FAIL
flags:
  --out <dir>        output folder (default pxfx-out)
  --opts '<json>'    override options, e.g. '{"palette":"ice","seed":7,"flip":true}'
  --zoom <n>         render: scale (default 3)
  --all              render: every frame (default: at most 24, evenly spaced)
  --frames 3-8,12    render: only these frames
  --bg dark|light|none   render: backdrop (default dark)
  --columns <n>      render/sheet: frames per row
  --layer both|back|front   render/sheet: which layer to draw (default both)
palettes: ${Object.keys(RAMPS).join(', ')}`;

function parseArgs(argv) {
  const a = { _: [] };
  for (let i = 0; i < argv.length; i++) {
    const s = argv[i];
    if (s === '--all' || s === '--loop' || s === '--help' || s === '-h') a[s.replace(/^-+/, '')] = true;
    else if (s.startsWith('--')) a[s.slice(2)] = argv[++i];
    else a._.push(s);
  }
  return a;
}
const DIGITS = ['111101101101111', '010110010010111', '111001111100111', '111001111001111', '101101111001001',
  '111100111001111', '111100111101111', '111001010010010', '111101111101111', '111101111001111'];

function contactSheet(frames, idx, meta, zoom, bgMode, cols, hitFrames) {
  const [w, h] = meta.size, [ox, oy] = meta.origin;
  const cw = w * zoom, ch = h * zoom, lab = 14, gap = 6;
  cols = cols || Math.max(1, Math.min(idx.length, Math.floor(1200 / (cw + gap)) || 1));
  const rows = Math.ceil(idx.length / cols);
  const W = gap + cols * (cw + gap), H = gap + rows * (ch + lab + gap);
  const img = new Uint8ClampedArray(W * H * 4);
  for (let i = 0; i < W * H; i++) img.set([10, 10, 14, 255], i * 4);
  const back = bgMode === 'light' ? [[200, 200, 208], [220, 220, 226], [176, 176, 186]]
    : [[22, 22, 30], [29, 29, 39], [44, 44, 58]];
  const bgFn = (x, y) => (y === oy || (x === ox && Math.abs(y - oy) <= 2) ? back[2] : back[((x >> 2) + (y >> 2)) & 1]);
  frames.forEach((f, n) => {
    const rgba = composite(f, bgMode === 'none' ? null : bgFn);
    const cx = gap + (n % cols) * (cw + gap), cy = gap + Math.floor(n / cols) * (ch + lab + gap);
    const s = String(idx[n]), hit = hitFrames.has(idx[n]);
    for (let d = 0; d < s.length; d++) {
      const g = DIGITS[+s[d]];
      for (let k = 0; k < 15; k++) if (g[k] === '1') {
        for (let yy = 0; yy < 2; yy++) for (let xx = 0; xx < 2; xx++) {
          const X = cx + d * 8 + (k % 3) * 2 + xx, Y = cy + 1 + Math.floor(k / 3) * 2 + yy;
          img.set(hit ? [255, 200, 60, 255] : [150, 150, 170, 255], (Y * W + X) * 4);
        }
      }
    }
    for (let y = 0; y < ch; y++) {
      for (let x = 0; x < cw; x++) {
        const si = ((Math.floor(y / zoom) * w) + Math.floor(x / zoom)) * 4, di = ((cy + lab + y) * W + cx + x) * 4;
        if (bgMode === 'none' && rgba[si + 3] === 0) img.set(((x >> 3) + (y >> 3)) & 1 ? [40, 40, 40, 255] : [60, 60, 60, 255], di);
        else if (bgMode === 'none') { const a = rgba[si + 3] / 255, base = ((x >> 3) + (y >> 3)) & 1 ? 40 : 60;
          img.set([rgba[si] * a + base * (1 - a), rgba[si + 1] * a + base * (1 - a), rgba[si + 2] * a + base * (1 - a), 255], di); }
        else img.set(rgba.subarray(si, si + 4), di);
      }
    }
  });
  return { W, H, img };
}

// frameList('3-8,12', n): frame indices from a range list.
function frameList(spec, n) {
  const out = [];
  for (const part of String(spec).split(',')) {
    const [lo, hi = lo] = part.split('-').map((v) => parseInt(v, 10));
    for (let i = lo; i <= hi; i++) if (i >= 0 && i < n) out.push(i);
  }
  if (!out.length) { console.error(`--frames ${spec}: no frames in 0..${n - 1}`); process.exit(2); }
  return out;
}

function hitFrameSet(meta, times) {
  const s = new Set();
  for (const hit of meta.timing?.hits ?? []) {
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

function checkTiming(meta) {
  const t = meta.timing, bad = [];
  if (!t) return null;
  if (!(t.windup >= 0)) bad.push('windup must be >= 0');
  if (!(t.recovery >= 0)) bad.push('recovery must be >= 0');
  const hits = t.hits ?? [];
  if (!Array.isArray(hits)) return ['hits must be an array'];
  hits.forEach((x, i) => {
    if (!(x.t >= 0 && x.t <= meta.duration)) bad.push(`hit ${i}: t outside 0..duration`);
    if (!(x.freeze >= 0 && x.freeze <= 0.3)) bad.push(`hit ${i}: freeze outside 0..0.3`);
    if (!(x.shake >= 0) || !(x.amp >= 0)) bad.push(`hit ${i}: shake and amp must be >= 0`);
    if (i > 0 && x.t < hits[i - 1].t) bad.push(`hit ${i}: hits not sorted by t`);
  });
  return bad;
}

async function runCheck(mod, extra) {
  const meta = mod.meta, lines = [];
  let fail = false;
  const line = (st, name, msg) => { lines.push(`${st.padEnd(4)} ${name}: ${msg}`); if (st === 'FAIL') fail = true; };
  const bad = checkMeta(mod);
  line(bad.length ? 'FAIL' : 'PASS', 'meta', bad.length ? bad.join('; ') : `${meta.name} ${meta.size.join('x')} ${meta.loop ? 'loop' : 'one-shot'} ${meta.duration}s @${meta.fps ?? 30}fps`);
  if (bad.length) return { lines, fail };
  const times = frameTimes(meta);
  const origRandom = Math.random, OrigDate = globalThis.Date;
  let runA, runB, err = null;
  const ms = [];
  Math.random = () => { throw new Error('Math.random() called; use rng(o.seed)'); };
  globalThis.Date = class { constructor() { throw new Error('Date used; effects must depend on t only'); } static now() { throw new Error('Date.now() used'); } };
  try {
    renderAll(mod, extra, times);
    runA = renderAll(mod, extra, times, 'both', ms);
    runB = renderAll(mod, extra, times);
  } catch (e) { err = e; } finally { Math.random = origRandom; globalThis.Date = OrigDate; }
  if (err) { line('FAIL', 'runs', String(err.message || err)); return { lines, fail }; }
  line('PASS', 'runs', `${times.length} frames, no Math.random or Date`);
  const outside = runA.reduce((s, f) => s + f.outside, 0);
  const worst = runA.reduce((b, f, i) => (f.outside > runA[b].outside ? i : b), 0);
  line(outside ? 'FAIL' : 'PASS', 'inside canvas', outside ? `${outside} pixel writes outside the canvas (worst frame ${worst})` : 'no writes outside the canvas');
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
    const [a, b] = renderAll(mod, extra, [0, meta.duration], 'both', null, false).map((f) => composite(f));
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
  const tb = checkTiming(meta);
  if (tb) line(tb.length ? 'FAIL' : 'PASS', 'timing', tb.length ? tb.join('; ') : `windup ${meta.timing.windup}s, ${(meta.timing.hits ?? []).length} hit(s), recovery ${meta.timing.recovery}s`);
  else line('INFO', 'timing', 'no meta.timing (add it to anything that hits)');
  return { lines, fail };
}

const STARTER_ONESHOT = (name, lib) => `// ${name}: starter one-shot (simulated). Brief: <what, how it moves, palette, timing>.
import { rng, particles, ease, clamp, shade } from '${lib}';

const FLASH = 1 / 30; // the white flash frame
const RING = 0.28;    // ring expands for this long

export const meta = {
  name: '${name}',
  size: [80, 56],
  origin: [40, 44],
  duration: 1.2,
  loop: false,
  fps: 30,
  options: { palette: 'fire', seed: 1 },
  tags: ['impact'],
  timing: { windup: 0.15, hits: [{ t: 0, freeze: 0.05, shake: 0.12, amp: 2 }], recovery: 0.25 },
};

export function create(o) {
  const r = rng(o.seed), ps = particles(r);
  for (let i = 0; i < 12; i++) {
    const a = -Math.PI * r.range(0.1, 0.9), s = r.range(50, 110);
    ps.spawn(0, -10, Math.cos(a) * s, Math.sin(a) * s, r.range(0.3, 0.6), { kind: 'spark', ramp: o.palette });
  }
  for (let i = 0; i < 3; i++) {
    ps.spawn(r.range(-4, 4), -10, r.range(-12, 12), r.range(-14, -6), r.range(0.6, 0.9),
      { kind: 'smoke', size: r.range(2, 3), delay: 0.05 + i * 0.04 });
  }
  let t = 0;
  return {
    update(dt) { t += dt; ps.update(dt); },
    draw(p) {
      const u = clamp(t / RING);
      if (u < 1) {
        p.ring(0, -10, 2 + 12 * ease.outQuart(u), u < 0.5 ? 2 : 1, shade(o.palette, 1 - u), 1 - u);
        p.glow(0, -10, 18, shade(o.palette, 0.8), 0.7 * (1 - u) ** 2);
      }
      if (t < FLASH) p.disc(0, -10, 4, '#ffffff');
      ps.draw(p);
    },
  };
}
`;
const STARTER_LOOP = (name, lib) => `// ${name}: starter loop (pure). Brief: <what, how it moves, palette, timing>.
import { cycle, shade, TAU } from '${lib}';

export const meta = {
  name: '${name}',
  size: [48, 48],
  origin: [24, 24],
  duration: 1,
  loop: true,
  fps: 24,
  options: { palette: 'arcane', seed: 1 },
  tags: ['aura'],
};

export function draw(p, t, o) {
  const ph = t / meta.duration, pulse = 0.5 + 0.5 * Math.sin(ph * TAU);
  p.glow(0, 0, 16, shade(o.palette, 0.7), 0.25 + 0.15 * pulse);
  p.disc(0, 0, 3 + pulse, shade(o.palette, 0.5));
  p.disc(0, -1, 1.5, shade(o.palette, 1));
  cycle(t, meta.duration, 6, (q, i, r) => {
    const a = q * TAU + r.range(0, 0.4), rad = 9 + 2 * Math.sin(q * TAU * 2 + i);
    const x = Math.cos(a) * rad, y = Math.sin(a) * rad * 0.6;
    p.px(x, y, shade(o.palette, 0.9));
    p.lit(x, y, shade(o.palette, 1), 0.6);
  }, { seed: o.seed });
}
`;

async function cli(argv) {
  const fs = await import('node:fs'), path = await import('node:path');
  const { pathToFileURL, fileURLToPath } = await import('node:url'), { deflateSync } = await import('node:zlib');
  const a = parseArgs(argv), [cmd, file] = a._;
  if (!cmd || a.help || !file) { console.log(HELP); process.exit(cmd && !a.help ? 2 : 0); }
  if (cmd === 'new') {
    if (fs.existsSync(file)) { console.error(`${file} exists; not overwriting`); process.exit(2); }
    const name = path.basename(file).replace(/\.m?js$/, '');
    let lib = path.relative(path.dirname(path.resolve(file)), fileURLToPath(import.meta.url)).split(path.sep).join('/');
    if (!lib.startsWith('.')) lib = './' + lib;
    fs.mkdirSync(path.dirname(path.resolve(file)), { recursive: true });
    fs.writeFileSync(file, (a.loop ? STARTER_LOOP : STARTER_ONESHOT)(name, lib));
    console.log(`wrote ${file} (${a.loop ? 'pure loop' : 'one-shot simulated'})`);
    return;
  }
  const mod = await import(pathToFileURL(path.resolve(file)).href);
  let extra = {};
  if (a.opts) { try { extra = JSON.parse(a.opts); } catch (e) { console.error(`--opts is not JSON: ${e.message}`); process.exit(2); } }
  const outDir = a.out ?? 'pxfx-out', layer = a.layer ?? 'both';
  if (!['both', 'back', 'front'].includes(layer)) { console.error('--layer must be both, back or front'); process.exit(2); }
  if (cmd === 'check') {
    const { lines, fail } = await runCheck(mod, extra);
    console.log(lines.join('\n'));
    console.log(fail ? 'RESULT: FAIL' : 'RESULT: PASS');
    process.exit(fail ? 1 : 0);
  }
  const bad = checkMeta(mod);
  if (bad.length) { console.error(`meta: ${bad.join('; ')}`); process.exit(1); }
  const meta = mod.meta, [w, h] = meta.size, times = frameTimes(meta), cols = a.columns ? Math.max(1, parseInt(a.columns, 10)) : 0;
  const ms = [], frames = renderAll(mod, extra, times, layer, ms);
  const suffix = layer === 'both' ? '' : `-${layer}`;
  if (cmd === 'render') {
    const n = times.length, cap = 24;
    let idx = a.all || n <= cap ? times.map((_, i) => i) : [...new Set(Array.from({ length: cap }, (_, i) => Math.round((i * (n - 1)) / (cap - 1))))];
    if (a.frames) idx = frameList(a.frames, n);
    const { W, H, img } = contactSheet(idx.map((i) => frames[i]), idx, meta, Math.max(1, parseInt(a.zoom ?? '3', 10)), a.bg ?? 'dark', cols, hitFrameSet(meta, times));
    const dir = path.join(outDir, meta.name);
    fs.mkdirSync(dir, { recursive: true });
    const out = path.join(dir, `contact${suffix}.png`);
    fs.writeFileSync(out, pngBytes(W, H, img, deflateSync));
    const med = [...ms].sort((p, q) => p - q)[Math.floor(ms.length / 2)];
    console.log(`wrote ${out} (${W}x${H}): ${idx.length} of ${n} frames, median ${med.toFixed(2)} ms/frame`);
    return;
  }
  if (cmd === 'sheet') {
    const n = frames.length, c = cols || Math.min(n, Math.max(1, Math.floor(2048 / w))), rows = Math.ceil(n / c);
    const W = c * w, H = rows * h, img = new Uint8ClampedArray(W * H * 4), name = meta.name + suffix;
    const fdur = Math.round((1000 * meta.duration) / Math.max(1, Math.round(meta.duration * (meta.fps ?? 30))));
    const json = { frames: [], meta: {} };
    frames.forEach((f, i) => {
      const rgba = composite(f), x = (i % c) * w, y = Math.floor(i / c) * h;
      for (let yy = 0; yy < h; yy++) img.set(rgba.subarray(yy * w * 4, (yy + 1) * w * 4), ((y + yy) * W + x) * 4);
      json.frames.push({ filename: `${name}_${String(i).padStart(3, '0')}`, frame: { x, y, w, h }, rotated: false, trimmed: false,
        spriteSourceSize: { x: 0, y: 0, w, h }, sourceSize: { w, h }, duration: fdur });
    });
    const o = options(meta, extra), timing = meta.timing ? { ...meta.timing,
      hits: (meta.timing.hits ?? []).map((x) => ({ ...x, frame: [...hitFrameSet({ timing: { hits: [x] } }, times)][0] })) } : undefined;
    json.meta = { app: 'pxfx', version: '1', image: `${name}.png`, format: 'RGBA8888', size: { w: W, h: H }, scale: '1',
      frameTags: [{ name: meta.name, from: 0, to: n - 1, direction: 'forward' }],
      vfx: { fps: meta.fps ?? 30, loop: !!meta.loop, frames: n, frameSize: { w, h }, columns: c,
        pivot: { x: meta.origin[0] + (o.x ?? 0), y: meta.origin[1] + (o.y ?? 0) }, layer, ...(timing ? { timing } : {}) } };
    fs.mkdirSync(outDir, { recursive: true });
    fs.writeFileSync(path.join(outDir, `${name}.png`), pngBytes(W, H, img, deflateSync));
    fs.writeFileSync(path.join(outDir, `${name}.json`), JSON.stringify(json, null, 1) + '\n');
    console.log(`wrote ${path.join(outDir, name + '.png')} (${W}x${H}, ${n} frames, ${c} columns) and ${name}.json`);
    return;
  }
  console.error(`unknown command: ${cmd}\n${HELP}`);
  process.exit(2);
}

if (typeof process !== 'undefined' && process.argv?.[1]) {
  const { pathToFileURL } = await import('node:url'), { realpathSync } = await import('node:fs');
  let me = '';
  try { me = pathToFileURL(realpathSync(process.argv[1])).href; } catch { /* not a file */ }
  // Not awaited: the effect module imports this file, which must finish loading first.
  if (me === import.meta.url) cli(process.argv.slice(2)).catch((e) => { console.error(e); process.exit(1); });
}
```
