---
name: motion-gsap
description: Use when a web page or component needs animation - entrances, timelines, scroll-driven or pinned sections, text or SVG effects, drag, layout transitions - or when GSAP code needs writing, review, cleanup or a performance or reduced-motion fix.
---

# Motion with GSAP

Build web motion that serves the content, runs at the display frame rate, respects reduced-motion users and cleans up after itself. GSAP is the default library here: it is framework-agnostic, has timelines and ScrollTrigger, and every plugin is free in the public `gsap` npm package. If the project already uses another motion library, keep it and apply the same workflow and rules.

## When to use

- A page, section or component needs entrance, hover, state-change, scroll-linked, pinned or horizontal-scroll motion.
- A design direction asks for a motion character ("calm", "kinetic", "cinematic") and someone must turn it into durations, eases and choreography.
- Existing GSAP code leaks on unmount, janks, ignores `prefers-reduced-motion`, or misbehaves with ScrollTrigger.

Do not use it for static visual design, colour or type decisions. If the `art-direction` or `ui-taste` skill is available, use it for those and take only the motion character from it.

## Inputs

- The page or component, its framework (vanilla, React or Next.js, Vue or Nuxt, Svelte or SvelteKit, other) and its build setup.
- The design direction, if one exists: the motion character, tokens and the sections that carry the main idea.
- The content: what the user must notice first, read, or act on.
- The constraints: target devices, bundle budget, SSR, existing animation code.

## Workflow

1. **Decide whether to animate.** For each candidate, write one line: what it animates and what it tells the user (orientation, feedback, hierarchy, continuity, delight). Cut a candidate with no answer. Use CSS transitions for a single-state hover or focus change; use GSAP when you need sequencing, runtime control (pause, reverse, seek), scroll linkage, dynamic values or plugins.
   Check: a motion plan lists each animation, its purpose and its tool (CSS or GSAP).
2. **Set the motion character.** Turn the direction into tokens: 2 or 3 durations, 1 or 2 eases, 1 stagger step, and the distance of a typical move. Put them in `gsap.defaults()` or timeline `defaults`. Use the table below when the direction gives only adjectives. For a film or "cinematic" brief, load `references/cinematic-motion.md`.
   Check: the tokens exist in one place in code; no tween repeats a raw duration or ease that a token covers.
3. **Budget the page.** At most 1 heavy interaction per page (pinned scrub sequence, horizontal scroll, smooth scroll, a WebGL-like effect). At most 2 attention-seeking reveals per page. Adjacent sections do not use the same entrance. A fade-plus-rise entrance appears at most 2 times per page.
   Check: the motion plan names the heavy interaction and the entrance for each section.
4. **Set up GSAP.** Install `gsap` (and `@gsap/react` for React). Import only the plugins you use and call `gsap.registerPlugin(...)` once at app level, before first use. Load `references/core.md` for tween API facts and `references/plugins.md` for any plugin.
   Check: one registration site; no `.npmrc` token or private registry.
5. **Build with timelines.** Sequence with `gsap.timeline()` and the position parameter, not chained `delay` values. Use labels for named beats. Load `references/timeline.md`.
   Check: no tween in a sequence uses `delay` to wait for another tween.
6. **Add scroll motion when the plan needs it.** Put `scrollTrigger` on a timeline or a top-level tween only. Choose `scrub` or `toggleActions`, not both. Create triggers in page order or set `refreshPriority`. Load `references/scrolltrigger.md`.
   Check: every `scrollTrigger` key sits in `gsap.timeline()`, a `gsap.to/from/fromTo` call or `ScrollTrigger.create()`, never in a `tl.to(...)` child; a grep for `markers: true` finds nothing.
7. **Wire the framework lifecycle.** Create animations after the DOM exists, scope selectors to the component root, and revert everything on unmount. React: `useGSAP()` with `scope` (load `references/react.md`). Vue, Nuxt, Svelte and others: `gsap.context(fn, root)` and `ctx.revert()` in the unmount hook (load `references/frameworks.md`).
   Check: mount and unmount the component 3 times; no console warnings, no duplicated triggers (`ScrollTrigger.getAll().length` returns to its start value), no leftover inline styles.
8. **Handle reduced motion and breakpoints.** Wrap setup in `gsap.matchMedia()` with a `reduceMotion: "(prefers-reduced-motion: reduce)"` condition. In reduced mode, keep the final state and meaning, and drop movement: no parallax, no scrub-driven transforms, no auto-playing loops, no pinning that hijacks scroll. Short opacity fades are acceptable.
   Check: with reduced motion emulated, every piece of content is visible and usable, and nothing moves more than an opacity change.
9. **Make it fast.** Animate transforms (`x`, `y`, `xPercent`, `scale`, `rotation`) and `opacity`/`autoAlpha`. Read layout before you write it. Use `gsap.quickTo()` for pointer followers. Load `references/performance.md`.
   Check: a grep of the animation code finds no `top`, `left`, `width`, `height`, `margin` or `padding` tween where a transform gives the same look.
