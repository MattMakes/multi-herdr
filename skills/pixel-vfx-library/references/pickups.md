# Pickups

Collectables: an idle loop to place in the level and a one-shot to play when it is taken.

## pickups/coin-spin

A gold coin spinning in place: the width squashes with the turn, a lit rim shows edge-on, an embossed face lit from the upper left, a sliding glint, a rim twinkle and a small bob.

```js
// id: pickups/coin-spin
import { shade, sparkle, clamp, TAU } from './pxfx.mjs';

const R = 7, CY = -12;   // coin radius, centre height above the floor

export const meta = {
  name: 'coin-spin', size: [32, 32], origin: [16, 28], duration: 1, loop: true, fps: 12,
  options: { palette: 'gold', seed: 1 },
  tags: ['pickup', 'coin', 'gold', 'loop'],
};

export function draw(p, t, o) {
  const P = o.palette, ph = t / meta.duration, th = ph * TAU;
  const c = Math.cos(th), s = Math.sin(th), rx = Math.abs(c) * R, cy = CY + Math.round(Math.sin(ph * TAU) * 1.5);
  const lit = 0.55 + 0.45 * Math.abs(c), e = Math.round(s * 1.6);   // face brightness; edge offset
  p.line(-3 - Math.round(Math.abs(c) * 2), 0, 3 + Math.round(Math.abs(c) * 2), 0, shade(P, 0)); // floor shadow
  p.glow(0, cy, 13, shade(P, 0.7), 0.18 + 0.1 * Math.abs(c));
  for (let y = -R; y <= R; y++) {
    const k = Math.sqrt(1 - (y / (R + 0.4)) ** 2), hw = rx * k;
    // the rim seen edge-on: a band between the face and its shifted copy
    for (let x = Math.round(Math.min(-hw, -hw + e)); x <= Math.round(Math.max(hw, hw + e)); x++) {
      const inFace = Math.abs(x) <= hw + 0.3;
      if (!inFace) { p.px(x, cy + y, shade(P, (y < -R + 2 ? 0.55 : 0.3)), 1); continue; }
      const rim = Math.abs(x) >= hw - 0.7 || Math.abs(y) >= R, ie = Math.abs(x) / Math.max(hw * 0.6, 0.5) + 0, iy = Math.abs(y) / 4.6;
      const q = Math.hypot(ie, iy), sx = x * Math.sign(c || 1);
      let u = rim ? 0.3 : 0.55;
      if (!rim && Math.abs(q - 1) < 0.22) u = sx + y < 0 ? 0.95 : 0.3;            // embossed inner ring: lit top-left
      else if (!rim && q < 0.78) u = sx === 0 && Math.abs(y) <= 2 && hw > 3 ? 1 : 0.7;   // the centre and its mark
      if (!rim && q > 1.22 && sx + y < -5) u = 0.75;                            // a lit crescent on the field
      if (hw < 1.2) u = y < 0 ? 0.75 : 0.45;                                    // edge-on: a thin bright line
      p.px(x, cy + y, shade(P, clamp(u * lit + 0.05)));
    }
  }
  // a glint slides across the face while it turns toward us, then a twinkle on the rim
  const g = ph * 2 % 1;
  if (Math.abs(c) > 0.6 && g > 0.2 && g < 0.8) {
    const gx = Math.round((g - 0.5) * rx * 2.4), gy = Math.round(-(g - 0.5) * 12);
    for (let i = -1; i <= 1; i++) if (Math.abs(gx + i) < rx - 1 && Math.abs(gy - i) < R - 1) p.px(gx + i, cy + gy - i, '#ffffff');
    p.lit(gx, cy + gy, shade(P, 1), 0.5);
  }
  const tw = Math.sin(ph * TAU * 2 - 1);
  if (tw > 0.6) sparkle(p, -Math.round(rx * 0.7) - 1, cy - 5, tw > 0.85 ? 2 : 1, '#ffffff', shade(P, 0.9));
}
```

## pickups/collect-burst

A pickup is taken: a white flash, a ring that pops out and dithers away, eight twinkling stars thrown outward with drag, and a quick column of sparkles rising.

```js
// id: pickups/collect-burst
import { rng, particles, lights, burst, sparkle, shade, ease, clamp, lerp } from './pxfx.mjs';

const FLASH = 0.05, RING = 0.32;

export const meta = {
  name: 'collect-burst', size: [80, 80], origin: [40, 48], duration: 0.6, loop: false, fps: 30,
  options: { palette: 'gold', seed: 2 },
  tags: ['pickup', 'collect', 'sparkle', 'coin'],
  timing: { windup: 0, hits: [{ t: 0, freeze: 0.03, shake: 0.06, amp: 1 }], recovery: 0.2 },
};

export function create(o) {
  const r = rng(o.seed), ps = particles(r), ls = lights(), P = o.palette;
  for (const [vx, vy] of burst(8, r, 120, 170, 0, Math.PI * 2, 0.25))
    ps.spawn(vx * 0.04, vy * 0.04, vx, vy, r.range(0.35, 0.5), { kind: 'star', ramp: P, size: r.int(2, 3), drag: 5 });
  for (let i = 0; i < 4; i++)
    ps.spawn(r.range(-3, 3), 0, r.range(-20, 20), -r.range(30, 80), r.range(0.3, 0.6), { kind: 'spark', ramp: P, gravity: 60, drag: 3, delay: r.range(0, 0.06), floor: null });
  ls.add(0, 0, 20, shade(P, 0.9), 1.2, 0.35);
  const rise = Array.from({ length: 4 }, (_, i) => ({ x: [0, 4, -4, 1][i], d: 0.03 + i * 0.06, h: 34 - i * 5 + r.range(-2, 2), l: r.range(0.3, 0.42) }));
  let t = 0;
  return {
    update(dt) { t += dt; ps.update(dt); ls.update(dt); },
    draw(p) {
      ls.draw(p);
      if (t < FLASH) { p.disc(0, 0, 4, '#ffffff'); sparkle(p, 0, 0, 7, '#ffffff', shade(P, 0.9)); }
      const u = clamp(t / RING);
      if (u < 1) {   // the ring pops out fast, thins and dithers away
        const rr = lerp(3, 17, ease.outQuart(u)), a = u < 0.5 ? 1 : 1 - (u - 0.5) / 0.5;
        p.ring(0, 0, rr, u < 0.25 ? 2 : 1, u < 0.15 ? '#ffffff' : shade(P, 0.95 - u * 0.5), a);
        if (u > 0.1 && u < 0.5) p.ring(0, 0, rr * 0.7, 1, shade(P, 0.5));
      }
      ps.draw(p);
      for (const s of rise) {   // a quick upward sparkle column
        const q = (t - s.d) / s.l;
        if (q <= 0 || q >= 1) continue;
        const y = -ease.outQuart(q) * s.h, arm = Math.round((1 - q) * 2.5);
        p.line(s.x, y, s.x, y + 3 + (1 - q) * 5, shade(P, 0.6), 1 - q);
        sparkle(p, s.x, y, arm, q < 0.3 ? '#ffffff' : shade(P, 1), shade(P, 0.8));
        p.lit(s.x, y, shade(P, 0.9), 0.7 * (1 - q));
      }
    },
  };
}
```
