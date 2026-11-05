# Water

Water as thin lines and rims: crowns, droplets, ripples on a flat surface and bubbles, in the water ramp.

## water/splash

Something drops into water: a crown of spikes, droplets breaking off its tips and falling back, a centre jet and spreading ripple rings.

```js
// id: water/splash
import { rng, particles, ease, clamp, lerp, shade, TAU } from './pxfx.mjs';

const CROWN = 0.32, JET = [0.2, 0.6], SQ = 0.25;   // crown life, centre jet window, surface squash
export const meta = {
  name: 'splash', size: [112, 76], origin: [56, 56], duration: 1.6, loop: false, fps: 30,
  options: { palette: 'water', seed: 5 }, tags: ['water', 'splash', 'ground'],
};

export function create(o) {
  const r = rng(o.seed), ps = particles(r), C = (u) => shade(o.palette, u), rings = [[0, 40, 1.1], [0.12, 30, 1], [0.3, 22, 0.9]];
  const land = (x) => rings.length < 9 && rings.push([t, 5, 0.35, x]);
  for (let i = 0; i < 16; i++) {
    const a = (i / 16) * TAU + r.range(-0.15, 0.15), s = r.range(30, 60);
    ps.spawn(Math.cos(a) * 13, Math.sin(a) * 13 * SQ - r.range(10, 17), Math.cos(a) * s, -r.range(60, 110), r.range(0.55, 0.8),
      { kind: 'spark', ramp: o.palette, gravity: 320, drag: 0.6, glow: 0.3, bounce: 0, floor: 1, onLand: land, back: Math.sin(a) < 0, delay: r.range(0.1, 0.16) });
  }
  let t = 0;
  const crown = (p, back) => {
    const u = clamp(t / CROWN);
    if (u >= 1) return;
    const rc = lerp(5, 13, ease.outQuart(u)), h = 18 * Math.sin(Math.PI * u ** 0.7);
    p.oval(0, 0, rc, rc * SQ, C(back ? 0.5 : 0.85), 1, { half: back ? 'back' : 'front', width: 1 });
    for (let i = 0; i < 11; i++) {
      const a = (i / 11) * TAU + 0.3, s = Math.sin(a);
      if ((s < 0) !== back) continue;
      const x = Math.cos(a) * rc, y = s * rc * SQ, hh = h * (0.6 + 0.4 * Math.sin(i * 2.3)), x2 = x * (1.1 + 0.4 * u);
      p.line(x, y, x2, y - hh, C(back ? 0.4 : 0.6));
      p.px(x2, y - hh, C(back ? 0.75 : 1)); p.lit(x2, y - hh, C(1), 0.3);
    }
  };
  const ripples = (p, half) => {
    for (const [t0, R, life, x0 = 0] of rings) {
      const u = (t - t0) / life;
      if (u < 0 || u >= 1) continue;
      const rx = lerp(4, R, ease.outQuad(u));
      p.oval(x0, 0, rx, rx * SQ, C(0.95 - u * 0.6 - (half === 'back' ? 0.25 : 0)), u < 0.75 ? 1 : 4 - u * 4, { half, width: 1 });
    }
  };
  return {
    update(dt) { t += dt; ps.update(dt); },
    drawBack(p) { ripples(p, 'back'); crown(p, true); ps.drawBack(p); },
    draw(p) {
      ripples(p, 'front'); crown(p, false);
      const j = (t - JET[0]) / (JET[1] - JET[0]);
      if (j > 0 && j < 1) {
        const h = 18 * Math.sin(Math.PI * j);
        p.line(0, 0, 0, -h, C(0.7)); p.line(1, 0, 1, -h * 0.7, C(0.5));
        p.disc(0, -h - 3 * j, 1, C(1)); p.lit(0, -h - 3 * j, C(1), 0.5);
      }
      if (t < 0.05) { p.oval(0, 0, 10, 3, '#ffffff', 1, { width: 2 }); p.glow(0, -2, 18, C(1), 0.6, 0.5); }
      p.glow(0, 0, 26, C(0.7), 0.25 * (1 - clamp(t / 1.2)), SQ);
      ps.draw(p);
    },
  };
}
```

## water/bubbles

A loop of bubbles of mixed sizes wobbling upward and popping at a wavy surface line with tiny splashes.

```js
// id: water/bubbles
import { cycle, shade, lerp, TAU } from './pxfx.mjs';

const S = 44, W = 18;   // surface height above the source, surface half-width
export const meta = {
  name: 'bubbles', size: [48, 60], origin: [24, 54], duration: 2, loop: true, fps: 20,
  options: { palette: 'water', seed: 7 }, tags: ['water', 'bubbles', 'loop', 'underwater'],
};

export function draw(p, t, o) {
  const ph = t / meta.duration, C = (u) => shade(o.palette, u), pops = [];
  for (const sp of [1, 2]) cycle(t, meta.duration, sp === 1 ? 4 : 3, (q, i, r) => {
    const R0 = sp === 1 ? r.int(2, 4) : r.int(0, 1), rise = 0.82, x0 = r.range(-10, 10);
    const u = q / rise, y = Math.round(-(u ** 1.3) * S), x = Math.round(x0 + Math.sin(q * TAU * r.int(2, 3) + i) * (1 + R0 * 0.4));
    const rad = R0 === 0 ? 0 : Math.max(1, Math.round(R0 * Math.min(1, u * 5)));
    if (u < 1) {
      if (rad < 2) { p.px(x, y, C(0.9)); if (rad) p.px(x + 1, y - 1, C(0.6)); p.lit(x, y, C(1), 0.3); return; }
      p.puff(x, y, rad - 0.5, C(0.45), 0.25);
      p.ring(x, y, rad, 1, C(0.7));
      p.px(x - Math.ceil(rad / 2), y - Math.ceil(rad / 2), '#ffffff');
      p.lit(x, y, C(0.9), 0.15);
    } else pops.push([x, (q - rise) / (1 - rise), R0]);
  }, { seed: o.seed + sp, speed: sp });
  for (let x = -W; x <= W; x++) {        // the surface: a wavy bright line over a darker step
    let y = Math.sin(x * 0.45 + ph * TAU * 2) * 0.8 + Math.sin(x * 0.2 - ph * TAU) * 0.6;
    for (const [px, k, rad] of pops) y -= Math.max(0, 2 - Math.abs(x - px) * 0.6) * Math.sin(Math.PI * k) * rad * 0.5;
    const e = Math.abs(x);
    p.px(x, Math.round(y) - S, C(e > W - 4 ? 0.55 : 0.85)); if (e < W - 2) p.px(x, Math.round(y) - S + 1, C(e > W - 6 ? 0.2 : 0.4));
  }
  for (const [x, k, rad] of pops) {      // pop: dots fly off, a little ring spreads on the surface
    const d = lerp(1, 3 + rad, k), a = 1 - k;
    for (const [dx, dy] of [[-1, -1], [1, -1], [-1.4, -0.3], [1.4, -0.3]]) p.px(x + dx * d, -S - 1 + dy * d * 1.5, C(1), a);
    if (k < 0.6) p.oval(x, -S, d + 1, (d + 1) * 0.25, C(0.9 - k), 1, { width: 1 });
  }
  p.glow(0, -S, 20, C(0.6), 0.15, 0.25);
}
```
