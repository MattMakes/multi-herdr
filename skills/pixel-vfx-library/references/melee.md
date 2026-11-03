# Melee

Weapon strokes with acts: the stand-in character winds up, releases in one snapped drawing and freezes on contact. Local (0, 0) is the attacker's feet; the target stands in front.

## melee/sword-slash

Acted sword stroke: a wide elliptical crescent sweeps from over the back shoulder (passing behind the head) to the front, a white leading edge and a trail that shortens and thins; the dummy is hit at the end of the arc with sparks and a flare. Option `arc` [cx, cy, rx, ry, a0, a1, tilt] reshapes it; a negative ry gives a rising backhand, rx = ry an overhead.

```js
// id: melee/sword-slash
import { rng, particles, lights, burst, ease, clamp, lerp, shade, sparkle } from './pxfx.mjs';
import { READY, WINDUP_BACK, STRIKE_FORWARD } from './pxact.mjs';

export const SWING = 0.1, HIT = SWING;   // the edge bites at the end of the arc
const FADE = 0.18, S = { weapon: 'sword' };

export const meta = {
  name: 'sword-slash', size: [104, 64], origin: [36, 54], duration: 0.6, loop: false, fps: 30,
  // arc: [cx, cy, rx, ry, a0, a1, tilt]; a negative ry mirrors the stroke (an upward backhand)
  options: { palette: 'metal', spark: 'holy', seed: 2, arc: [6, -22, 25, 11, -2.6, 0.2, 0.12] },
  tags: ['melee', 'slash', 'sword'],
};

const SF = { ...STRIKE_FORWARD, ...S }, WB = { ...WINDUP_BACK, ...S, hy: -22 };
export const act = {
  poses: [[-0.5, { ...READY, ...S }], [-0.25, { ...WB, wa: -2.9 }], [-1 / 12, { ...WB, crouch: 4, wa: -3.1 }],
    [0, { ...SF, x: 1, hx: 12, hy: -27, wa: -1.2, ghost: true }, 'snap'], [1 / 12, { ...SF, hy: -21, wa: 0.1 }],
    [0.25, { ...SF, x: 5, crouch: 5, hx: 14, hy: -14, wa: 0.8 }, 'out'], [7 / 12, { ...READY, ...S }]],
  target: { x: 27 },
  hits: [{ t: HIT, freeze: 0.08, shake: 0.14, amp: 2, push: 5, flash: 2 }],
};

export function create(o) {
  const [cx, cy, rx, ry, a0, a1, tl] = o.arc, ct = Math.cos(tl), st = Math.sin(tl), R = Math.max(rx, Math.abs(ry));
  const at = (a) => { const x = rx * Math.cos(a), y = ry * Math.sin(a); return [cx + x * ct - y * st, cy + x * st + y * ct]; };
  const [hx, hy] = at(a1), r = rng(o.seed), ps = particles(r), ls = lights();
  let t = 0, bit = false;
  const arc = (p, back) => {   // the far-back quarter goes behind the body
    const u = clamp(t / SWING), f = clamp((t - SWING) / FADE), head = lerp(a0, a1, 0.4 + 0.6 * ease.outQuart(u));
    const tail = Math.max(a0, head - lerp(2.2, 2.9, u) * (1 - ease.outQuad(f))), wmax = lerp(7, 1, f);
    if (f >= 1 || head <= tail + 0.05) return;
    p.shade(cx - R - 2, cy - R - 2, cx + R + 2, cy + R + 2, (x, y) => {
      const dx = x - cx, dy = y - cy, ex = (dx * ct + dy * st) / rx, ey = (-dx * st + dy * ct) / ry;
      if ((ex < 0.3 && ey < 0) !== back) return null;
      const d = Math.hypot(ex, ey), a = Math.atan2(ey, ex), k = (a - tail) / (head - tail);
      if (k < 0 || k > 1) return null;
      const rad = (1 - d) * Math.hypot(rx * ex / d, ry * ey / d), w = 0.6 + wmax * Math.sin(Math.PI * k ** 1.7);
      if (rad < -0.5 || rad > w) return null;
      const edge = rad < 0.5, al = (k < 0.3 ? k / 0.3 : 1) * (1 - f * f) * (back ? 0.8 : 1);
      const c = edge && k > 0.6 ? '#ffffff' : shade(o.palette, clamp(0.3 + 0.75 * k - (rad / w) * 0.35 + (edge ? 0.2 : 0) - (back ? 0.2 : 0)));
      return { c, a: al, l: shade(o.spark, 0.6), k: 0.25 * k * (1 - f) };
    });
    if (!back) { const [gx, gy] = at(head); p.glow(gx, gy, 9, shade(o.spark, 0.8), 0.4 * (1 - f)); }
  };
  return {
    update(dt) {
      t += dt;
      if (!bit && t >= HIT - 1e-6) {
        bit = true;
        for (const [vx, vy] of burst(10, r, 40, 150, -1.3, 1.1)) ps.spawn(hx, hy, vx, vy - 20, r.range(0.15, 0.35), { kind: 'spark', ramp: o.spark, bounce: 0.4 });
        ls.add(hx, hy, 18, shade(o.spark, 0.7), 1, 0.25);
      }
      ps.update(dt); ls.update(dt);
    },
    drawBack(p) { arc(p, true); },
    draw(p) {
      arc(p, false);
      if (t >= HIT - 1e-6 && t < HIT + 0.07) { sparkle(p, hx, hy, 4, '#ffffff', shade(o.spark, 0.8)); p.line(hx - 2, hy - 7, hx + 2, hy + 7, '#ffffff'); }
      ls.draw(p); ps.draw(p);
    },
  };
}
```

