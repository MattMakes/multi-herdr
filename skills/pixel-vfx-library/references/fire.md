# Fire

Flames built from looping noise quantised to the fire ramp, with embers, smoke and warm light pools. Loops for props and projectiles, a one-shot for spells.

## fire/torch

A wall torch loop: a tall flickering flame on a short wrapped stick, embers drifting up, a faint smoke wisp and a warm light pool.

```js
// id: fire/torch
import { fbm, cycle, shade, clamp, TAU } from './pxfx.mjs';

const H = 22, P = 4;   // flame height, noise period (scrolls exactly P cells per loop)
export const meta = {
  name: 'torch', size: [56, 76], origin: [28, 66], duration: 1.6, loop: true, fps: 15,
  options: { palette: 'fire', wood: 'wood', metal: 'metal', smoke: 'smoke', seed: 3 }, tags: ['fire', 'loop', 'light', 'torch', 'wall'],
};

export function draw(p, t, o) {
  const ph = t / meta.duration, B = -15;
  const fl = 0.5 + 0.3 * Math.sin(ph * TAU * 4) + 0.2 * Math.sin(ph * TAU * 7 + 2);
  p.glow(0, B - 8, 28, shade(o.palette, 0.45), 0.36 + 0.12 * fl);
  p.glow(0, B - 2, 16, shade(o.palette, 0.6), 0.2 + 0.1 * fl, 0.6);
  p.glow(0, B - 5, 9, shade(o.palette, 0.85), 0.35 + 0.15 * fl);
  cycle(t, meta.duration, 3, (q, i, r) => {
    const y = B - H + 2 - q * 22, x = Math.sin(q * TAU + i * 2) * 2 + q * r.range(-3, 3);
    p.puff(x, y, 1.5 + q * 2.5, shade(o.smoke, 0.35), 0.35 * Math.sin(q * Math.PI));
  }, { seed: o.seed + 9 });
  // stick, bracket and wrapped head
  for (let y = -11; y <= 0; y++) { p.px(-1, y, shade(o.wood, 0.15)); p.px(0, y, shade(o.wood, 0.45)); p.px(1, y, shade(o.wood, y < -8 ? 0.95 : 0.7)); }
  p.line(-3, -5, 3, -5, shade(o.metal, 0.3)); p.line(-2, -6, 2, -6, shade(o.metal, 0.7));
  for (let y = B; y <= -12; y++) { p.line(-3, y, 3, y, shade(o.wood, y % 2 ? 0.05 : 0.25)); p.px(3, y, shade(o.wood, 0.55)); }
  for (let y = B - H; y <= B; y++) {
    const h = (B - y) / H, half = 4.2 * (1 - h) ** 0.7 + 0.6;
    for (let x = -6; x <= 6; x++) {
      const n = fbm(x * 0.32 + o.seed, y * 0.2 + ph * P, o.seed, 3, 0, P);
      const sway = Math.sin(ph * TAU * 2 + h * 3) * h * 1.8;
      const heat = (1 - Math.abs(x - sway) / half) * (1.1 - h * 0.6) - (n - 0.5) * (0.8 + h * 1.4) - h * 0.25;
      if (heat > 0.06) p.px(x, y, shade(o.palette, clamp(0.16 + heat * 0.8 - h * 0.2)));
    }
  }
  for (const x of [-2, 0, 2]) p.lit(x, B, shade(o.palette, 0.9), 0.6 * fl);
  for (const sp of [1, 2]) cycle(t, meta.duration, 4 - sp, (q, i, r) => {
    const y = B - 8 - q * r.range(26, 44), x = r.range(-3, 3) + Math.sin(q * TAU * sp + r.range(0, 6)) * (1 + q * r.range(2, 6));
    if (q < 0.8) { p.px(x, y, shade(o.palette, 0.95 - q)); p.lit(x, y, shade(o.palette, 0.8), 0.6 * (1 - q)); }
  }, { seed: o.seed + sp, speed: sp });
}
```

