# D14 gsap-factcheck report: `motion-gsap` against the GSAP docs

Branch `ds/gsap-factcheck`. Commit `Skills: Verify motion-gsap against the GSAP docs`.

## Summary

- I checked the API facts in `skills/motion-gsap/` against the official docs at gsap.com on 2026-10-03. The current npm release is GSAP 3.15.0.
- The fact table has 132 rows. Some rows group related facts from 1 line.
- Row status: 103 `ok`, 10 `fixed`, 14 `refined`, 4 `source`, 1 `n/a`.
- `source` means that no docs page covers the fact, so I checked the GSAP 3.15.0 source. I kept all 4 facts. I also removed 3 Draggable `type` values that exist only in the source.
- D05's Flip and Observer corrections hold. The Flip `scale`, `nested` and Observer `type` defaults are correct. I made the Flip `scale`, `simple`, `targets` and `onLeave` rows more exact.
- The worst error was `refreshPriority`. The skill said "lower refreshes first", but the docs say higher refreshes first. Following the old text gave the wrong pin-spacing order.
- Sizes after the change: `SKILL.md` 10,206 bytes (budget 12 KB). The skill directory is 66 KB of text (budget 160 KB).
- Gate: `HORCH_REQUIRE_GIT=1 HORCH_REQUIRE_SQLITE=1 just gate` prints `GATE GREEN`. No oracle or golden changed.

## Method

- gsap.com serves every docs page as Markdown at `<page>.md`. The index is `https://gsap.com/llms.txt`. I fetched the pages with `curl` (read only) and searched them.
- The URLs in the table drop the `.md` suffix, so they open in a browser.
- When a docs page did not cover a fact, I read the published file on jsDelivr (`https://cdn.jsdelivr.net/npm/gsap@3.15.0/dist/<File>.js`). This is the same code that `npm install gsap` installs. I installed nothing.
- Line numbers in the table are for the files after the fix.

Abbreviations in the table: `D/` = `https://gsap.com/docs/v3/`, `R/` = `https://gsap.com/resources/`, `SRC/` = `https://cdn.jsdelivr.net/npm/gsap@3.15.0/dist/`.

Status values: `ok` (matches the docs), `fixed` (was wrong, now matches), `refined` (was imprecise or incomplete, now exact), `source` (not on a docs page; checked in source), `n/a` (not a GSAP fact).

## Fixes

| File:line | Before | After | Source |
|---|---|---|---|
| `references/scrolltrigger.md:55` | `refreshPriority`: "Lower refreshes first." | "Higher refreshes first (default `0`)." | D/Plugins/ScrollTrigger (refreshPriority: "`refreshPriority: 1` will get refreshed earlier than one with `refreshPriority: 0`"); SRC/ScrollTrigger.js sort uses `refreshPriority * -1e6` |
| `references/scrolltrigger.md:164` | "the first section on the page has the lowest number" | "the highest number" | same |
| `references/timeline.md:23,27` | `">"` was "the default" position | Default is `"+=0"` (end of the timeline). `">"` is the end of the most recent child, which can be earlier. | R/position-parameter ("the default position is `\"+=0\"`") |
| `references/core.md:50` | `immediateRender` true for `from()`/`fromTo()` only | Also true for any tween with a `scrollTrigger` | D/GSAP/Tween/vars (immediateRender) |
| `references/plugins.md:127` | Draggable `type` listed `"scroll"`, `"scrollTop"`, `"scrollLeft"`; left out `"top"`, `"left"` and the default | `"x,y"` (default), `"x"`, `"y"`, `"top,left"`, `"top"`, `"left"`, `"rotation"` | D/Plugins/Draggable (type) |
| `references/plugins.md:224` | `physicsProps: { x: { velocity: 100, end: 300 } }` | `friction: 0.1` in place of `end`. PhysicsPropsPlugin has only `velocity`, `acceleration`, `friction`. | D/Plugins/PhysicsPropsPlugin |
| `references/plugins.md:229` | `MotionPathHelper.create(".dot", "#path")` | `MotionPathHelper.create(tween)`, or an element to start a new path. The 2nd argument is a config object. | https://gsap.com/docs/v3/Plugins/MotionPathHelper/ (Usage) |
| `references/plugins.md:150` | SplitText `autoSplit`: re-split "on width change" | Re-split on a width change only when `lines` are split; default `false` | D/Plugins/SplitText (autoSplit) |
| `references/core.md:15` | No licence limit stated | The GSAP Standard License prohibits 1 use: a no-code visual animation builder that competes with Webflow | https://gsap.com/standard-license (Prohibited Uses) |
| `references/performance.md:67` | The ticker "already stops in a hidden tab" | The browser throttles it in a hidden tab | D/GSAP/gsap.ticker ("Throttling when tab is hidden") |

