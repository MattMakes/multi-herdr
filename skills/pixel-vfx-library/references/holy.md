# Holy

Light from above: heals, blessings and beams in gold and green. Bright cores, warm floor pools, slow drifting motes.

## holy/heal

On-body heal: green-gold motes rise around the body, a two-frame white flash, a gold rim and halo, the body tinted warm then fading back. One-shot, `meta.actor`.

```js
// id: holy/heal
import { rng, ease, clamp, lerp, shade, sparkle, band, FAR, NEAR, rgb, TAU } from './pxfx.mjs';

const FLASH = 0.3;   // the halo flash: the heal lands

export const meta = {
  name: 'heal', size: [64, 64], origin: [32, 58], duration: 1.6, loop: false, fps: 30, actor: true,
  options: { palette: 'heal', gold: 'holy', seed: 5 }, tags: ['holy', 'heal', 'buff', 'body'],
};

const warm = (t) => (t < FLASH ? clamp(t / FLASH) * 0.18 : t < FLASH + 0.05 ? 1 : 0.55 * (1 - ease.outQuad((t - FLASH - 0.05) / 0.8)));

export function actorPaint(p, t, o) {
  const k = warm(t);
  p.body(o.actor, (c) => {
    if (k <= 0 || k >= 1) return k ? '#ffffff' : c;
    const g = rgb(shade(o.gold, 0.2 + (0.3 * c[0] + 0.55 * c[1] + 0.15 * c[2]) / 255));
    return c.map((v, i) => Math.round(lerp(v, g[i], k)));
  });
}

// motes: rise around the body, the back half behind it and one step dimmer
function motes(p, t, o, back) {
  const r = rng(o.seed);
  for (let i = 0; i < 28; i++) {
    const x0 = r.range(-12, 12), z = r.range(-1, 1), d = r.range(0, 0.75), life = r.range(0.55, 0.85);
    const h = r.range(20, 40), gold = r.chance(0.4), big = r.chance(0.35), ph = r.range(0, TAU);
    if (z < 0 !== back) continue;
    const u = (t - d) / life;
    if (u < 0 || u >= 1) continue;
    const x = x0 + Math.sin(u * 5 + ph) * 1.5, y = -2 - h * ease.outQuad(u), a = clamp((1 - u) * 3);
    const R = gold ? o.gold : o.palette, c = shade(R, (back ? 0.55 : 0.75) - 0.25 * u);
    p.line(x, y + 1, x - Math.cos(u * 5 + ph) * 0.5, y + 3, shade(R, back ? 0.25 : 0.4), a * 0.7);
    if (big && u < 0.7) sparkle(p, x, y, 1, c, shade(R, 0.45), a); else p.px(x, y, c, a);
    p.lit(x, y, shade(R, 0.8), 0.5 * a);
  }
}
function ring(p, t, o, keep) {
  const u = clamp(t / 1.2);
  if (u < 1) band(p, 0, 0, lerp(5, 15, ease.outQuart(u)), 0.28, 1, 1 - u, o.palette, keep);
}

export function drawBack(p, t, o) {
  ring(p, t, o, FAR);
  motes(p, t, o, true);
}

export function draw(p, t, o) {
  const A = o.actor, f = clamp(1 - t / 1.4);
  if (f > 0) p.glow(0, 0, 18, shade(o.palette, 0.6), 0.4 * f * f, 0.3);
  ring(p, t, o, NEAR);
  const s = t - FLASH;
  if (s >= 0 && s < 0.6) {
    const u = s / 0.6, w = s < 0.05;
    if (u < 0.5) p.outline(A, A.x, A.y, w ? '#ffffff' : shade(o.gold, 0.95 - u));
    p.glow(A.chest[0], A.chest[1], lerp(12, 24, u), shade(o.gold, 0.8), 0.5 * (1 - u) ** 2);
    p.oval(A.head[0], A.head[1] - lerp(6, 9, u), lerp(4, 7, ease.outQuart(u)), 1.5, w ? '#ffffff' : shade(o.gold, 1 - 0.6 * u), clamp(2 - 2 * u), { width: 1 });
  }
  motes(p, t, o, false);
}
```

## holy/holy-pillar

A beam drops from the top edge onto the anchor, flashes white, widens with falling light streaks, throws three floor rings, then narrows away while feathers sink and motes drift. One-shot.

