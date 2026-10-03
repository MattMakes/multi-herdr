# Anti-patterns: the named tells

These are the patterns that make a UI read as generated. One is a problem; two in the same view confirm it. Each entry gives the tell, a search to find it, and the fix. Run the searches with `rg` (or `grep -rnE`) over the source; confirm visually where the search cannot decide.

A pattern is allowed when the brief or the brand system asks for it explicitly. Write the reason in one line next to it.

## Searches

Run these first. Each hit is a candidate, not a verdict; read the context.

```sh
# Gradient text
rg -n "background-clip:\s*text|bg-clip-text|-webkit-text-fill-color:\s*transparent"
# AI gradient
rg -ni "(purple|violet|indigo|fuchsia).*(blue|cyan|pink)|from-(purple|violet|indigo)"
# Pure black or white
rg -ni "#000\b|#000000|#fff\b|#ffffff|rgb\(0,\s*0,\s*0\)|bg-black\b|bg-white\b|text-black\b"
# transition: all
rg -n "transition:\s*all|transition-all"
# Uniform hover scale
rg -n "hover:scale-1|:hover[^{]*\{[^}]*scale\("
# Bounce easing on UI
rg -n "cubic-bezier\(0\.3[0-9]?,\s*1\.[3-9]|ease-?out-?back|bounce|elastic"
# Layout-property animation
rg -n "transition:[^;]*(width|height|top|left|margin|padding)"
# Focus removed
rg -n "outline:\s*(none|0)|outline-none|focus:outline-none"
# Scroll listener
rg -n "addEventListener\(['\"]scroll"
# Viewport units
rg -n "100vh|h-screen|100vw|w-screen"
# Arbitrary z-index
rg -n "z-index:\s*[0-9]{3,}|z-\[[0-9]{3,}\]"
# Default fonts
rg -ni "font-family:[^;]*(Inter|Roboto|Open Sans|Poppins|Lato|Montserrat|Arial)|font-(inter|roboto)\b"
# Italic headings (then check which hits are headings)
rg -n "<h[1-6][^>]*>[^<]*<(em|i)>|italic"
# Em and en dashes in visible text
rg -n "—|–|&mdash;|&ndash;"
# Emoji
rg -nP "[\x{1F300}-\x{1FAFF}\x{2600}-\x{27BF}]"
# Placeholder names
rg -ni "jane doe|john doe|john smith|lorem ipsum|acme|nexus|example user"
# Filler verbs
rg -ni "elevate|seamless|unleash|supercharge|empower|revolutioni|next-gen|game-chang|in today's|reimagine"
# Stub output (TODO(image) slots are allowed when the handoff lists them)
rg -n "// \.\.\.|/\* \.\.\. \*/|rest of (the )?code|implement here|similar to above|TODO"
# Eyebrow labels (then count them per section)
rg -n "uppercase[^\"']*tracking|tracking-\[0\.[0-9]+em\][^\"']*uppercase|letter-spacing:\s*0\.1"
# Lazy hero media (then check whether the hit is the first image or video)
rg -n "loading=\"lazy\""
# Fake chrome
rg -ni "traffic-light|window-dots|browser-bar|fake-(browser|phone|terminal)|notch"
```

## Critical

**Purple-to-blue gradient, aurora blobs, floating orbs.** The most recognized generated look: a purple, violet, or cyan gradient behind white centered text, or soft blurred blobs and spheres for "depth". Fix: one anchor hue on a tinted solid surface. If the surface needs texture, a two-stop gradient within one hue plus a grain overlay under 10% opacity.

**Gradient headline.** `background-clip: text` with a gradient fill. Fix: solid ink. Carry emphasis with weight, size, or the accent color.

**Three equal feature cards.** Three columns, each an icon in a tinted square above a two-line heading above three lines of body, same gap. Fix: change the shape. One large plus two small, a numbered list, a comparison table, a two-column split, or typography with no cards.

**Card inside a card.** A bordered container that holds bordered cards. Fix: keep one containment layer, usually the inner one. Group with spacing or a single rule instead.

**Centered-everything hero.** Eyebrow, headline, subline, and two buttons stacked on one center axis, often with `min-height: 100vh`. Fix: let the hero be the height of its content (about 60 to 88% of the first view), bias the text left or right, and put the visual on the other side. A centered hero is allowed for a manifesto, a launch statement, or a canvas-led brand; even then, at most two elements share the center axis.

**Invented proof.** "10x faster", "Trusted by 50,000+ teams", "99.9% uptime", fake logos, made-up testimonials. Fix: use the real number, a labeled slot (`metric to confirm`), or remove the section. A stat is never the only text in the hero.

**Fake product chrome.** A hand-built browser bar with three dots, a CSS phone frame with a notch, a "window" around a code block, or a dashboard made of `<div>` rectangles. Fix: a real screenshot in a `<figure>` with at most a hairline border, a real component rendered small, or no preview.