Refinements (the old text was not wrong, but it was incomplete or vague):

| File:line | Change | Source |
|---|---|---|
| `references/plugins.md:106` | Flip `scale`: "Faster, but distorts children" became "Typically faster, but children scale too". | D/Plugins/Flip (scale) |
| `references/plugins.md:108` | Flip `simple`: skips calculations for rotated, scaled or skewed containers (not just "rotation and skew"). | D/Plugins/Flip (simple) |
| `references/plugins.md:109` | Flip `targets`: set it when a framework re-renders new element instances. | D/Plugins/Flip ("targets" framework note) |
| `references/plugins.md:113` | Flip `onLeave`: leaving elements are visible only with `absolute: true`. | D/Plugins/Flip (onLeave) |
| `references/plugins.md:73` | Observer `wheelSpeed: -1` comment: it inverts wheel deltas. | D/Plugins/Observer (wheelSpeed, onUp, onDown) |
| `references/plugins.md:64` | ScrollTo: a bare value scrolls `y`; CSS `scroll-behavior: smooth` conflicts. | D/Plugins/ScrollToPlugin |
| `references/plugins.md:148,149` | SplitText `type` default `"chars,words,lines"`; `mask` takes 1 type only. | D/Plugins/SplitText (type, mask) |
| `references/plugins.md:208` | MotionPath `curviness` default 1; `start`/`end` defaults 0 and 1, and values outside 0 to 1 wrap. | D/Plugins/MotionPathPlugin (curviness, start, end) |
| `references/scrolltrigger.md:46` | `pinSpacing` defaults to `false` when the pin's parent is `display: flex`. | D/Plugins/ScrollTrigger (pinSpacing) |
| `references/core.md:93` | Stagger `from` ratios need `grid`. | R/getting-started/Staggers (from) |
| `references/core.md:127` | `matchMedia()`: "do not nest `gsap.context()`" became "you do not need a `gsap.context()` inside it". The docs do not forbid nesting. | D/GSAP/gsap.matchMedia() |
| `references/plugins.md:3` | Points to the licence limit in `core.md`. | https://gsap.com/standard-license |
| `references/core.md:13` | Added the `@gsap/business`, `@gsap/shockingly` and `@gsap/club` packages to the out-of-date list. | https://gsap.com/llms.txt (licence header) |
| `SKILL.md:65` | Ease rule: added `steps(n)`, and `none` has no `.in`/`.out`. | D/Eases, D/Plugins (core eases list) |

## Fact table

### SKILL.md

| File:line | Claim | Source | Status |
|---|---|---|---|
| `SKILL.md:8` | Every plugin is free in the public `gsap` npm package | https://gsap.com/llms.txt; R/private-repo-migration | ok |
| `SKILL.md:33` | `@gsap/react` for React; `gsap.registerPlugin()` once | R/React | ok |
| `SKILL.md:41` | `gsap.matchMedia()` with a `reduceMotion` condition | D/GSAP/gsap.matchMedia() | ok |
| `SKILL.md:62` | `autoAlpha` stops clicks at 0 | D/GSAP/CorePlugins/CSS (autoAlpha) | ok |
| `SKILL.md:63` | `from()`/`fromTo()` render their start state at once | D/GSAP/Tween/vars (immediateRender) | ok |
| `SKILL.md:65` | Built-in ease names | D/Eases; D/Plugins | refined |
| `SKILL.md:67` | Scope with `useGSAP` `scope`, `gsap.context(fn, root)`, `gsap.utils.selector` | R/React; D/GSAP/gsap.context(); D/GSAP/UtilityMethods/selector() | ok |

