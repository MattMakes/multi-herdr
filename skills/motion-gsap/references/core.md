# GSAP core API

## Install and import

```bash
npm install gsap            # all plugins are in this package and free, including commercial use
```

```javascript
import { gsap } from "gsap";
```

Do not create an `.npmrc` with a GreenSock token, do not use the old private registry, and do not tell anyone to buy a membership. Those instructions are out of date.

## Tween methods

| Method | What it does |
|---|---|
| `gsap.to(targets, vars)` | Animate from the current state to `vars`. |
| `gsap.from(targets, vars)` | Animate from `vars` to the current state. Good for entrances. Renders the start state at once (`immediateRender: true`). |
| `gsap.fromTo(targets, fromVars, toVars)` | Explicit start and end. Does not read current values. `immediateRender: true` by default. |
| `gsap.set(targets, vars)` | Apply at once (a zero-duration tween). |
| `gsap.quickTo(target, prop, vars)` | Returns a function that retargets one reusable tween. Use for values that change often (pointer followers). |

Targets: a selector string, an element, an array, or a NodeList. Property names are camelCase (`backgroundColor`, `rotationX`). CSS custom properties animate by name: `"--hue": 180`.

All methods return a Tween. Store it when you need control:

```javascript
const t = gsap.to(".box", { x: 100, duration: 1, repeat: 1, yoyo: true });
t.pause(); t.play(); t.reverse(); t.restart();
t.progress(0.5); t.time(0.2); t.totalTime(1.5);
t.kill();
```

## Common vars

| Var | Meaning |
|---|---|
| `duration` | Seconds. Default `0.5`. |
| `delay` | Seconds before start. In a sequence, use a timeline instead. |
| `ease` | Default `"power1.out"`. |
| `stagger` | Seconds between targets (`0.1`), or an object (see below). |
| `repeat` | Number of repeats; `-1` is infinite. |
| `yoyo` | With `repeat`, alternate direction. |
| `repeatDelay` | Seconds between repeats. |
| `overwrite` | `false` (default); `true` kills all tweens of the same targets at once; `"auto"` kills only overlapping properties of other active tweens when this tween first renders. |
| `immediateRender` | `true` by default for `from()` and `fromTo()`. |
| `paused` | Create paused. |
| `onStart`, `onUpdate`, `onComplete`, `onRepeat`, `onReverseComplete` | Callbacks. `this` is the tween. |
| `clearProps` | Comma list of properties (or `"all"`) to remove from inline style on completion. Clearing any transform property clears the whole transform. |

### Stacked `from()` tweens

When 2 or more `from()`/`fromTo()` tweens animate the same property of the same element, the later one renders its start state at once and overwrites the first tween's end state. Set `immediateRender: false` on the later tweens.

## Transforms and visibility

GSAP applies transform aliases in a fixed order (translate, scale, rotationX/Y, skew, rotation). Use them instead of a raw `transform` string.

| Alias | Equivalent |
|---|---|
| `x`, `y`, `z` | translate in px by default |
| `xPercent`, `yPercent` | translate in % of the element's own size; works on SVG |
| `scale`, `scaleX`, `scaleY` | scale |
| `rotation` (= `rotationZ`), `rotationX`, `rotationY` | rotate, deg by default; `"1.25rad"` also works |
| `skewX`, `skewY` | skew |
| `transformOrigin` | `"left top"`, `"50% 50%"` |
| `transformPerspective` | perspective for this element only |

- `autoAlpha`: animates `opacity` and sets `visibility: hidden` at 0 and `inherit` above 0. Use it for fades so hidden elements take no clicks.
- `svgOrigin` (SVG only): a transform origin in the SVG's global coordinates, for example `"250 100"`. No percentages. Use it, or `transformOrigin`, not both.
- Directional rotation: suffix `_short`, `_cw` or `_ccw`, for example `rotation: "-170_short"`.

