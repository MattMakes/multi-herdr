---
name: landing-page
description: Use when building, rebuilding or reviewing a landing, marketing, product, launch, waitlist or event page, or its hero or banner, from a brief, PRD or existing site.
---

# Landing Page

Build a distinctive, conversion-aware landing page end to end: brief, message, structure, copy, hero art, responsive build, performance, accessibility, then a generate-then-review loop with screenshots. One page, one audience, one primary action.

## When to use

- A new landing, marketing, product, launch, waitlist, pricing or event page.
- A rebuild of an existing landing page. Keep the brand assets, real copy and working URLs unless the brief says otherwise.
- A hero section, a website banner, or a social or ad banner that belongs to the same campaign.
- A review of someone else's landing page. Run steps 1, 8 and 9 only.

Not for dashboards, app screens, multi-step forms or docs sites. Say so and apply only the parts that fit.

## Inputs

- The brief or PRD: product, audience, the one action you want, proof you are allowed to use.
- Brand assets: logo, colors, type, photography, existing site. Use only what is supplied or verified.
- The stack: the project's framework and CSS approach. With no project, ship one self-contained `index.html` with CSS custom properties and no build step.
- If facts are missing (price, real customer names, real numbers), use marked placeholders and list them. Ask the orchestrator only when a missing fact changes the page structure: `horch tell orchestrator "[<role>] QUESTION: ..."`. Continue with the rest while you wait.

## Workflow

1. **Write the brief read.** One line: "Page kind for audience; primary action; vibe; aesthetic family." Then list: the visitor's problem, the product's one differentiator, the top 3 objections, the proof you have. Check: every later decision traces to this read. If a related `art-direction` or `ui-taste` skill is available, use it here to pick the aesthetic family.
2. **Set the message hierarchy.** Write, in order: the value proposition (one sentence, at most 12 words), 3 supporting benefits, 1 proof point per objection, and the primary CTA label (2 to 3 words, a verb first). Check: a stranger who reads only the hero headline and CTA can say what the product does and what clicking does.
3. **Choose the page pattern and section order.** Load `references/sections.md`. Pick 1 page pattern; map each section to a job (hook, proof, explain, handle objection, convert). Check: every section has a job; no 2 sections have the same job; the page has at least 4 different layout families.
4. **Write the copy.** Load `references/copy.md`. Write every visible string before you style anything. Check: the copy self-audit in `copy.md` passes; zero invented numbers, names or logos remain without a `TODO` marker.
5. **Direct the hero and banners.** Load `references/hero.md`. Pick a hero paradigm and a real visual (supplied asset, generated image, real product UI, or a labeled placeholder slot with the exact aspect ratio). Check: the hero fits the first viewport at 390 px and 1440 px with the CTA visible.
6. **Build mobile first.** Use the project's stack. Set design tokens once (color, type scale, spacing, radius). Declare the below-768 px layout for every multi-column section. Load `references/performance-budget.md` before you add images, fonts, video or scripts. Check: the page renders with no console errors and no horizontal scroll at 320 px.
7. **Add motion last, if at all.** One orchestrated entrance or one scroll device beats many small effects. If the `motion-gsap` skill is available, use it for scroll work. Check: every animation has a one-sentence reason, uses only `transform` and `opacity`, and stops under `prefers-reduced-motion: reduce`.
8. **Run the review loop.** Load `references/review-loop.md`. Screenshot at 390, 768 and 1440 px, score against the rubric, fix, and repeat until no item fails (at most 3 rounds, then report what remains). Check: the final screenshots and the scored rubric exist.
9. **Report.** Give the file paths, the screenshot paths, the rubric result, the measured budgets, and the list of placeholders and facts that the owner must supply.

## Rules

- **One primary action.** One primary CTA intent per page. You may repeat it (hero, after proof, final section, sticky bar), but with the same label every time. Secondary actions look secondary.
- **The audience picks the aesthetic.** Do not pick a style at random or by habit. Regulated, public-sector and accessibility-first audiences override taste.
- **Hero discipline.** Headline at most 2 lines at 1440 px. Subtext at most 20 words. At most 4 text elements: optional eyebrow, headline, subtext, CTAs (1 primary and at most 1 secondary). Logo walls, ratings and feature lists go in the section under the hero.
- **No invented proof.** No fake testimonials, logos, counts, ratings, "limited seats" or countdowns. Show a number only when it comes from the brief or a dated, verified source. Placeholders carry a visible `TODO` in the code and a line in your report.
- **No fake product UI.** Do not draw a product screenshot with styled `<div>` boxes. Use a real screenshot, a real live component, a generated image, or a labeled placeholder.
- **One theme, one accent, one radius system.** The page does not flip light and dark between sections. The accent color is the same in every section. Corner radii follow one written rule.
- **Copy is content first.** No filler verbs ("elevate", "seamless", "unleash", "revolutionize"), no em dash or en dash in visible copy, no cute wordplay you cannot explain.
- **Accessibility is part of done.** WCAG 2.2 AA: text contrast 4.5:1 (3:1 for 24 px+ or 18.66 px+ bold), visible focus, 24 x 24 px minimum pointer targets (44 x 44 px for primary mobile CTAs), one `h1`, no skipped heading levels, labels above inputs, alt text on meaningful images, keyboard access to every control, pause controls for anything that moves for more than 5 s.
- **Performance is part of done.** Meet the budgets in `references/performance-budget.md` or record why not.
- **Stay in your session.** The worker drafts, screenshots, critiques and revises in its own session. Do not start a subagent or a nested agent CLI. For an independent critique, ask the orchestrator for a separate critic worker.

## Review checklist

- [ ] The brief read and message hierarchy are written down, and the page follows them.
- [ ] The hero headline plus CTA explains the product and the action.
- [ ] Every section has one job; at least 4 layout families; no 3 image-text splits in a row.
- [ ] One CTA label per intent across nav, hero, body and footer; no CTA label wraps at 1440 px.
- [ ] Zero invented proof; every placeholder is marked `TODO` and listed.
- [ ] Zero `—` and `–` characters in visible copy; zero banned filler verbs.
- [ ] Screenshots at 390, 768 and 1440 px show no overflow, clipping, overlap or orphaned CTA.
- [ ] Contrast, focus, target size, headings, alt text and keyboard checks pass.
- [ ] `prefers-reduced-motion: reduce` gives a static, complete page.
- [ ] LCP, CLS and weight budgets are measured and met, or the gap is reported.

## References

- `references/sections.md`: load at step 3 for page patterns, section patterns and layout rules.
- `references/copy.md`: load at step 4 for headline and CTA formulas, section copy shapes and the copy self-audit.
- `references/hero.md`: load at step 5 for hero paradigms, hero visuals, banner sizes and banner art direction.
- `references/performance-budget.md`: load at step 6 for weight, Core Web Vitals, image, font and script budgets and how to measure them.
- `references/review-loop.md`: load at step 8 for the screenshot procedure, the scored rubric, the measurement snippets and the critic-worker request.
