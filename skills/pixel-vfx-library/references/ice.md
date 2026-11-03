# Ice

Faceted crystals, translucent blocks, tumbling shards and cold mist with pale floor light.

## ice/frost-nova

Acted: the caster rises and stamps; ice spikes erupt ring by ring along the floor with a white flash, shards spin off, frost lines stay, then it melts.

```js
// id: ice/frost-nova
import { rng, particles, crystal, ease, clamp, lerp, shade, TAU } from './pxfx.mjs';
import { READY, CROUCH } from './pxact.mjs';

const SPEED = 170, RMAX = 40, SQ = 0.26, MELT = 1.1, TX = 34;   // spikes race out, hold, melt from MELT
export const meta = {
  name: 'frost-nova', size: [128, 62], origin: [64, 46], duration: 2.2, loop: false, fps: 30,
  options: { palette: 'ice', seed: 5 }, tags: ['ice', 'nova', 'ground', 'spell', 'aoe'],
};
export const act = {
  poses: [
    [-0.5, READY],
    [-0.25, { lean: -1, crouch: -1, hx: 3, hy: -34, bx: -4, by: -32 }],
    [-1 / 12, { lean: -2, crouch: -1, hx: 2, hy: -37, bx: -5, by: -35 }],
    [0, { ...CROUCH, crouch: 8, hx: 9, hy: -5, bx: -5, by: -5, ghost: true }, 'snap'],
    [0.25, { ...CROUCH, crouch: 9, hx: 10, hy: -4, bx: -6, by: -4 }, 'out'],
    [0.75, READY],
  ],
  target: { x: TX },
  hits: [{ t: 0, freeze: 0.08, shake: 0.25, amp: 2 }, { t: TX / SPEED, freeze: 0.05, shake: 0.1, amp: 1, push: 3, flash: 1, tint: '#9cd8ff' }],
};

export function create(o) {
  const r = rng(o.seed), ps = particles(r), C = (u) => shade(o.palette, u), spikes = [];
  for (let R = 16, n = 6; R <= RMAX; R += 12, n += 3) for (let i = 0, a0 = r.range(0, 1); i < n; i++) {
    const a = a0 + ((i + r.range(-0.25, 0.25)) / n) * TAU;
    const x = Math.cos(a) * R, y = Math.sin(a) * R * SQ;
    spikes.push({ x, y, t0: R / SPEED, h: lerp(15, 7, R / RMAX) * r.range(0.7, 1), w: r.range(4, 6), lean: Math.cos(a) * 2.5 });
    if (r.chance(0.4)) ps.spawn(x, y - 3, Math.cos(a) * r.range(15, 40), -r.range(60, 110), r.range(0.5, 0.8), { kind: 'debris', ramp: o.palette, size: 2, delay: R / SPEED, back: y < 0 });
    if (r.chance(0.25)) ps.spawn(x, y, Math.cos(a) * 6, -r.range(4, 10), r.range(1, 1.4), { kind: 'vapour', ramp: o.palette, size: 3, alpha: 0.2, haze: 0.02, delay: R / SPEED + 0.1 });
  }
  spikes.sort((a, b) => a.y - b.y);
  const ferns = Array.from({ length: 9 }, (_, i) => [(i / 9) * TAU + r.range(-0.2, 0.2), r.range(20, RMAX)]);
  let t = 0;
  const floor = (p, back) => {
    const m = 1 - clamp((t - MELT - 0.3) / 0.7), u = clamp(t / 0.3);
    if (u < 1) p.oval(0, 0, lerp(6, RMAX + 6, ease.outQuart(u)), lerp(6, RMAX + 6, ease.outQuart(u)) * SQ, C(1 - u * 0.5 - (back ? 0.2 : 0)), 1, { half: back ? 'back' : 'front', width: u < 0.4 ? 2 : 1 });
    if (m <= 0) return;
    for (const [a, L] of ferns) if ((Math.sin(a) < 0) === back) {
      const k = Math.min(1, (t * SPEED) / L) * L * m;
      if (k > 9) p.line(Math.cos(a) * 8, Math.sin(a) * 8 * SQ, Math.cos(a) * k, Math.sin(a) * k * SQ, C(back ? 0.55 : 0.7));
    }
    p.glow(0, 0, RMAX + 6, C(0.7), 0.28 * m * Math.min(1, t * 5), SQ);
  };
  const draw = (p, back) => {
    for (const s of spikes) if ((s.y < 0) === back) {
      const u = (t - s.t0) / 0.12, melt = clamp((t - MELT - s.t0 * 2) / 0.4);
      if (u <= 0 || melt >= 1) continue;
      const h = Math.round(s.h * ease.outBack(Math.min(1, u)) * (1 - melt));
      if (h >= 1) crystal(p, s.x, s.y, h, s.w, s.lean, back ? [C(0), C(0.2), C(0.5), C(0.8)] : o.palette);
    }
  };
  return {
    update(dt) { t += dt; ps.update(dt); },
    drawBack(p) { floor(p, true); draw(p, true); ps.drawBack(p); },
    draw(p) {
      floor(p, false); draw(p, false);
      if (t < 0.05) { p.oval(0, 0, 16, 4, '#ffffff'); p.glow(0, -6, 30, C(1), 0.9, 0.6); }
      ps.draw(p);
    },
  };
}
```

