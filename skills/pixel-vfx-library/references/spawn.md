# Spawn

Appear and vanish effects on the body (`meta.actor: true`): they work on `o.actor` and set its opacity with `actorAlpha` (and redraw it with `actorPaint`), so they fit any sprite.

## spawn/poof

On-body vanish: the body flashes white and is gone, a round cloud of white smoke puffs bursts out, stops hard, rises a little and thins, with a popping ring and gold stars that twinkle out.

```js
// id: spawn/poof
import { rng, particles, burst, ease, clamp, lerp, shade } from './pxfx.mjs';

const GONE = 0.04, RING = 0.22;   // the body is gone after the white frame; the ring pops for RING

export const meta = {
  name: 'poof', size: [72, 72], origin: [36, 60], duration: 1.05, loop: false, fps: 30, actor: true,
  options: { palette: 'gold', smoke: ['#7c7e98', '#b0b2c6', '#d8dae6', '#f0f1f7', '#ffffff'], seed: 8 },
  tags: ['spawn', 'vanish', 'smoke', 'puff'],
};

export const actorAlpha = (t) => (t < GONE ? 1 : 0);
export function actorPaint(p, t, o) { if (t < GONE) p.body(o.actor, () => '#ffffff'); }

export function create(o) {
  const r = rng(o.seed), ps = particles(r);
  // a round cloud: puffs thrown out on all sides stop hard, swell, rise a little and thin
  for (const [vx, vy] of burst(16, r, 35, 85)) ps.spawn(vx * 0.05, vy * 0.05, vx, vy * 0.85, r.range(0.55, 0.9),
    { kind: 'smoke', ramp: o.smoke, size: r.range(3.5, 5), grow: 1.3, drag: 7, gravity: -10, alpha: 0.9, floor: null });
  for (let i = 0; i < 5; i++) ps.spawn(r.range(-4, 4), r.range(-4, 4), r.range(-10, 10), r.range(-10, 5), r.range(0.7, 0.95),
    { kind: 'smoke', ramp: o.smoke, size: r.range(5, 6.5), grow: 1.2, drag: 4, gravity: -8, alpha: 0.95, floor: null });
  for (const [vx, vy] of burst(9, r, 90, 160)) ps.spawn(0, 0, vx, vy, r.range(0.5, 0.9), { kind: 'star', ramp: o.palette, size: r.int(2, 3), drag: 4.5, delay: 0.03 });
  let t = 0;
  return {
    update(dt) { t += dt; ps.update(dt); },
    draw(p) {
      const [cx, cy] = o.actor ? o.actor.chest : [0, -20], q = p.sub(cx, cy), u = clamp(t / RING);
      if (t < 0.4) q.glow(0, 0, 26, shade(o.palette, 0.8), 0.4 * (1 - t / 0.4) ** 2);
      if (t < GONE) { q.disc(0, 0, 9, '#ffffff'); return; }
      ps.draw(q);
      if (u < 1) q.ring(0, 0, lerp(8, 24, ease.outQuart(u)), u < 0.4 ? 2 : 1, u < 0.3 ? '#ffffff' : shade(o.smoke, 0.8), 1 - u);
    },
  };
}
```

## spawn/dissolve-burn

On-body burn-away: a ragged, flickering edge climbs from the feet to the head; above it the body chars and glows white-hot, below it nothing remains; embers rise and cool from the edge, thin smoke curls up, the light follows the edge.

```js
// id: spawn/dissolve-burn
import { rng, hash, ease, clamp, lerp, shade } from './pxfx.mjs';

const B0 = 0.1, B1 = 1.3;   // the burn edge climbs from the feet (B0) past the head (B1)

export const meta = {
  name: 'dissolve-burn', size: [64, 84], origin: [32, 74], duration: 2, loop: false, fps: 30, actor: true,
  options: { palette: 'fire', smoke: 'smoke', seed: 11 }, tags: ['spawn', 'death', 'burn', 'dissolve', 'fire'],
};

const top = (A) => A.head[1] - 9;
const level = (t, A) => lerp(2, top(A), clamp((t - B0) / (B1 - B0)) ** 1.3);
// the edge height in column x: a ragged, flickering line
const edge = (t, A, x, s) => level(t, A) + (hash(x, 0, s) - 0.5) * 4 + Math.sin(x * 0.9 + t * 14) * 1.2;
export const actorAlpha = (t) => 1 - clamp((t - B0) / (B1 - B0));

// Above the edge the body chars, then glows; below it nothing is left.
export function actorPaint(p, t, o) {
  const A = o.actor;
  if (t < B1) p.body(A, (c, x, y) => {
    const d = edge(t, A, x, o.seed) - y;
    if (d < 0) return null;
    if (d < 1.5) { p.lit(x, y, shade(o.palette, 0.9), 0.6); return shade(o.palette, 1); }
    if (d < 3) return shade(o.palette, 0.7);
    if (d < 5) return shade(o.palette, 0.1);
    return d < 8 ? c.map((v) => v * 0.55) : c;
  });
}

export function draw(p, t, o) {
  const A = o.actor, cx = A.chest[0], L = level(t, A), k = 1 - clamp((t - B0) / (B1 - B0));
  if (t > B0 && k > 0) {
    p.glow(cx, L, 14, shade(o.palette, 0.7), 0.3 + 0.4 * k, 0.4);
    p.glow(0, 0, 20, shade(o.palette, 0.5), 0.45 * k, 0.3);
  }
  for (let i = 0; i < 46; i++) {   // embers born on the edge, rising, wandering and cooling
    const r = rng(o.seed * 97 + i), tb = lerp(B0, B1 - 0.1, i / 46) + r.range(0, 0.05), age = (t - tb) / r.range(0.45, 0.75);
    if (age < 0 || age >= 1) continue;
    const y0 = level(tb, A), x = cx + r.range(-7, 7) + r.range(-8, 8) * age + Math.sin(age * 7 + i) * 1.5, y = y0 - ease.outQuad(age) * r.range(16, 34);
    const c = shade(o.palette, 1 - age * 0.85);
    p.px(x, y, c, age < 0.7 ? 1 : (1 - age) / 0.3);
    p.lit(x, y, c, 0.6 * (1 - age));
  }
  for (let i = 0; i < 12; i++) {   // thin smoke that curls up from the edge
    const r = rng(o.seed * 31 + i), tb = lerp(B0 + 0.1, B1, i / 12), age = (t - tb) / 0.7;
    if (age < 0 || age >= 1) continue;
    p.puff(cx + r.range(-5, 5) + age * r.range(-5, 5), level(tb, A) - 3 - age * 20, 2 + age * 4, shade(o.smoke, 0.3 + 0.5 * (1 - age)), 0.65 * (1 - age));
  }
}
```
