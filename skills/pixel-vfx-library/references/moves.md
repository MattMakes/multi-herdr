# Moves

Moves chain library effects on one timeline with `chain`: a gather in the hand, a thrown projectile and a burst on the target, with an act for the character. Small helper effects used only by one move are written inline in it.

## moves/fireball-throw

Acted move: fire gathers in the drawn-back hand, a snapped throw with a white release frame, a fireball flies, small-explosion bursts on the target and leaves it burnt.

```js
// id: moves/fireball-throw
import { cycle, shade, sparkle, clamp, TAU } from './pxfx.mjs';
import { chain, READY, CAST_FORWARD } from './pxact.mjs';
import * as ball from './fire-fireball.mjs';
import * as boom from './explosions-small-explosion.mjs';

const THROW = 0.45, SPEED = 240, FROM = 19, TO = 64, Y = -24, CX = -9, CY = -20;     // charge in the drawn-back hand
const HIT = THROW + (TO - 4 - FROM) / SPEED;                                            // the ball meets the dummy's front
export const meta = {
  name: 'fireball-throw', size: [136, 96], origin: [28, 84], duration: 2, loop: false, fps: 30,
  options: { palette: 'fire', seed: 1 }, tags: ['move', 'fire', 'projectile', 'throw'],
};
// The gather: fire streaks curl into the hand, a hot core swells; with pop: true, the white release frame.
const charge = {
  meta: { name: 'fire-charge', size: [48, 48], origin: [24, 24], duration: 0.6, loop: false, fps: 30, options: { palette: 'fire', seed: 1 } },
  draw(p, t, o) {
    const C = (u) => shade(o.palette, u);
    if (o.pop) { p.disc(0, 0, 4, '#ffffff'); sparkle(p, 0, 0, 8, '#ffffff', C(0.8)); p.glow(0, 0, 16, C(1), 0.9); return; }
    const u = clamp(t / THROW), rad = 1.5 + 3 * u;
    p.glow(0, 0, 8 + 10 * u, C(0.6), 0.2 + 0.5 * u);
    cycle(t, 0.3, 9, (q, i, r) => {
      const a = r.range(0, TAU) + q * 1.5, d = rad + (1 - q) ** 2 * r.range(10, 18), x = Math.cos(a) * d, y = Math.sin(a) * d, k = (d + 5) / d;
      p.line(x, y, x * k, y * k, C(0.4 + q * 0.4)); p.px(x, y, C(0.95)); p.lit(x, y, C(0.9), 0.5 * q);
    }, { seed: o.seed });
    p.disc(0, 0, rad + 1, C(0.45)); p.disc(0, 0, rad, C(0.8 + 0.2 * Math.sin(t * 50))); p.px(0, 0, '#ffffff');
  },
};

export const create = chain([
  { fx: charge, t: 0, until: THROW, x: CX, y: CY },
  { fx: charge, t: THROW, until: THROW + 0.07, x: FROM, y: Y, pop: true },
  { fx: ball, t: THROW, until: HIT, x: (t) => FROM + (t - THROW) * SPEED, y: Y },
  { fx: boom, t: HIT, x: TO - 4, y: Y + 2, ground: -Y - 2 },
]);
export const act = {
  poses: [
    [-0.25, READY],
    [0, { x: -1, crouch: 3, lean: -2, hx: CX, hy: CY, bx: 5, by: -21 }],
    [THROW - 1 / 12, { x: -2, crouch: 4, lean: -3, hx: CX - 1, hy: CY - 1, bx: 6, by: -20 }],
    [THROW, { ...CAST_FORWARD, hx: FROM - 1, hy: Y, ghost: true }, 'snap'],
    [THROW + 0.25, { ...CAST_FORWARD, x: 3, crouch: 4, lean: 5, hx: FROM + 1, hy: Y + 2 }, 'out'],
    [THROW + 0.6, READY],
  ],
  target: { x: TO },
  hits: [{ t: HIT, freeze: 0.08, shake: 0.2, amp: 2, push: 6, flash: 2, tint: '#a8502c' }],
};
```

## moves/ice-lance-throw