### references/core.md

| File:line | Claim | Source | Status |
|---|---|---|---|
| `core.md:6,13` | `npm install gsap`; no `.npmrc` token or private registry | R/private-repo-migration; https://gsap.com/llms.txt | ok, refined |
| `core.md:15` | Licence: 1 prohibited use | https://gsap.com/standard-license | fixed |
| `core.md:21-25` | `to`, `from`, `fromTo`, `set`, `quickTo` signatures and behaviour | D/GSAP/gsap.to(); D/GSAP/gsap.from(); D/GSAP/gsap.quickTo() | ok |
| `core.md:27` | CSS variables animate by name | D/GSAP/CorePlugins/CSS (CSS variables) | ok |
| `core.md:42` | `duration` default `0.5` | D/GSAP/Tween/vars | ok |
| `core.md:44` | `ease` default `"power1.out"` | D/GSAP/Tween/vars; D/Eases | ok |
| `core.md:47` | `repeat: -1` is infinite | D/GSAP/Tween/vars | ok |
| `core.md:49` | `overwrite`: `false` default, `true`, `"auto"` | D/GSAP/Tween/vars | ok |
| `core.md:50` | `immediateRender` defaults | D/GSAP/Tween/vars | fixed |
| `core.md:53` | `clearProps` list or `"all"`; a transform property clears the whole transform | D/GSAP/CorePlugins/CSS (clearProps) | ok |
| `core.md:61` | Transform order: translate, scale, rotationX/Y, skew, rotation | D/GSAP/CorePlugins/CSS | ok |
| `core.md:66` | `xPercent`/`yPercent` work on SVG | D/GSAP/CorePlugins/CSS ("Percentage-based x/y translations also work on SVG") | ok |
| `core.md:68-71` | `rotation` = `rotationZ`; `"1.25rad"`; `transformPerspective` | D/GSAP/CorePlugins/CSS | ok |
| `core.md:73` | `autoAlpha`: `visibility: hidden` at 0, `inherit` above 0 | D/GSAP/CorePlugins/CSS (autoAlpha) | ok |
| `core.md:74` | `svgOrigin`: SVG only, global coordinates | D/GSAP/CorePlugins/CSS (svgOrigin) | ok |
| `core.md:75` | `_short`, `_cw`, `_ccw` | D/GSAP/CorePlugins/CSS (directionalRotation) | ok |
| `core.md:93` | Stagger `each`, `amount`, `from`, `grid`, `axis` | R/getting-started/Staggers | refined |
| `core.md:97` | Base ease name is the same as `.out` | SRC/gsap.js (`_insertEase` maps the bare name to `easeOut`) | source |
| `core.md:99` | Ease families and `steps(n)` | D/Eases; D/Plugins | ok |
| `core.md:104` | CustomEase accepts a cubic-bezier string `".17,.67,.83,.67"` | D/Eases/CustomEase (Using cubic-bezier values) | ok |
| `core.md:111-112` | Keyframes array, per-property arrays, `easeEach` | R/keyframes; D/GSAP/Tween/vars (keyframes) | ok |
| `core.md:120` | `gsap.defaults()` | D/GSAP/gsap.defaults() | ok |
| `core.md:127` | `matchMedia()` added in 3.11; reverts on no-match; creates a context | D/GSAP/gsap.matchMedia() | refined |
| `core.md:132-149` | Conditions object, `context.conditions`, cleanup return, scope 3rd argument, `mm.revert()` | D/GSAP/gsap.matchMedia() | ok |
| `core.md:155` | `gsap.matchMediaRefresh()` re-runs matching handlers | D/GSAP/gsap.matchMediaRefresh() | ok |

### references/timeline.md

