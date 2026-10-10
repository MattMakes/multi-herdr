# GSAP in React and Next.js

```bash
npm install gsap @gsap/react
```

```javascript
import { gsap } from "gsap";
import { useGSAP } from "@gsap/react";
import { ScrollTrigger } from "gsap/ScrollTrigger";
gsap.registerPlugin(useGSAP, ScrollTrigger);   // once, at module or app level, not in a component body
```

## useGSAP()

`useGSAP()` replaces `useEffect`/`useLayoutEffect` for GSAP. It runs the setup inside a `gsap.context()`, uses a layout effect on the client, and reverts everything (tweens, timelines, ScrollTriggers, SplitText, matchMedia, inline styles) on unmount.

```jsx
function Hero() {
  const root = useRef(null);
  const title = useRef(null);

  useGSAP(() => {
    gsap.from(title.current, { yPercent: 100, duration: 0.8, ease: "expo.out" });
    gsap.from(".hero-item", { autoAlpha: 0, y: 20, stagger: 0.08 });  // scoped to root
  }, { scope: root });

  return (
    <section ref={root}>
      <h1 ref={title}>Title</h1>
      <p className="hero-item">One</p>
      <p className="hero-item">Two</p>
    </section>
  );
}
```

- Always pass `scope`, so selector text only matches inside the component.
- Pass the DOM node, not the ref object: `gsap.to(box.current, ...)`, not `gsap.to(box, ...)`.

### Second argument

| Form | Effect |
|---|---|
| omitted | Runs once after mount (empty dependency array). |
| `[a, b]` | Re-runs when `a` or `b` change. |
| `{ dependencies: [a], scope: root, revertOnUpdate: true }` | Config form. `revertOnUpdate: true` reverts the previous run before the next one. Without it, earlier animations persist across re-runs. |

## Event handlers: contextSafe

Animations created after the hook runs (in click or pointer handlers) are not in the context, so they are not cleaned up. Wrap those functions with `contextSafe`:

```jsx
const { contextSafe } = useGSAP({ scope: root });

const onClick = contextSafe(() => {
  gsap.to(".panel", { rotation: "+=180" });
});

return <button onClick={onClick}>Spin</button>;
```

When you add listeners yourself inside the hook, use the second callback argument and remove the listener in the returned cleanup:

```jsx
useGSAP((context, contextSafe) => {
  const onEnter = contextSafe(() => gsap.to(card.current, { scale: 1.03 }));
  card.current.addEventListener("pointerenter", onEnter);
  return () => card.current?.removeEventListener("pointerenter", onEnter);
}, { scope: root });
```

## Toggles driven by state

Build the timeline once and drive it from state; do not rebuild it on every render.

```jsx
const tl = useRef();
useGSAP(() => {
  tl.current = gsap.timeline({ paused: true }).to(".drawer", { xPercent: -100, duration: 0.4 });
}, { scope: root });
useGSAP(() => { open ? tl.current.play() : tl.current.reverse(); }, { dependencies: [open], scope: root });
```

## Reduced motion and breakpoints

Create the `gsap.matchMedia()` inside `useGSAP`; the context reverts it on unmount.

```jsx
useGSAP(() => {
  const mm = gsap.matchMedia();
  mm.add({ reduce: "(prefers-reduced-motion: reduce)", ok: "(prefers-reduced-motion: no-preference)" }, (ctx) => {
    if (ctx.conditions.reduce) return;               // final state only
    gsap.from(".card", { y: 40, autoAlpha: 0, stagger: 0.1, scrollTrigger: ".cards" });
  }, root);                                          // scope for selector text in the handler
}, { scope: root });
```

## Without @gsap/react

```jsx
useLayoutEffect(() => {
  const ctx = gsap.context(() => {
    gsap.from(".item", { autoAlpha: 0, stagger: 0.1 });
  }, root);                // scope
  return () => ctx.revert();
}, []);
```

Always return `ctx.revert()`.

## Next.js and SSR

- GSAP touches the DOM. Run it only on the client: inside `useGSAP` or an effect. In the App Router, the component that calls `useGSAP` needs `"use client"`.
- Module-level `gsap.registerPlugin()` is safe on the server; calling `gsap.to()` or `ScrollTrigger.create()` during render is not.
- To avoid a flash of the final state before hydration, set the hidden start state in CSS for the elements you animate in, and make sure the reduced-motion branch and a no-JS fallback show them (for example, a `.js` class on `<html>` that the CSS rule depends on).
- After a client-side route change, the old page unmounts and `useGSAP` reverts its triggers. If layout of the new page settles late (fonts, images), call `ScrollTrigger.refresh()` after it settles.

## Strict Mode

React 18+ Strict Mode mounts, unmounts and mounts again in development. `useGSAP` handles this; a hand-written effect without `ctx.revert()` creates duplicate tweens and ScrollTriggers. Duplicate markers in development are a sign of missing cleanup.

## Common mistakes

- Selector text without `scope`.
- `gsap.registerPlugin()` inside a component body.
- Animations created in event handlers without `contextSafe`.
- `useEffect` without `ctx.revert()`.
- GSAP calls during server render.