Acted move: a frost lance forms in the raised hand, is thrown like a javelin, encases the target in ice and the block shatters with ice-shatter shards.

```js
// id: moves/ice-lance-throw
import { cycle, shade, sparkle, ease, clamp } from './pxfx.mjs';
import { chain, READY, STRIKE_FORWARD } from './pxact.mjs';
import * as shatter from './ice-ice-shatter.mjs';

const THROW = 0.5, SPEED = 260, GX = -9, GY = -31, FROM = 19, FY = -27, TO = 60, TIP = 11;   // grip in the raised hand, then thrown
const HIT = THROW + (TO - 5 - TIP - FROM) / SPEED;                                              // the tip meets the dummy's front
export const meta = {
  name: 'ice-lance-throw', size: [168, 104], origin: [44, 92], duration: 2.6, loop: false, fps: 30,
  options: { palette: 'ice', seed: 2 }, tags: ['move', 'ice', 'projectile', 'throw', 'javelin'],
};
// The lance: a long faceted crystal held at the grip. form: it grows in the hand; pop: the white burst frame.
const lance = {
  meta: { name: 'ice-lance', size: [64, 32], origin: [24, 16], duration: 1, loop: false, fps: 30, options: { palette: 'ice', seed: 1 } },
  draw(p, t, o) {
    const C = (u) => shade(o.palette, u);
    if (o.pop) { p.disc(0, 0, 3, '#ffffff'); sparkle(p, 0, 0, 7, '#ffffff', C(0.8)); p.glow(0, 0, 14, C(1), 0.9); return; }
    const g = o.form ? ease.outBack(clamp(t / 0.35)) : 1, a = -7 * g, b = TIP * g;
    if (b < 1) return;
    p.glow(b * 0.4, 0, 10 + 4 * g, C(0.7), 0.3);
    p.line(a, 0, b, 0, C(0.7)); p.line(a + 1, -1, b - 2, -1, C(1)); p.line(a + 1, 1, b - 3, 1, C(0.45)); p.line(a + 2, 2, b - 6, 2, C(0.2));
    p.line(a - 3, 0, a, 0, C(0.5)); p.px(b + 1, 0, '#ffffff'); p.px(b + 2, 0, C(0.9)); p.lit(b + 1, 0, C(1), 0.8);
    cycle(t, 0.5, 6, (q, i, r) => {
      const x = o.form ? r.range(-9, 13) : a - 2 - q * r.range(10, 24), y = o.form ? r.range(-6, 6) * (1 - q) : r.range(-3, 3) + q * 3;
      if (o.form ? r.chance(0.5) : q < 0.8) sparkle(p, x, y, o.form && q < 0.4 ? 1 : 0, C(1 - q * 0.5), C(0.6));
    }, { seed: o.seed });
  },
};

export const create = chain([
  { fx: lance, t: 0, until: THROW, x: GX, y: GY, form: true },
  { fx: lance, t: THROW, until: HIT, x: (t) => FROM + (t - THROW) * SPEED, y: FY },
  { fx: lance, t: HIT, until: HIT + 0.07, x: TO - 5, y: FY, pop: true },
  { fx: shatter, t: HIT, x: TO, y: 0, w: 17, h: 44 },
]);
export const act = {
  poses: [
    [-1 / 3, READY],
    [0, { x: -1, crouch: 2, lean: -2, hx: GX + 1, hy: GY, bx: 6, by: -24 }],
    [THROW - 1 / 12, { x: -2, crouch: 4, lean: -4, hx: GX, hy: GY - 1, bx: 8, by: -24 }],
    [THROW, { ...STRIKE_FORWARD, hx: FROM - 2, hy: FY, ghost: true }, 'snap'],
    [THROW + 0.25, { ...STRIKE_FORWARD, x: 5, crouch: 6, lean: 6, hx: FROM + 2, hy: -20 }, 'out'],
    [THROW + 0.65, READY],
  ],
  target: { x: TO },
  hits: [{ t: HIT, freeze: 0.08, shake: 0.18, amp: 2, push: 5, flash: 2, tint: '#a8dcff' },
    { t: HIT + 0.5, freeze: 0.06, shake: 0.15, amp: 2, push: 3, tint: null }],
};
```

