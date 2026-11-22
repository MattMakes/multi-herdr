# Particle recipes and the finish

## The replay wrapper

The shape of a seek-safe simulated system, independent of the library:

```js
function makeReplay(fps, reset, step) {
  let cursor = 0;
  reset();
  return function seek(frameIndex) {
    if (frameIndex < cursor) {
      reset();
      cursor = 0;
    }
    while (cursor < frameIndex) {
      step(1 / fps);
      cursor += 1;
    }
  };
}
```

- `reset` rebuilds the system to its state at frame 0 and reseeds the generator.
- `step` advances the system by a fixed delta. For three.quarks that is the update call of its batched renderer; check the name and the reset call in the vendored version.
- Call `seek(frameIndex)` inside `__seekToFrame`, then render.

When a library draws its random numbers from the built-in generator, install a seeded one before the library loads, and reseed it in `reset`:

```js
// Seeded on purpose: the particle library calls the built-in generator.
let rngState = 7;
function seededRandom() {
  rngState |= 0; rngState = (rngState + 0x6D2B79F5) | 0;
  let x = Math.imul(rngState ^ (rngState >>> 15), 1 | rngState);
  x = (x + Math.imul(x ^ (x >>> 7), 61 | x)) ^ x;
  return ((x ^ (x >>> 14)) >>> 0) / 4294967296;
}
Math.random = seededRandom;
```

## A burst in closed form

For sparks, confetti and debris with no collisions:

```js
function burstPosition(i, t, seed) {
  const r = mulberry32(seed + i * 7919);
  const angle = r() * Math.PI * 2, speed = 2 + r() * 4, lift = 2 + r() * 3;
  const life = 0.6 + r() * 0.6;
  const u = Math.min(t / life, 1);
  const drag = 1 - Math.exp(-3 * t);          // fast start, slows down
  return {
    x: Math.cos(angle) * speed * drag / 3,
    y: lift * t - 4.9 * t * t,
    z: Math.sin(angle) * speed * drag / 3,
    alpha: 1 - u * u,
    size: 1 - 0.6 * u,
  };
}
```

Draw the particles as 1 instanced mesh or 1 points object and write their attributes from this function on each seek.

## The layers of a good effect

An effect that reads as designed has several short layers, not 1 emitter:

| Layer | Length | What |
|---|---|---|
| Anticipation | 2 to 6 frames | a gather, a dim, a small pull inward |
| Core flash | 1 to 4 frames | a bright disk or sprite, additive |
| Burst | 0.2 to 0.5 s | fast particles, outward, with drag |
| Shock or ring | 0.2 to 0.4 s | 1 expanding shape that fades |
| Leftovers | 0.8 to 2 s | smoke, embers, dust, slow and few |
| Light on the scene | the first 0.1 to 0.3 s | a light or an emissive pulse on nearby objects |

- Shape over count: 40 well-placed particles read better than 4000 in a ball. Give the burst a direction.
- Vary size, speed and life per particle from the seed. Uniform particles look like a spray can.
- Size over life falls. Alpha over life falls faster at the end. Colour over life runs hot to cool.
- Stretch fast particles along their velocity.
- Additive blending for light (fire, sparks, magic). Normal blending for matter (smoke, dust, debris). Additive smoke is a common fault.
- The effect starts on the frame of its cause, and its sound starts on the same frame. Put that time in the plan for the audio seat.

## Recipes

| Effect | Emission | Motion | Look |
|---|---|---|---|
| Sparks | burst of 30 to 80 | fast, gravity, strong drag, life 0.3 to 0.8 s | additive, stretched, yellow to orange to dark red |
| Confetti | burst of 100 to 200 from 1 or 2 points | up and out, then a slow tumbling fall, life 2 to 4 s | flat quads in brand colours, normal blending |
| Snow, dust | steady, across the frame | slow drift with a noise field; larger near the camera | soft dots, low alpha, in and out of focus |
| Smoke | steady, 5 to 15 per second | rises, slows, spreads; rotates slowly | soft sprites, normal blending, 10 to 30 % alpha |
| Magic trail | emitted along a moving point | little own motion; a small outward drift | additive, shrinking, with a few bright seeds |
| Data stream | steady from a source to a target | along a curve, with speed variation | small sharp quads, brand colour, short trails |
| Fireflies | 20 to 60, always present | each on a slow closed path; a blink per particle | additive dots with a wide soft glow |
| Rain | steady, many | fast, straight, slight wind | streaks whose length is the travel of half a frame |

For a loop (snow, dust, fireflies), pre-run the system so the first frame is already full, or use closed-form paths that are periodic over the scene.

## Budgets

- Software rendering in headless Chrome is slow: a full-screen effect chain at 1080p can take a second per frame. Measure early.
- Each pass of the effect chain costs a full-screen draw. Combine vignette, grade and grain where the library allows it.
- Overdraw is the cost of particles: many large transparent sprites over each other. Fewer and smaller, or render the system at half size and scale up under a blur.
- Shadows and high-resolution environment maps cost load time and frame time. A film frame needs only what the camera sees.

## The finish, by look

| Look | Tone and grade | Bloom | Depth of field | Grain | Extras |
|---|---|---|---|---|---|
| Clean product | neutral tone mapping, contrast 1.05 | threshold high, strength low | shallow on the hero shot only | 2 % | soft contact shadows |
| Cinematic | filmic tone mapping, slight teal and warm split | medium, wide | yes, with focus pulls | 4 to 5 % | vignette 15 to 25 % |
| Neon, synth | saturated, crushed blacks | strong, 2 radii | no | 3 % | scan lines, colour split on hits |
| Soft, pastel | lifted blacks, low contrast | gentle, very wide | yes | 2 % | none |
| Technical, HUD | near-monochrome with 1 accent | on lines only | no | 3 % | slight colour split, scan lines |

Rules:

- Tone mapping happens once, at the end of the chain. A material colour that looks wrong is fixed in the material, not by a second tone curve.
- Colour textures are tagged as colour; data textures (normal, roughness) are not. A washed-out or too-dark render is usually this.
- Check the darkest frame for banding and the brightest for clipped highlights.
- Thin bright lines shimmer between frames. Thicken them to at least 2 px at the output size, or blur them slightly.

## Tuning panel layout

Group the panel the way the eye judges a frame, top to bottom: time, camera, light, materials, particles, finish. Give each slider a range that cannot produce a broken frame. Export the state after each session and commit `look.json`; the diff of that file is the record of what changed in a fix round.
