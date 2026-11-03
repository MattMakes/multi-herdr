# UI

Game-feel text over the play field: hit numbers and level-up bursts drawn with the tool's pixel fonts. Pass your own string with the `text` option.

## ui/damage-number

A damage number (option `text`) pops in at double size and white with burst ticks, drops to size with an overshoot, floats up, cools down the ramp and fades.

```js
// id: ui/damage-number
import { textMask, shade, ease, clamp, lerp } from './pxfx.mjs';

const POP = 0.1;      // big white pop, then the number drops to its size
const RISE = 0.35;    // overshoot jump, then a slow float
const FADE = 0.8;

export const meta = {
  name: 'damage-number', size: [88, 60], origin: [44, 46], duration: 1, loop: false, fps: 30,
  options: { palette: 'fire', seed: 1, text: '128' },
  tags: ['ui', 'damage', 'number', 'text'],
};

// a glyph mask drawn per row: dark outline, a 2-tone face (bright top, warmer bottom), a drop shadow
const number = (p, m, x, y, o, a, white, heat = 1) => {
  const s = (i, j) => i >= 0 && j >= 0 && i < m.w && j < m.h && m.mask[j * m.w + i];
  for (let j = -1; j <= m.h + 1; j++) for (let i = -1; i <= m.w; i++) {
    if (s(i, j)) p.px(x + i, y + j, white ? '#ffffff' : shade(o.palette, (j < m.h * 0.45 ? 1 : 0.78) * heat), a);
    else if (s(i - 1, j) || s(i + 1, j) || s(i, j - 1) || s(i, j + 1)) p.px(x + i, y + j, shade(o.palette, 0), a);
    else if (s(i, j - 2) || s(i - 1, j - 1)) p.px(x + i, y + j, shade(o.palette, 0.1), a * 0.6);
  }
};

export function create(o) {
  const big = textMask(o.text, 'big', 2), small = textMask(o.text, 'big', 1);
  let t = 0;
  return {
    update(dt) { t += dt; },
    draw(p) {
      if (t >= meta.duration) return;
      if (t < POP) {   // pop: double size, white on the first frames, a burst of ticks and a flash
        const w = t < 0.04, y = -big.h / 2 - 4 - t * 40;
        p.glow(0, -8, 22, shade(o.palette, 0.8), 0.9 * (1 - t / POP));
        for (let k = 0; k < 8; k++) {
          const a = k * Math.PI / 4 + 0.4, r0 = Math.max(21, big.w / 2 + 4) + t * 100, r1 = r0 + (w ? 6 : 3);
          p.line(Math.cos(a) * r0, -8 + Math.sin(a) * r0 * 0.5, Math.cos(a) * r1, -8 + Math.sin(a) * r1 * 0.5, w ? '#ffffff' : shade(o.palette, 0.8));
        }
        number(p, big, Math.round(-big.w / 2), Math.round(y), o, 1, w);
        return;
      }
      const u = clamp((t - POP) / RISE), f = clamp((t - POP - RISE) / (meta.duration - POP - RISE - 0.05));
      const y = lerp(-12, -20, ease.outBack(u)) - ease.outQuad((t - POP) / (meta.duration - POP)) * 14, x = -small.w / 2 + Math.sin(f * 3) * 1.5;
      const a = 1 - clamp((t - FADE) / (meta.duration - FADE - 0.05));
      if (u < 0.3) p.glow(0, y + 3, 14, shade(o.palette, 0.8), 0.5 * (1 - u / 0.3));
      number(p, small, Math.round(x), Math.round(y), o, a, u < 0.12, 1 - f * 0.45);
      if (a > 0.3) p.lit(Math.round(x) + 1, Math.round(y) + 1, '#ffffff', 0.6 * a);
    },
  };
}
```

## ui/level-up

On the body: motes gather, the body flashes white, a floor ring and a column of light burst up with stars spiralling round the body, the letters of `text` jump in one by one, a shine runs across them, then they rise and fade.

