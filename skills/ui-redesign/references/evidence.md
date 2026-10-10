# Evidence: before/after screenshots and the report

A redesign is finished when the improvement is shown, not described. Capture the same views before and after, under the same conditions, and compare them.

## Capture setup

- **Tool.** A browser tool, if your harness has one. Otherwise a headless browser the project already uses (for example Playwright or Puppeteer in `devDependencies`). If neither exists, do not install one without approval; ask: `horch tell orchestrator "[<role>] QUESTION: No browser tool. May I add <tool> as a dev dependency, or should I report without screenshots?"`
- **Same conditions.** Same commit of the data, same seed, same fonts loaded, animations settled (wait for `document.fonts.ready` and network idle), reduced motion on for stable frames, same browser and device scale factor.
- **Widths.** 375 x 812, 768 x 1024, 1280 x 800, 1440 x 900. Add 320 when layout risk is high. Capture the first view and the full page.
- **States.** For each key component: default, hover, focus-visible, error, empty, loading, where they exist. Use forced-state classes or devtools state toggles.
- **Themes.** Light and dark when the product has both.
- **Naming.** `<dir>/<page>-<width>[-<state>][-dark].png`, with `before/` and `after/` folders. Store them where the plan says (for example `ai_docs/reports/<unit>/screenshots/`), or outside the repository if binaries are not wanted. Never write them into the app's public assets.

A minimal Playwright capture, if the project already has it:

```js
import { chromium } from "playwright";
const widths = [[375, 812], [768, 1024], [1280, 800], [1440, 900]];
const browser = await chromium.launch();
for (const [w, h] of widths) {
  const page = await browser.newPage({ viewport: { width: w, height: h }, reducedMotion: "reduce" });
  await page.goto(process.env.URL, { waitUntil: "networkidle" });
  await page.evaluate(() => document.fonts.ready);
  await page.screenshot({ path: `${process.env.OUT}/home-${w}.png` });
  await page.screenshot({ path: `${process.env.OUT}/home-${w}-full.png`, fullPage: true });
  const overflow = await page.evaluate(() => document.documentElement.scrollWidth > innerWidth);
  if (overflow) console.log(`horizontal scroll at ${w}`);
  await page.close();
}
await browser.close();
```

## Measurements to record

Record each before and after:

- Hard-gate results from the audit (contrast pairs, horizontal scroll per width, wrapped labels, hero action visible at 1280 x 800).
- The six-axis score, or the checklist counts (`n critical · n major · n minor`).
- Console errors and failed requests.
- If the project has them: Lighthouse or Core Web Vitals (LCP under 2.5 s, CLS under 0.1, INP under 200 ms), bundle size, accessibility test results.

A change that improves the look but drops a measurement is a regression. Fix it or revert it.

## Comparing

- Put each before and after side by side at the same width; describe the visible difference in one line.
- Check the do-not-change list against the after state (routes, field names, analytics IDs, legal text).
- Check every width, not only desktop. Mobile is where redesigns break.
- Mark anything you could not capture as `not checked`, with the reason.

## Report template

```
# Redesign report: <target>

Mode: <preserve | overhaul>   Scope: <pages>   Commits: <before sha> -> <after sha>
Reference: <none | URL or screenshot, status>

## Result
Score: <before> -> <after> (<axes or counts>)
Critical findings: <before n> -> <after n>
Gates fixed: <list>

## Changes (one increment per line)
1. <concern> - <files> - <check run and result>
...

## Before / after
| View | Before | After | Difference |
| --- | --- | --- | --- |
| home 375 | before/home-375.png | after/home-375.png | hero action now in first view |

## Preserved
<routes, field names, analytics IDs, copy, brand elements confirmed unchanged>

## Deferred or not done
<finding - reason - suggested next step>

## Not checked
<check - reason>
```

Keep the report factual: what changed, the evidence, what stayed, what remains.