## fire/flame-pillar

A floor glyph draws itself and brightens, a column of fire bursts up with a white flash, roars, then collapses into smoke and embers.

```js
// id: fire/flame-pillar
import { rng, particles, fbm, ease, clamp, shade, TAU } from './pxfx.mjs';

const REL = 1 / 3, ROAR = 0.8, FALL = 0.35, H = 64, RX = 18, SQ = 0.26;
export const meta = {
  name: 'flame-pillar', size: [96, 128], origin: [48, 112], duration: 3, loop: false, fps: 30,
  options: { palette: 'fire', seed: 4 }, tags: ['fire', 'pillar', 'ground'],
  timing: { windup: REL, hits: [{ t: REL, freeze: 0.06, shake: 0.3, amp: 3 }], recovery: 0.6 },
};

export function create(o) {
  const r = rng(o.seed), ps = particles(r), C = (u) => shade(o.palette, u), W = "#ffffff";
  let t = 0, burst = false, fell = false;
  const height = () => t < REL - 1e-6 ? 0 : t < REL + ROAR ? H * ease.outExpo((t - REL) / 0.1) * (0.94 + 0.06 * Math.sin(t * 40))
    : H * (1 - ease.inQuad((t - REL - ROAR) / FALL));
  const glyph = (p, back) => {
    const g = clamp(t / REL), fade = 1 - clamp((t - REL - ROAR) / 0.6), hot = t >= REL - 1e-6 ? 1 - clamp((t - REL) / 0.4) : 0, dim = back ? 0.15 : 0;
    if (fade <= 0) return;
    for (const rr of [RX, RX - 4]) for (let a = 0; a < TAU * ease.outQuad(g); a += 0.05) {
      const s = Math.sin(a + rr);
      if ((s < 0) === back) p.px(Math.cos(a + rr) * rr, s * rr * SQ, C(0.45 + 0.5 * hot + 0.25 * g - dim), fade);
    }
    for (let i = 0; i < 8; i++) {
      const a = (i / 8) * TAU + 0.2, s = Math.sin(a), c = Math.cos(a);
      if ((s < 0) === back && g >= i / 8) p.line(c * (RX - 3), s * (RX - 3) * SQ, c * (RX - 1), s * (RX - 1) * SQ - 1, C(0.8 - dim), fade);
    }
    p.glow(0, 0, RX + 6, C(0.6), (0.7 * g * g + 0.6 * hot) * fade, SQ); p.glow(0, -2, 6, C(0.9), g ** 3 * fade);
  };
  const column = (p, back) => {
    const h = height();
    if (h < 1) return;
    for (let y = -Math.ceil(h); y <= 0; y++) {
      const k = -y / h, half = (back ? 10 : 8) * (1 - 0.15 * k) + 5 * (1 - k) ** 4;
      for (let x = -Math.ceil(half) - 2; x <= half + 2; x++) {
        const n = fbm(x * 0.25 + (back ? 9 : 0), y * 0.13 + t * 9, o.seed + (back ? 5 : 0), 3);
        const heat = (1 - Math.abs(x) / half) * 1.1 - (n - 0.5) * (0.9 + k * 1.2) - k ** 4 * 0.9 - (back ? 0.25 : 0);
        if (heat > 0.06) p.px(x, y, C(clamp(0.12 + heat * 0.85)));
      }
    }
    if (!back) for (let y = 4; y < h; y += 10) p.glow(0, -y, 16, C(0.55), 0.25 * (1 - y / h));
  };
  return {
    update(dt) {
      t += dt;
      if (!burst && t >= REL - 1e-6) {
        burst = true;
        for (let i = 0; i < 18; i++) {
          const a = -Math.PI * r.range(0.1, 0.9), s = r.range(60, 190);
          ps.spawn(r.range(-6, 6), -2, Math.cos(a) * s * 0.45, Math.sin(a) * s, r.range(0.3, 0.6), { kind: 'spark', ramp: o.palette, bounce: 0.3 });
        }
      }
      if (t > REL && t < REL + ROAR && r.chance(dt * 30)) ps.spawn(r.range(-6, 6), -r.range(10, 50), r.range(-10, 10), -r.range(30, 50), r.range(0.6, 1), { kind: 'ember', ramp: o.palette });
      if (!fell && t >= REL + ROAR - 1e-6) {
        fell = true;
        for (let i = 0; i < 14; i++) ps.spawn(r.range(-6, 6), -r.range(4, H * 0.8), r.range(-12, 12), -r.range(5, 20), r.range(0.9, 1.3),
          { kind: 'smoke', size: r.range(3, 5), grow: 2, alpha: 0.5, delay: r.range(0, FALL) });
      }
      ps.update(dt);
    },
    drawBack(p) { glyph(p, true); column(p, true); },
    draw(p) {
      column(p, false); glyph(p, false);
      if (t >= REL - 1e-6 && t < REL + 0.05) { p.rect(-3, -H, 7, H, W); p.oval(0, 0, RX, RX * SQ, W); p.glow(0, -30, 40, C(0.8), 0.8); }
      ps.draw(p);
    },
  };
}
```