```js
// id: ui/level-up
import { rng, shade, sparkle, band, FAR, NEAR, textMask, ease, clamp, lerp, TAU } from './pxfx.mjs';
import { READY, CROUCH, CAST_UP } from './pxact.mjs';

const { abs, cos, sin, round } = Math, GO = 0.2, TY = -74;

export const meta = {
  name: 'level-up', size: [96, 108], origin: [48, 96], duration: 2.4, loop: false, actor: true,
  options: { palette: 'holy', seed: 6, text: 'LEVEL UP' },
  tags: ['ui', 'level', 'text'],
};

// crouch, arms up on the release, hold
export const act = { poses: [[-0.3, READY], [GO - 1 / 12, CROUCH], [GO, CAST_UP, 'snap'], [GO + 15 / 12, CAST_UP], [GO + 19 / 12, READY]] };

export function actorPaint(p, t, o) {
  const k = t < GO ? ease.inQuad(t / GO) * 0.5 : 0.6 * (1 - clamp((t - GO) / 1.1)), sw = lerp(4, -46, clamp((t - GO) / 0.6));
  p.body(o.actor, (c, x, y) => {
    if (t >= GO && t < GO + 0.07) return '#ffffff';
    const l = (c[0] + 2 * c[1]) / 765, s = abs(y - sw) < 2 ? 0.35 : 0;
    return l + k + s > 0.7 ? shade(o.palette, clamp(l * 0.6 + k * 0.5 + s + 0.2)) : c;
  });
}
const ring = (p, t, P, keep) => {
  const u = clamp((t - GO) / 0.6);
  if (u > 0 && u < 1) band(p, 0, 0, lerp(6, 30, ease.outQuart(u)), 0.28, u < 0.3 ? 2 : 1, 1 - u, P, keep);
};
// spiralling stars, far half behind
const stars = (p, t, P, seed, back) => {
  const r = rng(seed), S = (u) => shade(P, u);
  for (let i = 0; i < 10; i++) {
    const u = (t - GO - r.range(0, 0.5)) / r.range(0.9, 1.4), a = r.range(0, TAU) + u * 5, R = r.range(11, 17), H = r.range(50, 80);
    if (u <= 0 || u >= 1 || (sin(a) < 0) !== back) continue;
    const x = cos(a) * R * (1 - u * 0.4), y = -4 - ease.outQuad(u) * H, s = round((1 - u) * (1.4 + sin(t * 30 + i)));
    sparkle(p, x, y, s, back || !s ? S(0.8) : '#ffffff', S(0.7));
    p.lit(x, y, S(0.8), 0.6 * (1 - u));
  }
};

export function drawBack(p, t, o) { ring(p, t, o.palette, FAR); stars(p, t, o.palette, o.seed, true); }
export function draw(p, t, o) {
  const P = o.palette, S = (u) => shade(P, u), f = 1 - clamp((t - 1.5) / 0.8);
  if (t < GO) {   // gather
    const g = t / GO, d = lerp(28, 3, ease.inQuad(g));
    for (let a = 0.3; a < TAU; a += 0.8) sparkle(p, cos(a) * d, sin(a) * d * 0.6 - 14, 1, '#ffffff', S(0.5 + g / 2));
    return p.glow(0, -16, 12 + g * 10, S(0.7), 0.5 * g);
  }
  ring(p, t, P, NEAR);
  const top = -4 - ease.outExpo((t - GO) / 0.25) * 60, w = lerp(10, 4, clamp(t - GO)), a = 1 - clamp((t - 0.9) / 0.6);
  if (a > 0) {
    const h = top * ease.outQuad(a), c = S(t < GO + 0.1 ? 1 : lerp(0.55, 0.8, a));
    for (let y = 0; y > h; y--) { const k = clamp((1 - y / h) * 3); p.px(-w, y, c, k); p.px(w, y, c, k); }
    if (t < GO + 0.07) p.rect(-w + 1, top, w * 2 - 1, -top, '#ffffff');
    p.glow(0, top / 2, 20, S(0.6), 0.3 * a, 0.6);
    p.glow(0, 0, 26, S(0.6), 0.5 * a, 0.3);
  }
  stars(p, t, P, o.seed, false);
  // letters jump in, shine, cool, rise, fade
  const tt = t - GO, s = o.text + '', W = s.length * 6 - 1, sh = lerp(-10, W + 10, clamp((tt - 0.6) / 0.4));
  if (f <= 0) return;
  [...s].forEach((ch, n) => {
    const q = tt - n * 0.035, m = textMask(ch, 'big'), x = round(-W / 2 + n * 6);
    if (q < 0) return;
    const y = round(TY + lerp(10, 0, ease.outBack(q / 0.2)) - (1 - f) * 8);
    p.outline(m, x, y, S(0), f);
    for (let j = 0; j < m.h; j++) for (let i = 0; i < m.w; i++) if (m.mask[j * m.w + i])
      p.px(x + i, y + j, q < 0.07 || abs(x + W / 2 + i + j - sh) < 2 ? '#ffffff' : S((j < 3 ? 0.85 : 0.6) * (1 + f) / 2), clamp(f * 3));
  });
}
```
