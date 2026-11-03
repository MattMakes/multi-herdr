# Sci-fi

Energy and hologram effects: a scan-line teleport that draws the body in, and a hex shield dome. Cyan by default; any ramp recolours them.

## scifi/warp-in

On-body spawn: a beam of scrolling scan lines drops onto a floor pad, a bright scan line draws the character in from head to feet with tinted, glitch-jolted rows (actorPaint; actorAlpha ramps 0 to 1 for sheets), a white body frame, then the beam lifts away and the pad ring spreads.

```js
// id: scifi/warp-in
import { ease, clamp, lerp, shade, hash, sparkle, cycle } from './pxfx.mjs';

const DROP = 0.15, SCAN = 0.8, WHITE = 0.05, END = 1.4;   // beam lands, scan reaches the feet, white frames

export const meta = {
  name: 'warp-in', size: [64, 72], origin: [32, 62], duration: 1.4, loop: false, fps: 30, actor: true,
  options: { palette: 'cyber', seed: 6 }, tags: ['scifi', 'teleport', 'spawn'],
};

// body box from the mask: centre x, half width, top
const box = (A) => { let l = 1e9, r = -1e9, u = 1e9;
  for (let k = 0; k < A.mask.length; k++) if (A.mask[k]) { const i = k % A.w; l = Math.min(l, i); r = Math.max(r, i); u = Math.min(u, (k / A.w) | 0); }
  return [A.x + (l + r) / 2, (r - l) / 2, A.y + u]; };
const scanY = (t, A) => lerp(box(A)[2] - 2, 1, ease.inOutSine((t - DROP) / (SCAN - DROP)));
export const actorAlpha = (t) => clamp((t - DROP) / (SCAN - DROP));

// above the scan line: fresh rows tinted, striped, jolted sideways
export function actorPaint(p, t, o) {
  const A = o.actor, sy = scanY(t, A), slot = Math.floor(t * 15);
  if (t < DROP) return;
  if (t >= SCAN - 0.01) return p.body(A, t < SCAN + WHITE ? () => '#ffffff' : null);
  for (let j = 0; j < A.h; j++) {
    const y = A.y + j, d = sy - y, g = hash(j >> 1, slot, o.seed), dx = d < 12 && g < 0.25 ? Math.round((g - 0.125) * 24) : 0;
    if (d >= 0 && !(d < 7 && j % 2)) for (let i = 0; i < A.w; i++) {
      const k = j * A.w + i, c = A.pixels[k];
      if (A.mask[k]) p.px(A.x + i + dx, y, d < 7 ? shade(o.palette, 0.35 + (c[0] + c[1] * 2 + c[2]) / 1570) : c);
    }
  }
}

const pad = (p, t, o, half) => {
  const u = clamp((t - DROP) / (END - DROP - 0.1)), v = clamp((t - SCAN) / 0.35), rx = lerp(12, 26, ease.outQuart(v));
  if (t < DROP || u >= 1) return;
  p.oval(0, 0, 12, 3, shade(o.palette, half === 'back' ? 0.45 : 0.7), u < 0.75 ? 1 : (1 - u) * 4, { width: 1, half });
  if (t > SCAN && v < 1) p.oval(0, 0, rx, rx * 0.25, shade(o.palette, 0.9 - v * 0.6), 1 - v, { width: 1, half });
};
export const drawBack = (p, t, o) => pad(p, t, o, 'back');

export function draw(p, t, o) {
  const A = o.actor, C = (u) => shade(o.palette, u), sy = scanY(t, A), [x0, hw] = box(A), w = Math.ceil(hw) + 3;
  const b1 = t < SCAN ? lerp(-60, 0, ease.outQuart(t / DROP)) : lerp(0, -60, ease.inQuad((t - SCAN) / 0.25));
  if (t < SCAN + 0.25) {   // rails, core of scan lines
    for (let y = -60; y <= b1; y++) if ((y - Math.floor(t * 60)) % 3) {
      p.px(x0 - w, y, C(0.6)); p.px(x0 + w, y, C(0.6));
      if (t < DROP || y > sy + 2) p.px(x0, y, C(0.85));
    }
    for (let y = -54; y < b1; y += 12) p.glow(x0, y, w + 4, C(0.5), 0.2);
  }
  if (t >= DROP && t < SCAN) {   // scan line, striped wireframe below
    p.line(x0 - w - 2, sy, x0 + w + 2, sy, '#ffffff');
    p.glow(x0, sy, w + 6, C(0.8), 0.7, 0.3);
    for (let j = 0; j < A.h; j += 2) if (A.y + j > sy + 1) for (let i = 0; i < A.w; i++) {
      const k = j * A.w + i;
      if (A.mask[k] && !(A.mask[k - 1] && A.mask[k + 1])) p.px(A.x + i, A.y + j, C(0.4));
    }
  }
  const f = clamp((t - SCAN) / 0.4);
  if (t >= SCAN && f < 1) { p.glow(x0, A.chest[1], 22, C(0.8), 0.9 * (1 - f) ** 2); p.glow(0, 0, 20, C(0.6), 0.5 * (1 - f), 0.3); }
  cycle(t, END, 6, (q, i, r) => {   // glints along the rails
    const s = Math.sin(clamp((t - DROP - i * 0.12) / 0.5) * Math.PI);
    if (s > 0.1) sparkle(p, x0 + (i % 2 ? w : -w), r.int(-34, -4), Math.round(s * 2), '#ffffff', C(0.7));
  }, { seed: o.seed });
  pad(p, t, o, 'front');
}
```

