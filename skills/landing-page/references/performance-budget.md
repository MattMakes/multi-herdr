# Responsive and performance budget

Load at workflow step 6, before you add images, fonts, video or third-party scripts. These are defaults for a marketing landing page. A budget in the brief or the project wins. If you cannot meet a budget, record the gap and the reason in the report.

## 1. Targets

| Metric | Budget | Notes |
|---|---|---|
| LCP (Largest Contentful Paint) | 2.5 s or less | Measured on a mobile profile (throttled CPU and network) |
| CLS (Cumulative Layout Shift) | 0.1 or less | Reserve space for every image, embed, font swap and banner |
| INP (Interaction to Next Paint) | 200 ms or less | Keep main-thread tasks short; no heavy work on scroll |
| Total transfer, first load | 1 MB or less | Without video; lazy media loads later |
| Critical CSS | 50 KB or less, compressed | Inline or one early stylesheet |
| JavaScript, first load | 100 KB or less, compressed | A static landing page often needs 0-20 KB |
| Hero image | 200 KB or less at 1440 px | AVIF or WebP, with a fallback if the stack needs it |
| Other images | 150 KB or less each | Below the fold, lazy loaded |
| Web fonts | At most 2 families, 4 files, 150 KB total | WOFF2 only, subset if possible |
| Third-party scripts | Each one justified in the report | Load `async` or `defer`, after the hero |
| DOM size | About 1500 nodes or less | Large DOMs slow mobile layout |

## 2. Responsive rules

- Mobile first. Check at 320, 390, 768, 1024 and 1440 px. Screenshots for review use 390, 768 and 1440 px.
- `<meta name="viewport" content="width=device-width, initial-scale=1">`. Never disable zoom.
- No horizontal scroll at any width. A full-width element is the usual cause: check `100vw` with a scrollbar, fixed widths and long words or URLs.
- Body text at least 16 px on mobile. Line length 45-75 characters on desktop, 35-60 on mobile.
- Fluid type with `clamp()`; one type scale for the page.
- Containers: one max width; side padding at least 16 px on mobile.
- Tap targets: at least 44 x 44 px for primary mobile actions, 8 px apart.
- Respect safe areas on notched phones for fixed bars: `env(safe-area-inset-bottom)`.
- Sticky headers and bars must not cover focused elements or anchor targets: set `scroll-margin-top` on targets.
- Test landscape on a phone size (844 x 390): the hero CTA must still be reachable.

## 3. Images

- Give every `<img>` `width` and `height` (or CSS `aspect-ratio`) so the layout does not shift.
- Use `srcset` and `sizes` for every content image wider than 400 px. Offer widths near 640, 960, 1440 and 1920 px.
- The LCP image (usually the hero visual): no `loading="lazy"`, add `fetchpriority="high"`, and preload it only if it is discovered late (for example a CSS background).
- Everything below the first viewport: `loading="lazy"` and `decoding="async"`.
- Use SVG for logos and icons. Inline small SVGs; keep icon sets to one library.
- Decorative grain or noise: one fixed, `pointer-events: none` layer, never on scrolling content.

## 4. Fonts

- Self-host or use the stack's font loader. Preload only the 1-2 files used in the hero.
- `font-display: swap` (or `optional` for display fonts on slow targets).
- Match fallback metrics (`size-adjust`, `ascent-override`) or use the framework's font tools to keep CLS low.
- Variable fonts are fine if one file replaces several weights.

## 5. Video and heavy media

- Hero video: muted, `playsinline`, a poster image, a visible pause control, compressed (aim for 2 MB or less for a loop), not loaded on reduced motion or save-data.
- Pause media when it is off screen or the tab is hidden.
- 3D, WebGL and large animation libraries load only after the hero is interactive, with a static fallback.

## 6. Scripts and motion

- Animate only `transform` and `opacity`. Use `will-change` only on elements that are about to animate.
- No `scroll` event listeners for effects. Use `IntersectionObserver`, CSS scroll-driven animations, or the motion library's scroll tools.
- Wrap all non-essential motion in `@media (prefers-reduced-motion: no-preference)`, or provide a reduce override that gives the final static state.
- Analytics, chat widgets and tag managers load after first paint and are listed in the report.

## 7. How to measure

Use what the project and harness already have. Do not install a new tool chain only to measure; report the limit instead.

- **Lighthouse or PageSpeed-style audit**: if a Lighthouse CLI or a browser audit panel is available, run the mobile profile and record LCP, CLS, TBT and the performance score.
- **Browser tool**: read `performance.getEntriesByType('largest-contentful-paint')` and layout-shift entries with the snippet in `review-loop.md`.
- **File sizes**: list the built assets with sizes, for example `du -ah <dist> | sort -h | tail -20`, and gzip size with `gzip -c <file> | wc -c`.
- **Image check**: confirm every `<img>` has dimensions and that only the hero image is eager.

Record the numbers in the report as a table: metric, budget, measured, pass or fail, note.
