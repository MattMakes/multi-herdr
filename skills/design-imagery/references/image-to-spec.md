# Image to spec

Use this file to turn one reference image (generated or supplied) into a spec a developer can build from without the image. Treat the image as a design specification, not a mood. The goal is faithful logic: exact text, real proportions, a consistent scale. Pixel-perfect OCR is not the goal.

## 1. Set the scale

1. Record the image size in pixels. Read it from the file metadata or an image tool, if available.
2. Choose the target viewport: 1440 px for a desktop web section, 390 px for an iOS screen, 412 px for Android, unless the task names one.
3. Compute `scale = target width / content width in the image`. For a framed phone image, measure the screen area inside the frame, not the whole canvas.
4. Multiply every measured length by `scale`. Snap spacing to a 4 px grid and type sizes to the nearest whole pixel.
5. Mark each value `measured` (you read it from pixels) or `estimated` (you judged it by eye). Never present an estimate as a measurement.

If the harness can read pixel colors or crop for inspection (an image tool, or a small script with a library the project already has), use it to measure. Do not install packages for this. A crop for your own inspection is fine; a crop is never the source for a new design image.

## 2. Inspect in this order

For each image, answer these before you write the spec:

1. What is the section and what is its job?
2. What is the visual priority: the first, second, and third thing the eye reads?
3. Which text is readable? Copy it exactly, including case and punctuation.
4. Which text is not readable? Add it to `unknowns`.
5. What grid holds the layout? Count columns, find the content width and side gutters.
6. How do sizes relate: headline to subline, subline to body, body to label?
7. What spacing repeats: between headline and subline, text and buttons, cards, and section top and bottom?
8. Which components appear, and how are primary and secondary actions told apart?
9. Which colors dominate, and which single color is the accent?
10. What is still unclear? If it matters, request a detail image before you code.

## 3. Spec template

Write one file per image, for example `design/specs/<nn>-<section>.md`. Keep the keys; delete lines that do not apply.

```yaml
source: <image path>            # and "generated" or "supplied"
section: <name>                 # job: hook | proof | educate | convert
viewport: 1440                  # target width in px
scale: 1.0                      # target px per image px

layout:
  container: 1200               # max content width, px
  gutters: 120                  # side padding at the viewport, px
  columns: 12                   # grid columns, with column gap
  column_gap: 24
  anchor: bottom-left over image  # composition anchor
  height: 860                   # section height at the viewport, px
  regions:                      # top to bottom, left to right
    - {name: text block, columns: "1-6", align: left}
    - {name: hero image, columns: "1-12", role: background}

type:
  display: {family_character: compressed grotesk, size: 96, weight: 500, line_height: 0.95, tracking: -0.02em, lines: 2}
  subline: {size: 20, weight: 400, line_height: 1.5}
  body:    {size: 16, weight: 400, line_height: 1.6}
  label:   {size: 13, weight: 500, case: upper, tracking: 0.08em}
  font_candidates: [<free or licensed fonts that match the character>]

color:                          # hex, with where each is used
  background: "#0E0F11"
  surface: "#17191C"
  text_primary: "#F2F0EA"
  text_secondary: "#A7A39A"
  accent: "#D9622B"             # CTA fill only
  border: "#2A2D31"
  overlay: "linear-gradient(0deg, rgba(14,15,17,.85) 0%, rgba(14,15,17,0) 60%)"

spacing:                        # px, on a 4 px grid
  section_padding_y: 160
  headline_to_subline: 24
  subline_to_cta: 40
  card_gap: 24
  card_padding: 32

radius: {button: 999, card: 12, image: 4}
shadow: none                    # or the visible depth logic

components:
  - name: primary CTA
    style: pill, accent fill, text_primary label
    size: {height: 52, padding_x: 28}
    text: "Start a project"
  - name: secondary CTA
    style: underlined inline link with arrow

text:                           # exact strings, in reading order
  - "Headline text"
  - "Subline text"

assets:
  - {slot: hero image, subject: <what it shows>, crop: <ratio and focal point>, treatment: <grade, duotone, overlay>}

responsive_intent: <how the section stacks at 768 px and 390 px>

unknowns:
  - item: testimonial author line too small to read
    decision: request a detail image | ask operator | use "<fallback>"

confidence: <which values are estimated>
```

## 4. Colors

- Sample flat areas, not anti-aliased edges or compressed gradients. Sample 3 points and take the median.
- Name roles, not just hues: background, surface, text primary, text secondary, accent, border.
- Check contrast now for every text and background pair you recorded. Body text needs 4.5:1 and large text (24 px, or 18.66 px bold) needs 3:1 for WCAG AA. If the reference fails, keep the hue and adjust lightness, and record the change.
- Write the image grade as a treatment (for example "warm duotone, shadows #1B1712, highlights #E8D9BF") so the code can recreate it with CSS or a processed asset.

## 5. Typography

- Measure cap height or x-height of the largest line, then derive the font size for the chosen candidate font. Record line count and wrap points of the headline.
- Do not name a font as fact unless the operator told you. Record the character and 2 or 3 candidates.
- Keep the scale ratio between levels. Do not flatten a 6:1 display-to-body contrast into a generic 3:1.

## 6. Multi-image sets

After all per-image specs exist, write `design/specs/_system.md` with the shared tokens: palette, type scale, spacing scale, radius, shadow, CTA family. Where two images disagree, choose the value that appears most often and list the deviation. Section specs then refer to the shared tokens by name.

## 7. Done when

- Every image has a spec with layout, type, color, spacing, components, text, and assets filled.
- Every readable string is copied exactly.
- Every `unknowns` item has a decision.
- The shared system file exists for a set of 2 or more images.