| File:line | Claim | Source | Status |
|---|---|---|---|
| `timeline.md:6-12` | Timeline `defaults`; duration comes from children | D/GSAP/Timeline/vars; D/GSAP/Timeline/duration() | ok |
| `timeline.md:14` | Constructor vars list (incl. `scrollTrigger`, `smoothChildTiming`) | D/GSAP/Timeline/vars; D/GSAP/Timeline/scrollTrigger | ok |
| `timeline.md:23` | Default position `"+=0"` | R/position-parameter | fixed |
| `timeline.md:22-33` | `1`, `"+=0.5"`, `"-=0.2"`, `"<"`, `">"`, `"<0.2"`, `">-0.1"`, `"<25%"`, labels | R/position-parameter | ok |
| `timeline.md:32` | A missing label is added at the end | R/position-parameter | ok |
| `timeline.md:50-52` | `play(label)`, `tweenFromTo()` pauses the timeline and returns a linear tween, `tweenTo()` | D/GSAP/Timeline/tweenFromTo(); D/GSAP/Timeline | ok |
| `timeline.md:55` | `snap: "labels"` | D/Plugins/ScrollTrigger (snap) | ok |
| `timeline.md:73` | Playback methods incl. `revert()` | D/GSAP/Timeline (methods list) | ok |

### references/scrolltrigger.md

| File:line | Claim | Source | Status |
|---|---|---|---|
| `scrolltrigger.md:20` | ScrollTrigger only on a top-level animation | R/st-mistakes | ok |
| `scrolltrigger.md:22` | Shorthand `scrollTrigger: ".box"`; `ScrollTrigger.create()` | D/Plugins/ScrollTrigger | ok |
| `scrolltrigger.md:26-31` | `start`/`end` format, keywords, offsets, `"+=300"`, `"+=100%"`, `"max"`, number | D/Plugins/ScrollTrigger (start, end) | ok |
| `scrolltrigger.md:32` | `clamp()` since 3.12 | D/Plugins/ScrollTrigger (start) | ok |
| `scrolltrigger.md:33` | A function value is re-evaluated on refresh | D/Plugins/ScrollTrigger (end) | ok |
| `scrolltrigger.md:35` | Defaults `"top bottom"` (`"top top"` with pin), `"bottom top"` | D/Plugins/ScrollTrigger (start, end) | ok |
| `scrolltrigger.md:43` | `scrub` boolean or catch-up seconds | D/Plugins/ScrollTrigger (scrub) | ok |
| `scrolltrigger.md:44` | `toggleActions` keywords; default `"play none none none"` | D/Plugins/ScrollTrigger (toggleActions) | ok |
| `scrolltrigger.md:46` | `pinSpacing` | D/Plugins/ScrollTrigger (pinSpacing) | refined |
| `scrolltrigger.md:47` | `anticipatePin: 1` | D/Plugins/ScrollTrigger (anticipatePin) | ok |
| `scrolltrigger.md:48` | `snap` forms | D/Plugins/ScrollTrigger (snap) | ok |
| `scrolltrigger.md:49` | `once` kills the trigger, not the animation | D/Plugins/ScrollTrigger (once) | ok |
| `scrolltrigger.md:50` | `toggleClass` string or `{ targets, className }` | D/Plugins/ScrollTrigger (toggleClass) | ok |
| `scrolltrigger.md:54` | `invalidateOnRefresh` | D/Plugins/ScrollTrigger (invalidateOnRefresh) | ok |
| `scrolltrigger.md:55` | `refreshPriority` direction | D/Plugins/ScrollTrigger; SRC/ScrollTrigger.js | fixed |
| `scrolltrigger.md:58` | Callback names; `self.progress`, `direction`, `isActive`, `getVelocity()` | D/Plugins/ScrollTrigger (onScrubComplete, callbacks) | ok |
| `scrolltrigger.md:60` | `scrub` wins over `toggleActions` | SRC/ScrollTrigger.js (`isToggle = !scrub && scrub !== 0`) | source |
| `scrolltrigger.md:75,82` | `batch()`: 1 trigger per element; `interval`; `batchMax` number or function | D/Plugins/ScrollTrigger/static.batch() | ok |
| `scrolltrigger.md:88` | Batched callbacks get `(targets, scrollTriggers)`; the 8 options that `batch()` does not accept | D/Plugins/ScrollTrigger/static.batch() | ok |
| `scrolltrigger.md:149-150` | `containerAnimation`: linear ease; no pin or snap | D/Plugins/ScrollTrigger (containerAnimation) | ok |
| `scrolltrigger.md:163` | Resize refresh waits for a 200 ms gap | D/Plugins/ScrollTrigger | ok |
| `scrolltrigger.md:164` | Refresh order and `refreshPriority` | D/Plugins/ScrollTrigger (refreshPriority) | fixed |
| `scrolltrigger.md:172-177` | `scrollerProxy()` methods; `pinType` `"transform"` / `"fixed"` | D/Plugins/ScrollTrigger/static.scrollerProxy() | ok |
| `scrolltrigger.md:183-185` | Lenis hook | Lenis docs, not GSAP | n/a |

