# Explosions

Bursts with a white flash frame, a fast swell, sparks and blended smoke that rises and thins until the frame is empty.

## explosions/small-explosion

A quick fiery pop: white flash, a noisy orange ball with an air ring, bouncing sparks, dark smoke rising behind it and thinning.

```js
// id: explosions/small-explosion
import { rng, particles, lights, burst, fbm, ease, clamp, lerp, shade } from './pxfx.mjs';

const FLASH = 0.04, GROW = 0.12, COOL = 0.6, R = 13;   // flash, swell, burn-out, ball radius
export const meta = {
  name: 'small-explosion', size: [80, 88], origin: [40, 56], duration: 1.3, loop: false, fps: 30,
  options: { palette: 'fire', smoke: 'smoke', ground: 20, seed: 6 }, tags: ['explosion', 'fire', 'impact'],
  timing: { windup: 0, hits: [{ t: 0, freeze: 0.06, shake: 0.2, amp: 2 }], recovery: 0.4 },
};

export function create(o) {
  const r = rng(o.seed), ps = particles(r), ls = lights(), C = (u) => shade(o.palette, u);
  for (const [vx, vy] of burst(12, r, 60, 130, 0, Math.PI * 2, 0.3))
    ps.spawn(vx * 0.05, vy * 0.05, vx, vy - 40, r.range(0.25, 0.5), { kind: 'spark', ramp: o.palette, bounce: 0.4, floor: o.ground });
  for (let i = 0; i < 11; i++)
    ps.spawn(r.range(-9, 9), r.range(-8, 4), r.range(-16, 16), -r.range(14, 30), r.range(0.85, 1.05),
      { kind: 'smoke', ramp: o.smoke, size: r.range(3.5, 5.5), grow: 1.9, alpha: 0.8, gravity: -50, back: true, delay: r.range(0.12, 0.3) });
  ls.add(0, 0, 30, C(0.75), 1.2, 0.35);
  let t = 0;
  return {
    update(dt) { t += dt; ps.update(dt); ls.update(dt); },
    drawBack(p) { ps.drawBack(p); },
    draw(p) {
      const g = ease.outExpo(t / GROW), cool = clamp(t / COOL), rad = R * g * (1 + cool * 0.25), cy = -t * 12;
      if (cool < 1) for (let y = -R - 5; y <= R + 5; y++) for (let x = -R - 5; x <= R + 5; x++) {
        const d = Math.hypot(x, y) / rad;
        if (d > 1.2) continue;
        const n = fbm(x * 0.2 + t * 3, y * 0.2 + t * 6, o.seed, 3);
        const heat = (1 - d) * 1.5 - (n - 0.5) * (0.5 + cool * 1.7) - cool ** 1.5 * 1.1;
        if (heat > 0.05) p.px(x, y + cy, C(clamp(0.1 + heat * 0.8 + (1 - cool) * 0.2)));
      }
      const u = clamp(t / 0.25);
      if (u < 1) p.ring(0, 0, lerp(6, 24, ease.outQuart(u)), 1, C(0.95 - u * 0.6), 1 - u);
      if (t < FLASH) { p.disc(0, 0, 8, '#ffffff'); p.glow(0, 0, 24, C(1), 1); }
      ls.draw(p); ps.draw(p);
    },
  };
}
```

## explosions/smoke-bomb

A white pop on the floor, then a grey cloud bursts out low and wide, rolls along the floor, hides the spot and clears slowly.

```js
// id: explosions/smoke-bomb
import { rng, particles, ease, clamp, lerp, shade, sparkle } from './pxfx.mjs';

const POP = 0.05, RING = 0.4, SQ = 0.26;   // white pop, floor ring life, floor squash
export const meta = {
  name: 'smoke-bomb', size: [144, 84], origin: [72, 70], duration: 3.2, loop: false, fps: 30,
  options: { palette: ['#2c2c36', '#4a4a56', '#706f7c', '#9c9ba6', '#cbcad2'], seed: 8 },
  tags: ['smoke', 'explosion', 'ground', 'stealth'],
  timing: { windup: 0, hits: [{ t: 0, freeze: 0.03, shake: 0.1, amp: 1 }], recovery: 0.3 },
};

export function create(o) {
  const r = rng(o.seed), ps = particles(r), C = (u) => shade(o.palette, u);
  for (let i = 0; i < 22; i++) {            // billows: burst out, swell, hang, thin
    const s = r.sign(), f = i / 21;
    ps.spawn(s * r.range(0, 6), -r.range(2, 6), s * lerp(4, 70, f) * r.range(0.7, 1), -lerp(130, 8, f) * r.range(0.7, 1), r.range(1.8, 2.7),
      { kind: 'smoke', ramp: o.palette, size: lerp(7, 4, f), grow: 1.7, drag: 3.2, gravity: -5, alpha: 0.95, back: i % 4 === 3, delay: r.range(0, 0.12) });
  }
  for (let i = 0; i < 9; i++)               // dense core over the spot
    ps.spawn(r.range(-7, 7), -4 - i * 4, r.range(-8, 8), -r.range(4, 12), r.range(2.2, 2.8),
      { kind: 'smoke', ramp: o.palette, size: r.range(5, 7), grow: 1.5, drag: 2, gravity: -3, alpha: 0.95, delay: 0.04 + i * 0.015 });
  for (let i = 0; i < 14; i++) {            // rolling skirt along the floor
    const s = i % 2 ? 1 : -1;
    ps.spawn(s * r.range(2, 8), 0, s * r.range(60, 140), -r.range(0, 8), r.range(1.1, 1.9),
      { kind: 'dust', ramp: o.palette, size: r.range(3, 5), grow: 2, alpha: 0.85, delay: r.range(0, 0.08) });
  }
  for (let i = 0; i < 6; i++) ps.spawn(0, -3, r.range(-90, 90), -r.range(60, 140), r.range(0.4, 0.7), { kind: 'grit', ramp: o.palette, shade: 0.9 });
  let t = 0;
  const ring = (p, half) => {
    const u = clamp(t / RING);
    if (u < 1) p.oval(0, 0, lerp(8, 50, ease.outQuart(u)), lerp(8, 50, ease.outQuart(u)) * SQ, C(1 - u * 0.6 - (half === 'back' ? 0.2 : 0)), 1 - u, { half, width: u < 0.3 ? 2 : 1 });
  };
  return {
    update(dt) { t += dt; ps.update(dt); },
    drawBack(p) { ring(p, 'back'); ps.drawBack(p); },
    draw(p) {
      ring(p, 'front');
      if (t < POP) { p.oval(0, -3, 14, 5, '#ffffff'); sparkle(p, 0, -6, 6, '#ffffff', C(0.8)); p.glow(0, -4, 22, C(1), 0.7, 0.5); }
      else if (t < 0.25) p.glow(0, -4, 22, C(0.9), 0.35 * (1 - t / 0.25), 0.5);
      ps.draw(p);
    },
  };
}
```
