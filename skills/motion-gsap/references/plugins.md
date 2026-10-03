# GSAP plugins

Every plugin is in the public `gsap` package and free for commercial use. Import from `gsap/<Name>` and register once, before first use:

```javascript
import { gsap } from "gsap";
import { Flip } from "gsap/Flip";
import { SplitText } from "gsap/SplitText";
gsap.registerPlugin(Flip, SplitText);
```

| Plugin | Import | Use for |
|---|---|---|
| ScrollTrigger | `gsap/ScrollTrigger` | Scroll-linked motion (see `scrolltrigger.md`). |
| ScrollSmoother | `gsap/ScrollSmoother` | Smoothed native scroll, `data-speed` parallax. Needs ScrollTrigger. |
| ScrollToPlugin | `gsap/ScrollToPlugin` | Animate scroll position. |
| Observer | `gsap/Observer` | Normalized wheel, touch and pointer gestures. |
| Flip | `gsap/Flip` | Animate between 2 layout states. |
| Draggable | `gsap/Draggable` | Drag, spin, throw. |
| InertiaPlugin | `gsap/InertiaPlugin` | Momentum after release; velocity tracking. |
| SplitText | `gsap/SplitText` | Split text into lines, words, chars. |
| ScrambleTextPlugin | `gsap/ScrambleTextPlugin` | Scrambled text reveal. |
| TextPlugin | `gsap/TextPlugin` | Replace text content over time (typewriter). |
| DrawSVGPlugin | `gsap/DrawSVGPlugin` | Draw and erase strokes. |
| MorphSVGPlugin | `gsap/MorphSVGPlugin` | Morph one path into another. |
| MotionPathPlugin | `gsap/MotionPathPlugin` | Move along a path. |
| MotionPathHelper | `gsap/MotionPathHelper` | Development-only path editor. |
| CustomEase, CustomBounce, CustomWiggle | `gsap/CustomEase`, `gsap/CustomBounce`, `gsap/CustomWiggle` | Custom curves, bounce and wiggle eases. |
| EasePack | `gsap/EasePack` | `SlowMo`, `RoughEase`, `ExpoScaleEase`. |
| Physics2DPlugin, PhysicsPropsPlugin | `gsap/Physics2DPlugin`, `gsap/PhysicsPropsPlugin` | Velocity, gravity, friction motion. |
| GSDevTools | `gsap/GSDevTools` | Development-only timeline scrubber. |
| PixiPlugin, EaselPlugin | `gsap/PixiPlugin`, `gsap/EaselPlugin` | Animate PixiJS or EaselJS objects. |

Revert or kill plugin instances on unmount (`split.revert()`, `draggable.kill()`, `observer.kill()`, `smoother.kill()`), or create them inside a context so the context reverts them.

## ScrollSmoother

```html
<body>
  <div id="smooth-wrapper">
    <div id="smooth-content"><!-- all scrolling content --></div>
  </div>
  <!-- position: fixed elements go outside the wrapper -->
</body>
```

```javascript
gsap.registerPlugin(ScrollTrigger, ScrollSmoother);
const smoother = ScrollSmoother.create({ smooth: 1, effects: true, smoothTouch: 0.1 });
```

- `smooth`: seconds to catch up to the native position. `effects: true` enables `data-speed="0.8"` and `data-lag="0.5"` attributes on elements.
- Create ScrollSmoother before the ScrollTriggers that depend on it.
- It counts as the page's heavy interaction. Do not create it in reduced-motion mode: in the `reduceMotion` branch of `gsap.matchMedia()`, skip `ScrollSmoother.create()`.
- `smoother.scrollTo(target, smooth, position)` scrolls with it; `ScrollSmoother.get()` returns the instance.

## ScrollToPlugin

```javascript
gsap.to(window, { duration: 0.8, scrollTo: { y: "#pricing", offsetY: 80 } });
gsap.to(listEl, { duration: 0.5, scrollTo: { x: "max" } });
```

`scrollTo` keys: `x`, `y` (number, selector, element or `"max"`), `offsetX`, `offsetY`, `autoKill` (stop when the user scrolls). In reduced-motion mode, use `duration: 0` or native `scrollIntoView()`.

## Observer

