# Critters

Small ambient creatures as seamless pure loops, drawn from shapes rather than sprite sheets, so palette and size options recolour them. Original designs.

## critters/bat

A small bat flapping in place at 12 fps: four wing poses with a lit leading edge and finger bones, glowing eyes, and a one-pixel bob with the downstroke.

```js
// id: critters/bat
import { shade } from './pxfx.mjs';

// four wing poses: arm angle (rad, negative = up), finger spread, span scale, body bob, finger fan offset
const POSES = [[-1.05, 0.5, 1, 1, 0.5], [-0.1, 0.7, 1, 0, 0.45], [0.65, 0.5, 0.9, -1, -0.45], [-0.55, 0.35, 0.7, 0, 0.6]];

export const meta = {
  name: 'bat', size: [48, 36], origin: [24, 18], duration: 0.6667, loop: true, fps: 12,
  options: { palette: ['#0a0610', '#2a1a3a', '#4a2f62', '#7a5490', '#c09ad6'], eye: '#ff5a6e', seed: 1 },
  tags: ['critter', 'bat', 'enemy', 'loop', 'flying'],
};

const wing = (p, P, s, [arm, spread, span, , fan], y) => {
  const sx = 2 * s, sy = y - 2, ax = Math.cos(arm) * 7 * span, ay = Math.sin(arm) * 7;
  const wx = sx + ax * s, wy = sy + ay;                       // the wrist
  const tips = [0, 1, 2].map((k) => {
    const a = arm + fan + k * spread, L = (9 - k * 1.5) * span;
    return [wx + Math.cos(a) * L * s, wy + Math.sin(a) * L];
  });
  // membrane: shoulder, wrist, fingertips with scallops between them, back to the hip
  const hip = [s * 1.5, y + 2], pts = [[sx, sy], [wx, wy], tips[0]];
  for (let k = 1; k < 3; k++) {
    const [x0, y0] = tips[k - 1], [x1, y1] = tips[k];
    pts.push([(x0 + x1) / 2 + (wx - (x0 + x1) / 2) * 0.25, (y0 + y1) / 2 + (wy - (y0 + y1) / 2) * 0.25], tips[k]);
  }
  pts.push([(tips[2][0] + hip[0]) / 2 + (wx - (tips[2][0] + hip[0]) / 2) * 0.2, (tips[2][1] + hip[1]) / 2], hip);
  p.poly(pts, shade(P, 0.2));
  p.tri(sx, sy, wx, wy, ...tips[0], shade(P, 0.3));                           // the near panel catches more light
  for (const [tx, ty] of tips) p.line(wx, wy, tx, ty, shade(P, 0.6));      // finger bones
  p.line(sx, sy, wx, wy, shade(P, 0.85));                                    // the arm: a lit leading edge
  p.px(wx, wy - 1, shade(P, 0.6));                                           // thumb claw
};

export function draw(p, t, o) {
  const P = o.palette, f = Math.floor((t / meta.duration) * 8 + 1e-6) % 4, pose = POSES[f], y = pose[3];
  wing(p, P, -1, pose, y); wing(p, P, 1, pose, y);
  // body: a fuzzy oval with a lighter chest, ears, glowing eyes, little fangs and feet
  p.disc(0, y, 3, shade(P, 0.3));
  p.line(-1, y - 3, -3, y - 1, shade(P, 0.6));                                                   // moonlit rim, upper left
  p.rect(-2, y - 1, 5, 4, shade(P, 0.3));
  p.line(-1, y + 3, 1, y + 3, shade(P, 0.3));
  p.line(0, y - 1, 0, y + 2, shade(P, 0.55)); p.px(-1, y + 1, shade(P, 0.5)); p.px(1, y + 1, shade(P, 0.5));
  p.line(-2, y - 3, -3, y - 6, shade(P, 0.3)); p.line(2, y - 3, 3, y - 6, shade(P, 0.3));      // ears
  p.px(-2, y - 4, shade(P, 0.7)); p.px(2, y - 4, shade(P, 0.7));
  p.line(-1, y - 3, 1, y - 3, shade(P, 0.45));                                                  // brow
  p.px(-1, y - 2, o.eye); p.px(1, y - 2, o.eye);
  p.lit(-1, y - 2, o.eye, 0.5); p.lit(1, y - 2, o.eye, 0.5);
  p.glow(0, y - 2, 5, o.eye, 0.12);
  p.px(0, y, shade(P, 0.95));                                                                    // a single fang glint
  p.px(-1, y + 4, shade(P, 0.1)); p.px(1, y + 4, shade(P, 0.1));                                 // feet
}
```

## critters/slime