**Unreadable text on a flipped surface.** A section set to a dark background that still inherits dark text, or white text on a light accent. Fix: every rule that changes `background` also sets `color`; accent fills use an accent-ink token.

**Stub or truncated output.** `// ...`, `// rest of code`, a skeleton where a full component was asked for. Fix: write it all, or stop at a clean boundary and list what remains.

## Major

**Default fonts.** Inter, Roboto, Open Sans, Poppins, or the system stack as the display face, with no pairing. Fix: choose a display face for the tone and pair it with a body face (`typography.md`).

**Italic emphasis in a heading.** An upright headline with one italic word ("Built to *think*"), or an all-italic display face. Fix: roman headings; emphasis by weight, accent color, or a drawn underline. Italic stays inside body paragraphs.

**Eyebrow on every section.** A small uppercase, wide-tracked label above each heading (`FEATURES`, `01 / THE TOUR`). Fix: none by default. At most one per three sections, and numbers only when the content is a real sequence. Never put the label in a column beside the heading; stack it above.

**Split section header.** A big headline on the left and a small explainer paragraph floating on the right. Fix: stack the headline and the paragraph (paragraph max 65 ch). Use two columns only when the right column holds a real visual.

**The generic nav.** Wordmark left, four or five links, a filled button right, full width, white, hairline below, on every kind of site. Fix: shape the nav for the site: a minimal two-destination bar, a floating pill, a masthead, a side rail, or a search-first bar. The nav fits on one line at 1024 px and is 80 px tall or less.

**The link-farm footer.** Four columns (Product, Company, Resources, Legal), a social icon row, and a tiny copyright, on a site with five pages. Fix: a single line, a closing statement, or a newsletter-first footer. Keep the four-column index for real hubs and docs.

**Zigzag rows.** Image-left text-right, then text-left image-right, for three or more sections. Fix: two in a row at most; break with a full-width section, a grid, or a list.

**Glow on dark.** A colored `box-shadow` halo around a card on a dark surface. Fix: on dark surfaces, show elevation with a lighter surface, not a shadow.

**Glassmorphism as decoration.** Frosted panels everywhere, usually over a gradient. Fix: use blur only for an overlay that sits above content, with a solid fallback.

**Hover-only affordances.** A menu, delete button, or key information that appears only on hover. Fix: the same action on focus and on tap.

**Motion on everything.** Every section fades up on scroll, every card lifts, loops run forever. Fix: one orchestrated entrance; at most three motion primitives per page; nothing that loops without a function.

**`transition: all`, uniform `hover:scale-105`, bounce easing.** Fix: name the transitioned properties; one hover signal per element; ease-out for UI state (`states-and-interaction.md`).

**Mixed icon sets and emoji icons.** Two icon libraries on one page, or ✨ 🚀 ⚡ as feature icons. Fix: one library, one stroke width, `currentColor`.

**Generic copy.** "Unleash your potential", "Seamless integration", "Built for the modern team", "Where X meets Y". Fix: name the feature, the user, the number, or the place (`copy.md`).

**Lazy-loaded hero media.** `loading="lazy"` on the first image or video. Fix: `fetchpriority="high"` on the hero media; lazy-load below the fold only.

**Autoplay with sound, rotating carousels without pause.** Fix: `autoplay muted loop playsinline` with a `poster`; carousels advance manually or pause on hover and focus.

**Theme flip mid-page.** A light page with one dark section (or the reverse) that is not part of the system. Fix: lock the theme; vary sections with tints of the same family.

## Minor

**Decorative noise.** Version labels (`v2.0`, `BETA`) in a marketing hero, status dots before every item, `01 / 04` counters on images, scroll cues ("Scroll to explore"), city and weather strips, mono-caps word strips (`DESIGN · BUILD · SHIP`), photo-credit captions on stock images, pills overlaid on photos. Fix: remove. Keep a dot or label only when it carries real state.

**Straight quotes and fake ellipses.** `"x"`, `'x'`, `...`. Fix: `“x”`, `‘x’`, `…`.

**Dash pile-up.** Em dashes in headings, labels, buttons, captions, or quote attribution; more than one per paragraph in prose. Fix: a period, comma, colon, or parentheses. Ranges use an en dash (`10–20`) in prose and a hyphen in compact UI.

**Every section padded the same.** Identical top, bottom, and side padding everywhere. Fix: vary rhythm on the scale; bottom padding of the hero 1.3 times its top padding or more.

**One radius for everything, or radii that disagree.** Pill buttons on a sharp-cornered layout, or random radii. Fix: one radius rule (for example buttons full, cards 12 px, inputs 8 px) applied everywhere.

**`100vw`, `100vh`, `z-index: 9999`.** Fix: `width: 100%`; `dvh` or `svh` for full-height sections; a named z-index scale.

**Lists as hairline tables.** Ten rows, each with `border-bottom`. Fix: group into two or three clusters with one divider each, or use cards, columns, or a disclosure.

**Duplicate CTA intent.** "Get started", "Sign up free", and "Try it" on one page. Fix: one label per intent, used in the nav, the hero, and the footer.
