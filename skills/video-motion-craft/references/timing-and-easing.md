# Timing and easing tables

All times are in milliseconds unless a unit is given. Convert to frames with `frames = round(ms * fps / 1000)` and never go below 1 frame for a step or 6 frames at 60 fps for a visible move.

## Duration by what moves

| Element | Duration |
|---|---|
| Tooltip, cursor change | 80 to 120 |
| Button, toggle | 120 to 180 |
| Icon | 150 to 250 |
| Card, list row | 200 to 350 |
| Panel, modal | 300 to 400 |
| Full-screen change | 400 to 600 |
| Dramatic reveal | 600 to 1200 |
| Ambient loop | 2 to 20 s |

Scale by distance. Take a 100 px move at 1920 px width as 1.0:

| Distance | 50 px | 100 px | 200 px | 300 px | 400 px | full frame |
|---|---|---|---|---|---|---|
| Factor | 0.8 | 1.0 | 1.3 | 1.5 | 1.6 | 1.8 to 2.0 |

Scale by material when the thing has one: rigid 1.2, elastic 0.8, fluid 1.5, paper 1.0, gas 2.0, glass 0.9.

Measured medians in professional films, at 30 fps: translate 12 frames, scale 10, rotation 11, opacity 6, blur 7. A call-to-action group moves fast (about 6 frames); a hero mark takes 18 to 30.

## Easing catalogue

Pick by role. The names are for `MOTION.md` tokens.

| Token | Curve | Use |
|---|---|---|
| `land-soft` | `cubic-bezier(0.22, 1, 0.36, 1)` | the default arrival |
| `land-hard` | `cubic-bezier(0.16, 1, 0.3, 1)` | a fast arrival with a long tail |
| `land-quiet` | `cubic-bezier(0.33, 1, 0.68, 1)` | a small arrival |
| `travel` | `cubic-bezier(0.65, 0, 0.35, 1)` | A to B on screen, both ends visible |
| `travel-strong` | `cubic-bezier(0.83, 0, 0.17, 1)` | a big move that should feel deliberate |
| `leave` | `cubic-bezier(0.32, 0, 0.67, 0)` | an exit |
| `leave-hard` | `cubic-bezier(0.7, 0, 0.84, 0)` | an exit that ends on a cut |
| `standard` | `cubic-bezier(0.4, 0, 0.2, 1)` | a sober interface move |
| `emphasis` | `cubic-bezier(0.2, 0, 0, 1)` | a sober move with a fast start |
| `pop` | `cubic-bezier(0.34, 1.56, 0.64, 1)` | 1 playful overshoot |
| `breathe` | sine in-out | every ambient loop |
| `constant` | linear | rotation, progress, scroll, a timer |

How to read a curve in a reference clip: measure the share of travel done at 1/3 of the time. Over 1/2 is an ease-out. Under 1/4 is an ease-in. Even on both sides is in-out. Equal steps are linear. Passing the end and coming back is a spring or a back curve.

How much of the travel is done at 10, 25, 50 and 75 % of the time, for the shapes that professional films use most:

| Shape | 10 % | 25 % | 50 % | 75 % |
|---|---|---|---|---|
| Soft landing (the workhorse) | 30 | 59 | 85 | 95 |
| Snap and settle | 54 | 84 | 96 | 98 |
| Glide | 16 | 39 | 69 | 91 |
| Soft in-out | 4 | 17 | 50 | 83 |
| Gentle start | 1 | 6 | 24 | 55 |
| Accelerating exit | 0 | 3 | 16 | 47 |
| Crash out | 0 | 0 | 6 | 32 |

Fit a custom bezier to a row when the catalogue has no match.

## Springs in closed form

A spring in a seek-safe scene is a formula of `t`. For an under-damped spring from 0 to 1:

```js
// zeta: damping ratio (0 < zeta < 1). omega: natural frequency in rad/s.
function spring(t, zeta, omega) {
  if (t <= 0) return 0;
  const wd = omega * Math.sqrt(1 - zeta * zeta);
  const decay = Math.exp(-zeta * omega * t);
  return 1 - decay * (Math.cos(wd * t) + (zeta * omega / wd) * Math.sin(wd * t));
}
```