## melee/hammer-smash

Acted overhead hammer blow into the floor: a smear arc and two white frames on the release, a long freeze and heavy shake, a floor shockwave in two halves, glowing cracks and a crater behind, tumbling rocks that kick dust where they land, sparks and rolling dust. Option `smear: false` drops the hammer smear for reuse under other weapons.

```js
// id: melee/hammer-smash
import { rng, particles, lights, burst, ease, clamp, lerp, shade, TAU } from './pxfx.mjs';
import { READY, CAST_UP } from './pxact.mjs';

export const HIT = 0, IX = 18;   // the head meets the floor at (IX, 0) on the release frame
const FLASH = 0.05, WAVE = 0.45, SQ = 0.28, H = { weapon: 'hammer' };

export const meta = {
  name: 'hammer-smash', size: [128, 72], origin: [46, 58], duration: 1.6, loop: false, fps: 30,
  options: { palette: 'fire', rock: 'stone', dust: 'dust', seed: 4, smear: true }, tags: ['melee', 'hammer', 'heavy'],
};

const UP = { ...CAST_UP, ...H, x: -1, lean: -3, hx: -3, hy: -36, wa: -2.2 }, DN = { ...H, x: 3, crouch: 7, lean: 4, hx: 14, hy: -10, bx: 9, by: -12, wa: 1.2 };
export const act = {
  poses: [[-7 / 12, { ...READY, ...H }], [-1 / 3, UP], [-1 / 12, { ...UP, crouch: 2, lean: -4, hx: -6, hy: -35, wa: -2.6 }],
    [0, { ...DN, ghost: true }, 'snap'], [1 / 3, { ...DN, crouch: 8, lean: 5, hy: -9 }], [0.9, { ...READY, ...H }]],
  target: { x: 38 },
  hits: [{ t: HIT, freeze: 0.18, shake: 0.4, amp: 4, push: 4, flash: 1 }],
};

export function create(o) {
  const r = rng(o.seed), ps = particles(r), ls = lights(), D = { kind: 'dust', ramp: o.dust }, P = (u) => shade(o.palette, u), K = (u) => shade(o.rock, u);
  for (let i = 0; i < 14; i++) { const s = i % 2 ? 1 : -1, q = r.next();
    if (i < 9) ps.spawn(IX + s * r.range(1, 6), -1, s * r.range(10, 55), -r.range(80, 180), 1.3, { kind: 'debris', ramp: o.rock, size: r.int(2, 3), bounce: 0.35, back: i % 3 === 0,
      onLand: (x) => ps.spawn(x, -1, 0, -4, 0.6, { ...D, size: 1.5, alpha: 0.5 }) });
    ps.spawn(IX + s * r.range(2, 8), -1, s * lerp(25, 120, q), -r.range(3, 14), lerp(1.4, 0.8, q), { ...D, size: lerp(4, 2, q), drag: 3, back: i % 4 === 0, delay: r.range(0, 0.08) }); }
  for (const [vx, vy] of burst(12, r, 70, 190, -2.8, -0.3)) ps.spawn(IX, -2, vx, vy, r.range(0.2, 0.5), { kind: 'spark', ramp: o.palette, bounce: 0.5 });
  ls.add(IX, -8, 20, P(0.7), 1, 0.4);
  const cracks = Array.from({ length: 6 }, (_, i) => ({ a: (i / 6) * TAU + r.range(-0.4, 0.4), L: r.range(9, 20), b: r.range(-0.5, 0.5) }));
  const fp = (a, d) => [IX + Math.cos(a) * d, Math.sin(a) * d * SQ];
  let t = 0;
  const wave = (p, half) => { const u = clamp(t / WAVE), rx = lerp(5, 40, ease.outQuart(u));
    if (u < 1) p.oval(IX, 0, rx, rx * SQ, P(0.9 - u * 0.7 - (half === 'back' ? 0.2 : 0)), u < 0.5 ? 1 : 2 - 2 * u, { width: u < 0.25 ? 2 : 1, half }); };
  return {
    update(dt) { t += dt; ps.update(dt); ls.update(dt); },
    drawBack(p) {
      const k = 1 - clamp((t - 0.6) / 0.9), g = ease.outExpo(t / 0.1);
      if (k > 0) { p.oval(IX, 0, 9 * k, 2.5 * k, K(0));
        for (const c of cracks) { const [x1, y1] = fp(c.a, c.L * 0.55 * g * k), [x2, y2] = fp(c.a + c.b, c.L * g * k);
          p.line(IX, 0, x1, y1, K(0)); p.line(x1, y1, x2, y2, K(0.1));
          const e = 1 - t / 0.5; if (e > 0) { p.px(x1, y1, P(0.7), e); p.lit(x1, y1, P(0.8), e); } } }
      wave(p, 'back'); ps.drawBack(p);
    },
    draw(p) {
      wave(p, 'front');
      const f = clamp(t / 0.6);
      if (f < 1) { p.glow(IX, 0, 30, P(0.5), 0.5 * (1 - f), 0.3); p.glow(IX, -3, 8, P(1), 1.2 * (1 - f) ** 3); }
      if (t < FLASH) {
        if (o.smear) for (let i = 0; i < 4; i++) p.arc(5, -21, 25 - i, -1.9 + i * 0.25, 1.0, 1, K(0.5 + i * 0.15), 1 - i * 0.15);
        p.oval(IX, -1, 15, 3, '#ffffff'); p.rect(IX - 1, -22, 3, 21, '#ffffff');
      }
      ls.draw(p); ps.draw(p);
    },
  };
}
```
