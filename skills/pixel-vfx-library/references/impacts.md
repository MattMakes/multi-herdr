# Impacts

Hit bursts for any blow. Local (0, 0) is the floor under the target and the burst sits `h` px up (set `h: 0` to centre it on the anchor); each has an act with a stand-in attacker so you can judge the hit.

## impacts/hit-spark

Light hit: one white star frame, then a shrinking four-point star with dashes that fly off the rays, a few sparks that fall and bounce; acted as a quick jab.

```js
// id: impacts/hit-spark
import { rng, particles, burst, ease, clamp, lerp, shade, star, TAU } from './pxfx.mjs';
import { READY, STRIKE_FORWARD } from './pxact.mjs';

const HIT = 0, POP = 0.18, FLASH = 0.02;   // rays live for POP; only frame 0 is the white core

export const meta = {
  name: 'hit-spark', size: [56, 60], origin: [28, 50], duration: 0.45, loop: false, fps: 30,
  options: { palette: 'holy', seed: 3, h: 22 }, tags: ['impact', 'hit', 'spark', 'light'],
  timing: { windup: 0.17, hits: [{ t: HIT, freeze: 0.04, shake: 0.08, amp: 1 }], recovery: 0.25 },
};

// A light jab: the fist snaps into the dummy; local (0, 0) is the floor under the hit, the spark sits h px up.
export const act = {
  poses: [[-1 / 3, { ...READY, x: -20 }], [-1 / 6, { ...READY, x: -21, crouch: 3, hx: -2, hy: -20, lean: -2 }],
    [0, { ...STRIKE_FORWARD, x: -19, hx: 16, hy: -22, ghost: true }, 'snap'], [1 / 6, { ...STRIKE_FORWARD, x: -19, hx: 14 }, 'out'], [5 / 12, { ...READY, x: -20 }]],
  target: { x: 5 },
  hits: [{ t: HIT, freeze: 0.04, shake: 0.08, amp: 1, push: 3, flash: 1 }],
};

export function create(o) {
  const r = rng(o.seed), ps = particles(r), y0 = -o.h, rot = r.range(-0.3, 0.3);
  for (const [vx, vy] of burst(7, r, 60, 130, -2.9, 0.4)) ps.spawn(0, y0, vx, vy, r.range(0.15, 0.32), { kind: 'spark', ramp: o.palette, bounce: 0.4 });
  const rays = Array.from({ length: 8 }, (_, i) => ({ a: rot + (i * TAU) / 8, L: (i % 2 ? 5 : 10) * r.range(0.85, 1.15) }));
  let t = 0;
  return {
    update(dt) { t += dt; ps.update(dt); },
    draw(p) {
      const u = clamp(t / POP);
      if (t < 0.25) p.glow(0, y0, 14, shade(o.palette, 0.7), 0.8 * (1 - t / 0.25) ** 2);
      const st = (r1, r2, c) => p.poly(star(4, r1, r2, rot).map(([x, y]) => [x, y + y0]), c);
      if (t < FLASH) {
        st(12, 2.5, '#ffffff');
        for (const q of rays) p.line(0, y0, Math.cos(q.a) * q.L, y0 + Math.sin(q.a) * q.L, '#ffffff');
        p.glow(0, y0, 7, '#ffffff', 1);
      } else if (u < 1) {
        const k = (1 - u) ** 2;
        if (k > 0.15) { st(11 * k, 2.2 * k + 0.6, shade(o.palette, 0.55)); st(6 * k, 1.2, shade(o.palette, 0.8)); p.px(0, y0, '#ffffff'); }
        for (const q of rays) {
          const r1 = q.L * lerp(1, 1.6, ease.outExpo(u)), r0 = q.L * lerp(0.55, 1.5, ease.outQuart(u)), c = Math.cos(q.a), s = Math.sin(q.a);
          p.line(c * r0, y0 + s * r0, c * r1, y0 + s * r1, shade(o.palette, 0.75 - u * 0.4), u < 0.5 ? 1 : 1 - (u - 0.5) / 0.5);
          p.lit(c * r1, y0 + s * r1, shade(o.palette, 1), 0.6 * (1 - u));
        }
      }
      ps.draw(p);
    },
  };
}
```

## impacts/crit-hit

Heavy hit: two full white frames (a jagged star and long rays), then a red-orange star that collapses slowly inside a fading ring, radiating speed lines, big bouncing sparks and a smoky afterglow; acted as a sword blow with a 0.2 s freeze.

