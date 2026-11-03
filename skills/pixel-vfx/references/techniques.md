# Techniques

Recipes for common effects. Each snippet is a fragment of a `draw` (pure) or a simulated
`draw`/`update`. All use the `pxfx.mjs` library. Borrow the technique, then make the design
your own.

## Flames from looping noise

Sample `fbm` with a whole-number period in y and scroll it by exactly one period per loop.
Shape it with a profile that is wide at the base and narrow at the tip, subtract the noise,
and quantise the result to the ramp with `shade`. Hard steps, no blending.

```js
const H = 24, P = 4;                          // flame height, noise period
const ph = t / meta.duration;                 // 0..1 over the loop
for (let y = -H; y <= 0; y++) {
  const h = -y / H, half = 7 * (1 - h) ** 0.75 + 0.5;
  for (let x = -8; x <= 8; x++) {
    const n = fbm(x * 0.28, y * 0.16 + ph * P, o.seed, 3, 0, P);
    const edge = 1 - Math.abs(x - Math.sin(ph * TAU * 2 + h * 3) * h * 1.5) / half;
    const heat = edge * (1.15 - h * 0.6) - (n - 0.5) * (0.8 + h * 1.2) - h * 0.35;
    if (heat > 0.08) p.px(x, y, shade(o.palette, clamp(0.15 + heat * 0.95)));
  }
}
```

- More noise near the tip (`h * 1.2`) breaks the cone into tongues.
- A torch is the same with a narrower `half`; a wall of fire repeats it along x with a
  different noise x offset per tongue; a back row in `drawBack` is one step darker.
- Add rising embers with `cycle()` and a flickering `glow` built from `sin(ph * TAU * n)`.

## Sparks

Use `particles(r)` with `kind: 'spark'`. Launch them in a cone, vary speed and life, let them
bounce on the floor.

```js
for (let i = 0; i < 14; i++) {
  const a = -Math.PI * r.range(0.1, 0.9), s = r.range(80, 190);
  ps.spawn(0, -4, Math.cos(a) * s, Math.sin(a) * s, r.range(0.25, 0.6),
    { kind: 'spark', ramp: o.palette, bounce: 0.5 });
}
```

The spark draws a streak behind its head whose length grows with speed, a hot head pixel,
and a small light. For a hand-drawn spark in a pure effect, draw a `line` from
`(x - vx * k, y - vy * k)` to `(x, y)` in a mid step, the head in the top step, and `lit` the head.

## Shockwave rings on the floor

A flat ellipse that races out with `outQuart`, thins from 2 px to 1 px, and dithers out near
the end. Split it: far half in `drawBack`, near half in `draw`, back half one step dimmer.

```js
const ring = (p, half) => {
  const u = clamp(t / 0.5);
  if (u >= 1) return;
  const rx = lerp(6, 56, ease.outQuart(u)), fade = u < 0.55 ? 1 : 1 - (u - 0.55) / 0.45;
  const dim = half === 'back' ? 0.2 : 0;
  p.oval(0, 0, rx, rx * 0.26, shade(o.palette, 0.95 - 0.75 * u - dim), fade, { width: u < 0.3 ? 2 : 1, half });
};
// drawBack(p) { ring(p, 'back'); }   draw(p) { ring(p, 'front'); }
```

An air shockwave (mid-air hit, explosion) is a `ring` instead: round, no halves, with a
second thinner ring a little behind it.

## Crystals and shards

A shard is a thin diamond: two long lines from base to tip, a bright 1 px edge on the lit
side and a dark step on the other. Grow it with `outBack` so it overshoots a little.

```js
const shard = (x, y, len, ang, u) => {
  const L = len * ease.outBack(u), dx = Math.cos(ang), dy = Math.sin(ang);
  const nx = -dy * 1.5, ny = dx * 1.5, tx = x + dx * L, ty = y + dy * L;
  p.line(x + nx, y + ny, tx, ty, shade(o.palette, 0.4));       // dark side
  p.line(x - nx, y - ny, tx, ty, shade(o.palette, 0.95));      // lit edge
  p.line(x, y, tx, ty, shade(o.palette, 0.7));                 // body
  p.lit(tx, ty, shade(o.palette, 1), 0.8);                     // glint at the tip
};
```

Shatter them at the end: stop drawing the shard and spawn 3 to 5 `debris` particles with the
ice ramp along its length. Add mist puffs and a cold floor pool while they stand.

## Lightning

`bolt(r, x0, y0, x1, y1, rough, depth)` splits each segment at its middle and pushes the
middle sideways by a random amount that halves at each level (midpoint displacement).
`forks(r, pts, n)` adds side branches. Seed the rng with a time slot, so the bolt re-rolls
every few frames but stays deterministic.

```js
const r = rng(o.seed * 1000 + Math.floor((t * meta.fps) / 2));   // new shape every 2 frames
const pts = bolt(r, 0, -60, 0, 0, 0.3, 5);
p.polyline(pts.map(([x, y]) => [x - 1, y]), shade(o.palette, 0.5));   // coloured edge
p.polyline(pts, '#ffffff');                                            // white core
for (const f of forks(r, pts, 2)) p.polyline(f, shade(o.palette, 0.7));
for (const [x, y] of pts.filter((_, i) => i % 4 === 0)) p.glow(x, y, 6, shade(o.palette, 0.7), 0.15);
```

