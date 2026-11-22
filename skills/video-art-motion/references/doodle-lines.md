# Doodle lines, draw-on and loops

## The pen

A hand-drawn line is a filled ribbon around a path. Build it in 5 steps.

1. **Control points.** The path as a short list of points.
2. **Wobble.** Move each point once by a seeded offset of up to about 1.1 px (at 1080p). This is the hand's inaccuracy. It does not change over time.
3. **Boil.** Add a second offset of about 0.55 px whose seed includes `Math.floor(frame / 4)`. At 30 fps the line is redrawn about 7.5 times per second.
4. **Smooth.** Resample with a Catmull-Rom spline at about 10 points per segment.
5. **Ribbon.** At each sample, offset to both sides by the half-width, and fill the polygon.

The half-width along the path, with `u` from 0 to 1:

```js
function penWidth(u, base, taper, pressure, a, b) {
  const ends = Math.min(1, u / 0.14, (1 - u) / 0.2);
  const tip = 1 - taper + taper * Math.max(0.04, ends) ** 0.7;
  const press = Math.max(0.14,
    1 - 0.26 * pressure + pressure * (0.3 * Math.sin(5.5 * u + a) + 0.14 * Math.sin(17 * u + b)));
  return base * tip * press;
}
```

- `a` and `b` are seeded phases, so every stroke swells in its own places.
- A rough edge: resample the ribbon edge every 1.5 px and move each edge point by a small seeded amount.
- A retrace: draw the stroke a second time at half width and 40 % alpha with more wobble. It reads as a line gone over twice.

Media as presets of the same model:

| Medium | Width | Taper | Pressure | Retrace | Wobble | Edge |
|---|---|---|---|---|---|---|
| Pencil | 1.45 | 1.0 | 1.0 | yes | 1.0 | rough |
| Ink pen | 2 to 3 | 0.8 | 0.7 | no | 0.8 | clean |
| Marker, print line | 1.7 | 0.5 | 0.5 | no | 1.6 | slightly rough |
| Crayon, chalk | 4 to 7 | 0.3 | 0.9 | no | 1.2 | very rough, with gaps |

Layer kinds: a paint layer softens and granulates; an ink layer keeps pencil tooth; a plain layer is untouched. Inks multiply, so lay a paper-coloured shape under each figure at each depth, or the lines behind show through.

## Boil rules

- Boil the line, not the fill and not the text.
- 0.5 to 2 px at the art's own size. More reads as shaking.
- 3 or 4 distinct states are enough; a hand animator redraws a held pose 3 or 4 times and cycles them.
- A stepped frame is never motion-blurred.
- A scene on 2s or 3s computes all drawn layers from the stepped frame, and the camera from the real one.

## Draw-on

- Cut the stroke on its centre line at `progress`, then build the ribbon. The tip then tapers like a pen that lifts. Clipping a finished ribbon gives a blunt, wrong tip.
- Speed: a short mark takes 0.3 to 0.8 s; a long confident line is fast; a small detail is slow.
- Order: the order a hand works in.

| Medium | Order of marks |
|---|---|
| Pencil sketch | light construction lines, main contours, detail, shading |
| Watercolour | pencil, light washes, darker washes, detail, final line |
| Woodcut | the key block, then colours from light to dark |
| Oil | toned ground, dark-to-light block-in, thick highlights last |
| Marker doodle | outline, fill, accents, 1 or 2 movement lines |

- Frame 0 of a drawing film is the bare ground. The last frames are the finished still: hold them, and say so in `MOTION.md`.
- A rest between marks is at most 0.5 s.
- Implement a long drawing film as a cache that only grows: strokes finished before frame `k` are on a canvas that later frames reuse. The key of that cache is the index of the last finished stroke. Check a cold seek to a late frame against a sequential play.

## Seamless loops

A loop of `duration` seconds is seamless when every moving value is periodic over a whole number of cycles:

```js
function loopValue(t, duration, cycles, phase) {
  return Math.sin(2 * Math.PI * (cycles * t / duration + phase));
}
```

- `cycles` is a whole number. Give neighbours different whole numbers (1, 2, 3) and phases, so the loop does not look like 1 pendulum.
- Boil is periodic when the loop length in frames is a multiple of the boil step times the number of boil states.
- Noise is periodic when you sample it on a circle: `noise(cos(a) * r, sin(a) * r)` with `a = 2 * Math.PI * t / duration`.
- A draw-on is not periodic. In a loop the mark draws on, settles, lives, and leaves in a way that hands over to the start: it is erased, covered, or drawn off.
- Logo or sticker loop: 2 to 4 s. The mark draws on (it does not fade in), settles, and keeps a small idle.
- Check: the step from the last frame to frame 0 is no larger than 1.5 times the median step between neighbours in the middle of the loop.

## Text and reading

- Hand lettering is pen strokes too. Give a word at least `max(0.7 s, 0.3 s per word)` on screen after it is complete.
- Typed text appears at about 50 characters per second after a pause of about 0.6 s, and then stays for a full read.
- At most 7 blocks of text in a frame. Lines are at least 28 px high at 1080p in the last second.
- Vertical video: keep text out of the top 14 %, the bottom 25 % and the right 14 %.

## Seams between doodle scenes

Choose the seam by what it says:

| The seam says | Mechanism |
|---|---|
| "And now this" | something in the foreground crosses and wipes |
| "The same thing, later" | 1 actor stays and changes form |
| "This one, out of those" | 1 of many is selected and grows to fill the frame |
| "The same things, in order" | the parts move to new places (at most 7 movers) |
| "Next to that" | the camera travels across 1 large canvas |
| "What is inside" | the layers come apart |

- A matched cut: speed up over 0.2 to 0.4 s, cut just after the fastest frame, arrive and settle over 0.8 to 1.4 s.
- Type and faces leave in 0.5 to 0.7 s and are never blurred.
- Nothing crosses the frame faster than about 2.5 frame widths per second.
- Never dissolve between 2 copies of 1 object.
- A brush wipe, an iris and a 1-frame flash are the ready-made seams. Never 2 flashes within 0.5 s.
