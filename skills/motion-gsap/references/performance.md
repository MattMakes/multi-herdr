# Performance

The goal: every frame of the main motion finishes inside the display's frame budget (16.7 ms at 60 Hz), with no layout shift.

## Animate compositor-friendly properties

| Prefer | Instead of |
|---|---|
| `x`, `y`, `xPercent`, `yPercent` | `top`, `left`, `margin` |
| `scale`, `scaleX`, `scaleY` | `width`, `height`, `padding` |
| `rotation`, `rotationX`, `rotationY` | rotating via re-layout |
| `opacity`, `autoAlpha` | `visibility` toggles in a loop |
| `clipPath` (`inset()`, `circle()`) for reveals | animating container height |

- `filter: blur()` and large `box-shadow` animations repaint every frame. Use them on small elements only, or animate the `opacity` of a pre-blurred or pre-shadowed layer.
- To animate height (accordion), measure once and tween `height` from 0 to the measured value on a short duration, or use Flip. Do not tween height on many elements at once.

## will-change

```css
.hero-layer { will-change: transform; }
```

Set it only on elements that animate, and only while they animate if there are many. Every promoted layer costs GPU memory. Do not set `will-change` or `force3D` on everything.

## Avoid layout thrash

Reading layout (`offsetWidth`, `getBoundingClientRect()`, `scrollHeight`) after a style write forces a synchronous layout. In loops, read everything first, then write:

```javascript
const widths = items.map((el) => el.offsetWidth);        // all reads
items.forEach((el, i) => gsap.set(el, { x: -widths[i] })); // all writes
```

Use function-based values with `invalidateOnRefresh` for measured ScrollTrigger distances, so GSAP measures on refresh instead of every frame.

## Many elements

- One tween with `stagger` instead of many tweens with manual delays.
- `ScrollTrigger.batch()` instead of one trigger per card.
- Animate only what is visible; for long lists, virtualize or reveal on enter.
- Reuse a paused timeline for toggles; never create a timeline per frame or per event.

## Frequent updates

`gsap.quickTo()` reuses one tween for values that change on every event:

```javascript
const xTo = gsap.quickTo(".cursor", "x", { duration: 0.4, ease: "power3" });
const yTo = gsap.quickTo(".cursor", "y", { duration: 0.4, ease: "power3" });
addEventListener("pointermove", (e) => { xTo(e.clientX); yTo(e.clientY); });
```

`gsap.quickSetter(target, prop, unit)` sets a value with no tween at all. Use it inside your own ticker callback.

## ScrollTrigger

- Pin only what needs it; each pin adds a spacer and work on refresh.
- Do not call `ScrollTrigger.refresh()` on every resize; it already refreshes on resize. Call it after content changes, debounced.
- Avoid `scrub` on effects that repaint (blur, shadow, large filters).
- Fixed image dimensions (`width`/`height` attributes or `aspect-ratio`) keep trigger positions stable.

## Lifecycle

- Kill or revert what you no longer show: route changes, closed modals, unmounted components.
- Pause decorative loops when they leave the viewport (`ScrollTrigger` `toggleActions: "play pause resume pause"`).
- GSAP's ticker runs on `requestAnimationFrame`, so it already stops in a hidden tab. `gsap.ticker.lagSmoothing()` keeps time sane after a stall; leave the default unless a smooth-scroll library needs `lagSmoothing(0)`.

## Measure

Use a browser tool, if available:

1. Record a performance trace while the main motion plays, with 4x CPU throttling.
2. Check frames: no long frames (over 50 ms) and few dropped frames during the motion. Look for "Layout" or "Recalculate Style" blocks inside animation frames; they point to a layout property tween or thrash.
3. Check layout shift: Cumulative Layout Shift from the motion is 0. Entrances must not push other content.
4. Check layers: the layers panel or paint flashing shows no full-page repaint per frame.

Without a browser tool, a quick in-page check:

```javascript
let frames = 0, long = 0, last = performance.now();
gsap.ticker.add(() => { const now = performance.now(); if (now - last > 50) long++; frames++; last = now; });
// play the motion, then read frames and long; report both
```

Report the numbers. Do not claim "60 fps" without a measurement.
