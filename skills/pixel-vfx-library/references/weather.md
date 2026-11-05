# Weather

Full-screen weather loops for a 320x180 screen. The floor line sits at the origin (canvas row 150), so `--bg ground` shows the scene; both are pure loops built from `cycle()` particles that wrap across the width.

## weather/rain

Full-screen rain loop: diagonal streaks in two depths wrap across the screen, near drops land lower and splash a crown and a ripple on the floor, a cool mist light breathes along the floor line.

```js
// id: weather/rain
import { cycle, shade, clamp, TAU } from './pxfx.mjs';

const W = 320, TOP = -150, GROUND = 30;   // floor line at local y 0 (canvas row 150)

export const meta = {
  name: 'rain', size: [320, 180], origin: [0, 150], duration: 1, loop: true, fps: 30,
  options: { palette: 'water', seed: 7, wind: 0.22, far: 90, near: 56 },
  tags: ['weather', 'rain', 'loop', 'fullscreen'],
};

// one streak: pixels from the head back along the slant, wrapped across the screen width
const streak = (p, x, y, len, wind, c, c2, a) => {
  for (let k = 0; k < len; k++) {
    const yy = Math.round(y - k), xx = (((Math.round(x - k * wind)) % W) + W) % W;
    if (yy >= TOP && yy < GROUND) p.px(xx, yy, k < len * 0.4 ? c : c2, a);
  }
};

const layer = (p, t, o, n, near) => {
  const c0 = shade(o.palette, near ? 0.75 : 0.45), c1 = shade(o.palette, near ? 1 : 0.65), c2 = shade(o.palette, near ? 0.55 : 0.3);
  cycle(t, meta.duration, n, (q, i, r) => {
    const land = near ? r.range(6, 28) : r.range(0, 7), fall = land - TOP + 12, x0 = r.range(0, W);
    const len = near ? r.int(9, 14) : r.int(3, 5), L = near ? 0.72 : 0.8;   // fall share of the cycle
    const wx = (x) => (((Math.round(x)) % W) + W) % W;
    if (q < L) {
      const u = q / L, y = TOP - 12 + u * fall, x = x0 + u * fall * o.wind;
      streak(p, x, y, len, o.wind, c0, c2, 1);
      if (y >= TOP && y < GROUND) p.px(wx(x), Math.round(y), c1);
    } else {   // splash: two droplets hop out, a flat ripple spreads
      const s = (q - L) / (1 - L), x = wx(x0 + fall * o.wind), a = 1 - s;
      const h = Math.sin(s * Math.PI) * (near ? 3 : 1.5), d = 1 + s * (near ? 4 : 2);
      p.px(wx(x - d), land - h, c1, a); p.px(wx(x + d), land - h * 0.8, near ? c1 : c0, a);
      if (near && s < 0.5) p.px(x, land - 1 - s * 6, c1);
      if (near) {
        const rx = 1 + s * 5;
        p.px(wx(x - rx), land, c1, a); p.px(wx(x + rx), land, c1, a);
        p.px(wx(x - rx + 1), land + 1, c0, a * 0.5); p.px(wx(x + rx - 1), land + 1, c0, a * 0.5);
        if (s < 0.3) p.lit(x, land - 1, c1, 0.4);
      }
    }
  }, { seed: o.seed + (near ? 99 : 0), speed: near ? 2 : 1 });
};

export function draw(p, t, o) {
  const ph = t / meta.duration;
  for (let x = 40; x < W; x += 80) {   // a cool mist band breathing on the floor line
    const s = 0.5 + 0.5 * Math.sin(ph * TAU + x);
    p.glow(x, 3, 40, shade(o.palette, 0.45), 0.1 + 0.06 * s, 0.22);
  }
  layer(p, t, o, o.far, false);
  layer(p, t, o, o.near, true);
}
```

## weather/snow

Full-screen snow loop: flakes in three sizes and depths drift down with sway and wind, the near ones twinkle and glow, flakes melt where they land, and a soft blended haze breathes along the floor.

```js
// id: weather/snow
import { cycle, shade, sparkle, clamp, TAU } from './pxfx.mjs';

const W = 320, TOP = -150, GROUND = 30;   // floor line at local y 0 (canvas row 150)

export const meta = {
  name: 'snow', size: [320, 180], origin: [0, 150], duration: 4, loop: true, fps: 15,
  options: { palette: 'ice', seed: 3, wind: 14 },
  tags: ['weather', 'snow', 'loop', 'fullscreen'],
};

const wrap = (x) => 1 + ((Math.round(x) % (W - 2)) + W - 2) % (W - 2);

// three depths: far specks drift slowly, mid flakes, near crosses fall faster and sway wider
const DEPTH = [[70, 1, 0.5, 2, 1], [40, 2, 0.8, 4, 1], [22, 3, 1, 7, 2]];   // count, size, shade, sway, speed

export function draw(p, t, o) {
  const ph = t / meta.duration;
  for (let x = 9; x <= 311; x += 7) {                 // ground haze: low overlapping puffs, slowly breathing
    const s = Math.sin(ph * TAU + x * 0.13), j = (x * 37) % 5;
    p.puff(x, 3 + j - s, 7 + (j & 1) + s, shade(o.palette, 0.5), 0.07);
    if (x % 70 === 9 && x < 250) p.glow(x + 30, 4, 34, shade(o.palette, 0.6), 0.07 + 0.03 * s, 0.25);
  }
  DEPTH.forEach(([n, size, u, sway, sp], d) => {
    cycle(t, meta.duration, n, (q, i, r) => {
      const land = d === 0 ? r.range(-4, 4) : d === 1 ? r.range(2, 16) : r.range(14, 27);
      const y = TOP - 3 + q * (land - TOP + 3), k = r.int(1, 3);
      const x = wrap(r.range(0, W) + q * o.wind * (d + 1) + Math.sin(q * TAU * k + i) * sway);
      const melt = clamp((land - y) / 6), c = shade(o.palette, u * (0.8 + 0.2 * melt));
      if (y < TOP + 1) return;
      if (size === 1 || melt < 0.4) p.px(x, y, c, melt < 1 ? melt : 1);
      else if (size === 2) { p.rect(x, y, 2, 2, shade(o.palette, u * 0.75)); p.px(x, y, c); }
      else {   // near flake: a soft plus with a white heart, lit
        sparkle(p, x, y, 1, Math.sin(q * TAU * 3 + i) > 0.5 ? '#ffffff' : c, shade(o.palette, 0.7));
        p.lit(x, y, shade(o.palette, 0.8), 0.3);
      }
    }, { seed: o.seed + d * 31, speed: sp });
  });
}
```