10. **Verify in a browser.** Run the checks in the review checklist. Use a browser tool, if available: record a performance trace while the main motion plays, emulate `prefers-reduced-motion: reduce`, and take screenshots before and after each entrance. Without a browser tool, state which checks you could not run.
    Check: the report states the measured frame rate or long-frame count, the layout-shift result, and the reduced-motion result.

### Motion character from adjectives

| Direction says | Durations (s) | Eases | Stagger (s) | Move distance |
|---|---|---|---|---|
| calm, editorial, luxury | 0.8, 1.2 | `power2.out`, `power2.inOut` | 0.08 to 0.12 | 16 to 24 px or a mask reveal |
| crisp, product, utility | 0.25, 0.4 | `power3.out` | 0.03 to 0.05 | 8 to 12 px |
| kinetic, playful | 0.4, 0.7 | `back.out(1.7)`, `expo.out` | 0.04 to 0.06 | 30 to 60 px, small rotation |
| dramatic, cinematic | 1.0, 1.6 | `expo.inOut`, `power4.out` | 0.1 to 0.15 | clip-path or scale reveals |

UI feedback (hover, press, toggle) stays at or below 0.3 s in every character.

## Rules

- Every animation has a stated purpose. Motion never blocks reading or input: interactive elements are usable while an entrance plays, and no entrance on a primary action lasts more than 1 s.
- Prefer transform aliases over the raw `transform` string and over layout properties. Prefer `autoAlpha` over `opacity` when an element fades to 0, so it stops taking clicks.
- Set initial hidden states in a way that avoids a flash: use `gsap.from()`/`fromTo()` (they render their start state at once) or `gsap.set()` before first paint. Do not hide content with CSS that only JavaScript can reveal unless a reduced-motion and a no-JS path also reveal it.
- When several `from()`/`fromTo()` tweens target the same property of the same element, set `immediateRender: false` on the later ones.
- Use built-in ease names only (`power1` to `power4`, `back`, `elastic`, `expo`, `circ`, `sine`, `bounce`, `none`, each with `.in`, `.out`, `.inOut`). Use CustomEase for any other curve.
- Store the return value of a tween or timeline you need to control. Kill or revert what you create: tweens, timelines, ScrollTriggers, SplitText, Draggable, Observer.
- Scope selectors in components (`useGSAP` `scope`, `gsap.context(fn, root)` or `gsap.utils.selector`).
- Call `ScrollTrigger.refresh()` after layout changes that are not a viewport resize (fonts, images, async content).
- Do not ship GSDevTools, MotionPathHelper or `markers: true`.
- Do not run gsap or ScrollTrigger during server rendering.

## Review checklist

- [ ] Every animation has a purpose line; the page has at most 1 heavy interaction and at most 2 attention-seeking reveals.
- [ ] Motion tokens live in one place; durations and eases match the motion character.
- [ ] No layout-property tweens where a transform works; `will-change` only on elements that animate.
- [ ] No layout shift: entrances use transforms, clip-path or opacity, and the element's box does not move other content. Cumulative Layout Shift from motion is 0 in a trace, if a browser tool is available.
- [ ] Frame rate: the main motion runs without long frames (over 50 ms) on a CPU-throttled (4x) trace, if a browser tool is available.
- [ ] Reduced motion: with `prefers-reduced-motion: reduce` emulated, all content shows, nothing translates, scales, pins or loops.
- [ ] Cleanup: mount and unmount 3 times; trigger count and inline styles return to the start state; no console errors.
- [ ] ScrollTrigger: on timelines or top-level tweens only; no `scrub` with `toggleActions`; no markers; correct refresh order.
- [ ] Plugins registered once; no development plugins in the bundle.
- [ ] Keyboard focus is visible and not hidden behind an animating element.

## References

- `references/core.md` - load for tween methods, vars, transforms, eases, stagger, function-based values, `gsap.matchMedia()`.
- `references/timeline.md` - load when you sequence more than 2 tweens or use labels and the position parameter.
- `references/scrolltrigger.md` - load for any scroll-linked, pinned, scrubbed, batched or horizontal-scroll motion.
- `references/plugins.md` - load when you use Flip, SplitText, ScrollSmoother, ScrollTo, Observer, Draggable, Inertia, SVG, text, physics or ease plugins.
- `references/utils.md` - load when you map, clamp, snap, randomize, wrap or distribute values.
- `references/performance.md` - load when motion janks, when you animate many elements, or before the verification step.
- `references/react.md` - load for React or Next.js.
- `references/frameworks.md` - load for Vue, Nuxt, Svelte, SvelteKit or another component framework.
- `references/cinematic-motion.md` - load when the direction is film-like or asks for "cinematic" reveals, camera moves or scene transitions.
