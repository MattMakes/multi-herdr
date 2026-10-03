# ScrollTrigger

```javascript
import { gsap } from "gsap";
import { ScrollTrigger } from "gsap/ScrollTrigger";
gsap.registerPlugin(ScrollTrigger);   // once, at app level
```

## Attach to a timeline or a top-level tween

```javascript
gsap.timeline({
  scrollTrigger: { trigger: ".chapter", start: "top top", end: "+=2000", scrub: 1, pin: true },
})
  .to(".chapter .a", { x: 100 })
  .to(".chapter .b", { y: 50 })
  .to(".chapter .c", { autoAlpha: 0 });
```

Wrong: `gsap.timeline().to(".a", { scrollTrigger: {...} })`. A ScrollTrigger only works on a top-level animation; never nest an animation that owns one inside a timeline.

Shorthand: `scrollTrigger: ".box"` sets only `trigger`. Standalone, with callbacks only: `ScrollTrigger.create({ trigger, start, end, onUpdate: (self) => ... })`.

## start and end

Format: `"<trigger point> <scroller point>"`. `"top center"` means: when the top of the trigger meets the centre of the viewport.

- Keywords `top`, `center`, `bottom` (`left`, `right` for horizontal), percentages, pixels: `"bottom 80%"`, `"top 100px"`.
- Offsets: `"top center+=100"`.
- Relative end: `"+=300"` (300 px after start), `"+=100%"` (one scroller height after start), `"max"`.
- A number is an absolute scroll position in px.
- `clamp()` (3.12+) keeps a value inside the page: `start: "clamp(top bottom)"`.
- A function is re-evaluated on each refresh: `end: () => "+=" + el.offsetWidth`.

Defaults: `start: "top bottom"` (or `"top top"` when `pin: true`), `end: "bottom top"`.

## Config

| Property | Notes |
|---|---|
| `trigger` | Element or selector whose position sets start and end. |
| `endTrigger` | A different element for `end`. |
| `scrub` | `true` links progress directly to scroll; a number (for example `1`) is the catch-up time in seconds. |
| `toggleActions` | 4 actions for onEnter, onLeave, onEnterBack, onLeaveBack. Each is `play`, `pause`, `resume`, `reset`, `restart`, `complete`, `reverse` or `none`. Default `"play none none none"`. |
| `pin` | `true` pins the trigger; or a selector or element. Animate the children, not the pinned element. |
| `pinSpacing` | Default `true`: adds padding so content after the pin waits. `false` or `"margin"`. |
| `anticipatePin` | A small number (for example `1`) applies the pin slightly early to avoid a jump on fast scroll. |
| `snap` | A number (increments of progress), an array, a function, `"labels"`, or `{ snapTo, duration, delay, ease }`. |
| `once` | Kill the trigger after it reaches `end` once; the animation keeps its state. |
| `toggleClass` | A class name for the trigger, or `{ targets, className }`. |
| `horizontal` | For a horizontally scrolling scroller. |
| `scroller` | A scrollable element instead of the viewport. |
| `containerAnimation` | The horizontal tween of a fake horizontal scroll (see below). |
| `invalidateOnRefresh` | Recompute function-based values on refresh. Use with responsive distances. |
| `refreshPriority` | Lower refreshes first. Use when triggers are not created in page order. |
| `id` | For `ScrollTrigger.getById(id)`. |
| `markers` | Development only. |
| `onEnter`, `onLeave`, `onEnterBack`, `onLeaveBack`, `onToggle`, `onUpdate`, `onRefresh`, `onScrubComplete` | Callbacks get the instance (`self.progress`, `self.direction`, `self.isActive`, `self.getVelocity()`). |

Choose `scrub` or `toggleActions`. When both are set, `scrub` wins.

## Patterns

### Reveal on enter, once

```javascript
gsap.from(".section-title", {
  yPercent: 100, autoAlpha: 0, duration: 0.8,
  scrollTrigger: { trigger: ".section-title", start: "top 85%", once: true },
});
```

### Batch reveals for many elements

`ScrollTrigger.batch()` creates one trigger per element and groups callbacks that fire within a short interval. Use it instead of one tween per card or an IntersectionObserver.

```javascript
gsap.set(".card", { autoAlpha: 0, y: 40 });
ScrollTrigger.batch(".card", {
  start: "top 85%",
  interval: 0.1,     // seconds to collect a batch
  batchMax: 4,       // or a function, re-evaluated on refresh
  onEnter: (els) => gsap.to(els, { autoAlpha: 1, y: 0, stagger: 0.1, overwrite: true }),
  onLeaveBack: (els) => gsap.set(els, { autoAlpha: 0, y: 40, overwrite: true }),
});
```

