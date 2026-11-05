# Lightning

Jagged bolts by midpoint displacement, re-rolled every few frames, with white strike frames, forks, crackling arcs and sparks.

## lightning/lightning-strike

A leader feels down from the top, a white forked bolt strikes and restrikes, cools and dithers out, leaving a scorch and crackling sparks.

```js
// id: lightning/lightning-strike
import { rng, particles, bolt, forks, burst, clamp, shade } from './pxfx.mjs';

const HIT = 0.066, RE = 0.24, TOP = -119;   // leader before HIT, a restrike at RE
export const meta = {
  name: 'lightning-strike', size: [96, 132], origin: [48, 120], duration: 1.4, loop: false, fps: 30,
  options: { palette: 'electric', scorch: 'smoke', seed: 9 }, tags: ['lightning', 'electric', 'strike', 'ground'],
  timing: { windup: HIT, hits: [{ t: HIT, freeze: 0.08, shake: 0.3, amp: 3 }], recovery: 0.5 },
};

export function create(o) {
  const r = rng(o.seed), ps = particles(r), C = (u) => shade(o.palette, u);
  const shape = (k) => { const q = rng(o.seed * 1000 + k), pts = bolt(q, q.range(-14, 14), TOP, 0, 0, 0.22, 6); return [pts, forks(q, pts, 3, 0.35).map((f) => f.filter(([x, y]) => y > TOP + 2 && y < 1 && Math.abs(x) < 44))]; };
  const S = [shape(0), shape(1)];
  const spray = (n) => { for (const [vx, vy] of burst(n, r, 40, 110, -Math.PI * 0.9, -Math.PI * 0.1)) ps.spawn(0, -1, vx, vy, r.range(0.3, 0.6), { kind: 'spark', ramp: o.palette, bounce: 0.5 }); };
  let t = 0, n = 0;
  return {
    update(dt) {
      t += dt; ps.update(dt);
      if (n === 0 && t >= HIT - 1e-6) { n = 1; spray(14); }
      if (n === 1 && t >= RE - 1e-6) { n = 2; spray(6); }
    },
    drawBack(p) {
      if (t < HIT - 1e-6) return;
      const s = 1 - clamp((t - 0.5) / 0.85), rx = 12 * s;
      if (rx < 1) return;
      p.oval(0, 0, rx, rx * 0.3, shade(o.scorch, 0));
      p.oval(0, 0, rx * 0.6, rx * 0.18, shade(o.scorch, 0.15));
      p.glow(0, 0, 30, C(0.6), 0.5 * (1 - clamp((t - HIT) / 0.6)), 0.3);
    },
    draw(p) {
      const [pts, fk] = S[t < RE - 0.04 ? 0 : 1];
      if (t < HIT - 1e-6) {                              // the leader feels its way down
        const k = Math.max(2, Math.ceil((pts.length * t) / HIT));
        p.polyline(pts.slice(0, k), C(0.7), 0.8); if (k > 9) p.glow(...pts[k - 1], 6, C(0.8), 0.4);
      } else {
        const hot = t < HIT + 0.05 || (t >= RE - 1e-6 && t < RE + 0.05), a = hot ? 1 : 1 - clamp((t - HIT - 0.07) / (t > RE ? 0.3 : 0.5));
        if (a > 0) {
          for (const f of fk) p.polyline(f, C(hot ? 0.85 : 0.6), a);
          p.polyline(pts.map(([x, y]) => [x - 1, y]), C(hot ? 0.9 : 0.5), a);
          p.polyline(pts, hot ? '#ffffff' : C(0.85), a);
          for (let i = 6; i < pts.length; i += 8) p.glow(...pts[i], hot ? 14 : 8, C(0.7), (hot ? 0.35 : 0.12) * a);
        }
        if (hot) { p.glow(0, -55, 40, C(0.8), 0.35, 1.4); p.glow(0, -2, 14, '#ffffff', 0.9); p.oval(0, 0, 9, 2, '#ffffff'); }
        const c = 1 - clamp((t - HIT) / 0.9);            // crackle at the base, re-rolled every 2 frames
        if (c > 0) {
          const q = rng(o.seed * 77 + Math.floor(t * 15));
          for (let i = 0; i < 3 * c + 0.5; i++) {
            const x0 = q.range(-12, 12), arc = bolt(q, x0, 0, x0 + q.range(-9, 9), -q.range(4, 12), 0.3, 3);
            p.polyline(arc, C(q.chance(0.5) ? 1 : 0.75)); p.lit(...arc.at(-1), C(1), 0.6);
          }
        }
      }
      ps.draw(p);
    },
  };
}
```

