# Generate-then-review loop

Load at workflow step 8. The worker drafts, screenshots, critiques against a fixed rubric and revises, all in its own session. Do not start a subagent or a nested agent CLI. For an independent critique, ask the orchestrator for a second worker (section 6).

## 1. Before the first draft: the direction note

Write 3 short paragraphs and keep them in your notes. The review compares the page against them.

1. **Vision and atmosphere**: the aesthetic family, the mood on arrival, how the hierarchy should feel while scrolling, where color carries emotion.
2. **Type, motion and narrative arc**: how the type should feel (authoritative, warm, precise), how interactions should feel (snappy, smooth, still), and how the page moves from first impression through proof to the final CTA.
3. **Reference qualities**: abstract references (a kind of space, a print tradition, an architectural style, a material) and what each one means for the page. No brand names to copy.

## 2. One review round

1. **Serve the page.** Use the project's dev server, or a static server for a single file (for example `python3 -m http.server 8000` in the page folder). Static `file://` URLs are acceptable if fonts and scripts load.
2. **Capture screenshots.** Use a browser tool if the harness has one, or a headless browser that the project already uses (for example an existing Playwright setup). Capture:
   - full page at 390 x 844, 768 x 1024 and 1440 x 900;
   - the first viewport only at each of the 3 widths (the fold check);
   - name them `review/r<round>-<width>.png` and `review/r<round>-<width>-fold.png` next to the page, or where the brief says.
3. **Run the measurements** in section 4 at 390 and 1440 px.
4. **Score the rubric** in section 3. Write each score with one line of evidence (a screenshot name, a selector, a number).
5. **Fix** every item that scores 0, highest weight first. Then fix 1s where the fix is cheap.
6. **Repeat** until no item scores 0 and the total is at least 80% of the maximum. Stop after 3 rounds and report what remains.

If no screenshot capability exists: do a code-level review with section 4 run in any available JavaScript runtime or by inspection, mark the visual items "not verified", and tell the orchestrator that visual review is pending.

## 3. Rubric

Score each item 0 (fails), 1 (weak) or 2 (good). Weight 3 items are blockers: a 0 there fails the round.

| # | Item | Weight | A 2 looks like |
|---|---|---|---|
| 1 | Message clarity | 3 | Headline plus CTA say what it is and what happens next, without the rest of the page |
| 2 | Hero fit | 3 | CTA visible in the first viewport at 390 and 1440 px; headline at most 2 lines at 1440 px |
| 3 | Honest content | 3 | No invented proof; every placeholder marked `TODO` and listed |
| 4 | Accessibility | 3 | Measurements in section 4 pass; focus visible; keyboard reaches every control |
| 5 | Responsive integrity | 3 | No overflow, clipping or overlap at 320, 390, 768, 1440 px |
| 6 | Hierarchy and flow | 2 | Each section has one job; proof sits before each CTA; the eye path is obvious |
| 7 | Distinctiveness | 2 | Matches the direction note; none of the default tells (purple glow, 3 equal cards, centered blob hero, glass everywhere) |
| 8 | Typography | 2 | One scale, readable measure, no widows in headings, no clipped descenders |
| 9 | Color and theme | 2 | One theme, one accent, contrast passes, the accent marks actions |
| 10 | Layout variety | 2 | At least 4 layout families; eyebrow count within 1 per 3 sections; no 3 split rows in a row |
| 11 | Copy quality | 2 | Copy self-audit passes; zero banned words or dash characters |
| 12 | Imagery | 2 | Real or clearly placeholder visuals; no fake UI from boxes; consistent treatment |
| 13 | Motion | 1 | Every animation has a reason; reduced motion gives a complete static page |
| 14 | Performance | 2 | Budgets in `performance-budget.md` met or the gap is recorded |
| 15 | States | 1 | Form loading, success and error states and hover, focus, active states exist |

Total = sum of (score x weight). Maximum: 2 x 33 = 66. Pass: no item at 0 and a total of at least 53 (80%).

## 4. Measurement snippets

Run in the browser console or through the browser tool's JavaScript evaluation, at each width.

