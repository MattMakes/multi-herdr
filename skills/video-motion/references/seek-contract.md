# Seek contract reference

This file holds the full frame-seeking contract for video scenes, a scene template, helper code and a self-check. `SKILL.md` gives the summary.

## 1. The contract

A renderer loads `scenes/<scene-id>/index.html` in headless Chrome at the target viewport. Then it does these steps:

1. It waits for `document.fonts.ready` and for every image to decode.
2. It checks `typeof window.__seekToFrame === "function"`.
3. For each frame index `k` it wants, it calls `await window.__seekToFrame(k, fps)`.
4. It captures a screenshot.

The renderer can ask for frames in any order: `0, 1, 2, ...`, a reverse order, or single frames such as `0, 15, 30`. A QA check renders sample frames forward and then in reverse, and it requires the same pixels both times. So the scene obeys these rules:

| Rule | Meaning |
| --- | --- |
| Pure | The frame depends only on `frameIndex` and `fps`. No state carries over from the previous call. |
| Total | Every visual property that ever changes is set on every call. |
| Idempotent | 2 calls with the same arguments give the same pixels. |
| Order-free | Frame 90 after frame 10 equals frame 90 after frame 200. |
| Offline | The scene loads no network resource. |

`window.__videoMeta` gives the renderer the scene size and length:

```js
window.__videoMeta = { width: 1920, height: 1080, fps: 60, duration: 6.0 };
```

`duration` is in seconds. The frame count is `Math.round(duration * fps)`. The last frame index is that count minus 1.

When a seekable scene selects a frame from a raster or video plate, use
`SeekTimeline.plateFrame(t, fps, lastFrame)`. It computes
`Math.min(Math.floor(t * fps + 1e-6), lastFrame)`. The floor is required for
fractional motion-blur seeks, and the clamp prevents the last subframe from
requesting a frame beyond the exported plate. This rule changes only plate
selection; output frame counts still use `Math.round(duration * fps)`.

## 2. Scene template

Write the scene as 1 HTML file. Copy `<skill-dir>/scripts/seek-timeline.js` into the scene folder as `scenes/<scene-id>/seek-timeline.js`. `<skill-dir>` means the directory where the `video-motion` SKILL.md sits. The file has no dependencies and loads nothing from the network.

GSAP is an option only when the user project already ships its own GSAP file. Then load that local file, build `gsap.timeline({ paused: true })`, and seek it with `tl.time(t)`.