## moves/sword-combo

Acted three-stroke combo stepping forward: a high slash, a rising backhand and a heavy overhead into the floor, chained from melee/sword-slash (reshaped with `arc`) and the floor impact of melee/hammer-smash, with three freezes that grow heavier.

```js
// id: moves/sword-combo
import { chain, READY, WINDUP_BACK, STRIKE_FORWARD } from './pxact.mjs';
import * as slash from './melee-sword-slash.mjs';
import * as smash from './melee-hammer-smash.mjs';

const G1 = 0.75, G2 = 1.5, H = slash.HIT;   // stroke starts; each bites H after its start
const S = { weapon: 'sword' }, SF = { ...STRIKE_FORWARD, ...S };

export const meta = {
  name: 'sword-combo', size: [136, 84], origin: [44, 66], duration: 3.2, loop: false, fps: 30,
  options: { palette: 'metal', seed: 1 }, tags: ['move', 'melee', 'sword', 'combo'],
};
export const create = chain([
  { fx: slash, t: 0 },                                                   // 1: high-back to front
  { fx: slash, t: G1, x: 4, arc: [6, -21, 25, -10, -2.6, 0.2, -0.12] },  // 2: low-back rising to front
  { fx: slash, t: G2, x: 8, arc: [9, -21, 23, 23, -2.3, 1.15, 0] },      // 3: overhead into the floor
  { fx: smash, t: G2 + H - smash.HIT - 1 / 240, x: 26 - smash.IX, smear: false },  // its impact (half a step early)
]);
export const act = {
  poses: [[-0.5, { ...READY, ...S }], [-0.25, { ...WINDUP_BACK, ...S, hy: -22, wa: -2.9 }], [-1 / 12, { ...WINDUP_BACK, ...S, crouch: 4, hy: -22, wa: -3.1 }],
    [0, { ...SF, x: 1, hx: 12, hy: -27, wa: -1.2, ghost: true }, 'snap'], [1 / 12, { ...SF, hy: -21, wa: 0.1 }],
    [0.25, { ...SF, x: 5, crouch: 5, hx: 14, hy: -14, wa: 0.8 }, 'out'],
    [0.5, { ...SF, x: 4, crouch: 5, lean: -1, hx: -2, hy: -11, bx: -4, by: -14, wa: 2.7 }, 'line'],
    [G1 - 1 / 12, { ...SF, x: 4, crouch: 6, lean: -2, hx: -4, hy: -10, bx: -5, by: -13, wa: 2.9 }],
    [G1, { ...SF, x: 6, hx: 12, hy: -14, wa: 0.5, ghost: true }, 'snap'], [G1 + 1 / 12, { ...SF, x: 7, hy: -22, wa: -0.2 }],
    [G1 + 0.25, { ...SF, x: 7, crouch: 2, hx: 12, hy: -30, wa: -1.1 }, 'out'],
    [G2 - 0.25, { ...SF, x: 8, crouch: 2, lean: -2, hx: -1, hy: -36, bx: -3, by: -32, wa: -2.4 }],
    [G2 - 1 / 12, { ...SF, x: 8, crouch: 4, lean: -3, hx: -3, hy: -35, bx: -4, by: -31, wa: -2.8 }],
    [G2, { ...SF, x: 9, hx: 14, hy: -31, wa: -0.9, ghost: true }, 'snap'], [G2 + 1 / 12, { ...SF, x: 10, crouch: 7, lean: 5, hx: 16, hy: -10, wa: 1.1 }, 'snap'],
    [G2 + 1 / 3, { ...SF, x: 10, crouch: 8, lean: 5, hx: 16, hy: -9, wa: 1.15 }], [G2 + 0.9, { ...READY, ...S, x: 8 }]],
  target: { x: 31 },
  hits: [{ t: H, freeze: 0.09, shake: 0.1, amp: 1, push: 4, flash: 1 }, { t: G1 + H, freeze: 0.09, shake: 0.12, amp: 2, push: 5, flash: 1 },
    { t: G2 + H, freeze: 0.18, shake: 0.4, amp: 4, push: 8, flash: 2 }],
};
```
