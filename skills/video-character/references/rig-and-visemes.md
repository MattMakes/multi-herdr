# Rig and viseme reference

This file holds the full rig tree, the `MOTION.md` character tokens, the micro-motion formulas, the lip-sync data formats and a rig check. `SKILL.md` gives the summary. All code is plain JavaScript that runs inside `window.__seekToFrame(frameIndex, fps)`.

## 1. MOTION.md character tokens

Add a `## Character` section to `MOTION.md`. Every scene reads these values by name.

```markdown
## Character

- id: pip
- personality: curious, calm, quick to celebrate
- palette.body: #3BA5F5
- palette.belly: #E8F4FD
- palette.line: #0B1020
- line.width_px: 6
- proportion.head_to_body: 0.55
- seed: 7311
- breath.hz: 0.3
- breath.scale_y: 0.015
- breath.lift_px: 2
- blink.interval_s: [2.5, 5.0]
- blink.close_ms: 60
- blink.open_ms: 60
- anticipation.frames: 3
- squash.max: 0.12
- follow.lag_frames: 4
- follow.damping: 0.45
- state.blend_ms: 220
- mouth.hold_frames: 2
```

The `id` is a project name for the character. Pick a new, original design and name for each project.

## 2. Full rig tree

```text
rig-root                 pivot: feet centre        whole-body position, squash
  rig-shadow             pivot: centre             scales with jump height
  rig-torso              pivot: torso base         breathing, lean
    rig-belly
    rig-head             pivot: neck               tilt, nod
      rig-eyes           pivot: between the eyes   gaze offset
        rig-eye-l        pivot: eye centre
          rig-lid-l      pivot: top of eye         blink (scale Y)
        rig-eye-r
          rig-lid-r
      rig-brows
        rig-brow-l       pivot: inner end          raise, frown
        rig-brow-r
      rig-mouth          pivot: mouth centre
        rig-mouth-rest
        rig-mouth-open
        rig-mouth-wide
        rig-mouth-round
        rig-mouth-closed
    rig-limbs
      rig-arm-l          pivot: shoulder
        rig-hand-l       pivot: wrist
      rig-arm-r
        rig-hand-r
      rig-leg-l          pivot: hip
      rig-leg-r
  rig-props
    rig-ear-l            pivot: ear base           follow-through
    rig-ear-r
    rig-badge            pivot: pin point          follow-through
```

Set pivots in SVG like this:

```css
#rig-head  { transform-box: fill-box; transform-origin: 50% 100%; }
#rig-arm-l { transform-box: fill-box; transform-origin: 90% 10%; }
#rig-lid-l { transform-box: fill-box; transform-origin: 50% 0%; }
```

Apply transforms with `el.style.transform = "translate(...) rotate(...) scale(...)"` on every call. Keep the order translate, rotate, scale on every group so that values compose the same way each time.

For Canvas, draw the same tree with `ctx.save()`, `ctx.translate(pivot)`, `ctx.rotate()`, `ctx.scale()`, `ctx.translate(-pivot)`, draw, then `ctx.restore()`. Clear the canvas at the start of each call.

## 3. Formulas

All times are seconds. `t = frameIndex / fps`. `T` holds the tokens.

### Breathing

```js
function breath(t) {
  var w = Math.sin(2 * Math.PI * T.breath_hz * t);   // -1 .. 1
  return { scaleY: 1 + T.breath_scale_y * w, liftPx: -T.breath_lift_px * (w + 1) / 2 };
}
```

0.3 Hz gives 1 breath per 3.33 s. Keep the frequency in 0.25-0.4 Hz.

### Blink schedule

Draw the blink times once at load. Never draw them inside the seek call.

```js
function mulberry32(seed) {
  return function () {
    seed |= 0; seed = (seed + 0x6D2B79F5) | 0;
    var r = Math.imul(seed ^ (seed >>> 15), 1 | seed);
    r = (r + Math.imul(r ^ (r >>> 7), 61 | r)) ^ r;
    return ((r ^ (r >>> 14)) >>> 0) / 4294967296;
  };
}
var rand = mulberry32(T.seed);
var BLINKS = [];
for (var bt = 0.8; bt < DURATION_S; ) {
  BLINKS.push(bt);
  bt += 2.5 + rand() * 2.5;          // 2.5-5.0 s interval
}

function lidScale(t) {                // 1 = open, 0.05 = closed
  var close = T.blink_close_ms / 1000, open = T.blink_open_ms / 1000;
  for (var i = 0; i < BLINKS.length; i++) {
    var d = t - BLINKS[i];
    if (d < 0) break;
    if (d < close) return 1 - 0.95 * (d / close);
    if (d < close + open) return 0.05 + 0.95 * ((d - close) / open);
  }
  return 1;
}
```

The lid group scales in Y from its top pivot. A scale of 0.05 keeps a thin line, which reads better than 0.

### Anticipation

For a primary move from value `a` to value `b` that starts at `t0` and lasts `dur` seconds:

```js
function withAnticipation(t, t0, dur, a, b, fps) {
  var pre = T.anticipation_frames / fps;         // 2-4 frames
  var back = a - (b - a) * 0.15;                  // 15% counter-move
  if (t < t0 - pre) return a;
  if (t < t0) return lerp(a, back, easeOut((t - (t0 - pre)) / pre));
  if (t < t0 + dur) return lerp(back, b, easeEnter((t - t0) / dur));
  return b;
}
```

`easeEnter` samples `cubic-bezier(0.22, 1, 0.36, 1)`. `SeekTimeline.ease("cubic-bezier(0.22, 1, 0.36, 1)")` from the `video-motion` skill's `seek-timeline.js` returns this exact function.

### Squash and stretch

For an impact at `tHit` with strength `k` in [0, 1]:

