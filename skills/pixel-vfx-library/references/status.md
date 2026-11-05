# Status

Looping conditions on the character: each sets `meta.actor`, repaints the body through `actorPaint` and reads `o.actor` every frame.

## status/frozen

The body turned to ice by brightness with rime on its upper edges and a shine crossing once per loop, a frost outline with rime spikes, crystal clusters at the feet, flakes and floor mist. Loop.

```js
// id: status/frozen
import { cycle, hash, shade, sparkle, crystal, clamp, lerp, TAU } from './pxfx.mjs';

export const meta = {
  name: 'frozen', size: [60, 60], origin: [30, 54], duration: 2.4, loop: true, fps: 12, actor: true,
  options: { palette: 'ice', seed: 3 }, tags: ['status', 'ice', 'frozen', 'body', 'loop'],
};

const on = (A, x, y) => { const i = x - A.x, j = y - A.y; return i >= 0 && j >= 0 && i < A.w && j < A.h && A.mask[j * A.w + i]; };

// the body turns to ice by brightness; rime lines its edges; a shine band crosses once per loop
export function actorPaint(p, t, o) {
  const A = o.actor, P = o.palette, pos = lerp(-50, 40, t / meta.duration);
  p.body(A, (c, x, y) => {
    const lum = (0.3 * c[0] + 0.55 * c[1] + 0.15 * c[2]) / 255;
    const rim = !on(A, x, y - 1) ? 0.35 : !on(A, x + 1, y) ? 0.2 : 0, sh = Math.abs(x - y * 0.6 - pos) < 2 ? 0.35 : 0;
    return shade(P, clamp(0.05 + lum * 1.3 + rim + sh));
  });
}

export function drawBack(p, t, o) {
  const P = o.palette;
  p.glow(0, 0, 22, shade(P, 0.6), 0.4, 0.3);
  p.oval(0, 0, 15, 3, shade(P, 0.35));
  p.oval(0, 0, 15, 3, shade(P, 0.65), 1, { width: 1, half: 'back' });
}

export function draw(p, t, o) {
  const A = o.actor, P = o.palette, s = o.seed;
  p.oval(0, 0, 15, 3, shade(P, 0.8), 1, { width: 1, half: 'front' });
  p.line(-8, 1, -2, 1, shade(P, 0.9));
  // frost outline with rime spikes pointing away from the body
  for (let y = A.y - 1; y <= A.y + A.h; y++) for (let x = A.x - 1; x <= A.x + A.w; x++) {
    if (on(A, x, y)) continue;
    const dx = on(A, x - 1, y) ? 1 : on(A, x + 1, y) ? -1 : 0, dy = on(A, x, y - 1) ? 1 : on(A, x, y + 1) ? -1 : 0;
    if (!dx && !dy) continue;
    p.px(x, y, shade(P, dy > 0 ? 0.85 : dx ? 0.6 : 0.4));
    if (y < -3 && dy <= 0 && hash(x, y, s) < 0.12) { p.px(x + dx, y + dy, shade(P, 0.7)); if (hash(y, x, s) < 0.4) p.px(x + 2 * dx, y + 2 * dy, shade(P, 0.85)); }
  }
  crystal(p, -8, 0, 8, 6, -3, P); crystal(p, -4, 0, 5, 4, -1, P); crystal(p, 7, 0, 11, 7, 3, P); crystal(p, 11, 0, 5, 4, 2, P);
  cycle(t, meta.duration, 4, (q, i, r) => {        // twinkles on the rime
    const k = Math.sin(q * Math.PI) ** 3;
    if (k > 0.2) sparkle(p, r.int(-6, 7), r.int(-34, -4), Math.round(k * 2.4), '#ffffff', shade(P, 0.8));
  }, { seed: s });
  cycle(t, meta.duration, 10, (q, i, r) => {       // flakes sway down
    const x = r.range(-22, 22) + Math.sin(q * TAU * 2 + i) * 2, y = -50 + q * 50, c = shade(P, r.range(0.75, 1));
    if (r.chance(0.3)) sparkle(p, x, y, 1, c, shade(P, 0.55)); else p.px(x, y, c);
    p.lit(x, y, c, 0.3 * (1 - q));
  }, { seed: s + 1 });
  cycle(t, meta.duration, 5, (q, i, r) => {        // cold mist rolls out along the floor
    const d = r.sign(), x = d * (r.range(2, 6) + q * 14), rad = 2 + 3 * q, a = 0.4 * Math.sin(q * Math.PI);
    p.puff(x, -rad * 0.5, rad, shade(P, 0.7), a);
    p.puff(x - 0.5, -rad, rad * 0.55, shade(P, 1), a * 0.6);
  }, { seed: s + 2 });
}
```

