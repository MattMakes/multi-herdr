# Audit checklist and fix priority

Use this checklist when the `ui-taste` skill is not available. If it is, its `references/rubric.md` and `references/anti-patterns.md` replace sections 1 to 8 here, and section 9 still applies.

For every finding, record: severity (`critical`, `major`, `minor`), the evidence (`file:line`, a screenshot name, or a measurement), and a one-line fix. A check you could not run is `not checked`, never a pass.

## 1. Typography

- [ ] Display face is a default (Inter, Roboto, Open Sans, Arial, system stack) with no pairing. major
- [ ] More than 3 font families on a page. major
- [ ] Body text below 16 px, or any text below 12 px. major
- [ ] Prose wider than 75 characters or narrower than 45. major
- [ ] Sizes do not follow a scale (consecutive sizes closer than 1.2x), or more than 6 sizes. minor
- [ ] Headings at the same weight as body, or only 400 and 700 with no deliberate contrast. minor
- [ ] Display headings with loose tracking and 1.3+ line height. minor
- [ ] Italic emphasis words inside headings. major
- [ ] All-caps labels above every section. major
- [ ] Proportional figures in tables, prices, stats. minor
- [ ] One-word last lines on headings (fix with `text-wrap: balance`). minor
- [ ] Straight quotes, `...`, `--`. minor

## 2. Color and surfaces

- [ ] Body text or focus ring below contrast targets (4.5:1 body, 3:1 large text, icons, borders, focus). critical
- [ ] Button text unreadable on its fill, or text inherited into a dark section. critical
- [ ] Purple-blue AI gradient, gradient text, aurora blobs, glows. critical
- [ ] Pure `#000` or `#fff` base surfaces. major
- [ ] More than one accent, or the accent covers much more than 5% of a view. major
- [ ] Warm and cool grays mixed; untinted grays beside a chromatic accent. minor
- [ ] Hard-coded colors outside tokens. major
- [ ] One section flips light or dark for no system reason. major
- [ ] Black shadows at low opacity everywhere, inconsistent light direction. minor

## 3. Layout and spacing

- [ ] Horizontal scroll at any of 320, 375, 414, 768, 1280, 1440 px. critical
- [ ] Hero `min-height: 100vh` with everything centered; the primary action below the fold at 1280 x 800. critical
- [ ] Three equal icon cards; card inside a card. major
- [ ] The same section layout repeated; three or more zigzag rows in a row. major
- [ ] No max-width container; content edge to edge on wide screens. major
- [ ] Spacing values off any scale; every section padded the same. minor
- [ ] Buttons in card rows not aligned to the bottom; list starts at different heights across pricing columns. minor
- [ ] Bento grid with empty cells or all-identical cells. major
- [ ] `100vh` or `h-screen` for full-height sections (mobile jump); `100vw` widths. minor
- [ ] Flexbox percentage math where a grid is simpler. minor
- [ ] `z-index: 9999` and other values outside a scale. minor

## 4. Navigation and footer

- [ ] Nav wraps to two lines at 1024 px, or is taller than 80 px. major
- [ ] No indication of the current page. major
- [ ] Generic nav or four-column link-farm footer on a small site. minor
- [ ] Links to `#` or dead routes. major
- [ ] No skip-to-content link. major

## 5. Interaction and states

- [ ] Focus outline removed without a replacement. critical
- [ ] Missing hover, focus-visible, active, or disabled styles. major
- [ ] No loading, empty, or error states; `window.alert()` for errors. major
- [ ] Placeholder used as label; errors by color only. major
- [ ] Touch targets under 44 x 44 px. major
- [ ] Hover-only menus or actions. major
- [ ] Confirmation dialogs for reversible actions; celebratory toasts. minor

## 6. Motion

- [ ] No `prefers-reduced-motion` handling. major
- [ ] `transition: all`; animating width, height, top, left, margin, padding. major
- [ ] Scroll event listeners; parallax; fade-up on every section; infinite decorative loops. major
- [ ] Bounce or elastic easing on UI state; browser `ease` everywhere. minor
- [ ] Auto-rotating carousels with no pause. major

## 7. Content

- [ ] Invented metrics, testimonials, logos. critical
- [ ] Lorem ipsum, Jane Doe, Acme, identical dates, one avatar for many people. major
- [ ] Filler phrases (unleash, seamless, elevate, next-gen). major
- [ ] "Oops!" errors, exclamation marks in success messages, passive voice. minor
- [ ] Title Case On Every Heading. minor
- [ ] Several labels for one intent ("Get started", "Sign up", "Try free"). minor

## 8. Imagery and icons

- [ ] Fake browser or phone chrome; `<div>` dashboards as product previews. critical
- [ ] Two or more icon libraries; emoji as icons; mixed stroke widths. major
- [ ] Generic stock or blob illustration; untouched stock photos. minor
- [ ] Subject-led page (product, place, people) with no real images. major
- [ ] Hero image lazy-loaded; images without `width` and `height`; missing `alt`. major
- [ ] Missing favicon, `<title>`, description, or social preview tags. minor

## 9. Code and structure

- [ ] `<div>` soup where `nav`, `main`, `section`, `article`, `footer` belong. minor
- [ ] Inline styles mixed with the styling system. minor
- [ ] Imports of packages that are not installed. critical
- [ ] Commented-out dead code, debug output. minor
- [ ] Missing 404 page, missing back navigation, missing legal links. minor

## Fix priority

Order increments by visual impact per unit of risk. Stop when the brief is satisfied.

1. **Broken and inaccessible.** Contrast failures, horizontal scroll, missing focus, unreadable buttons, dead links. Always first.
2. **Typography.** Font pairing, scale, measure, leading. The largest visible gain for the lowest risk.
3. **Color.** One accent, tinted neutrals, tokens, theme lock.
4. **Spacing and rhythm.** One scale, container width, varied section padding, alignment.
5. **States.** Hover, focus, active, disabled, loading, empty, error.
6. **Component voice.** Replace generic patterns (three cards, link-farm footer, fake chrome) with fitting ones.
7. **Hero and key sections.** Recompose the top of the page and the main conversion path.
8. **Motion.** Remove unmotivated motion first; add only motion that has a reason.
9. **Full block replacement.** Only for a block that cannot be saved.

Choosing the size of the job:

- Information architecture, content, and SEO are sound: targeted evolution (priorities 1 to 6). Most of the value at much less risk.
- The debt is structural (no system, broken mobile, broken navigation): full visual redesign, with content and routes preserved.
- The brand itself is changing: treat it as a new build with the content carried over.
