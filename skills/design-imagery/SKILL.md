---
name: design-imagery
description: Use when a visual design should be explored or locked as images before code (web or mobile screen concepts, brand boards, logo explorations), or when a reference image or screenshot must become a design spec and then matching code.
---

# Design Imagery

Use images as the design source of truth: generate or receive reference images, turn them into a measured spec, then build code that matches them and prove the match with screenshots. The image decides the design; the code translates it.

## When to use

- A web page, section, or mobile flow where visual quality is the main requirement, and the design is not yet fixed.
- A brand board, logo exploration, or identity concept the operator wants to see before anyone commits to tokens or code.
- The operator supplies a mockup, screenshot, or competitor reference and asks for a spec or a faithful build.
- A built page must be checked against its reference images.

Use it only when one of these is true:

- The harness has an image generation tool. You can then run every step.
- The operator or the task supplies reference images or screenshots. Skip generation and start at step 5.

Without either, do not describe imaginary images. Run steps 5 to 8 on any screenshots you can capture, or report to the orchestrator: `[<role>] BLOCKED: no image generation tool and no reference images for <task>.`

Do not use it for bug fixes, structural refactors, or tasks that already have a precise design system; build those directly. No specific image API is required. Use the generation tool your harness provides, with its own parameters.

## Inputs

- The brief: product, audience, the job of each page or screen, and the platform (web, iOS, Android, cross-platform).
- Any constraints: brand colors, fonts, existing tokens, copy, required sections, accessibility target (default WCAG AA).
- Reference images, if supplied. Treat them as a quality bar and a rhythm source. Do not copy their logo, brand name, slogan, or unique assets.
- The target codebase and stack, if the task includes implementation.

## Workflow

1. **Classify the job and count the images.** Decide the mode: web concept, mobile concept, brand board, logo exploration, or reference-only. Count sections or screens. Defaults: hero only = 1, landing page = 6, full website or marketing site = 8, mobile flow = at least 3 screens, logo exploration = 4 to 6 directions, brand board = 1 overview. Check: write the count and the section or screen list in your notes before you generate anything.

2. **Lock a direction brief.** Write one short brief that every prompt reuses: theme, palette (1 primary, 1 secondary, 1 accent, a neutral scale, with hex values), type character, radius language, image treatment, texture level, and one concept spine. For mobile, add the platform mode and the device frame. If the ui-taste or art-direction skill is available, use it to choose the direction. Check: the brief exists as text and names exact hex values.

3. **Write the prompts.** Write one prompt per section or screen. Each prompt repeats the direction brief, then states the section job, the composition anchor, the background mode, the readable copy, and the exclusions. Load the reference for the mode: `web-prompts.md`, `mobile-prompts.md`, or `brand-board-prompts.md`. Check: the number of prompts equals the count from step 1, and no two adjacent sections share the same composition anchor.

4. **Generate and review each image.** Generate one image per prompt, label it `Section N of M: <name>` or `Screen N of M: <name>`, and save it under the task's asset or report folder. Review each image against the checklist below. Regenerate a failed image as a fresh image from an edited prompt. Do not crop or zoom an earlier image to get a section or a detail. Check: every planned image exists, every image passes the checklist, and the set reads as one product.

5. **Analyze each image into a spec.** Load `image-to-spec.md`. For every image, record the layout grid, type scale, colors, spacing, components, exact readable text, assets, and an `unknowns` list. Measure in pixels against the image's real width; mark each value `measured` or `estimated`. If text or a component is too small to read, generate a closer detail image of the same section, or ask the operator for one. Check: one spec file per image, and each `unknowns` item has a decision or a question.

6. **Implement from the spec.** Load `spec-to-code.md`. Turn the spec into tokens first (color, type, spacing, radius), then build sections in image order. Copy the reference; do not improve it into a generic layout. Resolve a gap in this order: keep the visible design language, keep layout and spacing logic, keep the component family, then generate a detail image, and only then pick the most buildable faithful option. Check: every spec value appears as a token or a style, and the page renders without errors.

7. **Compare screenshots and fix.** Capture each section at the reference's viewport width with a browser tool, if available. Compare against the reference with the checks in `spec-to-code.md`. Fix the largest deviation first, then capture again. Stop when every check passes or after 3 rounds; report what remains. Check: a comparison table per section with pass or fail per check.

8. **Report.** Give the image list with paths, the spec paths, the code paths, the comparison table, and every `estimated` value or deviation that remains. Check: a fresh reader can find each artifact from the report alone.

## Rules

- One image per section or screen. Never put several sections in one tall image, and never return one "best" image instead of the set. Use horizontal frames for web sections (16:9, 16:10, or 21:9) and portrait frames for mobile screens.
- Readable beats dense. If the text in an image is too small to read, the image fails. Split the content or regenerate larger.
- Keep one brand world across the set: same palette, type family and scale, CTA family, radius, image treatment, and device frame.
- Vary composition, not identity. Do not default to text-left, image-right. Use at least 3 composition anchors in a set of 4 or more sections.
- Keep the hero or first screen calm: one focal point, a headline of 1 to 3 lines (about 5 to 10 words), one primary action, no pills, fake stats, or system labels.
- Avoid boxes inside boxes. Use one framing move per section.
- Use believable product copy. Do not use the words unleash, elevate, revolutionize, next-gen, seamless, or transformative, and do not use placeholder brands such as Acme, Nexus, Flowbit, NovaCore, or Quantumly.
- Do not use purple-to-blue or pink-to-orange default gradients, mesh blobs, neon glow without purpose, or gradient headline text. Low-chroma, palette-matched gradients are allowed.
- Text over an image needs a scrim, fade, or mask, and must meet WCAG AA contrast (4.5:1 body, 3:1 large text) in the built page.
- Do not copy a real company's logo, name, or slogan from a reference.
- If a decision belongs to the operator (brand name, a direction choice between finished concepts), send `horch tell orchestrator "[<role>] QUESTION: ..."` with the options and your recommendation, and continue with work that does not depend on it.

## Review checklist

Apply to each generated or supplied image before you analyze it:

- [ ] The image shows exactly one section or one screen, at the planned aspect ratio.
- [ ] Every headline, button label, and body line is readable at 100% zoom.
- [ ] Palette, type, radius, and image treatment match the direction brief.
- [ ] The composition anchor differs from the previous section.
- [ ] No banned word, placeholder brand, default AI gradient, or box-in-box layout.
- [ ] Mobile: safe areas, status bar, and home indicator are respected; the phone frame does not touch the canvas edge.
- [ ] Logo: the mark reads at 16 x 16 px and in one color.

Apply to the built page:

- [ ] Section order, count, and copy match the references.
- [ ] Every comparison check in `spec-to-code.md` passes, or the report lists the deviation.

## References

- `references/web-prompts.md`: load at step 3 for web pages and sections (direction menus, composition anchors, background modes, prompt template).
- `references/mobile-prompts.md`: load at step 3 for mobile screens and flows (platform mode, device frame, flow logic, prompt template).
- `references/brand-board-prompts.md`: load at step 3 for brand boards and logo explorations (symbol methods, board layouts, logo prompt template).
- `references/image-to-spec.md`: load at step 5 to turn any image into a measured spec.
- `references/spec-to-code.md`: load at steps 6 and 7 to build from a spec and run the screenshot comparison loop.