```js
// id: impacts/crit-hit
import { rng, particles, burst, ease, clamp, lerp, shade, star, TAU } from './pxfx.mjs';
import { READY, WINDUP_BACK, STRIKE_FORWARD } from './pxact.mjs';

const HIT = 0, WHITE = 0.05, LINES = 0.3, FLARE = 0.55;   // 2 white frames, speed lines, red flare

export const meta = {
  name: 'crit-hit', size: [112, 92], origin: [56, 74], duration: 0.9, loop: false, fps: 30,
  options: { palette: 'fire', seed: 5, h: 22 }, tags: ['impact', 'hit', 'heavy', 'critical'],
};

// A heavy sword blow; local (0, 0) is the floor under the target, the burst sits h px up.
const S = { weapon: 'sword', x: -24 };
export const act = {
  poses: [[-7 / 12, { ...READY, ...S }], [-1 / 3, { ...WINDUP_BACK, ...S, x: -26 }, 'line'], [-1 / 12, { ...WINDUP_BACK, ...S, x: -26, crouch: 5, wa: -3 }],
    [0, { ...STRIKE_FORWARD, ...S, x: -20, ghost: true }, 'snap'], [0.25, { ...STRIKE_FORWARD, ...S, x: -19, crouch: 6, hx: 15, hy: -11, wa: 1.1 }, 'out'], [0.75, { ...READY, ...S }]],
  target: { x: 4 },
  hits: [{ t: HIT, freeze: 0.2, shake: 0.35, amp: 3, push: 8, flash: 2 }],
};

export function create(o) {
  const r = rng(o.seed), ps = particles(r), y0 = -o.h, rot = r.range(0, 1);
  for (const [vx, vy] of burst(18, r, 70, 170, 0, TAU)) ps.spawn(0, y0, vx * 1.2, vy - 30, r.range(0.2, 0.45), { kind: 'spark', ramp: o.palette, bounce: 0.4, size: 2, drag: 2.2 });
  for (let i = 0; i < 5; i++) ps.spawn(r.range(-4, 4), y0 + r.range(-3, 3), r.range(-40, 40), r.range(-30, 0), r.range(0.5, 0.75), { kind: 'smoke', size: r.range(2, 3.5), delay: 0.08 + i * 0.03 });
  const lines = Array.from({ length: 14 }, (_, i) => ({ a: (i / 14) * TAU + r.range(-0.15, 0.15), L: r.range(10, 18), d: r.range(0, 0.04) }));
  let t = 0;
  const burstStar = (k, c, a = 1) => p => p.poly(star(8, 18 * k, 6 * k, rot).map(([x, y]) => [x, y + y0]), c, a);
  return {
    update(dt) { t += dt; ps.update(dt); },
    draw(p) {
      const f = clamp(t / FLARE);
      if (f < 1) {
        p.glow(0, y0, 44, shade(o.palette, 0.45), 0.9 * (1 - f) ** 2, 0.8);
        p.glow(0, y0, 12, shade(o.palette, 0.9), 1.2 * (1 - f) ** 3);
        p.glow(0, 0, 30, shade(o.palette, 0.4), 0.5 * (1 - f), 0.25);
      }
      ps.draw(p);
      if (t < WHITE) {
        burstStar(1.1, '#ffffff')(p);
        for (let i = 0; i < 4; i++) { const a = rot + (i * TAU) / 4 + TAU / 16; p.line(0, y0, Math.cos(a) * 32, y0 + Math.sin(a) * 32, '#ffffff'); }
        return;
      }
      const u = clamp((t - WHITE) / 0.25);
      if (u < 1) {
        const k = 1 - u * u;
        burstStar(k * 1.1, shade(o.palette, 0.3))(p); burstStar(k * 0.8, shade(o.palette, 0.65))(p); burstStar(k * 0.45, shade(o.palette, 1))(p);
        if (u < 0.6) p.ring(0, y0, lerp(10, 26, ease.outQuart(u)), 1, shade(o.palette, 0.7 - u * 0.6), 1 - u / 0.6);
      }
      for (const q of lines) {
        const v = clamp((t - WHITE - q.d) / LINES);
        if (v <= 0 || v >= 1) continue;
        const c = Math.cos(q.a), s = Math.sin(q.a), m = s > 0 ? (o.h - 1) / s : 99, r1 = Math.min(m, lerp(14, 40, ease.outExpo(v))), r0 = lerp(10, 38, ease.outQuart(v));
        if (r0 >= r1) continue;
        p.line(c * r0, y0 + s * r0, c * r1, y0 + s * r1, v < 0.25 ? '#ffffff' : shade(o.palette, 0.9 - v * 0.6), 1 - v * v);
      }
    },
  };
}
```
