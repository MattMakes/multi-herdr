# Projectiles

Things that fly and the flashes that launch them. Flights are pure loops facing right that stay in place (the move or game moves and turns them); launches are short one-shots at the muzzle or hand.

## projectiles/magic-missile

Loop: a bright lens-shaped bolt with a white core and a pulsing glow, two strands braided around the flight line that widen and cool toward the tail, a core streak and sparkles shed backward.

```js
// id: projectiles/magic-missile
import { cycle, shade, sparkle, clamp, TAU } from './pxfx.mjs';

const L = 30;   // tail length in px; the head sits at (0, 0) and faces right

export const meta = {
  name: 'magic-missile', size: [64, 24], origin: [46, 12], duration: 0.4, loop: true, fps: 30,
  options: { palette: 'arcane', seed: 7 }, tags: ['projectile', 'magic', 'bolt', 'loop'],
};

export function draw(p, t, o) {
  const ph = t / meta.duration, pulse = 0.5 + 0.5 * Math.sin(ph * TAU * 2);
  p.glow(-4, 0, 14, shade(o.palette, 0.5), 0.35 + 0.1 * pulse, 0.6);
  // two strands twist around the flight line, widen and cool toward the tail
  for (const s of [0, Math.PI]) for (let x = -2; x >= -L; x--) {
    const d = -x / L, y = Math.sin(x * 0.42 + ph * TAU * 3 + s) * Math.min(2.6, 0.5 + d * 4);
    const c = shade(o.palette, 0.9 - d * 0.7), a = clamp((1 - d) * 4);
    p.px(x, y, c, a);
    if (d < 0.5) p.lit(x, y, c, 0.3 * (1 - d * 2));
  }
  for (let x = -3; x > -16; x--) p.px(x, 0, shade(o.palette, 0.95 + x * 0.04));   // core streak
  // shed sparkles drift back, twinkle and fade
  cycle(t, meta.duration, 9, (q, i, r) => {
    const x = -3 - q * (L + 4), y = r.range(-1.5, 1.5) + r.range(-6, 6) * q * q, k = 1 - q;
    const arm = Math.round(1.6 * k * (0.5 + 0.5 * Math.sin(q * TAU * 2 + i)));
    sparkle(p, x, y, arm, shade(o.palette, 0.6 + 0.4 * k), shade(o.palette, 0.4 * k + 0.2), k > 0.2 ? 1 : k * 5);
  }, { seed: o.seed });
  // the head: a bright lens with a white core and a hot nose
  const w = pulse > 0.7 ? 8 : 7;
  p.poly([[-w, 0], [-1, -3.5], [4.5, 0], [-1, 3.5]], shade(o.palette, 0.45));
  p.poly([[-w + 2, 0], [-1, -2.5], [3.5, 0], [-1, 2.5]], shade(o.palette, 0.8));
  p.disc(0, 0, 1, '#ffffff'); p.line(-4, 0, 3, 0, '#ffffff');
  p.glow(0, 0, 5, shade(o.palette, 1), 0.5 + 0.2 * pulse);
}
```

## projectiles/muzzle-flash

One-shot at a gun muzzle: a white star frame, a smaller hot star, an ember, a few fast forward sparks, a spent spark that pops up and falls behind, and a thin wisp of smoke that stretches forward and rises.

```js
// id: projectiles/muzzle-flash
import { rng, particles, shade, lerp } from './pxfx.mjs';

const F = 1 / 30;   // one flash drawing per 30 fps frame: white star, hot star, small ember

export const meta = {
  name: 'muzzle-flash', size: [72, 44], origin: [18, 26], duration: 0.8, loop: false, fps: 30,
  options: { palette: 'fire', smoke: 'smoke', seed: 9 }, tags: ['projectile', 'gun', 'muzzle', 'shot'],
  timing: { windup: 0, hits: [{ t: 0, freeze: 0.02, shake: 0.06, amp: 1 }], recovery: 0.1 },
};

export function create(o) {
  const r = rng(o.seed), ps = particles(r), C = (u) => shade(o.palette, u);
  for (let i = 0; i < 4; i++) ps.spawn(4, r.range(-1, 1), r.range(160, 260), r.range(-30, 30), r.range(0.06, 0.12), { kind: 'spark', ramp: o.palette, floor: null, drag: 6 });
  ps.spawn(-5, -2, -r.range(15, 25), -r.range(90, 110), 0.5, { kind: 'spark', ramp: o.palette, floor: null, gravity: 420, delay: 0.05 });
  for (let i = 0; i < 8; i++) ps.spawn(3 + i * 2, r.range(-1, 0), lerp(4, 50, i / 7), -r.range(4, 14), r.range(0.45, 0.65), { kind: 'smoke', ramp: o.smoke, size: lerp(1, 1.8, i / 7), grow: 2.2, wander: 30, alpha: 0.7, delay: 0.04 + i * 0.03 });
  let t = 0;
  // a star: a long forward spike, short side spikes, a stub behind, a diamond core
  const star = (p, s, c, core) => {
    p.poly([[0, -2.5 * s], [21 * s, 0], [0, 2.5 * s]], c);
    p.poly([[1, 0], [6 * s, -8 * s], [10 * s, 0], [6 * s, 8 * s]], c);
    p.line(0, 0, -4 * s, 0, c); p.line(3, -1, 12 * s, -6 * s, c); p.line(3, 1, 12 * s, 6 * s, c);
    p.poly([[0, 0], [5 * s, -3 * s], [12 * s, 0], [5 * s, 3 * s]], core);
  };
  return {
    update(dt) { t += dt; ps.update(dt); },
    draw(p) {
      const g = Math.max(0, 1 - t / 0.2);
      if (g > 0) { p.glow(8, 0, 20, C(0.7), 0.9 * g * g); p.glow(6, 0, 7, C(1), 1.2 * g ** 3); }
      if (t < F) { star(p, 1, '#ffffff', '#ffffff'); }
      else if (t < 2 * F) star(p, 0.75, C(0.75), C(1));
      else if (t < 3 * F) { p.disc(3, 0, 2, C(0.4)); p.disc(3, 0, 1, C(0.8)); }
      ps.draw(p);
    },
  };
}
```
