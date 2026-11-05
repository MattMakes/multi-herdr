# Idle life for drawn things

A drawn thing that holds perfectly still between moves looks dead. Idle life is a small, slow, endless motion on top of the main animation: a breath, a blink, a sway, a flicker, a ripple. This file gives 1 recipe for each. Every recipe computes its value from `t` (seconds) and a seed alone, so it runs inside `window.__seekToFrame(frameIndex, fps)` and gives the same frame in any render order.

For a character rig, the breath and blink formulas with their `MOTION.md` tokens are in `rig-and-visemes.md`. Use the recipes here for everything else: plants, flames, flags, water, badges, screens, background props.

## Rules for all recipes

- Keep idle motion small. It must not pull the eye from the focal point of the beat.
- Give each copy of a thing its own phase from a seed, so 10 plants do not sway in step.
- Draw random numbers once at load from a seeded PRNG. Never call the PRNG inside the seek call.
- Write each amplitude and frequency as a token in `MOTION.md` before you use it twice.
- Fade idle life in over 0.3 s after a main move ends. Do not stack it on top of a large move.

## Shared helpers

```js
function mulberry32(seed) {
  return function () {
    seed = (seed + 0x6d2b79f5) | 0;
    var r = Math.imul(seed ^ (seed >>> 15), 1 | seed);
    r = (r + Math.imul(r ^ (r >>> 7), 61 | r)) ^ r;
    return ((r ^ (r >>> 14)) >>> 0) / 4294967296;
  };
}
var TAU = 2 * Math.PI;
// Smooth value noise in [-1, 1]: random values at whole numbers, joined by a smooth curve.
function makeNoise(seed, size) {
  var rand = mulberry32(seed), v = [];
  for (var i = 0; i < size; i++) v.push(rand() * 2 - 1);
  return function (x) {
    var i = Math.floor(x), f = x - i, s = f * f * (3 - 2 * f);
    var a = v[((i % size) + size) % size], b = v[(((i + 1) % size) + size) % size];
    return a + (b - a) * s;
  };
}
```

## Breath

A slow swell of size. Use it on a badge, a glowing button, a sleeping prop.

```js
// hz 0.2-0.4, amount 0.01-0.03 of scale. phase in [0, 1) from the seed.
function breathe(t, hz, amount, phase) {
  return 1 + amount * Math.sin(TAU * (hz * t + phase));
}
```

Scale from the bottom edge (`transform-origin: 50% 100%`) so the thing grows up, not out of its centre.

## Blink

A short close and open at uneven times. Use it on a status light, a cursor, a screen that flicks off.

```js
// Draw the blink start times once, between minGap and maxGap seconds apart.
function blinkTimes(seed, length, minGap, maxGap) {
  var rand = mulberry32(seed), out = [], at = minGap * rand();
  while (at < length) { out.push(at); at += minGap + (maxGap - minGap) * rand(); }
  return out;
}
// 1 is open, 0 is closed. close and open are in seconds, 0.05-0.08 each.
function openness(t, times, close, open) {
  for (var i = 0; i < times.length; i++) {
    var d = t - times[i];
    if (d >= 0 && d < close) return 1 - d / close;
    if (d >= close && d < close + open) return (d - close) / open;
  }
  return 1;
}
```

## Sway

A slow turn back and forth around a fixed base. Use it on grass, plants, hanging signs, flags.

```js
// Two sine waves at a ratio that is not a whole number never line up, so the sway does not loop visibly.
function sway(t, hz, degrees, phase) {
  return degrees * (0.7 * Math.sin(TAU * (hz * t + phase)) + 0.3 * Math.sin(TAU * (hz * 1.618 * t + phase * 2)));
}
```

Pivot at the base (`transform-origin: 50% 100%`). Use 0.15-0.4 Hz and 1-4 degrees. For a tall stem, split it into 2 or 3 parts and give each upper part the sway of the part below with a delay of 0.1-0.2 s, so the tip trails the base.

## Flicker

An uneven change of brightness. Use it on a flame, a neon sign, a candle, an old screen.

```js
// base 0.85-0.95, depth 0.05-0.15, speed 6-12 noise steps per second.
function flicker(t, noise, base, depth, speed) {
  var slow = noise(t * speed), fast = noise(t * speed * 2.3 + 50);
  return Math.min(1, base + depth * (0.65 * slow + 0.35 * fast));
}
```

Apply the value to opacity or to a glow radius. For a flame, also move its tip sideways by 1-3 px with a second noise from another seed. A neon sign that drops out uses `openness` from Blink with long gaps, not this recipe.

## Ripple

Rings that grow and fade from a point. Use it on water, a tap on a screen, a sound wave, a radar.

```js
// Ring k starts at k * gap seconds, grows to maxRadius over life seconds, and fades out.
function ripples(t, gap, life, maxRadius) {
  var rings = [], first = Math.max(0, Math.ceil((t - life) / gap));
  for (var k = first; k * gap <= t; k++) {
    var age = (t - k * gap) / life;
    var grow = 1 - Math.pow(1 - age, 3); // fast out, slow at the edge
    rings.push({ radius: maxRadius * grow, opacity: 1 - age });
  }
  return rings;
}
```

The ring list depends only on `t`, so a seek to any frame draws the right rings. Use a gap of 0.6-1.5 s and keep at most 3 rings alive. For a water surface, offset a line of points by `amplitude * Math.sin(TAU * (x / wavelength - hz * t))` instead.

## Check

1. Render 3 frames of an idle stretch forward, then in reverse. The images are byte-identical.
2. Step through 1 s frame by frame. No idle motion jumps between 2 frames.
3. Look at a group of 3 or more copies. No 2 copies move in step.
