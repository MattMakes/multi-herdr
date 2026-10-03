# Cinematic motion vocabulary

Use this when the direction is film-like. It translates camera and editing language into GSAP recipes, so a brief such as "a slow dolly-in, then a hard cut" becomes code. If the `art-direction` skill is available, take the director, film and motion character from it; this file covers only the motion.

## Plan the page like an edit

1. Write an **entrance map**: one line per section with its entrance and the beat it serves.
2. No 2 adjacent sections share an entrance. A plain fade-plus-rise appears at most 2 times per page. A page with 5 or more sections uses at least 4 entrance types.
3. One **signature move** per page (the heavy interaction): a pinned scrub, a horizontal track, a dolly zoom. Everything else stays quiet so the signature reads.
4. Vary tempo like scene lengths: a slow establishing beat, faster cuts through dense content, a held frame before the call to action.

```text
Entrance map - home
1 hero        iris-in on the key image, then title line mask        (establishing)
2 manifesto   rack focus on the pull quote                          (slow)
3 features    jump-cut stagger, 0.04 s                              (fast)
4 showcase    SIGNATURE: pinned dolly-in, scrub                     (held)
5 proof       curtain wipe left to right                            (medium)
6 cta         hard cut (no motion) on a held frame                  (stop)
```

Check: each section has 1 named entrance; the map passes rule 2.

## Entrances (camera moves to recipes)

| Term | Effect | GSAP recipe |
|---|---|---|
| Iris-in | Circle opens from the centre | `gsap.fromTo(el, { clipPath: "circle(0% at 50% 50%)" }, { clipPath: "circle(75% at 50% 50%)", duration: 1.4, ease: "power2.out" })` |
| Fade from black | Slow exposure | Black overlay `autoAlpha` 1 to 0 over 1.5 to 2 s, `ease: "sine.inOut"` |
| Dolly-in | Camera pushes toward subject | `gsap.from(el, { scale: 0.85, autoAlpha: 0, duration: 1.2, ease: "power3.out" })` |
| Push-in on scroll | Slow approach while scrolling | `gsap.fromTo(img, { scale: 1 }, { scale: 1.15, ease: "none", scrollTrigger: { trigger: frame, scrub: true } })` with `overflow: hidden` on the frame |
| Crane down | Frame descends into place | `gsap.from(el, { yPercent: -100, duration: 1.6, ease: "expo.out" })` inside a clipping parent |
| Whip pan | Fast lateral entrance with blur feel | `gsap.from(el, { xPercent: -120, skewX: 10, duration: 0.45, ease: "power4.out" })` |
| Rack focus | Blurred to sharp | `gsap.from(el, { filter: "blur(16px)", scale: 1.06, autoAlpha: 0, duration: 1 })`. Blur repaints: use on 1 element, not a grid. |
| Curtain wipe | Edge-to-edge reveal | `gsap.fromTo(el, { clipPath: "inset(0 100% 0 0)" }, { clipPath: "inset(0 0% 0 0)", duration: 1, ease: "power3.inOut" })` |
| Split diopter | Two halves open from the centre | Left `clipPath: "inset(0 50% 0 0)"` and right `"inset(0 0 0 50%)"` both to `"inset(0 0% 0 0%)"`, same position |
| Tilt-up | Rises from a slight perspective | `gsap.from(el, { rotationX: 12, y: 60, autoAlpha: 0, transformPerspective: 800, transformOrigin: "50% 100%" })` |
| Line mask reveal | Title-card text | SplitText `type: "lines", mask: "lines"`, `from(self.lines, { yPercent: 100, stagger: 0.08, ease: "expo.out" })` |
| Snap zoom | Comic punch-in | `gsap.from(el, { scale: 0.2, autoAlpha: 0, duration: 0.3, ease: "back.out(2)" })`. Use once per page at most. |

## Transitions between sections

| Term | Effect | Recipe |
|---|---|---|
| Hard cut | No transition | No motion. Use it on purpose after a long scrubbed beat. |
| Match cut | Shape in A becomes shape in B | Flip: `Flip.getState(aShape)`, swap to B with the same `data-flip-id`, `Flip.from(state, { duration: 0.8, ease: "power2.inOut" })` |
| Crossfade | Scenes overlap | Pinned stack; scrubbed timeline fades scene B in over scene A with `"<"` overlap |
| Jump cut stagger | Rapid sequence of items | `stagger: 0.04`, `duration: 0.25`, `ease: "power3.out"`, no easing on opacity |
| L-cut / J-cut | Background changes before or after the text | Two tracks in one timeline: background tween at `"-=0.4"` relative to the text |
| Wipe | Directional replacement | `clipPath: "inset(0 0 0 100%)"` to `"inset(0 0 0 0%)"` on the incoming scene, scrubbed |

## During scroll

| Term | Effect | Recipe |
|---|---|---|
| Dolly zoom (Vertigo) | Background grows while subject shrinks | One scrubbed timeline: background `scale: 1.3`, foreground `scale: 0.85` at position `0`, `ease: "none"`, pinned frame |
| Tracking shot | Lateral move through a scene | Fake horizontal scroll with `containerAnimation` (see `scrolltrigger.md`) |
| Parallax depth | Layers at different speeds | `yPercent` -10 to -30 per layer, `scrub: true`, or ScrollSmoother `data-speed` |
| Establishing hold | The hero stays while content arrives | `pin: true` on the hero for `"+=50%"`, `pinSpacing: false`, next section scrolls over it |
| Steadicam drift | Slow, continuous float | A long `yoyo` tween (`y: 8`, `duration: 4`, `ease: "sine.inOut"`, `repeat: -1`), paused off-screen |

## Tempo tokens for cinematic work

| Beat | Duration | Ease |
|---|---|---|
| Establishing | 1.2 to 2 s | `expo.out`, `power2.inOut` |
| Dialogue (reading content) | 0.6 to 0.9 s | `power3.out` |
| Action (cuts, staggers) | 0.25 to 0.45 s | `power4.out` |
| Held frame | 0 s | none |

## Smooth scroll choice

- Prefer GSAP ScrollSmoother when the project already uses GSAP: no adapter needed.
- If the project uses Lenis, connect it with `lenis.on("scroll", ScrollTrigger.update)` and drive Lenis from `gsap.ticker` (see `scrolltrigger.md`).
- Use no more than 1 smooth-scroll library, and turn it off in reduced-motion mode.

## Reduced motion for cinematic pages

Keep the edit, drop the camera. Every reveal becomes a short opacity change or none. Pinned and scrubbed sequences become a plain stack of their end states. The dolly zoom, parallax and drift loops are removed. The page must still read in order with every scene's content visible.

## Rules that keep it cinematic, not noisy

- Restraint is the effect: remove 1 move before you add 1.
- The signature move gets the longest duration and the most space; others defer to it.
- Never animate process labels such as "chapter", "scene" or "director" into the UI unless the direction asks for an editorial treatment.
- Test the page with all motion off: the composition must still work.