Batched callbacks get `(targets, scrollTriggers)`, not one instance. Do not pass `trigger`, `animation`, `scrub`, `snap`, `toggleActions`, `invalidateOnRefresh`, `onSnapComplete` or `onScrubComplete` to `batch()`.

### Pinned, scrubbed sequence

```javascript
const tl = gsap.timeline({
  defaults: { ease: "none" },
  scrollTrigger: { trigger: ".feature", start: "top top", end: "+=150%", pin: true, scrub: 0.5, snap: "labels" },
});
tl.addLabel("one").to(".feature .step-1", { autoAlpha: 0 })
  .addLabel("two").from(".feature .step-2", { yPercent: 30, autoAlpha: 0 })
  .addLabel("three");
```

Use `ease: "none"` in scrubbed timelines unless an eased segment is a deliberate choice; the scroll itself supplies the pacing.

### Parallax

```javascript
gsap.to(".layer-back", {
  yPercent: -20, ease: "none",
  scrollTrigger: { trigger: ".scene", start: "top bottom", end: "bottom top", scrub: true },
});
```

Keep parallax distances small (10% to 30%). Remove parallax in reduced-motion mode.

### Fake horizontal scroll (containerAnimation)

Pin a section and move an inner track sideways while the user scrolls down.

```javascript
const section = document.querySelector(".h-section");   // pinned, viewport-sized
const track = section.querySelector(".h-track");        // wide row of panels
const distance = () => track.scrollWidth - window.innerWidth;

const scrollTween = gsap.to(track, {
  x: () => -distance(),
  ease: "none",                      // required: keeps scroll and position 1:1
  scrollTrigger: {
    trigger: section,
    pin: true,
    scrub: true,
    start: "top top",
    end: () => "+=" + distance(),
    invalidateOnRefresh: true,
  },
});

// Trigger something when a panel moves into view horizontally:
gsap.from(".h-track .panel-3 h2", {
  yPercent: 100,
  scrollTrigger: {
    containerAnimation: scrollTween,
    trigger: ".h-track .panel-3",
    start: "left center",
    toggleActions: "play none none reverse",
  },
});
```

- The horizontal tween must use `ease: "none"`.
- A trigger that uses `containerAnimation` cannot pin or snap.
- Animate the track, not the pinned section.
- In reduced-motion mode, replace this with a normal vertical stack or a native `overflow-x: auto` row.

### Scroll progress indicator

```javascript
gsap.fromTo(".progress", { scaleX: 0 }, { scaleX: 1, ease: "none", transformOrigin: "left center",
  scrollTrigger: { start: 0, end: "max", scrub: 0.3 } });
```

## Refresh and order

- ScrollTrigger refreshes on viewport resize (debounced about 200 ms). Call `ScrollTrigger.refresh()` yourself after fonts load, images without fixed dimensions load, or async content changes the layout.
- Refresh runs in creation order. Create triggers top to bottom, or set `refreshPriority` so the first section on the page has the lowest number. Wrong order breaks pin spacing.
- Give images and media fixed dimensions or `aspect-ratio` so late loads do not move trigger positions.

## Third-party smooth scroll

GSAP's own ScrollSmoother needs no adapter (see `plugins.md`). For another library, tell ScrollTrigger how to read the scroll position and update it on every scroll:

```javascript
ScrollTrigger.scrollerProxy(document.body, {
  scrollTop(value) { if (arguments.length) scroller.scrollTop = value; return scroller.scrollTop; },
  getBoundingClientRect() { return { top: 0, left: 0, width: innerWidth, height: innerHeight }; },
  // optional: pinType: "transform" if pins do not stick, "fixed" if pins jitter
});
scroller.addListener(ScrollTrigger.update);
```

Libraries that keep native scroll (for example Lenis) only need the update hook and a shared ticker:

```javascript
lenis.on("scroll", ScrollTrigger.update);
gsap.ticker.add((time) => lenis.raf(time * 1000));
gsap.ticker.lagSmoothing(0);
```

## Cleanup

```javascript
ScrollTrigger.getById("hero")?.kill();
ScrollTrigger.getAll().forEach((t) => t.kill());   // for example on a route change without a context
```

In components, create triggers inside `useGSAP` or `gsap.context()`; reverting the context kills them. See `react.md` and `frameworks.md`.

## Common mistakes

- `scrollTrigger` on a child tween of a timeline.
- `scrub` and `toggleActions` on the same trigger.
- An eased horizontal tween with `containerAnimation`.
- Animating the pinned element itself.
- Triggers created out of page order without `refreshPriority`.
- No `refresh()` after content changes the layout.
- `markers: true` in production.