### references/plugins.md

| File:line | Claim | Source | Status |
|---|---|---|---|
| `plugins.md:3` | All plugins public and free for commercial use | https://gsap.com/llms.txt; https://gsap.com/standard-license | refined |
| `plugins.md:12-32` | Plugin names and `gsap/<Name>` import paths | D/Plugins; file list of `https://cdn.jsdelivr.net/npm/gsap@3.15.0/` | ok |
| `plugins.md:29` | EasePack holds SlowMo, RoughEase, ExpoScaleEase | D/Eases/SlowMo; D/Eases/RoughEase; D/Eases/ExpoScaleEase | ok |
| `plugins.md:40-44` | `#smooth-wrapper` / `#smooth-content`; fixed elements outside | D/Plugins/ScrollSmoother | ok |
| `plugins.md:49,52` | `smooth` seconds; `effects` enables `data-speed`, `data-lag`; `smoothTouch` | D/Plugins/ScrollSmoother (smooth, effects, smoothTouch) | ok |
| `plugins.md:53` | Create ScrollSmoother before its ScrollTriggers | D/Plugins/ScrollSmoother (Example) | ok |
| `plugins.md:55` | `smoother.scrollTo(target, smooth, position)`; `ScrollSmoother.get()` | D/Plugins/ScrollSmoother (methods) | ok |
| `plugins.md:64` | `scrollTo` keys | D/Plugins/ScrollToPlugin | refined |
| `plugins.md:72,81` | Observer `type` default `"wheel,touch,pointer"`; also `"scroll"` | D/Plugins/Observer (type) | ok (D05 correction holds) |
| `plugins.md:73` | `wheelSpeed: -1` | D/Plugins/Observer (wheelSpeed) | refined |
| `plugins.md:74` | `tolerance` | D/Plugins/Observer (tolerance) | ok |
| `plugins.md:75` | `preventDefault` | SRC/Observer.js (`preventDefault = vars.preventDefault`) | source: not on the docs page |
| `plugins.md:83` | Observer callbacks; `deltaX/Y`, `velocityX/Y` | D/Plugins/Observer | ok |
| `plugins.md:91-93` | `Flip.getState()` then `Flip.from()` | D/Plugins/Flip | ok |
| `plugins.md:105` | `absolute`, default `false` | D/Plugins/Flip (absolute) | ok |
| `plugins.md:106` | `scale`, default `false` (width/height by default) | D/Plugins/Flip (scale) | refined (D05 default holds) |
| `plugins.md:107` | `nested` prevents compounding offsets | D/Plugins/Flip (nested) | ok (D05 correction holds) |
| `plugins.md:108` | `simple` | D/Plugins/Flip (simple) | refined |
| `plugins.md:109` | `targets` | D/Plugins/Flip (targets) | refined |
| `plugins.md:110-112` | `toggleClass`, `zIndex`, `fade` with `data-flip-id` | D/Plugins/Flip | ok |
| `plugins.md:113` | `onEnter`, `onLeave` | D/Plugins/Flip (onLeave) | refined |
| `plugins.md:114` | `prune` | D/Plugins/Flip (prune) | ok |
| `plugins.md:116` | `getState(targets, { props })`; `Flip.fit()` | D/Plugins/Flip | ok |
| `plugins.md:122-124` | `Draggable.create()` with `bounds`, `inertia`, `edgeResistance`, `snap` | D/Plugins/Draggable | ok |
| `plugins.md:127` | Draggable `type` values | D/Plugins/Draggable (type) | fixed |
| `plugins.md:128` | `bounds` forms; `inertia: true` needs InertiaPlugin | D/Plugins/Draggable (bounds, inertia) | ok |
| `plugins.md:129` | Draggable callbacks incl. `onThrowUpdate`, `onThrowComplete`; `this.x` | D/Plugins/Draggable | ok |
| `plugins.md:131` | `InertiaPlugin.track()`; `inertia: { x: "auto" }` | D/Plugins/InertiaPlugin (velocity, track) | ok |
| `plugins.md:133` | SplitText 3.13+ API (`SplitText.create`, `onSplit`) | D/Plugins/SplitText | ok |
| `plugins.md:138` | `mask` wraps in an `overflow: clip` element | SRC/SplitText.js (`maskEl.style.overflow = "clip"`). The docs page says `visibility: clip`, a typo. | source |
| `plugins.md:148-149` | `type`, `mask` | D/Plugins/SplitText | refined |
| `plugins.md:150` | `autoSplit`; return the animation from `onSplit()` | D/Plugins/SplitText (autoSplit, onSplit) | fixed |
| `plugins.md:151` | `aria` values; default `"auto"` | D/Plugins/SplitText (aria) | ok |
| `plugins.md:152` | `"line++"` adds an index class | D/Plugins/SplitText (linesClass) | ok |
| `plugins.md:153` | `tag` default `div`; inline elements do not render transforms | D/Plugins/SplitText (tag) | ok |
| `plugins.md:154-158` | `smartWrap`, `ignore`, `wordDelimiter`, `prepareText`, `propIndex`, `deepSlice`, `reduceWhiteSpace` default `true` | D/Plugins/SplitText | ok |
| `plugins.md:160-163` | `document.fonts.ready`; `font-kerning: none`; no `text-wrap: balance`; no SVG `<text>`; `revert()` | D/Plugins/SplitText (Tips & Limitations, Reverting) | ok |
| `plugins.md:169` | ScrambleText `text`, `chars`, `revealDelay`, `speed` | D/Plugins/ScrambleTextPlugin | ok |
| `plugins.md:170` | TextPlugin `value`, `delimiter` | D/Plugins/TextPlugin | ok |
| `plugins.md:177-184` | DrawSVG segment values; single value starts at 0; needs a stroke; `getLength()` | D/Plugins/DrawSVGPlugin | ok |
| `plugins.md:189-199` | MorphSVG `convertToPath`, `shape`, `type`, `map`, `shapeIndex` (`"log"`), `smooth`/`curveMode` (3.14), `precompile`, `render`, `updateTarget` | D/Plugins/MorphSVGPlugin | ok |
| `plugins.md:205-208` | MotionPath `path`, `align`, `alignOrigin`, `autoRotate`, `curviness`, `start`, `end` | D/Plugins/MotionPathPlugin | refined |
| `plugins.md:214` | `CustomEase.create(id, path)` | D/Eases/CustomEase | ok |
| `plugins.md:215` | CustomWiggle `wiggles`, `type: "easeOut"` | D/Eases/CustomWiggle | ok |
| `plugins.md:216` | CustomBounce `strength`, `squash` | D/Eases/CustomBounce | ok |
| `plugins.md:223` | Physics2D `velocity`, `angle`, `gravity` | D/Plugins/Physics2DPlugin | ok |
| `plugins.md:224` | PhysicsProps options | D/Plugins/PhysicsPropsPlugin | fixed |
| `plugins.md:229` | `GSDevTools.create({ animation })` | D/Plugins/GSDevTools | ok |
| `plugins.md:229` | `MotionPathHelper.create()` signature | https://gsap.com/docs/v3/Plugins/MotionPathHelper/ | fixed |

