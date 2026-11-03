# Moves: effects made of effects

A move is a chain of effects on one timeline: charge in the hand, a projectile that flies,
a burst on the target. `chain(cues)` from `./pxact.mjs` returns a `create(o)`, so from outside
a move is an ordinary simulated effect: it renders, checks, acts and exports.

```js
// fire-throw: gather fire in the hand, throw a fireball, burst on the target.
import { chain, CAST_FORWARD } from './pxact.mjs';
import * as charge from './arcane-spell-charge.mjs';
import * as ball from './fire-fireball.mjs';
import * as boom from './explosions-small-explosion.mjs';

const THROW = 0.35, SPEED = 220, FROM = 16, TO = 60;
const HIT = THROW + (TO - FROM) / SPEED;                  // from constants, never eyeballed

export const meta = { name: 'fire-throw', size: [136, 96], origin: [34, 84], duration: 1.6,
  loop: false, fps: 30, options: { palette: 'fire', seed: 1 }, tags: ['move', 'fire'] };
export const create = chain([
  { fx: charge, t: 0, until: THROW, x: 16, y: -24 },        // a loop: give it an until
  { fx: ball, t: THROW, until: HIT, x: (t) => FROM + (t - THROW) * SPEED, y: -24 },
  { fx: boom, t: HIT, x: TO, y: -20 },
]);
export const act = {
  poses: [[-0.25, {}], [0, CAST_FORWARD], [THROW, { ...CAST_FORWARD, x: 2, hx: 19 }, 'snap'], [1, {}]],
  target: { x: TO },
  hits: [{ t: HIT, freeze: 0.08, shake: 0.2, amp: 2, push: 6, flash: 2, tint: '#ff7040' }],
};
```

## Cues

`{ fx, t, until, x, y, flip, ...options }`:

- `fx`: an effect module (`import * as name`).
- `t`: start time in the move. Before it the cue is not created.
- `until`: when it stops drawing (default: its own duration for one-shots, never for loops;
  give loops an `until`).
- `x`, `y`: placement in the move's local space, a number, or a function of time since the move began.
  A projectile needs nothing special; give it a moving x: `x: (t) => 20 + (t - THROW) * SPEED`.
- `flip`: mirror the cue. Cues live inside the move's own space, so flipping or turning the
  move flips and turns every cue with it.
- Anything else is an option for that cue (`palette`, `light`, `size` options and so on).
  Cue i gets `seed: moveSeed + i` unless it sets its own; `o.actor` is passed on.

Each cue runs its own clock from 0 at its start, so a reused effect behaves exactly as it
does alone, and its own hit constants stay valid: the move's hit time is cue start plus the
effect's `HIT`.

## Reusing acts

`shiftKeys(keys, dt, dx)` copies act keys later by `dt` and `dx` px further on (the
character stepped in); `shiftHits(hits, dt)` does the same for hits. A combo of two strokes:

```js
const k = slash.act.poses, GAP = 0.75;
export const act = {
  poses: [...k.slice(0, -1), ...shiftKeys(k.slice(2), GAP, 2)],   // skip the first settle
  target: { x: 28 },
  hits: [...slash.act.hits, ...shiftHits(slash.act.hits, GAP)],
};
```

Leave enough time between the follow-through of one stroke and the wind-up of the next:
`check` fails a hand that jumps more than 10 px in one 12 fps step.

## Checks and tips

- `check` validates every cue's `meta` first (`move cues`), then the move as a whole.
- Size the move's canvas for the whole flight; `meta.duration` must cover the last cue's
  smoke, so the move ends empty.
- Library moves import their parts as `./<category>-<name>.mjs`; `lib get` extracts the
  parts with them.
