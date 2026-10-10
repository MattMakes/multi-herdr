# Timelines and the position parameter

## Create

```javascript
const tl = gsap.timeline({ defaults: { duration: 0.5, ease: "power2.out" } });
tl.to(".a", { x: 100 })
  .to(".b", { y: 50 })
  .to(".c", { autoAlpha: 0, duration: 0.3 });   // overrides the default duration
```

Children are appended one after another by default. A timeline's duration comes from its children; there is no `duration` var on the constructor.

Constructor vars: `defaults`, `paused`, `repeat`, `repeatDelay`, `yoyo`, `delay`, `scrollTrigger`, `onStart`, `onUpdate`, `onComplete`, `onRepeat`, `onReverseComplete`, `smoothChildTiming`.

## Position parameter

The third argument of `.to()`, `.from()`, `.fromTo()`, `.add()`, `.set()` and `.call()`:

| Value | Placement |
|---|---|
| `1` | At 1 s from the timeline start (absolute). |
| omitted | At the end of the timeline (`"+=0"`), the default. |
| `"+=0.5"` | 0.5 s after the end of the timeline. |
| `"-=0.2"` | 0.2 s before the end of the timeline (overlap). |
| `"<"` | At the start of the most recently added child. |
| `">"` | At the end of the most recently added child. |
| `"<0.2"` | 0.2 s after the start of the most recently added child. |
| `">-0.1"` | 0.1 s before the end of the most recently added child. |
| `"<25%"` | At 25% of the most recently added child's duration. |
| `"label"` | At a label (created if missing, at the end). |
| `"label+=0.3"` | 0.3 s after a label. |

```javascript
tl.to(".title", { y: 0, autoAlpha: 1 })
  .to(".subtitle", { y: 0, autoAlpha: 1 }, "<0.15")   // overlap: starts 0.15 s after the title
  .to(".cta", { scale: 1, autoAlpha: 1 }, "-=0.2");    // overlaps the end by 0.2 s
```

Overlap beats is the main tool for a fluid sequence. A strict one-after-another chain reads as slow and mechanical.

## Labels

```javascript
tl.addLabel("intro", 0)
  .to(".a", { x: 100 }, "intro")
  .addLabel("outro", "+=0.5")
  .to(".b", { autoAlpha: 0 }, "outro");

tl.play("outro");                       // jump to a label and play
tl.tweenFromTo("intro", "outro");       // pauses tl, returns a linear tween of its playhead
tl.tweenTo("outro");
```

With ScrollTrigger, `snap: "labels"` snaps scroll progress to label positions.

## Nesting

Build each scene as a function that returns a timeline, then assemble a master. This keeps scenes testable and reorderable.

```javascript
function intro() { return gsap.timeline().from(".logo", { autoAlpha: 0, y: 20 }).from(".nav a", { autoAlpha: 0, stagger: 0.05 }, "<0.1"); }
function hero()  { return gsap.timeline().from(".hero h1", { yPercent: 100 }).from(".hero p", { autoAlpha: 0 }, "<0.3"); }

const master = gsap.timeline();
master.add(intro()).add(hero(), "-=0.2");
```

Do not put a `scrollTrigger` on a nested child. Only a top-level timeline or tween can own one.

## Playback control

`play()`, `pause()`, `resume()`, `reverse()`, `restart()`, `seek(time | label)`, `time(2)`, `progress(0.5)`, `timeScale(2)`, `kill()`, `revert()`.

For a toggle (menu open and close), build one paused timeline and call `play()` or `reverse()`; do not create a new timeline on each click.

```javascript
const menu = gsap.timeline({ paused: true })
  .to(".menu", { clipPath: "inset(0% 0% 0% 0%)", duration: 0.5 })
  .from(".menu a", { y: 20, autoAlpha: 0, stagger: 0.04 }, "<0.2");
button.addEventListener("click", () => (menu.reversed() || menu.progress() === 0 ? menu.play() : menu.reverse()));
```

## Callbacks in a sequence

```javascript
tl.call(() => el.classList.add("is-live"), null, "+=0.2");
tl.set(".badge", { autoAlpha: 1 }, "<");
```

## Common mistakes

- Chaining with `delay` instead of a timeline and the position parameter.
- Repeating the same duration and ease on every child instead of `defaults`.
- Expecting `duration` on the constructor to set the length of the timeline.
- A `scrollTrigger` on a child tween of a timeline.
