# Arcane

Raw magic in violet: portals, gathering energy and release pops. Recolour with `palette` for any school of magic.

## arcane/portal

An upright oval portal: a dark eye with spiral arms turning once per loop, a rim lit from the upper left with a running glint, motes spiralling in. Pure loop.

```js
// id: arcane/portal
import { fbm, cycle, shade, clamp, fract, TAU } from './pxfx.mjs';

const RX = 13, RY = 22, CY = -25;   // portal half-width, half-height, centre height

export const meta = {
  name: 'portal', size: [72, 80], origin: [36, 70], duration: 2, loop: true, fps: 15,
  options: { palette: 'arcane', seed: 3 }, tags: ['arcane', 'portal', 'magic', 'loop'],
};

export function drawBack(p, t, o) {
  const ph = t / meta.duration;
  p.glow(0, 0, 26, shade(o.palette, 0.6), 0.35 + 0.08 * Math.sin(ph * TAU * 2), 0.25);
  p.oval(0, 0, RX + 2, 3, shade(o.palette, 0.1));
}

export function draw(p, t, o) {
  const ph = t / meta.duration, P = o.palette, s = o.seed;
  p.glow(0, CY, 30, shade(P, 0.5), 0.22 + 0.06 * Math.sin(ph * TAU * 2));
  // interior: a spiral of looping noise turning once per loop, dark in the eye
  p.shade(-RX, CY - RY, RX, CY + RY, (x, y) => {
    const dx = x / RX, dy = (y - CY) / RY, d = Math.hypot(dx, dy);
    if (d > 0.93) return null;
    const a = Math.atan2(dy, dx) / TAU, u = fract(a * 3 + d * 2.2 - ph * 3);
    const n = fbm(a * 8, d * 3 + ph * 3, s, 2, 8, 3), arm = Math.abs(u - 0.5) * 2;
    const v = (1 - arm) * (0.35 + d) + (n - 0.5) * 0.8;
    const st = v > 1.05 ? 0.8 : v > 0.75 ? 0.55 : v > 0.45 ? 0.3 : d > 0.75 ? 0.12 : 0;
    return { c: shade(P, st), l: st > 0.5 ? shade(P, 0.9) : null, k: 0.2 };
  });
  // rim: dark outer step, a band lit from the upper left, a white glint running round once per loop
  p.oval(0, CY, RX + 1, RY + 1, shade(P, 0.25), 1, { width: 1 });
  p.glow(0, CY + 2, 7, shade(P, 0.8), 0.35 + 0.15 * Math.sin(ph * TAU * 4));
  p.oval(0, CY, RX, RY, shade(P, 0.55), 1, { width: 2 });
  for (let i = 0; i < 120; i++) {
    const a = (i / 120) * TAU, lt = -Math.sin(a + 0.8);
    if (lt > 0.2) p.px(Math.cos(a) * RX, CY + Math.sin(a) * RY, shade(P, 0.75 + 0.25 * lt));
  }
  for (let k = 0; k < 2; k++) for (let i = 0; i < 10; i++) {
    const a = (ph + k / 2) * TAU + i * 0.06, x = Math.cos(a) * RX, y = CY + Math.sin(a) * RY;
    p.px(x, y, i > 6 ? '#ffffff' : shade(P, 1));
    p.lit(x, y, shade(P, 1), 0.6 * i / 10);
  }
  // motes spiral in from outside and vanish into the rim
  cycle(t, meta.duration, 14, (q, i, r) => {
    const a0 = r.range(0, TAU), turn = r.range(0.8, 1.6), d = 1 + (1 - q) ** 1.5 * r.range(0.4, 0.85);
    const pt = (dd, qq) => [Math.cos(a0 + qq * turn) * RX * dd, CY + Math.sin(a0 + qq * turn) * RY * dd];
    const [x, y] = pt(d, q), tl = [pt(d + 0.12, q - 0.05), pt(d + 0.05, q - 0.025), [x, y]];
    const c = shade(P, 0.6 + 0.4 * q), a = clamp(q * 4) * clamp((1 - q) * 6);
    p.polyline(tl, shade(P, 0.3 + 0.3 * q), a);
    p.px(x, y, c, a);
    p.lit(x, y, c, 0.6 * a);
  }, { seed: s });
}
```