```js
// id: holy/holy-pillar
import { rng, hash, ease, clamp, lerp, shade, sparkle, fract, TAU } from './pxfx.mjs';

const TOP = -112, HIT = 0.08;   // the beam falls from the top edge and lands at HIT
const OPEN = 0.25, HOLD = 0.9, CLOSE = 0.5, TILT = 0.27;

export const meta = {
  name: 'holy-pillar', size: [96, 132], origin: [48, 116], duration: 2.6, loop: false, fps: 30,
  options: { palette: 'holy', seed: 7 }, tags: ['holy', 'beam', 'pillar', 'light', 'ground'],
  timing: { windup: 0, hits: [{ t: HIT, freeze: 0.06, shake: 0.2, amp: 2 }], recovery: 0.5 },
};

const half = (t) => t < HIT ? 1 : t < HOLD ? lerp(2, 9, ease.outQuart((t - HIT) / OPEN)) : 9 * (1 - ease.inQuad((t - HOLD) / CLOSE));

function rings(p, t, o, side) {
  for (let k = 0; k < 3; k++) {
    const u = clamp((t - HIT - k * 0.16) / 0.8);
    if (u <= 0 || u >= 1) continue;
    const rx = lerp(8, 40, ease.outQuart(u)), a = u < 0.5 ? 1 : 2 - 2 * u;
    p.oval(0, 0, rx, rx * TILT, shade(o.palette, 0.95 - 0.6 * u - (side === 'back' ? 0.2 : 0)), a, { width: u < 0.2 ? 2 : 1, half: side });
  }
}

export function drawBack(p, t, o) {
  rings(p, t, o, 'back');
  const f = t < HIT ? 0 : 1 - clamp((t - HOLD) / 0.9);
  if (f > 0) p.glow(0, 0, 34, shade(o.palette, 0.6), 0.5 * f, 0.28);
}

export function draw(p, t, o) {
  const P = o.palette, hw = half(t), head = t < HIT ? lerp(TOP, 0, t / HIT) : 0, fl = t >= HIT && t < HIT + 0.067;
  if (hw > 0.3) {
    for (let y = TOP + 28; y <= head - 14; y += 12) p.glow(0, y, hw * 2.5 + 4, shade(P, 0.55), 0.13);
    const w = Math.round(fl ? 7 : hw);
    for (let x = -w; x <= w; x++) {
      const edge = Math.abs(x) === w, sp = 1 + hash(x + 40, o.seed), len = 8 + 10 * hash(x + 9, o.seed);
      for (let y = TOP; y <= head; y++) {
        if (fl || edge) { p.px(x, y, fl ? '#ffffff' : shade(P, 0.6)); continue; }
        if (Math.abs(x) <= 1 && w > 2) { p.px(x, y, shade(P, x ? 0.75 : 1)); continue; }
        if (fract((y - t * 260 * sp) / len + hash(x, 3)) < 0.16) p.px(x, y, shade(P, 0.55));
      }
    }
    p.lit(0, head, '#ffffff', 0.8);
    if (t > HIT) { p.oval(0, 0, w + 3, 1.5, shade(P, 0.75), 1, { width: 1 }); p.glow(0, -2, w + 8, shade(P, 0.8), 0.5, 0.4); }
  }
  if (fl) { p.oval(0, 0, 18, 4, '#ffffff'); p.glow(0, -6, 30, shade(P, 0.9), 1.2, 0.5); }
  rings(p, t, o, 'front');
  const r = rng(o.seed);
  for (let i = 0; i < 16; i++) {      // motes rise in the beam, then drift and fade
    const x0 = r.range(-8, 8), d = HIT + r.range(0, 0.9), life = r.range(0.8, 1.4), v = r.range(25, 60), u = (t - d) / life;
    if (u < 0 || u >= 1) continue;
    const x = x0 * (1 + u) + Math.sin(u * 6 + i) * 2, y = -2 - v * life * ease.outQuad(u), c = shade(P, 0.95 - 0.4 * u);
    p.px(x, y, c, clamp(3 - 3 * u)); p.lit(x, y, c, 0.5 * (1 - u));
  }
  for (let i = 0; i < 7; i++) {       // feathers rock and sink
    const x0 = r.range(-30, 30), y0 = r.range(-100, -60), d = HIT + r.range(0.1, 0.7), life = r.range(1.2, 1.7), u = (t - d) / life;
    if (u < 0 || u >= 1) continue;
    const a = 1.2 + Math.sin(u * TAU * 1.5 + i) * 0.6, x = x0 + Math.sin(u * TAU * 1.5 + i) * 5, y = y0 + u * 45;
    const dx = Math.cos(a), dy = Math.sin(a), al = clamp(4 - 4 * u);
    p.line(x - dx * 3, y - dy * 3, x + dx * 3, y + dy * 3, shade(P, 0.95), al);
    for (let k = -2; k <= 2; k++) p.px(x + dx * k + dy, y + dy * k - dx, shade(P, 0.7 - k * 0.05), al);
    if (u < 0.3) sparkle(p, x + dx * 3, y + dy * 3, 0, '#ffffff');
  }
}
```