```javascript
gsap.registerPlugin(Observer);
const obs = Observer.create({
  target: window,
  type: "wheel,touch,pointer",
  wheelSpeed: -1,          // makes wheel direction match touch-drag direction
  tolerance: 10,
  preventDefault: true,
  onDown: () => goTo(index - 1),
  onUp: () => goTo(index + 1),
});
```

- `type` default is `"wheel,touch,pointer"`. Also `"scroll"`.
- Direction names follow the pointer or finger movement. Test the paging direction with a wheel and with touch.
- Callbacks: `onUp`, `onDown`, `onLeft`, `onRight`, `onChange`, `onPress`, `onRelease`, `onDrag`, `onHover`, `onStop`. `self.deltaX`, `self.deltaY`, `self.velocityX`, `self.velocityY`.
- Observer does not link to scroll position. Use it for full-screen section paging and swipe gestures. A paged section hijacks scroll: give keyboard users the same navigation and disable it in reduced-motion mode.

## Flip

Record the state, change the DOM, then animate from the old state.

```javascript
const state = Flip.getState(".card");          // 1. first
container.classList.toggle("grid--list");      // 2. last (any DOM or class change)
Flip.from(state, {                             // 3. invert and play
  duration: 0.6,
  ease: "power2.inOut",
  absolute: true,
  stagger: 0.03,
  onEnter: (els) => gsap.fromTo(els, { autoAlpha: 0, scale: 0.9 }, { autoAlpha: 1, scale: 1 }),
  onLeave: (els) => gsap.to(els, { autoAlpha: 0, scale: 0.9 }),
});
```

| `Flip.from()` var | Meaning |
|---|---|
| `absolute` | Use `position: absolute` during the flip. Fixes flex and grid reflow problems. Default `false`. |
| `scale` | Use `scaleX`/`scaleY` instead of `width`/`height` for the size change. Faster, but distorts children. Default `false`. |
| `nested` | Set `true` when flipping elements are inside other flipping elements, so offsets do not compound. |
| `simple` | Skip rotation and skew calculations. Faster. |
| `targets` | Limit the flip to these elements. |
| `toggleClass` | Add a class during the flip. |
| `zIndex` | z-index during the flip. |
| `fade` | Cross-fade elements that swap (same `data-flip-id`). |
| `onEnter`, `onLeave` | Animate elements that were added or removed. |
| `prune` | Drop targets that did not move. |

Give matching elements across states the same `data-flip-id`. `Flip.getState(targets, { props: "backgroundColor,color" })` also records extra properties. `Flip.fit(el, target)` resizes one element to another's box.

## Draggable and InertiaPlugin

```javascript
gsap.registerPlugin(Draggable, InertiaPlugin);
Draggable.create(".slider-track", { type: "x", bounds: ".slider", inertia: true, edgeResistance: 0.85,
  snap: (v) => Math.round(v / 320) * 320 });
Draggable.create(".knob", { type: "rotation", inertia: true });
```

- `type`: `"x"`, `"y"`, `"x,y"`, `"top,left"`, `"rotation"`, `"scroll"`, `"scrollTop"`, `"scrollLeft"`.
- `bounds`: element, selector or `{ minX, maxX, minY, maxY }`. `inertia: true` needs InertiaPlugin. `snap` works on the throw end value.
- Callbacks: `onPress`, `onDragStart`, `onDrag`, `onDragEnd`, `onRelease`, `onClick`, `onThrowUpdate`, `onThrowComplete`. `this.x`, `this.y`, `this.rotation` inside them.
- A draggable control also needs a keyboard path (arrow keys or buttons).
- `InertiaPlugin.track(el, "x")` tracks velocity; `gsap.to(el, { inertia: { x: "auto" } })` continues it to a stop.

## SplitText (3.13+ API)

```javascript
SplitText.create(".headline", {
  type: "lines,words",
  mask: "lines",                 // wraps each line in an overflow: clip element for a reveal
  autoSplit: true,               // re-split when fonts load or width changes
  onSplit(self) {
    return gsap.from(self.lines, { yPercent: 100, duration: 0.8, stagger: 0.08, ease: "expo.out" });
  },
});
```