Choose the 2 numbers from what you want to see:

| Overshoot | 20 % | 10 % | 5 % | 2 % |
|---|---|---|---|---|
| `zeta` | 0.46 | 0.59 | 0.69 | 0.78 |

Settle time is about `4 / (zeta * omega)` seconds, so `omega = 4 / (zeta * settle_sec)`. Example: 10 % overshoot that settles in 0.6 s gives `zeta = 0.59`, `omega = 11.3`.

With mass 1, stiffness is `omega * omega` and damping is `2 * zeta * omega`. Rough families, as stiffness and damping: very stiff 400 and 25 to 30; standard 250 to 350 and 18 to 24; bouncy 150 to 250 and 10 to 15; gentle 100 to 150 and 20 to 25.

End the tween when the curve is within 0.1 % of 1, and set the exact end value on the last frame.

A lag that follows a target ("x moves 10 % of the way each frame") depends on the frame before, so it breaks the seek contract. Replace it with an exponential in closed form, `x = target - (target - start) * Math.exp(-k * t)`, or precompute the path.

## Words

- Move whole words. Gaps between words: 2 to 4 frames at 30 fps for a hook, 4 to 6 for a headline, 8 to 12 when a voice leads. Vary each gap by up to 40 % with a seeded value.
- Leave 1.5 to 2 times the gap before the word that carries the point.
- A word rises 3 to 7 % of the frame height. Later words rise less.
- A 3 to 6 px blur that clears in 4 to 8 frames softens the arrival.
- On-screen words land 0 to 2 frames before the voice says them. Lock them to `audio/sync-sheet.json`.
- A caption that pops with the voice: scale 0 to 1.15 to 1.0 in about 8 frames, 2 or 3 frames after the sound.

## Type that holds

- Headline height 7 to 11 % of the frame height. Supporting text 5 to 7 %. A hero number 22 to 29 %, once per film.
- Interface text in a captured or rebuilt screen is at least 4 % of the frame height, or the shot is too wide.
- Sentence case, weight 400 to 550, tracking slightly negative for large sizes. Ink is a dark grey or the brand's dark, not pure black.

## Counters, presses, small things

- A counter steps 1 value per frame with steps that shrink toward the end. Use a round value for a claim and an exact one for data.
- A press: squeeze to 0.71 to 0.9 over 2 frames. The target reacts 0 to 1 frame later. Release slower, with at most a 5 % rebound. Brighten the target. No ripple ring.
- A check mark: the container scales 0.9 to 1 on `pop` in 200 ms; the stroke draws in 150 ms, starting 100 ms later.
- A shake for an error: 300 to 400 ms, 2 or 3 swings of 10 to 15 px that decay, no overshoot at the end.

## Ambient loops

| Loop | Amount | Period |
|---|---|---|
| Breathe | scale 0.98 to 1.02 | 2 to 4 s |
| Float | 5 to 15 px | 3 to 5 s |
| Gradient drift | hue or position | 8 to 20 s |
| Spinner | linear | 1 to 1.5 s per turn |
| Shimmer | 1 sweep | 1.5 to 2.5 s, then 2 to 5 s of rest |
| Sparkle | 1 flash | 0.5 to 1.5 s |

- Give neighbours different periods (for example 4.0, 5.5 and 3.5 s) and phase offsets of 30 % and 60 %, so that they never line up.
- Above 5 % of scale, a loop asks for attention. Keep ambient motion under 20 elements.
- Compute a loop as `Math.sin(2 * Math.PI * (t / period + phase))`. It needs no state.

## Recipes

| Beat | Sequence |
|---|---|
| Panel opens | backdrop 200 ms; panel scale 0.95 to 1 from 50 ms, 300 ms; title at 200 ms; body at 280 ms; actions at 350 ms |
| Dashboard builds | frame at 100 ms; the hero figure at 100 ms for 250 ms; widgets at 60 ms steps; the chart at 350 ms; 1 ambient pulse from 650 ms |
| Card arrives | rise 20 px with a 10 px arc; shadow 50 ms late; content 100 ms late |
| Logo lands | scale from about 3.5 to 1.0; 40 % of the drop in frame 1, most by frame 6, rest by frame 20; then a slow creep |
