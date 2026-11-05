# Nature

Earth, plants and gas: ground-anchored effects with debris, dust and blended clouds.

## nature/poison-cloud

A white-green pop on the floor, then a lit sickly cloud billows out and up from a venting floor, bubbles rise and pop inside it, and it thins away over four seconds. One-shot.

```js
// id: nature/poison-cloud
import { rng, particles, ease, clamp, lerp, shade, band, FAR, NEAR } from './pxfx.mjs';

const FEED = 1.4;    // the floor keeps venting gas until FEED

export const meta = {
  name: 'poison-cloud', size: [112, 80], origin: [56, 70], duration: 4, loop: false, fps: 30,
  options: { palette: 'poison', seed: 3 }, tags: ['nature', 'poison', 'gas', 'cloud', 'ground'],
};

export function create(o) {
  const r = rng(o.seed), ps = particles(r), P = o.palette, bubs = [];
  const gas = (x, vx, vy, life, size, g = -4) => ps.spawn(x, -2, vx, vy, life,
    { kind: 'smoke', ramp: P, size, grow: 2.3, drag: 2.4, gravity: g, alpha: 0.62 });
  for (let i = 0; i < 14; i++) { const s = r.sign(), f = r.next(); gas(s * r.range(0, 5), s * lerp(20, 80, f), -r.range(4, 16), lerp(2.6, 1.8, f), lerp(4.5, 2.5, f)); }
  let t = 0, acc = 0;
  return {
    update(dt) {
      t += dt;
      if (t < FEED) for (acc += dt * 12; acc >= 1; acc--) gas(r.range(-14, 14), r.range(-8, 8), -r.range(10, 26), r.range(1.8, 2.4), r.range(3, 5), -r.range(6, 14));
      if (t < 2.4 && r.chance(dt * 9)) bubs.push({ x: r.range(-18, 18), y: -r.range(4, 14), v: r.range(6, 12), age: 0, life: r.range(0.5, 0.9) });
      for (const b of bubs) { b.age += dt; b.y -= b.v * dt; }
      for (let i = bubs.length; i--;) if (bubs[i].age > bubs[i].life + 0.07) bubs.splice(i, 1);
      ps.update(dt);
    },
    drawBack(p) {
      const u = clamp(t / 0.6);
      if (u < 1) band(p, 0, 0, lerp(8, 36, ease.outQuart(u)), 0.25, 1, 1 - u, P, FAR);
    },
    draw(p) {
      const g = 1 - clamp((t - 0.3) / 3.3);
      if (g > 0) { p.glow(0, -2, 40, shade(P, 0.6), 0.35 * g, 0.22); p.glow(0, -12, 26, shade(P, 0.7), 0.15 * g); }
      const u = clamp(t / 0.6);
      if (u < 1) band(p, 0, 0, lerp(8, 36, ease.outQuart(u)), 0.25, u < 0.25 ? 2 : 1, 1 - u, P, NEAR);
      if (t < 0.05) { p.oval(0, -1, 12, 3, '#ffffff'); p.glow(0, -4, 20, shade(P, 1), 1, 0.45); }
      ps.draw(p);
      for (const b of bubs) {
        const q = b.age / b.life, x = b.x + Math.sin(b.age * 9) * 0.8;
        if (q < 1) { p.px(x, b.y, shade(P, 0.2)); for (const [dx, dy] of [[0, -1], [1, 0], [0, 1], [-1, 0]]) p.px(x + dx, b.y + dy, shade(P, 0.85)); p.lit(x, b.y, shade(P, 0.8), 0.3); }
        else for (const [dx, dy] of [[-2, 0], [2, 0], [0, -2]]) p.px(x + dx, b.y + dy, shade(P, 1));
      }
    },
  };
}
```

## nature/earth-spike

Acted: the caster raises an arm and slams the floor; under the target the floor cracks and a faceted rock spike bursts up with a white flash, rocks and rolling dust, holds, then breaks up and sinks. One-shot.