| Var | Meaning |
|---|---|
| `type` | `"chars"`, `"words"`, `"lines"`, comma-separated. Split only what you animate. |
| `mask` | `"lines"`, `"words"` or `"chars"`: adds a clipping wrapper, available on `self.masks`. |
| `autoSplit` | Re-split on font load and width change. Create the animation inside `onSplit()` and return it, so SplitText reverts and syncs it on re-split. |
| `aria` | `"auto"` (default: `aria-label` on the parent, `aria-hidden` on pieces), `"hidden"` or `"none"`. |
| `charsClass`, `wordsClass`, `linesClass` | Class names; `"line++"` adds an index (`line1`, `line2`). |
| `tag` | Wrapper tag, default `"div"`. Inline `span` pieces may not render transforms. |
| `smartWrap` | With chars only, keeps words from breaking mid-word. |
| `ignore` | Elements to leave unsplit. |
| `wordDelimiter`, `prepareText` | Custom word boundaries for other scripts. |
| `propIndex` | Add `--word`, `--char` index custom properties. |
| `deepSlice`, `reduceWhiteSpace` | Default `true`. |

- Split after fonts load (`document.fonts.ready`) or use `autoSplit`.
- For char splits, set `font-kerning: none` to avoid a kerning jump.
- Do not use `text-wrap: balance` on split text. SplitText does not support SVG `<text>`.
- `split.revert()` restores the original markup.
- Animate at most one headline per view by character. Body text animates by line, if at all.

## Text effects

```javascript
gsap.to(".code", { duration: 1, scrambleText: { text: "Deployed", chars: "01", revealDelay: 0.4, speed: 0.4 } });
gsap.to(".type", { duration: 2, text: { value: "Hello world", delimiter: "" }, ease: "none" });
```

## SVG

### DrawSVGPlugin

The value is the visible segment of the stroke: `"0% 100%"` is the full stroke, `"20% 80%"` the middle. A single value means start at 0.

```javascript
gsap.from("#line", { drawSVG: 0, duration: 1.2, ease: "power2.inOut" });
gsap.fromTo("#ring", { drawSVG: "50% 50%" }, { drawSVG: "0% 100%" });
```

The element needs a visible `stroke` and `stroke-width`. It affects stroke only. `DrawSVGPlugin.getLength(el)` returns the length.

### MorphSVGPlugin

```javascript
MorphSVGPlugin.convertToPath("circle, rect, ellipse, line, polygon, polyline");
gsap.to("#play", { morphSVG: "#pause", duration: 0.4, ease: "power2.inOut" });
gsap.to("#a", { morphSVG: { shape: "#b", type: "rotational", shapeIndex: 2 } });
```

- `shape` (required in the object form): selector, element or path data.
- `type`: `"linear"` (default) or `"rotational"` (try it when a morph kinks).
- `map`: `"size"` (default), `"position"`, `"complexity"`.
- `shapeIndex`: which start point maps to the end's first point; an array for multi-segment paths. Use `shapeIndex: "log"` once to print the computed value.
- `smooth` and `curveMode` (3.14+): smoothing points and curve-aware interpolation.
- `precompile: "log"` for very complex first frames. `render` and `updateTarget: false` to draw to a canvas.

### MotionPathPlugin

```javascript
gsap.to(".dot", { duration: 3, ease: "none",
  motionPath: { path: "#route", align: "#route", alignOrigin: [0.5, 0.5], autoRotate: true } });
```

`path` accepts a path element, a selector, path data or an array of points; `curviness` (0 to 2) smooths point arrays; `start` and `end` (0 to 1) use part of the path.

## Ease plugins

```javascript
gsap.registerPlugin(CustomEase, CustomWiggle, CustomBounce);
CustomEase.create("hop", "M0,0 C0.05,0.44 0.17,0.44 0.29,0.44 0.33,0 0.33,0 0.33,0 0.41,1 0.67,1 1,0");
CustomWiggle.create("shake", { wiggles: 6, type: "easeOut" });
CustomBounce.create("drop", { strength: 0.6, squash: 2 });
gsap.to(".bell", { rotation: 15, ease: "shake", duration: 0.8 });
```

## Physics

```javascript
gsap.to(".ball", { duration: 2, physics2D: { velocity: 250, angle: -60, gravity: 500 } });
gsap.to(".obj", { duration: 2, physicsProps: { x: { velocity: 100, end: 300 }, y: { velocity: -50, acceleration: 200 } } });
```

## Development only

`GSDevTools.create({ animation: tl })` adds a scrubber. `MotionPathHelper.create(".dot", "#path")` edits a path. Remove both before you ship.
