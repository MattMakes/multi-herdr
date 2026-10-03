---
name: ui-taste
description: Use when building, styling, or reviewing any web interface - a page, a component, or a screen - and the result must not look generic or AI-generated, or when a UI needs a scored design critique.
---

# UI Taste

Make interfaces look designed, not generated. This skill serves two modes:

- **Build**: follow the rules while you write markup and styles.
- **Review**: score an existing interface against the rubric, and do not edit it.

Most generated UI fails in the same places: default fonts, one blue or purple accent everywhere, centered everything, three equal cards, invented numbers, missing states. Every rule below has a check. Run the check; do not trust the impression.

## When to use

- You build or restyle a page, a section, a component, or an app screen.
- You review a UI (code, screenshots, or a live URL) and must give a score and a ranked fix list.
- Not for: brand strategy, a full design system, motion choreography, or image generation. For those, use `art-direction`, `brand-identity`, `design-system`, `motion-gsap`, or `design-imagery` if they are available. For changing an existing site step by step, use `ui-redesign` if available.

## Inputs

- The brief: audience, the one job of the interface, the tone, the brand assets.
- The project: framework, styling method, existing tokens (CSS custom properties, Tailwind theme, `tokens.json`, `design.md` or `DESIGN.md`), installed fonts, icon library, motion library.
- For review: the files, screenshots at the widths in the rubric, or a URL.

If a design-system file (`design.md`, `DESIGN.md`, a token file) exists, it is the authority. These rules fill only what it leaves open.

## Workflow

1. **Scan the project before you choose anything.** Record the font stack, palette tokens, spacing scale, icon set, motion library, and framework, each with a `file:line`.
   Done when: you can list what you will preserve and what you will introduce.
2. **Write the design read.** One line: `Reading this as: <page kind> for <audience>, tone <one extreme word>, structure <named layout>, density <airy|medium|dense>.` "Clean and modern" is not a tone. If two readings are equally likely and the choice changes the build, ask once: `horch tell orchestrator "[<role>] QUESTION: ..."` with both readings and your recommendation. Otherwise state your inference and continue.
   Done when: the read is in your output or report.
3. **Lock the tokens.** Define color, type, spacing, radius, easing, and duration tokens in one place before any component code (`references/color.md`, `references/typography.md`, `references/layout-and-space.md`). Reuse existing project token names.
   Done when: every color and `font-family` in the work refers to a token.
4. **Build structure, then surface, then states.** Choose the layout first, then type and color, then all states of every interactive element (`references/states-and-interaction.md`). Write the copy with `references/copy.md`.
   Done when: every interactive element has default, hover, focus-visible, active, and disabled styles, plus loading, error, and success where they apply.
5. **Self-critique and fix.** Run the hard gates and the six-axis score in `references/rubric.md`. Fix every failed gate and every axis below 3. Repeat once; if it still fails, the brief or the structure is wrong, so change the structure.
   Done when: all hard gates pass and no axis is below 3.
6. **Verify in a browser.** Render at 320, 375, 414, 768, 1280 x 800, and 1440 px wide. Use a browser tool if one is available. If none is available, say which checks you did not run.
   Done when: no horizontal scroll, no wrapped button or nav label, and the hero's headline and primary action are visible at 1280 x 800.

**Review mode** uses steps 1, 5, and 6 only. Do not edit files. Output the report format in `references/rubric.md`.

## Rules

These apply every time. The references give the detail and the checks.

- **Complete output.** Deliver every file, section, and state that was asked for. No `// ...`, `// rest of code`, `TODO` stubs, "similarly for the others", or skeletons in place of an implementation. If the work is too large for one response, stop at a clean boundary and state exactly what remains.
- **Honest content.** Do not invent metrics, customer names, logos, testimonials, or awards. Use a labeled slot (`metric to confirm`) or remove the section.
- **Tokens only.** No raw hex, `rgb()`, `oklch()`, or `font-family` outside the token block. A new value becomes a named token first.
- **Type.** Two families is normal, three is the maximum. Body text 16 px minimum, line length 45 to 75 characters, a fixed scale ratio of at least 1.2 between steps. Headings are roman, not italic.
- **Color.** One accent, used for emphasis on about 5% of any view. Neutrals are tinted toward the accent hue. No `#000` or `#fff` base surfaces. Body text contrast 4.5:1 or more; large text, icons, and focus rings 3:1 or more.
- **Layout.** One spacing scale on a 4 px base; every gap, padding, and margin is on it. No three equal icon-above-heading cards, no card inside a card, no centered-everything hero, no section layout repeated more than once.
- **States.** Keyboard focus is always visible and appears instantly. Touch targets are 44 x 44 px or more. Errors never rely on color alone.
- **Motion.** Animate only `transform` and `opacity`. Every animation has a reason and a `prefers-reduced-motion: reduce` fallback. No `transition: all`, no bounce on UI state, no scroll listeners.
- **Imagery.** No hand-drawn fake browser bars, phone frames, or dashboards made of `<div>` boxes. Use a real screenshot, a labeled placeholder slot, or no image. One icon library; no emoji as icons.
- **Respect the project.** Do not replace an existing design system, framework, or global stylesheet. Add to the global stylesheet; never remove its framework directives. Check `package.json` (or the equivalent) before you import a library.

## Review checklist

- [ ] Design read stated; tokens defined in one place and used everywhere.
- [ ] Every hard gate in `references/rubric.md` passes, with the evidence recorded.
- [ ] Six-axis score recorded; no axis below 3.
- [ ] `references/anti-patterns.md` searches run; every hit fixed or justified in one line.
- [ ] Rendered at all six widths; findings noted for any width not checked.
- [ ] All copy re-read: no invented facts, no banned phrases, no broken or "clever" lines.
- [ ] Every requested deliverable is present and complete.

## References

- `references/rubric.md` - load for step 5 and for every review: hard gates with measurement procedures, six-axis scoring, report format.
- `references/anti-patterns.md` - load for step 5 and reviews: named generic patterns, each with a search command and a fix.
- `references/typography.md` - load when you choose fonts or a type scale.
- `references/color.md` - load when you build or check a palette, contrast, or dark mode.
- `references/layout-and-space.md` - load when you plan structure, spacing, hero, sections, or responsive behavior.
- `references/states-and-interaction.md` - load when the UI has buttons, forms, overlays, or motion.
- `references/copy.md` - load when you write or review visible text.
- `references/imagery.md` - load when the UI needs images, icons, logos, or product previews.