## ice/ice-shatter

A translucent ice block cracks with a white line, then bursts into tumbling, bouncing shards and a puff of cold mist (options w, h).

```js
// id: ice/ice-shatter
import { rng, particles, ease, clamp, shade, sparkle } from './pxfx.mjs';

const CRACK = 0.25, BREAK = 0.5;   // crack, then burst
export const meta = {
  name: 'ice-shatter', size: [120, 76], origin: [60, 62], duration: 2, loop: false, fps: 30,
  options: { palette: 'ice', w: 18, h: 24, seed: 3 }, tags: ['ice', 'shatter', 'break', 'ground'],
  timing: { windup: 0, hits: [{ t: BREAK, freeze: 0.06, shake: 0.15, amp: 2 }], recovery: 0.3 },
};

export function create(o) {
  const r = rng(o.seed), ps = particles(r), C = (u) => shade(o.palette, u), W = o.w, H = o.h, x0 = -W / 2;
  const crack = [[x0, -H * 0.72]];
  for (let i = 1; i <= 5; i++) crack.push([x0 + (W * i) / 5, -H * (0.72 - i * 0.08) + (i % 2 ? 3 : -2)]);
  const shards = Array.from({ length: Math.round((W * H) / 45) }, () => {
    const x = r.range(x0 + 2, -x0 - 2), y = -r.range(2, H - 2), s = r.range(2, 5), a = r.range(0, 6);
    return { x, y, vx: Math.sign(x) * r.range(18, 55) + x * 2, vy: -r.range(70, 150), s, a, va: r.range(-14, 14), life: r.range(0.9, 1.4) };
  });
  let t = 0, broke = false;
  return {
    update(dt) {
      t += dt; ps.update(dt);
      if (!broke && t >= BREAK) {
        broke = true;
        for (let i = 0; i < 4; i++) ps.spawn(r.range(x0, -x0), -r.range(2, H), r.range(-25, 25), -r.range(5, 20), r.range(0.9, 1.3), { kind: 'vapour', ramp: [C(0.6), C(0.8), C(1)], size: r.range(2, 3.5), alpha: 0.16, haze: 0.06 });
        for (let i = 0; i < 12; i++) ps.spawn(r.range(x0, -x0), -r.range(2, H), r.range(-90, 90), -r.range(40, 140), r.range(0.4, 0.8), { kind: 'grit', ramp: o.palette, shade: 0.9 });
      }
      if (broke) for (const q of shards) {
        q.vy += 380 * dt; q.x += q.vx * dt; q.y += q.vy * dt; q.a += q.va * dt;
        if (q.y > 0) { q.y = 0; q.vy *= -0.3; q.vx *= 0.4; q.va *= 0.4; }
      }
    },
    drawBack(p) { p.glow(0, 0, W + 10, C(0.7), 0.3 * (1 - clamp((t - BREAK) / 1.2)), 0.3); },
    draw(p) {
      const fl = (t >= CRACK && t < CRACK + 0.04) || t >= BREAK - 0.04;
      if (t < BREAK) {
        for (let y = -H + 1; y < 0; y++) for (let x = Math.ceil(x0) + 1; x < -x0; x++)
          p.puff(x, y, 0.4, C(y < -H + 4 ? 0.8 : x > -x0 * 0.4 ? 0.3 : 0.5), y < -H + 4 ? 0.75 : 0.5);
        const e = (u) => (fl ? '#fff' : C(u));
        p.line(x0, 0, x0, -H, e(0.95)); p.line(x0, -H, -x0, -H, e(1)); p.line(-x0, -H, -x0, 0, e(0.55)); p.line(x0, 0, -x0, 0, C(0.35));
        p.line(x0 + 3, -H + 6, x0 + 7, -H + 2, C(1)); p.line(x0 + 3, -H + 10, x0 + 5, -H + 8, C(0.9));
        if (t >= CRACK) {
          const k = ease.outQuart((t - CRACK) / 0.1) * 5, pts = crack.slice(0, Math.floor(k) + 1);
          p.polyline(pts, '#fff'); for (const [x, y] of pts) p.lit(x, y, C(1), 0.5);
        }
        p.glow(0, -H / 2, W, C(0.8), 0.15 + (t > CRACK ? 0.25 : 0));
      } else for (const q of shards) {
        const u = clamp((t - BREAK) / q.life), s = q.s * (1 - ease.inQuad(u));
        if (s < 0.6) continue;
        const pt = (k) => [q.x + Math.cos(q.a + k) * s, q.y - 1 + Math.sin(q.a + k) * s * 0.8];
        p.tri(...pt(0), ...pt(2.3), ...pt(4), C(0.55)); p.line(...pt(0), ...pt(2.3), C(0.95));
        if (u < 0.3) p.lit(...pt(0), C(1), 0.5);
      }
      if (t >= BREAK && t < BREAK + 0.07) { p.rect(x0, -H, W + 1, 2, '#fff'); p.glow(0, -H / 2, W * 1.4, C(1), 0.7); sparkle(p, 0, -H / 2, 7, '#fff', C(0.8)); }
      ps.draw(p);
    },
  };
}
```
