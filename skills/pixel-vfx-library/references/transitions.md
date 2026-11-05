# Transitions

Full-screen 320x180 scene wipes. Each covers the screen fully at its single timing hit, where the game swaps scenes, and ends empty, so it passes `check` like any one-shot.

## transitions/iris

A circle closes to black on a point (options `cx`, `cy`) with a dithered edge and a thin cool rim, holds shut while the scene swaps, then opens fast and settles.

```js
// id: transitions/iris
import { shade, ease, clamp, lerp, rgb } from './pxfx.mjs';

const CLOSE = 0.6, OPEN = 0.95, EDGE = 10;   // closed from CLOSE to OPEN; the game swaps scenes at CLOSE

export const meta = {
  name: 'iris', size: [320, 180], origin: [0, 0], duration: 1.6, loop: false, fps: 30,
  options: { palette: 'night', seed: 1, cx: 160, cy: 100 },
  tags: ['transition', 'iris', 'wipe', 'fullscreen'],
  timing: { windup: 0, hits: [{ t: CLOSE, freeze: 0, shake: 0, amp: 0 }], recovery: 0 },
};

export function draw(p, t, o) {
  const { cx, cy } = o, far = Math.hypot(Math.max(cx, 320 - cx), Math.max(cy, 180 - cy)) + EDGE + 2;
  // the radius: eases in to a small pause just before shutting, holds shut, then opens fast and settles
  const R = t < CLOSE ? lerp(far, -EDGE, ease.inOutSine(t / CLOSE)) : t < OPEN ? -EDGE
    : lerp(-EDGE, far, ease.outCubic((t - OPEN) / (meta.duration - OPEN - 0.04)));
  const ink = rgb(shade(o.palette, 0)), rim = rgb(shade(o.palette, 0.8));
  const span = (a, b, y) => { for (let j = y; j < y + 4; j++) for (let x = a; x < b; x++) p.px(x, j, ink); };
  const Rf = Math.max(0, R + EDGE + 4), Ri = R - 3;
  for (let y = 0; y < 180; y += 4) {   // solid spans outside the edge, per pixel only across it
    const dy = Math.max(0, Math.abs(y + 1.5 - cy) - 2), hw = Rf > dy ? Math.sqrt(Rf * Rf - dy * dy) : -1;
    const x0 = Math.max(0, Math.floor((cx - hw) / 4) * 4), x1 = Math.min(320, Math.ceil((cx + hw) / 4) * 4);
    if (hw < 0) { span(0, 320, y); continue; }
    if (x0 > 0) span(0, x0, y);
    if (x1 < 320) span(x1, 320, y);
    const dm = Math.abs(y + 1.5 - cy) + 2, hi = Ri > dm ? Math.sqrt(Ri * Ri - dm * dm) : -1;   // clear inside: skip it
    for (let yy = y; yy < y + 4; yy++) for (let x = x0; x < x1; x++) {
      if (Math.abs(x - cx) < hi) { x = Math.floor(cx + hi); continue; }
      const e = Math.hypot(x - cx, yy - cy) - R;
      if (e < -1.2 || (e < 0 && R <= 2)) continue;
      if (e > 0) p.px(x, yy, ink, clamp(e / EDGE + 0.06));
      else p.px(x, yy, rim);   // a thin cool rim on the opening
    }
  }
  if (R > -EDGE && R < 16) p.glow(cx, cy, 10, rim, 0.25 * clamp(R / 16 + 0.6));
}
```

## transitions/pixel-dissolve

The screen goes black cell by cell in 16 ordered-dither steps along a slight diagonal sweep, the newest cells glint, it holds, then clears in reverse (options `cell`, `sweep`).

```js
// id: transitions/pixel-dissolve
import { shade, bayer, clamp, rgb } from './pxfx.mjs';

const IN = 0.5, OUT = 0.7, STEPS = 16;   // covered from IN to OUT; the game swaps scenes at IN

export const meta = {
  name: 'pixel-dissolve', size: [320, 180], origin: [0, 0], duration: 1.2, loop: false, fps: 30,
  options: { palette: 'night', seed: 1, cell: 4, sweep: 0.3 },
  tags: ['transition', 'dissolve', 'dither', 'fullscreen'],
  timing: { windup: 0, hits: [{ t: IN, freeze: 0, shake: 0, amp: 0 }], recovery: 0 },
};

export function draw(p, t, o) {
  const C = Math.max(1, Math.round(o.cell)), s = clamp(o.sweep), nx = Math.ceil(320 / C), ny = Math.ceil(180 / C);
  // the cover level climbs in whole dither steps, holds, then falls back the same way
  const u = t < IN ? t / IN : t < OUT ? 1 : 1 - (t - OUT) / (meta.duration - OUT - 0.05);
  const L = Math.floor(clamp(u) * (STEPS + 1)) / STEPS;
  if (L <= 0) return;
  const ink = rgb(shade(o.palette, 0)), edge = rgb(shade(o.palette, 0.4)), hot = rgb(shade(o.palette, 0.8)), row = [];
  for (let y = 0; y < 180; y++) {
    const j = Math.floor(y / C);
    if (y % C === 0) for (let i = 0; i < nx; i++) {
      // a 4x4 ordered threshold per cell, leaned along a diagonal sweep so the dissolve flows
      const th = bayer(i, j) * (1 - s) + s * (i / nx + j / ny) / 2;
      row[i] = th >= L ? null : th >= L - 1 / STEPS && L < 1 ? (th >= L - 0.5 / STEPS ? hot : edge) : ink;
    }
    for (let x = 0; x < 320; x++) { const c = row[Math.floor(x / C)]; if (c) p.px(x, y, c); }
  }
}
```
