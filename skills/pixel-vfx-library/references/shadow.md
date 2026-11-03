# Shadow

Darkness with a thin violet edge: near-black bodies, lit rims, light only at the edges and tips.

## shadow/void-orb

A near-black sphere with a slow inner swirl, a thin violet rim and a running highlight; it squashes gently, pull rings contract onto it and motes spiral in. Pure loop.

```js
// id: shadow/void-orb
import { cycle, shade, clamp, TAU } from './pxfx.mjs';

const R = 9;   // orb radius

export const meta = {
  name: 'void-orb', size: [64, 64], origin: [32, 32], duration: 2, loop: true, fps: 15,
  options: { palette: 'shadow', seed: 2 }, tags: ['shadow', 'void', 'orb', 'dark', 'loop', 'projectile'],
};

export function draw(p, t, o) {
  const ph = t / meta.duration, P = o.palette, by = Math.round(Math.sin(ph * TAU) * 1.2);
  const rad = (a) => R + 0.7 * Math.sin(2 * a) * Math.sin(ph * TAU * 2) + 0.4 * Math.sin(4 * a - ph * TAU * 3);
  for (let k = 0; k < 6; k++) { const a = (k / 6) * TAU + ph * TAU; p.glow(Math.cos(a) * (R + 5), by + Math.sin(a) * (R + 5), 10, shade(P, 0.7), 0.13); }
  for (let k = 0; k < 2; k++) {                   // pull rings contract onto the orb
    const u = (ph * 2 + k / 2) % 1, rr = R + 3 + 13 * (1 - u) ** 2, a0 = u * 3 + k * 2;
    for (let j = 0; j < 3; j++) p.arc(0, by, rr, a0 + j * 2.1, a0 + j * 2.1 + 1.2, 1, shade(P, 0.15 + 0.4 * u));
  }
  cycle(t, meta.duration, 16, (q, i, r) => {      // motes spiral in, faster near the rim
    const a0 = r.range(0, TAU), turn = r.range(1.5, 2.5), d0 = r.range(18, 27);
    const pt = (s) => { const a = a0 + s * s * turn, d = R + 1 + (d0 - R) * (1 - s * s); return [Math.cos(a) * d, by + Math.sin(a) * d]; };
    const al = 1, c = shade(P, 0.3 + 0.7 * q);
    p.polyline([pt(Math.max(0, q - 0.12)), pt(Math.max(0, q - 0.05)), pt(q)], shade(P, 0.15 + 0.45 * q), al);
    const [x, y] = pt(q);
    p.px(x, y, q > 0.9 ? '#ffffff' : c, al);
    p.lit(x, y, c, 0.5 * al);
  }, { seed: o.seed, speed: 2 });
  // the body: near black, a slow inner swirl, a lit inner crescent and a thin violet rim
  p.shade(-R - 2, by - R - 2, R + 2, by + R + 2, (x, y) => {
    const dy = y - by, d = Math.hypot(x, dy), a = Math.atan2(dy, x), e = rad(a);
    if (d > e + 0.5) return null;
    if (d > e - 0.5) {
      const hl = Math.cos(a - ph * TAU - 3.9);
      return { c: shade(P, hl > 0.75 ? 1 : 0.75), l: shade(P, 0.9), k: hl > 0.75 ? 0.5 : 0.15 };
    }
    if (d > e - 2 && (-x - dy) / d > 0.9) return { c: shade(P, 0.4) };
    const sw = Math.sin(a * 2 + d * 0.7 - ph * TAU * 2);
    return { c: shade(P, sw > 0.8 && d > 3 ? 0.2 : 0) };
  });
}
```

## shadow/shadow-tendrils

A dark pool spreads, tendrils whip up with a travelling wave, clench shut on a flash (the hit), squeeze, then curl back and sink; wisps leak up. One-shot.