### references/utils.md

| File:line | Claim | Source | Status |
|---|---|---|---|
| `utils.md:3-11` | Omit the value to get a function; `random()` needs `true` | D/GSAP/gsap.utils; D/GSAP/UtilityMethods/random() | ok |
| `utils.md:15-19` | `clamp`, `mapRange`, `normalize`, `interpolate`, `snap` (incl. `{values, radius}`) | D/GSAP/gsap.utils; D/GSAP/UtilityMethods/snap(); D/GSAP/UtilityMethods/interpolate() | ok |
| `utils.md:20-22` | `wrap` (number and array), `wrapYoyo` | D/GSAP/UtilityMethods/wrap(); D/GSAP/gsap.utils | ok |
| `utils.md:23-25` | `random`, `shuffle` in place | D/GSAP/UtilityMethods/random(); D/GSAP/gsap.utils (shuffle) | ok (D05 correction holds) |
| `utils.md:26,55` | `distribute()` returns a function; config keys | D/GSAP/UtilityMethods/distribute() | ok |
| `utils.md:27-28` | `getUnit`; `unitize(fn, unit)` wraps a function | D/GSAP/UtilityMethods/unitize() | ok (D05 correction holds) |
| `utils.md:29` | `splitColor("#6fb936")` gives `[111, 185, 54]`; HSL option | D/GSAP/UtilityMethods/splitColor() | ok |
| `utils.md:30-33` | `toArray`, `selector`, `pipe`, `checkPrefix` | D/GSAP/gsap.utils | ok |

