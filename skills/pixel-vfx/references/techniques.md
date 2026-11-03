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

Midpoint displacement: start with the two end points, split each segment at its middle and
push the middle sideways by a random amount that halves at each level. Seed the rng with the
time slot, so the bolt re-rolls every few frames but stays deterministic.

```js
function bolt(x0, y0, x1, y1, seed, rough = 0.35, levels = 5) {
  const r = rng(seed); let pts = [[x0, y0], [x1, y1]], amp = Math.hypot(x1 - x0, y1 - y0) * rough;
  for (let l = 0; l < levels; l++, amp /= 2) {
    const out = [pts[0]];
    for (let i = 1; i < pts.length; i++) {
      const [ax, ay] = pts[i - 1], [bx, by] = pts[i], d = Math.hypot(bx - ax, by - ay) || 1;
      const off = r.range(-amp, amp);
      out.push([(ax + bx) / 2 - ((by - ay) / d) * off, (ay + by) / 2 + ((bx - ax) / d) * off], pts[i]);
    }
    pts = out;
  }
  return pts;
}
const slot = Math.floor(t * meta.fps / 2);        // new shape every 2 frames
const pts = bolt(0, -60, 0, 0, o.seed * 1000 + slot);
for (let i = 1; i < pts.length; i++) {
  const [ax, ay] = pts[i - 1], [bx, by] = pts[i];
  p.line(ax - 1, ay, bx - 1, by, shade(o.palette, 0.5));     // coloured edge
  p.line(ax, ay, bx, by, '#ffffff');                          // white core
  p.glow(bx, by, 6, shade(o.palette, 0.7), 0.15);             // glow along the line
}
```

- Branches: at 2 or 3 random points, run a shorter `bolt` off to the side with fewer levels.
- Strike timing: the first 2 frames are the full white bolt plus a big flare; then the core
  goes to a ramp colour and the bolt dithers out over 3 to 5 frames. Put `HIT` at the first frame.
- For a loop (a crackling orb), make the slot wrap: `slot % slotsPerLoop`.

## Slash arcs

A crescent: an `arc` band whose width is a function of position along the arc, widest just
behind the leading edge and tapering to 0 at the tail. A white leading edge, a ramp-coloured
body, and a trail that fades from the tail first.

```js
const SWING = 0.12, HIT = 0.06;                   // swing time; the hit lands mid-swing
const u = clamp(t / SWING), head = lerp(-2.2, 1.0, ease.outQuart(u));   // leading angle
const tail = head - lerp(0.4, 2.2, ease.outQuad(u)) * (1 - clamp((t - SWING) / 0.2));
const width = (k) => 1 + 5 * Math.sin(Math.PI * k ** 1.6);          // k: 0 tail -> 1 head
if (head > tail) {
  p.arc(0, -16, 22, tail, head, width, shade(o.palette, 0.7), 1 - clamp((t - SWING) / 0.25));
  p.arc(0, -16, 22, head - 0.25, head, 2, '#ffffff');                   // leading edge
  p.glow(Math.cos(head) * 20, -16 + Math.sin(head) * 20, 10, shade(o.palette, 0.8), 0.5 * (1 - u));
}
```

It faces right, so the caller flips it for a left swing. Add 3 to 5 sparks at the hit point
on `HIT` and a short, bright line along the cut.

## Pickups and sparkles

A sparkle is a plus-shaped cross whose arms grow and shrink: centre pixel in white, arms in
the top ramp steps, a `lit` centre. Twinkle each one on its own cycle.

```js
const sparkle = (x, y, s) => {                    // s: 0..1 size
  if (s <= 0.05) return;
  const arm = Math.round(s * 3);
  p.px(x, y, '#ffffff'); p.lit(x, y, shade(o.palette, 1), s);
  for (let k = 1; k <= arm; k++) {
    const c = shade(o.palette, 1 - k / (arm + 1));
    p.px(x + k, y, c); p.px(x - k, y, c); p.px(x, y + k, c); p.px(x, y - k, c);
  }
};
cycle(t, meta.duration, 4, (q, i, r) => sparkle(r.int(-8, 8), r.int(-12, 0), Math.sin(q * Math.PI) ** 3), { seed: o.seed });
```

For the collect burst: a quick white ring, 6 to 8 sparkles thrown outward that shrink, and
an upward rising `+1`-style shimmer (draw your own glyph pixels).

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

## Recolouring

Draw every colour through `shade(o.palette, u)` or `ramp(o.palette, n)`, never as a fixed
hex inside the effect (white is the one exception). Then a recolour is an option:

```sh
node pxfx.mjs render fx/slash.mjs --opts '{"palette":"ice"}' --out recolour
node pxfx.mjs sheet fx/slash.mjs --opts '{"palette":["#1a0a10","#5c1530","#b02a4a","#f06070","#ffd0d8"]}'
```

Ramps of any length work, because `shade` and `ramp` resample. Use `ramp(p, n, true)` only
when you really need blended in-between colours; it adds colours that are not in the palette.