```js
function squash(t, tHit, k) {
  var d = t - tHit;
  if (d < 0 || d > 0.25) return { sx: 1, sy: 1 };
  var s = -T.squash_max * k * Math.exp(-d * 18) * Math.cos(d * 40);  // decays in about 250 ms
  return { sx: 1 / (1 + s), sy: 1 + s };          // sx * sy = 1, so area stays the same
}
```

`|s|` never exceeds `T.squash_max` (0.12, which is 12%). Apply it to `rig-root` with the pivot at the feet.

### Follow-through

A secondary part copies its parent's motion with a lag and less amplitude:

```js
function follow(valueAt, t, fps) {
  var lag = T.follow_lag_frames / fps;            // 3-6 frames
  return valueAt(t - lag) * (1 - T.follow_damping);
}
// Example: ear rotation follows head rotation.
ear.style.transform = "rotate(" + follow(headAngle, t, fps) + "deg)";
```

`valueAt` must itself be a pure function of `t`, so this stays order-free.

## 4. Reaction states

Store cues beside the scene in `cues.json`:

```json
{ "cues": [
  { "t": 0.0, "state": "idle" },
  { "t": 1.2, "state": "point", "target": { "x": 0.72, "y": 0.40 } },
  { "t": 3.6, "state": "celebrate" },
  { "t": 5.0, "state": "talk" }
] }
```

Each state is a pose table: 1 target value per layer property.

```js
var POSES = {
  idle:      { head: 0,   armL: 0,    armR: 0,   brow: 0,  lean: 0 },
  point:     { head: -4,  armL: 0,    armR: -70, brow: 2,  lean: 3 },
  celebrate: { head: 0,   armL: -150, armR: 150, brow: 4,  lean: 0 },
  think:     { head: 8,   armL: 0,    armR: -40, brow: 3,  lean: -2 },
  warn:      { head: -2,  armL: 20,   armR: -20, brow: -5, lean: -5 },
  talk:      { head: 0,   armL: 0,    armR: -10, brow: 1,  lean: 0 }
};
```

At time `t`, find the last cue at or before `t` and the cue before it. Blend from the previous pose to the current pose over `T.state_blend_ms` (150-300 ms) with the enter ease, after the anticipation frames. Add breathing and blinks on top in every state. For `point`, also offset `rig-eyes` toward `target` by at most 6 px.

## 5. Lip-sync data

### Option A: amplitude envelope

Precompute 1 RMS value per video frame. `astats` measures each audio frame. Set the audio frame to 1 video frame of samples with `asetnsamples`:

```sh
# 48 kHz audio, 60 fps video: 800 samples per frame
ffmpeg -i audio/vo-s03-explain.wav -af \
  "aresample=48000,asetnsamples=n=800:p=0,astats=metadata=1:reset=1,ametadata=print:key=lavfi.astats.Overall.RMS_level:file=rms.txt" \
  -f null -
```

Convert `rms.txt` to `envelope.json`: an array of dBFS values, 1 per frame, starting at the scene's first frame. Offset by the voice-over start in scene time. Then map:

| RMS level | Viseme |
| --- | --- |
| below -45 dBFS | `rest` |
| -45 to -28 dBFS | `open` |
| above -28 dBFS | `wide` |

Tune the 2 thresholds per voice by looking at 5-10 frames. This option needs no phoneme data and works with any TTS.

### Option B: viseme table

Build `visemes.json` from word timings in `audio/sync-sheet.json`: `vo_segments[].words[]`, each with `word`, `start_sec` and `end_sec` in project time. Subtract the scene `start_sec` for scene time. Use `rest` between `vo_segments[].phrases`. When `timing` is `estimated`, word times inside a phrase are approximate: let the amplitude envelope decide open or closed. Map each word to a short viseme run by its letters:

| Viseme | Letters or sounds | Shape |
| --- | --- | --- |
| `rest` | silence | Mouth relaxed, lips touch |
| `open` | most consonants, short vowels (a, e, i) | Small opening |
| `wide` | long open vowels, stressed syllables | Large opening |
| `round` | o, u, w, oo | Lips forward, round |
| `closed` | m, b, p | Lips pressed |

```json
{ "fps": 60, "frames": [
  { "frame": 0,  "viseme": "rest" },
  { "frame": 12, "viseme": "closed" },
  { "frame": 14, "viseme": "open" },
  { "frame": 19, "viseme": "round" },
  { "frame": 25, "viseme": "rest" }
] }
```

Each entry holds until the next entry. In the seek call, binary-search the last entry with `frame <= frameIndex`, then show only that mouth child.

### Rules for both options

- Hold each shape for at least `T.mouth_hold_frames` (2 frames). Drop a shorter run.
- Return to `rest` or `closed` within 3 frames after a word ends.
- Lead the mouth by 1-2 frames before the sound. A late mouth looks wrong sooner than an early one.
- Use the `talk` state while the voice-over plays, so arms and head also move a little.

## 6. Rig check

1. Every animated group has an id that starts with `rig-` and an explicit `transform-origin`.
2. Rotate each limb by 30 degrees in the console. It turns around its joint, not around the page corner.
3. Search the scene for `Math.random`, `Date.now`, `performance.now`, `setTimeout`, `setInterval` and `requestAnimationFrame`. Expect 0 hits.
4. Render frames 0, 60, 120 forward and then in reverse. The images match.
5. Step through 1 blink frame by frame. It lasts about 7 frames at 60 fps.
6. Check 1 impact: the largest squash is `<= 12%`, and width times height stays about 1.
7. Play 1 voice-over line. The mouth never shows a shape for 1 frame only.
8. Compare the character in 2 scenes. Colours, line width and proportions match the `MOTION.md` tokens.
