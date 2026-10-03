# States, interaction, and motion

Generated UI styles one state, the happy default. Interfaces break in the other seven.

## The eight states

| State | When | Treatment |
| --- | --- | --- |
| Default | at rest | base style |
| Hover | pointer over, inside `@media (hover: hover)` | one signal: a color shift, a 1 px lift, or an underline; not several |
| Focus-visible | keyboard focus | `outline: 2px solid var(--color-focus); outline-offset: 2px;` appears instantly |
| Active | while pressed | `translateY(1px)` or `scale(0.98)`, darker fill |
| Disabled | not available | `opacity: 0.55`, `cursor: not-allowed`, native `disabled` or `aria-disabled="true"`, and a reason nearby |
| Loading | working | label stays readable or is replaced by an inline spinner; skeletons for known layouts |
| Error | failed | message, icon or border change, `aria-invalid="true"`; never color alone |
| Success | done | quiet: a check or a label change; no celebration for visible results |

A production interactive element is not finished until it has all of the states that apply to it. For a component build, render every state on one demo page (force hover and focus with classes such as `.is-hover` next to the real pseudo-classes) and check each one.

## Focus and targets

- Never `outline: none` without a replacement. Use `:focus-visible`, not `:focus`.
- The ring is 2 to 3 px, 3:1 or more against its surroundings, and never animated.
- Touch targets 44 x 44 px or more. Extend small icons with padding or an `::before` inset.

## Forms

- Label above the input, visible, never a placeholder used as a label. Placeholders show format (`01 Jan 2026`), not instructions.
- Helper text below; the error replaces the helper in the same slot. The slot reserves one line (`min-height: 1lh`) so an error does not push the page.
- Validate on blur; after the first blur, revalidate on change. Never on every keystroke from the start.
- Error text: what happened, why if known, what to do. Connect with `aria-describedby`.
- One height for inputs and the buttons beside them (44 px floor).
- `border-width` is the same in every state. Reserve the focus ring with `outline: 2px solid transparent` at rest so focus causes no layout shift.
- Disable submit only while invalid or in flight, not while idle.
- Native `<select>`, checkbox, and radio unless a custom one keeps full keyboard and screen-reader support. `accent-color` styles checkboxes cheaply.

## Overlays

- Modal: native `<dialog>` with `showModal()`; `inert` on the page behind; close on Escape, backdrop click, and a close button; first focus on the first field. Keep it centered: `position: fixed; inset: 0; margin: auto; height: fit-content; max-height: min(80vh, 40rem);`.
- Menus, popovers, tooltips: the `popover` attribute where available; flip near the viewport edge; never inside an `overflow: hidden` parent.
- Tooltips: hover delay 800 to 1000 ms, focus delay 0 ms; hoverable, persistent, dismissible with Escape (WCAG 1.4.13).
- Prefer undo to confirm: do reversible actions at once and show an Undo toast for 5 to 10 seconds. Keep confirm dialogs for irreversible actions, and make the user type the name of what is destroyed.
- Toasts stack in one corner and do not move existing content. Use them for failures and for effects the user cannot see. Pause auto-dismiss on hover and focus.

## Feedback timing

- Spinners: show after 150 ms; once shown, keep for at least 300 ms. No flash.
- Copy-to-clipboard: the button label changes to "Copied" for about 2.5 s; no toast.
- Optimistic update: change the UI at once; on failure, roll back and show an error with Retry.
- Live values: announce the final value with `aria-live="polite"`, not every tick.

## Empty, loading, error screens

- Empty: one line that names what is empty, one line on why it matters, one action.
- Loading: a skeleton in the shape of the final layout for predictable content; a spinner with text for waits over 2 s; a progress indication for waits over 10 s.
- Error: say what failed and offer the next action. No `window.alert()`.

## Motion

Motion has a reason or it is cut. Valid reasons: feedback on an action, a change of state, guiding attention to the primary element, revealing a sequence that matters. "It looks nice" is not a reason. For scroll choreography, pinning, and timelines, use the `motion-gsap` skill if it is available.

- Animate only `transform` and `opacity`. Accordions animate `grid-template-rows: 0fr` to `1fr`, not `height`.
- Name the transitioned properties. Never `transition: all`.
- Easing tokens; never the browser `ease`; `linear` only for progress and loaders:

```css
:root {
  --ease-out: cubic-bezier(0.16, 1, 0.3, 1);    /* entering */
  --ease-in: cubic-bezier(0.7, 0, 0.84, 0);     /* leaving */
  --ease-in-out: cubic-bezier(0.65, 0, 0.35, 1);/* toggles */
  --dur-micro: 120ms;  /* press, toggle, color */
  --dur-short: 220ms;  /* hover, tooltip, menu */
  --dur-long: 420ms;   /* modal, drawer, page reveal */
}
```

- Exits take about 75% of the entrance duration.
- No overshoot or bounce on UI state. Springs only for physical interactions (drag release, swipe).
- One orchestrated entrance per page, staggered by index, total under about 500 ms. No fade-up on every section.
- At most three motion primitives per page (for example: one entrance, one hover lift, one marquee). At most one marquee.
- No scroll event listeners. Use IntersectionObserver, CSS scroll-driven animation, or the motion library's scroll API. No parallax by default.
- No infinite loops except functional loaders. No cursor followers, no custom cursors.
- Carousels and auto-advancing content pause on hover and focus, or advance manually (WCAG 2.2.2).
- Nothing flashes more than 3 times per second.
- `will-change` only on the element, only while it animates.
- In React, continuous values (pointer, scroll) use motion values or refs, never component state.

Reduced motion is a first-class state:

```css
@media (prefers-reduced-motion: reduce) {
  *, *::before, *::after {
    animation-duration: 150ms !important;
    animation-iteration-count: 1 !important;
    transition-duration: 150ms !important;
    scroll-behavior: auto !important;
  }
}
```

Spatial motion becomes an opacity crossfade of 150 ms or less. Functional motion (progress, spinners) still runs.

Before handoff, ask of each animation: if it were instant, would the user lose information? If not, remove it.
