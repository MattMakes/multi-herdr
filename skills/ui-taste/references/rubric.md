# Rubric: hard gates, six-axis score, report

Use this file for the self-critique in build mode and for every review. A review has three parts: hard gates (pass or fail, measured), the six-axis score (judgment with anchors), and a ranked fix list.

## 1. Hard gates

Each gate is pass or fail. Record the evidence (a measurement, a `file:line`, a screenshot name). A gate you could not check is `not checked`, never `pass`.

Severity:
- **critical**: ships broken or reads as AI-generated at a glance.
- **major**: a trained eye sees it immediately.
- **minor**: polish.

### Accessibility and legibility

| # | Gate | How to check | Severity |
| --- | --- | --- | --- |
| A1 | Body text contrast is 4.5:1 or more (WCAG 2.x) against its actual background. | Pair every `color` with the computed `background-color` of its container. Compute the ratio (procedure below). Check text on cards, tinted panels, dark sections, and image overlays. | critical |
| A2 | Large text (24 px+, or 18.66 px+ bold), icons, input borders, and focus rings are 3:1 or more. | Same as A1. Check the focus ring against both the element and the page. | critical |
| A3 | Button text is readable on its fill. | No pair where text and fill are within 5% OKLCH lightness. An accent fill uses an `--color-accent-ink` (or equal) token, not a hard-coded white. | critical |
| A4 | Every interactive element shows a focus ring on keyboard focus, and the ring appears instantly. | Tab through the page. Search `outline: none`, `outline-none`, `outline: 0`; each hit needs a `:focus-visible` replacement. The ring must not be in a `transition`. | critical |
| A5 | Touch targets are 44 x 44 CSS px or more. | Measure buttons, icon buttons, nav links, and form controls in devtools at 375 px. | major |
| A6 | No state is signaled by color alone. | Errors have text and an icon or border change; required fields have a label marker. | major |
| A7 | All motion has a `prefers-reduced-motion: reduce` fallback. | Every `@keyframes`, `transition` on `transform`, and JS animation is covered. | major |
| A8 | Images that carry meaning have useful `alt`; decorative SVG has `aria-hidden="true"`. | Search `<img` and `<svg`. | major |

### Typography

| # | Gate | How to check | Severity |
| --- | --- | --- | --- |
| T1 | At most 3 font families. Mono counts if used outside code. | List every `font-family` value. | major |
| T2 | The display face is not an unmotivated default (Inter, Roboto, Open Sans, Arial, Helvetica, Lato, Poppins, Montserrat, system-ui). | Read the display token. A default passes only with a written reason (brand rule, existing system, public-sector or accessibility brief). | major |
| T3 | Body text is 16 px or more; nothing is below 12 px. | List computed font sizes. | major |
| T4 | The type scale has a fixed ratio of 1.2 or more between consecutive steps, and at most 6 distinct sizes on the page. | List the sizes in use, sort them, divide each by the one below. Ratios below 1.2 (for example 16 to 18 px) mean two sizes that read as one. | minor |
| T5 | Prose line length is 45 to 75 characters. | `max-width` of prose containers in `ch`, or count characters on one rendered line at 1440 px. | major |
| T6 | Line height: body 1.5 to 1.65; display 1.0 to 1.15; all-caps display 1.0 or more. | Read computed `line-height`. | minor |
| T7 | Headings are not italic, and no `<em>` or `<i>` inside a heading is italic. | Search `font-style: italic`, `italic` classes, and `<em>` inside `h1` to `h6`. | major |
| T8 | Numbers in tables, prices, and stats use `font-variant-numeric: tabular-nums`. | Search the containers that show columns of numbers. | minor |

### Color and tokens

| # | Gate | How to check | Severity |
| --- | --- | --- | --- |
| C1 | One accent hue (two at most, with a stated role each). | List chromatic tokens and where they are used. | major |
| C2 | The accent covers about 5% of any view or less. | Look at each full-width screenshot. Large accent fills, accent section backgrounds, and accent display text all count. | major |
| C3 | No pure `#000` / `#000000` or `#fff` / `#ffffff` base surface or body text, unless the brand system says so. | Search; see `anti-patterns.md`. | major |
| C4 | Neutrals are tinted toward the anchor hue (OKLCH chroma 0.005 or more), and one gray family is used. | Read the neutral tokens. A warm accent with cool grays fails. | minor |
| C5 | No color or `font-family` value outside the token block. | Search for hex, `rgb(`, `hsl(`, `oklch(` and `font-family` outside the token file. Tailwind arbitrary values (`bg-[#...]`) count. | major |
| C6 | No gradient text, no purple-to-blue or cyan-to-magenta gradient, no aurora blobs. | Search; see `anti-patterns.md`. | critical |
| C7 | Light or dark is locked per page. A section that flips its background also sets its text color in the same rule. | Check each section with a dark background for inherited dark text. | critical if text is unreadable, else minor |

### Layout and responsive