A green slime hopping in place: anticipation squash, stretch on launch, a parabolic hop over a shrinking shadow, a splat landing with droplets and a wobble, a white shine and a translucent core.

```js
// id: critters/slime
import { shade, smoothstep, clamp, lerp } from './pxfx.mjs';

const W = 9, H = 11, JUMP = 16, AIR = [0.36, 0.72];   // rest half-width and height; hop height; airborne window
// squash and stretch keys over one loop: [phase, width scale, height scale]
const KEYS = [[0, 1, 1], [0.16, 1.05, 0.95], [0.3, 1.3, 0.68], [0.37, 0.75, 1.32], [0.54, 0.96, 1.05],
  [0.7, 0.82, 1.22], [0.75, 1.4, 0.6], [0.83, 0.88, 1.14], [0.91, 1.06, 0.95], [1, 1, 1]];

export const meta = {
  name: 'slime', size: [48, 48], origin: [24, 42], duration: 1.2, loop: true, fps: 15,
  options: { palette: ['#0c2410', '#1b5220', '#2e8f33', '#5ccc48', '#b6f58c'], seed: 1 },
  tags: ['critter', 'slime', 'enemy', 'loop', 'hop'],
};

const key = (u) => {
  for (let i = 1; i < KEYS.length; i++) if (u <= KEYS[i][0]) {
    const [u0, a0, b0] = KEYS[i - 1], [u1, a1, b1] = KEYS[i], k = smoothstep(0, 1, (u - u0) / (u1 - u0));
    return [lerp(a0, a1, k), lerp(b0, b1, k)];
  }
  return [1, 1];
};

export function draw(p, t, o) {
  const P = o.palette, u = t / meta.duration, [sx, sy] = key(u);
  const s = clamp((u - AIR[0]) / (AIR[1] - AIR[0])), lift = Math.round(JUMP * 4 * s * (1 - s));
  const w = W * sx, h = Math.round(H * sy), base = -lift;
  // floor: a shadow that shrinks as it rises, a faint green glow
  const sw = Math.round(w * (1 - lift / (JUMP * 1.6)));
  p.line(-sw, 0, sw, 0, shade(P, 0)); p.line(-sw + 2, 1, sw - 2, 1, shade(P, 0));
  p.glow(0, 0, 14, shade(P, 0.6), 0.22, 0.3);
  const hwAt = (j) => { const v = j / h; return w * Math.sqrt(Math.max(0, 1 - v ** 2.2)) * Math.min(1, 0.8 + v * 3); };
  for (let j = 0; j <= h; j++) {
    const hw = hwAt(j), y = base - j, v = j / h, n = Math.round(hw);
    for (let x = -n; x <= n; x++) {
      const edge = Math.abs(x) >= n || j === h || j === 0 || Math.round(hwAt(j + 1)) < Math.abs(x);
      let c = edge ? 0.12 : v < 0.2 || x > hw * 0.62 ? 0.38 : 0.55;                   // shadow side and base
      if (!edge && ((x + w * 0.15) / w) ** 2 + ((v - 0.5) / 0.4) ** 2 < 0.3) c = 0.68;  // translucent bright core
      if (!edge && x < 0 && Math.round(hwAt(j + 1)) < Math.abs(x - 1) && v > 0.3) c = 0.8; // lit upper-left rim
      p.px(x, y, shade(P, c));
    }
  }
  // the shine: a short white arc and a dot, upper left; two eyes that squash with the body
  const top = base - h;
  p.px(-Math.round(w * 0.5), top + Math.round(h * 0.28), '#ffffff');
  p.px(-Math.round(w * 0.5) + 1, top + Math.round(h * 0.2), '#ffffff');
  p.px(-Math.round(w * 0.5) + 2, top + Math.round(h * 0.2), shade(P, 1));
  p.px(-Math.round(w * 0.5), top + Math.round(h * 0.42), shade(P, 1));
  p.lit(-Math.round(w * 0.5), top + Math.round(h * 0.25), shade(P, 1), 0.4);
  const ey = base - Math.round(h * 0.45), ex = Math.max(2, Math.round(w * 0.32));
  for (const e of [-ex, ex + 1]) { p.px(e, ey, shade(P, 0)); if (sy > 0.8) p.px(e, ey - 1, shade(P, 0)); }
  // landing: two droplets splash out and fall back
  const d = (u - 0.74) / 0.14;
  if (d > 0 && d < 1) for (const k of [-1, 1]) {
    p.px(k * (w + 2 + d * 6), -1 - Math.sin(d * Math.PI) * 6, shade(P, d < 0.5 ? 0.85 : 0.6));
  }
}
```