## lightning/zap-hit

Acted: a palm thrust at the target; a white contact frame, a ring, arcs crawling over the body and grounding to the floor, sparks.

```js
// id: lightning/zap-hit
import { rng, particles, lights, bolt, burst, sparkle, ease, clamp, lerp, shade } from './pxfx.mjs';
import { READY, CAST_FORWARD } from './pxact.mjs';

const HIT = 0, CRACK = 0.36, HY = 22;   // contact, crackle time, hit height above the target's feet
export const meta = {
  name: 'zap-hit', size: [72, 60], origin: [36, 50], duration: 0.6, loop: false, fps: 30,
  options: { palette: 'volt', arc: 'electric', seed: 4 }, tags: ['lightning', 'electric', 'hit', 'impact'],
};
const X = -26;   // in the act the caster stands left of the target at the anchor
export const act = {
  poses: [
    [-0.5, { ...READY, x: X }],
    [-0.25, { ...READY, x: X - 1, crouch: 3, lean: -2, hx: 0, hy: -22, bx: -4, by: -18 }],
    [-1 / 12, { ...READY, x: X - 2, crouch: 4, lean: -3, hx: -3, hy: -23, bx: -6, by: -18 }],
    [0, { ...CAST_FORWARD, x: X + 1, ghost: true }, 'snap'],
    [0.25, { ...CAST_FORWARD, x: X + 2, crouch: 3, hx: 18, hy: -22 }, 'out'],
    [7 / 12, { ...READY, x: X }],
  ],
  target: { x: 0 },
  hits: [{ t: HIT, freeze: 0.08, shake: 0.15, amp: 2, push: 4, flash: 2 }],
};

export function create(o) {
  const r = rng(o.seed), ps = particles(r), ls = lights(), C = (u) => shade(o.palette, u);
  for (const [vx, vy] of burst(12, r, 50, 130, -Math.PI * 0.95, Math.PI * 0.15))
    ps.spawn(-4, -HY, vx, vy, r.range(0.25, 0.5), { kind: 'spark', ramp: o.palette, bounce: 0.5 });
  ls.add(0, -HY, 18, C(0.8), 0.8, 0.3);
  let t = 0;
  return {
    update(dt) { t += dt; ps.update(dt); ls.update(dt); },
    drawBack(p) { p.glow(0, 0, 18, C(0.7), 0.5 * (1 - clamp(t / 0.4)), 0.3); },
    draw(p) {
      ls.draw(p);
      const u = clamp((t - HIT) / CRACK);
      if (u < 1) {                                           // arcs crawl over the body, re-rolled every 2 frames
        const q = rng(o.seed * 31 + Math.floor(t * 15));
        for (let i = 0; i < Math.ceil(4 * (1 - u)); i++) {
          const a = q.range(0, Math.PI * 2), d = q.range(12, 18), x1 = i ? Math.cos(a) * d * 0.7 : q.range(-6, 6), y1 = i ? -HY + Math.sin(a) * d * 1.2 : 0;
          const pts = bolt(q, q.range(-2, 2), -HY + q.range(-3, 3), x1, Math.min(0, y1), 0.25, 4);
          p.polyline(pts, C(0.8)); p.polyline(pts.slice(0, 9), u < 0.3 ? '#ffffff' : C(1));
          p.lit(...pts.at(-1), C(1), 0.7);
          if (q.chance(0.4)) sparkle(p, Math.round(pts.at(-1)[0]), Math.round(pts.at(-1)[1]), 1, C(1), shade(o.arc, 0.7));
        }
      }
      if (t < HIT + 0.04) {                                   // the white contact frame
        p.disc(-3, -HY, 4, '#ffffff'); sparkle(p, -3, -HY, 9, '#ffffff', C(0.8)); p.glow(-3, -HY, 16, C(1), 0.6);
      } else if (t < HIT + 0.12) {
        const k = ease.outQuart((t - HIT) / 0.2);
        p.ring(-3, -HY, lerp(4, 13, k), 1, C(1 - k * 0.35));
      }
      ps.draw(p);
    },
  };
}
```