## status/poisoned

The body pulses toward poison green in hard steps with a pulsing outline, bubbles rise off its sides and pop, and drops gather at the hand, fall and splat. Loop.

```js
// id: status/poisoned
import { cycle, shade, rgb, clamp, lerp, ease, TAU } from './pxfx.mjs';

export const meta = {
  name: 'poisoned', size: [60, 64], origin: [30, 56], duration: 2, loop: true, fps: 15, actor: true,
  options: { palette: 'poison', seed: 2 }, tags: ['status', 'poison', 'sick', 'body', 'loop'],
};

const BUB = [[[0, -1], [1, 0], [0, 1], [-1, 0]],
  [[-1, -2], [0, -2], [1, -2], [2, -1], [2, 0], [2, 1], [1, 2], [0, 2], [-1, 2], [-2, 1], [-2, 0], [-2, -1]]];
const pulse = (t) => Math.round((0.3 + 0.25 * Math.sin((t / meta.duration) * TAU * 2)) * 4) / 4;

// the body's own colours pulled toward the poison ramp by brightness, in a few hard pulse steps
export function actorPaint(p, t, o) {
  const k = pulse(t);
  p.body(o.actor, (c) => {
    const g = rgb(shade(o.palette, clamp((0.3 * c[0] + 0.55 * c[1] + 0.15 * c[2]) / 255 * 1.4)));
    return c.map((v, i) => Math.round(lerp(v, g[i], k)));
  });
}

export function draw(p, t, o) {
  const A = o.actor, P = o.palette, ph = t / meta.duration, k = pulse(t);
  p.glow(0, 0, 18, shade(P, 0.6), 0.25 + 0.3 * k, 0.3);
  p.glow(A.chest[0], A.chest[1], 14, shade(P, 0.7), 0.1 + 0.15 * k);
  p.outline(A, A.x, A.y, shade(P, k > 0.4 ? 0.5 : 0.25));
  cycle(t, meta.duration, 8, (q, i, r) => {     // bubbles swell as they rise off the body, then pop
    const sd = r.sign(), x0 = sd * r.range(5, 9), y0 = r.range(-34, -10), x = x0 + sd * q * 4 + Math.sin(q * TAU * 2 + i) * 1.5, y = y0 - q * 18;
    if (q < 0.8) {
      const rad = q < 0.3 ? 0 : q < 0.6 ? 1 : 2;
      for (const [dx, dy] of rad ? BUB[rad - 1] : [[0, 0]]) p.px(x + dx, y + dy, shade(P, rad ? 0.7 : 0.9));
      if (rad) p.px(x - 1, y - rad + 1, shade(P, 1));
      p.lit(x, y, shade(P, 0.8), 0.3);
    } else if (q < 0.9) {
      for (const [dx, dy] of [[-3, 0], [3, 0], [0, -3], [-2, -2], [2, -2]]) p.px(x + dx, y + dy, shade(P, 0.9));
      p.lit(x, y, shade(P, 1), 0.6);
    }
  }, { seed: o.seed, speed: 1 });
  cycle(t, meta.duration, 2, (q, i, r) => {     // a drop gathers, falls and splats
    const [hx, hy] = A.hand, x = hx + (i ? 1 : -1);
    if (q < 0.45) {
      const s = ease.inQuad(q / 0.45) * 2;
      p.px(x, hy + 1, shade(P, 0.7)); if (s > 0.8) p.px(x, hy + 2, shade(P, 0.9)); if (s > 1.5) p.px(x, hy + 3, shade(P, 1));
    } else if (q < 0.62) {
      const y = lerp(hy + 2, 0, ease.inQuad((q - 0.45) / 0.17));
      p.line(x, y - 2, x, y, shade(P, 0.6)); p.px(x, y, shade(P, 1)); p.lit(x, y, shade(P, 0.8), 0.4);
    } else {
      const u = (q - 0.62) / 0.38, a = clamp(2 - 2 * u);
      p.oval(x, 0, 1 + 4 * ease.outQuart(u), 1, shade(P, 0.5), a);
      p.oval(x, 0, 1 + 4 * ease.outQuart(u), 1, shade(P, 0.85), a, { width: 1, half: 'front' });
      if (u < 0.3) { const h = 3 * Math.sin(u / 0.3 * Math.PI); p.px(x - 1 - u * 8, -h, shade(P, 0.9)); p.px(x + 1 + u * 8, -h, shade(P, 0.9)); }
    }
  }, { seed: o.seed + 1 });
}
```