## fire/fireball

A looping fireball flying right: a ringed hot head, noisy flame tongues streaming back, shed embers and a thin smoke trail.

```js
// id: fire/fireball
import { fbm, cycle, shade, clamp, TAU } from './pxfx.mjs';

const L = 26, P = 4;   // tail length, noise period (scrolls exactly P cells per loop)
export const meta = {
  name: 'fireball', size: [52, 30], origin: [36, 15], duration: 0.4, loop: true, fps: 30,
  options: { palette: 'fire', smoke: 'smoke', seed: 2 }, tags: ['fire', 'projectile', 'loop', 'light'],
};

export function draw(p, t, o) {
  const ph = t / meta.duration, wob = Math.sin(ph * TAU * 2) * 0.6;
  cycle(t, meta.duration, 4, (q, i, r) => {
    const x = -14 - q * 20, y = r.range(-2, 2) - q * 4;
    p.puff(x, y, 1.5 + q * 2.5, shade(o.smoke, 0.3 + 0.2 * r.next()), 0.45 * (1 - q) * Math.min(1, q * 5));
  }, { seed: o.seed + 5 });
  p.glow(-4, 0, 16, shade(o.palette, 0.5), 0.35);
  p.glow(0, 0, 7, shade(o.palette, 0.9), 0.3);
  for (let x = -L; x <= 5; x++) {
    const k = clamp(-x / L), half = x > 0 ? Math.sqrt(Math.max(0, 30 - x * x)) : 5.4 * (1 - k) ** 0.8 + 0.4;
    for (let y = -7; y <= 7; y++) {
      const n = fbm(x * 0.22 + ph * P, y * 0.35 + o.seed, o.seed, 3, P, 0);
      const edge = 1 - Math.abs(y - wob * k * 3) / half;
      const heat = edge * (1.1 - k * 0.4) - (n - 0.5) * (0.15 + k * 1.9) - k * 0.4;
      if (heat > 0.04 + k * 0.12) p.px(x, y, shade(o.palette, clamp(0.08 + heat * 0.8)));
    }
  }
  p.disc(1, 0, 1.2, '#ffffff'); p.lit(1, 0, '#ffffff', 0.5);
  cycle(t, meta.duration, 6, (q, i, r) => {
    const x = -4 - q * r.range(22, 34), y = r.range(-4, 4) + Math.sin(q * TAU + i) * 1.5 - q * 3;
    if (q < 0.9) { p.px(x, y, shade(o.palette, 0.95 - q * 0.9)); p.lit(x, y, shade(o.palette, 0.8), 0.5 * (1 - q)); }
  }, { seed: o.seed });
}
```