```html
<!doctype html>
<html>
<head>
<meta charset="utf-8">
<style>
  html, body { margin: 0; width: 1920px; height: 1080px; overflow: hidden; background: #0B1020; }
  /* No animation: or transition: rules here. All motion comes from the timeline. */
  .title { position: absolute; left: 192px; top: 380px; font: 700 96px/1.1 "Inter", sans-serif; color: #F5F7FA; }
  .word { display: inline-block; margin-right: 0.28em; opacity: 0.45; }
  .word.on { opacity: 1; }
</style>
</head>
<body>
<div class="title" id="title"></div>
<svg id="fx" width="1920" height="1080" style="position:absolute;inset:0"></svg>
<script src="seek-timeline.js"></script>
<script>
(function () {
  var META = { width: 1920, height: 1080, fps: 60, duration: 6.0 };
  window.__videoMeta = META;

  // Tokens copied from MOTION.md. Names match the token names there.
  var ENTER_S = 0.55, EXIT_S = 0.3, STAGGER_S = 0.06;

  // Word timings copied from audio/sync-sheet.json for this scene, in scene time.
  var WORDS = [
    { word: "Ship", start_sec: 0.20, end_sec: 0.48 },
    { word: "every", start_sec: 0.48, end_sec: 0.80 },
    { word: "frame.", start_sec: 0.80, end_sec: 1.30 }
  ];

  var title = document.getElementById("title");
  var spans = WORDS.map(function (w) {
    var s = document.createElement("span");
    s.className = "word";
    s.textContent = w.word;
    title.appendChild(s);
    return s;
  });

  // The ease strings are the ease.enter and ease.exit tokens.
  var master = SeekTimeline.timeline();
  master.fromTo(title, { y: 40, opacity: 0 },
                { y: 0, opacity: 1, duration: ENTER_S, ease: "cubic-bezier(0.22, 1, 0.36, 1)" }, 0.0);
  master.to(title, { opacity: 0, duration: EXIT_S, ease: "cubic-bezier(0.4, 0, 1, 1)" }, META.duration - EXIT_S);

  // Seeded PRNG: same seed, same sequence, on every machine.
  function mulberry32(seed) {
    return function () {
      seed |= 0; seed = (seed + 0x6D2B79F5) | 0;
      var r = Math.imul(seed ^ (seed >>> 15), 1 | seed);
      r = (r + Math.imul(r ^ (r >>> 7), 61 | r)) ^ r;
      return ((r ^ (r >>> 14)) >>> 0) / 4294967296;
    };
  }
  var rand = mulberry32(1234);
  var DOTS = [];
  var fx = document.getElementById("fx");
  for (var i = 0; i < 24; i++) {
    var c = document.createElementNS("http://www.w3.org/2000/svg", "circle");
    c.setAttribute("r", 3);
    c.setAttribute("fill", "#F2C14E");
    fx.appendChild(c);
    DOTS.push({ el: c, x: rand() * 1920, y: rand() * 1080, phase: rand() * Math.PI * 2, speed: 20 + rand() * 30 });
  }

  // Computed state: a function of t only. It is set on every call.
  function renderComputed(t) {
    DOTS.forEach(function (d) {
      d.el.setAttribute("cx", (d.x + d.speed * t) % 1920);
      d.el.setAttribute("cy", d.y + 12 * Math.sin(d.phase + t * 2 * Math.PI * 0.3));
    });
    WORDS.forEach(function (w, i) {
      spans[i].classList.toggle("on", t >= w.start_sec && t < w.end_sec + 0.15);
    });
  }

  window.__seekToFrame = function (frameIndex, fps) {
    var t = frameIndex / fps;
    master.seek(t);
    renderComputed(t);
  };

  // Paint frame 0 so a plain page load already shows the first frame.
  window.__seekToFrame(0, META.fps);
})();
</script>
</body>
</html>
```

## 3. Rules for timelines

- Put every tween at a fixed position: `master.to(el, vars, 1.25)`, a label (`"cta"`, `"cta+=0.2"`), or `"<"`, `">"`, `"+=0.1"`, `"-=0.1"` relative to the previous tween. Do not chain motion by callbacks. A seek can jump over a callback, so a callback must never start motion.
- Use `fromTo()` to give a start state. Before its start time, a `fromTo()` shows its start values, so frame 0 shows the start state.
- Use `set(el, vars, at)` for an instant change at a time.
- The timeline has no clock and no play call. Only `master.seek(t)` moves it.
- Name each beat with `master.addLabel("beat-2", 2.0)` and place its tweens relative to the label.
- For Canvas, pass `SeekTimeline.timeline({ onSeek: function (t) { draw(t); } })`. The callback runs after each seek. Tween a plain object (`{ x: 0 }`) and read it in `draw`.
- Stagger with `stagger: 0.06` (60 ms). Keep a group at `<= 5` items.
- Do not use ScrollTrigger in a video scene. There is no scroll.

## 4. Rules for computed motion

Some motion is easier as a formula than as tweens: noise, particles, idle loops, counters.

- Write it as `f(t)` with `t` in seconds. Call it from `__seekToFrame`.
- Do not integrate state step by step (`x += v`). That breaks the order-free rule. Use a closed form: `x = x0 + v * t`.
- For physics that has no closed form, simulate from `t = 0` up to the target `t` at a fixed step (for example 1/240 s) on every call. Cache results by step index. A cache is allowed because it does not change the result.
- Seed every random choice once at load with `mulberry32(seed)`. Store the results. Never draw a random number inside `__seekToFrame`.
- Round sub-pixel values only when the design needs crisp edges. Use the same rounding on every call.