| # | Gate | How to check | Severity |
| --- | --- | --- | --- |
| L1 | Every margin, padding, and gap is on one spacing scale (4 px base). | Search for raw `px` values in spacing properties and Tailwind arbitrary spacing (`p-[17px]`). | minor |
| L2 | No horizontal scroll at 320, 375, 414, 768, 1280, and 1440 px. | Render each width; check `document.documentElement.scrollWidth <= innerWidth`. `html, body { overflow-x: clip; }` is the safety net, not the fix. | critical |
| L3 | No button, nav link, tab, or CTA label wraps to two lines at any width. | Render each width. | major |
| L4 | At 1280 x 800 the hero headline, its supporting line, and the primary action are visible without scrolling. The headline is at most 3 lines at 1280 px. | Screenshot at 1280 x 800. | major |
| L5 | No three-equal-column icon-card row, no card inside a card, no repeated section layout. | List sections and their layout family. | major |
| L6 | Grid tracks that hold images use `minmax(0, 1fr)`, not bare `1fr`; display headings have `overflow-wrap: anywhere; min-width: 0`. | Search `grid-template-columns` and the heading rules. | minor |
| L7 | Z-index values come from a named scale; none above the scale (no `9999`). | Search `z-index`, `z-[`. | minor |

### States and content

| # | Gate | How to check | Severity |
| --- | --- | --- | --- |
| S1 | Each interactive element has default, hover (inside `@media (hover: hover)`), focus-visible, active, and disabled styles. | Read the component styles. | major |
| S2 | Async and data UI has loading, empty, error, and success states. | Read the component; trigger them if you can. | major |
| S3 | Inputs keep a constant border width in every state; the label is above the input; the helper or error slot reserves its height. | Read the input styles. | major |
| S4 | No invented metric, testimonial, customer, or logo. | Compare every number and name with the brief. | critical |
| S5 | No placeholder names or stock filler (Jane Doe, Acme, Lorem ipsum). | Search; see `anti-patterns.md`. | major |
| S6 | Every requested deliverable is present and complete; no stub comments. | Search `TODO`, `// ...`, `rest of`, `similar to above`; compare deliverables with the request. `TODO(image)` slots pass only when the handoff lists them. | critical |

## 2. Six-axis score

Score each axis 1 to 5. Any axis below 3 needs a fix before handoff. Anchor your score to the descriptions; do not average impressions.

| Axis | 1 | 3 | 5 |
| --- | --- | --- | --- |
| **Intent** | A layout with no position. Could be any product. | The structure fits the job, but the page takes no stance. | A clear point of view; every section serves the one job of the page. |
| **Hierarchy** | Everything has the same weight; the eye has no entry point. | Primary and secondary are clear; the third level is muddy. | In 2 seconds a reader knows the primary, secondary, and tertiary items. |
| **Craft** | Contrast, spacing, or states are broken. | Bones are right; details drift (off-scale gaps, a missing state, uneven radii). | Every gate passes; rules, radii, tracking, and alignment are exact. |
| **Specificity** | Template content and default patterns; swap the logo and it is another company. | Some brief-specific content; generic structure. | Copy, structure, and imagery belong only to this brief. |
| **Restraint** | Decoration everywhere: glows, gradients, badges, motion on every section. | Some ornament does no work. | Every element earns its place; removing anything loses information. |
| **Usability** | The primary task is hard to find or complete; mobile is broken. | The task works; friction on small screens or keyboard. | The task is obvious and fast on every width, with keyboard and touch. |

Total out of 30. Read the verdict with the gates:

- **Fail**: any critical gate fails, or any axis is 1.
- **Needs work**: any major gate fails, or any axis is 2, or total below 20.
- **Ship**: all critical and major gates pass, every axis 3 or more, total 20 or more.

## 3. Measurement procedures

**Contrast ratio (WCAG 2.x).** For each color, convert sRGB channels `c` (0 to 1) to linear: `c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ^ 2.4`. Luminance `L = 0.2126 R + 0.7152 G + 0.0722 B`. Ratio `= (L_light + 0.05) / (L_dark + 0.05)`. For OKLCH tokens, convert to sRGB first, or use the browser: devtools shows the ratio in the color picker. Quick pre-check: if two OKLCH lightness values differ by less than 50 percentage points, compute the full ratio. If the project has APCA tooling, body text needs Lc 60 or more, large text and UI Lc 45 or more.

**Type scale.** Sort the distinct font sizes. Divide each size by the one below. Common ratios: 1.2 (minor third), 1.25 (major third), 1.333 (perfect fourth), 1.5 (perfect fifth). Steps closer than 1.2 do not read as hierarchy.

**Line length.** Count characters (with spaces) on a full line of body text at 1440 px, or read `max-width` in `ch`. Target 65 ch.

**Spacing scale.** Collect every `padding`, `margin`, `gap`, `inset` value. Each must equal a scale token. In Tailwind, any `[...]` arbitrary spacing value is a miss unless it maps to a token.

**Accent footprint.** On a full-width screenshot, estimate the area of accent-colored fills and accent text. More than about 5% of the view is a fail, except for a single deliberate accent band in the system.

**No horizontal scroll.** In the browser console at each width: `document.documentElement.scrollWidth > window.innerWidth` must be `false`.

**Widths.** 320, 375, 414, 768, 1280 x 800, 1440 x 900. Name screenshots `<page>-<width>.png`.

## 4. Report format

```
UI review: <target> (<files or URL>, <commit or date>)
Design read: <one line>
Checked at: <widths>  Not checked: <gates or widths, with reason>

Verdict: <Fail | Needs work | Ship>   Score: <n>/30
Axes: Intent <n> · Hierarchy <n> · Craft <n> · Specificity <n> · Restraint <n> · Usability <n>

Findings (most severe first)
[critical] <gate id or tell name> - <file:line or screenshot>
  Evidence: <measurement or quote>
  Fix: <one concrete change>
...
Summary: <n> critical · <n> major · <n> minor
Top 3 fixes by impact: 1. ... 2. ... 3. ...
```

Rank findings by severity, then by how much of the page they affect. Put a fix on every finding. Say what you could not check.
