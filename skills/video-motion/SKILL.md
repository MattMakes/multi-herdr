---
name: video-motion
description: Use when building a video scene as HTML, CSS, SVG, Canvas or GSAP that a headless browser renders frame by frame - the window.__seekToFrame contract, paused timelines, seeded randomness, motion grammar, kinetic typography and step-by-step diagram reveals.
---

# Deterministic video motion

A video scene is a web page that a renderer photographs one frame at a time. The renderer does not let time pass. It calls a function with a frame number, waits, and takes a screenshot. So every pixel of frame `k` must come from `k` and `fps` alone. When this holds, a cold forward render, a reverse render and a re-render of one frame give the same image.

## When to use

- You build `scenes/<scene-id>/index.html` for a video project.
- You add motion, kinetic type, a diagram walkthrough or particles to such a scene.
- A render shows jitter, dropped motion, or frames that differ between 2 renders.

For a character or mascot rig, also use the `video-character` skill. To copy the motion of a reference clip, also use the `video-reanimate` skill. For general GSAP API detail, use the `motion-gsap` skill, but the rules in this skill win for video.

## Inputs

- `timeline.json`: scene id, `start_s`, `duration_s`, `focal_point`, `text_overlays`, transitions.
- `direction.md` and `BRAND.md` when present: colour, type and motion tokens.
- `MOTION.md`: motion grammar tokens and character identity tokens.
- `audio/sync-sheet.json`: word timings (`words[].start_s`) and beats (`beats_s`).

## The seek contract

Read `references/seek-contract.md` for the full contract, a complete scene template and a self-check.

1. Every scene exposes `window.__seekToFrame = function (frameIndex, fps) { ... }`.
   - It is a pure function of `(frameIndex, fps)`. The same arguments give the same frame, in any call order.
   - It computes `t = frameIndex / fps` in seconds and sets all visual state from `t`.
   - It returns nothing, or a Promise that resolves when the frame is painted.
2. Every GSAP timeline runs paused.
   - Build it with `gsap.timeline({ paused: true })`. If other code starts the GSAP ticker, stop it with `gsap.ticker.sleep()` so that only manual seeks move time.
   - Seek it with `tl.time(frameIndex / fps)`. Do not call `tl.play()`.
   - Use 1 master timeline per scene. Nest child timelines with `master.add(child, atSeconds)`.
3. These APIs are forbidden in scene code, because they read the wall clock or random state:
   - `requestAnimationFrame` loops
   - `setTimeout` and `setInterval`
   - `Date.now()` and `performance.now()`
   - `Math.random()` without a seed. Use a seeded PRNG such as `mulberry32(seed)`.
   - free-running CSS `animation:` or `transition:`. A CSS animation is allowed only when it is paused and you set its time from `t` (negative `animation-delay` or Web Animations `currentTime`).
   - `<video>` or `<audio>` elements that play on their own. Set `currentTime` from `t` and wait for `seeked`.
4. The scene also sets `window.__videoMeta = { width, height, fps, duration }` with the values of `timeline.json`.
5. Load fonts and images before the first seek. The renderer waits for `document.fonts.ready`. Inline or bundle every asset. Do not fetch from the network.

## Motion grammar

Take every value from `MOTION.md` or `BRAND.md` tokens when they exist. These are the defaults when no token exists.

1. Easing. Use spring or custom bezier curves, not linear or the browser default.
   - Enter: `cubic-bezier(0.22, 1, 0.36, 1)`, 400-700 ms.
   - Exit: `cubic-bezier(0.4, 0, 1, 1)`, 200-350 ms. An exit is shorter than its enter.
   - Linear is allowed only for constant motion such as a slow camera drift or a progress bar.
   - A move lasts at least 100 ms (6 frames at 60 fps).
2. Staggered reveals.
   - Offset items by 40-80 ms each.
   - Reveal at most 5 items per group. Split a longer list into groups, or reveal the rest together.
3. Kinetic typography.
   - Lock word highlights to `audio/sync-sheet.json` word times, not to guessed times.
   - Highlight 1-3 words at a time: the words spoken now.
   - Keep the rest of the line visible at a lower weight or opacity. Do not make words jump in size more than 8%.
4. One focal point per beat.
   - Each beat has 1 `focal_point` (normalized `x`, `y` from `timeline.json`). The primary motion of the beat starts there.
   - Other elements hold still or move at most at 30-40% opacity while the focal element moves.
   - Between scenes, keep the focal jump at `<= 0.35` of the frame, or bridge it with a camera move.
5. Tokens.
   - Read durations, eases, stagger and colours from `MOTION.md` and `BRAND.md` by name. Do not invent new values when a token exists.
   - Record a new value as a token in `MOTION.md` before you use it twice.

## Diagram and code walkthroughs

- Reveal 1 node, edge or code line per step, in narration order.
- Dim context to 30-40% opacity. Keep the active element at 100%.
- Animate a connector by its stroke length (`strokeDashoffset`) from source to target over 300-600 ms.
- Hold each finished step for at least 0.6 s before the next step starts.

## Workflow

1. Read the inputs. List the beats of the scene, each with a start time, a focal point and its tokens.
2. Copy the template in `references/seek-contract.md` into `scenes/<scene-id>/index.html`.
3. Build all motion as tweens on the paused master timeline, at absolute times in seconds.
4. Put any computed motion (noise, particles, character idle) in a function of `t` that `__seekToFrame` calls.
5. Run the self-check in `references/seek-contract.md`. Search the scene for every forbidden API. Fix each hit.
6. Render 3 sample frames twice: forward, then in reverse order. The images must be identical.

## Output

- `scenes/<scene-id>/index.html`, with all scripts and styles inline or in that folder.
- New or changed tokens in `MOTION.md`.

## Common mistakes

- A tween uses `onComplete` to start the next tween. A seek skips callbacks, so use absolute positions on the timeline instead.
- `gsap.from()` on a paused timeline flashes the end state at frame 0. Use `immediateRender: true` or `fromTo()`.
- Particles call `Math.random()` per frame. Seed the PRNG once per particle index, then compute the position from `t`.
- Text reflows while fonts load. Seek only after `document.fonts.ready`.