## 5. CSS and media

- No free-running CSS `animation:` or `transition:`. A rule such as `transition: opacity 0.3s` makes a seek show a half-finished fade.
- If you must use a CSS keyframe animation, pause it and set its time:

```js
var anim = el.getAnimations()[0];
anim.pause();
// in __seekToFrame:
anim.currentTime = t * 1000;   // milliseconds
```

- For a `<video>` plate, set `video.currentTime = t` and return a Promise that resolves on the `seeked` event.
- For a `<canvas>`, clear and redraw the whole canvas on every call.
- Lottie or other players: use their "go to frame" call with the computed frame, and never their play call.

## 6. Forbidden API list

A QA scan searches `scenes/**/*.{html,js,mjs,ts}` for these patterns. Each hit fails the scan unless it is in a library file that the scene does not call at render time.

| Pattern | Why it is forbidden | Use instead |
| --- | --- | --- |
| `requestAnimationFrame` | The browser decides when it runs | `__seekToFrame` |
| `setTimeout`, `setInterval` | Wall-clock delay | A position on the timeline |
| `Date.now`, `performance.now`, `new Date()` | Wall clock | `t = frameIndex / fps` |
| `Math.random` | Different value on each run | `mulberry32(seed)` |
| CSS `animation:` running | Runs on the compositor clock | Paused animation with `currentTime` |
| CSS `transition:` | Runs on the compositor clock | A tween on the timeline |
| `fetch`, remote `<script src="https://...">` | Network timing and offline failure | A local file |

## 7. Motion grammar values

| Token | Default | Notes |
| --- | --- | --- |
| `ease.enter` | `cubic-bezier(0.22, 1, 0.36, 1)` | Fast start, soft landing. `expo.out` is close. |
| `ease.exit` | `cubic-bezier(0.4, 0, 1, 1)` | Slow start, fast leave. `power2.in` is close. |
| `duration.enter` | 400-700 ms | Larger elements take the longer end. |
| `duration.exit` | 200-350 ms | Shorter than the enter. |
| `duration.min` | 100 ms | 6 frames at 60 fps. |
| `stagger.item` | 40-80 ms | Per item. |
| `stagger.max_items` | 5 | Per group. |
| `type.highlight_words` | 1-3 | Words highlighted at the active voice-over time. |
| `focus.dim_opacity` | 0.30-0.40 | Context while a diagram node is active. |
| `focus.max_jump` | 0.35 | Normalized focal distance between scenes without a bridge. |

A spring is allowed in place of the enter bezier. Use a critically damped or slightly under-damped spring (damping ratio 0.7-1.0) and sample it as a function of `t`. Do not let a spring overshoot more than 6% of its travel on text.

## 8. Kinetic typography steps

1. Read the scene's words from `audio/sync-sheet.json` (`vo_segments[].words`). Convert each time to scene time: `word.start_sec - scene.start_sec`.
2. Split the line into word spans once, at load.
3. In `__seekToFrame`, mark the words whose window holds `t` as active. Keep at most 3 active.
4. Animate the active state by opacity, weight or colour. Keep size changes `<= 8%` so the line does not reflow.
5. Lead the highlight by 0-50 ms. A late highlight looks out of sync.

## 9. Self-check before hand-off

Run each step. Fix each failure before you hand the scene to the render step.

1. Search the scene folder for the forbidden patterns in section 6. Expect 0 hits outside library files.
2. Open the scene and run in the console: `typeof window.__seekToFrame` gives `"function"`, and `window.__videoMeta.fps` matches `timeline.json`.
3. Render frames `0, N/2, N-1` forward, then `N-1, N/2, 0`. Compare the 2 sets. They must be byte-identical or have PSNR `>= 45 dB`.
4. Look at frame 0. It must show designed content, not an empty or half-loaded page.
5. Check that each beat has 1 moving focal element and that stagger groups have `<= 5` items.
6. Check every duration and ease against `MOTION.md` tokens.