- Strike timing: the first 2 frames are the full white bolt plus a big `flare`; then the core
  goes to a ramp colour and the bolt dithers out over 3 to 5 frames. Put `HIT` at the first frame.
- For a loop (a crackling orb), make the slot wrap: `slot % slotsPerLoop`.

## Slash arcs

A crescent: an `arc` band whose width is a function of position along the arc (0 at the
tail, 1 at the head), widest just behind the leading edge. A white leading edge, a
ramp-coloured body, a trail that shrinks from the tail. Centre the arc on the shoulder of the
strike pose and make it already long on the release frame. Effect-contract example 3 is a
complete, acted slash; compute `HIT` from the arc's constants as it does.

## Pickups and sparkles

`sparkle(p, x, y, arm, c, tip)` draws a plus-shaped glint; grow and shrink `arm` on its own
cycle to twinkle. `star` particles twinkle by themselves.

```js
cycle(t, meta.duration, 4, (q, i, r) => {
  const s = Math.sin(q * Math.PI) ** 3;
  if (s > 0.05) sparkle(p, r.int(-8, 8), r.int(-12, 0), Math.floor(s * 3.5), '#ffffff', shade(o.palette, 0.7));
}, { seed: o.seed });
```

For the collect burst: a quick white ring, 6 to 8 `star` particles thrown outward with
`burst`, and a `+1` from `p.text(..., { font: 'big', outline })` that pops up with `outBack`.

## Screen transitions

Use the Bayer threshold directly. A pixel is covered when the wipe value at that pixel beats
its threshold, so the wipe edge dithers cleanly with no extra code.

```js
// A diagonal dither wipe on a 320x180 canvas, origin [0, 0], duration 0.8. It covers the
// screen by MID; the game swaps scenes at MID; then it uncovers in the same direction.
const MID = 0.4, out = t > MID;
const u = out ? ease.inOutSine((t - MID) / (meta.duration - MID)) : ease.inOutSine(t / MID);
for (let y = 0; y < 180; y++) for (let x = 0; x < 320; x++) {
  const d = (x + y) / 500;                            // 0 at top-left, about 1 at bottom-right
  const v = out ? d - (u * 1.6 - 0.6) : u * 1.6 - d;  // > 0 where the screen is covered
  p.px(x, y, '#0b0b12', clamp(v * 4));                 // alpha below 1 dithers the edge
}
```

- Put the swap in `meta.timing`: `hits: [{ t: MID, freeze: 0, shake: 0, amp: 0 }]`, so the
  game knows the frame where the screen is fully covered. The effect then ends empty and
  passes `check` like any one-shot.
- Iris: make `d` the distance to the centre divided by the half-diagonal. Blinds: use
  `(y % 16) / 16`. Pixel dissolve: use `hash(x, y)`.
- On big canvases, one `p.rect` per 4x4 cell with its own alpha is faster than per pixel.

## Projectiles

A head (a small hot disc with a `glow`), a trail of the last positions drawn as a tapering
`line` in darker steps, and a few sparks shed behind. Face right; the game sets `angle`. Make
the flight part a pure loop (so it can fly any distance) and the hit a separate one-shot.

## Auras and status effects

Pure loops. A flat floor ring in two halves (`drawBack` and `draw`), rising motes on a
`cycle`, a slow breathing `glow`. Keep them quiet: an aura sits on a character for a long
time, so use low light and few pixels.

## Weather

Rain: short diagonal 1 to 2 px lines on a `cycle`, each with its own x and speed, wrapping
from top to bottom; a 1 px splash on the floor where each one ends. Snow: single pixels that
drift with `sin` wander. Both are pure loops sized to the screen.

## UI juice

Button press flash (2 frames of white rim, then a ramp step), a damage number pop
(`outBack` scale by redrawing glyph pixels), a coin burst (sparkles plus debris with the gold
ramp). Keep them short (0.15 to 0.4 s) and small.

## The particle kinds in practice

- Gathering energy: `streak` particles spawned on a ring, aimed inward with `to: [x, y]`;
  they accelerate (negative `drag`) and die at the hand. Add `mote`s that orbit and fall in.
- Fireballs and burning debris: `fire` particles (`smokes: true` for a few), `brand` chunks
  whose `onLand` starts a `blazes` fire; `lights()` flares at each landing.
- Frost and mist: `vapour` with `haze` for a coloured glow, `flake` for drifting crystals.
- Ground impacts: `dust` rolls along the floor, `grit` bounces once and settles, `debris`
  tumbles; put far ones in the back layer with `back: true` (drawn by `ps.drawBack`).
- Rings on the floor: `band(p, ..., FAR)` in `drawBack`, `band(p, ..., NEAR)` in `draw`.
- Ice: `crystal` spikes along a `band`; shatter them into `debris` with the ice ramp.

## Recolouring

Draw every colour through `shade(o.palette, u)` or `ramp(o.palette, n)`, never as a fixed
hex inside the effect (white is the one exception). Then a recolour is an option:

```sh
node pxfx.mjs render fx/slash.mjs --opts '{"palette":"ice"}' --out recolour
node pxfx.mjs sheet fx/slash.mjs --opts '{"palette":["#1a0a10","#5c1530","#b02a4a","#f06070","#ffd0d8"]}'
```

Ramps of any length work, because `shade` and `ramp` resample. Use `ramp(p, n, true)` only
when you really need blended in-between colours; it adds colours that are not in the palette.