## scifi/force-field

Loop: a hex-patterned dome over the anchor; the far wall's hex lines sit behind the character, the near wall is light only, with a bright 1 px rim, cells that shimmer at random and a highlight band that travels across once per loop.

```js
// id: scifi/force-field
import { shade, hash, TAU } from './pxfx.mjs';

const RX = 21, RY = 44, CELL = 8;   // dome radii (feet at 0, 0) and hex size in px

export const meta = {
  name: 'force-field', size: [64, 64], origin: [32, 56], duration: 2, loop: true, fps: 15,
  options: { palette: 'cyber', seed: 3 }, tags: ['scifi', 'shield', 'dome', 'hex', 'loop'],
};

// distance to the nearest hex border (0 on the edge) and the cell id, for a point in cell units
function hex(x, y) {
  const m = (v, s) => v - s * Math.floor(v / s), K = 1.732;
  const ax = m(x, 1) - 0.5, ay = m(y, K) - K / 2, bx = m(x - 0.5, 1) - 0.5, by = m(y - K / 2, K) - K / 2;
  const [gx, gy] = ax * ax + ay * ay < bx * bx + by * by ? [ax, ay] : [bx, by];
  return [0.5 - Math.max(Math.abs(gx), Math.abs(gx) * 0.5 + Math.abs(gy) * 0.866), Math.round((x - gx) * 2), Math.round((y - gy) * 2)];
}

// one wall of the dome: the far wall (back) as dim lines, the near wall (front) as light
function wall(p, t, o, back) {
  const ph = t / meta.duration, band = -1.7 + 3.4 * ph, s = o.seed;
  p.shade(-RX - 1, -RY - 1, RX + 1, 0, (x, y) => {
    const nx = x / RX, ny = y / RY, d = nx * nx + ny * ny;
    if (d > 1) return null;
    const nz = Math.sqrt(1 - d), k = 0.35 + 0.65 * nz, rim = (1 - Math.sqrt(d)) * Math.hypot(x, y) / Math.sqrt(d || 1);
    const [e, ci, cj] = hex((x / k + (back ? 3 : 0)) / CELL, y / k / CELL), hl = Math.max(0, 1 - Math.abs(nx + ny * 0.5 - band) * 3);
    const fl = Math.max(0, Math.sin(ph * TAU * 2 + hash(ci, cj, s) * TAU)) ** 12;
    if (back) return e < 0.06 && rim > 2 ? { c: shade(o.palette, 0.25 + 0.3 * hl) } : null;
    if (rim < 2) return { c: shade(o.palette, rim < 1 ? 0.85 + 0.15 * hl : 0.4), l: shade(o.palette, 0.7), k: 0.2 + 0.4 * hl };
    const L = (e < 0.06 ? 0.1 + 0.7 * hl + 0.25 * (1 - nz) : 0) + 0.1 * fl;
    return L > 0.02 ? { l: shade(o.palette, 0.75), k: L } : null;
  });
}

export function drawBack(p, t, o) {
  wall(p, t, o, true);
  p.oval(0, 0, RX + 2, 5, shade(o.palette, 0.4), 1, { width: 1, half: 'back' });
}

export function draw(p, t, o) {
  const ph = t / meta.duration, pulse = 0.5 + 0.5 * Math.sin(ph * TAU * 2);
  wall(p, t, o, false);
  p.oval(0, 0, RX + 2, 5, shade(o.palette, 0.7), 1, { width: 1, half: 'front' });
  p.glow(0, 0, RX + 8, shade(o.palette, 0.6), 0.3 + 0.1 * pulse, 0.25);
  p.glow(0, -RY * 0.5, RX + 6, shade(o.palette, 0.5), 0.12 + 0.05 * pulse, 1.6);
}
```