## arcane/spell-charge

Energy streaks converge on the hand while three orbit arcs tighten and spin up, then a white pop, a ring and sparks; exports `RELEASE` for moves. One-shot.

```js
// id: arcane/spell-charge
import { rng, particles, lights, burst, ease, clamp, lerp, shade, sparkle, TAU } from './pxfx.mjs';

export const RELEASE = 0.55;   // the pop, after the gather
const POP = 0.3;               // ring life after the pop

export const meta = {
  name: 'spell-charge', size: [64, 64], origin: [32, 32], duration: 1.1, loop: false, fps: 30,
  options: { palette: 'arcane', seed: 2 }, tags: ['arcane', 'charge', 'cast', 'hand', 'gather'],
};

export function create(o) {
  const r = rng(o.seed), ps = particles(r), ls = lights(), P = o.palette;
  for (let i = 0; i < 22; i++) {               // streaks pulled in from a wide ring
    const a = r.range(0, TAU), d = r.range(15, 24), dl = (RELEASE - 0.24) * Math.sqrt(r.next()), v = r.range(45, 60);
    ps.spawn(Math.cos(a) * d, Math.sin(a) * d, -Math.cos(a) * v, -Math.sin(a) * v, 0.5,
      { kind: 'mote', ramp: P, to: [0, 0], drag: -5, tail: r.int(3, 6), delay: dl });
  }
  let t = 0, popped = false;
  return {
    update(dt) {
      t += dt;
      if (!popped && t > RELEASE - 1e-6) {
        popped = true;
        for (const [vx, vy] of burst(9, r, 70, 130)) ps.spawn(0, 0, vx, vy, r.range(0.2, 0.45), { kind: 'spark', ramp: P, gravity: 40, drag: 4, floor: null });
        for (const [vx, vy] of burst(4, r, 20, 45)) ps.spawn(0, 0, vx, vy, r.range(0.35, 0.5), { kind: 'star', ramp: P, size: 2 });
        ls.add(0, 0, 22, shade(P, 0.8), 1.2, 0.35);
      }
      ps.update(dt); ls.update(dt);
    },
    draw(p) {
      const g = clamp(t / RELEASE), k = ease.inCubic(g);
      if (t < RELEASE - 1e-6) {
        p.glow(0, 0, lerp(6, 16, k), shade(P, 0.7), lerp(0.1, 0.6, k));
        const rad = lerp(13, 3, ease.inOutQuad(g)), spin = g * g * 14;
        for (let j = 0; j < 3; j++) {           // three orbit arcs that tighten and spin up
          const a0 = spin + (j * TAU) / 3;
          p.arc(0, 0, rad, a0, a0 + lerp(0.6, 1.6, g), 1, shade(P, 0.5 + 0.4 * g), clamp(g * 3));
          p.lit(Math.cos(a0 + 1) * rad, Math.sin(a0 + 1) * rad, shade(P, 1), 0.5 * g);
        }
        const cr = lerp(0.5, 2.5, k);
        p.disc(0, 0, cr, shade(P, 0.6 + 0.3 * k));
        p.disc(0, 0, cr - 1, shade(P, 1));
        p.lit(0, 0, '#ffffff', 0.6 * k);
      } else {
        const u = clamp((t - RELEASE) / POP), fl = t - RELEASE < 0.067;
        if (fl) { p.disc(0, 0, 5, '#ffffff'); sparkle(p, 0, 0, 9, '#ffffff', shade(P, 0.8)); }
        if (u < 1) {
          const rr = lerp(4, 17, ease.outQuart(u));
          p.ring(0, 0, rr, u < 0.3 ? 2 : 1, fl ? '#ffffff' : shade(P, 0.9 - 0.6 * u), 1 - u * u);
          if (!fl) p.disc(0, 0, 2 * (1 - u), shade(P, 1 - u * 0.5));
        }
      }
      ls.draw(p); ps.draw(p);
    },
  };
}
```
