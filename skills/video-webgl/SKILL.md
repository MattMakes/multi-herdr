---
name: video-webgl
description: Use when a video scene needs real-time 3D, particles or a shader look rendered in the browser - a three.js scene, a three.quarks particle system, a post-processing chain (bloom, depth of field, vignette, grain, colour grade) with the postprocessing library or react-postprocessing, a full-screen shader, or look-development with Tweakpane sliders - built so that every frame depends on the frame index only and renders the same forward, in reverse and cold in headless Chrome.
---

# WebGL scenes: 3D, particles and the cinematic finish

A WebGL scene is an HTML scene. It follows the seek contract of the `video-motion` skill: `window.__seekToFrame(frameIndex, fps)` sets every value from the frame index, draws once, and resolves when the frame is on the canvas. Libraries written for real-time use assume a running clock and a render loop. This skill removes both.

Your knowledge of these libraries may not match the files in the project. Read the version of each vendored file and check every call against the docs of that version before you rely on it.

## Libraries and the offline rule

| Library | For | Rule |
|---|---|---|
| three.js | scene, camera, materials | 1 file in the scene folder |
| three.quarks | particle systems | simulated: replay rule below |
| postprocessing | the effect chain for plain three.js | fixed time step |
| react-postprocessing | the same effects as components, when the project uses React Three Fiber | frame loop off |
| Tweakpane | sliders during look-development | never in a rendered frame |

A scene loads nothing from the network. Each library file sits in the scene folder or the project's vendor folder. When the project does not already ship a library, ask the operator with 1 `QUESTION:` before you add it, and record its licence and version in `MOTION.md`. Do not download one yourself.

## The deterministic renderer

1. **No clock.** No `requestAnimationFrame` loop, no library clock object, no animation-loop setter. `__seekToFrame` computes `t = frameIndex / fps`, sets state, and calls render once.
2. **Fixed pixels.** Set the pixel ratio to 1 and the canvas to the timeline size. Create the renderer with `preserveDrawingBuffer: true` so the screenshot reads the frame that was drawn.
3. **Wait for assets.** Models, textures and environment maps load before the first seek. Resolve a ready promise, and let the first `__seekToFrame` await it.
4. **Animation from `t`.** Keyframed clips: set the mixer's time to `t` and update with a delta of 0. Camera paths, uniforms and transforms: compute from `t` with the easing helpers of the scene library.
5. **Seeded chance.** A scene-local generator such as `mulberry32(seed)` feeds every random choice. A library that calls the built-in generator inside gets a seeded replacement installed before it loads, and reset at every replay.
6. **1 GPU mode per film.** Software rendering and a real GPU do not give the same pixels. All scenes and all rounds of 1 film use the same mode. Write the mode in `MOTION.md`.

Shader time is a uniform that you set from `t`. A shader never reads a clock.

## Particles: the replay rule

A particle system is a simulation: the state at frame `k` depends on frame `k - 1`. To make it a function of the frame index:

1. Keep a cursor: the last frame simulated.
2. On a seek to frame `k` at or after the cursor, step the simulation `k - cursor` times with a fixed step of `1 / fps`.
3. On a seek before the cursor, reset the system and the seeded generator, set the cursor to 0, and step forward to `k`.
4. For a long scene, store a snapshot every 60 frames when the library can restore one; otherwise accept the replay cost and report it.
5. With motion-blur subframes, the step is `1 / (fps * subframes)` and the cursor counts subframes.

Prefer a closed-form effect when one will do. A burst whose particles follow `p0 + v * t + 0.5 * g * t * t` with seeded `p0` and `v` needs no simulation and seeks for free. Use the particle library for what closed form cannot give: collisions, turbulence fields, trails, sub-emitters.

`references/particles-and-post.md` gives effect recipes, the layers of a good burst, and budgets.

## The finish: post-processing

The chain order matters. Use this order unless the look needs another:

1. render the scene;
2. anti-aliasing;
3. depth of field;
4. bloom;
5. tone mapping and colour grade;
6. vignette;
7. chromatic offset (moments only);
8. grain, last.

- Render the chain with a fixed delta of `1 / fps`, never a measured one.
- Any effect with its own time or random input (grain, glitch) takes its seed or time from the frame index.
- Bloom needs a threshold: only real light sources and highlights bloom. Bloom on everything is haze.
- Depth of field has 1 subject. Key the focus distance; pull focus in 0.4 to 0.8 s.
- Grain is 2 to 5 % and fixed to the screen. It also hides banding in dark gradients.
- The same finish runs on every scene of a film. Put the values in `MOTION.md` as tokens.

With React Three Fiber: set the canvas frame loop to "never" and advance the renderer by hand from `__seekToFrame`. A per-tick callback that reads delta time is forbidden; compute from the frame.

## Look-development with sliders

Guessing numbers in code and re-rendering is slow. Tune live, then freeze.

1. Keep every tunable value of the scene in 1 object: light strengths, bloom threshold and strength, focus distance, particle counts, curve points, colours.
2. Bind a Tweakpane panel to that object. Build the panel only when the page URL carries a tuning flag such as `?tune=1`. Add a time slider that calls `__seekToFrame`, so you tune on the frame that matters.
3. Tune on 3 frames at least: the hero frame, the darkest frame, the busiest frame.
4. Export the panel state and write it into `scenes/<scene-id>/look.json`. The scene reads its values from that file's content, inlined at build or loaded before the first seek.
5. The render never sees the panel: no flag, no panel, no panel code path. The forward and reverse probe proves it.

The same method serves any HTML scene with many numbers, not only 3D.

## When the scene is too slow

Measure the time per frame at the output size. Above about 0.5 s per frame, or when replay cost grows with every seek, render the scene once in order into `plates/<clip>/f%05d.png` and hand over a plate: the `video-plates` skill holds the contract. Say in `DONE:` that the scene is a plate and why.

## Gates

1. The `video-motion` gates: 3 sample frames rendered forward and then in reverse are byte-identical. For a simulated system, also compare a cold seek to the last frame with a sequential play.
2. No forbidden clock or random call in scene code. A seeded replacement of the built-in generator is declared in a comment that says why.
3. No network request while the scene loads or renders.
4. The tuning panel is absent from the rendered page.
5. The time per frame and the GPU mode are in `DONE:`.

## Common mistakes

| Mistake | Result | Fix |
|---|---|---|
| A render loop left running | Frames differ between renders | Render only inside the seek |
| The drawing buffer not preserved | Blank or stale screenshots | `preserveDrawingBuffer: true` |
| Device pixel ratio from the host | A different size on each machine | Pixel ratio 1 |
| A measured delta in the effect chain | Grain and blur change per run | A fixed delta |
| Particles stepped by the seek distance in 1 step | A different result for each seek order | Fixed steps, replay from 0 |
| A texture that is still loading at the first seek | A black first frame | Await the ready promise |
| Tuning values left in the panel only | The render uses defaults | Freeze them in `look.json` |