```js
(() => {
  const vw = document.documentElement.clientWidth, vh = innerHeight;
  const out = {};
  // Horizontal overflow: elements wider than the viewport.
  out.overflow = [...document.querySelectorAll('body *')]
    .filter(e => e.getBoundingClientRect().right > vw + 1)
    .slice(0, 10).map(e => e.tagName + '.' + (e.getAttribute('class') || ''));
  // Headings: one h1, no skipped levels.
  const hs = [...document.querySelectorAll('h1,h2,h3,h4,h5,h6')].map(h => +h.tagName[1]);
  out.h1Count = hs.filter(l => l === 1).length;
  out.skippedLevels = hs.some((l, i) => i && l - hs[i - 1] > 1);
  // Images: missing alt or missing dimensions; eager images below the fold.
  const imgs = [...document.images];
  out.noAlt = imgs.filter(i => !i.hasAttribute('alt')).length;
  out.noSize = imgs.filter(i => !i.getAttribute('width') && getComputedStyle(i).aspectRatio === 'auto').length;
  out.eagerBelowFold = imgs.filter(i => i.loading !== 'lazy' && i.getBoundingClientRect().top > vh).length;
  // Small targets: links and buttons under 24 x 24 px.
  out.smallTargets = [...document.querySelectorAll('a,button,input,select,textarea,[role=button]')]
    .filter(e => { const r = e.getBoundingClientRect(); return r.width && (r.width < 24 || r.height < 24); }).length;
  // Visible dash characters and banned words.
  const text = document.body.innerText;
  out.dashes = (text.match(/[–—]/g) || []).length;
  out.banned = (text.match(/elevate|seamless|unleash|empower|revolutioni[sz]e|next-gen|supercharge|cutting-edge/gi) || []);
  // Primary CTA in the first viewport: mark it with data-cta="primary".
  const cta = document.querySelector('[data-cta="primary"]');
  out.ctaAboveFold = cta ? cta.getBoundingClientRect().bottom <= vh : 'no [data-cta=primary]';
  // Layout shift and LCP so far.
  out.cls = performance.getEntriesByType('layout-shift').reduce((s, e) => s + (e.hadRecentInput ? 0 : e.value), 0);
  const lcp = performance.getEntriesByType('largest-contentful-paint').pop();
  out.lcpMs = lcp ? Math.round(lcp.startTime) : 'n/a';
  return out;
})();
```

Some browsers expose `layout-shift` and `largest-contentful-paint` only to a `PerformanceObserver` with `buffered: true`; use one if the entries are empty.

Contrast check for one pair of colors (WCAG relative luminance):

```js
const L = h => { const c = h.match(/\w\w/g).map(x => parseInt(x, 16) / 255)
  .map(v => v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4);
  return 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]; };
const ratio = (a, b) => { const [x, y] = [L(a), L(b)].sort((p, q) => q - p); return (x + 0.05) / (y + 0.05); };
ratio('#1f2937', '#ffffff'); // 4.5 or more for body text, 3 or more for large text and UI parts
```

Check at least: body text on each background, CTA label on CTA fill, CTA fill on page background (3:1), placeholder and helper text, focus ring on its background (3:1), text over hero imagery (sample the darkest and lightest area behind the text).

Keyboard pass: press Tab from the top. Every control gets a visible focus ring in a logical order, nothing is hidden under a sticky bar, and Escape closes any open menu or dialog.

Reduced motion pass: emulate `prefers-reduced-motion: reduce` (browser tool setting or DevTools rendering panel). All content is visible in its final state and nothing moves for more than a moment.

## 5. How to critique your own draft

Look at the screenshots before you reread the code. For each screenshot, answer in writing:

1. Where does the eye land first, second, third? Is that the intended order?
2. What would a visitor in the brief's audience misunderstand?
3. Which section could be removed with no loss? Remove it or give it a job.
4. Which element looks like a default template? Replace it with a decision from the direction note.
5. What breaks at this width that does not break at the others?

Then score the rubric. Fix the cause, not the symptom: if 3 sections look alike, change the section plan, not the padding.

## 6. Asking for a separate critic

Ask when the page is high stakes, when 2 rounds did not reach a pass, or when the brief asks for an independent review. Send one message:

```
horch tell orchestrator "[<role>] QUESTION: I request a critic worker for the landing page. The page is at <path>. The screenshots are at <path>/review/. The rubric is in the landing-page skill, file references/review-loop.md. I continue with <independent work> while I wait."
```

When the critique arrives, treat each finding as a rubric item: verify it in the screenshots, fix it or record why not, then run one more round.