### references/react.md, frameworks.md, performance.md

| File:line | Claim | Source | Status |
|---|---|---|---|
| `react.md:4,11` | `npm install gsap @gsap/react`; `gsap.registerPlugin(useGSAP)` | R/React | ok |
| `react.md:16` | `useGSAP()` uses `gsap.context()`; layout effect on the client | R/React (SSR: useIsomorphicLayoutEffect) | ok |
| `react.md:45` | No second argument means an empty dependency array | R/React (dependencies: default `[]`) | ok |
| `react.md:47` | `revertOnUpdate` | R/React (revertOnUpdate) | ok |
| `react.md:54,66` | `contextSafe` from the return value and as the 2nd callback argument | R/React | ok |
| `react.md:114` | App Router needs `"use client"` | R/React (SSR) | ok |
| `react.md:121` | Strict Mode runs effects twice | R/React; R/frameworks | ok |
| `frameworks.md:9,50` | `ctx.revert()` on unmount; `ctx.add()` | R/frameworks; D/GSAP/gsap.context() | ok |
| `performance.md:49-51` | `quickTo` example | D/GSAP/gsap.quickTo() | ok |
| `performance.md:54` | `quickSetter(target, prop, unit)` | D/GSAP/gsap.quickSetter() | ok |
| `performance.md:67` | Hidden-tab throttling; `lagSmoothing(0)` | D/GSAP/gsap.ticker | fixed |

`cinematic-motion.md` uses only API forms that the rows above already check (`fromTo`, `clipPath`, `scrollTrigger`, `Flip.getState`/`Flip.from`, SplitText `mask`). It has no separate API claims.

## Decisions

- Draggable `"scroll"`, `"scrollTop"`, `"scrollLeft"`: the 3.15.0 source still accepts them (`~type.indexOf("scroll")` creates a ScrollProxy), but the current docs list none of them. I removed them. A skill should not teach undocumented API.
- Observer `preventDefault`: the source reads it, but the docs page does not list it. I kept it, because the paging pattern needs it to stop native scroll. A follow-up can replace it if GSAP removes the option.
- SplitText `mask`: I kept "`overflow: clip`" because the source sets that. The docs text "visibility: clip" is not a valid CSS value.
- Licence: I added 1 sentence about the Standard License restriction. The skill tells the worker to raise the question, not to decide it.
- "scrub wins over toggleActions" and "the base ease name equals `.out`" are only in the source. I kept both.

## Gotchas

- `WebFetch`-style summaries lose exact defaults. Fetch `https://gsap.com/docs/v3/<Page>.md` with `curl` and search the raw text. The index is `https://gsap.com/llms.txt`.
- Some pages exist only as HTML: the MotionPathHelper page is at `https://gsap.com/docs/v3/Plugins/MotionPathHelper/` and has no `.md` copy. Eases live under `D/Eases/`, not `D/Plugins/`.
- `https://gsap.com/standard-license` still has an old "Plain English Summary" in an HTML comment that mentions a paid Business licence. The live licence text has no fee; its only restriction is the Webflow-competitor one.

## Follow-ups (outside this unit)

- GSAP 3.15.0 adds `easeReverse` (directional easing). The skill does not mention it. A future motion update could add it to `core.md`.
- No test checks the API facts. If the skill changes often, a small script that greps the skill for known-wrong strings (for example `"Lower refreshes first"`) would stop regressions. That needs a plan.
