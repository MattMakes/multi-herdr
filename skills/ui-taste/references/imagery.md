# Imagery, icons, logos, previews

Choosing images is a taste decision before it is a production task. For generating images, logos, or illustrations, use the `design-imagery` skill if it is available. This file covers what to show and what never to fake.

## Does the brief need images?

Decide from the subject. Act on the first row that matches.

| Brief | Images |
| --- | --- |
| Products, fashion, food, places, property, people, portfolio, photography | Real photographs are the content. Use the user's assets or labeled placeholder slots until they arrive. |
| News, blog, publication | One feature image per item; placeholder slots until provided. |
| SaaS or app marketing | A real product screenshot or a real rendered component. Abstract imagery only as a small accent. |
| Docs, API, CLI, developer reference | Usually none. Code blocks and diagrams carry the page. |
| Manifesto, essay, type-led editorial | Usually none. Type is the design. |
| Unclear | Ask one question: "Will real photos be available, or should I leave swappable slots?" |

A text-only page is right for docs and statements, and wrong for a subject people need to see. A page about a product, a place, or a person without images is unfinished, not minimal.

## Never fake it

- **No re-drawn chrome.** No hand-built browser bar with three dots, no CSS phone with a notch, no fake window title bar around a code block, no fake IDE. The user's browser and device already supply chrome. Use a real screenshot in a `<figure>` with at most a hairline border, or a typographic frame (rule, label, rule) for code.
- **No `<div>` dashboards.** A product preview built from gray rectangles and fake rows is the most recognized generated tell. Use a real screenshot, render the real component at small size, or show no preview.
- **No invented stock as final design.** A placeholder must look like a placeholder.
- **No generic illustration.** Smooth blob people, "modern flat" laptop poses, corporate doodles, and raw generator output with symmetric lighting read as generated. Simple geometric marks and diagrams that explain something (a flow, an architecture) are fine to build in SVG or CSS.

## Placeholder slots

- Mark each slot in the code: `<!-- TODO(image): hand-thrown mug on linen, 1600x1200 -->`. This is the one allowed TODO, and it must be listed in the handoff.
- Use deterministic placeholder sources for previews, for example `https://picsum.photos/seed/<brand>-<slot>/<w>/<h>`. Put the base URL in one constant so one edit swaps every slot.
- `alt` text describes the intended subject, not the placeholder.
- In production, prefer self-hosted assets. Third-party hosts add requests to other servers and a dependency on their uptime; say so in the handoff.

## Photos

- Crop, grade, or tint to the palette; an untouched stock photo is on a hundred other sites.
- Asymmetric crops over centered subjects.
- Hero media: `fetchpriority="high"`, never `loading="lazy"`. Lazy-load below the fold. Always `width` and `height`.
- Video: `autoplay muted loop playsinline`, a `poster`, captions if speech; never sound on autoplay.
- No captions, pills, or labels laid over photos as decoration; a functional caption goes below the image.
- No photo-credit lines unless a real photographer is credited for a real photo.

## Icons

- One library per project, one stroke width, sizes 16, 20, 24, or 32 px, colored with `currentColor`. Use the library the project already has. Otherwise choose for the need: Phosphor (several weights), Tabler (breadth), Heroicons (Tailwind projects), Lucide (common default), Radix icons (Radix projects).
- Do not hand-draw icon paths. If a glyph is missing, use a close match from the same library.
- No emoji as icons, bullets, or badges. Emoji render differently on every device and break the stroke voice.
- Avoid cliché metaphors (a rocket for "launch", a shield for "security") when a more precise icon exists.
- Many feature lists need no icons at all.

## Logo walls

- Only real customer logos the brief supplies or confirms. No invented logos; an empty wall is better than fake proof.
- Monochrome, height-aligned (about 24 to 40 px), with gutters 2 to 3 times the logo height.
- Logos only: no category labels under each logo.
- The wall goes below the hero, not inside it.
- Sources for real marks: the brand's media kit first, then Simple Icons (`simple-icons` package or `https://cdn.simpleicons.org/<slug>`).
- A logo works on both light and dark surfaces (single-color via `currentColor`, or two variants).

## Backgrounds and texture

- Solid tinted surfaces are the default.
- A gradient has two stops in one hue family, sits on a hero or one band (not the whole page), and does not animate.
- Grain: an SVG `feTurbulence` overlay under 10% opacity on a fixed, `pointer-events: none` pseudo-element, never on scrolling containers.
- No aurora blobs, floating orbs, particles, or starfields.
