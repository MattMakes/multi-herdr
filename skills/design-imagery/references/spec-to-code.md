# Spec to code, and the screenshot loop

Use this file to build a page or screen from the specs written with `image-to-spec.md`, then prove the build matches the reference images. The goal is "visually faithful to the image, translated into real frontend", not "inspired by the image".

## 1. Build order

1. Read the project's stack and conventions first. Use its framework, styling method, and component patterns. If the design-system skill is available and the project has tokens, map the spec onto them instead of adding a parallel set.
2. Create tokens from `_system.md`: colors, type scale, spacing scale, radius, shadows. One token per spec value; no unnamed literals in components.
3. Load fonts that match the recorded character. Self-host or use the project's font pipeline. Set fallbacks with similar metrics.
4. Build the shared components (buttons, cards, nav) once, from the spec's component list.
5. Build sections in reference order, one at a time. Match the grid, anchor, and spacing of each spec.
6. Place real assets in the asset slots. If an asset is not available, use a correctly sized placeholder with the same aspect ratio and dominant color, and list it in the report.
7. Add the responsive intent from each spec. Check 1440, 1024, 768, and 390 px widths.

## 2. Anti-drift rules

The most common failure is a strong reference that becomes a generic page in code. While you build:

- Do not replace a distinctive section with a generic row of cards.
- Do not compress generous spacing. If the spec says 160 px section padding, use 160 px.
- Do not flatten the type contrast or the headline line count.
- Do not add elements the reference does not have: badges, pills, stat rows, extra buttons, decorative icons.
- Do not wrap open layouts in bordered or rounded containers.
- Do not swap the palette for framework default colors.
- Keep image treatments (overlay, duotone, grade) in CSS or in processed assets, not dropped.

## 3. Screenshot comparison loop

Run the loop per section. Use a browser tool, if available, or the project's screenshot or visual test tooling. If no capture method exists, report that the comparison could not run and list the manual checks the operator should do.

1. Serve the page locally.
2. Set the viewport to the spec's viewport width (and a height that fits the section).
3. Capture the section only, at device scale 1, to `design/compare/<nn>-<section>-build.png`.
4. Put the reference and the capture side by side at the same width. If an image tool can produce a pixel diff or an overlay at 50% opacity, produce it; otherwise compare by eye with the checks below.
5. Fill the comparison table.
6. Fix the largest failed check first. Capture again.
7. Stop when all checks pass, or after 3 rounds. Report what remains and why.

Checks (pass or fail per section):

| Check | Pass when |
|---|---|
| Structure | Same regions in the same order and the same composition anchor |
| Copy | Every string matches the spec exactly |
| Headline wrap | Same line count; break points within 1 word |
| Content width and gutters | Within 4% of the spec |
| Vertical spacing | Each recorded gap within 8 px or 10%, whichever is larger |
| Type size | Each level within 2 px of the spec |
| Colors | Each role matches its token; sampled build color within a visible-difference margin of the spec hex |
| CTA | Same style family, size within 4 px, primary and secondary still distinct |
| Image | Same crop focus, aspect ratio, and treatment |
| Contrast | Every text pair meets WCAG AA in the build |
| Extras | No element the reference does not have |
| Responsive | No overflow or overlap at 1024, 768, and 390 px |

Comparison table format for the report:

```
| Section | Structure | Copy | Wrap | Width | Spacing | Type | Color | CTA | Image | Contrast | Extras | Responsive | Notes |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| 01 Hero | pass | pass | fail: 3 lines vs 2 | pass | ... | ... | ... | ... | ... | ... | ... | ... | font fallback wider |
```

## 4. Common deviations and fixes

| Symptom | Likely cause | Fix |
|---|---|---|
| Headline wraps one line more | fallback font wider, or tracking missing | load the right font; apply the spec tracking; check `max-width` |
| Section feels cramped | padding taken from framework defaults | apply the spacing tokens |
| Colors look washed | image overlay missing, or color profile differs | add the overlay; compare in the same browser |
| Image focus wrong | `object-position` default | set the focal point from the spec crop |
| Build looks busier | extra borders, shadows, or containers | remove anything the reference lacks |
| Mobile overflow | fixed widths copied from the 1440 spec | use the grid and fluid units; recheck at 390 px |

## 5. Report

List: the code files touched, the token file, the comparison table, every placeholder asset, every value marked `estimated` in the specs that you had to choose, and every check that still fails with the reason.