## Relative and function-based values

```javascript
gsap.to(".a", { x: "+=20", rotation: "-=30" });       // also "*=2" and "/=2"
gsap.to(".item", { x: (i, el, all) => i * 50, stagger: 0.1 }); // called once per target on first render
gsap.to(".dot", { x: "random(-100, 100, 5)" });          // evaluated per target
```

## Stagger

```javascript
gsap.to(".item", { y: -20, stagger: 0.1 });
gsap.to(".item", { y: -20, stagger: { each: 0.1, from: "center" } });
gsap.to(".cell", { scale: 0, stagger: { amount: 0.6, grid: "auto", from: "edges", ease: "power1.in" } });
```

`each` is the gap between targets; `amount` is the total spread. `from`: `"start"`, `"center"`, `"edges"`, `"random"`, `"end"`, an index, or ratios like `[0.25, 0.75]`. `grid`: `[rows, columns]` or `"auto"`; `axis`: `"x"` or `"y"`.

## Eases

Every family has a base name (same as `.out`), `.in`, `.out` and `.inOut`:

`none`, `power1` to `power4` (1 is gentle, 4 is steep), `back` (overshoot, `back.out(1.7)`), `elastic` (`elastic.out(1, 0.3)`), `bounce`, `circ`, `expo`, `sine`. `steps(n)` gives stepped motion.

Use only these names. For any other curve, register CustomEase (see `plugins.md`):

```javascript
const ease = CustomEase.create("brand", ".17,.67,.83,.67");   // cubic-bezier values
gsap.to(".item", { x: 100, ease });
```

## Keyframes

```javascript
gsap.to(".box", { keyframes: [{ x: 100, duration: 0.4 }, { y: 50, duration: 0.3 }, { rotation: 90 }] });
gsap.to(".box", { keyframes: { x: [0, 100, 100, 0], y: [0, 0, 50, 0], easeEach: "power1.inOut" }, duration: 2 });
```

For more than a few steps, or steps on different targets, use a timeline.

## Defaults

```javascript
gsap.defaults({ duration: 0.6, ease: "power2.out" });
```

Put the motion tokens here or in timeline `defaults`.

## gsap.matchMedia(): breakpoints and reduced motion

`gsap.matchMedia()` (3.11+) runs a setup function while a media query matches. When it stops matching, GSAP reverts every animation and ScrollTrigger created in that run. It creates its own context: do not nest `gsap.context()` in it.

```javascript
const mm = gsap.matchMedia();

mm.add(
  {
    isDesktop: "(min-width: 800px)",
    isMobile: "(max-width: 799px)",
    reduceMotion: "(prefers-reduced-motion: reduce)",
  },
  (context) => {
    const { isDesktop, reduceMotion } = context.conditions;
    if (reduceMotion) {
      gsap.from(".hero > *", { autoAlpha: 0, duration: 0.3, stagger: 0 });
      return;                                   // no movement, no scroll-linked effects
    }
    gsap.from(".hero > *", { autoAlpha: 0, y: isDesktop ? 40 : 20, stagger: 0.08 });
    return () => { /* optional custom cleanup */ };
  },
  containerEl                                   // optional scope for selector text
);

// later, for example on unmount:
mm.revert();
```

- A plain string query also works: `mm.add("(min-width: 800px)", () => { ... })`.
- `gsap.matchMediaRefresh()` re-runs all matching handlers, for example after an in-page "reduce motion" toggle.
- In reduced mode keep the end state and the meaning. Do not just set `duration: 0` on a scroll-scrubbed or pinned effect: remove the effect.

## Common mistakes

- A layout property (`width`, `height`, `top`, `left`) tween where `x`, `y` or `scale` gives the same look.
- `svgOrigin` and `transformOrigin` on the same element.
- An ease name that is not in the list above and is not a registered CustomEase.
- Forgetting that `from()` uses the current state as the end state and renders its start state at once.