```js
// id: nature/earth-spike
import { rng, hash, particles, ease, clamp, lerp, shade } from './pxfx.mjs';
import { READY, CAST_UP, CROUCH } from './pxact.mjs';

const HIT = 0.1, LEAD = 0.03, GROW = 0.09, FALL = 0.8, SINK = 0.3, H = 34, C = -40;   // C: caster's feet in the act

export const meta = {
  name: 'earth-spike', size: [96, 80], origin: [48, 70], duration: 2, loop: false, fps: 30,
  options: { palette: 'earth', dust: 'dust', seed: 4 }, tags: ['nature', 'earth', 'spike', 'rock', 'ground'],
};

export const act = {
  poses: [
    [-0.5, { ...READY, x: C }], [-0.25, { ...CAST_UP, x: C }], [-1 / 12, { ...CAST_UP, x: C, lean: -2, hy: -40 }],
    [0, { ...CROUCH, x: C + 2, hx: 12, hy: -2, ghost: true }, 'snap'],   // slam
    [1 / 3, { ...CROUCH, x: C + 2, hx: 11, hy: -3 }], [0.75, { ...READY, x: C }],
  ],
  target: { x: 0 },
  hits: [{ t: HIT, freeze: 0.12, shake: 0.25, amp: 3, push: 4, flash: 2 }],
};

// a faceted rock spike risen h px out of the floor (height hh, half-width w)
function spike(p, P, cx, hh, w, h, white) {
  const M = Math.round;
  for (let y = -Math.floor(h); y <= 0; y++) {
    const j = M(y + h), hw = w * Math.min(1, j / hh), k = hw > 1.5 ? 1.4 : 0, b = j % 6 === 4;
    const x0 = M(cx - hw - hash(j, cx, 1) * k), x1 = M(cx + hw + hash(j, cx, 2) * k), rg = M(cx - hw * 0.25);
    for (let x = x0; x <= x1; x++)
      p.px(x, y, white ? '#fff' : shade(P, x === x0 || x === x1 ? 0 : x === rg ? 0.95 : x < rg ? 0.7 : b ? 0.12 : 0.35));
  }
}

export function create(o) {
  const r = rng(o.seed), ps = particles(r), P = o.palette, D = o.dust, R = (a, b) => r.range(a, b);
  const sp = (x, y, vx, vy, l, kind, size, ramp = P) => ps.spawn(x, y, vx, vy, l, { kind, ramp, size, bounce: 0.3 });
  let t = 0, n = 0;
  return {
    update(dt) {
      t += dt;
      if (!n && t > HIT - 1e-6) {
        n = 1;
        for (let i = 0; i < 10; i++) { const s = r.sign(); sp(s * R(2, 6), -R(2, 14), s * R(12, 40), -R(70, 140), R(0.9, 1.2), 'debris', r.int(2, 3)); }
        for (let i = 0; i < 12; i++) { const s = r.sign(), f = r.next(); sp(s * R(3, 8), -1, s * lerp(25, 75, f), -R(2, 8), lerp(1.2, 0.7, f), 'dust', lerp(4, 2, f), D); }
      }
      if (n === 1 && t >= FALL) {   // it breaks up
        n = 2;
        for (let i = 0; i < 12; i++) sp(R(-4, 4), -R(4, H - 4), R(-25, 25), -R(10, 40), R(0.6, 0.9), 'debris', r.int(1, 3));
        for (let i = 0; i < 6; i++) sp(R(-6, 6), -R(2, 14), R(-20, 20), -R(4, 12), R(0.8, 1.1), 'smoke', R(3, 5), D);
      }
      ps.update(dt);
    },
    drawBack(p) {
      const f = 1 - clamp((t - 1.1) / 0.7), u = ease.outExpo(t / HIT);
      if (t < 0.02 || f <= 0) return;
      for (const [a, l] of [[-2.9, 13], [-0.3, 11], [3, 12], [0.4, 9]]) p.line(0, 0, Math.cos(a) * l * u, Math.sin(a) * l * u * 0.3, shade(P, 0), f);
      p.oval(0, 0, 9, 2, shade(P, 0.1), f);
    },
    draw(p) {
      const g = clamp((t - HIT + LEAD) / GROW), on = t > HIT - 1e-6, fl = on && t < HIT + 0.05;
      const sk = ease.inQuad((t - FALL) / SINK), q = clamp((t - HIT) / 0.35), h = (k) => H * k * ease.outBack(g) * (1 - sk);
      if (t >= HIT - LEAD && sk < 1) {
        p.glow(0, 0, 22, shade(P, 0.8), 0.25 * (1 - sk), 0.3);
        spike(p, P, -9, 16, 5, h(0.47), fl); spike(p, P, 8, 13, 4, h(0.38), fl); spike(p, P, 1, H, 7, h(1), fl);
      }
      if (on && q < 1) p.glow(0, -16, 22, shade(P, 1), 1.2 * (1 - q) ** 2);
      if (fl) { p.oval(0, 0, 16, 3, '#fff'); p.glow(0, -4, 26, shade(P, 1), 1, 0.45); }
      ps.draw(p);
    },
  };
}
```