```js
// id: shadow/shadow-tendrils
import { rng, ease, clamp, lerp, shade, sparkle, TAU } from './pxfx.mjs';

const RISE = 0.25, GRASP = 0.4, SINK = 0.95, GONE = 1.45, TILT = 0.28;

export const meta = {
  name: 'shadow-tendrils', size: [96, 72], origin: [48, 62], duration: 2.2, loop: false, fps: 30,
  options: { palette: 'shadow', seed: 4 }, tags: ['shadow', 'tendril', 'grab', 'root', 'ground'],
  timing: { windup: 0.1, hits: [{ t: GRASP, freeze: 0.08, shake: 0.15, amp: 2 }], recovery: 0.5 },
};

const pool = (t) => 18 * ease.outQuart(t / 0.18) * (1 - ease.inQuad((t - GONE) / (meta.duration - 0.15 - GONE)));

function tendrils(p, t, o, back) {
  const r = rng(o.seed), P = o.palette, rx = pool(t) * 0.7;
  const grow = ease.outExpo(t / RISE) * (1 - ease.inQuad((t - SINK) / (GONE - SINK)));
  const curl = ease.outBack((t - GRASP + 0.08) / 0.12) - 0.5 * ease.inOutQuad((t - SINK) / 0.3) + (t > GRASP && t < SINK ? 0.12 * Math.sin((t - GRASP) * 13) : 0);
  for (let i = 0; i < 7; i++) {
    const a = (i / 7) * TAU + r.range(-0.3, 0.3), Lm = r.range(24, 38), ph = r.range(0, TAU), dl = r.range(0, 0.08);
    if (Math.sin(a) < 0 !== back) continue;
    const bx = Math.cos(a) * rx, by = Math.sin(a) * rx * TILT, L = Lm * clamp(grow - dl * 4), sg = Math.sign(bx) || 1;
    if (L < 1) continue;
    let x = bx, y = by, ang = -Math.PI / 2 + sg * 0.22;
    const pts = [[x, y]];
    for (let k = 1; k <= 8; k++) {
      ang += (k > 4 ? -sg * curl * 0.42 : 0) + Math.sin(t * 9 + ph - k * 0.9) * 0.16 * (1 - 0.5 * clamp(curl));
      x += Math.cos(ang) * L / 8; y += Math.sin(ang) * L / 8; pts.push([x, y]);
    }
    const dim = back ? 0.15 : 0, flash = t >= GRASP && t < GRASP + 0.05;
    for (let k = 1; k <= 8; k++) p.beam(...pts[k - 1], ...pts[k], lerp(3.6, 1, k / 8), shade(P, 0), 1);
    for (let k = 2; k <= 8; k++) p.line(pts[k - 1][0] - sg, pts[k - 1][1], pts[k][0] - sg, pts[k][1], shade(P, 0.6 + 0.2 * k / 8 - dim));
    p.px(x, y, flash ? '#ffffff' : shade(P, 1 - dim)); p.lit(x, y, shade(P, 0.9), flash ? 1 : 0.4);
    if (flash && !back) sparkle(p, x, y, 2, '#ffffff', shade(P, 0.8));
  }
}

export function drawBack(p, t, o) {
  const P = o.palette, rx = pool(t);
  if (rx < 1) return;
  p.glow(0, 0, rx + 10, shade(P, 0.7), 0.3, 0.3);
  p.oval(0, 0, rx, rx * TILT, shade(P, 0));
  p.oval(0, 0, rx, rx * TILT, shade(P, 0.3), 1, { width: 1, half: 'back' });
  p.oval(0, 0, rx * 0.6, rx * 0.6 * TILT, shade(P, 0.15), 1, { width: 1 });
  tendrils(p, t, o, true);
}

export function draw(p, t, o) {
  const P = o.palette, rx = pool(t);
  if (rx >= 1) p.oval(0, 0, rx, rx * TILT, shade(P, 0.55), 1, { width: 1, half: 'front' });
  tendrils(p, t, o, false);
  const g = clamp((t - GRASP) / 0.35);
  if (t >= GRASP && g < 1) p.glow(0, -16, 22, shade(P, 0.8), 0.7 * (1 - g) ** 2);
  const r = rng(o.seed + 9);
  for (let i = 0; i < 10; i++) {      // dark wisps leak up from the pool, lit from above
    const x0 = r.range(-12, 12), d = r.range(0.1, 1.2), life = r.range(0.6, 0.9), u = (t - d) / life;
    if (u < 0 || u >= 1) continue;
    const x = x0 + Math.sin(u * 4 + i) * 2, y = -2 - 22 * ease.outQuad(u), rad = 1.5 + 2.5 * u, a = 0.6 * (1 - u) ** 1.3;
    p.puff(x, y, rad, shade(P, 0.15), a);
    p.puff(x - 0.5, y - rad * 0.4, rad * 0.55, shade(P, 0.6), a * 0.7);
  }
}
```
